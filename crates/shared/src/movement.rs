//! Player movement. The host and the predicting client run this same code, so
//! a client's prediction matches the host unless packets are lost.
//!
//! Pure functions on plain arrays; the Bevy wrappers live in `net`.

use crate::bar::{BLOCKS, PLAYER_RADIUS};

/// Walk speed in meters per second.
pub const WALK_SPEED: f32 = 4.0;
/// Sprint speed in meters per second.
pub const SPRINT_SPEED: f32 = 6.5;

/// Input button bits.
pub mod buttons {
    pub const SPRINT: u16 = 1 << 0;
    pub const JUMP: u16 = 1 << 1;
    pub const CROUCH: u16 = 1 << 2;
    pub const INTERACT: u16 = 1 << 3;
    pub const THROW: u16 = 1 << 4;
    pub const DROP: u16 = 1 << 5;
    pub const PRIMARY: u16 = 1 << 6;
    pub const SECONDARY: u16 = 1 << 7;
    /// R: use a consumable (drink the beer in hand).
    pub const USE: u16 = 1 << 8;
}

/// Move a player for one tick. `mv` is the stick or WASD vector in the
/// player's own frame (x right, y forward), each axis in [-1, 1]. `yaw` is in
/// radians; yaw 0 faces -Z.
pub fn step(pos: [f32; 3], mv: [f32; 2], yaw: f32, buttons: u16, dt: f32) -> [f32; 3] {
    step_scaled(pos, mv, yaw, buttons, dt, 1.0)
}

/// [`step`] with a speed multiplier (the drunk meter's walk bonus).
pub fn step_scaled(pos: [f32; 3], mv: [f32; 2], yaw: f32, buttons: u16, dt: f32, speed_mult: f32) -> [f32; 3] {
    let len = (mv[0] * mv[0] + mv[1] * mv[1]).sqrt();
    if !len.is_finite() || len < 1e-4 {
        return pos;
    }
    let scale = len.min(1.0) / len;
    let (right, fwd) = (mv[0] * scale, mv[1] * scale);
    let speed = if buttons & buttons::SPRINT != 0 { SPRINT_SPEED } else { WALK_SPEED } * speed_mult;
    let (s, c) = crate::math::sin_cos(yaw);
    // Forward is -Z rotated by yaw; right is +X rotated by yaw.
    let dx = (right * c - fwd * s) * speed * dt;
    let dz = (-right * s - fwd * c) * speed * dt;
    collide([pos[0] + dx, pos[1], pos[2] + dz])
}

/// Unit vector the player faces (horizontal). Yaw 0 faces -Z.
pub fn forward(yaw: f32) -> [f32; 3] {
    let (s, c) = crate::math::sin_cos(yaw);
    [-s, 0.0, -c]
}

/// Height of the hand above the feet: above the counter top, so a held glass
/// or bottle clears it.
pub const HAND_HEIGHT: f32 = 1.3;

/// Where a player's hand is, in front of the chest. Held props sit here.
pub fn hand_point(pos: [f32; 3], yaw: f32) -> [f32; 3] {
    let f = forward(yaw);
    [pos[0] + f[0] * 0.7, pos[1] + HAND_HEIGHT, pos[2] + f[2] * 0.7]
}

/// Horizontal distance from a player to the beer tap.
pub fn distance_to_tap(pos: [f32; 3]) -> f32 {
    let (tx, tz) = crate::bar::TAP;
    ((pos[0] - tx).powi(2) + (pos[2] - tz).powi(2)).sqrt()
}

/// Keep a player inside the walls and out of every block (counter, office
/// walls, safe). Two passes settle a player pushed from one block into another.
pub fn collide(mut p: [f32; 3]) -> [f32; 3] {
    let r = PLAYER_RADIUS;
    for _ in 0..2 {
        for b in BLOCKS.iter().chain(crate::world::walls()) {
            let (ox, oz) = (b.hx + r - (p[0] - b.cx).abs(), b.hz + r - (p[2] - b.cz).abs());
            if ox > 0.0 && oz > 0.0 {
                // Push out along the axis of least overlap.
                if ox < oz {
                    p[0] += ox * (p[0] - b.cx).signum();
                } else {
                    p[2] += oz * (p[2] - b.cz).signum();
                }
            }
        }
        let (x0, x1, z0, z1) = crate::world::bounds();
        p[0] = p[0].clamp(x0 + r, x1 - r);
        p[2] = p[2].clamp(z0 + r, z1 - r);
    }
    p
}

/// Horizontal distance from a player to the office safe.
pub fn distance_to_safe(pos: [f32; 3]) -> f32 {
    let (sx, sz) = crate::bar::SAFE;
    ((pos[0] - sx).powi(2) + (pos[2] - sz).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const DT: f32 = 1.0 / 64.0;

    #[test]
    fn forward_at_yaw_zero_goes_minus_z() {
        let p = step([0.0, 0.0, 3.0], [0.0, 1.0], 0.0, 0, DT);
        assert!(p[2] < 3.0 && p[0].abs() < 1e-6);
        assert!((3.0 - p[2] - WALK_SPEED * DT).abs() < 1e-5);
    }

    #[test]
    fn diagonal_is_not_faster() {
        let a = step([0.0, 0.0, 3.0], [1.0, 1.0], 0.0, 0, DT);
        let moved = ((a[0]).powi(2) + (a[2] - 3.0).powi(2)).sqrt();
        assert!((moved - WALK_SPEED * DT).abs() < 1e-5);
    }

    #[test]
    fn the_speed_multiplier_scales_both_gaits() {
        let a = step_scaled([0.0, 0.0, 3.0], [0.0, 1.0], 0.0, 0, DT, 1.1);
        assert!((3.0 - a[2] - WALK_SPEED * 1.1 * DT).abs() < 1e-5);
    }

    #[test]
    fn sprint_is_faster() {
        let a = step([0.0, 0.0, 3.0], [0.0, 1.0], 0.0, buttons::SPRINT, DT);
        assert!((3.0 - a[2] - SPRINT_SPEED * DT).abs() < 1e-5);
    }

    #[test]
    fn walls_and_counter_block() {
        use crate::bar::COUNTER;
        // The bar's east wall stops a walk east except at the roof door.
        let p = walk([0.0, 0.0, 0.0], [15.0, 0.0], 600);
        assert!(p[0] <= 10.0 - PLAYER_RADIUS + 1e-4, "{p:?}");
        let (cx, cz, _, _) = COUNTER;
        let p = collide([cx, 0.0, cz]);
        assert!((p[2] - cz).abs() >= COUNTER.3 + PLAYER_RADIUS - 1e-5);
    }

    /// Walk a straight line in small steps, as the game does.
    fn walk(mut p: [f32; 3], to: [f32; 2], ticks: u32) -> [f32; 3] {
        for _ in 0..ticks {
            let (dx, dz) = (to[0] - p[0], to[1] - p[2]);
            if dx.hypot(dz) < 0.05 {
                break;
            }
            let yaw = (-dx).atan2(-dz);
            p = step(p, [0.0, 1.0], yaw, 0, DT);
        }
        p
    }

    #[test]
    fn every_room_is_reached_through_its_door() {
        use crate::world::{Room, room_at};
        // Bar to parking lot to pier.
        let mut p = walk([0.0, 0.0, 5.0], [0.0, 10.0], 600);
        p = walk(p, [0.0, 40.0], 2000);
        assert_eq!(room_at(p[0], p[2]), Some(Room::Pier), "{p:?}");
        // Bar to the basement, by the stairs.
        let mut p = walk([-8.0, 0.0, 3.4], [-12.0, 3.4], 600);
        p = walk(p, [-20.0, 3.4], 600);
        assert_eq!(room_at(p[0], p[2]), Some(Room::Basement), "{p:?}");
        // Bar to the roof.
        let p = walk([8.0, 0.0, 3.4], [15.0, 3.4], 600);
        assert_eq!(room_at(p[0], p[2]), Some(Room::Roof), "{p:?}");
        // Behind the counter to the kitchen; bar to the back room.
        let mut p = walk([-6.2, 0.0, -5.5], [-6.2, -10.0], 600);
        assert_eq!(room_at(p[0], p[2]), Some(Room::Kitchen), "{p:?}");
        // The back room door is behind the counter: round the counter's end.
        p = walk([-7.0, 0.0, -1.0], [-7.0, -5.5], 600);
        p = walk(p, [2.0, -5.5], 600);
        let p = walk(p, [2.0, -10.0], 600);
        assert_eq!(room_at(p[0], p[2]), Some(Room::BackRoom), "{p:?}");
    }

    #[test]
    fn the_office_is_reached_through_its_door() {
        use crate::bar::{OFFICE_DOOR, SAFE, SAFE_REACH};
        let door_x = (OFFICE_DOOR.0 + OFFICE_DOOR.1) / 2.0;
        // From the main room, through the doorway, to the safe.
        let mut p = walk([door_x, 0.0, 0.0], [door_x, -1.0], 400);
        p = walk(p, [door_x, -3.0], 400);
        p = walk(p, [SAFE.0, SAFE.1 + 1.0], 400);
        assert!(distance_to_safe(p) < SAFE_REACH, "ended at {p:?}");
        // The side wall blocks a straight walk from the counter's end.
        let q = walk([5.6, 0.0, -4.5], [8.0, -4.5], 400);
        assert!(q[0] < 6.0, "walked through the office wall to {q:?}");
    }

    proptest! {
        #[test]
        fn never_leaves_the_map(x in -9.0f32..9.0, z in -6.0f32..6.0, mx in -1.0f32..1.0, my in -1.0f32..1.0, yaw in -7.0f32..7.0) {
            let mut p = [x, 0.0, z];
            for _ in 0..400 {
                p = step(p, [mx, my], yaw, buttons::SPRINT, DT);
            }
            prop_assert!(crate::world::room_at(p[0], p[2]).is_some(), "left every room: {:?}", p);
        }

        #[test]
        fn nan_input_does_not_move(x in -9.0f32..9.0) {
            let p = step([x, 0.0, 0.0], [f32::NAN, 0.0], 0.0, 0, DT);
            prop_assert_eq!(p, [x, 0.0, 0.0]);
        }
    }
}
