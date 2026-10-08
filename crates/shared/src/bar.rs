//! Gray-box layout of the main bar. Distances in meters; +Y is up.
//! The room spans x in [-HALF_X, HALF_X] and z in [-HALF_Z, HALF_Z].

/// Half the room's width along X.
pub const HALF_X: f32 = 10.0;
/// Half the room's depth along Z.
pub const HALF_Z: f32 = 7.0;
/// Wall height.
pub const WALL_HEIGHT: f32 = 3.2;
/// Player capsule radius.
pub const PLAYER_RADIUS: f32 = 0.35;
/// Player capsule height, feet to head.
pub const PLAYER_HEIGHT: f32 = 1.6;

/// The bar counter: an axis-aligned box players cannot walk through.
/// (center x, center z, half x, half z)
pub const COUNTER: (f32, f32, f32, f32) = (0.0, -4.0, 5.0, 0.5);
/// Counter top height.
pub const COUNTER_HEIGHT: f32 = 1.1;

/// Spawn points for up to 8 players, spread along the front of the room.
pub fn spawn_point(slot: usize) -> [f32; 3] {
    let i = (slot % 8) as f32;
    [-7.0 + i * 2.0, 0.0, 4.0]
}
