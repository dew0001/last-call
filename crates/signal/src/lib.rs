//! LAST CALL signaling Worker.
//!
//! Brokers WebRTC session setup between a room host and its players. It holds no
//! game state. Each room code maps to one `Room` Durable Object, which relays the
//! matchbox signaling protocol over hibernating WebSockets (see `relay`).
//!
//! Routes:
//! - `GET /health`
//! - `GET /room/CODE?role=host|player` (WebSocket upgrade, game signaling)
//! - `GET /room/CODE?role=voice&peer=PLAYER_ID` (WebSocket upgrade, voice mesh)
//! - `POST /report` (a crash report, JSON; kept in Workers KV, capped)

mod relay;
mod report;
mod voice;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use worker::*;

use relay::{Out, Peer, Role};

const SERVICE: &str = "last-call-signal";
/// How long an ended room keeps refusing joins before its storage is wiped.
const ENDED_ROOM_TTL: Duration = Duration::from_secs(120);

#[derive(Serialize)]
struct Health {
    ok: bool,
    service: &'static str,
    version: &'static str,
}

fn cors(mut res: Response) -> Result<Response> {
    let h = res.headers_mut();
    h.set("Access-Control-Allow-Origin", "*")?;
    h.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")?;
    h.set("Access-Control-Allow-Headers", "Content-Type")?;
    Ok(res)
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if req.method() == Method::Options {
        return cors(Response::empty()?);
    }
    Router::new()
        .get("/health", |_, _| {
            cors(Response::from_json(&Health { ok: true, service: SERVICE, version: env!("CARGO_PKG_VERSION") })?)
        })
        .post_async("/report", |mut req, ctx| async move {
            let body = req.bytes().await?;
            let r = match report::parse(&body) {
                Ok(r) => r,
                Err(e) => return cors(Response::error(e, 400)?),
            };
            // Without the KV binding (a local dev Worker), accept and drop.
            let Ok(kv) = ctx.env.kv("REPORTS") else { return cors(Response::empty()?.with_status(204)) };
            let key = report::key(Date::now().as_millis(), &uuid::Uuid::new_v4().to_string());
            let json = serde_json::to_string(&r).map_err(|e| Error::RustError(e.to_string()))?;
            kv.put(&key, json)?.execute().await?;
            let keys: Vec<String> = kv.list().prefix("r:".into()).execute().await?.keys.into_iter().map(|k| k.name).collect();
            for old in report::to_drop(keys, report::MAX_REPORTS) {
                kv.delete(&old).await?;
            }
            cors(Response::empty()?.with_status(204))
        })
        .get_async("/room/:code", |req, ctx| async move {
            let Some(code) = ctx.param("code").and_then(|c| shared::room::parse_code(c)) else {
                return Response::error("bad room code", 400);
            };
            let stub = ctx.env.durable_object("ROOMS")?.id_from_name(&code)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .run(req, env)
        .await
}

/// Stored on each socket so it survives hibernation.
#[derive(Serialize, Deserialize)]
struct Attachment {
    id: String,
    role: String,
}

fn peer_of(ws: &WebSocket) -> Option<Peer> {
    let a: Attachment = ws.deserialize_attachment().ok().flatten()?;
    Some(Peer { id: a.id, role: Role::parse(&a.role)? })
}

/// One Durable Object per room code.
#[durable_object]
pub struct Room {
    state: State,
}

impl Room {
    /// Voice peer ids connected now, except `skip`.
    fn voice_peers(&self, skip: Option<&str>) -> Vec<String> {
        self.state
            .get_websockets_with_tag("voice")
            .iter()
            .filter_map(|ws| ws.deserialize_attachment::<Attachment>().ok().flatten())
            .map(|a| a.id)
            .filter(|id| Some(id.as_str()) != skip)
            .collect()
    }

    async fn accept_voice(&self, url: &Url) -> Result<Response> {
        let peer = url.query_pairs().find(|(k, _)| k == "peer").map(|(_, v)| v.to_string()).unwrap_or_default();
        let pair = WebSocketPair::new()?;
        let ended = self.state.storage().get::<bool>("ended").await?.unwrap_or(false);
        let has_host = self.peers(None).iter().any(|p| p.role == Role::Host);
        let refusal = if !voice::valid_id(&peer) {
            Some((4400, "bad voice peer id"))
        } else if ended || !has_host {
            Some((relay::close::NO_HOST, "no such room"))
        } else {
            None
        };
        if let Some((code, reason)) = refusal {
            pair.server.accept()?;
            pair.server.close(Some(code), Some(reason))?;
            return Response::from_websocket(pair.client);
        }
        // A refreshed tab reconnects with the same id: drop the stale socket.
        let voice_tag = format!("v:{peer}");
        for old in self.state.get_websockets_with_tag(&voice_tag) {
            let _ = old.close(Some(1000), Some("replaced"));
        }
        let others = self.voice_peers(Some(&peer));
        if others.len() >= voice::MAX_VOICE {
            pair.server.accept()?;
            pair.server.close(Some(relay::close::ROOM_FULL), Some("voice full"))?;
            return Response::from_websocket(pair.client);
        }
        self.state.accept_websocket_with_tags(&pair.server, &["voice", voice_tag.as_str()]);
        pair.server.serialize_attachment(Attachment { id: peer.clone(), role: "voice".into() })?;
        self.dispatch_voice(voice::on_join(&others, &peer));
        Response::from_websocket(pair.client)
    }

    fn dispatch_voice(&self, outs: Vec<Out>) {
        for out in outs {
            if let Out::Send { to, text } = out {
                for ws in self.state.get_websockets_with_tag(&format!("v:{to}")) {
                    let _ = ws.send_with_str(&text);
                }
            }
        }
    }

    /// Peers connected now, except `skip`.
    fn peers(&self, skip: Option<&str>) -> Vec<Peer> {
        self.state.get_websockets().iter().filter_map(peer_of).filter(|p| Some(p.id.as_str()) != skip).collect()
    }

    fn dispatch(&self, outs: Vec<Out>) {
        for out in outs {
            match out {
                Out::Send { to, text } => {
                    for ws in self.state.get_websockets_with_tag(&to) {
                        let _ = ws.send_with_str(&text);
                    }
                }
                Out::Close { to, code, reason } => {
                    for ws in self.state.get_websockets_with_tag(&to) {
                        let _ = ws.close(Some(code), Some(reason));
                    }
                }
            }
        }
    }

    async fn leave(&self, ws: &WebSocket) -> Result<()> {
        if let Some(a) = ws.deserialize_attachment::<Attachment>().ok().flatten()
            && a.role == "voice"
        {
            // A replaced socket shares its id with the live one; only announce
            // the leave when no socket with this id remains.
            let still_here = self.state.get_websockets_with_tag(&format!("v:{}", a.id)).len() > 1;
            if !still_here {
                let peers = self.voice_peers(Some(&a.id));
                self.dispatch_voice(voice::on_leave(&peers, &a.id));
            }
            return Ok(());
        }
        let Some(left) = peer_of(ws) else { return Ok(()) };
        let peers = self.peers(Some(&left.id));
        if left.role == Role::Host {
            let storage = self.state.storage();
            storage.put("ended", true).await?;
            storage.set_alarm(ENDED_ROOM_TTL).await?;
        }
        self.dispatch(relay::on_leave(&peers, &left));
        Ok(())
    }
}

impl DurableObject for Room {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        if req.headers().get("Upgrade")?.as_deref() != Some("websocket") {
            return Response::error("expected a WebSocket upgrade", 426);
        }
        let url = req.url()?;
        if url.query_pairs().any(|(k, v)| k == "role" && v == "voice") {
            return self.accept_voice(&url).await;
        }
        let role = url.query_pairs().find(|(k, _)| k == "role").and_then(|(_, v)| Role::parse(&v));
        let Some(role) = role else { return Response::error("role must be host or player", 400) };

        let ended = self.state.storage().get::<bool>("ended").await?.unwrap_or(false);
        let peers = self.peers(None);
        let pair = WebSocketPair::new()?;

        if let Err(refusal) = relay::admit(&peers, role, ended) {
            pair.server.accept()?;
            pair.server.close(Some(refusal.code), Some(refusal.reason))?;
            return Response::from_websocket(pair.client);
        }

        let peer = Peer { id: uuid::Uuid::new_v4().to_string(), role };
        self.state.accept_websocket_with_tags(&pair.server, &[peer.id.as_str(), role.as_str()]);
        pair.server.serialize_attachment(Attachment { id: peer.id.clone(), role: role.as_str().into() })?;
        self.dispatch(relay::on_join(&peers, &peer));
        Response::from_websocket(pair.client)
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        let WebSocketIncomingMessage::String(text) = message else { return Ok(()) };
        if let Some(a) = ws.deserialize_attachment::<Attachment>().ok().flatten()
            && a.role == "voice"
        {
            let peers = self.voice_peers(Some(&a.id));
            self.dispatch_voice(voice::on_message(&peers, &a.id, &text));
            return Ok(());
        }
        let Some(from) = peer_of(&ws) else { return Ok(()) };
        let peers = self.peers(Some(&from.id));
        self.dispatch(relay::on_message(&peers, &from, &text));
        Ok(())
    }

    async fn websocket_close(&self, ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        self.leave(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, _error: Error) -> Result<()> {
        self.leave(&ws).await
    }

    async fn alarm(&self) -> Result<Response> {
        self.state.storage().delete_all().await?;
        Response::ok("room cleared")
    }
}
