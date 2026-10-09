//! Player movement. The host and the predicting client run this same code, so
//! a client's prediction matches the host unless packets are lost.
//!
//! Pure functions on plain arrays; the Bevy wrappers live in `net`.

use crate::bar::{BLOCKS, HALF_X, HALF_Z, PLAYER_RADIUS};

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
}

/// Move a player for one tick. `mv` is the stick or WASD vector in the
/// player's own frame (x right, y forward), each axis in [-1, 1]. `yaw` is in
/// radians; yaw 0 faces -Z.
pub fn step(pos: [f32; 3], mv: [f32; 2], yaw: f32, buttons: u16, dt: f32) -> [f32; 3] {
    let len = (mv[0] * mv[0] + mv[1] * mv[1]).sqrt();
    if !len.is_finite() || len < 1e-4 {
        return pos;
    }
    let scale = len.min(1.0) / len;
    let (right, fwd) = (mv[0] * scale, mv[1] * scale);
    let speed = if buttons & buttons::SPRINT != 0 { SPRINT_SPEED } else { WALK_SPEED };
    let (s, c) = yaw.sin_cos();
    // Forward is -Z rotated by yaw; right is +X rotated by yaw.
    let dx = (right * c - fwd * s) * speed * dt;
    let dz = (-right * s - fwd * c) * speed * dt;
    collide([pos[0] + dx, pos[1], pos[2] + dz])
}

/// Unit vector the player faces (horizontal). Yaw 0 faces -Z.
pub fn forward(yaw: f32) -> [f32; 3] {
    let (s, c) = yaw.sin_cos();
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
        for b in &BLOCKS {
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
        p[0] = p[0].clamp(-HALF_X + r, HALF_X - r);
        p[2] = p[2].clamp(-HALF_Z + r, HALF_Z - r);
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
    fn sprint_is_faster() {
        let a = step([0.0, 0.0, 3.0], [0.0, 1.0], 0.0, buttons::SPRINT, DT);
        assert!((3.0 - a[2] - SPRINT_SPEED * DT).abs() < 1e-5);
    }

    #[test]
    fn walls_and_counter_block() {
        use crate::bar::COUNTER;
        assert_eq!(collide([50.0, 0.0, 0.0])[0], HALF_X - PLAYER_RADIUS);
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
        fn never_leaves_the_room(x in -9.0f32..9.0, z in -6.0f32..6.0, mx in -1.0f32..1.0, my in -1.0f32..1.0, yaw in -7.0f32..7.0) {
            let mut p = [x, 0.0, z];
            for _ in 0..200 {
                p = step(p, [mx, my], yaw, buttons::SPRINT, DT);
            }
            prop_assert!(p[0].abs() <= HALF_X - PLAYER_RADIUS + 1e-4);
            prop_assert!(p[2].abs() <= HALF_Z - PLAYER_RADIUS + 1e-4);
        }

        #[test]
        fn nan_input_does_not_move(x in -9.0f32..9.0) {
            let p = step([x, 0.0, 0.0], [f32::NAN, 0.0], 0.0, 0, DT);
            prop_assert_eq!(p, [x, 0.0, 0.0]);
        }
    }
}
