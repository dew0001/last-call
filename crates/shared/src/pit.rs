//! The fight pit in the basement (plan section 5.9): an airsoft arena.
//!
//! 60-second rounds for 2 to 8 players, free for all or teams by vote.
//! Weapons from the wall racks: pistol (semi, 12 rounds, two hits), SMG
//! (auto, 30 rounds, four hits), pump shotgun (6 shells, a spread of
//! pellets, one hit up close) and a foam bat (melee, knockback). Shots are
//! hitscan; the host rewinds the other players to what the shooter saw
//! (lag compensation, at most 200 ms). Headshots do 1.5 times the damage.
//! A player who runs out of health goes down for 3 seconds, then respawns
//! at a corner. Each takedown scores 1; most takedowns take the pot.

use serde::{Deserialize, Serialize};

use crate::minigame::Who;
use crate::sports::split_pot;

/// The pit: (x0, x1, z0, z1), inside the basement.
pub const PIT: (f32, f32, f32, f32) = (-32.0, -18.0, -6.0, 6.0);
/// Respawn corners.
pub const CORNERS: [(f32, f32); 4] = [(-31.0, -5.0), (-19.0, -5.0), (-31.0, 5.0), (-19.0, 5.0)];
/// Weapon racks: (weapon, x, z).
pub const RACKS: [(Weapon, f32, f32); 4] = [
    (Weapon::Pistol, -25.0, -5.6),
    (Weapon::Smg, -25.0, 5.6),
    (Weapon::Shotgun, -31.6, 0.0),
    (Weapon::Bat, -18.4, 0.0),
];
pub const RACK_REACH: f32 = 1.2;
pub const ROUND_SECS: u32 = 60;
pub const DOWN_TICKS: u32 = 3 * crate::TICK_HZ;
pub const HEALTH: i32 = 100;
pub const HEADSHOT_PERCENT: i32 = 150;
/// The most the host rewinds for a shot.
pub const MAX_REWIND_TICKS: u32 = 200 * crate::TICK_HZ / 1000;
/// Player hit shape: a capsule from the floor, and a head sphere on top.
pub const BODY_RADIUS: f32 = 0.35;
pub const BODY_TOP: f32 = 1.45;
pub const HEAD_CENTER: f32 = 1.62;
pub const HEAD_RADIUS: f32 = 0.18;
pub const EYE_HEIGHT: f32 = 1.6;
pub const ENTRY: i64 = 25;
/// A spectator's thrown beer that hits a player: drunk points.
pub const BEER_HIT_DRUNK: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Weapon {
    Pistol,
    Smg,
    Shotgun,
    Bat,
}

impl Weapon {
    pub fn damage(self) -> i32 {
        match self {
            Weapon::Pistol => 50,
            Weapon::Smg => 25,
            // Per pellet; eight pellets.
            Weapon::Shotgun => 20,
            Weapon::Bat => 50,
        }
    }

    pub fn magazine(self) -> u8 {
        match self {
            Weapon::Pistol => 12,
            Weapon::Smg => 30,
            Weapon::Shotgun => 6,
            Weapon::Bat => 0,
        }
    }

    /// Ticks between shots.
    pub fn cooldown(self) -> u32 {
        match self {
            Weapon::Pistol => 16,
            Weapon::Smg => 6,
            Weapon::Shotgun => 51,
            Weapon::Bat => 38,
        }
    }

    pub fn range(self) -> f32 {
        match self {
            Weapon::Bat => 1.6,
            Weapon::Shotgun => 12.0,
            _ => 40.0,
        }
    }

    pub fn pellets(self) -> u32 {
        if self == Weapon::Shotgun { 8 } else { 1 }
    }

    /// Pellet spread, radians from the center line.
    pub fn spread(self) -> f32 {
        if self == Weapon::Shotgun { 0.07 } else { 0.0 }
    }

    pub fn label(self) -> &'static str {
        match self {
            Weapon::Pistol => "pistol",
            Weapon::Smg => "SMG",
            Weapon::Shotgun => "shotgun",
            Weapon::Bat => "foam bat",
        }
    }
}

/// Shotgun pellets lose damage with distance: full to 4 m, half past 8 m.
pub fn falloff(weapon: Weapon, dist: f32) -> i32 {
    if weapon != Weapon::Shotgun || dist <= 4.0 {
        100
    } else if dist >= 8.0 {
        50
    } else {
        (100.0 - (dist - 4.0) / 4.0 * 50.0) as i32
    }
}

/// The unit direction for a look (yaw 0 faces -Z, pitch up is positive).
pub fn look_dir(yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = crate::math::sin_cos(yaw);
    let (sp, cp) = crate::math::sin_cos(pitch);
    [-sy * cp, sp, -cy * cp]
}

/// Where a ray from `o` along unit `d` first meets a sphere, if it does.
fn ray_sphere(o: [f32; 3], d: [f32; 3], c: [f32; 3], r: f32) -> Option<f32> {
    let oc = [o[0] - c[0], o[1] - c[1], o[2] - c[2]];
    let b = oc[0] * d[0] + oc[1] * d[1] + oc[2] * d[2];
    let cc = oc[0] * oc[0] + oc[1] * oc[1] + oc[2] * oc[2] - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t >= 0.0).then_some(t)
}

/// Where a ray meets a vertical cylinder (radius `r`, from `y0` to `y1`) at (cx, cz).
fn ray_cylinder(o: [f32; 3], d: [f32; 3], c: [f32; 2], r: f32, y0: f32, y1: f32) -> Option<f32> {
    let (ox, oz) = (o[0] - c[0], o[2] - c[1]);
    let a = d[0] * d[0] + d[2] * d[2];
    if a < 1e-9 {
        return None;
    }
    let b = ox * d[0] + oz * d[2];
    let cc = ox * ox + oz * oz - r * r;
    let disc = b * b - a * cc;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    let y = o[1] + d[1] * t;
    (t >= 0.0 && (y0..=y1).contains(&y)).then_some(t)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Part {
    Body,
    Head,
}

/// Does a ray hit a player standing at `feet`? (distance, part)
pub fn hit_player(o: [f32; 3], d: [f32; 3], feet: [f32; 3]) -> Option<(f32, Part)> {
    let head = ray_sphere(o, d, [feet[0], feet[1] + HEAD_CENTER, feet[2]], HEAD_RADIUS).map(|t| (t, Part::Head));
    let body =
        ray_cylinder(o, d, [feet[0], feet[2]], BODY_RADIUS, feet[1], feet[1] + BODY_TOP).map(|t| (t, Part::Body));
    match (head, body) {
        (Some(h), Some(b)) => Some(if h.0 <= b.0 { h } else { b }),
        (h, b) => h.or(b),
    }
}

/// The damage one hit does.
pub fn damage(weapon: Weapon, part: Part, dist: f32) -> i32 {
    let base = weapon.damage() * falloff(weapon, dist) / 100;
    if part == Part::Head { base * HEADSHOT_PERCENT / 100 } else { base }
}

pub fn in_pit(x: f32, z: f32) -> bool {
    (PIT.0..=PIT.1).contains(&x) && (PIT.2..=PIT.3).contains(&z)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Teams {
    FreeForAll,
    TwoTeams,
}

/// A fighter in a round.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fighter {
    pub who: Who,
    pub team: u8,
    pub health: i32,
    pub kills: u8,
    pub weapon: Option<Weapon>,
    pub ammo: u8,
    /// Ticks until the next shot.
    pub cooldown: u32,
    /// Ticks left down.
    pub down: u32,
    pub vote_teams: bool,
}

/// One round in the pit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub fighters: Vec<Fighter>,
    pub teams: Teams,
    pub ticks_left: u32,
    pub started: bool,
    pub pot: i64,
}

impl Round {
    pub fn new() -> Self {
        Self {
            fighters: Vec::new(),
            teams: Teams::FreeForAll,
            ticks_left: ROUND_SECS * crate::TICK_HZ,
            started: false,
            pot: 0,
        }
    }

    pub fn join(&mut self, who: Who, fee: i64, vote_teams: bool) -> bool {
        if self.started || self.fighters.len() >= crate::MAX_PLAYERS || self.fighters.iter().any(|f| f.who == who) {
            return false;
        }
        self.fighters.push(Fighter {
            who,
            team: 0,
            health: HEALTH,
            kills: 0,
            weapon: None,
            ammo: 0,
            cooldown: 0,
            down: 0,
            vote_teams,
        });
        self.pot += fee;
        true
    }

    /// Start with two or more: teams if most voted for them and the count is even.
    pub fn start(&mut self) -> bool {
        let n = self.fighters.len();
        if self.started || n < 2 {
            return false;
        }
        let votes = self.fighters.iter().filter(|f| f.vote_teams).count();
        self.teams = if votes * 2 > n && n.is_multiple_of(2) && n >= 4 { Teams::TwoTeams } else { Teams::FreeForAll };
        for (i, f) in self.fighters.iter_mut().enumerate() {
            f.team = if self.teams == Teams::TwoTeams { (i % 2) as u8 } else { i as u8 };
        }
        self.started = true;
        true
    }

    pub fn fighter(&self, who: Who) -> Option<usize> {
        self.fighters.iter().position(|f| f.who == who)
    }

    /// Pick a weapon from a rack: a full magazine.
    pub fn pick(&mut self, who: Who, weapon: Weapon) {
        if let Some(i) = self.fighter(who) {
            self.fighters[i].weapon = Some(weapon);
            self.fighters[i].ammo = weapon.magazine();
        }
    }

    /// May this fighter fire now? Spends a round (not for the bat) and starts the cooldown.
    pub fn fire(&mut self, who: Who) -> Option<Weapon> {
        let i = self.fighter(who)?;
        let f = &mut self.fighters[i];
        let w = f.weapon?;
        if !self.started || f.down > 0 || f.cooldown > 0 || (w != Weapon::Bat && f.ammo == 0) {
            return None;
        }
        if w != Weapon::Bat {
            f.ammo -= 1;
        }
        f.cooldown = w.cooldown();
        Some(w)
    }

    /// Apply damage from `by` to `to`. Returns true on a takedown.
    pub fn hit(&mut self, by: Who, to: Who, dmg: i32) -> bool {
        let (Some(a), Some(b)) = (self.fighter(by), self.fighter(to)) else { return false };
        if a == b || self.fighters[b].down > 0 {
            return false;
        }
        if self.teams == Teams::TwoTeams && self.fighters[a].team == self.fighters[b].team {
            return false;
        }
        self.fighters[b].health -= dmg;
        if self.fighters[b].health <= 0 {
            self.fighters[b].down = DOWN_TICKS;
            self.fighters[b].health = HEALTH;
            self.fighters[a].kills += 1;
            return true;
        }
        false
    }

    /// One tick: cooldowns and the clock. Returns the fighters who got up
    /// this tick (to respawn at a corner), and whether the round is over.
    pub fn step(&mut self) -> (Vec<Who>, bool) {
        let mut up = Vec::new();
        for f in &mut self.fighters {
            f.cooldown = f.cooldown.saturating_sub(1);
            if f.down > 0 {
                f.down -= 1;
                if f.down == 0 {
                    up.push(f.who);
                }
            }
        }
        if self.started {
            self.ticks_left = self.ticks_left.saturating_sub(1);
        }
        (up, self.started && self.ticks_left == 0)
    }

    /// Most takedowns win; a team's takedowns count together. Ties split.
    pub fn payouts(&self) -> Vec<(Who, i64)> {
        let winners: Vec<Who> = match self.teams {
            Teams::FreeForAll => crate::sports::best(
                &self
                    .fighters
                    .iter()
                    .map(|f| crate::sports::Entrant { who: f.who, tries: 0, score: f.kills, out: false })
                    .collect::<Vec<_>>(),
            ),
            Teams::TwoTeams => {
                let score =
                    |t: u8| self.fighters.iter().filter(|f| f.team == t).map(|f| u32::from(f.kills)).sum::<u32>();
                let (a, b) = (score(0), score(1));
                self.fighters
                    .iter()
                    .filter(|f| (a >= b && f.team == 0) || (b >= a && f.team == 1))
                    .map(|f| f.who)
                    .collect()
            }
        };
        split_pot(self.pot, &winners)
    }
}

impl Default for Round {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_hit_bodies_and_heads() {
        let o = [0.0, EYE_HEIGHT, 0.0];
        let target = [0.0, 0.0, -10.0];
        let (t, part) = hit_player(o, look_dir(0.0, 0.0), target).unwrap();
        assert_eq!(part, Part::Head);
        assert!((t - (10.0 - HEAD_RADIUS)).abs() < 0.05, "{t}");
        let (_, part) = hit_player(o, look_dir(0.0, -0.08), target).unwrap();
        assert_eq!(part, Part::Body);
        assert!(hit_player(o, look_dir(0.2, 0.0), target).is_none(), "wide");
        assert!(hit_player(o, look_dir(std::f32::consts::PI, 0.0), target).is_none(), "behind");
    }

    #[test]
    fn hits_to_take_down_match_the_plan() {
        let to_down = |w: Weapon, dist: f32| (HEALTH + damage(w, Part::Body, dist) - 1) / damage(w, Part::Body, dist);
        assert_eq!(to_down(Weapon::Pistol, 10.0), 2);
        assert_eq!(to_down(Weapon::Smg, 10.0), 4);
        // Eight pellets up close: one blast.
        assert!(damage(Weapon::Shotgun, Part::Body, 2.0) * 8 >= HEALTH);
        assert!(damage(Weapon::Shotgun, Part::Body, 10.0) * 8 < HEALTH * 2);
        assert_eq!(damage(Weapon::Pistol, Part::Head, 5.0), 75);
    }

    #[test]
    fn a_round_counts_takedowns_and_pays_the_best() {
        let (a, b, c) = (Who::Player(1), Who::Player(2), Who::Player(3));
        let mut r = Round::new();
        r.join(a, ENTRY, false);
        assert!(!r.start(), "two at least");
        r.join(b, ENTRY, false);
        r.join(c, ENTRY, false);
        assert!(r.start());
        assert_eq!(r.fire(a), None, "no weapon");
        r.pick(a, Weapon::Pistol);
        assert_eq!(r.fire(a), Some(Weapon::Pistol));
        assert_eq!(r.fire(a), None, "cooldown");
        assert!(!r.hit(a, b, 50));
        assert!(r.hit(a, b, 50), "two pistol hits");
        assert!(!r.hit(a, b, 50), "down players take no hits");
        let mut ups = Vec::new();
        for _ in 0..DOWN_TICKS {
            ups.extend(r.step().0);
        }
        assert_eq!(ups, vec![b]);
        assert_eq!(r.fighters[0].ammo, 11);
        let p = r.payouts();
        assert_eq!(p, vec![(a, ENTRY * 3)]);
    }

    #[test]
    fn teams_need_a_majority_and_an_even_count() {
        let mut r = Round::new();
        for i in 0..4 {
            r.join(Who::Player(i), ENTRY, i < 3);
        }
        r.start();
        assert_eq!(r.teams, Teams::TwoTeams);
        assert!(!r.hit(Who::Player(0), Who::Player(2), 200), "no friendly fire");
        assert!(r.hit(Who::Player(0), Who::Player(1), 200));
        let p = r.payouts();
        assert_eq!(p.len(), 2, "the team splits");
        let mut r = Round::new();
        for i in 0..3 {
            r.join(Who::Player(i), ENTRY, true);
        }
        r.start();
        assert_eq!(r.teams, Teams::FreeForAll, "three cannot split evenly");
    }

    #[test]
    fn the_round_ends_after_sixty_seconds() {
        let mut r = Round::new();
        r.join(Who::Player(1), ENTRY, false);
        r.join(Who::Player(2), ENTRY, false);
        r.start();
        let mut over = false;
        for _ in 0..(ROUND_SECS * 64) {
            over = r.step().1;
        }
        assert!(over);
    }
}
