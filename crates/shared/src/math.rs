//! Trigonometry for the simulation, through `libm`.
//!
//! The standard library's `sin`, `cos` and `atan2` call the platform's math
//! library natively and a Rust port on wasm; their last bits can differ, and
//! then a native and a browser host drift apart. Everything the host
//! simulates uses these instead (glam and avian already use `libm` through
//! avian's `enhanced-determinism` feature).

/// `(sin x, cos x)`.
pub fn sin_cos(x: f32) -> (f32, f32) {
    (libm::sinf(x), libm::cosf(x))
}

/// The angle of the point (x, y), like `f32::atan2` called as `y.atan2(x)`.
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_library_closely() {
        for i in -50..50 {
            let x = i as f32 * 0.13;
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 1e-6 && (c - x.cos()).abs() < 1e-6);
            assert!((atan2(x, 1.5) - x.atan2(1.5)).abs() < 1e-6);
        }
    }
}
