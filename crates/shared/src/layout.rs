//! The furniture and set dressing, in one list that both sides read: the
//! client draws each piece's model, and movement collides with each solid
//! piece's footprint (so nobody walks through a couch, and nothing floats:
//! every piece names the height it stands at).
//!
//! Models are CC0 packs in `web/assets/`: KayKit Furniture, Restaurant and
//! City Builder Bits (`kk/...`). A piece with an empty model is a solid
//! footprint only, for something the client builds in code (the basement's
//! keg rack, the TV).
//!
//! Model frames: +Y up, the front faces +Z. A yaw of 0 puts the back toward
//! -Z (a north wall), PI toward +Z, FRAC_PI_2 toward -X (a west wall),
//! -FRAC_PI_2 toward +X.

use std::f32::consts::{FRAC_PI_2 as QUARTER, PI as HALF};
use std::sync::OnceLock;

use crate::bar::{Block, BlockKind};

/// One piece: a model at a place, and the footprint players collide with.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    /// Asset path under `web/assets/`, without `.gltf` ("" for a footprint only).
    pub model: &'static str,
    /// Where its origin stands (y is the floor or shelf it stands on).
    pub at: [f32; 3],
    pub yaw: f32,
    pub scale: [f32; 3],
    /// Half extents (x, z) of the footprint in the world, centered on `at`;
    /// [0, 0] for a piece nobody bumps into (a rug, a picture).
    pub solid: [f32; 2],
}

const fn p(model: &'static str, at: [f32; 3], yaw: f32, scale: [f32; 3], solid: [f32; 2]) -> Piece {
    Piece { model, at, yaw, scale, solid }
}

/// KayKit furniture at real size (its grid is about 4/3 of a metre).
const F: [f32; 3] = [0.75, 0.75, 0.75];
/// Kitchen modules: a 0.9 m counter, made shallower than the pack's 2 m tiles.
const KC: [f32; 3] = [0.9, 0.9, 0.42];
/// City Builder Bits are toy-sized: about 1/5 of a metre per unit.
const CITY: [f32; 3] = [4.8, 4.8, 4.8];
const NONE: [f32; 2] = [0.0, 0.0];

/// Every piece, room by room.
pub const PIECES: &[Piece] = &[
    // ---- Main bar (x -10..10, z -7..7) ----
    // A corner table on each side of the front door, with two chairs.
    p("kk/restaurant/table_round_A_small_decorated", [-7.6, 0.0, 5.0], 0.0, F, [0.55, 0.55]),
    p("kk/restaurant/chair_A", [-8.5, 0.0, 5.0], -QUARTER, F, NONE),
    p("kk/restaurant/chair_A", [-6.7, 0.0, 5.0], QUARTER, F, NONE),
    p("kk/restaurant/table_round_A_small_decorated", [6.2, 0.0, 5.2], 0.0, F, [0.55, 0.55]),
    p("kk/restaurant/chair_B", [5.3, 0.0, 5.2], -QUARTER, F, NONE),
    p("kk/restaurant/chair_B", [7.1, 0.0, 5.2], QUARTER, F, NONE),
    // Plants by the door, a standing lamp in the corner by the kitchen.
    p("kk/furniture/cactus_medium_A", [-9.4, 0.0, 6.4], 0.0, F, [0.3, 0.3]),
    p("kk/furniture/cactus_medium_B", [3.8, 0.0, 6.5], 0.6, F, [0.3, 0.3]),
    p("kk/furniture/lamp_standing", [-9.5, 0.0, -6.5], 0.0, F, [0.3, 0.3]),
    // Pictures on the side walls.
    p("kk/furniture/pictureframe_large_A", [-9.9, 1.9, -4.5], QUARTER, F, NONE),
    p("kk/furniture/pictureframe_large_B", [9.9, 1.9, -0.6], -QUARTER, F, NONE),
    p("kk/furniture/pictureframe_medium", [-9.9, 1.8, 5.2], QUARTER, F, NONE),
    // ---- Office (x 6..10, z -7..-2; door x 7.6..8.8 on z = -2) ----
    // The desk along the east wall, its chair facing it.
    p("kk/furniture/table_medium_long", [9.4, 0.0, -4.5], -QUARTER, [0.5, 0.75, 0.42], [0.42, 0.75]),
    p("kk/furniture/chair_A_wood", [8.5, 0.0, -4.5], -QUARTER, F, NONE),
    p("kk/furniture/book_set", [9.45, 0.82, -5.0], -QUARTER, F, NONE),
    p("kk/furniture/lamp_table", [9.45, 0.75, -3.95], 0.0, [0.4, 0.4, 0.4], NONE),
    // ---- Kitchen (x -10..-2, z -15..-7; door from the bar x -7.2..-5.2) ----
    // A row on the north wall: fridge, stove under its hood, sink, counter.
    p("kk/restaurant/fridge_A_decorated", [-9.0, 0.0, -14.48], 0.0, [0.9, 0.8, 0.4], [0.9, 0.45]),
    p("kk/restaurant/stove_multi", [-7.2, 0.0, -14.48], 0.0, KC, [0.9, 0.45]),
    p("kk/restaurant/extractorhood", [-7.2, 0.6, -14.9], 0.0, [0.9, 0.6, 0.42], NONE),
    p("kk/restaurant/kitchencounter_sink_backsplash", [-5.4, 0.0, -14.48], 0.0, KC, [0.9, 0.45]),
    p("kk/restaurant/kitchencounter_straight_A_decorated", [-3.6, 0.0, -14.48], 0.0, KC, [0.9, 0.45]),
    p("kk/restaurant/kitchencabinet", [-5.4, 0.2, -14.9], 0.0, [0.9, 0.6, 0.42], NONE),
    // Prep counters on the west wall.
    p("kk/restaurant/kitchencounter_straight_A_backsplash", [-9.48, 0.0, -11.6], QUARTER, KC, [0.45, 0.9]),
    p("kk/restaurant/kitchencounter_straight_B_backsplash", [-9.48, 0.0, -9.8], QUARTER, KC, [0.45, 0.9]),
    p("kk/restaurant/dishrack_plates", [-9.5, 0.9, -9.8], QUARTER, [0.5, 0.5, 0.5], NONE),
    p("kk/restaurant/pot_A_stew", [-7.2, 0.9, -14.4], 0.0, [0.45, 0.45, 0.45], NONE),
    // Paper towels on the east wall, crates of produce by the bar door.
    p("kk/restaurant/shelf_papertowel_decorated", [-2.1, 1.5, -11.0], -QUARTER, F, NONE),
    p("kk/restaurant/crate_tomatoes", [-3.0, 0.0, -7.9], 0.2, [0.36, 0.36, 0.36], [0.4, 0.4]),
    p("kk/restaurant/crate_potatoes", [-3.0, 0.29, -7.9], -0.1, [0.34, 0.34, 0.34], NONE),
    p("kk/restaurant/crate_carrots", [-3.9, 0.0, -7.8], 0.4, [0.36, 0.36, 0.36], [0.4, 0.4]),
    // ---- Back room (x -2..6, z -15..-7; door from the bar x 1..3) ----
    p("kk/furniture/rug_rectangle_stripes_A", [3.4, 0.0, -12.2], 0.0, [0.9, 1.0, 0.9], NONE),
    p("kk/furniture/couch_pillows", [3.4, 0.0, -14.36], 0.0, F, [1.15, 0.6]),
    p("kk/furniture/armchair_pillows", [0.4, 0.0, -12.6], QUARTER, F, [0.62, 0.68]),
    p("kk/furniture/table_low", [3.4, 0.0, -12.2], 0.0, [0.6, 0.8, 0.6], [0.72, 0.45]),
    p("kk/furniture/lamp_standing", [5.5, 0.0, -14.5], 0.0, F, [0.3, 0.3]),
    p("kk/furniture/cabinet_small_decorated", [-1.5, 0.0, -14.55], 0.0, F, [0.4, 0.4]),
    p("kk/furniture/pictureframe_large_B", [-1.9, 1.8, -10.6], QUARTER, F, NONE),
    // The TV on its stand by the door (built in code: art.rs).
    p("", [3.9, 0.0, -7.6], HALF, F, [0.6, 0.3]),
    // ---- Basement (x -34..-14, z -8..8): the keg rack, boiler, bench and
    // pallets are built in code (dressing.rs); their footprints. ----
    p("", [-25.0, 0.0, -7.5], 0.0, F, [5.0, 0.4]),
    p("", [-32.6, 0.0, 6.6], 0.0, F, [0.65, 0.65]),
    p("", [-18.5, 0.0, 7.55], 0.0, F, [1.25, 0.4]),
    p("", [-26.3, 0.0, 7.2], 0.0, F, [1.35, 0.55]),
    p("kk/restaurant/crate", [-15.0, 0.0, -7.4], 0.3, [0.4, 0.4, 0.4], [0.45, 0.45]),
    p("kk/restaurant/crate_lid", [-15.0, 0.32, -7.4], 0.3, [0.4, 0.4, 0.4], NONE),
    p("kk/restaurant/crate", [-16.0, 0.0, -7.5], -0.2, [0.35, 0.35, 0.35], [0.4, 0.4]),
    // The barrels in the far corner (built in code: look.rs).
    p("", [-32.95, 0.0, -6.95], 0.0, F, [0.65, 0.8]),
    // ---- Roof (x 10..24, z -5..9) ----
    p("kk/city/watertower", [22.6, 0.0, -3.6], 0.0, [4.4, 4.4, 4.4], [1.2, 1.2]),
    p("kk/city/bench", [14.5, 0.0, 8.4], HALF, [6.0, 6.0, 6.0], [1.2, 0.5]),
    // The air conditioners (built in code).
    p("", [23.3, 0.0, 6.6], 0.0, F, [0.35, 1.15]),
    // ---- Fixtures (drawn in chaos.rs): what a player stands at, not in ----
    p("", [6.45, 0.0, -6.45], 0.0, F, [0.3, 0.3]),
    p("", [6.45, 0.0, -3.0], 0.0, F, [0.3, 0.3]),
    p("", [-3.0, 0.0, 6.65], 0.0, F, [0.5, 0.25]),
    p("", [9.55, 0.0, 5.5], 0.0, F, [0.35, 0.5]),
    p("", [-6.0, 0.0, -12.0], 0.0, F, [0.68, 0.3]),
    // ---- Parking lot (x -14..14, z 7..27) and pier ----
    p("kk/city/streetlight", [-13.5, 0.0, 7.6], QUARTER, CITY, [0.15, 0.15]),
    p("kk/city/streetlight", [13.5, 0.0, 7.6], -QUARTER, CITY, [0.15, 0.15]),
    p("kk/city/streetlight", [-13.5, 0.0, 26.4], QUARTER, CITY, [0.15, 0.15]),
    p("kk/city/dumpster", [-12.6, 0.0, 11.0], QUARTER, [4.0, 4.0, 4.0], [0.75, 1.15]),
    p("kk/city/firehydrant", [13.3, 0.0, 15.0], 0.0, [3.6, 3.6, 3.6], [0.25, 0.25]),
    p("kk/city/trash_A", [-2.3, 0.0, 7.6], 0.0, [6.0, 6.0, 6.0], [0.3, 0.3]),
    p("kk/city/trash_A", [2.3, 0.0, 7.6], 1.3, [6.0, 6.0, 6.0], [0.3, 0.3]),
    p("kk/furniture/table_medium_long", [-2.4, 0.0, 30.6], QUARTER, [0.6, 0.75, 0.5], [0.5, 0.9]),
    p("kk/city/bench", [-3.3, 0.0, 30.6], -QUARTER, [5.0, 5.0, 5.0], [0.3, 1.0]),
    p("kk/city/bench", [-1.5, 0.0, 30.6], QUARTER, [5.0, 5.0, 5.0], [0.3, 1.0]),
];

/// The solid pieces as blocks, for collision.
pub fn solids() -> &'static [Block] {
    static SOLIDS: OnceLock<Vec<Block>> = OnceLock::new();
    SOLIDS.get_or_init(|| {
        PIECES
            .iter()
            .filter(|p| p.solid[0] > 0.0 && p.solid[1] > 0.0)
            .map(|p| Block {
                cx: p.at[0],
                cz: p.at[2],
                hx: p.solid[0],
                hz: p.solid[1],
                height: 1.0,
                kind: BlockKind::Wall,
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_stands_in_a_room_or_on_the_streets() {
        let (x0, x1, z0, z1) = crate::world::bounds();
        for p in PIECES {
            assert!(p.at[0] >= x0 && p.at[0] <= x1 && p.at[2] >= z0 && p.at[2] <= z1, "{} at {:?}", p.model, p.at);
            assert!(p.at[1] >= 0.0 && p.at[1] < crate::bar::WALL_HEIGHT, "{} at height {}", p.model, p.at[1]);
        }
    }

    #[test]
    fn no_solid_piece_blocks_a_door() {
        for o in crate::world::OPENINGS.iter() {
            let mid = (o.a + o.b) / 2.0;
            let (x, z) = if o.vertical { (o.at, mid) } else { (mid, o.at) };
            for b in solids() {
                let (dx, dz) = ((x - b.cx).abs() - b.hx, (z - b.cz).abs() - b.hz);
                assert!(
                    dx.max(dz) > crate::bar::PLAYER_RADIUS * 2.0,
                    "a piece at {},{} blocks the door at {x},{z}",
                    b.cx,
                    b.cz
                );
            }
        }
    }

    #[test]
    fn every_fixture_and_station_stays_reachable() {
        // A fixture is the object itself: a player pressed against its
        // footprint must still be within reach of it.
        for f in crate::fixtures::Fixture::all() {
            let (x, z) = f.position();
            for b in solids().iter().filter(|b| (x - b.cx).abs() < b.hx && (z - b.cz).abs() < b.hz) {
                let stand = b.hx.min(b.hz) + crate::bar::PLAYER_RADIUS;
                assert!(stand < crate::world::FIXTURE_REACH, "{f:?} is out of reach behind its footprint");
            }
        }
        let mut spots: Vec<(f32, f32)> = crate::fishing::SPOTS.to_vec();
        spots.extend([crate::kicks::TEE, crate::kicks::PENALTY_SPOT, crate::kicks::GOALIE_SPOT, crate::bar::TAP]);
        for t in crate::casino::TableId::all() {
            spots.extend(crate::casino::bettor_spots(t));
            spots.extend(crate::casino::role_spot(t));
        }
        for (x, z) in spots {
            for b in solids() {
                let inside = (x - b.cx).abs() < b.hx && (z - b.cz).abs() < b.hz;
                assert!(!inside, "the spot {x},{z} is inside a piece at {},{}", b.cx, b.cz);
            }
        }
    }
}
