//! Blackjack rules (plan section 5.1).
//!
//! 6-deck shoe, reshuffle at 75%. Dealer stands on soft 17 and peeks for
//! blackjack under an ace or a ten. Blackjack pays 3 to 2. Double on any first
//! two cards (not after a split). Split once. No surrender. Insurance when the
//! dealer shows an ace, paying 2 to 1.
//!
//! A [`Round`] is a pure state machine: the host feeds it the actions of
//! players, customers and the dealer, and it refuses anything the rules do
//! not allow. Bets are even dollar amounts, so 3 to 2 and half-bet insurance
//! stay whole.

use serde::{Deserialize, Serialize};

use crate::cards::{self, Card, Shoe};
use crate::rng::Draw;

/// Seats at a table.
pub const SEATS: usize = 5;
/// Per-decision chance that a customer plays off the strategy table.
pub const CUSTOMER_MISTAKE_PERCENT: u32 = 15;
/// The dealer's commission on house wins at their table.
pub const DEALER_COMMISSION_PERCENT: i64 = 10;

/// Total and softness of a hand: soft when an ace counts 11.
pub fn hand_value(cards: &[Card]) -> (u8, bool) {
    let hard: u8 = cards.iter().map(|c| cards::points(*c)).sum();
    let has_ace = cards.iter().any(|c| cards::rank(*c) == 1);
    if has_ace && hard + 10 <= 21 { (hard + 10, true) } else { (hard, false) }
}

/// The dealer draws below 17 and stands on every 17, soft ones included.
pub fn dealer_hits(cards: &[Card]) -> bool {
    hand_value(cards).0 < 17
}

/// Two cards worth 21.
pub fn is_natural(cards: &[Card]) -> bool {
    cards.len() == 2 && hand_value(cards).0 == 21
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Hit,
    Stand,
    Double,
    Split,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hand {
    pub cards: Vec<Card>,
    pub bet: i64,
    pub doubled: bool,
    /// One of the two hands of a split: no blackjack, no double.
    pub from_split: bool,
    pub done: bool,
}

impl Hand {
    pub fn value(&self) -> (u8, bool) {
        hand_value(&self.cards)
    }

    pub fn is_blackjack(&self) -> bool {
        !self.from_split && is_natural(&self.cards)
    }

    pub fn busted(&self) -> bool {
        self.value().0 > 21
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub hands: Vec<Hand>,
    /// The insurance bet; `None` until the seat decides.
    pub insurance: Option<i64>,
    /// Everything this seat put on the table this round.
    pub staked: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// The dealer shows an ace: every seat takes or declines insurance.
    Insurance,
    /// A seat's hand is to act.
    Players { seat: u8, hand: u8 },
    /// The dealer plays the house hand.
    Dealer,
    /// Settled.
    Done,
}

/// Why an action was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refused {
    NotYourTurn,
    NotAllowed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    /// The dealer's cards. `dealer[1]` is the hole card.
    pub dealer: Vec<Card>,
    pub seats: Vec<Seat>,
    pub phase: Phase,
}

impl Round {
    /// Deal a round: one card to each seat, the dealer's up card, a second
    /// card to each seat, then the hole card. `bets` must be even and positive.
    pub fn deal(bets: &[i64], shoe: &mut Shoe) -> Self {
        let mut seats: Vec<Seat> = bets
            .iter()
            .map(|&bet| Seat {
                hands: vec![Hand { cards: Vec::new(), bet, doubled: false, from_split: false, done: false }],
                insurance: None,
                staked: bet,
            })
            .collect();
        let mut dealer = Vec::new();
        for _ in 0..2 {
            for s in &mut seats {
                s.hands[0].cards.push(shoe.deal());
            }
            dealer.push(shoe.deal());
        }
        for s in &mut seats {
            s.hands[0].done = s.hands[0].is_blackjack();
        }
        let mut round = Self { dealer, seats, phase: Phase::Insurance };
        if cards::rank(round.up_card()) == 1 && !round.seats.is_empty() {
            round.phase = Phase::Insurance;
        } else {
            round.after_insurance();
        }
        round
    }

    pub fn up_card(&self) -> Card {
        self.dealer[0]
    }

    pub fn dealer_blackjack(&self) -> bool {
        is_natural(&self.dealer)
    }

    /// The hand to act now, if any.
    pub fn current(&self) -> Option<(usize, usize)> {
        match self.phase {
            Phase::Players { seat, hand } => Some((usize::from(seat), usize::from(hand))),
            _ => None,
        }
    }

    /// Peek for blackjack, then hand the turn to the first player.
    fn after_insurance(&mut self) {
        let up = cards::points(self.up_card());
        if (up == 1 || up == 10) && self.dealer_blackjack() {
            self.phase = Phase::Done;
            return;
        }
        self.advance();
    }

    /// Move to the first unfinished hand, else to the dealer.
    fn advance(&mut self) {
        for (s, seat) in self.seats.iter().enumerate() {
            for (h, hand) in seat.hands.iter().enumerate() {
                if !hand.done {
                    self.phase = Phase::Players { seat: s as u8, hand: h as u8 };
                    return;
                }
            }
        }
        let live = self.seats.iter().flat_map(|s| &s.hands).any(|h| !h.busted() && !h.is_blackjack());
        self.phase = if live { Phase::Dealer } else { Phase::Done };
    }

    /// Take or decline insurance for `seat` (half the bet).
    pub fn insure(&mut self, seat: usize, take: bool) -> Result<(), Refused> {
        if self.phase != Phase::Insurance {
            return Err(Refused::NotYourTurn);
        }
        let s = self.seats.get_mut(seat).ok_or(Refused::NotAllowed)?;
        if s.insurance.is_some() {
            return Err(Refused::NotAllowed);
        }
        let amount = if take { s.hands[0].bet / 2 } else { 0 };
        s.insurance = Some(amount);
        s.staked += amount;
        if self.seats.iter().all(|s| s.insurance.is_some()) {
            self.after_insurance();
        }
        Ok(())
    }

    /// Actions the rules allow for the hand to act. Money is not checked here.
    pub fn legal(&self) -> Vec<Action> {
        let Some((s, h)) = self.current() else { return Vec::new() };
        let seat = &self.seats[s];
        let hand = &seat.hands[h];
        let mut out = vec![Action::Hit, Action::Stand];
        if hand.cards.len() == 2 && !hand.from_split {
            out.push(Action::Double);
            if seat.hands.len() == 1 && cards::points(hand.cards[0]) == cards::points(hand.cards[1]) {
                out.push(Action::Split);
            }
        }
        out
    }

    /// Extra money an action puts on the table.
    pub fn cost(&self, action: Action) -> i64 {
        match (action, self.current()) {
            (Action::Double | Action::Split, Some((s, h))) => self.seats[s].hands[h].bet,
            _ => 0,
        }
    }

    /// Play `action` for the hand to act (`seat` must be the seat whose turn it is).
    pub fn act(&mut self, seat: usize, action: Action, shoe: &mut Shoe) -> Result<(), Refused> {
        let Some((s, h)) = self.current() else { return Err(Refused::NotYourTurn) };
        if s != seat {
            return Err(Refused::NotYourTurn);
        }
        if !self.legal().contains(&action) {
            return Err(Refused::NotAllowed);
        }
        let st = &mut self.seats[s];
        match action {
            Action::Hit => {
                let hand = &mut st.hands[h];
                hand.cards.push(shoe.deal());
                hand.done = hand.value().0 >= 21;
            }
            Action::Stand => st.hands[h].done = true,
            Action::Double => {
                let hand = &mut st.hands[h];
                st.staked += hand.bet;
                hand.bet *= 2;
                hand.doubled = true;
                hand.cards.push(shoe.deal());
                hand.done = true;
            }
            Action::Split => {
                let first = &mut st.hands[0];
                let bet = first.bet;
                let second_card = first.cards.pop().expect("two cards");
                let aces = cards::rank(second_card) == 1;
                let mut second = Hand { cards: vec![second_card], bet, doubled: false, from_split: true, done: false };
                first.from_split = true;
                st.staked += bet;
                for hand in [&mut st.hands[0], &mut second] {
                    hand.cards.push(shoe.deal());
                    // Split aces get one card each.
                    hand.done = aces || hand.value().0 == 21;
                }
                st.hands.push(second);
            }
        }
        if st.hands[h].done || action == Action::Split {
            self.advance();
        }
        Ok(())
    }

    /// What the dealer must press now (the house plays by the rules).
    pub fn dealer_should(&self) -> Option<Action> {
        (self.phase == Phase::Dealer).then(|| if dealer_hits(&self.dealer) { Action::Hit } else { Action::Stand })
    }

    /// The dealer's press. Only the action the rules call for is accepted.
    pub fn dealer_act(&mut self, action: Action, shoe: &mut Shoe) -> Result<(), Refused> {
        match self.dealer_should() {
            None => Err(Refused::NotYourTurn),
            Some(want) if want != action => Err(Refused::NotAllowed),
            Some(Action::Hit) => {
                self.dealer.push(shoe.deal());
                if hand_value(&self.dealer).0 > 21 {
                    self.phase = Phase::Done;
                }
                Ok(())
            }
            Some(_) => {
                self.phase = Phase::Done;
                Ok(())
            }
        }
    }

    /// Money handed back to each seat (stakes plus winnings). Zero for a seat
    /// that lost everything. Only meaningful once [`Phase::Done`].
    pub fn settle(&self) -> Vec<i64> {
        let dealer_bj = self.dealer_blackjack();
        let (dealer_total, _) = hand_value(&self.dealer);
        self.seats
            .iter()
            .map(|seat| {
                let hands: i64 = seat
                    .hands
                    .iter()
                    .map(|hand| {
                        let (total, _) = hand.value();
                        if hand.is_blackjack() {
                            if dealer_bj { hand.bet } else { hand.bet + hand.bet * 3 / 2 }
                        } else if dealer_bj || total > 21 {
                            0
                        } else if dealer_total > 21 || total > dealer_total {
                            hand.bet * 2
                        } else if total == dealer_total {
                            hand.bet
                        } else {
                            0
                        }
                    })
                    .sum();
                let insurance = seat.insurance.unwrap_or(0);
                hands + if dealer_bj { insurance * 3 } else { 0 }
            })
            .collect()
    }
}

/// Basic strategy for this game (6 decks, dealer stands on soft 17, no double
/// after a split, no surrender). Picks from `legal`.
pub fn basic_strategy(hand: &Hand, up: Card, legal: &[Action]) -> Action {
    let d = match cards::points(up) {
        1 => 11,
        p => p,
    };
    let can = |a: Action| legal.contains(&a);
    let (total, soft) = hand.value();
    let pair =
        (hand.cards.len() == 2 && can(Action::Split) && cards::points(hand.cards[0]) == cards::points(hand.cards[1]))
            .then(|| cards::points(hand.cards[0]));
    if let Some(p) = pair {
        let split = match p {
            1 | 8 => true,
            9 => matches!(d, 2..=6 | 8 | 9),
            7 => (2..=7).contains(&d),
            6 => (3..=6).contains(&d),
            2 | 3 => (4..=7).contains(&d),
            _ => false,
        };
        if split {
            return Action::Split;
        }
    }
    let double_or = |fallback: Action| if can(Action::Double) { Action::Double } else { fallback };
    if soft {
        return match total {
            19.. => Action::Stand,
            18 => match d {
                3..=6 => double_or(Action::Stand),
                2 | 7 | 8 => Action::Stand,
                _ => Action::Hit,
            },
            17 if (3..=6).contains(&d) => double_or(Action::Hit),
            15 | 16 if (4..=6).contains(&d) => double_or(Action::Hit),
            13 | 14 if (5..=6).contains(&d) => double_or(Action::Hit),
            _ => Action::Hit,
        };
    }
    match total {
        17.. => Action::Stand,
        13..=16 if d <= 6 => Action::Stand,
        12 if (4..=6).contains(&d) => Action::Stand,
        11 if d <= 10 => double_or(Action::Hit),
        10 if d <= 9 => double_or(Action::Hit),
        9 if (3..=6).contains(&d) => double_or(Action::Hit),
        _ => Action::Hit,
    }
}

/// A customer's play: the strategy table, but `mistake_percent` of decisions
/// go to another legal action picked at random.
pub fn customer_choice(round: &Round, mistake_percent: u32, d: &mut impl Draw) -> Action {
    let legal = round.legal();
    let (s, h) = round.current().expect("a hand to act");
    let right = basic_strategy(&round.seats[s].hands[h], round.up_card(), &legal);
    if d.chance(mistake_percent) {
        let others: Vec<Action> = legal.into_iter().filter(|a| *a != right).collect();
        if !others.is_empty() {
            return others[d.below(others.len() as u32) as usize];
        }
    }
    right
}

/// A customer's insurance call: basic strategy never insures; a mistake does.
pub fn customer_insures(mistake_percent: u32, d: &mut impl Draw) -> bool {
    d.chance(mistake_percent)
}

/// Total staked and total handed back over `hands` one-seat rounds of $2,
/// played by the strategy table with `mistake_percent` mistakes. House edge
/// is `1 - returned / staked` measured against the initial bets.
pub fn simulate(hands: u32, mistake_percent: u32, d: &mut impl Draw) -> (i64, i64) {
    const BET: i64 = 2;
    let mut shoe = Shoe::shuffled(d);
    let (mut initial, mut net) = (0i64, 0i64);
    for _ in 0..hands {
        if shoe.needs_shuffle() {
            shoe = Shoe::shuffled(d);
        }
        let mut round = Round::deal(&[BET], &mut shoe);
        if round.phase == Phase::Insurance {
            let take = customer_insures(mistake_percent, d);
            round.insure(0, take).expect("insurance");
        }
        while round.current().is_some() {
            let a = customer_choice(&round, mistake_percent, d);
            round.act(0, a, &mut shoe).expect("legal");
        }
        while let Some(a) = round.dealer_should() {
            round.dealer_act(a, &mut shoe).expect("dealer");
        }
        initial += BET;
        net += round.settle()[0] - round.seats[0].staked;
    }
    (initial, initial + net)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    // Card helpers: rank 1 to 13 in spades.
    fn c(rank: u8) -> Card {
        rank - 1
    }

    fn hand(ranks: &[u8]) -> Hand {
        Hand { cards: ranks.iter().map(|r| c(*r)).collect(), bet: 2, doubled: false, from_split: false, done: false }
    }

    #[test]
    fn hand_values() {
        assert_eq!(hand_value(&[c(1), c(13)]), (21, true));
        assert_eq!(hand_value(&[c(1), c(6)]), (17, true));
        assert_eq!(hand_value(&[c(1), c(6), c(10)]), (17, false));
        assert_eq!(hand_value(&[c(1), c(1), c(9)]), (21, true));
        assert_eq!(hand_value(&[c(10), c(12), c(2)]), (22, false));
        assert!(is_natural(&[c(11), c(1)]));
        assert!(!is_natural(&[c(5), c(6), c(10)]));
    }

    #[test]
    fn the_dealer_stands_on_soft_17() {
        assert!(!dealer_hits(&[c(1), c(6)]));
        assert!(dealer_hits(&[c(10), c(6)]));
        assert!(!dealer_hits(&[c(10), c(7)]));
        assert!(dealer_hits(&[c(1), c(5)]));
    }

    /// Seat cards are dealt first, then the dealer's: seat1, up, seat2, hole.
    fn round(seat: [u8; 2], dealer: [u8; 2], rest: &[u8]) -> (Round, Shoe) {
        let mut order = vec![c(seat[0]), c(dealer[0]), c(seat[1]), c(dealer[1])];
        order.extend(rest.iter().map(|r| c(*r)));
        let mut shoe = Shoe::stacked(order);
        (Round::deal(&[10], &mut shoe), shoe)
    }

    #[test]
    fn blackjack_pays_three_to_two() {
        let (r, _) = round([1, 13], [9, 8], &[]);
        assert_eq!(r.phase, Phase::Done, "nothing to play");
        assert_eq!(r.settle(), vec![25]);
    }

    #[test]
    fn the_dealer_peeks_and_takes_only_the_bet() {
        let (r, _) = round([10, 9], [10, 1], &[]);
        assert_eq!(r.phase, Phase::Done);
        assert_eq!(r.settle(), vec![0]);
        // Blackjack against blackjack pushes.
        let (r, _) = round([1, 10], [13, 1], &[]);
        assert_eq!(r.settle(), vec![10]);
    }

    #[test]
    fn insurance_pays_two_to_one() {
        let (mut r, _) = round([10, 9], [1, 13], &[]);
        assert_eq!(r.phase, Phase::Insurance);
        r.insure(0, true).unwrap();
        assert_eq!(r.phase, Phase::Done);
        assert_eq!(r.seats[0].staked, 15);
        // The hand loses 10, the insurance of 5 returns 15: even.
        assert_eq!(r.settle(), vec![15]);
        let (mut r, _) = round([10, 9], [1, 7], &[]);
        r.insure(0, true).unwrap();
        assert_eq!(r.phase, Phase::Players { seat: 0, hand: 0 });
        assert_eq!(r.insure(0, true), Err(Refused::NotYourTurn));
    }

    #[test]
    fn hit_stand_and_the_dealer_plays_by_the_rules() {
        let (mut r, mut shoe) = round([10, 2], [10, 6], &[5, 5]);
        assert_eq!(r.legal(), vec![Action::Hit, Action::Stand, Action::Double]);
        r.act(0, Action::Hit, &mut shoe).unwrap();
        assert_eq!(r.seats[0].hands[0].value().0, 17);
        r.act(0, Action::Stand, &mut shoe).unwrap();
        assert_eq!(r.phase, Phase::Dealer);
        assert_eq!(r.dealer_should(), Some(Action::Hit));
        assert_eq!(r.dealer_act(Action::Stand, &mut shoe), Err(Refused::NotAllowed), "16 must hit");
        r.dealer_act(Action::Hit, &mut shoe).unwrap();
        assert_eq!(hand_value(&r.dealer).0, 21);
        assert_eq!(r.dealer_should(), Some(Action::Stand));
        r.dealer_act(Action::Stand, &mut shoe).unwrap();
        assert_eq!(r.settle(), vec![0]);
    }

    #[test]
    fn a_bust_ends_the_hand_and_the_dealer_does_not_draw() {
        let (mut r, mut shoe) = round([10, 6], [10, 7], &[9]);
        r.act(0, Action::Hit, &mut shoe).unwrap();
        assert!(r.seats[0].hands[0].busted());
        assert_eq!(r.phase, Phase::Done);
        assert_eq!(r.settle(), vec![0]);
    }

    #[test]
    fn double_takes_one_card_for_twice_the_bet() {
        let (mut r, mut shoe) = round([6, 5], [10, 7], &[10]);
        r.act(0, Action::Double, &mut shoe).unwrap();
        assert_eq!(r.seats[0].staked, 20);
        assert_eq!(r.phase, Phase::Dealer);
        r.dealer_act(Action::Stand, &mut shoe).unwrap();
        assert_eq!(r.settle(), vec![40]);
    }

    #[test]
    fn split_once_and_split_aces_get_one_card() {
        let (mut r, mut shoe) = round([8, 8], [10, 7], &[3, 8, 10]);
        assert!(r.legal().contains(&Action::Split));
        assert_eq!(r.cost(Action::Split), 10);
        r.act(0, Action::Split, &mut shoe).unwrap();
        assert_eq!(r.seats[0].hands.len(), 2);
        assert_eq!(r.seats[0].staked, 20);
        // Second 8 drew another 8, but there is no second split and no double.
        assert_eq!(r.legal(), vec![Action::Hit, Action::Stand]);
        r.act(0, Action::Stand, &mut shoe).unwrap(); // 8+3 = 11, stands (silly but legal)
        r.act(0, Action::Hit, &mut shoe).unwrap(); // 8+8+10 = 26
        assert_eq!(r.phase, Phase::Dealer);

        let (mut r, mut shoe) = round([1, 1], [10, 7], &[13, 9]);
        r.act(0, Action::Split, &mut shoe).unwrap();
        assert!(r.seats[0].hands.iter().all(|h| h.done));
        // A split ace and a ten is 21, not blackjack: it pays even money.
        assert!(!r.seats[0].hands[0].is_blackjack());
        assert_eq!(r.phase, Phase::Dealer);
        r.dealer_act(Action::Stand, &mut shoe).unwrap();
        assert_eq!(r.settle(), vec![40]);
    }

    #[test]
    fn turns_are_enforced() {
        let mut shoe = Shoe::stacked((0..20).map(|i| c(2 + i % 5)).collect());
        let mut r = Round::deal(&[2, 2], &mut shoe);
        assert_eq!(r.current(), Some((0, 0)));
        assert_eq!(r.act(1, Action::Stand, &mut shoe), Err(Refused::NotYourTurn));
        r.act(0, Action::Stand, &mut shoe).unwrap();
        assert_eq!(r.current(), Some((1, 0)));
        assert_eq!(r.dealer_act(Action::Hit, &mut shoe), Err(Refused::NotYourTurn));
    }

    #[test]
    fn strategy_spot_checks() {
        let all = [Action::Hit, Action::Stand, Action::Double, Action::Split];
        let two = [Action::Hit, Action::Stand];
        assert_eq!(basic_strategy(&hand(&[10, 6]), c(10), &all), Action::Hit);
        assert_eq!(basic_strategy(&hand(&[10, 6]), c(6), &all), Action::Stand);
        assert_eq!(basic_strategy(&hand(&[10, 2]), c(3), &all), Action::Hit);
        assert_eq!(basic_strategy(&hand(&[6, 5]), c(10), &all), Action::Double);
        assert_eq!(basic_strategy(&hand(&[6, 5]), c(1), &all), Action::Hit);
        assert_eq!(basic_strategy(&hand(&[8, 8]), c(1), &all), Action::Split);
        assert_eq!(basic_strategy(&hand(&[10, 10]), c(6), &all), Action::Stand);
        assert_eq!(basic_strategy(&hand(&[1, 7]), c(4), &all), Action::Double);
        assert_eq!(basic_strategy(&hand(&[1, 7]), c(4), &two), Action::Stand);
        assert_eq!(basic_strategy(&hand(&[1, 7]), c(9), &all), Action::Hit);
        assert_eq!(basic_strategy(&hand(&[5, 5]), c(9), &all), Action::Double);
        assert_eq!(basic_strategy(&hand(&[9, 9]), c(7), &all), Action::Stand);
    }

    #[test]
    fn a_customer_sometimes_misplays() {
        let (r, _) = round([10, 6], [10, 7], &[]);
        let mut d = SimRng::new(4);
        let n = 2000;
        let wrong = (0..n).filter(|_| customer_choice(&r, 15, &mut d) != Action::Hit).count();
        let rate = wrong as f64 / f64::from(n);
        assert!((0.12..0.18).contains(&rate), "{rate}");
        assert!((0..100).all(|_| customer_choice(&r, 0, &mut d) == Action::Hit));
    }

    /// Plan section 10, Phase 3: house edge against perfect basic strategy
    /// between 0.4% and 0.9%. A million hands (the plan asks for at least
    /// 100,000) keeps the measurement's standard error near 0.11%.
    #[test]
    fn house_edge_against_basic_strategy() {
        let (staked, returned) = simulate(1_000_000, 0, &mut SimRng::new(0x5eed_b1ac));
        let edge = 1.0 - returned as f64 / staked as f64;
        println!("blackjack edge vs basic strategy: {:.3}%", edge * 100.0);
        assert!((0.004..=0.009).contains(&edge), "edge {:.3}%", edge * 100.0);
    }

    /// Reported, not gated: the edge against the 15%-mistake customer.
    #[test]
    fn house_edge_against_customers_is_reported() {
        let (staked, returned) = simulate(100_000, CUSTOMER_MISTAKE_PERCENT, &mut SimRng::new(0x00c0_ffee));
        let edge = 1.0 - returned as f64 / staked as f64;
        println!("blackjack edge vs customers (15% mistakes): {:.2}%", edge * 100.0);
        assert!(edge > 0.0);
    }
}
