//! Customer rules (plan section 4.4): wave sizes, cash, prices, patience, and
//! where a stool counts as a bar seat. Pure functions; the host drives the NPCs.

use serde::{Deserialize, Serialize};

/// Seconds between customer waves during Open (before `?fast` scaling).
pub const WAVE_EVERY_SECS: u32 = 90;
/// Seconds a customer waits for a drink before leaving.
pub const PATIENCE_SECS: u32 = 20;
/// Seconds between a customer's drinks.
pub const DRINK_EVERY_SECS: u32 = 120;
/// What a customer pays the house for a beer.
pub const BEER_PRICE: i64 = 8;
/// Tip to the server for a perfect pour.
pub const PERFECT_TIP: i64 = 2;
/// What a player pays from their pocket for their own beer.
pub const PLAYER_BEER_COST: i64 = 5;
/// Walking speed in m/s.
pub const WALK_SPEED: f32 = 1.6;
/// Body radius for the navmesh.
pub const RADIUS: f32 = 0.3;
/// Where customers come in and leave: (x, z) just inside the front wall.
pub const DOOR: (f32, f32) = (0.0, 6.3);

/// Customers per wave in `week`.
pub fn wave_size(week: u8) -> u32 {
    4 + u32::from(week) * 2
}

/// A customer's cash from a uniform draw `r` in `0..=360` (so the base is
/// 40 to 400) in `week`: `base * (1 + 0.15 * week)`.
pub fn starting_cash(r: u32, week: u8) -> i64 {
    let base = 40 + i64::from(r.min(360));
    base * (100 + 15 * i64::from(week)) / 100
}

/// What a customer is doing. Replicated so clients can draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Mood {
    /// Walking to a stool.
    #[default]
    Entering,
    /// Seated, waiting for a beer.
    Waiting,
    /// Seated with a beer.
    Drinking,
    /// Walking out.
    Leaving,
}

/// Is a stool at (x, z) with this up-vector Y component a usable bar seat?
/// It must stand upright in the strip in front of the counter.
pub fn is_bar_seat(x: f32, z: f32, up_y: f32) -> bool {
    let (cx, cz, hx, hz) = crate::bar::COUNTER;
    let front = cz + hz;
    up_y > 0.9 && (x - cx).abs() < hx - 0.2 && z > front + 0.3 && z < front + 1.6
}

/// Where a customer on a stool at (x, z) wants their beer: the counter top
/// straight in front of them. Returns (center x, center z, half x, half z).
pub fn serve_zone(stool_x: f32) -> (f32, f32, f32, f32) {
    let (_, cz, _, hz) = crate::bar::COUNTER;
    (stool_x, cz + hz * 0.5, 0.4, hz * 0.5 + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_grow_with_weeks() {
        assert_eq!(wave_size(1), 6);
        assert_eq!(wave_size(6), 16);
    }

    #[test]
    fn cash_stays_in_the_planned_range() {
        assert_eq!(starting_cash(0, 0), 40);
        assert_eq!(starting_cash(360, 0), 400);
        assert_eq!(starting_cash(9_999, 0), 400, "draws above the range clamp");
        assert_eq!(starting_cash(0, 2), 52);
        assert_eq!(starting_cash(360, 6), 760);
    }

    #[test]
    fn stools_in_front_of_the_counter_are_seats() {
        let (_, cz, _, hz) = crate::bar::COUNTER;
        let z = cz + hz + 0.8;
        assert!(is_bar_seat(0.0, z, 1.0));
        assert!(!is_bar_seat(0.0, z, 0.5), "a stool on its side");
        assert!(!is_bar_seat(0.0, 3.0, 1.0), "out in the room");
        assert!(!is_bar_seat(8.0, z, 1.0), "past the end of the counter");
    }

    #[test]
    fn the_serve_zone_is_on_the_counter_top() {
        let (cx, cz, hx, hz) = crate::bar::COUNTER;
        let (x, z, zx, zz) = serve_zone(1.5);
        assert_eq!(x, 1.5);
        assert!(x + zx <= cx + hx && z - zz >= cz - hz - 0.1 && z + zz <= cz + hz + 0.1);
    }
}
