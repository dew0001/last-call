//! Soccer penalties and football field goals in the parking lot (plan
//! sections 5.7 and 5.8), with real balls.
//!
//! The balls are physics props. Walk up to one and hold and let go of the
//! left button: the charge (0 to 100) sets the speed, the look sets the
//! direction (a little loft is added), and A or D held at the release
//! curves it. Over 80 the kick gets wild, and drink adds aim error. The
//! host flies the ball and watches where it crosses the goal line or the
//! uprights' plane.
//!
//! Penalties: from the spot, into a 7.32 by 2.44 m goal with posts, a bar
//! and a net. A goalie (a player, or the house's keeper) dives left,
//! center or right; the dive is a body in the way, so a save is the ball
//! bouncing off it. The house's keeper reads the kicker better each week.
//!
//! Field goals: from three tees, 10, 15 and 20 yards out. Good when the
//! ball crosses the uprights' plane over the 3.05 m bar and inside the
//! uprights (2.82 m each side). They pay 1, 2 and 4 to 1.

use serde::{Deserialize, Serialize};

use crate::math::sin_cos;
use crate::minigame::Who;
use crate::rng::Draw;
use crate::sports::{Entrant, best};

/// Goal mouth half width and height.
pub const GOAL_HALF_WIDTH: f32 = 3.66;
pub const BAR: f32 = 2.44;
/// How deep the net is behind the line.
pub const NET_DEPTH: f32 = 1.6;
/// The goal line's middle and the penalty spot in the parking lot: (x, z).
/// Kicks go toward +z.
pub const GOAL: (f32, f32) = (-7.0, 25.5);
pub const PENALTY_SPOT: (f32, f32) = (-7.0, 14.5);
/// Where the goalie stands.
pub const GOALIE_SPOT: (f32, f32) = (-7.0, 25.0);
/// The middle field goal tee (15 yards), between the penalty goal (west)
/// and the gauntlet lane (east).
pub const TEE: (f32, f32) = (0.0, 12.9);
/// The uprights, at the lot's north end straight up from the tees.
pub const UPRIGHTS: (f32, f32) = (0.0, 26.6);
/// How close a kicker or goalie must stand to their spot.
pub const SPOT_REACH: f32 = 1.5;
/// Kicks per entrant in a shootout.
pub const KICKS: u8 = 5;
/// Shootout entry fee.
pub const ENTRY: i64 = 20;

/// The balls: a soccer ball (a size 5) and a football (drawn as a prolate
/// ball, flown as a sphere).
pub const SOCCER_RADIUS: f32 = 0.11;
pub const FOOTBALL_RADIUS: f32 = 0.09;
/// A ball this close in front of a player's feet can be kicked.
pub const KICK_REACH: f32 = 1.3;
/// Loft added to the look's pitch, radians.
pub const LOFT: f32 = 0.1;
/// Kick speed range, m/s.
pub const SPEED: (f32, f32) = (8.0, 28.0);
/// Sideways pull at full curve, m/s².
pub const CURVE_ACCEL: f32 = 7.0;
pub const GRAVITY: f32 = 9.81;
/// A kicked ball is followed this long at most.
pub const MAX_FLIGHT_TICKS: u32 = 4 * crate::TICK_HZ;

/// A kick as the player made it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kick {
    /// Charge, 0 to 100.
    pub power: u8,
    /// The look when the button was let go.
    pub yaw: f32,
    pub pitch: f32,
    /// -1 (A held: curls left) to 1 (D held: curls right).
    pub curve: f32,
}

impl Kick {
    pub fn clamped(self) -> Self {
        Self {
            power: self.power.min(100),
            yaw: self.yaw,
            pitch: self.pitch.clamp(-0.3, 1.3),
            curve: self.curve.clamp(-1.0, 1.0),
        }
    }

    pub fn speed(&self) -> f32 {
        SPEED.0 + (SPEED.1 - SPEED.0) * f32::from(self.power.min(100)) / 100.0
    }

    /// The ball's velocity off the foot.
    pub fn velocity(&self) -> [f32; 3] {
        let k = self.clamped();
        let (sy, cy) = sin_cos(k.yaw);
        let (sp, cp) = sin_cos(k.pitch + LOFT);
        let v = k.speed();
        [-sy * cp * v, sp * v, -cy * cp * v]
    }

    /// The curve's sideways pull, m/s²: to the kick's right for a positive
    /// curve.
    pub fn curve_accel(&self) -> [f32; 2] {
        let (sy, cy) = sin_cos(self.yaw);
        // Forward is (-sin, -cos); right is (cos, -sin).
        let a = self.clamped().curve * CURVE_ACCEL;
        [cy * a, -sy * a]
    }
}

/// Yaw error (radians) from over-hitting: none to 80, up to 0.12 at 100.
pub fn power_error(power: u8) -> f32 {
    if power > 80 { f32::from(power - 80) / 20.0 * 0.12 } else { 0.0 }
}

/// The flight without anything in the way: positions per tick from `from`.
pub fn fly(from: [f32; 3], k: &Kick) -> Vec<[f32; 3]> {
    let dt = 1.0 / crate::TICK_HZ as f32;
    let mut p = from;
    let mut v = k.velocity();
    let c = k.curve_accel();
    let mut out = vec![p];
    for _ in 0..MAX_FLIGHT_TICKS {
        v[0] += c[0] * dt;
        v[2] += c[1] * dt;
        v[1] -= GRAVITY * dt;
        p = [p[0] + v[0] * dt, p[1] + v[1] * dt, p[2] + v[2] * dt];
        out.push(p);
        if p[1] < 0.0 {
            break;
        }
    }
    out
}

/// Where a ball moving from `a` to `b` crosses the plane z = `line` going
/// toward +z: (x, height).
pub fn crossing(a: [f32; 3], b: [f32; 3], line: f32) -> Option<(f32, f32)> {
    if !(a[2] < line && b[2] >= line) {
        return None;
    }
    let t = (line - a[2]) / (b[2] - a[2]);
    Some((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dive {
    Left,
    Center,
    Right,
}

impl Dive {
    /// Sideways, as seen by the kicker (-1 left, 1 right). The kicker faces
    /// +z, so their left is +x.
    pub fn side(self) -> f32 {
        match self {
            Dive::Left => 1.0,
            Dive::Center => 0.0,
            Dive::Right => -1.0,
        }
    }

    /// The dive toward a ball crossing at goal-relative x.
    pub fn toward(x: f32) -> Dive {
        if x > 1.0 {
            Dive::Left
        } else if x < -1.0 {
            Dive::Right
        } else {
            Dive::Center
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KickResult {
    Goal,
    Saved,
    Wide,
    OverTheBar,
}

/// A ball crossing the goal line at (x, height): in, wide or over.
pub fn at_the_line(x: f32, height: f32) -> KickResult {
    let r = SOCCER_RADIUS;
    if (x - GOAL.0).abs() < GOAL_HALF_WIDTH - r && height < BAR - r {
        KickResult::Goal
    } else if (x - GOAL.0).abs() < GOAL_HALF_WIDTH {
        KickResult::OverTheBar
    } else {
        KickResult::Wide
    }
}

/// The house keeper's dive at a kick heading for goal-relative `x`: which
/// way, and how many ticks after the kick. Week 1 guesses and goes late;
/// by week 6 he reads most kicks and goes early.
pub fn npc_dive(x: f32, week: u8, d: &mut impl Draw) -> (Dive, u32) {
    let w = u32::from(week.clamp(1, 6)) - 1;
    let dive = if d.below(100) < 30 + 10 * w {
        Dive::toward(x)
    } else {
        [Dive::Left, Dive::Center, Dive::Right][d.below(3) as usize]
    };
    let at = 22u32.saturating_sub(3 * w) + d.below(6);
    (dive, at)
}

/// The aim that sends a kick of `power` from `from` through `target`
/// (x, height, z), ignoring curve: (yaw, pitch). None if it cannot reach.
pub fn aim_at(from: [f32; 3], target: [f32; 3], power: u8) -> Option<(f32, f32)> {
    let (dx, dz) = (target[0] - from[0], target[2] - from[2]);
    let yaw = crate::math::atan2(-dx, -dz);
    let height = |pitch: f32| {
        let k = Kick { power, yaw, pitch, curve: 0.0 };
        let path = fly(from, &k);
        path.windows(2).find_map(|w| crossing(w[0], w[1], target[2])).map(|(_, h)| h)
    };
    // The low solution: raise the pitch until the ball gets there high enough.
    let (mut lo, mut hi) = (-LOFT, 0.7);
    if height(hi).is_none_or(|h| h < target[1]) {
        return None;
    }
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        match height(mid) {
            Some(h) if h >= target[1] => hi = mid,
            _ => lo = mid,
        }
    }
    Some((yaw, hi))
}

// ---------- Shootouts ----------

/// A penalty shootout: each entrant kicks five times; most goals take the pot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shootout {
    pub entrants: Vec<Entrant>,
    pub turn: usize,
    /// A player in goal (else the NPC).
    pub goalie: Option<Who>,
    pub pot: i64,
    pub started: bool,
    /// Played for fun: no fees, no pot.
    pub fun: bool,
}

impl Shootout {
    pub fn new() -> Self {
        Self { entrants: Vec::new(), turn: 0, goalie: None, pot: 0, started: false, fun: false }
    }

    /// What joining costs: nothing in a game played for fun. The first
    /// entrant decides; a fun game earns nobody anything.
    pub fn fee(&self, fun: bool, fee: i64) -> i64 {
        if self.fun || (self.entrants.is_empty() && fun) { 0 } else { fee }
    }

    pub fn join(&mut self, who: Who, fee: i64) -> bool {
        if self.started || self.goalie == Some(who) || self.entrants.iter().any(|e| e.who == who) {
            return false;
        }
        if self.entrants.is_empty() {
            self.fun = fee == 0;
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

/// The tees: (yards, x, z, odds to 1). The ball sits on the 15-yard one.
pub const TEES: [(u8, f32, f32, i64); 3] = [
    (10, UPRIGHTS.0, UPRIGHTS.1 - 9.14, 1),
    (15, UPRIGHTS.0, UPRIGHTS.1 - 13.72, 2),
    (20, UPRIGHTS.0, UPRIGHTS.1 - 18.29, 4),
];
pub const UPRIGHT_HALF: f32 = 2.82;
pub const CROSSBAR: f32 = 3.05;
/// The uprights rise this high above the bar.
pub const UPRIGHT_HEIGHT: f32 = 6.0;

/// The tee a ball at (x, z) sits on: (yards, odds).
pub fn tee_at(x: f32, z: f32) -> Option<(u8, i64)> {
    TEES.iter().find(|(_, tx, tz, _)| (x - tx).hypot(z - tz) < 1.2).map(|(y, _, _, o)| (*y, *o))
}

/// Good: over the bar and between the uprights where it crosses their plane.
pub fn field_goal_good(x: f32, height: f32) -> bool {
    height > CROSSBAR && (x - UPRIGHTS.0).abs() < UPRIGHT_HALF
}

/// What a field goal returns on a stake.
pub fn field_goal_return(yards: u8, stake: i64, good: bool) -> i64 {
    let odds = TEES.iter().find(|(y, ..)| *y == yards).map_or(1, |(.., o)| *o);
    if good { stake * (odds + 1) } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    fn spot() -> [f32; 3] {
        [PENALTY_SPOT.0, SOCCER_RADIUS, PENALTY_SPOT.1]
    }

    fn result(from: [f32; 3], k: &Kick) -> Option<KickResult> {
        let path = fly(from, k);
        path.windows(2).find_map(|w| crossing(w[0], w[1], GOAL.1)).map(|(x, h)| at_the_line(x, h))
    }

    #[test]
    fn an_aimed_kick_goes_where_it_is_aimed() {
        for (x, h) in [(2.5, 0.5), (-2.5, 1.8), (0.0, 1.0)] {
            let target = [GOAL.0 + x, h, GOAL.1];
            let (yaw, pitch) = aim_at(spot(), target, 70).expect("in range");
            let k = Kick { power: 70, yaw, pitch, curve: 0.0 };
            let path = fly(spot(), &k);
            let (cx, ch) = path.windows(2).find_map(|w| crossing(w[0], w[1], GOAL.1)).unwrap();
            assert!((cx - target[0]).abs() < 0.05 && (ch - h).abs() < 0.05, "{x},{h}: {cx},{ch}");
            assert_eq!(at_the_line(cx, ch), KickResult::Goal);
        }
    }

    #[test]
    fn wide_high_and_curled_kicks() {
        let (yaw, pitch) = aim_at(spot(), [GOAL.0 + 3.4, 0.5, GOAL.1], 70).unwrap();
        let curled = Kick { power: 70, yaw, pitch, curve: -1.0 };
        assert_eq!(result(spot(), &curled), Some(KickResult::Wide), "curls out past the post");
        let straight = Kick { power: 70, yaw, pitch, curve: 0.0 };
        assert_eq!(result(spot(), &straight), Some(KickResult::Goal));
        let (yaw, _) = aim_at(spot(), [GOAL.0, 1.0, GOAL.1], 90).unwrap();
        let skied = Kick { power: 90, yaw, pitch: 0.5, curve: 0.0 };
        assert_eq!(result(spot(), &skied), Some(KickResult::OverTheBar));
    }

    #[test]
    fn the_npc_keeper_reads_more_kicks_and_goes_earlier_by_the_week() {
        let mut d = SimRng::new(7);
        let mut reads = |week: u8| {
            let (mut right, mut ticks) = (0, 0);
            for i in 0..3_000 {
                let x = [-2.5, 0.0, 2.5][i % 3];
                let (dive, at) = npc_dive(x, week, &mut d);
                right += usize::from(dive == Dive::toward(x));
                ticks += at;
            }
            (right, ticks / 3_000)
        };
        let ((r1, t1), (r6, t6)) = (reads(1), reads(6));
        assert!(r1 < 1_700 && r6 > 2_400, "{r1} {r6}");
        assert!(t6 + 10 < t1, "{t1} {t6}");
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
    fn field_goals_need_height_and_aim_and_pay_by_distance() {
        for (yards, x, z, odds) in TEES {
            let from = [x, FOOTBALL_RADIUS, z];
            let target = [UPRIGHTS.0, CROSSBAR + 1.0, UPRIGHTS.1];
            let (yaw, pitch) = aim_at(from, target, 80).expect("in range");
            let k = Kick { power: 80, yaw, pitch, curve: 0.0 };
            let path = fly(from, &k);
            let (cx, ch) = path.windows(2).find_map(|w| crossing(w[0], w[1], UPRIGHTS.1)).unwrap();
            assert!(field_goal_good(cx, ch), "{yards}: {cx} {ch}");
            assert_eq!(field_goal_return(yards, 10, true), 10 * (odds + 1));
        }
        assert!(!field_goal_good(0.0, 2.0), "under the bar");
        assert!(!field_goal_good(3.0, 5.0), "wide");
        assert_eq!(tee_at(TEES[2].1 + 0.3, TEES[2].2), Some((20, 4)));
    }
}
