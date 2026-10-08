//! Browser glue: panic hook, entry point, and the `window.__lastCall` status object
//! that Playwright tests read.

use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::RenderStatus;

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        // Tests and the boot screen read this to fail fast.
        if let Some(window) = web_sys::window() {
            let _ = Reflect::set(&window, &"__lastCallError".into(), &JsValue::from_str(&format!("panic: {info}")));
        }
    }));
    crate::build_app().run();
}

pub(crate) fn publish_status(status: &RenderStatus) {
    let Some(window) = web_sys::window() else { return };
    let obj = Object::new();
    let _ = Reflect::set(&obj, &"frames".into(), &JsValue::from_f64(status.frames as f64));
    let _ = Reflect::set(&obj, &"backend".into(), &JsValue::from_str(&status.backend));
    let _ = Reflect::set(&window, &"__lastCall".into(), &obj);
}
