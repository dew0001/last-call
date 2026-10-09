//! The whole building and its lot (plan section 4.1): the main bar, the back
//! office, the kitchen, the back room, the basement and its stairwell, the
//! roof, the parking lot and the pier. Every room is open from the start
//! (user decision, see docs/DECISIONS.md).
//!
//! Rooms are axis-aligned floor rectangles at ground level (the gray box
//! keeps the basement and the roof on one level; the art pass can lift
//! them). Walls run along every room edge except where a door, or an open
//! edge between two outdoor areas, lets people through. Player collision,
//! host physics and client meshes all read [`walls`].

use std::sync::OnceLock;

use crate::bar::{Block, BlockKind, WALL_HEIGHT};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Room {
    Bar,
    Office,
    Kitchen,
    BackRoom,
    Stairwell,
    Basement,
    Roof,
    ParkingLot,
    Pier,
}

impl Room {
    pub fn label(self) -> &'static str {
        match self {
            Room::Bar => "Main bar",
            Room::Office => "Office",
            Room::Kitchen => "Kitchen",
            Room::BackRoom => "Back room",
            Room::Stairwell => "Basement stairs",
            Room::Basement => "Basement",
            Room::Roof => "Roof",
            Room::ParkingLot => "Parking lot",
            Room::Pier => "Pier",
        }
    }

    pub fn outdoors(self) -> bool {
        matches!(self, Room::Roof | Room::ParkingLot | Room::Pier)
    }
}

/// A floor rectangle: x0 < x1, z0 < z1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub room: Room,
    pub x0: f32,
    pub x1: f32,
    pub z0: f32,
    pub z1: f32,
}

const fn area(room: Room, x0: f32, x1: f32, z0: f32, z1: f32) -> Area {
    Area { room, x0, x1, z0, z1 }
}

/// Every floor. The office sits inside the bar's rectangle (its own walls
/// are blocks in [`crate::bar::BLOCKS`]), so it is listed first for lookups
/// and has no outer walls of its own.
pub const AREAS: [Area; 9] = [
    area(Room::Office, 6.0, 10.0, -7.0, -2.0),
    area(Room::Bar, -10.0, 10.0, -7.0, 7.0),
    area(Room::Kitchen, -10.0, -2.0, -15.0, -7.0),
    area(Room::BackRoom, -2.0, 6.0, -15.0, -7.0),
    area(Room::Stairwell, -14.0, -10.0, 1.6, 5.0),
    area(Room::Basement, -34.0, -14.0, -8.0, 8.0),
    area(Room::Roof, 10.0, 24.0, -5.0, 9.0),
    area(Room::ParkingLot, -14.0, 14.0, 7.0, 27.0),
    area(Room::Pier, -4.0, 4.0, 27.0, 47.0),
];

/// A gap in a wall: on the line `x = at` (spanning z from `a` to `b`) when
/// `vertical`, else on the line `z = at` (spanning x).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    pub vertical: bool,
    pub at: f32,
    pub a: f32,
    pub b: f32,
}

const fn door_x(z: f32, x0: f32, x1: f32) -> Opening {
    Opening { vertical: false, at: z, a: x0, b: x1 }
}

const fn door_z(x: f32, z0: f32, z1: f32) -> Opening {
    Opening { vertical: true, at: x, a: z0, b: z1 }
}

/// Doors and open edges.
pub const OPENINGS: [Opening; 7] = [
    // Front door: bar to parking lot (customers come in here).
    door_x(7.0, -1.2, 1.2),
    // Behind the counter, to the kitchen.
    door_x(-7.0, -7.2, -5.2),
    // Bar to the back room.
    door_x(-7.0, 1.0, 3.0),
    // West wall, past the slot machines, to the basement stairs.
    door_z(-10.0, 2.6, 4.2),
    // Stairs to the basement.
    door_z(-14.0, 2.6, 4.2),
    // East wall to the roof stairs.
    door_z(10.0, 2.6, 4.2),
    // The parking lot runs onto the pier.
    door_x(27.0, -4.0, 4.0),
];

/// The basement breaker (power outage chaos): (x, z), on the stairwell wall.
pub const BREAKER: (f32, f32) = (-12.0, 4.6);
/// The kitchen pass, where food is bought: (x, z).
pub const KITCHEN_PASS: (f32, f32) = (-6.0, -12.0);
/// The office drawer with the Zeen pouches: (x, z).
pub const ZEEN_DRAWER: (f32, f32) = (6.6, -6.4);
/// The upgrade terminal in the office: (x, z).
pub const SHOP: (f32, f32) = (6.6, -3.0);
/// How close a player must stand to use a fixture (breaker, drawer, shop, pass).
pub const FIXTURE_REACH: f32 = 1.2;

const WALL_T: f32 = 0.1;
/// Fence height around outdoor areas.
pub const FENCE_HEIGHT: f32 = 1.0;

/// Wall height for an edge of `room`.
fn height(room: Room) -> f32 {
    if room.outdoors() { FENCE_HEIGHT } else { WALL_HEIGHT }
}

/// Cut `[a, b]` along the line by every opening on that line.
fn cut(vertical: bool, at: f32, a: f32, b: f32) -> Vec<(f32, f32)> {
    let mut pieces = vec![(a, b)];
    for o in OPENINGS.iter().filter(|o| o.vertical == vertical && (o.at - at).abs() < 1e-3) {
        pieces = pieces
            .into_iter()
            .flat_map(|(p, q)| {
                let mut out = Vec::new();
                if o.a > p {
                    out.push((p, o.a.min(q)));
                }
                if o.b < q {
                    out.push((o.b.max(p), q));
                }
                out
            })
            .filter(|(p, q)| q - p > 1e-3)
            .collect();
    }
    pieces
}

/// Every wall segment, as blocks. Shared edges appear once per room; the
/// overlap is harmless.
pub fn walls() -> &'static [Block] {
    static WALLS: OnceLock<Vec<Block>> = OnceLock::new();
    WALLS.get_or_init(|| {
        let mut out = Vec::new();
        for ar in AREAS.iter().filter(|a| a.room != Room::Office) {
            let h = height(ar.room);
            for (vertical, at, a, b) in [
                (false, ar.z0, ar.x0, ar.x1),
                (false, ar.z1, ar.x0, ar.x1),
                (true, ar.x0, ar.z0, ar.z1),
                (true, ar.x1, ar.z0, ar.z1),
            ] {
                for (p, q) in cut(vertical, at, a, b) {
                    let (mid, half) = ((p + q) / 2.0, (q - p) / 2.0);
                    out.push(if vertical {
                        Block { cx: at, cz: mid, hx: WALL_T, hz: half, height: h, kind: BlockKind::Wall }
                    } else {
                        Block { cx: mid, cz: at, hx: half, hz: WALL_T, height: h, kind: BlockKind::Wall }
                    });
                }
            }
        }
        out
    })
}

/// The room at (x, z), if any.
pub fn room_at(x: f32, z: f32) -> Option<Room> {
    AREAS.iter().find(|a| x >= a.x0 && x <= a.x1 && z >= a.z0 && z <= a.z1).map(|a| a.room)
}

/// The area of a room (the first listed).
pub fn area_of(room: Room) -> Area {
    *AREAS.iter().find(|a| a.room == room).expect("every room has an area")
}

/// The whole map's bounds: (x0, x1, z0, z1).
pub fn bounds() -> (f32, f32, f32, f32) {
    AREAS.iter().fold((f32::MAX, f32::MIN, f32::MAX, f32::MIN), |(a, b, c, d), r| {
        (a.min(r.x0), b.max(r.x1), c.min(r.z0), d.max(r.z1))
    })
}

/// Is (x, z) within reach of a fixture at `at`?
pub fn near(at: (f32, f32), x: f32, z: f32) -> bool {
    (x - at.0).hypot(z - at.1) < FIXTURE_REACH
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(x: f32, z: f32) -> bool {
        walls().iter().any(|b| (x - b.cx).abs() < b.hx + 0.05 && (z - b.cz).abs() < b.hz + 0.05)
    }

    #[test]
    fn every_door_is_open_and_walls_stand_elsewhere() {
        for o in OPENINGS {
            let mid = (o.a + o.b) / 2.0;
            let (x, z) = if o.vertical { (o.at, mid) } else { (mid, o.at) };
            assert!(!blocked(x, z), "door at {x},{z} is walled");
        }
        // The bar's south wall beside the front door is solid.
        assert!(blocked(5.0, 7.0));
        assert!(blocked(-10.0, -5.0), "bar west wall");
    }

    #[test]
    fn rooms_are_found_and_the_office_wins_inside_the_bar() {
        assert_eq!(room_at(0.0, 0.0), Some(Room::Bar));
        assert_eq!(room_at(8.0, -5.0), Some(Room::Office));
        assert_eq!(room_at(-6.0, -12.0), Some(Room::Kitchen));
        assert_eq!(room_at(0.0, 40.0), Some(Room::Pier));
        assert_eq!(room_at(-20.0, 0.0), Some(Room::Basement));
        assert_eq!(room_at(30.0, 0.0), None);
        for (x, z) in [BREAKER, KITCHEN_PASS, ZEEN_DRAWER, SHOP] {
            assert!(room_at(x, z).is_some(), "fixture at {x},{z} is inside a room");
        }
        assert_eq!(room_at(BREAKER.0, BREAKER.1), Some(Room::Stairwell));
        assert_eq!(room_at(SHOP.0, SHOP.1), Some(Room::Office));
    }

    #[test]
    fn the_map_bounds_hold_every_room() {
        let (x0, x1, z0, z1) = bounds();
        assert_eq!((x0, x1, z0, z1), (-34.0, 24.0, -15.0, 47.0));
    }
}
