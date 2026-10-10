//! Soccer penalties and football field goals in the parking lot (plan
//! sections 5.7 and 5.8).
//!
//! A kick: aim (-1 left to 1 right), power (0 to 100) and curve (-1 to 1,
//! from the mouse at release). Power over 80 adds a random error, and drink
//! adds aim error. The host draws the errors; the rest is arithmetic.
//!
//! Penalties: the ball crosses the goal line at x = aim * 3.4 + curve * 0.6
//! (plus error), at a height that grows with power; over 2.44 m is over the
//! bar, past 3.66 m wide. A goalie dives left, center or right at a moment
//! after the kick; he saves when his side covers the ball and he moves
//! before it arrives. The NPC goalie reads the kicker better each week.
//!
//! Field goals: from 20, 30 or 40 yards. The kick must clear the 3.05 m bar
//! (enough power for the distance) and pass inside the uprights (2.82 m each
//! side). They pay 1, 2 and 4 to 1.

use serde::{Deserialize, Serialize};

use crate::minigame::Who;
use crate::rng::Draw;
use crate::sports::{Entrant, best, signed_unit};

/// Goal mouth half width and height.
pub const GOAL_HALF_WIDTH: f32 = 3.66;
pub const BAR: f32 = 2.44;
/// The goal line and the penalty spot in the parking lot: (x, z).
pub const GOAL: (f32, f32) = (0.0, 25.5);
pub const PENALTY_SPOT: (f32, f32) = (0.0, 14.5);
/// The field goal tee.
pub const TEE: (f32, f32) = (-8.0, 12.0);
/// Kicks per entrant in a shootout.
pub const KICKS: u8 = 5;
/// Shootout entry fee.
pub const ENTRY: i64 = 20;

/// A kick as the player made it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kick {
    pub aim: f32,
    pub power: u8,
    pub curve: f32,
}

impl Kick {
    pub fn clamped(self) -> Self {
        Self { aim: self.aim.clamp(-1.0, 1.0), power: self.power.min(100), curve: self.curve.clamp(-1.0, 1.0) }
    }
}

/// Lateral error (meters at the goal) from over-hitting: none to 80, up to
/// 1.5 m at 100.
pub fn power_error(power: u8) -> f32 {
    if power > 80 { f32::from(power - 80) / 20.0 * 1.5 } else { 0.0 }
}

/// Where a penalty crosses the goal line: (x, height). `aim_err` is the
/// drink error in radians; the host draws `d`.
pub fn penalty_flight(k: Kick, aim_err: f32, d: &mut impl Draw) -> (f32, f32) {
    let k = k.clamped();
    let dist = GOAL.1 - PENALTY_SPOT.1;
    let x = k.aim * 3.4 + k.curve * 0.6 + signed_unit(d) * (power_error(k.power) + aim_err * dist);
    let height = 0.2 + f32::from(k.power) / 100.0 * 2.5 + signed_unit(d) * power_error(k.power) * 0.3;
    (x, height.max(0.0))
}

/// Ticks the ball takes to reach the line (faster with power).
pub fn flight_ticks(power: u8) -> u32 {
    let speed = 14.0 + f32::from(power.min(100)) / 100.0 * 16.0;
    ((GOAL.1 - PENALTY_SPOT.1) / speed * crate::TICK_HZ as f32) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dive {
    Left,
    Center,
    Right,
}

impl Dive {
    /// Does a dive this way reach a ball crossing at `x`?
    pub fn covers(self, x: f32) -> bool {
        match self {
            Dive::Left => x < -1.0,
            Dive::Center => x.abs() <= 1.6,
            Dive::Right => x > 1.0,
        }
    }
}

/// A goalie's choice: which way, and how many ticks after the kick he goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Save {
    pub dive: Dive,
    pub at: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KickResult {
    Goal,
    Saved,
    Wide,
    OverTheBar,
}

/// Did the kick score against this dive?
pub fn resolve_penalty(x: f32, height: f32, power: u8, save: Option<Save>) -> KickResult {
    if height > BAR {
        return KickResult::OverTheBar;
    }
    if x.abs() > GOAL_HALF_WIDTH {
        return KickResult::Wide;
    }
    if let Some(s) = save {
        // Corners need an early dive; the middle only an on-time one.
        let needed = if x.abs() > 2.8 { flight_ticks(power).saturating_sub(10) } else { flight_ticks(power) };
        if s.dive.covers(x) && s.at <= needed {
            return KickResult::Saved;
        }
    }
    KickResult::Goal
}

/// The NPC goalie: right way 30% of the time in week 1, 70% by week 6
/// (else a random way); late at week 1, early by week 6.
pub fn npc_save(x: f32, power: u8, week: u8, d: &mut impl Draw) -> Save {
    let w = u32::from(week.clamp(1, 6)) - 1;
    let right = if x < -1.0 {
        Dive::Left
    } else if x > 1.0 {
        Dive::Right
    } else {
        Dive::Center
    };
    let dive =
        if d.below(100) < 30 + 8 * w { right } else { [Dive::Left, Dive::Center, Dive::Right][d.below(3) as usize] };
    let at = (flight_ticks(power) + 6).saturating_sub(3 * w + d.below(6));
    Save { dive, at }
}

/// A penalty shootout: each entrant kicks five times; most goals take the pot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shootout {
    pub entrants: Vec<Entrant>,
    pub turn: usize,
    /// A player in goal (else the NPC).
    pub goalie: Option<Who>,
    pub pot: i64,
    pub started: bool,
}

impl Shootout {
    pub fn new() -> Self {
        Self { entrants: Vec::new(), turn: 0, goalie: None, pot: 0, started: false }
    }

    pub fn join(&mut self, who: Who, fee: i64) -> bool {
        if self.started || self.goalie == Some(who) || self.entrants.iter().any(|e| e.who == who) {
            return false;
        }
        self.entrants.push(Entrant::new(who));
        self.pot += fee;
        true
    }

    pub fn start(&mut self) -> bool {
        if self.started || self.entrants.is_empty() {
            return false;
        }
        self.started = true;
        true
    }

    pub fn kicker(&self) -> Option<Who> {
        (self.started && !self.finished()).then(|| self.entrants[self.turn].who)
    }

    pub fn finished(&self) -> bool {
        self.started && self.entrants.iter().all(|e| e.tries >= KICKS)
    }

    pub fn record(&mut self, scored: bool) {
        let t = self.turn;
        self.entrants[t].tries += 1;
        if scored {
            self.entrants[t].score += 1;
        }
        let n = self.entrants.len();
        self.turn = (1..=n).map(|k| (t + k) % n).find(|i| self.entrants[*i].tries < KICKS).unwrap_or(t);
    }

    pub fn winners(&self) -> Vec<Who> {
        best(&self.entrants)
    }
}

impl Default for Shootout {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- Field goals ----------

/// Field goal distances, yards, and what each pays (to 1).
pub const FIELD_GOALS: [(u8, i64); 3] = [(20, 1), (30, 2), (40, 4)];
/// Half the gap between the uprights, meters, and the crossbar.
pub const UPRIGHT_HALF: f32 = 2.82;
pub const CROSSBAR: f32 = 3.05;

/// The least power that clears the bar from `yards`.
pub fn power_needed(yards: u8) -> u8 {
    match yards {
        0..=20 => 40,
        21..=30 => 58,
        _ => 76,
    }
}

/// A field goal attempt: (lateral miss in meters, cleared the bar).
pub fn field_goal_flight(yards: u8, k: Kick, aim_err: f32, d: &mut impl Draw) -> (f32, bool) {
    let k = k.clamped();
    let dist = f32::from(yards) * 0.9144;
    let lateral = (k.aim * 0.08 + k.curve * 0.02) * dist + signed_unit(d) * (power_error(k.power) + aim_err * dist);
    (lateral, k.power >= power_needed(yards))
}

pub fn field_goal_good(lateral: f32, cleared: bool) -> bool {
    cleared && lateral.abs() < UPRIGHT_HALF
}

/// What a field goal stake returns.
pub fn field_goal_return(yards: u8, stake: i64, good: bool) -> i64 {
    let odds = FIELD_GOALS.iter().find(|(y, _)| *y == yards).map_or(1, |(_, o)| *o);
    if good { stake * (odds + 1) } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn placed_kicks_score_and_wild_ones_miss() {
        let mut d = SimRng::new(1);
        let (x, h) = penalty_flight(Kick { aim: 0.8, power: 70, curve: 0.0 }, 0.0, &mut d);
        assert!((x - 2.72).abs() < 1e-4 && h < BAR);
        assert_eq!(resolve_penalty(x, h, 70, None), KickResult::Goal);
        let (x, h) = penalty_flight(Kick { aim: 1.0, power: 70, curve: 1.0 }, 0.0, &mut d);
        assert_eq!(resolve_penalty(x, h, 70, None), KickResult::Wide);
        let (x, h) = penalty_flight(Kick { aim: 0.0, power: 100, curve: 0.0 }, 0.0, &mut d);
        assert!(h > 2.0, "{h}");
        let over = (0..100)
            .filter(|_| {
                let (x, h) = penalty_flight(Kick { aim: 0.0, power: 100, curve: 0.0 }, 0.0, &mut d);
                resolve_penalty(x, h, 100, None) != KickResult::Goal
            })
            .count();
        assert!(over > 30, "power 100 often misses: {over}");
        let _ = (x, h);
    }

    #[test]
    fn a_goalie_saves_what_he_covers_in_time() {
        let early = Save { dive: Dive::Left, at: 0 };
        assert_eq!(resolve_penalty(-2.0, 1.0, 60, Some(early)), KickResult::Saved);
        assert_eq!(resolve_penalty(2.0, 1.0, 60, Some(early)), KickResult::Goal, "wrong way");
        let late = Save { dive: Dive::Left, at: flight_ticks(60) + 1 };
        assert_eq!(resolve_penalty(-2.0, 1.0, 60, Some(late)), KickResult::Goal, "too late");
        let on_time = Save { dive: Dive::Left, at: flight_ticks(60) - 2 };
        assert_eq!(resolve_penalty(-3.2, 1.0, 60, Some(on_time)), KickResult::Goal, "corners need an early dive");
    }

    #[test]
    fn the_npc_goalie_improves_by_the_week() {
        let mut d = SimRng::new(7);
        let saves = |week: u8, d: &mut SimRng| {
            (0..5_000)
                .filter(|i| {
                    let x = [-2.0, 0.0, 2.0][i % 3];
                    let s = npc_save(x, 60, week, d);
                    resolve_penalty(x, 1.0, 60, Some(s)) == KickResult::Saved
                })
                .count()
        };
        let (w1, w6) = (saves(1, &mut d), saves(6, &mut d));
        assert!(w1 < 1_000, "week 1 dives late: {w1}");
        assert!(w6 > 3_000, "week 6 reads the kicker: {w6}");
    }

    #[test]
    fn a_shootout_runs_five_kicks_each() {
        let (a, b) = (Who::Player(1), Who::Player(2));
        let mut s = Shootout::new();
        assert!(s.join(a, ENTRY) && s.join(b, ENTRY));
        s.start();
        let mut n = 0;
        while let Some(k) = s.kicker() {
            s.record(k == b);
            n += 1;
        }
        assert_eq!(n, 10);
        assert_eq!(s.winners(), vec![b]);
    }

    #[test]
    fn field_goals_need_power_for_distance_and_pay_by_distance() {
        let mut d = SimRng::new(3);
        for (yards, odds) in FIELD_GOALS {
            let need = power_needed(yards);
            let (l, c) = field_goal_flight(yards, Kick { aim: 0.0, power: need, curve: 0.0 }, 0.0, &mut d);
            assert!(field_goal_good(l, c), "{yards}");
            let (l, c) = field_goal_flight(yards, Kick { aim: 0.0, power: need - 1, curve: 0.0 }, 0.0, &mut d);
            assert!(!field_goal_good(l, c), "{yards} short");
            assert_eq!(field_goal_return(yards, 10, true), 10 * (odds + 1));
        }
        let (l, c) = field_goal_flight(40, Kick { aim: 1.0, power: 80, curve: 0.0 }, 0.0, &mut d);
        assert!(!field_goal_good(l, c), "hooked wide");
    }
}
