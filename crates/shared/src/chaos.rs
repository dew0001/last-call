//! Chaos events (plan section 4.7): fired by the host during Open. One per
//! shift in week 1, up to three by week 6, picked at random by weight. Each
//! has a duration, a counter players can pull off, and a consequence if
//! they do not.

use serde::{Deserialize, Serialize};

use crate::rng::Draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChaosKind {
    /// 60 s: siren, cops enter after 20 s. Counter: clear the tables (pick
    /// up chip stacks, finish rounds) before they come in; with the Back
    /// Door, cash in hand walks out the back. Else all cash on tables seized.
    Raid,
    /// 45 s: two customers fight, props fly. Counter: drag both brawlers out
    /// the front door. Else customers leave and 3 props break.
    Brawl,
    /// 60 s (10 s with the Generator): darkness, tables pause, slots freeze.
    /// Counter: flip the breaker in the basement stairwell.
    Outage,
    /// Until fixed: one slot machine pays 10x. Counter: turn its service
    /// key. Hitting it with a stool makes it worse. Costs the house up to 2,000.
    SlotJam,
    /// 90 s: an inspector walks the bar. Counter: no loose beers and no
    /// vomit or puddles when the visit ends. Else a 1,500 fine.
    Inspector,
    /// 30 s: the loan shark plays blackjack. Deal fairly: he tips big if he
    /// wins. If the dealer is drunk, he breaks the table for the shift.
    LoanShark,
    /// 40 s: smoke, customers cough and leave. Counter: the extinguisher
    /// (upgrade) or a thrown beer. Else the kitchen is offline next shift.
    KitchenFire,
    /// Until caught: a customer at blackjack wins every hand. The Security
    /// Camera shows him; drag him out to stop it (with the camera, +500).
    CardCounter,
}

pub const ALL: [ChaosKind; 8] = [
    ChaosKind::Raid,
    ChaosKind::Brawl,
    ChaosKind::Outage,
    ChaosKind::SlotJam,
    ChaosKind::Inspector,
    ChaosKind::LoanShark,
    ChaosKind::KitchenFire,
    ChaosKind::CardCounter,
];

impl ChaosKind {
    /// Seconds the event lasts; `None` runs until countered (or the shift ends).
    pub fn duration(self, upgrades: &crate::upgrades::Upgrades) -> Option<u32> {
        match self {
            ChaosKind::Raid => Some(60),
            ChaosKind::Brawl => Some(upgrades.brawl_duration(45)),
            ChaosKind::Outage => Some(upgrades.outage_secs()),
            ChaosKind::SlotJam | ChaosKind::CardCounter => None,
            ChaosKind::Inspector => Some(90),
            ChaosKind::LoanShark => Some(30),
            ChaosKind::KitchenFire => Some(40),
        }
    }

    /// Relative chance of being picked.
    pub fn weight(self) -> u32 {
        match self {
            ChaosKind::Brawl => 3,
            _ => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChaosKind::Raid => "Police raid",
            ChaosKind::Brawl => "Brawl",
            ChaosKind::Outage => "Power outage",
            ChaosKind::SlotJam => "Rigged slot jam",
            ChaosKind::Inspector => "Health inspector",
            ChaosKind::LoanShark => "Loan shark visit",
            ChaosKind::KitchenFire => "Kitchen fire",
            ChaosKind::CardCounter => "Card counter",
        }
    }

    /// What players should do, for the HUD.
    pub fn hint(self) -> &'static str {
        match self {
            ChaosKind::Raid => "Cops in 20 s: pick up the chips, finish the rounds",
            ChaosKind::Brawl => "Drag the brawlers out the front door (E)",
            ChaosKind::Outage => "Flip the breaker on the basement stairs",
            ChaosKind::SlotJam => "Turn the jammed machine's service key",
            ChaosKind::Inspector => "Hide the beers, mop the floor",
            ChaosKind::LoanShark => "Deal him fair, and sober",
            ChaosKind::KitchenFire => "Extinguisher, or throw a beer on it",
            ChaosKind::CardCounter => "Find the counter and drag him out",
        }
    }
}

/// Events per shift: 1 in week 1, up to 3 by week 6 (more with new game plus).
pub fn events_per_shift(week: u8, ng: u8) -> u32 {
    (1 + u32::from(week.saturating_sub(1)) * 2 / 5 + u32::from(ng)).min(4)
}

/// Pick an event, weighted, from those not already running.
pub fn pick(running: &[ChaosKind], d: &mut impl Draw) -> Option<ChaosKind> {
    let open: Vec<ChaosKind> = ALL.into_iter().filter(|k| !running.contains(k)).collect();
    let total: u32 = open.iter().map(|k| k.weight()).sum();
    if total == 0 {
        return None;
    }
    let mut roll = d.below(total);
    for k in open {
        if roll < k.weight() {
            return Some(k);
        }
        roll -= k.weight();
    }
    None
}

/// Seconds into Open when each of `n` events starts: spread over the first
/// 80% of Open, each in its own slot.
pub fn schedule(n: u32, open_secs: u32, d: &mut impl Draw) -> Vec<u32> {
    let span = open_secs * 4 / 5;
    if n == 0 || span == 0 {
        return Vec::new();
    }
    let slot = (span / n).max(1);
    (0..n).map(|i| i * slot + d.below(slot)).collect()
}

/// Seconds after a raid starts until the cops come in.
pub const RAID_COPS_AFTER: u32 = 20;
/// The slot jam's payout multiplier.
pub const JAM_MULTIPLIER: i64 = 10;
/// The most a jammed machine can cost the house.
pub const JAM_CAP: i64 = 2_000;
/// The inspector's fine.
pub const INSPECTOR_FINE: i64 = 1_500;
/// The loan shark's tip to the dealer when he wins a round.
pub const SHARK_TIP: i64 = 200;
/// The loan shark's bet.
pub const SHARK_BET: i64 = 100;
/// Props broken by a brawl nobody stopped.
pub const BRAWL_BREAKS: usize = 3;

/// How an event ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ending {
    /// Players dealt with it.
    Countered,
    /// It ran its course: the consequence applies.
    Consequence,
    /// The shift moved on (Last call) first.
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn more_events_later_in_the_run() {
        assert_eq!(events_per_shift(1, 0), 1);
        assert_eq!(events_per_shift(3, 0), 1);
        assert_eq!(events_per_shift(4, 0), 2);
        assert_eq!(events_per_shift(6, 0), 3);
        assert_eq!(events_per_shift(6, 1), 4);
    }

    #[test]
    fn picks_follow_weights_and_skip_running_events() {
        let mut d = SimRng::new(3);
        let mut brawls = 0;
        for _ in 0..17_000 {
            if pick(&[], &mut d) == Some(ChaosKind::Brawl) {
                brawls += 1;
            }
        }
        // Brawl is 3 of 17.
        assert!((2_700..3_300).contains(&brawls), "{brawls}");
        assert_eq!(pick(&ALL, &mut d), None);
        assert_ne!(pick(&[ChaosKind::Brawl], &mut d), Some(ChaosKind::Brawl));
    }

    #[test]
    fn schedules_spread_over_open() {
        let mut d = SimRng::new(1);
        let s = schedule(3, 540, &mut d);
        assert_eq!(s.len(), 3);
        assert!(s.windows(2).all(|w| w[0] < w[1]));
        assert!(*s.last().unwrap() < 432);
    }

    #[test]
    fn durations_follow_upgrades() {
        let mut u = crate::upgrades::Upgrades::default();
        assert_eq!(ChaosKind::Outage.duration(&u), Some(60));
        assert_eq!(ChaosKind::SlotJam.duration(&u), None);
        let mut house = 100_000;
        u.buy(crate::upgrades::UpgradeId::Generator, &mut house).unwrap();
        u.buy(crate::upgrades::UpgradeId::Bouncer, &mut house).unwrap();
        assert_eq!(ChaosKind::Outage.duration(&u), Some(10));
        assert_eq!(ChaosKind::Brawl.duration(&u), Some(22));
    }
}
