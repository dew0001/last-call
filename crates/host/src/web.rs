//! Web Worker entry point. `web/host-worker.js` imports this module, calls
//! [`host_worker_start`], and forwards peer events to [`host_connect`],
//! [`host_packet`] and [`host_leave`]. Ticks run on a `setTimeout` chain inside
//! the Worker. After each wake-up, bytes for peers go out through the JS
//! function `globalThis.__hostOut(peer, bytes)`, and a tick report is posted
//! to the page once per second.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use bevy::prelude::Entity;
use js_sys::{Object, Reflect, Uint8Array};
use shared::pipe::{PipeEnd, PipeIo};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::DedicatedWorkerGlobalScope;

use crate::runner::Pacer;
use crate::{HostConfig, HostSim};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = globalThis, js_name = __hostOut)]
    fn host_out(peer: u32, bytes: &Uint8Array);

    /// Store the newest run save (IndexedDB, `web/saves.js`).
    #[wasm_bindgen(js_namespace = globalThis, js_name = __hostSave)]
    fn host_save(json: &str);

    /// Store a JSONL chunk of the RNG audit log (IndexedDB, `web/audit.js`).
    #[wasm_bindgen(js_namespace = globalThis, js_name = __hostAudit)]
    fn host_audit(text: &str);
}

struct Peer {
    link: Entity,
    end: PipeEnd,
}

struct State {
    sim: HostSim,
    pacer: Pacer,
    peers: BTreeMap<u32, Peer>,
    /// Tick cost since the last report: (sum ms, max ms, count).
    cost: (f64, f64, u32),
}

/// A self-rescheduling `setTimeout` callback.
type TickCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

fn now_ms(scope: &DedicatedWorkerGlobalScope) -> f64 {
    scope.performance().map(|p| p.now()).unwrap_or_default()
}

fn post_report(scope: &DedicatedWorkerGlobalScope, sim: &mut HostSim, tps: f32, avg_ms: f64, max_ms: f64) {
    let tick = sim.tick_count();
    let players = js_sys::Array::new();
    for (id, pos) in sim.player_positions() {
        let row =
            js_sys::Array::of4(&JsValue::from_str(&format!("{id:016x}")), &pos.x.into(), &pos.y.into(), &pos.z.into());
        players.push(&row);
    }
    let msg = Object::new();
    let _ = Reflect::set(&msg, &"players".into(), &players);
    let _ = Reflect::set(&msg, &"t".into(), &"tick".into());
    let _ = Reflect::set(&msg, &"type".into(), &"tick".into());
    let _ = Reflect::set(&msg, &"tick".into(), &JsValue::from_f64(tick as f64));
    let _ = Reflect::set(&msg, &"tps".into(), &JsValue::from_f64(f64::from(tps)));
    let _ = Reflect::set(&msg, &"tickAvgMs".into(), &JsValue::from_f64(avg_ms));
    let _ = Reflect::set(&msg, &"tickMaxMs".into(), &JsValue::from_f64(max_ms));
    let _ = scope.post_message(&msg);
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

/// Send every queued outgoing packet to JS.
fn flush_outgoing(state: &State) {
    for (key, peer) in &state.peers {
        for bytes in peer.end.outgoing() {
            host_out(*key, &Uint8Array::from(bytes.as_ref()));
        }
    }
}

/// A player's data channel opened (or the host's own client attached).
#[wasm_bindgen]
pub fn host_connect(peer: u32) {
    with_state(|s| {
        if let Some(old) = s.peers.remove(&peer) {
            s.sim.disconnect_peer(old.link);
        }
        let (io, end) = PipeIo::new();
        let link = s.sim.connect_peer(io);
        s.peers.insert(peer, Peer { link, end });
    });
}

/// Bytes arrived from a player.
#[wasm_bindgen]
pub fn host_packet(peer: u32, bytes: &[u8]) {
    with_state(|s| {
        if let Some(p) = s.peers.get(&peer) {
            p.end.deliver(bytes);
        }
    });
}

/// A player left or its channel failed.
/// Start a chaos event (tests): `kind` is a [`shared::chaos::ChaosKind`] name, such as `Outage`.
#[wasm_bindgen]
pub fn host_force_chaos(kind: &str) {
    let Some(k) = shared::chaos::ALL.into_iter().find(|k| format!("{k:?}") == kind) else {
        web_sys::console::error_1(&format!("no chaos event {kind}").into());
        return;
    };
    with_state(|s| s.sim.force_chaos(k));
}

#[wasm_bindgen]
pub fn host_leave(peer: u32) {
    with_state(|s| {
        if let Some(old) = s.peers.remove(&peer) {
            s.sim.disconnect_peer(old.link);
        }
    });
}

/// Start the host simulation loop inside the current dedicated Worker.
///
/// `fast` divides every shift phase length (1 for the plan's timings; tests
/// use `?fast=60` for a 14-second shift). `seed` is 32 random bytes. `preset`
/// picks a test start (`lastweek`, `broke`) or is empty. `customers` is
/// `bar` to send every customer to the bar (tests of the beer tap), or empty.
/// `chaos` is `off` for no scheduled chaos events (tests force them), or empty.
/// `resume` is a saved run as JSON ([`shared::save::RunSave`]), or empty for
/// a new run; a save this build cannot read starts a new run.
#[wasm_bindgen]
pub fn host_worker_start(fast: u32, seed: &[u8], preset: &str, customers: &str, chaos: &str, resume: &str) {
    console_error_panic_hook::set_once();
    let scope = scope();
    let mut room_seed = [0u8; 32];
    for (dst, src) in room_seed.iter_mut().zip(seed) {
        *dst = *src;
    }
    let config = HostConfig {
        timings: shared::shift::Timings::PLAN.scaled_down(fast),
        seed: room_seed,
        preset: crate::economy::Preset::parse(preset),
        tastes: (customers == "bar").then_some(crate::customers::BAR_ONLY),
        manual_chaos: chaos == "off",
        resume: if resume.is_empty() {
            None
        } else {
            shared::save::RunSave::from_json(resume)
                .inspect_err(|e| web_sys::console::error_1(&format!("cannot resume: {e}; starting a new run").into()))
                .ok()
        },
    };
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            sim: HostSim::with_config(config),
            pacer: Pacer::new(now_ms(&scope)),
            peers: BTreeMap::new(),
            cost: (0.0, 0.0, 0),
        });
    });

    let callback: TickCallback = Rc::new(RefCell::new(None));
    let callback_inner = callback.clone();
    *callback.borrow_mut() = Some(Closure::new(move || {
        let scope = self::scope();
        let wait = with_state(|s| {
            let now = now_ms(&scope);
            for _ in 0..s.pacer.due(now) {
                let t0 = now_ms(&scope);
                s.sim.tick();
                flush_outgoing(s);
                let ms = now_ms(&scope) - t0;
                s.cost = (s.cost.0 + ms, s.cost.1.max(ms), s.cost.2 + 1);
            }
            if let Some(tps) = s.pacer.poll_rate(now) {
                let (sum, max, n) = std::mem::take(&mut s.cost);
                let avg = if n > 0 { sum / f64::from(n) } else { 0.0 };
                post_report(&scope, &mut s.sim, tps, avg, max);
                if let Some(save) = s.sim.take_save() {
                    host_save(&save.to_json());
                }
                let text: String = s.sim.drain_audit().iter().map(|e| e.to_line() + "\n").collect();
                if !text.is_empty() {
                    host_audit(&text);
                }
            }
            s.pacer.wait_ms(now)
        })
        .unwrap_or(1000.0);
        if let Some(cb) = callback_inner.borrow().as_ref() {
            let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                wait.floor() as i32,
            );
        }
    }));
    if let Some(cb) = callback.borrow().as_ref() {
        let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), 0);
    }
}

/// Run the determinism replay (`replay::run`) for `ticks` ticks and return
/// the state hash as 16 hex digits. Tests compare it with the native result.
/// The soak run (Phase 7): eight scripted players for `seconds` of play,
/// every tick timed. Returns the result as JSON.
#[wasm_bindgen]
pub fn host_soak(seconds: u32) -> String {
    console_error_panic_hook::set_once();
    let r = crate::replay::soak(u64::from(seconds) * u64::from(shared::TICK_HZ));
    serde_json::to_string(&r).unwrap_or_default()
}

#[wasm_bindgen]
pub fn host_replay(ticks: u32) -> String {
    console_error_panic_hook::set_once();
    format!("{:016x}", crate::replay::run(u64::from(ticks)).hash)
}

/// The replay's state hash split by part (see `replay::state_parts`), plus
/// every player's position, as text: for finding where native and wasm differ.
#[wasm_bindgen]
pub fn host_replay_parts(ticks: u32) -> String {
    console_error_panic_hook::set_once();
    let mut out = String::new();
    crate::replay::run_with(u64::from(ticks), |world| out = crate::replay::describe(world));
    out
}

/// Replay an RNG audit log (JSONL) with [`shared::audit::verify`]. Returns
/// the report as JSON, or `{"error": "..."}`.
#[wasm_bindgen]
pub fn host_audit_verify(text: &str) -> String {
    console_error_panic_hook::set_once();
    match shared::audit::verify(text.lines()) {
        Ok(report) => serde_json::to_string(&report).unwrap_or_default(),
        Err(e) => serde_json::json!({ "error": e }).to_string(),
    }
}
