//! Testing rule (plan section 11): 8 bot clients play the bar for 1,000
//! ticks over the in-memory transport, with no panics, and every 100 ticks
//! each client's replicated game state matches the host's.
//!
//! Clients see the host's state a few ticks late, and an update can reach a
//! client in pieces, so at each checkpoint a client's hash must equal one of
//! the host's recent hashes within one second of play.

use std::collections::VecDeque;

use bots::{LocalRoom, Script};
use host::HostConfig;
use host::economy::Preset;
use shared::shift::Timings;
use shared::state::replicated_hash;

/// Both tests run bots in real time; run in parallel they starve each other
/// of CPU, so take turns.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn eight_bots_play_the_bar_and_every_client_agrees_with_the_host() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // Customers from the first second; a wave every 5 s; $300 pockets so
    // the drinkers can pay.
    let timings = Timings { setup: 1, open: 600, last_call: 30, payment: 1, outcome: 1, wave: 5 };
    let config = HostConfig { timings, preset: Preset::LastWeek, ..Default::default() };
    let mut room = LocalRoom::with_config(8, config, |i| match i {
        0 | 1 => Script::Pour { hold: 148, pitch: -0.4, drink: true },
        2 => Script::Pour { hold: 400, pitch: 0.4, drink: false },
        3 => Script::GrabAndThrow,
        _ => Script::Circle { phase: f32::from(i) },
    });

    let mut history: VecDeque<u64> = VecDeque::new();
    let step = |room: &mut LocalRoom, history: &mut VecDeque<u64>| {
        room.step();
        std::thread::sleep(shared::TICK);
        history.push_back(replicated_hash(room.host.world_mut()));
        if history.len() > 128 {
            history.pop_front();
        }
    };

    let mut checks = 0;
    for tick in 1..=1000u32 {
        step(&mut room, &mut history);
        if tick % 100 != 0 {
            continue;
        }
        for bot in 0..room.bots.len() {
            let mut agreed = false;
            for _ in 0..64 {
                let client = replicated_hash(room.bots[bot].world_mut());
                if history.contains(&client) {
                    agreed = true;
                    break;
                }
                step(&mut room, &mut history);
            }
            assert!(agreed, "tick {tick}: bot {bot}'s state never matched the host's within a second");
            checks += 1;
        }
    }
    assert_eq!(checks, 80);
    // The game actually happened: customers came, beers were poured.
    let world = room.host.world_mut();
    let customers = world.query::<&shared::protocol::Customer>().iter(world).count();
    let drinkers = world.query::<&shared::protocol::Drunk>().iter(world).filter(|d| d.level > 0).count();
    println!("customers in the bar: {customers}; players who drank: {drinkers}");
    assert!(customers > 0);
}

/// The same rule for the casino: a dealer, a croupier and six players at
/// blackjack, roulette and the slots, with customers at every table.
#[test]
fn eight_bots_play_the_casino_and_every_client_agrees_with_the_host() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    use shared::casino::TableId;
    let timings = Timings { setup: 1, open: 600, last_call: 30, payment: 1, outcome: 1, wave: 5 };
    // No chaos: this test is about the tables staying in sync (chaos has its own tests).
    let config = HostConfig { timings, preset: Preset::Casino, manual_chaos: true, ..Default::default() };
    let mut room = LocalRoom::with_config(8, config, |i| match i {
        0 => Script::Casino { table: TableId::Blackjack, role: true, spot: 0 },
        1 => Script::Casino { table: TableId::Blackjack, role: false, spot: 1 },
        2 => Script::Casino { table: TableId::Blackjack, role: false, spot: 3 },
        3 => Script::Casino { table: TableId::Roulette, role: true, spot: 0 },
        4 => Script::Casino { table: TableId::Roulette, role: false, spot: 1 },
        5 => Script::Casino { table: TableId::Roulette, role: false, spot: 4 },
        6 => Script::Casino { table: TableId::Slot(0), role: false, spot: 0 },
        _ => Script::Casino { table: TableId::Slot(1), role: false, spot: 0 },
    });

    let mut history: VecDeque<u64> = VecDeque::new();
    let step = |room: &mut LocalRoom, history: &mut VecDeque<u64>| {
        room.step();
        std::thread::sleep(shared::TICK);
        history.push_back(replicated_hash(room.host.world_mut()));
        if history.len() > 128 {
            history.pop_front();
        }
    };

    let mut checks = 0;
    for tick in 1..=1000u32 {
        step(&mut room, &mut history);
        if tick % 100 != 0 {
            continue;
        }
        for bot in 0..room.bots.len() {
            let mut agreed = false;
            for _ in 0..64 {
                let client = replicated_hash(room.bots[bot].world_mut());
                if history.contains(&client) {
                    agreed = true;
                    break;
                }
                step(&mut room, &mut history);
            }
            assert!(agreed, "tick {tick}: bot {bot}'s state never matched the host's within a second");
            checks += 1;
        }
    }
    assert_eq!(checks, 80);
    let world = room.host.world_mut();
    let bj = world.query::<&shared::protocol::BlackjackView>().single(world).unwrap().clone();
    let wheel = world.query::<&shared::protocol::RouletteView>().single(world).unwrap().clone();
    let pulls: u32 = world.query::<&shared::protocol::SlotView>().iter(world).map(|v| v.pulls).sum();
    println!("{} blackjack rounds, {} spins, {pulls} slot pulls", bj.rounds, wheel.spins);
    assert!(bj.dealer.is_some() && wheel.croupier.is_some(), "both tables staffed");
    assert!(bj.rounds >= 2 && wheel.spins >= 1 && pulls >= 10);
}
