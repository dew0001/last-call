//! Playing cards and the 6-deck blackjack shoe (plan section 5.1).
//!
//! A card is a `u8` in `0..52`: rank is `card % 13` (0 is an ace, 12 a king),
//! suit is `card / 13`.

use serde::{Deserialize, Serialize};

use crate::rng::Draw;

pub type Card = u8;

/// Decks in the shoe.
pub const DECKS: usize = 6;
/// Cards in the shoe.
pub const SHOE_CARDS: usize = DECKS * 52;
/// Reshuffle once this share of the shoe is dealt (75% penetration).
pub const PENETRATION: usize = SHOE_CARDS * 3 / 4;

/// Rank 1 (ace) to 13 (king).
pub fn rank(card: Card) -> u8 {
    card % 13 + 1
}

/// Blackjack points: aces count 1 here, face cards 10.
pub fn points(card: Card) -> u8 {
    rank(card).min(10)
}

/// Suit 0 to 3 (spades, hearts, diamonds, clubs).
pub fn suit(card: Card) -> u8 {
    card / 13 % 4
}

/// Short name, such as "A♠" or "10♥".
pub fn name(card: Card) -> String {
    let r = match rank(card) {
        1 => "A".to_string(),
        11 => "J".to_string(),
        12 => "Q".to_string(),
        13 => "K".to_string(),
        n => n.to_string(),
    };
    let s = ['♠', '♥', '♦', '♣'][usize::from(suit(card))];
    format!("{r}{s}")
}

/// A Fisher-Yates shuffle of `decks` full decks. Draws exactly one value per
/// card after the first (plus rejections), so the replay tool can re-derive it.
pub fn shuffle(decks: usize, d: &mut impl Draw) -> Vec<Card> {
    let mut cards: Vec<Card> = (0..decks * 52).map(|i| (i % 52) as Card).collect();
    for i in (1..cards.len()).rev() {
        let j = d.below(i as u32 + 1) as usize;
        cards.swap(i, j);
    }
    cards
}

/// The dealing shoe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shoe {
    pub cards: Vec<Card>,
    pub next: usize,
}

impl Shoe {
    /// A freshly shuffled 6-deck shoe.
    pub fn shuffled(d: &mut impl Draw) -> Self {
        Self { cards: shuffle(DECKS, d), next: 0 }
    }

    /// A shoe that deals `cards` in order (tests).
    pub fn stacked(cards: Vec<Card>) -> Self {
        Self { cards, next: 0 }
    }

    /// Deal the next card. An exhausted shoe starts over from its top (only
    /// reachable with a stacked test shoe; a real one reshuffles first).
    pub fn deal(&mut self) -> Card {
        if self.next >= self.cards.len() {
            self.next = 0;
        }
        let c = self.cards[self.next];
        self.next += 1;
        c
    }

    /// True once the cut card is reached: shuffle before the next round.
    pub fn needs_shuffle(&self) -> bool {
        self.next >= PENETRATION.min(self.cards.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Recorded, SimRng};

    #[test]
    fn ranks_points_and_names() {
        assert_eq!(rank(0), 1);
        assert_eq!(points(0), 1);
        assert_eq!(points(9), 10);
        assert_eq!(points(12), 10);
        assert_eq!(points(13 + 4), 5);
        assert_eq!(name(0), "A♠");
        assert_eq!(name(13 + 9), "10♥");
        assert_eq!(name(51), "K♣");
    }

    #[test]
    fn a_shuffle_keeps_every_card() {
        let mut d = SimRng::new(1);
        let mut cards = shuffle(DECKS, &mut d);
        assert_eq!(cards.len(), 312);
        cards.sort_unstable();
        for (i, c) in cards.iter().enumerate() {
            assert_eq!(usize::from(*c), i / DECKS);
        }
    }

    #[test]
    fn a_shuffle_is_rederived_from_its_draws() {
        struct Tape(SimRng, Vec<u32>);
        impl Draw for Tape {
            fn next(&mut self) -> u32 {
                let v = self.0.next();
                self.1.push(v);
                v
            }
        }
        let mut tape = Tape(SimRng::new(9), Vec::new());
        let first = shuffle(DECKS, &mut tape);
        let mut again = Recorded::new(&tape.1);
        assert_eq!(shuffle(DECKS, &mut again), first);
        assert_eq!(again.left(), 0);
    }

    #[test]
    fn the_cut_card_is_at_three_quarters() {
        let mut shoe = Shoe::shuffled(&mut SimRng::new(2));
        for _ in 0..233 {
            shoe.deal();
        }
        assert!(!shoe.needs_shuffle());
        shoe.deal();
        assert!(shoe.needs_shuffle());
    }
}
