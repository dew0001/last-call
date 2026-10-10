//! Basketball on the roof (plan section 5.6).
//!
//! A shot: charge (0 to 1000) sets the release speed, the look sets the
//! direction. The host draws the aim error (drink, Focus) and simulates the
//! ball as a point under gravity and the roof's wind (stronger each week),
//! in fixed 64 Hz steps. It is made when it drops through the rim. Clients
//! run the same flight to draw the ball.
//!
//! Contests: "3 of 5" (each entrant shoots five; most makes take the pot)
//! and HORSE (2 to 4 players; match the leader's make from the same spot or
//! take a letter; five letters and you are out).

use serde::{Deserialize, Serialize};

use crate::math::sin_cos;
use crate::minigame::Who;
use crate::sports::{Entrant, best};

/// The hoop: rim center (x, z) and height.
pub const HOOP: (f32, f32) = (22.0, 2.0);
pub const RIM_Y: f32 = 3.05;
/// Rim radius less a margin for the ball.
pub const MAKE_RADIUS: f32 = 0.18;
/// Release height above the floor.
pub const RELEASE_Y: f32 = 2.0;
/// Release speed range, m/s.
pub const SPEED: (f32, f32) = (3.0, 13.0);
pub const GRAVITY: f32 = 9.81;
/// Where shooters stand: the roof, west of the hoop.
pub const COURT: (f32, f32, f32, f32) = (11.0, 21.0, -4.0, 8.0);
/// Standing this close to a HORSE spot counts as the same spot.
pub const SPOT_REACH: f32 = 1.0;
/// Entry fee for a contest.
pub const ENTRY: i64 = 20;

/// Roof wind along +x, m/s², by week (none in week 1).
pub fn wind(week: u8) -> f32 {
    0.12 * f32::from(week.saturating_sub(1))
}

/// A shot as released (after the host adds aim error).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    pub from: [f32; 2],
    /// Charge, 0 to 1000.
    pub power: u16,
    pub yaw: f32,
    pub pitch: f32,
    pub wind: f32,
}

impl Shot {
    pub fn velocity(&self) -> [f32; 3] {
        let speed = SPEED.0 + (SPEED.1 - SPEED.0) * f32::from(self.power.min(1000)) / 1000.0;
        let (sy, cy) = sin_cos(self.yaw);
        let (sp, cp) = sin_cos(self.pitch);
        [-sy * cp * speed, sp * speed, -cy * cp * speed]
    }
}

/// The ball's flight: positions per tick, and whether it went in.
#[derive(Clone, Debug, PartialEq)]
pub struct Flight {
    pub made: bool,
    pub points: Vec<[f32; 3]>,
}

/// Ticks a flight may last at most.
pub const MAX_FLIGHT_TICKS: usize = 4 * crate::TICK_HZ as usize;

pub fn fly(shot: &Shot) -> Flight {
    let dt = 1.0 / crate::TICK_HZ as f32;
    let mut p = [shot.from[0], RELEASE_Y, shot.from[1]];
    let mut v = shot.velocity();
    let mut points = vec![p];
    for _ in 0..MAX_FLIGHT_TICKS {
        v[1] -= GRAVITY * dt;
        v[0] += shot.wind * dt;
        let q = [p[0] + v[0] * dt, p[1] + v[1] * dt, p[2] + v[2] * dt];
        points.push(q);
        if p[1] >= RIM_Y && q[1] < RIM_Y {
            // Where it crossed the rim's plane.
            let t = (p[1] - RIM_Y) / (p[1] - q[1]);
            let (x, z) = (p[0] + (q[0] - p[0]) * t, p[2] + (q[2] - p[2]) * t);
            let d = ((x - HOOP.0) * (x - HOOP.0) + (z - HOOP.1) * (z - HOOP.1)).sqrt();
            if d < MAKE_RADIUS {
                return Flight { made: true, points };
            }
        }
        if q[1] < 0.0 {
            break;
        }
        p = q;
    }
    Flight { made: false, points }
}

/// The aim a perfect shooter takes from `from`: (yaw, pitch, power). Pitch
/// is fixed at 55 degrees; the speed is solved for the distance.
pub fn perfect_aim(from: [f32; 2]) -> (f32, f32, u16) {
    let (dx, dz) = (HOOP.0 - from[0], HOOP.1 - from[1]);
    let d = (dx * dx + dz * dz).sqrt();
    let yaw = crate::math::atan2(-dx, -dz);
    let pitch = 0.96f32;
    let (sp, cp) = sin_cos(pitch);
    let dh = RIM_Y - RELEASE_Y;
    let denom = 2.0 * cp * cp * (d * sp / cp - dh);
    let v = if denom > 0.0 { (GRAVITY * d * d / denom).sqrt() } else { SPEED.1 };
    let power = ((v - SPEED.0) / (SPEED.1 - SPEED.0) * 1000.0).round().clamp(0.0, 1000.0);
    (yaw, pitch, power as u16)
}

// ---------- Contests ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    ThreeOfFive,
    Horse,
}

/// A contest on the roof.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contest {
    pub mode: Mode,
    pub entrants: Vec<Entrant>,
    /// Whose shot it is.
    pub turn: usize,
    /// HORSE: the leader's spot to match, and who set it.
    pub spot: Option<([f32; 2], usize)>,
    pub pot: i64,
    pub started: bool,
}

/// Letters in HORSE.
pub const HORSE: &str = "HORSE";

impl Contest {
    pub fn new(mode: Mode) -> Self {
        Self { mode, entrants: Vec::new(), turn: 0, spot: None, pot: 0, started: false }
    }

    pub fn max_entrants(&self) -> usize {
        match self.mode {
            Mode::ThreeOfFive => crate::MAX_PLAYERS,
            Mode::Horse => 4,
        }
    }

    pub fn min_entrants(&self) -> usize {
        match self.mode {
            Mode::ThreeOfFive => 1,
            Mode::Horse => 2,
        }
    }

    /// Add an entrant (and their fee) before the start.
    pub fn join(&mut self, who: Who, fee: i64) -> bool {
        if self.started || self.entrants.len() >= self.max_entrants() || self.entrants.iter().any(|e| e.who == who) {
            return false;
        }
        self.entrants.push(Entrant::new(who));
        self.pot += fee;
        true
    }

    pub fn start(&mut self) -> bool {
        if self.started || self.entrants.len() < self.min_entrants() {
            return false;
        }
        self.started = true;
        true
    }

    pub fn shooter(&self) -> Option<Who> {
        (self.started && !self.finished()).then(|| self.entrants[self.turn].who)
    }

    /// May `who` shoot from `from` now?
    pub fn may_shoot(&self, who: Who, from: [f32; 2]) -> bool {
        if self.shooter() != Some(who) {
            return false;
        }
        match self.spot {
            Some((s, leader)) if leader != self.turn => {
                ((s[0] - from[0]).powi(2) + (s[1] - from[1]).powi(2)).sqrt() <= SPOT_REACH
            }
            _ => true,
        }
    }

    fn alive(&self) -> usize {
        self.entrants.iter().filter(|e| !e.out).count()
    }

    pub fn finished(&self) -> bool {
        self.started
            && match self.mode {
                Mode::ThreeOfFive => self.entrants.iter().all(|e| e.tries >= 5),
                Mode::Horse => self.alive() <= 1,
            }
    }

    fn next_alive(&self, from: usize) -> usize {
        let n = self.entrants.len();
        (1..=n).map(|k| (from + k) % n).find(|i| !self.entrants[*i].out).unwrap_or(from)
    }

    /// Record the current shooter's shot from `from`.
    pub fn record(&mut self, from: [f32; 2], made: bool) {
        let t = self.turn;
        self.entrants[t].tries += 1;
        match self.mode {
            Mode::ThreeOfFive => {
                if made {
                    self.entrants[t].score += 1;
                }
                self.turn = (t + 1) % self.entrants.len();
                if self.entrants[self.turn].tries >= 5 {
                    self.turn = self.entrants.iter().position(|e| e.tries < 5).unwrap_or(0);
                }
            }
            Mode::Horse => match self.spot {
                // The leader sets a spot with a make; a miss passes the lead.
                None => {
                    if made {
                        self.spot = Some((from, t));
                    }
                    self.turn = self.next_alive(t);
                }
                Some((_, leader)) => {
                    if !made {
                        self.entrants[t].score += 1;
                        if usize::from(self.entrants[t].score) >= HORSE.len() {
                            self.entrants[t].out = true;
                        }
                    }
                    let next = self.next_alive(t);
                    if next == leader || self.entrants[leader].out {
                        // Everyone matched: the next player leads.
                        self.spot = None;
                        self.turn = self.next_alive(leader);
                    } else {
                        self.turn = next;
                    }
                }
            },
        }
    }

    /// Who takes the pot.
    pub fn winners(&self) -> Vec<Who> {
        match self.mode {
            Mode::ThreeOfFive => best(&self.entrants),
            Mode::Horse => self.entrants.iter().filter(|e| !e.out).map(|e| e.who).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot_from(from: [f32; 2], wind: f32) -> Shot {
        let (yaw, pitch, power) = perfect_aim(from);
        Shot { from, power, yaw, pitch, wind }
    }

    #[test]
    fn a_perfect_shot_goes_in_from_anywhere_on_the_court() {
        for x in [12.0, 15.0, 18.0, 20.0] {
            for z in [-3.0, 0.0, 2.0, 5.0, 7.0] {
                let f = fly(&shot_from([x, z], 0.0));
                assert!(f.made, "missed from {x},{z}");
            }
        }
    }

    #[test]
    fn wind_and_aim_error_cost_shots() {
        let from = [12.0, 2.0];
        assert!(!fly(&shot_from(from, wind(6))).made, "week 6 wind blows a windless aim off");
        let mut s = shot_from(from, 0.0);
        s.yaw += 0.08;
        assert!(!fly(&s).made);
        let mut s = shot_from(from, 0.0);
        s.power = s.power.saturating_sub(150);
        assert!(!fly(&s).made, "short");
    }

    #[test]
    fn flights_end_on_the_floor() {
        let s = Shot { from: [12.0, 2.0], power: 0, yaw: 0.0, pitch: 0.0, wind: 0.0 };
        let f = fly(&s);
        assert!(!f.made);
        assert!(f.points.len() < MAX_FLIGHT_TICKS);
    }

    #[test]
    fn three_of_five_gives_everyone_five_and_pays_the_best() {
        let (a, b) = (Who::Player(1), Who::Player(2));
        let mut c = Contest::new(Mode::ThreeOfFive);
        assert!(c.join(a, ENTRY) && c.join(b, ENTRY) && !c.join(a, ENTRY));
        assert!(c.start());
        assert!(!c.join(Who::Player(3), ENTRY), "no joining after the start");
        let mut shots = 0;
        while let Some(who) = c.shooter() {
            c.record([12.0, 0.0], who == a);
            shots += 1;
        }
        assert_eq!(shots, 10);
        assert_eq!(c.winners(), vec![a]);
        assert_eq!(c.pot, 40);
    }

    #[test]
    fn horse_letters_until_one_is_left() {
        let (a, b, c3) = (Who::Player(1), Who::Player(2), Who::Player(3));
        let mut c = Contest::new(Mode::Horse);
        assert!(!c.start(), "HORSE needs two");
        c.join(a, ENTRY);
        c.join(b, ENTRY);
        c.join(c3, ENTRY);
        c.start();
        // a makes from (12, 0): b and c must match from there.
        c.record([12.0, 0.0], true);
        assert_eq!(c.shooter(), Some(b));
        assert!(!c.may_shoot(b, [15.0, 0.0]), "from the leader's spot");
        assert!(c.may_shoot(b, [12.5, 0.2]));
        c.record([12.5, 0.2], false);
        c.record([12.0, 0.0], true);
        assert_eq!(c.entrants[1].score, 1, "b has an H");
        assert_eq!(c.spot, None, "b leads next");
        assert_eq!(c.shooter(), Some(b));
        // a keeps setting and only b keeps missing.
        let mut guard = 0;
        while !c.finished() && guard < 200 {
            let who = c.shooter().unwrap();
            let leading = c.spot.is_none();
            c.record([12.0, 0.0], who == a || (who == c3 && !leading));
            guard += 1;
            if c.entrants[1].out {
                break;
            }
        }
        assert!(c.entrants[1].out);
        assert_eq!(c.entrants[1].score, 5);
    }
}
