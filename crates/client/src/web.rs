//! Browser glue: entry point, packet bridge to `web/net.js`, scripted input,
//! and the `window.__lastCall` status object that the page and tests read.
//!
//! JS calls [`client_start`] with `{ online, code, uuid, name }` after the
//! module loads. Received packets go in through [`client_deliver`]; packets to
//! send come out through `globalThis.__clientOut(bytes)`.

use std::cell::RefCell;

use bevy::prelude::*;
use js_sys::{Array, Object, Reflect, Uint8Array};
use serde::Deserialize;
use shared::client::Identity;
use shared::pipe::PipeEnd;
use shared::protocol::PlayerInput;
use wasm_bindgen::prelude::*;

use crate::RenderStatus;
use crate::online::{NetBridge, NetStatus, OnlineConfig, ScriptedInput};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = globalThis, js_name = __clientOut)]
    fn client_out(bytes: &Uint8Array);
}

thread_local! {
    static INBOX: RefCell<Option<PipeEnd>> = const { RefCell::new(None) };
}

#[derive(Deserialize)]
struct StartConfig {
    online: bool,
    #[serde(default)]
    code: String,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    nodraw: bool,
}

fn parse_uuid(hex: &str) -> [u8; 16] {
    let digits: Vec<u8> = hex.bytes().filter(u8::is_ascii_hexdigit).collect();
    let mut out = [0u8; 16];
    for (i, pair) in digits.chunks(2).take(16).enumerate() {
        let s = std::str::from_utf8(pair).unwrap_or("0");
        out[i] = u8::from_str_radix(s, 16).unwrap_or(0);
    }
    out
}

fn set_error(message: String) {
    if let Some(window) = web_sys::window() {
        let _ = Reflect::set(&window, &"__lastCallError".into(), &JsValue::from_str(&message));
    }
}

/// Start the client. Never returns normally: Bevy hands the loop to the browser.
#[wasm_bindgen]
pub fn client_start(config: JsValue) -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        set_error(format!("panic: {info}"));
    }));
    let cfg: StartConfig = serde_wasm_bindgen::from_value(config)?;
    let online = cfg.online.then(|| OnlineConfig {
        identity: Identity {
            code: cfg.code,
            player_uuid: parse_uuid(&cfg.uuid),
            display_name: cfg.name,
            cosmetic_id: 0,
        },
        nodraw: cfg.nodraw,
    });
    let mut app = crate::build_app(online);
    if let Some(bridge) = app.world().get_resource::<NetBridge>() {
        INBOX.with(|i| *i.borrow_mut() = Some(bridge.0.clone()));
        app.add_systems(PreUpdate, read_scripted_input);
        app.add_systems(Last, send_outgoing);
    }
    app.add_systems(Last, publish_status);
    app.run();
    Ok(())
}

/// Bytes arrived from the host.
#[wasm_bindgen]
pub fn client_deliver(bytes: &[u8]) {
    INBOX.with(|i| {
        if let Some(end) = i.borrow().as_ref() {
            end.deliver(bytes);
        }
    });
}

fn send_outgoing(bridge: Res<NetBridge>) {
    for bytes in bridge.0.outgoing() {
        client_out(&Uint8Array::from(bytes.as_ref()));
    }
}

/// `window.__lcInput = { mx, my, yaw, pitch, buttons }` overrides keyboard and
/// mouse (tests and automation). `null` hands control back.
fn read_scripted_input(mut scripted: ResMut<ScriptedInput>) {
    let Some(window) = web_sys::window() else { return };
    let value = Reflect::get(&window, &"__lcInput".into()).unwrap_or(JsValue::NULL);
    if value.is_null() || value.is_undefined() {
        scripted.0 = None;
        return;
    }
    let num = |k: &str| Reflect::get(&value, &k.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    scripted.0 =
        Some(PlayerInput::new(Vec2::new(num("mx"), num("my")), num("yaw"), num("pitch"), num("buttons") as u16));
}

fn publish_status(status: Res<RenderStatus>, net: Option<Res<NetStatus>>) {
    let Some(window) = web_sys::window() else { return };
    let obj = Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = Reflect::set(&obj, &k.into(), &v);
    };
    set("frames", JsValue::from_f64(status.frames as f64));
    set("backend", JsValue::from_str(&status.backend));
    if let Some(net) = net {
        set("connected", JsValue::from_bool(net.connected));
        set("playerId", net.player_id.map(|id| JsValue::from_str(&format!("{id:016x}"))).unwrap_or(JsValue::NULL));
        set("playersSeen", JsValue::from_f64(net.players_seen as f64));
        set(
            "ownPos",
            net.own_pos.map(|p| Array::of3(&p.x.into(), &p.y.into(), &p.z.into()).into()).unwrap_or(JsValue::NULL),
        );
        set("refused", net.refused.as_deref().map(JsValue::from_str).unwrap_or(JsValue::NULL));
        set("propsSeen", JsValue::from_f64(net.props_seen as f64));
        set("holding", JsValue::from_bool(net.holding));
        set("propsOnFloor", JsValue::from_f64(net.props_on_floor as f64));
        let players = Array::new();
        for (id, p) in &net.players {
            let row = Array::of4(&JsValue::from_str(&format!("{id:016x}")), &p.x.into(), &p.y.into(), &p.z.into());
            players.push(&row);
        }
        set("players", players.into());
        set("rttMs", JsValue::from_f64(f64::from(net.rtt_ms)));
        set("jitterMs", JsValue::from_f64(f64::from(net.jitter_ms)));
        set("tick", JsValue::from_f64(f64::from(net.tick)));
        let ser = serde_wasm_bindgen::Serializer::json_compatible();
        set("game", serde::Serialize::serialize(&net.game, &ser).unwrap_or(JsValue::NULL));
    }
    let _ = Reflect::set(&window, &"__lastCall".into(), &obj);
}
