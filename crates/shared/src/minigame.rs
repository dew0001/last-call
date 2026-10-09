//! The contract every minigame follows (plan section 5).
//!
//! The host runs the state machine; clients render the state and send
//! inputs; customers are host-side bots that call [`Minigame::apply`] with
//! scripted inputs. One change from the plan's sketch: `apply` takes the
//! table's RNG instead of a tick, because blackjack reshuffles and customers
//! decide inside `apply`. The caller's RNG logs each draw with its tick.

use serde::{Deserialize, Serialize};

use crate::blackjack::{self, Action, Round};
use crate::cards::Shoe;
use crate::rng::Draw;
use crate::{roulette, slots};

/// Who placed a bet: a player (by id) or a customer (by id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Who {
    Player(u64),
    Customer(u32),
}

/// A stake on a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bet<S> {
    pub who: Who,
    pub amount: i64,
    pub selection: S,
}

/// Money handed back to a bettor: stakes plus winnings (0 for a loss).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payout {
    pub who: Who,
    pub staked: i64,
    pub returned: i64,
}

/// Why an input was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refused {
    NotYourTurn,
    NotAllowed,
}

impl From<blackjack::Refused> for Refused {
    fn from(r: blackjack::Refused) -> Self {
        match r {
            blackjack::Refused::NotYourTurn => Refused::NotYourTurn,
            blackjack::Refused::NotAllowed => Refused::NotAllowed,
        }
    }
}

pub trait Minigame {
    type Params;
    type Input;
    type State;
    type Outcome;
    type Selection;
    fn start(rng: &mut impl Draw, params: &Self::Params) -> Self::State;
    fn apply(
        state: &mut Self::State,
        who: Who,
        input: Self::Input,
        rng: &mut impl Draw,
    ) -> Result<Option<Self::Outcome>, Refused>;
    fn payout(outcome: &Self::Outcome, bets: &[Bet<Self::Selection>]) -> Vec<Payout>;
}

// ---------- Roulette ----------

pub struct Roulette;

/// One spin: the result is fixed when the spin starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spin {
    pub result: u8,
    pub settled: bool,
}

/// The ball dropped: `result` is final.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouletteOutcome {
    pub result: u8,
}

impl Minigame for Roulette {
    type Params = ();
    /// The spin timer ran out: drop the ball.
    type Input = ();
    type State = Spin;
    type Outcome = RouletteOutcome;
    type Selection = roulette::Bet;

    fn start(rng: &mut impl Draw, _: &()) -> Spin {
        Spin { result: roulette::spin(rng), settled: false }
    }

    fn apply(state: &mut Spin, _: Who, _: (), _: &mut impl Draw) -> Result<Option<RouletteOutcome>, Refused> {
        if state.settled {
            return Err(Refused::NotAllowed);
        }
        state.settled = true;
        Ok(Some(RouletteOutcome { result: state.result }))
    }

    fn payout(outcome: &RouletteOutcome, bets: &[Bet<roulette::Bet>]) -> Vec<Payout> {
        bets.iter()
            .map(|b| Payout {
                who: b.who,
                staked: b.amount,
                returned: roulette::payout(b.selection, b.amount, outcome.result),
            })
            .collect()
    }
}

// ---------- Slots ----------

pub struct Slots;

impl Minigame for Slots {
    type Params = ();
    /// The reels finished their animation.
    type Input = ();
    type State = [u8; 3];
    type Outcome = [u8; 3];
    type Selection = ();

    fn start(rng: &mut impl Draw, _: &()) -> [u8; 3] {
        slots::pull(rng)
    }

    fn apply(state: &mut [u8; 3], _: Who, _: (), _: &mut impl Draw) -> Result<Option<[u8; 3]>, Refused> {
        Ok(Some(*state))
    }

    fn payout(stops: &[u8; 3], bets: &[Bet<()>]) -> Vec<Payout> {
        bets.iter()
            .map(|b| Payout { who: b.who, staked: b.amount, returned: slots::payout(b.amount, *stops) })
            .collect()
    }
}

// ---------- Blackjack ----------

pub struct Blackjack;

/// A table: the shoe and the round in play.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackjackTable {
    pub shoe: Shoe,
    pub round: Option<Round>,
    /// Who sits in each seat of the current round, in seat order.
    pub seated: Vec<Who>,
    /// Shuffles so far (for the UI and the audit log).
    pub shuffles: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlackjackInput {
    /// The dealer deals a round to these bets (seat order).
    Deal(Vec<Bet<()>>),
    Insure(bool),
    Play(Action),
    /// The dealer's press for the house hand.
    Dealer(Action),
}

impl Minigame for Blackjack {
    type Params = ();
    type Input = BlackjackInput;
    type State = BlackjackTable;
    /// The finished round.
    type Outcome = Round;
    type Selection = ();

    fn start(rng: &mut impl Draw, _: &()) -> BlackjackTable {
        BlackjackTable { shoe: Shoe::shuffled(rng), round: None, seated: Vec::new(), shuffles: 1 }
    }

    fn apply(
        t: &mut BlackjackTable,
        who: Who,
        input: BlackjackInput,
        rng: &mut impl Draw,
    ) -> Result<Option<Round>, Refused> {
        match input {
            BlackjackInput::Deal(bets) => {
                if t.round.is_some() || bets.is_empty() || bets.iter().any(|b| b.amount <= 0 || b.amount % 2 != 0) {
                    return Err(Refused::NotAllowed);
                }
                if t.shoe.needs_shuffle() {
                    t.shoe = Shoe::shuffled(rng);
                    t.shuffles += 1;
                }
                t.seated = bets.iter().map(|b| b.who).collect();
                let amounts: Vec<i64> = bets.iter().map(|b| b.amount).collect();
                t.round = Some(Round::deal(&amounts, &mut t.shoe));
            }
            BlackjackInput::Insure(take) => {
                let seat = t.seated.iter().position(|w| *w == who).ok_or(Refused::NotYourTurn)?;
                t.round.as_mut().ok_or(Refused::NotYourTurn)?.insure(seat, take)?;
            }
            BlackjackInput::Play(action) => {
                let round = t.round.as_mut().ok_or(Refused::NotYourTurn)?;
                let (seat, _) = round.current().ok_or(Refused::NotYourTurn)?;
                if t.seated.get(seat) != Some(&who) {
                    return Err(Refused::NotYourTurn);
                }
                round.act(seat, action, &mut t.shoe)?;
            }
            BlackjackInput::Dealer(action) => {
                t.round.as_mut().ok_or(Refused::NotYourTurn)?.dealer_act(action, &mut t.shoe)?;
            }
        }
        if t.round.as_ref().is_some_and(|r| r.phase == blackjack::Phase::Done) {
            return Ok(t.round.take());
        }
        Ok(None)
    }

    fn payout(round: &Round, bets: &[Bet<()>]) -> Vec<Payout> {
        round
            .settle()
            .into_iter()
            .zip(&round.seats)
            .zip(bets)
            .map(|((returned, seat), bet)| Payout { who: bet.who, staked: seat.staked, returned })
            .collect()
    }
}

/// The house's net from a set of payouts, and the commission owed to the
/// dealer or croupier (`percent` of a positive net).
pub fn house_take(payouts: &[Payout], percent: i64) -> (i64, i64) {
    let net: i64 = payouts.iter().map(|p| p.staked - p.returned).sum();
    let commission = if net > 0 { net * percent / 100 } else { 0 };
    (net, commission)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn roulette_settles_once() {
        let mut d = SimRng::new(3);
        let mut spin = Roulette::start(&mut d, &());
        let out = Roulette::apply(&mut spin, Who::Player(1), (), &mut d).unwrap().unwrap();
        assert_eq!(out.result, spin.result);
        assert_eq!(Roulette::apply(&mut spin, Who::Player(1), (), &mut d), Err(Refused::NotAllowed));
        let bets = [Bet { who: Who::Customer(1), amount: 10, selection: roulette::Bet::Straight(out.result) }];
        assert_eq!(Roulette::payout(&out, &bets)[0].returned, 360);
    }

    #[test]
    fn slots_pay_from_the_stops() {
        let stops = [9, 8, 11];
        let bets = [Bet { who: Who::Player(7), amount: 2, selection: () }];
        assert_eq!(Slots::payout(&stops, &bets)[0].returned, 300);
    }

    #[test]
    fn a_blackjack_round_through_the_trait() {
        let mut d = SimRng::new(11);
        let mut t = Blackjack::start(&mut d, &());
        let bets = vec![
            Bet { who: Who::Player(1), amount: 10, selection: () },
            Bet { who: Who::Customer(2), amount: 4, selection: () },
        ];
        assert_eq!(
            Blackjack::apply(&mut t, Who::Player(1), BlackjackInput::Deal(vec![]), &mut d),
            Err(Refused::NotAllowed)
        );
        let odd = vec![Bet { who: Who::Player(1), amount: 5, selection: () }];
        assert_eq!(
            Blackjack::apply(&mut t, Who::Player(1), BlackjackInput::Deal(odd), &mut d),
            Err(Refused::NotAllowed)
        );
        let mut outcome = Blackjack::apply(&mut t, Who::Player(9), BlackjackInput::Deal(bets.clone()), &mut d).unwrap();
        let mut guard = 0;
        while outcome.is_none() {
            guard += 1;
            assert!(guard < 50);
            let round = t.round.as_ref().unwrap();
            let (who, input) = match round.phase {
                blackjack::Phase::Insurance => {
                    let s = round.seats.iter().position(|s| s.insurance.is_none()).unwrap();
                    (t.seated[s], BlackjackInput::Insure(false))
                }
                blackjack::Phase::Players { seat, .. } => {
                    // The wrong seat is refused.
                    let other = t.seated[1 - usize::from(seat)];
                    assert_eq!(
                        Blackjack::apply(&mut t, other, BlackjackInput::Play(Action::Stand), &mut d),
                        Err(Refused::NotYourTurn)
                    );
                    (t.seated[usize::from(seat)], BlackjackInput::Play(Action::Stand))
                }
                blackjack::Phase::Dealer => (Who::Player(9), BlackjackInput::Dealer(round.dealer_should().unwrap())),
                blackjack::Phase::Done => unreachable!(),
            };
            outcome = Blackjack::apply(&mut t, who, input, &mut d).unwrap();
        }
        let payouts = Blackjack::payout(&outcome.unwrap(), &bets);
        assert_eq!(payouts.len(), 2);
        assert!(t.round.is_none(), "ready for the next deal");
        let (net, commission) = house_take(&payouts, 10);
        assert_eq!(net, payouts.iter().map(|p| p.staked - p.returned).sum::<i64>());
        assert_eq!(commission, if net > 0 { net / 10 } else { 0 });
    }

    #[test]
    fn commission_is_ten_percent_of_house_wins_only() {
        let p = |staked, returned| Payout { who: Who::Customer(1), staked, returned };
        assert_eq!(house_take(&[p(100, 0), p(50, 100)], 10), (50, 5));
        assert_eq!(house_take(&[p(100, 200)], 10), (-100, 0));
    }
}
