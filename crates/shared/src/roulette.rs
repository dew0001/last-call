//! European roulette, single zero (plan section 5.2).
//!
//! The layout has 0 and 36 numbers in 12 rows of 3: row `r` (0 to 11) holds
//! `3r + 1`, `3r + 2`, `3r + 3`. Every bet returns `36 / covered` times its
//! stake on a win, so every bet has the same house edge of 1/37 (2.70%).

use serde::{Deserialize, Serialize};

use crate::rng::Draw;

/// Pockets on the wheel, 0 to 36.
pub const POCKETS: u8 = 37;
/// The shortest spin, in seconds.
pub const MIN_SPIN_SECS: u32 = 6;
/// The croupier's commission on house wins at the wheel.
pub const CROUPIER_COMMISSION_PERCENT: i64 = 10;

const REDS: [u8; 18] = [1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36];

/// Pocket order around a European wheel, clockwise from 0 (for drawing).
pub const WHEEL_ORDER: [u8; 37] = [
    0, 32, 15, 19, 4, 21, 2, 25, 17, 34, 6, 27, 13, 36, 11, 30, 8, 23, 10, 5, 24, 16, 33, 1, 20, 14, 31, 9, 22, 18, 29,
    7, 28, 12, 35, 3, 26,
];

pub fn is_red(n: u8) -> bool {
    REDS.contains(&n)
}

pub fn is_black(n: u8) -> bool {
    n != 0 && n <= 36 && !is_red(n)
}

/// What a chip is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bet {
    /// One number, 0 to 36. Pays 35 to 1.
    Straight(u8),
    /// Two neighbours on the layout (`a < b`): side by side in a row, one
    /// above the other, or 0 with 1, 2 or 3. Pays 17 to 1.
    Split(u8, u8),
    /// A row of three, 0 to 11. Pays 11 to 1.
    Street(u8),
    /// Four numbers in a square, named by its smallest. Pays 8 to 1.
    Corner(u8),
    /// Two rows next to each other, named by the first (0 to 10). Pays 5 to 1.
    Line(u8),
    /// A column, 0 to 2 (column 0 holds 1, 4, 7 ...). Pays 2 to 1.
    Column(u8),
    /// A dozen, 0 to 2 (1 to 12, 13 to 24, 25 to 36). Pays 2 to 1.
    Dozen(u8),
    Red,
    Black,
    Odd,
    Even,
    /// 1 to 18.
    Low,
    /// 19 to 36.
    High,
}

impl Bet {
    /// Is this a bet the layout has?
    pub fn is_valid(&self) -> bool {
        match *self {
            Bet::Straight(n) => n <= 36,
            Bet::Split(a, b) => {
                (a == 0 && (1..=3).contains(&b)) || (a >= 1 && b <= 36 && ((b == a + 1 && a % 3 != 0) || b == a + 3))
            }
            Bet::Street(r) => r <= 11,
            Bet::Corner(n) => (1..=32).contains(&n) && n % 3 != 0,
            Bet::Line(r) => r <= 10,
            Bet::Column(c) | Bet::Dozen(c) => c <= 2,
            Bet::Red | Bet::Black | Bet::Odd | Bet::Even | Bet::Low | Bet::High => true,
        }
    }

    /// Does this bet win when the ball lands in `n`?
    pub fn covers(&self, n: u8) -> bool {
        if !self.is_valid() || n > 36 {
            return false;
        }
        let row = |n: u8| (n - 1) / 3;
        match *self {
            Bet::Straight(x) => n == x,
            Bet::Split(a, b) => n == a || n == b,
            Bet::Street(r) => n != 0 && row(n) == r,
            Bet::Corner(x) => [x, x + 1, x + 3, x + 4].contains(&n),
            Bet::Line(r) => n != 0 && (row(n) == r || row(n) == r + 1),
            Bet::Column(c) => n != 0 && (n - 1) % 3 == c,
            Bet::Dozen(d) => n != 0 && (n - 1) / 12 == d,
            Bet::Red => is_red(n),
            Bet::Black => is_black(n),
            Bet::Odd => n != 0 && n % 2 == 1,
            Bet::Even => n != 0 && n.is_multiple_of(2),
            Bet::Low => (1..=18).contains(&n),
            Bet::High => (19..=36).contains(&n),
        }
    }

    /// How many numbers the bet covers.
    pub fn covered(&self) -> u32 {
        (0..POCKETS).filter(|n| self.covers(*n)).count() as u32
    }

    /// Winnings per unit staked: 35, 17, 11, 8, 5, 2 or 1.
    pub fn odds(&self) -> i64 {
        match *self {
            Bet::Straight(_) => 35,
            Bet::Split(..) => 17,
            Bet::Street(_) => 11,
            Bet::Corner(_) => 8,
            Bet::Line(_) => 5,
            Bet::Column(_) | Bet::Dozen(_) => 2,
            _ => 1,
        }
    }

    /// Short label for the UI, such as "17", "Red" or "2nd 12".
    pub fn label(&self) -> String {
        match *self {
            Bet::Straight(n) => n.to_string(),
            Bet::Split(a, b) => format!("{a}/{b}"),
            Bet::Street(r) => format!("Street {}-{}", 3 * r + 1, 3 * r + 3),
            Bet::Corner(n) => format!("Corner {n}"),
            Bet::Line(r) => format!("Line {}-{}", 3 * r + 1, 3 * r + 6),
            Bet::Column(c) => format!("Col {}", c + 1),
            Bet::Dozen(d) => ["1st 12", "2nd 12", "3rd 12"][usize::from(d)].into(),
            Bet::Red => "Red".into(),
            Bet::Black => "Black".into(),
            Bet::Odd => "Odd".into(),
            Bet::Even => "Even".into(),
            Bet::Low => "1-18".into(),
            Bet::High => "19-36".into(),
        }
    }
}

/// Money handed back for `amount` on `bet` when the ball lands in `result`:
/// the stake plus winnings, or 0.
pub fn payout(bet: Bet, amount: i64, result: u8) -> i64 {
    if bet.covers(result) { amount * (bet.odds() + 1) } else { 0 }
}

/// Where the ball lands. Decided the moment the spin starts.
pub fn spin(d: &mut impl Draw) -> u8 {
    d.below(u32::from(POCKETS)) as u8
}

/// Every bet the layout has.
pub fn all_bets() -> Vec<Bet> {
    let mut out: Vec<Bet> = (0..=36).map(Bet::Straight).collect();
    for a in 0..=36u8 {
        for b in a + 1..=36 {
            if Bet::Split(a, b).is_valid() {
                out.push(Bet::Split(a, b));
            }
        }
    }
    out.extend((0..=11).map(Bet::Street));
    out.extend((1..=32).filter(|n| n % 3 != 0).map(Bet::Corner));
    out.extend((0..=10).map(Bet::Line));
    out.extend((0..=2).map(Bet::Column));
    out.extend((0..=2).map(Bet::Dozen));
    out.extend([Bet::Red, Bet::Black, Bet::Odd, Bet::Even, Bet::Low, Bet::High]);
    out
}

/// A customer's pick: mostly even-money and dozen bets, sometimes a number.
pub fn customer_bet(d: &mut impl Draw) -> Bet {
    match d.below(10) {
        0..=3 => [Bet::Red, Bet::Black, Bet::Odd, Bet::Even, Bet::Low, Bet::High][d.below(6) as usize],
        4..=6 => {
            if d.chance(50) {
                Bet::Dozen(d.below(3) as u8)
            } else {
                Bet::Column(d.below(3) as u8)
            }
        }
        7 => Bet::Corner([1, 4, 8, 11, 17, 20, 25, 29][d.below(8) as usize]),
        _ => Bet::Straight(d.below(37) as u8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;
    use proptest::prelude::*;

    #[test]
    fn colours() {
        assert_eq!((1..=36).filter(|n| is_red(*n)).count(), 18);
        assert!(!is_red(0) && !is_black(0));
        assert!(is_red(1) && is_black(2) && is_red(36) && is_black(35));
        let mut wheel = WHEEL_ORDER;
        wheel.sort_unstable();
        assert_eq!(wheel.to_vec(), (0..37).collect::<Vec<u8>>());
    }

    #[test]
    fn bet_sizes_match_their_odds() {
        for bet in all_bets() {
            assert!(bet.is_valid(), "{bet:?}");
            // covered * (odds + 1) == 36 for every bet: the edge is 1/37.
            assert_eq!(bet.covered() as i64 * (bet.odds() + 1), 36, "{bet:?}");
        }
        assert_eq!(all_bets().len(), 37 + 60 + 12 + 22 + 11 + 3 + 3 + 6);
    }

    #[test]
    fn invalid_bets_are_refused() {
        for bad in
            [Bet::Straight(37), Bet::Split(3, 4), Bet::Split(1, 3), Bet::Corner(3), Bet::Corner(33), Bet::Line(11)]
        {
            assert!(!bad.is_valid(), "{bad:?}");
            assert_eq!(payout(bad, 10, 3), 0);
        }
    }

    #[test]
    fn payouts() {
        assert_eq!(payout(Bet::Straight(17), 10, 17), 360);
        assert_eq!(payout(Bet::Straight(17), 10, 18), 0);
        assert_eq!(payout(Bet::Split(0, 2), 10, 0), 180);
        assert_eq!(payout(Bet::Street(0), 10, 2), 120);
        assert_eq!(payout(Bet::Corner(1), 10, 5), 90);
        assert_eq!(payout(Bet::Line(0), 10, 6), 60);
        assert_eq!(payout(Bet::Column(0), 10, 34), 30);
        assert_eq!(payout(Bet::Dozen(2), 10, 25), 30);
        assert_eq!(payout(Bet::Red, 10, 0), 0, "zero loses outside bets");
        assert_eq!(payout(Bet::Even, 10, 0), 0);
        assert_eq!(payout(Bet::Low, 10, 18), 20);
        assert_eq!(payout(Bet::High, 10, 18), 0);
    }

    proptest! {
        /// Over the 37 pockets, every bet hands back exactly 36 stakes.
        #[test]
        fn every_bet_returns_36_of_37(i in 0usize..154, amount in 1i64..10_000) {
            let bet = all_bets()[i];
            let back: i64 = (0..POCKETS).map(|n| payout(bet, amount, n)).sum();
            prop_assert_eq!(back, amount * 36);
        }
    }

    /// Exact edge from the payout table, then 100,000 simulated spins.
    #[test]
    fn house_edge_is_2_7_percent() {
        let bets = all_bets();
        let staked = bets.len() as i64 * i64::from(POCKETS);
        let back: i64 = bets.iter().map(|b| (0..POCKETS).map(|n| payout(*b, 1, n)).sum::<i64>()).sum();
        let exact = 1.0 - back as f64 / staked as f64;
        assert!((exact - 1.0 / 37.0).abs() < 1e-12);
        let mut d = SimRng::new(0x2011);
        let (mut staked, mut back) = (0i64, 0i64);
        for _ in 0..100_000 {
            let bet = customer_bet(&mut d);
            let n = spin(&mut d);
            staked += 10;
            back += payout(bet, 10, n);
        }
        let edge = 1.0 - back as f64 / staked as f64;
        println!("roulette edge: exact {:.3}%, simulated {:.3}%", exact * 100.0, edge * 100.0);
        // Customer bets mix in 35 to 1 numbers: the standard error is near 0.9%.
        assert!((-0.03..0.09).contains(&edge), "{edge}");
    }
}
