//! The casino tables on the host, driven by local players: dealing,
//! betting, payouts, commissions, rakes, slot pulls, customers at the tables,
//! and the RNG audit log replaying every outcome.

use bevy::prelude::*;
use host::economy::Preset;
use host::{HostConfig, HostSim};
use shared::blackjack::Action;
use shared::casino::{self, TableAction, TableId};
use shared::customers::Mood;
use shared::protocol::*;
use shared::roulette::Bet as RBet;
use shared::shift::Timings;

fn sim(timings: Timings) -> HostSim {
    HostSim::with_config(HostConfig { preset: Preset::Casino, timings, seed: [7; 32], tastes: None })
}

/// Shifts that stay in Setup (no customers) for a long time.
fn quiet() -> Timings {
    Timings { setup: 10_000, ..Timings::PLAN }
}

fn put(sim: &mut HostSim, e: Entity, (x, z): (f32, f32)) {
    sim.world_mut().get_mut::<PlayerPos>(e).unwrap().0 = Vec3::new(x, 0.0, z);
}

fn ask(sim: &mut HostSim, e: Entity, table: TableId, action: TableAction) {
    sim.table_request(e, TableRequest { table, action });
}

fn pocket(sim: &HostSim, e: Entity) -> i64 {
    sim.world().get::<Pocket>(e).unwrap().0
}

fn house(sim: &mut HostSim) -> i64 {
    let w = sim.world_mut();
    w.query::<&RunLedger>().single(w).unwrap().ledger.house
}

fn bj(sim: &mut HostSim) -> BlackjackView {
    let w = sim.world_mut();
    w.query::<&BlackjackView>().single(w).unwrap().clone()
}

fn wheel(sim: &mut HostSim) -> RouletteView {
    let w = sim.world_mut();
    w.query::<&RouletteView>().single(w).unwrap().clone()
}

fn chips(sim: &mut HostSim) -> i64 {
    let w = sim.world_mut();
    w.query::<&ChipValue>().iter(w).map(|c| c.0).sum()
}

fn steps(sim: &mut HostSim, n: u32) {
    for _ in 0..n {
        sim.tick();
    }
}

/// Play one blackjack round: everyone stands, the dealer follows the rules.
fn play_round(sim: &mut HostSim, dealer: Entity, bettors: &[Entity]) {
    let ids: Vec<u64> = bettors.iter().map(|e| sim.world().get::<Player>(*e).unwrap().id).collect();
    let rounds = bj(sim).rounds;
    ask(sim, dealer, TableId::Blackjack, TableAction::Deal);
    for _ in 0..2000 {
        sim.tick();
        let v = bj(sim);
        if v.rounds > rounds {
            return;
        }
        match v.phase {
            BjPhase::Insurance => {
                for (e, id) in bettors.iter().zip(&ids) {
                    if v.seats
                        .iter()
                        .any(|s| s.who == Some(shared::minigame::Who::Player(*id)) && s.insurance.is_none())
                    {
                        ask(sim, *e, TableId::Blackjack, TableAction::Insure(false));
                    }
                }
            }
            BjPhase::Players { seat, .. } => {
                if let Some(i) =
                    ids.iter().position(|id| v.seats[usize::from(seat)].who == Some(shared::minigame::Who::Player(*id)))
                {
                    ask(sim, bettors[i], TableId::Blackjack, TableAction::Play(Action::Stand));
                }
            }
            BjPhase::Dealer => {
                if let Some(a) = v.dealer_should {
                    ask(sim, dealer, TableId::Blackjack, TableAction::Dealer(a));
                }
            }
            BjPhase::Betting => {}
        }
    }
    panic!("the round did not finish");
}

#[test]
fn a_dealer_deals_and_the_money_adds_up() {
    let mut sim = sim(quiet());
    let dealer = sim.add_local_player(1, "dealer", 0);
    let a = sim.add_local_player(2, "a", 1);
    let b = sim.add_local_player(3, "b", 2);
    steps(&mut sim, 2);
    put(&mut sim, dealer, casino::role_spot(TableId::Blackjack).unwrap());
    let spots = casino::bettor_spots(TableId::Blackjack);
    put(&mut sim, a, spots[1]);
    put(&mut sim, b, spots[3]);
    steps(&mut sim, 2);
    let total = |sim: &mut HostSim| house(sim) + pocket(sim, dealer) + pocket(sim, a) + pocket(sim, b) + chips(sim);
    let before = total(&mut sim);

    ask(&mut sim, a, TableId::Blackjack, TableAction::TakeRole);
    ask(&mut sim, dealer, TableId::Blackjack, TableAction::TakeRole);
    ask(&mut sim, a, TableId::Blackjack, TableAction::Bet(20));
    ask(&mut sim, b, TableId::Blackjack, TableAction::Bet(15));
    steps(&mut sim, 1);
    let v = bj(&mut sim);
    assert_eq!(v.dealer, Some(1), "the player at the dealer spot deals; the bettor cannot");
    assert_eq!(v.seats[1].bet, 20, "a sits at the nearest seat");
    assert!(v.seats[3].who.is_none(), "an odd bet is refused");
    ask(&mut sim, b, TableId::Blackjack, TableAction::Bet(150));
    steps(&mut sim, 1);
    assert!(bj(&mut sim).seats[3].who.is_none(), "over the sober maximum");
    ask(&mut sim, b, TableId::Blackjack, TableAction::Bet(40));
    ask(&mut sim, a, TableId::Blackjack, TableAction::Deal);
    steps(&mut sim, 1);
    assert_eq!(bj(&mut sim).rounds, 0, "only the dealer deals");

    for _ in 0..10 {
        play_round(&mut sim, dealer, &[a, b]);
        assert_eq!(total(&mut sim), before, "money is neither made nor lost");
        let v = bj(&mut sim);
        assert!(v.seats[1].last.is_some() && v.seats[3].last.is_some());
    }
    assert!(pocket(&sim, dealer) >= 1_000, "the dealer only ever gains commission");
    assert_eq!(bj(&mut sim).rounds, 10);
}

#[test]
fn winnings_wait_on_the_felt_until_picked_up() {
    let mut sim = sim(quiet());
    let dealer = sim.add_local_player(1, "dealer", 0);
    let a = sim.add_local_player(2, "a", 1);
    steps(&mut sim, 2);
    put(&mut sim, dealer, casino::role_spot(TableId::Blackjack).unwrap());
    put(&mut sim, a, casino::bettor_spots(TableId::Blackjack)[2]);
    ask(&mut sim, dealer, TableId::Blackjack, TableAction::TakeRole);
    ask(&mut sim, a, TableId::Blackjack, TableAction::Bet(10));
    steps(&mut sim, 1);
    let mut won = false;
    for _ in 0..40 {
        play_round(&mut sim, dealer, &[a]);
        if chips(&mut sim) > 0 {
            won = true;
            break;
        }
    }
    assert!(won, "a win in 40 rounds");
    let on_felt = chips(&mut sim);
    let before = pocket(&sim, a);
    // Face the table (yaw 0 faces -Z) and press E.
    for b in [0, shared::movement::buttons::INTERACT, 0] {
        sim.set_input(a, PlayerInput::new(Vec2::ZERO, 0.0, -0.5, b));
        steps(&mut sim, 4);
    }
    assert_eq!(pocket(&sim, a), before + on_felt, "picked up");
    assert_eq!(chips(&mut sim), 0);
}

#[test]
fn a_wasted_player_cannot_deal() {
    let mut sim = sim(quiet());
    let p = sim.add_local_player(1, "p", 0);
    steps(&mut sim, 2);
    put(&mut sim, p, casino::role_spot(TableId::Blackjack).unwrap());
    sim.world_mut().get_mut::<Drunk>(p).unwrap().level = 80;
    ask(&mut sim, p, TableId::Blackjack, TableAction::TakeRole);
    steps(&mut sim, 1);
    assert_eq!(bj(&mut sim).dealer, None);
    sim.world_mut().get_mut::<Drunk>(p).unwrap().level = 30;
    ask(&mut sim, p, TableId::Blackjack, TableAction::TakeRole);
    steps(&mut sim, 1);
    assert_eq!(bj(&mut sim).dealer, Some(1));
    // Getting Wasted on the job loses the table.
    sim.world_mut().get_mut::<Drunk>(p).unwrap().level = 75;
    steps(&mut sim, 1);
    assert_eq!(bj(&mut sim).dealer, None);
}

#[test]
fn courage_bets_more() {
    let mut sim = sim(quiet());
    let p = sim.add_local_player(1, "p", 0);
    steps(&mut sim, 2);
    put(&mut sim, p, casino::bettor_spots(TableId::Blackjack)[0]);
    ask(&mut sim, p, TableId::Blackjack, TableAction::Bet(150));
    steps(&mut sim, 1);
    assert!(bj(&mut sim).seats[0].who.is_none());
    sim.world_mut().get_mut::<Drunk>(p).unwrap().level = 30;
    ask(&mut sim, p, TableId::Blackjack, TableAction::Bet(150));
    steps(&mut sim, 1);
    assert_eq!(bj(&mut sim).seats[0].bet, 150);
}

#[test]
fn roulette_spins_pays_and_waits_for_the_rake() {
    let mut sim = sim(quiet());
    let croupier = sim.add_local_player(1, "c", 0);
    let a = sim.add_local_player(2, "a", 1);
    steps(&mut sim, 2);
    put(&mut sim, croupier, casino::role_spot(TableId::Roulette).unwrap());
    put(&mut sim, a, casino::bettor_spots(TableId::Roulette)[2]);
    ask(&mut sim, croupier, TableId::Roulette, TableAction::TakeRole);
    steps(&mut sim, 1);
    let total = |sim: &mut HostSim| house(sim) + pocket(sim, croupier) + pocket(sim, a) + chips(sim);
    let before = total(&mut sim);

    // Cover everything but one dozen so some bets lose and some win.
    ask(&mut sim, a, TableId::Roulette, TableAction::RouletteBet(RBet::Red, 10));
    ask(&mut sim, a, TableId::Roulette, TableAction::RouletteBet(RBet::Black, 10));
    ask(&mut sim, a, TableId::Roulette, TableAction::RouletteBet(RBet::Straight(17), 5));
    ask(&mut sim, a, TableId::Roulette, TableAction::RouletteBet(RBet::Split(3, 4), 5));
    steps(&mut sim, 1);
    assert_eq!(wheel(&mut sim).bets.len(), 3, "the bad split is refused");
    assert_eq!(pocket(&sim, a), 975);
    ask(&mut sim, a, TableId::Roulette, TableAction::Spin);
    steps(&mut sim, 1);
    assert!(!wheel(&mut sim).spinning, "only the croupier spins");
    ask(&mut sim, croupier, TableId::Roulette, TableAction::Spin);
    steps(&mut sim, 1);
    let v = wheel(&mut sim);
    assert!(v.spinning);
    let result = v.result.expect("the result is known when the spin starts");
    ask(&mut sim, a, TableId::Roulette, TableAction::RouletteBet(RBet::Red, 10));
    steps(&mut sim, 6 * 64);
    let v = wheel(&mut sim);
    assert!(!v.spinning);
    assert_eq!(v.last, Some(result));
    assert!(v.bets.is_empty(), "no bets taken while spinning; the table is cleared");
    assert_eq!(total(&mut sim), before);
    let losers = [RBet::Red, RBet::Black, RBet::Straight(17)].iter().filter(|b| !b.covers(result)).count();
    steps(&mut sim, 32);
    assert_eq!(usize::from(wheel(&mut sim).to_rake), losers);

    // No spin until the layout is raked.
    ask(&mut sim, a, TableId::Roulette, TableAction::RouletteBet(RBet::Odd, 10));
    steps(&mut sim, 1);
    ask(&mut sim, croupier, TableId::Roulette, TableAction::Spin);
    steps(&mut sim, 1);
    assert!(!wheel(&mut sim).spinning, "losing chips still on the layout");
    for _ in 0..8 {
        ask(&mut sim, croupier, TableId::Roulette, TableAction::Rake);
        steps(&mut sim, 16);
    }
    assert_eq!(wheel(&mut sim).to_rake, 0, "raked off");
    ask(&mut sim, croupier, TableId::Roulette, TableAction::Spin);
    steps(&mut sim, 1);
    assert!(wheel(&mut sim).spinning);
}

#[test]
fn a_slot_pull_pays_into_the_pocket() {
    let mut sim = sim(quiet());
    let p = sim.add_local_player(1, "p", 0);
    steps(&mut sim, 2);
    put(&mut sim, p, casino::bettor_spots(TableId::Slot(0))[0]);
    let before = pocket(&sim, p) + house(&mut sim);
    let mut paid = 0;
    for _ in 0..20 {
        ask(&mut sim, p, TableId::Slot(0), TableAction::Pull(5));
        steps(&mut sim, 2 * 64 + 2);
        let w = sim.world_mut();
        let v = w.query::<&SlotView>().iter(w).find(|v| v.machine == 0).unwrap().clone();
        assert!(!v.spinning);
        assert_eq!(v.last_return, shared::slots::payout(5, v.stops));
        paid += v.last_return;
    }
    assert_eq!(pocket(&sim, p) + house(&mut sim), before, "the machine only moves money");
    assert_eq!(pocket(&sim, p), 1_000 - 100 + paid);
    ask(&mut sim, p, TableId::Slot(1), TableAction::Pull(5));
    steps(&mut sim, 1);
    assert_eq!(pocket(&sim, p), 1_000 - 100 + paid, "too far from machine 1");
}

/// Customers come in, sit at the tables and play. A dealer keeps the
/// blackjack customers; nobody runs roulette, so its customers give up.
#[test]
fn customers_gamble_and_leave() {
    let timings = Timings { setup: 1, open: 400, last_call: 30, payment: 1, outcome: 1, wave: 30 };
    let mut sim = sim(timings);
    let dealer = sim.add_local_player(1, "dealer", 0);
    steps(&mut sim, 2);
    put(&mut sim, dealer, casino::role_spot(TableId::Blackjack).unwrap());
    ask(&mut sim, dealer, TableId::Blackjack, TableAction::TakeRole);
    let house0 = house(&mut sim);
    let mut gambling = std::collections::BTreeSet::new();
    let mut slot_pulls = 0;
    let mut left_roulette = false;
    for t in 0..(150 * 64) {
        sim.tick();
        if t % 16 == 0 {
            let v = bj(&mut sim);
            if v.phase == BjPhase::Betting && v.seats.iter().any(|s| s.bet > 0) {
                ask(&mut sim, dealer, TableId::Blackjack, TableAction::Deal);
            }
            if let Some(a) = v.dealer_should {
                ask(&mut sim, dealer, TableId::Blackjack, TableAction::Dealer(a));
            }
        }
        let w = sim.world_mut();
        for (c, npc) in w.query::<(&Customer, &host::customers::Npc)>().iter(w) {
            if c.mood == Mood::Gambling {
                gambling.insert(format!("{:?}", npc.activity));
            }
            if c.mood == Mood::Leaving && matches!(npc.activity, host::customers::Activity::Table(TableId::Roulette, _))
            {
                left_roulette = true;
            }
        }
        slot_pulls = w.query::<&SlotView>().iter(w).map(|v| v.pulls).sum::<u32>();
    }
    let v = bj(&mut sim);
    println!("gambling at {gambling:?}; {} blackjack rounds, {slot_pulls} slot pulls", v.rounds);
    assert!(gambling.iter().any(|a| a.contains("Blackjack")), "{gambling:?}");
    assert!(gambling.iter().any(|a| a.contains("Roulette")), "{gambling:?}");
    assert!(gambling.iter().any(|a| a.contains("Slot")), "{gambling:?}");
    assert!(v.rounds >= 5, "customers played blackjack: {} rounds", v.rounds);
    assert!(slot_pulls >= 5, "customers played slots");
    assert!(left_roulette, "no croupier: roulette customers ran out of patience");
    assert_ne!(house(&mut sim), house0, "the house took or paid money");
}

/// Every outcome in a busy room's audit log re-derives from its draws.
#[test]
fn the_audit_log_replays() {
    let timings = Timings { setup: 1, open: 400, last_call: 30, payment: 1, outcome: 1, wave: 20 };
    let mut sim = sim(timings);
    let dealer = sim.add_local_player(1, "dealer", 0);
    let croupier = sim.add_local_player(2, "croupier", 1);
    let gambler = sim.add_local_player(3, "gambler", 2);
    steps(&mut sim, 2);
    put(&mut sim, dealer, casino::role_spot(TableId::Blackjack).unwrap());
    put(&mut sim, croupier, casino::role_spot(TableId::Roulette).unwrap());
    put(&mut sim, gambler, casino::bettor_spots(TableId::Slot(1))[0]);
    ask(&mut sim, dealer, TableId::Blackjack, TableAction::TakeRole);
    ask(&mut sim, croupier, TableId::Roulette, TableAction::TakeRole);
    let mut lines: Vec<String> = Vec::new();
    for t in 0..(120 * 64) {
        sim.tick();
        if t % 16 == 0 {
            let v = bj(&mut sim);
            if v.phase == BjPhase::Betting && v.seats.iter().any(|s| s.bet > 0) {
                ask(&mut sim, dealer, TableId::Blackjack, TableAction::Deal);
            }
            if let Some(a) = v.dealer_should {
                ask(&mut sim, dealer, TableId::Blackjack, TableAction::Dealer(a));
            }
            let r = wheel(&mut sim);
            if r.to_rake > 0 {
                ask(&mut sim, croupier, TableId::Roulette, TableAction::Rake);
            } else if !r.spinning && !r.bets.is_empty() {
                ask(&mut sim, croupier, TableId::Roulette, TableAction::Spin);
            }
            ask(&mut sim, gambler, TableId::Slot(1), TableAction::Pull(2));
        }
        lines.extend(sim.drain_audit().iter().map(|e| e.to_line()));
    }
    let report = shared::audit::verify(lines.iter().map(String::as_str)).expect("the log replays");
    println!("{report:?}");
    assert!(report.shuffles >= 1);
    assert!(report.spins >= 3, "{report:?}");
    assert!(report.reels >= 20, "{report:?}");
    assert!(report.streams >= 4, "room, blackjack, roulette and a slot: {report:?}");

    // A tampered line is caught.
    let i = lines.iter().position(|l| l.contains(r#""reels""#)).unwrap();
    lines[i] = lines[i].replacen("\"stops\":[", "\"stops\":[1", 1);
    assert!(shared::audit::verify(lines.iter().map(String::as_str)).is_err());
}
