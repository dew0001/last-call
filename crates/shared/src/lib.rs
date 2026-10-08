//! Shared game logic for LAST CALL.
//!
//! Pure functions and data types used by both the host simulation and the client.
//! This crate has no Bevy render code and compiles for native and `wasm32`.

pub mod protocol;
pub mod rng;
pub mod room;

/// Simulation tick rate of the host, in ticks per second.
pub const TICK_HZ: u32 = 64;

/// Snapshot broadcast rate from host to clients, in snapshots per second.
pub const SNAPSHOT_HZ: u32 = 20;

/// Maximum number of players in one room, host included.
pub const MAX_PLAYERS: usize = 8;

/// Length of one tick in seconds.
pub fn tick_seconds() -> f64 {
    1.0 / f64::from(TICK_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_length_matches_rate() {
        assert!((tick_seconds() * f64::from(TICK_HZ) - 1.0).abs() < 1e-12);
    }
}
