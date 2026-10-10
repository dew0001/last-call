//! What the side games share: aim error from drink and Focus, a turn-based
//! contest of entrants paying into a pot, and splitting the pot.

use serde::{Deserialize, Serialize};

use crate::minigame::Who;
use crate::rng::Draw;

/// The most aim error (radians, each way) a sober, unfocused player has.
pub const BASE_AIM_ERROR: f32 = 0.0;

/// Aim error, radians each way: none sober, growing past drunk 40 to 0.1 at
/// 100. Focus halves it (plan sections 4.6, 5.6, 5.9).
pub fn aim_error(drunk: u8, focus: u8) -> f32 {
    let base = if drunk > 40 { 0.03 + f32::from(drunk - 40) / 60.0 * 0.07 } else { BASE_AIM_ERROR };
    base * crate::buffs::aim_sway(focus)
}

/// A uniform value in -1..=1 from two-decimal steps (deterministic everywhere).
pub fn signed_unit(d: &mut impl Draw) -> f32 {
    (d.below(201) as f32 - 100.0) / 100.0
}

/// Split `pot` among `winners` evenly; the first winners get the odd units.
pub fn split_pot(pot: i64, winners: &[Who]) -> Vec<(Who, i64)> {
    if winners.is_empty() {
        return Vec::new();
    }
    let n = winners.len() as i64;
    winners.iter().enumerate().map(|(i, w)| (*w, pot / n + i64::from((i as i64) < pot % n))).collect()
}

/// One entrant in a contest: who, and their score so far.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrant {
    pub who: Who,
    /// Attempts taken.
    pub tries: u8,
    /// Makes (goals, baskets), or letters in HORSE.
    pub score: u8,
    pub out: bool,
}

impl Entrant {
    pub fn new(who: Who) -> Self {
        Self { who, tries: 0, score: 0, out: false }
    }
}

/// Entrants with the best score.
pub fn best(entrants: &[Entrant]) -> Vec<Who> {
    let top = entrants.iter().map(|e| e.score).max().unwrap_or(0);
    entrants.iter().filter(|e| e.score == top).map(|e| e.who).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drink_spoils_aim_and_focus_steadies_it() {
        assert_eq!(aim_error(30, 0), 0.0);
        assert!(aim_error(70, 0) > aim_error(45, 0));
        assert!((aim_error(100, 0) - 0.1).abs() < 1e-6);
        assert!((aim_error(70, 30) - aim_error(70, 0) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn pots_split_without_losing_a_dollar() {
        let w = [Who::Player(1), Who::Player(2), Who::Player(3)];
        let s = split_pot(100, &w);
        assert_eq!(s.iter().map(|x| x.1).sum::<i64>(), 100);
        assert_eq!(s[0].1, 34);
        assert!(split_pot(5, &[]).is_empty());
    }

    #[test]
    fn signed_unit_spans_minus_one_to_one() {
        let mut d = crate::rng::SimRng::new(5);
        let v: Vec<f32> = (0..2000).map(|_| signed_unit(&mut d)).collect();
        assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
        assert!(v.iter().any(|x| *x < -0.9) && v.iter().any(|x| *x > 0.9));
    }
}
