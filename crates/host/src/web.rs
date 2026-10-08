//! Web Worker entry point. `web/host-worker.js` imports this module and calls
//! [`host_worker_start`]. Ticks run on a `setTimeout` chain inside the Worker,
//! and a tick report is posted to the page once per second.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::DedicatedWorkerGlobalScope;

use crate::HostSim;
use crate::runner::Pacer;

/// A self-rescheduling `setTimeout` callback.
type TickCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

struct State {
    sim: HostSim,
    pacer: Pacer,
}

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

fn now_ms(scope: &DedicatedWorkerGlobalScope) -> f64 {
    scope.performance().map(|p| p.now()).unwrap_or_default()
}

fn post_report(scope: &DedicatedWorkerGlobalScope, tick: u64, tps: f32) {
    let msg = Object::new();
    let _ = Reflect::set(&msg, &"type".into(), &"tick".into());
    let _ = Reflect::set(&msg, &"tick".into(), &JsValue::from_f64(tick as f64));
    let _ = Reflect::set(&msg, &"tps".into(), &JsValue::from_f64(f64::from(tps)));
    let _ = scope.post_message(&msg);
}

/// Start the host simulation loop inside the current dedicated Worker.
#[wasm_bindgen]
pub fn host_worker_start() {
    console_error_panic_hook::set_once();
    let scope = scope();
    let state = Rc::new(RefCell::new(State { sim: HostSim::new(), pacer: Pacer::new(now_ms(&scope)) }));

    let callback: TickCallback = Rc::new(RefCell::new(None));
    let callback_inner = callback.clone();
    *callback.borrow_mut() = Some(Closure::new(move || {
        let scope = self::scope();
        let wait = {
            let mut s = state.borrow_mut();
            let now = now_ms(&scope);
            for _ in 0..s.pacer.due(now) {
                s.sim.tick();
            }
            if let Some(tps) = s.pacer.poll_rate(now) {
                post_report(&scope, s.sim.tick_count(), tps);
            }
            s.pacer.wait_ms(now)
        };
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
