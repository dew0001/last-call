//! Slot machines (plan section 5.3): 3 reels, 5 symbols, 1 payline.
//!
//! Each reel strip has 20 stops. The return to player is fixed by the strips
//! and [`PAYTABLE`]: 7,353 of 8,000 equally likely spins' worth, 91.9%.

use serde::{Deserialize, Serialize};

use crate::rng::Draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Symbol {
    Cherry,
    Lemon,
    Bell,
    Bar,
    Seven,
}

use Symbol::*;

/// Stops per reel.
pub const STOPS: usize = 20;

/// The reel strips: 5 cherries, 6 lemons, 5 bells, 3 bars and 1 seven each,
/// in a different order per reel so the reels do not spin in step.
pub const REELS: [[Symbol; STOPS]; 3] = [
    [
        Cherry, Lemon, Bell, Cherry, Bar, Lemon, Bell, Cherry, Lemon, Seven, Bell, Lemon, Cherry, Bar, Bell, Lemon,
        Cherry, Bell, Bar, Lemon,
    ],
    [
        Lemon, Cherry, Bell, Lemon, Bar, Cherry, Lemon, Bell, Seven, Cherry, Lemon, Bell, Bar, Cherry, Lemon, Bell,
        Cherry, Bar, Bell, Lemon,
    ],
    [
        Bell, Lemon, Cherry, Bar, Lemon, Bell, Cherry, Lemon, Bell, Bar, Cherry, Seven, Lemon, Bell, Cherry, Lemon,
        Bar, Bell, Cherry, Lemon,
    ],
];

/// Credits paid per credit bet, stake included. Checked top to bottom.
pub const PAYTABLE: [(&str, u32); 7] = [
    ("7 7 7", 150),
    ("BAR BAR BAR", 50),
    ("Bell Bell Bell", 10),
    ("Lemon Lemon Lemon", 8),
    ("Cherry Cherry Cherry", 5),
    ("Cherry Cherry any", 2),
    ("Cherry any any", 1),
];

/// The multiplier for a line of three symbols.
pub fn line_pays(line: [Symbol; 3]) -> u32 {
    match line {
        [Seven, Seven, Seven] => 150,
        [Bar, Bar, Bar] => 50,
        [Bell, Bell, Bell] => 10,
        [Lemon, Lemon, Lemon] => 8,
        [Cherry, Cherry, Cherry] => 5,
        [Cherry, Cherry, _] => 2,
        [Cherry, _, _] => 1,
        _ => 0,
    }
}

/// The symbols on the payline for three stops.
pub fn line(stops: [u8; 3]) -> [Symbol; 3] {
    [0, 1, 2].map(|r| REELS[r][usize::from(stops[r]) % STOPS])
}

/// Where the reels stop. Decided when the lever is pulled.
pub fn pull(d: &mut impl Draw) -> [u8; 3] {
    [0; 3].map(|_| d.below(STOPS as u32) as u8)
}

/// Money handed back for `bet` on a spin that stopped at `stops`.
pub fn payout(bet: i64, stops: [u8; 3]) -> i64 {
    bet * i64::from(line_pays(line(stops)))
}

/// The exact return to player over every stop combination.
pub fn rtp() -> f64 {
    let mut back = 0u64;
    for a in 0..STOPS as u8 {
        for b in 0..STOPS as u8 {
            for c in 0..STOPS as u8 {
                back += u64::from(line_pays(line([a, b, c])));
            }
        }
    }
    back as f64 / (STOPS * STOPS * STOPS) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn every_reel_has_the_same_symbol_counts() {
        for reel in REELS {
            let count = |s: Symbol| reel.iter().filter(|x| **x == s).count();
            assert_eq!([count(Cherry), count(Lemon), count(Bell), count(Bar), count(Seven)], [5, 6, 5, 3, 1]);
        }
    }

    #[test]
    fn the_paytable() {
        assert_eq!(line_pays([Seven, Seven, Seven]), 150);
        assert_eq!(line_pays([Cherry, Cherry, Cherry]), 5);
        assert_eq!(line_pays([Cherry, Cherry, Seven]), 2);
        assert_eq!(line_pays([Cherry, Bell, Cherry]), 1);
        assert_eq!(line_pays([Bell, Cherry, Cherry]), 0);
        assert_eq!(line_pays([Bar, Bar, Bell]), 0);
        assert_eq!(payout(3, [9, 8, 11]), 450, "three sevens");
        assert_eq!(PAYTABLE.len(), 7);
    }

    /// Plan section 10, Phase 3: RTP between 91% and 93%.
    #[test]
    fn return_to_player() {
        let exact = rtp();
        assert!((exact - 0.919125).abs() < 1e-9, "{exact}");
        assert!((0.91..=0.93).contains(&exact));
        let mut d = SimRng::new(0x5107);
        let back: i64 = (0..100_000).map(|_| payout(1, pull(&mut d))).sum();
        let simulated = back as f64 / 100_000.0;
        println!("slots RTP: exact {:.3}%, simulated {:.3}%", exact * 100.0, simulated * 100.0);
        // One spin's standard deviation is 3.8 credits: 1.2% over 100,000 spins.
        assert!((simulated - exact).abs() < 0.06, "{simulated}");
    }
}
