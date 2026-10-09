//! The drunk meter (plan section 4.6). Pure rules; the host owns the meter,
//! clients draw its effects, and movement reads its walk multiplier on both
//! sides so prediction agrees with the host.
//!
//! Effects build up: a Wasted player also has every Sloppy and Courage effect
//! (OPINION; the plan lists effects per band).

use serde::{Deserialize, Serialize};

/// Meter gained per beer.
pub const PER_BEER: u8 = 20;
/// Ticks per point of decay: 1 point every 2 seconds.
pub const DECAY_TICKS: u32 = 2 * crate::TICK_HZ;
/// Seconds between stumbles while Wasted.
pub const STUMBLE_EVERY_SECS: u32 = 8;
/// Seconds a passed-out player lies on the floor.
pub const PASS_OUT_SECS: u32 = 45;
/// Ticks a stumble lasts, and how far it carries the player (m).
pub const STUMBLE_TICKS: u32 = 16;
pub const STUMBLE_DISTANCE: f32 = 0.9;

/// The bands of the meter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub enum Tier {
    /// 0 to 19.
    #[default]
    Sober,
    /// 20 to 39: max bet x1.5, slight camera sway.
    Courage,
    /// 40 to 69: throws go wide, walk +10%, blur, voice pitched down.
    Sloppy,
    /// 70 to 99: a stumble every 8 s; cannot deal; bet buttons shuffle.
    Wasted,
    /// 100: on the floor for 45 s.
    PassedOut,
}

impl Tier {
    pub fn of(level: u8) -> Self {
        match level {
            0..20 => Self::Sober,
            20..40 => Self::Courage,
            40..70 => Self::Sloppy,
            70..100 => Self::Wasted,
            _ => Self::PassedOut,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sober => "sober",
            Self::Courage => "courage",
            Self::Sloppy => "sloppy",
            Self::Wasted => "wasted",
            Self::PassedOut => "passed out",
        }
    }
}

/// The meter after one more beer.
pub fn after_beer(level: u8) -> u8 {
    level.saturating_add(PER_BEER).min(100)
}

/// Walking and sprinting speed multiplier: +10% from Sloppy up.
pub fn walk_multiplier(tier: Tier) -> f32 {
    if tier >= Tier::Sloppy { 1.1 } else { 1.0 }
}

/// Personal max bet multiplier (used by the tables in Phase 3): x1.5 from Courage up.
pub fn max_bet_multiplier(tier: Tier) -> f32 {
    if tier >= Tier::Courage { 1.5 } else { 1.0 }
}

/// Largest throw direction error in radians. "Accuracy -40%": up to 40% of a
/// right angle off the aim, from Sloppy up.
pub fn throw_spread(tier: Tier) -> f32 {
    if tier >= Tier::Sloppy { 0.4 * std::f32::consts::FRAC_PI_2 } else { 0.0 }
}

/// How much faster a carried glass spills (1.0 sober).
pub fn spill_multiplier(tier: Tier) -> f32 {
    match tier {
        Tier::Sober | Tier::Courage => 1.0,
        Tier::Sloppy => 1.5,
        Tier::Wasted | Tier::PassedOut => 2.5,
    }
}

/// Can this player deal at a table (Phase 3)?
pub fn can_deal(tier: Tier) -> bool {
    tier < Tier::Wasted
}

/// Camera sway amplitude in radians for the client, from Courage up.
pub fn camera_sway(level: u8) -> f32 {
    if Tier::of(level) >= Tier::Courage { 0.01 + f32::from(level.min(100)) * 0.0006 } else { 0.0 }
}

/// Screen blur strength 0 to 1 for the client, from Sloppy up.
pub fn blur(level: u8) -> f32 {
    if Tier::of(level) >= Tier::Sloppy { (f32::from(level.min(100)) - 30.0) / 70.0 } else { 0.0 }
}

/// Voice pitch factor others hear (1.0 normal), from Sloppy up.
pub fn voice_pitch(level: u8) -> f32 {
    if Tier::of(level) >= Tier::Sloppy { 0.8 } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_match_the_plan() {
        assert_eq!(Tier::of(0), Tier::Sober);
        assert_eq!(Tier::of(19), Tier::Sober);
        assert_eq!(Tier::of(20), Tier::Courage);
        assert_eq!(Tier::of(40), Tier::Sloppy);
        assert_eq!(Tier::of(70), Tier::Wasted);
        assert_eq!(Tier::of(99), Tier::Wasted);
        assert_eq!(Tier::of(100), Tier::PassedOut);
        assert_eq!(Tier::of(255), Tier::PassedOut);
    }

    #[test]
    fn five_beers_pass_you_out() {
        let mut level = 0;
        for _ in 0..4 {
            level = after_beer(level);
        }
        assert_eq!(Tier::of(level), Tier::Wasted);
        assert_eq!(after_beer(level), 100);
        assert_eq!(after_beer(100), 100);
    }

    #[test]
    fn effects_build_up() {
        assert_eq!(walk_multiplier(Tier::Courage), 1.0);
        assert_eq!(walk_multiplier(Tier::Sloppy), 1.1);
        assert_eq!(walk_multiplier(Tier::Wasted), 1.1);
        assert_eq!(max_bet_multiplier(Tier::Sober), 1.0);
        assert_eq!(max_bet_multiplier(Tier::Wasted), 1.5);
        assert_eq!(throw_spread(Tier::Courage), 0.0);
        assert!(throw_spread(Tier::Sloppy) > 0.6);
        assert!(can_deal(Tier::Sloppy) && !can_deal(Tier::Wasted));
        assert!(spill_multiplier(Tier::Wasted) > spill_multiplier(Tier::Sloppy));
        assert_eq!(camera_sway(10), 0.0);
        assert!(camera_sway(30) > 0.0);
        assert_eq!(blur(39), 0.0);
        assert!(blur(40) > 0.0 && blur(100) <= 1.0);
        assert_eq!(voice_pitch(39), 1.0);
        assert!(voice_pitch(40) < 1.0);
    }
}
