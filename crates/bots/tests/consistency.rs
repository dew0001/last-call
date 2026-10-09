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

#[test]
fn eight_bots_play_the_bar_and_every_client_agrees_with_the_host() {
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
