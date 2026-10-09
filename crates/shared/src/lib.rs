//! Shared game logic for LAST CALL.
//!
//! Pure functions and data types used by both the host simulation and the client.
//! This crate has no Bevy render code and compiles for native and `wasm32`.
//! The `net` feature (default) adds the lightyear protocol and the Bevy-side
//! simulation shared by host and predicting clients.

pub mod audit;
pub mod bar;
pub mod beer;
pub mod blackjack;
pub mod buffs;
pub mod cards;
pub mod casino;
pub mod chaos;
pub mod customers;
pub mod drunk;
pub mod economy;
pub mod fixtures;
pub mod math;
pub mod minigame;
pub mod movement;
pub mod rng;
pub mod room;
pub mod roulette;
pub mod save;
pub mod shift;
pub mod slots;
pub mod upgrades;
pub mod world;

#[cfg(feature = "net")]
pub mod client;
#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "net")]
pub mod pipe;
#[cfg(feature = "net")]
pub mod protocol;
#[cfg(feature = "net")]
pub mod state;

use core::time::Duration;

/// Simulation tick rate of the host, in ticks per second.
pub const TICK_HZ: u32 = 64;

/// Snapshot broadcast rate from host to clients, in snapshots per second.
pub const SNAPSHOT_HZ: u32 = 20;

/// Maximum number of players in one room, host included.
pub const MAX_PLAYERS: usize = 8;

/// Length of one tick.
pub const TICK: Duration = Duration::from_micros(1_000_000 / TICK_HZ as u64);

/// Time between snapshots.
pub const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(1000 / SNAPSHOT_HZ as u64);

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
        assert_eq!(TICK.as_micros(), 15_625);
    }
}
