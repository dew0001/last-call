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

/// A solid box standing on the floor that players cannot walk through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub cx: f32,
    pub cz: f32,
    pub hx: f32,
    pub hz: f32,
    pub height: f32,
    pub kind: BlockKind,
}

/// What a block is, for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Counter,
    Wall,
    Safe,
}

const fn block(cx: f32, cz: f32, hx: f32, hz: f32, height: f32, kind: BlockKind) -> Block {
    Block { cx, cz, hx, hz, height, kind }
}

/// The back office: the back-right corner, x in [6, 10], z in [-7, -2].
/// A doorway in its front wall spans x in [OFFICE_DOOR.0, OFFICE_DOOR.1].
pub const OFFICE_DOOR: (f32, f32) = (7.6, 8.8);
const WALL_T: f32 = 0.1;

/// The office safe: (center x, center z). Players deposit at it.
pub const SAFE: (f32, f32) = (9.4, -6.4);
/// How close (horizontal distance to the safe's center) a player must stand to use it.
pub const SAFE_REACH: f32 = 1.3;

/// Everything players collide with besides the outer walls.
pub const BLOCKS: [Block; 5] = [
    block(COUNTER.0, COUNTER.1, COUNTER.2, COUNTER.3, COUNTER_HEIGHT, BlockKind::Counter),
    // Office side wall along x = 6.
    block(6.0, -4.5, WALL_T, 2.5, WALL_HEIGHT, BlockKind::Wall),
    // Office front wall along z = -2, either side of the doorway.
    block((6.0 + OFFICE_DOOR.0) / 2.0, -2.0, (OFFICE_DOOR.0 - 6.0) / 2.0, WALL_T, WALL_HEIGHT, BlockKind::Wall),
    block((OFFICE_DOOR.1 + HALF_X) / 2.0, -2.0, (HALF_X - OFFICE_DOOR.1) / 2.0, WALL_T, WALL_HEIGHT, BlockKind::Wall),
    block(SAFE.0, SAFE.1, 0.4, 0.4, 1.0, BlockKind::Safe),
];

/// Spawn points for up to 8 players, spread along the front of the room.
pub fn spawn_point(slot: usize) -> [f32; 3] {
    let i = (slot % 8) as f32;
    [-7.0 + i * 2.0, 0.0, 4.0]
}
