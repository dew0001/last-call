//! Browser glue: panic hook, entry point, and the `window.__lastCall` status object
//! that Playwright tests read.

use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::RenderStatus;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    crate::build_app().run();
}

pub(crate) fn publish_status(status: &RenderStatus) {
    let Some(window) = web_sys::window() else { return };
    let obj = Object::new();
    let _ = Reflect::set(&obj, &"frames".into(), &JsValue::from_f64(status.frames as f64));
    let _ = Reflect::set(&obj, &"backend".into(), &JsValue::from_str(&status.backend));
    let _ = Reflect::set(&window, &"__lastCall".into(), &obj);
}
