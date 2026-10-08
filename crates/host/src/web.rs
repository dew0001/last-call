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

use crate::HostSim;
use crate::runner::Pacer;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = globalThis, js_name = __hostOut)]
    fn host_out(peer: u32, bytes: &Uint8Array);
}

struct Peer {
    link: Entity,
    end: PipeEnd,
}

struct State {
    sim: HostSim,
    pacer: Pacer,
    peers: BTreeMap<u32, Peer>,
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

fn post_report(scope: &DedicatedWorkerGlobalScope, sim: &mut HostSim, tps: f32) {
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
#[wasm_bindgen]
pub fn host_leave(peer: u32) {
    with_state(|s| {
        if let Some(old) = s.peers.remove(&peer) {
            s.sim.disconnect_peer(old.link);
        }
    });
}

/// Start the host simulation loop inside the current dedicated Worker.
#[wasm_bindgen]
pub fn host_worker_start() {
    console_error_panic_hook::set_once();
    let scope = scope();
    STATE.with(|s| {
        *s.borrow_mut() =
            Some(State { sim: HostSim::new(), pacer: Pacer::new(now_ms(&scope)), peers: BTreeMap::new() });
    });

    let callback: TickCallback = Rc::new(RefCell::new(None));
    let callback_inner = callback.clone();
    *callback.borrow_mut() = Some(Closure::new(move || {
        let scope = self::scope();
        let wait = with_state(|s| {
            let now = now_ms(&scope);
            for _ in 0..s.pacer.due(now) {
                s.sim.tick();
                flush_outgoing(s);
            }
            if let Some(tps) = s.pacer.poll_rate(now) {
                post_report(&scope, &mut s.sim, tps);
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
