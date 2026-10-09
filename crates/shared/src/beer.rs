//! Beer serving rules (plan section 5.4): pouring, foam, spilling, and what
//! a served glass earns. Pure functions; the host runs them every tick.

use serde::{Deserialize, Serialize};

/// Fill gained per second while E is held at the tap (full in 2.5 s).
pub const FILL_PER_SEC: f32 = 0.4;
/// Release inside this fill range for a full glass.
pub const GREEN: (f32, f32) = (0.85, 1.0);
/// Past this fill the glass overflows: the beer is wasted and a puddle forms.
pub const OVERFLOW: f32 = 1.05;
/// Look pitch (radians, negative is down) that holds the glass at a good tilt.
pub const GOOD_TILT: (f32, f32) = (-0.6, -0.2);
/// Foam gained per second at a good tilt, and at a bad one.
pub const FOAM_GOOD_PER_SEC: f32 = 0.03;
pub const FOAM_BAD_PER_SEC: f32 = 0.4;
/// Most foam a perfect pour may have.
pub const FOAM_PERFECT_MAX: f32 = 0.25;
/// Beer lost per second while carrying a glass at a sprint.
pub const SPILL_PER_SEC: f32 = 0.3;
/// A glass below this fill is refused.
pub const EMPTY_BELOW: f32 = 0.6;

/// A pour in progress.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Pour {
    pub fill: f32,
    pub foam: f32,
}

/// How a finished pour turned out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PourResult {
    /// In the green zone with little foam: the customer tips.
    Perfect,
    /// Released too early.
    Short,
    /// Full, but too much foam.
    Foamy,
    /// Past the green zone but not yet overflowing.
    Overfull,
    /// Held too long: wasted.
    Overflow,
}

impl Pour {
    /// One tick of holding E at the tap with look pitch `pitch`. Returns true
    /// when the glass overflows.
    pub fn step(&mut self, pitch: f32, dt: f32) -> bool {
        self.fill += FILL_PER_SEC * dt;
        let good = pitch.is_finite() && (GOOD_TILT.0..=GOOD_TILT.1).contains(&pitch);
        self.foam = (self.foam + if good { FOAM_GOOD_PER_SEC } else { FOAM_BAD_PER_SEC } * dt).min(1.0);
        self.fill > OVERFLOW
    }

    /// The result if E is released now.
    pub fn result(&self) -> PourResult {
        if self.fill > OVERFLOW {
            PourResult::Overflow
        } else if self.fill < GREEN.0 {
            PourResult::Short
        } else if self.fill > GREEN.1 {
            PourResult::Overfull
        } else if self.foam > FOAM_PERFECT_MAX {
            PourResult::Foamy
        } else {
            PourResult::Perfect
        }
    }
}

/// Fill after one tick of carrying. Sprinting spills; `drunk` scales it up
/// (1.0 sober).
pub fn carry(fill: f32, sprinting: bool, drunk: f32, dt: f32) -> f32 {
    if sprinting { (fill - SPILL_PER_SEC * drunk.max(1.0) * dt).max(0.0) } else { fill }
}

/// What serving a glass earns: (paid to the house, tip to the pourer).
/// A glass that was perfect and is still in the green zone tips.
pub fn serve(fill: f32, perfect: bool) -> Option<(i64, i64)> {
    if fill < EMPTY_BELOW {
        return None;
    }
    let tip = if perfect && fill >= GREEN.0 { crate::customers::PERFECT_TIP } else { 0 };
    Some((crate::customers::BEER_PRICE, tip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const DT: f32 = 1.0 / 64.0;

    fn hold(ticks: u32, pitch: f32) -> Pour {
        let mut p = Pour::default();
        for _ in 0..ticks {
            if p.step(pitch, DT) {
                break;
            }
        }
        p
    }

    #[test]
    fn a_good_tilt_released_in_the_green_is_perfect() {
        // 2.3 s at 40%/s = 92%.
        let p = hold(147, -0.4);
        assert!((0.91..0.93).contains(&p.fill), "{p:?}");
        assert_eq!(p.result(), PourResult::Perfect);
    }

    #[test]
    fn early_bad_tilt_and_late_releases() {
        assert_eq!(hold(64, -0.4).result(), PourResult::Short);
        assert_eq!(hold(147, 0.3).result(), PourResult::Foamy);
        // 2.55 s: 102%, past the green but not overflowing.
        assert_eq!(hold(163, -0.4).result(), PourResult::Overfull);
        let over = hold(400, -0.4);
        assert!(over.fill > OVERFLOW);
        assert_eq!(over.result(), PourResult::Overflow);
    }

    #[test]
    fn walking_keeps_the_beer_and_sprinting_spills_it() {
        assert_eq!(carry(0.9, false, 1.0, DT), 0.9);
        let mut f = 0.9;
        for _ in 0..64 {
            f = carry(f, true, 1.0, DT);
        }
        assert!((f - 0.6).abs() < 0.01, "a second of sprinting spills 30%: {f}");
        let drunk = carry(0.9, true, 2.0, 1.0);
        assert!(drunk < carry(0.9, true, 1.0, 1.0));
    }

    #[test]
    fn serving_pays_and_tips() {
        assert_eq!(serve(0.92, true), Some((8, 2)));
        assert_eq!(serve(0.92, false), Some((8, 0)));
        assert_eq!(serve(0.7, true), Some((8, 0)), "spilled out of the green: no tip");
        assert_eq!(serve(0.5, true), None, "too empty to sell");
    }

    proptest! {
        #[test]
        fn pours_stay_in_range(pitch in -2.0f32..2.0, ticks in 0u32..500) {
            let p = hold(ticks, pitch);
            prop_assert!(p.fill >= 0.0 && p.fill <= OVERFLOW + FILL_PER_SEC * DT);
            prop_assert!((0.0..=1.0).contains(&p.foam));
        }
    }
}
