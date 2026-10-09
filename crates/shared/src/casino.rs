//! The casino floor (plan section 5, Phase 3): where the tables stand, what a
//! player can ask a table to do, and the betting limits.

use serde::{Deserialize, Serialize};

use crate::drunk::{self, Tier};
use crate::rng::StreamId;
use crate::{blackjack, roulette};

/// A table or machine on the floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TableId {
    Blackjack,
    Roulette,
    Slot(u8),
}

/// Slot machines on the floor.
pub const SLOT_MACHINES: u8 = 2;

impl TableId {
    /// Each table draws from its own RNG stream.
    pub fn stream(self) -> StreamId {
        match self {
            TableId::Blackjack => StreamId(10),
            TableId::Roulette => StreamId(20),
            TableId::Slot(i) => StreamId(30 + u32::from(i)),
        }
    }

    pub fn all() -> Vec<TableId> {
        let mut out = vec![TableId::Blackjack, TableId::Roulette];
        out.extend((0..SLOT_MACHINES).map(TableId::Slot));
        out
    }
}

/// What a player asks a table to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableAction {
    /// Run the table: deal blackjack or spin the wheel. Refused when Wasted.
    TakeRole,
    LeaveRole,
    /// Blackjack: take the nearest free seat and set the bet for the next
    /// round. 0 stands up.
    Bet(i64),
    /// Blackjack, the dealer: deal the round.
    Deal,
    Insure(bool),
    Play(blackjack::Action),
    /// Blackjack, the dealer: hit or stand for the house.
    Dealer(blackjack::Action),
    /// Roulette: put chips on the layout before the spin.
    RouletteBet(roulette::Bet, i64),
    /// Roulette, the croupier: spin. Needs bets on the layout and no losing
    /// chips left from the last spin.
    Spin,
    /// Roulette, the croupier: sweep losing chips off the layout.
    Rake,
    /// Slots: pull the lever with this bet.
    Pull(i64),
}

// ---------- Layout ----------

/// Blackjack table: center (x, z), half extents, felt height.
pub const BLACKJACK: (f32, f32) = (-5.0, 1.0);
pub const BLACKJACK_HALF: (f32, f32) = (1.1, 0.55);
/// Roulette table; the wheel sits at its west end, the layout to the east.
pub const ROULETTE: (f32, f32) = (4.5, 1.0);
pub const ROULETTE_HALF: (f32, f32) = (1.3, 0.6);
pub const FELT_HEIGHT: f32 = 0.8;
/// Slot machines stand against the west wall.
pub const SLOTS: [(f32, f32); 2] = [(-9.6, -1.5), (-9.6, 0.5)];
pub const SLOT_HALF: (f32, f32) = (0.35, 0.35);
pub const SLOT_HEIGHT: f32 = 1.6;

/// How close to a role spot (dealer, croupier) a player must stand.
pub const ROLE_REACH: f32 = 1.2;
/// How close to a table's edge a bettor must stand.
pub const BET_REACH: f32 = 1.0;
/// How close to a slot machine's front a player must stand.
pub const SLOT_REACH: f32 = 1.0;

/// Where the dealer or croupier stands (north side, facing +Z).
pub fn role_spot(table: TableId) -> Option<(f32, f32)> {
    match table {
        TableId::Blackjack => Some((BLACKJACK.0, BLACKJACK.1 - BLACKJACK_HALF.1 - 0.6)),
        TableId::Roulette => Some((ROULETTE.0, ROULETTE.1 - ROULETTE_HALF.1 - 0.6)),
        TableId::Slot(_) => None,
    }
}

/// Spots for bettors along a table's south side (or a machine's front).
pub fn bettor_spots(table: TableId) -> Vec<(f32, f32)> {
    match table {
        TableId::Blackjack => {
            let z = BLACKJACK.1 + BLACKJACK_HALF.1 + 0.45;
            (0..blackjack::SEATS).map(|i| (BLACKJACK.0 - 1.0 + i as f32 * 0.5, z)).collect()
        }
        TableId::Roulette => {
            let z = ROULETTE.1 + ROULETTE_HALF.1 + 0.45;
            (0..ROULETTE_SPOTS).map(|i| (ROULETTE.0 - 1.0 + i as f32 * 0.4, z)).collect()
        }
        TableId::Slot(i) => {
            let (x, z) = SLOTS[usize::from(i)];
            vec![(x + SLOT_HALF.0 + 0.45, z)]
        }
    }
}

/// Bettor spots at the roulette table.
pub const ROULETTE_SPOTS: usize = 6;

/// Distance from (x, z) to the nearest point of a box.
fn to_box(x: f32, z: f32, (cx, cz): (f32, f32), (hx, hz): (f32, f32)) -> f32 {
    let dx = ((x - cx).abs() - hx).max(0.0);
    let dz = ((z - cz).abs() - hz).max(0.0);
    dx.hypot(dz)
}

/// Can a player at (x, z) bet at `table`?
pub fn can_reach(table: TableId, x: f32, z: f32) -> bool {
    match table {
        TableId::Blackjack => to_box(x, z, BLACKJACK, BLACKJACK_HALF) < BET_REACH,
        TableId::Roulette => to_box(x, z, ROULETTE, ROULETTE_HALF) < BET_REACH,
        TableId::Slot(i) => SLOTS.get(usize::from(i)).is_some_and(|s| to_box(x, z, *s, SLOT_HALF) < SLOT_REACH),
    }
}

/// Is a player at (x, z) close enough to run `table`?
pub fn at_role_spot(table: TableId, x: f32, z: f32) -> bool {
    role_spot(table).is_some_and(|(sx, sz)| (x - sx).hypot(z - sz) < ROLE_REACH)
}

fn table_box(table: TableId) -> ((f32, f32), (f32, f32)) {
    match table {
        TableId::Blackjack => (BLACKJACK, BLACKJACK_HALF),
        TableId::Roulette => (ROULETTE, ROULETTE_HALF),
        TableId::Slot(i) => (SLOTS[usize::from(i) % SLOTS.len()], SLOT_HALF),
    }
}

/// Which table, if any, a player at (x, z) is at: a role spot first, else
/// the nearest table in reach.
pub fn nearest_table(x: f32, z: f32) -> Option<TableId> {
    if let Some(t) = TableId::all().into_iter().find(|t| at_role_spot(*t, x, z)) {
        return Some(t);
    }
    TableId::all()
        .into_iter()
        .filter(|t| can_reach(*t, x, z))
        .map(|t| {
            let (c, h) = table_box(t);
            (t, to_box(x, z, c, h))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(t, _)| t)
}

// ---------- Limits ----------

/// Blackjack bets: even dollars, from the minimum to the table maximum.
pub const BLACKJACK_MIN: i64 = 10;
pub const BLACKJACK_MAX: i64 = 100;
/// Roulette: per bet on the layout.
pub const ROULETTE_MIN: i64 = 1;
pub const ROULETTE_MAX: i64 = 100;
/// Bets on the layout per player per spin.
pub const ROULETTE_BETS_PER_PLAYER: usize = 8;
pub const SLOT_MIN: i64 = 1;
pub const SLOT_MAX: i64 = 10;

/// A player's personal maximum: Courage and worse bet 1.5 times the table's.
pub fn max_bet(table_max: i64, tier: Tier) -> i64 {
    (table_max as f32 * drunk::max_bet_multiplier(tier)) as i64
}

/// Is `amount` a blackjack bet a player of `tier` may make?
pub fn blackjack_bet_ok(amount: i64, tier: Tier) -> bool {
    amount % 2 == 0 && (BLACKJACK_MIN..=max_bet(BLACKJACK_MAX, tier)).contains(&amount)
}

/// The bet buttons a client shows: (key label, amount). A Wasted player's
/// buttons shuffle once a second (`second` seeds the order).
pub fn bet_buttons(amounts: &[i64], tier: Tier, second: u64) -> Vec<i64> {
    let mut out = amounts.to_vec();
    if tier >= Tier::Wasted && out.len() > 1 {
        // A small deterministic shuffle: rotate and swap by the second.
        let n = out.len();
        out.rotate_left((second as usize * 7 + 3) % n);
        if second % 2 == 1 {
            out.swap(0, n - 1);
        }
    }
    out
}

// ---------- Customers ----------

/// A customer leaves after losing this share of their starting cash...
pub const WALK_AWAY_LOSS_PERCENT: i64 = 70;
/// ... or after winning 150% on top of it.
pub const WALK_AWAY_WIN_PERCENT: i64 = 150;
/// Seconds a customer waits for a dealer or croupier before leaving.
pub const TABLE_PATIENCE_SECS: u32 = 30;
/// Seconds a player has to act on a hand before it stands for them.
pub const PLAYER_TURN_SECS: u32 = 20;

/// Should a customer with `cash` who came in with `start` walk away?
pub fn walks_away(cash: i64, start: i64) -> bool {
    cash * 100 <= start * (100 - WALK_AWAY_LOSS_PERCENT) || cash * 100 >= start * (100 + WALK_AWAY_WIN_PERCENT)
}

/// A customer's blackjack bet: about an eighth of what they came with.
pub fn customer_blackjack_bet(start: i64, cash: i64) -> Option<i64> {
    let bet = (start / 8 / 2 * 2).clamp(BLACKJACK_MIN, BLACKJACK_MAX);
    let bet = bet.min(cash / 2 * 2);
    (bet >= BLACKJACK_MIN).then_some(bet)
}

/// A customer's roulette chip: about a tenth of what they came with.
pub fn customer_roulette_bet(start: i64, cash: i64) -> Option<i64> {
    let bet = (start / 10).clamp(5, 50).min(cash);
    (bet >= ROULETTE_MIN).then_some(bet)
}

/// A customer's slot bet.
pub fn customer_slot_bet(start: i64, cash: i64) -> Option<i64> {
    let bet = (start / 40).clamp(SLOT_MIN, 5).min(cash);
    (bet >= SLOT_MIN).then_some(bet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_are_distinct() {
        let mut ids: Vec<u32> = TableId::all().iter().map(|t| t.stream().0).collect();
        ids.push(crate::rng::ROOM_STREAM.0);
        ids.push(1); // player effects
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn spots_are_on_the_right_sides() {
        let (dx, dz) = role_spot(TableId::Blackjack).unwrap();
        assert!(dz < BLACKJACK.1 && at_role_spot(TableId::Blackjack, dx, dz));
        for t in TableId::all() {
            for (x, z) in bettor_spots(t) {
                assert!(can_reach(t, x, z), "{t:?} spot {x},{z}");
                assert_eq!(nearest_table(x, z), Some(t));
                // Clear of every block, so players and customers can stand there.
                assert!(
                    crate::bar::BLOCKS.iter().all(|b| (x - b.cx).abs() > b.hx + 0.2 || (z - b.cz).abs() > b.hz + 0.2),
                    "{t:?} spot {x},{z} is inside a block"
                );
            }
        }
        assert_eq!(bettor_spots(TableId::Blackjack).len(), 5);
        assert_eq!(nearest_table(0.0, 4.0), None);
    }

    #[test]
    fn courage_raises_the_max_bet() {
        assert!(blackjack_bet_ok(100, Tier::Sober));
        assert!(!blackjack_bet_ok(150, Tier::Sober));
        assert!(blackjack_bet_ok(150, Tier::Courage));
        assert!(!blackjack_bet_ok(15, Tier::Courage), "odd");
        assert!(!blackjack_bet_ok(8, Tier::Sober), "under the minimum");
    }

    #[test]
    fn wasted_bet_buttons_shuffle() {
        let amounts = [10, 20, 50, 100];
        assert_eq!(bet_buttons(&amounts, Tier::Sloppy, 5), amounts.to_vec());
        let a = bet_buttons(&amounts, Tier::Wasted, 1);
        let b = bet_buttons(&amounts, Tier::Wasted, 2);
        assert_ne!(a, b);
        assert_ne!(a, amounts.to_vec());
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, amounts.to_vec());
    }

    #[test]
    fn walk_away_thresholds() {
        assert!(!walks_away(100, 100));
        assert!(walks_away(30, 100));
        assert!(!walks_away(31, 100));
        assert!(walks_away(250, 100));
        assert!(!walks_away(249, 100));
    }

    #[test]
    fn customer_bets() {
        assert_eq!(customer_blackjack_bet(400, 400), Some(50));
        assert_eq!(customer_blackjack_bet(40, 40), Some(10));
        assert_eq!(customer_blackjack_bet(40, 9), None);
        assert_eq!(customer_roulette_bet(1000, 1000), Some(50));
        assert_eq!(customer_roulette_bet(40, 3), Some(3));
        assert_eq!(customer_slot_bet(400, 400), Some(5));
        assert_eq!(customer_slot_bet(40, 0), None);
    }
}
