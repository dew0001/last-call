//! Fishing on the pier (plan section 5.5).
//!
//! Cast: the charge (0 to 100) picks a depth zone, and the zone's table
//! picks the fish. Bite: 5 to 25 seconds later the bobber dips; hook within
//! 600 ms. Reel: hold to raise the line's tension, let go to lower it. Keep
//! the tension in the band until the fish tires (seconds by fish). The fish
//! pulls in a steady rhythm. The line snaps at full tension; a slack line
//! for 3 seconds lets the fish go. Focus widens the band.
//!
//! Other players bet pocket money on "lands it" or "snaps" at 1 to 1 while
//! a fight is on.
//!
//! All integer math, so native and wasm hosts agree tick for tick.

use serde::{Deserialize, Serialize};

use crate::minigame::{Bet, Minigame, Payout, Refused, Who};
use crate::rng::Draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Fish {
    Minnow,
    Bass,
    Catfish,
    Shark,
    /// Worth nothing, but an achievement ("Catch the boot").
    Boot,
}

impl Fish {
    /// What it sells for.
    pub fn value(self) -> i64 {
        match self {
            Fish::Minnow => 5,
            Fish::Bass => 20,
            Fish::Catfish => 45,
            Fish::Shark => 300,
            Fish::Boot => 0,
        }
    }

    /// Seconds in the band to land it.
    pub fn fight_secs(self) -> u32 {
        match self {
            Fish::Minnow => 3,
            Fish::Bass => 5,
            Fish::Catfish => 8,
            Fish::Shark => 90,
            Fish::Boot => 2,
        }
    }

    /// The most the fish adds to the tension in one tick.
    pub fn pull(self) -> u32 {
        match self {
            Fish::Minnow => 3,
            Fish::Bass => 6,
            Fish::Catfish => 9,
            Fish::Shark => 12,
            Fish::Boot => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fish::Minnow => "minnow",
            Fish::Bass => "bass",
            Fish::Catfish => "catfish",
            Fish::Shark => "shark",
            Fish::Boot => "boot",
        }
    }
}

/// Fish tables by zone (near, middle, far): (fish, weight out of 100).
/// The shark is 1% everywhere.
pub const ZONES: [[(Fish, u32); 5]; 3] = [
    [(Fish::Minnow, 60), (Fish::Bass, 25), (Fish::Catfish, 5), (Fish::Boot, 9), (Fish::Shark, 1)],
    [(Fish::Minnow, 30), (Fish::Bass, 40), (Fish::Catfish, 20), (Fish::Boot, 9), (Fish::Shark, 1)],
    [(Fish::Minnow, 10), (Fish::Bass, 30), (Fish::Catfish, 50), (Fish::Boot, 9), (Fish::Shark, 1)],
];

/// The zone a cast with this charge reaches.
pub fn zone(power: u8) -> u8 {
    match power {
        0..=33 => 0,
        34..=66 => 1,
        _ => 2,
    }
}

pub fn pick_fish(zone: u8, d: &mut impl Draw) -> Fish {
    let table = &ZONES[usize::from(zone.min(2))];
    let mut roll = d.below(100);
    for (fish, w) in table {
        if roll < *w {
            return *fish;
        }
        roll -= w;
    }
    Fish::Minnow
}

/// Fishing spots at the end of the pier: (x, z).
pub const SPOTS: [(f32, f32); 2] = [(-2.0, 45.5), (2.0, 45.5)];
/// How close a fisher stands to a spot.
pub const SPOT_REACH: f32 = 1.5;
/// Bet limits on a fight.
pub const BET_MAX: i64 = 200;

/// The spot a player at (x, z) stands at.
pub fn spot_at(x: f32, z: f32) -> Option<u8> {
    SPOTS.iter().position(|(sx, sz)| (sx - x).hypot(sz - z) <= SPOT_REACH).map(|i| i as u8)
}

/// Bite delay, seconds.
pub const BITE_SECS: (u32, u32) = (5, 25);
/// Ticks to hook once the bobber dips (600 ms).
pub const HOOK_TICKS: u32 = 38;
/// Full tension: the line snaps.
pub const SNAP: u32 = 1000;
/// The band to hold, and the wider band with Focus.
pub const BAND: (u32, u32) = (400, 750);
pub const FOCUS_BAND: (u32, u32) = (330, 820);
/// Tension per tick while reeling, and lost per tick while not.
pub const RISE: u32 = 12;
pub const FALL: u32 = 16;
/// Ticks of a slack line before the fish gets away.
pub const SLACK_TICKS: u32 = 3 * crate::TICK_HZ;
/// Ticks in one cycle of the fish's pull.
pub const PULL_PERIOD: u32 = 96;
/// Tension when the fish is hooked.
pub const HOOKED_TENSION: u32 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinePhase {
    /// Waiting for a bite: ticks left.
    Waiting {
        left: u32,
    },
    /// The bobber is down: ticks left to hook.
    Biting {
        left: u32,
    },
    Reeling,
    Done(Catch),
}

/// How a cast ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Catch {
    Landed(Fish),
    Snapped,
    /// The line went slack and the fish swam off.
    Escaped,
    /// Hooked too early or too late.
    Missed,
}

/// One line in the water.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub zone: u8,
    pub fish: Fish,
    pub phase: LinePhase,
    /// 0 to [`SNAP`].
    pub tension: u32,
    /// Ticks spent in the band.
    pub progress: u32,
    pub slack: u32,
    pub ticks: u32,
    pub band: (u32, u32),
}

impl Line {
    /// Ticks in the band needed to land the fish.
    pub fn need(&self) -> u32 {
        self.fish.fight_secs() * crate::TICK_HZ
    }

    /// The fish's pull this tick: a triangle wave from 0 to its strength.
    pub fn pull(&self) -> u32 {
        let p = self.ticks % PULL_PERIOD;
        let tri = if p < PULL_PERIOD / 2 { p } else { PULL_PERIOD - p };
        self.fish.pull() * tri * 2 / PULL_PERIOD
    }

    pub fn in_band(&self) -> bool {
        (self.band.0..=self.band.1).contains(&self.tension)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastParams {
    /// Cast charge, 0 to 100.
    pub power: u8,
    /// The fisher has Focus: a wider band.
    pub focus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FishInput {
    /// One tick passes, with the reel held or not.
    Tick { held: bool },
    /// Strike to set the hook.
    Hook,
}

/// What a bettor backs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Lands,
    Snaps,
}

pub struct Fishing;

impl Minigame for Fishing {
    type Params = CastParams;
    type Input = FishInput;
    type State = Line;
    type Outcome = Catch;
    type Selection = Side;

    fn start(d: &mut impl Draw, p: &CastParams) -> Line {
        let zone = zone(p.power);
        let fish = pick_fish(zone, d);
        let secs = BITE_SECS.0 + d.below(BITE_SECS.1 - BITE_SECS.0 + 1);
        Line {
            zone,
            fish,
            phase: LinePhase::Waiting { left: secs * crate::TICK_HZ },
            tension: 0,
            progress: 0,
            slack: 0,
            ticks: 0,
            band: if p.focus { FOCUS_BAND } else { BAND },
        }
    }

    fn apply(line: &mut Line, _: Who, input: FishInput, _: &mut impl Draw) -> Result<Option<Catch>, Refused> {
        let done = |line: &mut Line, c: Catch| {
            line.phase = LinePhase::Done(c);
            Ok(Some(c))
        };
        match (line.phase, input) {
            (LinePhase::Done(_), _) => Err(Refused::NotAllowed),
            (LinePhase::Waiting { .. }, FishInput::Hook) => done(line, Catch::Missed),
            (LinePhase::Waiting { left }, FishInput::Tick { .. }) => {
                line.phase = if left <= 1 {
                    LinePhase::Biting { left: HOOK_TICKS }
                } else {
                    LinePhase::Waiting { left: left - 1 }
                };
                Ok(None)
            }
            (LinePhase::Biting { .. }, FishInput::Hook) => {
                line.phase = LinePhase::Reeling;
                line.tension = HOOKED_TENSION;
                Ok(None)
            }
            (LinePhase::Biting { left }, FishInput::Tick { .. }) => {
                if left <= 1 {
                    return done(line, Catch::Missed);
                }
                line.phase = LinePhase::Biting { left: left - 1 };
                Ok(None)
            }
            (LinePhase::Reeling, FishInput::Hook) => Ok(None),
            (LinePhase::Reeling, FishInput::Tick { held }) => {
                line.ticks += 1;
                let pull = line.pull();
                line.tension =
                    if held { line.tension + RISE + pull } else { (line.tension + pull).saturating_sub(FALL) };
                if line.tension >= SNAP {
                    line.tension = SNAP;
                    return done(line, Catch::Snapped);
                }
                if line.in_band() {
                    line.progress += 1;
                }
                if line.tension == 0 {
                    line.slack += 1;
                    if line.slack >= SLACK_TICKS {
                        return done(line, Catch::Escaped);
                    }
                } else {
                    line.slack = 0;
                }
                if line.progress >= line.need() {
                    return done(line, Catch::Landed(line.fish));
                }
                Ok(None)
            }
        }
    }

    /// Bets pay 1 to 1: "lands it" wins on a landed fish, "snaps" on anything else.
    fn payout(c: &Catch, bets: &[Bet<Side>]) -> Vec<Payout> {
        let landed = matches!(c, Catch::Landed(_));
        bets.iter()
            .map(|b| {
                let won = (b.selection == Side::Lands) == landed;
                Payout { who: b.who, staked: b.amount, returned: if won { b.amount * 2 } else { 0 } }
            })
            .collect()
    }
}

/// The reel a steady hand would use: hold below the band's middle.
pub fn bot_holds(line: &Line) -> bool {
    line.tension < (line.band.0 + line.band.1) / 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    fn tick(line: &mut Line, held: bool, d: &mut SimRng) -> Option<Catch> {
        // After the end, ticks are refused: nothing more happens.
        Fishing::apply(line, Who::Player(1), FishInput::Tick { held }, d).ok().flatten()
    }

    /// Wait for the bite and hook it.
    fn hooked(power: u8, focus: bool, d: &mut SimRng) -> Line {
        let mut line = Fishing::start(d, &CastParams { power, focus });
        while matches!(line.phase, LinePhase::Waiting { .. }) {
            assert_eq!(tick(&mut line, false, d), None);
        }
        Fishing::apply(&mut line, Who::Player(1), FishInput::Hook, d).unwrap();
        assert_eq!(line.phase, LinePhase::Reeling);
        line
    }

    #[test]
    fn zones_follow_the_charge_and_tables_sum_to_100() {
        assert_eq!((zone(0), zone(50), zone(100)), (0, 1, 2));
        for t in &ZONES {
            assert_eq!(t.iter().map(|(_, w)| w).sum::<u32>(), 100);
            assert!(t.contains(&(Fish::Shark, 1)));
        }
    }

    #[test]
    fn deeper_casts_catch_bigger_fish() {
        let mut d = SimRng::new(4);
        let avg = |z: u8, d: &mut SimRng| (0..20_000).map(|_| pick_fish(z, d).value()).sum::<i64>() / 20_000;
        let (near, far) = (avg(0, &mut d), avg(2, &mut d));
        assert!(far > near, "{near} {far}");
        let sharks = (0..100_000).filter(|_| pick_fish(1, &mut d) == Fish::Shark).count();
        assert!((800..1200).contains(&sharks), "{sharks}");
    }

    #[test]
    fn the_bite_comes_in_5_to_25_seconds_and_must_be_hooked_in_600_ms() {
        let mut d = SimRng::new(1);
        for _ in 0..200 {
            let line = Fishing::start(&mut d, &CastParams { power: 50, focus: false });
            let LinePhase::Waiting { left } = line.phase else { panic!() };
            assert!((5 * 64..=25 * 64).contains(&left));
        }
        // Too early.
        let mut line = Fishing::start(&mut d, &CastParams { power: 50, focus: false });
        assert_eq!(Fishing::apply(&mut line, Who::Player(1), FishInput::Hook, &mut d), Ok(Some(Catch::Missed)));
        // Too late.
        let mut line = Fishing::start(&mut d, &CastParams { power: 50, focus: false });
        let mut out = None;
        for _ in 0..(26 * 64) {
            out = out.or(tick(&mut line, false, &mut d));
        }
        assert_eq!(out, Some(Catch::Missed));
    }

    #[test]
    fn a_steady_hand_lands_every_fish_and_holding_on_snaps_the_line() {
        let mut d = SimRng::new(9);
        for fish in [Fish::Minnow, Fish::Bass, Fish::Catfish, Fish::Shark, Fish::Boot] {
            let mut line = hooked(50, false, &mut d);
            line.fish = fish;
            let mut out = None;
            for _ in 0..(200 * 64) {
                let held = bot_holds(&line);
                if let Some(c) = tick(&mut line, held, &mut d) {
                    out = Some(c);
                    break;
                }
            }
            assert_eq!(out, Some(Catch::Landed(fish)), "{fish:?}");
        }
        let mut line = hooked(50, false, &mut d);
        let mut out = None;
        for _ in 0..640 {
            out = out.or(tick(&mut line, true, &mut d));
        }
        assert_eq!(out, Some(Catch::Snapped));
        let mut line = hooked(50, false, &mut d);
        let mut out = None;
        for _ in 0..(10 * 64) {
            out = out.or(tick(&mut line, false, &mut d));
        }
        assert_eq!(out, Some(Catch::Escaped));
    }

    #[test]
    fn focus_widens_the_band() {
        let mut d = SimRng::new(2);
        let a = Fishing::start(&mut d, &CastParams { power: 10, focus: false });
        let b = Fishing::start(&mut d, &CastParams { power: 10, focus: true });
        assert!(b.band.1 - b.band.0 > a.band.1 - a.band.0);
    }

    #[test]
    fn bets_pay_one_to_one() {
        let bets = [
            Bet { who: Who::Player(1), amount: 10, selection: Side::Lands },
            Bet { who: Who::Player(2), amount: 20, selection: Side::Snaps },
        ];
        let p = Fishing::payout(&Catch::Landed(Fish::Bass), &bets);
        assert_eq!((p[0].returned, p[1].returned), (20, 0));
        let p = Fishing::payout(&Catch::Escaped, &bets);
        assert_eq!((p[0].returned, p[1].returned), (0, 40));
    }
}
