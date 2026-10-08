//! LAST CALL signaling Worker.
//!
//! Brokers WebRTC session setup between a room host and its players. It holds no
//! game state. Each room code maps to one `Room` Durable Object. Phase 0 serves
//! `/health` only; the SDP and ICE relay arrives in Phase 1.

use serde::Serialize;
use worker::*;

const SERVICE: &str = "last-call-signal";

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
    let res = Router::new()
        .get("/health", |_, _| {
            Response::from_json(&Health { ok: true, service: SERVICE, version: env!("CARGO_PKG_VERSION") })
        })
        .run(req, env)
        .await?;
    cors(res)
}

/// One Durable Object per room code. Phase 1 adds the WebSocket relay.
#[durable_object]
pub struct Room {
    #[allow(dead_code)]
    state: State,
}

impl DurableObject for Room {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        Response::error("room relay arrives in Phase 1", 501)
    }
}
