//! Money rules (plan section 4.4): the house pool, the loan shark's payment
//! schedule, debt tiers, and how a run is won or lost.
//!
//! Amounts are whole dollars in `i64`.

use serde::{Deserialize, Serialize};

/// What the crew owes at the start of a first run.
pub const DEBT: i64 = 120_000;

/// The loan shark's cut per week, weeks 1 to 6. Sums to [`DEBT`].
pub const PAYMENTS: [i64; 6] = [8_000, 11_000, 15_000, 20_000, 26_000, 40_000];

/// Paid-so-far thresholds for debt tiers 0 to 6 (plan section 4.5).
pub const TIER_THRESHOLDS: [i64; 7] = [0, 8_000, 19_000, 34_000, 54_000, 80_000, 120_000];

/// Dollars moved from a pocket to the house pool per press at the office safe.
pub const SAFE_DEPOSIT: i64 = 100;

/// Missed payments in a row that end the run.
pub const MISSES_TO_LOSE: u8 = 2;

/// The scheduled payment for a week of a first run (`0` after week 6).
pub fn next_payment(week: u8) -> i64 {
    match week {
        1..=6 => PAYMENTS[usize::from(week - 1)],
        _ => 0,
    }
}

/// Scale an amount for new game plus level `ng`: +25% per level.
pub fn scale_for_ng(amount: i64, ng: u8) -> i64 {
    amount * (100 + 25 * i64::from(ng)) / 100
}

/// Customers' patience in new game plus: 10% less per level, at least 60%.
pub fn ng_patience_percent(ng: u8) -> u32 {
    100u32.saturating_sub(10 * u32::from(ng)).max(60)
}

/// Debt tier for an amount paid so far, scaled for new game plus.
pub fn tier(paid: i64, ng: u8) -> u8 {
    TIER_THRESHOLDS.iter().rposition(|&t| paid >= scale_for_ng(t, ng)).unwrap_or(0) as u8
}

/// How a run stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Outcome {
    #[default]
    Playing,
    /// The debt is paid: the crew owns the bar.
    Won,
    /// Two payments missed in a row: the enforcers burned the bar down.
    Lost,
}

/// Result of one collection at the end of a week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Collection {
    Paid {
        amount: i64,
    },
    /// The house pool could not cover it. Nothing is taken; the amount carries
    /// into next week's payment.
    Missed {
        owed: i64,
    },
}

/// The run's money: the house pool and the debt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Ledger {
    /// Shared house pool.
    pub house: i64,
    /// Paid to the loan shark so far this run.
    pub paid: i64,
    /// Missed payments carried into the next one.
    pub carried: i64,
    /// Payments missed in a row.
    pub missed_in_a_row: u8,
    /// New game plus level: 0 for a first run.
    pub ng: u8,
}

impl Ledger {
    /// A fresh run at new game plus level `ng`.
    pub fn new_run(ng: u8) -> Self {
        Self { ng, ..Default::default() }
    }

    /// The whole debt for this run.
    pub fn debt(&self) -> i64 {
        scale_for_ng(DEBT, self.ng)
    }

    /// What is still owed.
    pub fn remaining(&self) -> i64 {
        (self.debt() - self.paid).max(0)
    }

    /// The payment due at the end of `week`. From week 6 on, the whole
    /// remaining balance is due.
    pub fn due(&self, week: u8) -> i64 {
        if week >= 6 {
            self.remaining()
        } else {
            (scale_for_ng(next_payment(week), self.ng) + self.carried).min(self.remaining())
        }
    }

    /// Collect the payment for `week` from the house pool.
    pub fn collect(&mut self, week: u8) -> Collection {
        let due = self.due(week);
        if self.house >= due {
            self.house -= due;
            self.paid += due;
            self.carried = 0;
            self.missed_in_a_row = 0;
            Collection::Paid { amount: due }
        } else {
            self.carried = due;
            self.missed_in_a_row = self.missed_in_a_row.saturating_add(1);
            Collection::Missed { owed: due }
        }
    }

    /// Whether the run is over.
    pub fn outcome(&self) -> Outcome {
        if self.paid >= self.debt() {
            Outcome::Won
        } else if self.missed_in_a_row >= MISSES_TO_LOSE {
            Outcome::Lost
        } else {
            Outcome::Playing
        }
    }

    /// Debt tier for the money paid so far.
    pub fn tier(&self) -> u8 {
        tier(self.paid, self.ng)
    }
}

/// Move money from a pocket into the house pool at the safe: up to
/// [`SAFE_DEPOSIT`], never more than the pocket holds. Returns the amount moved.
pub fn deposit(pocket: &mut i64, ledger: &mut Ledger) -> i64 {
    let amount = (*pocket).clamp(0, SAFE_DEPOSIT);
    *pocket -= amount;
    ledger.house += amount;
    amount
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_game_plus_makes_customers_less_patient() {
        assert_eq!(ng_patience_percent(0), 100);
        assert_eq!(ng_patience_percent(2), 80);
        assert_eq!(ng_patience_percent(9), 60, "never under 60%");
        assert_eq!(scale_for_ng(100, 2), 150, "debts grow 25% a level");
    }
    use proptest::prelude::*;

    #[test]
    fn schedule_sums_to_the_debt() {
        assert_eq!(PAYMENTS.iter().sum::<i64>(), DEBT);
        assert_eq!(next_payment(1), 8_000);
        assert_eq!(next_payment(6), 40_000);
        assert_eq!(next_payment(0), 0);
        assert_eq!(next_payment(7), 0);
    }

    #[test]
    fn tiers_follow_the_table() {
        assert_eq!(tier(0, 0), 0);
        assert_eq!(tier(7_999, 0), 0);
        assert_eq!(tier(8_000, 0), 1);
        assert_eq!(tier(19_000, 0), 2);
        assert_eq!(tier(80_000, 0), 5);
        assert_eq!(tier(120_000, 0), 6);
        // New game plus raises every threshold by 25% per level.
        assert_eq!(tier(8_000, 1), 0);
        assert_eq!(tier(10_000, 1), 1);
    }

    #[test]
    fn paying_every_week_wins_in_week_six() {
        let mut l = Ledger::new_run(0);
        for week in 1..=6 {
            l.house += l.due(week);
            assert_eq!(l.collect(week), Collection::Paid { amount: PAYMENTS[usize::from(week - 1)] });
            let expected = if week == 6 { Outcome::Won } else { Outcome::Playing };
            assert_eq!(l.outcome(), expected, "week {week}");
        }
        assert_eq!(l.house, 0);
        assert_eq!(l.tier(), 6);
    }

    #[test]
    fn a_missed_payment_carries_and_two_in_a_row_lose() {
        let mut l = Ledger::new_run(0);
        l.house = 5_000;
        assert_eq!(l.collect(1), Collection::Missed { owed: 8_000 });
        assert_eq!(l.house, 5_000, "nothing is taken on a miss");
        assert_eq!(l.outcome(), Outcome::Playing);
        assert_eq!(l.due(2), 19_000, "week 1 carries into week 2");
        l.house = 19_000;
        assert_eq!(l.collect(2), Collection::Paid { amount: 19_000 });
        assert_eq!(l.missed_in_a_row, 0);
        l.house = 0;
        l.collect(3);
        l.collect(4);
        assert_eq!(l.outcome(), Outcome::Lost);
    }

    #[test]
    fn week_six_takes_the_whole_remaining_balance() {
        let mut l = Ledger { paid: 50_000, ..Ledger::new_run(0) };
        assert_eq!(l.due(6), 70_000);
        assert_eq!(l.due(9), 70_000);
        l.house = 70_000;
        l.collect(6);
        assert_eq!(l.outcome(), Outcome::Won);
    }

    #[test]
    fn new_game_plus_scales_the_debt() {
        let l = Ledger::new_run(2);
        assert_eq!(l.debt(), 180_000);
        assert_eq!(l.due(1), 12_000);
    }

    #[test]
    fn deposits_move_at_most_the_pocket() {
        let mut l = Ledger::default();
        let mut pocket = 250;
        assert_eq!(deposit(&mut pocket, &mut l), 100);
        assert_eq!(deposit(&mut pocket, &mut l), 100);
        assert_eq!(deposit(&mut pocket, &mut l), 50);
        assert_eq!(deposit(&mut pocket, &mut l), 0);
        assert_eq!((pocket, l.house), (0, 250));
    }

    proptest! {
        #[test]
        fn money_is_conserved(house in 0i64..200_000, paid in 0i64..120_000, week in 1u8..10, ng in 0u8..4) {
            let mut l = Ledger { house, paid, ng, ..Default::default() };
            let before = l.house + l.paid;
            l.collect(week);
            prop_assert_eq!(l.house + l.paid, before);
            prop_assert!(l.house >= 0);
            prop_assert!(l.paid <= l.debt().max(paid));
        }

        #[test]
        fn deposit_conserves_money(pocket in -50i64..1_000, house in 0i64..1_000) {
            let mut l = Ledger { house, ..Default::default() };
            let mut p = pocket;
            let moved = deposit(&mut p, &mut l);
            prop_assert_eq!(p + l.house, pocket + house);
            prop_assert!((0..=SAFE_DEPOSIT).contains(&moved));
            prop_assert!(p >= pocket.min(0));
        }
    }
}
