//! Money in a running room: deposits at the office safe, the loan shark's
//! collection, the win and loss screens, and the next run.

use bevy::ecs::query::With;
use bevy::ecs::world::World;
use bots::{LocalRoom, ROUTE_TO_SAFE, Script};
use host::HostConfig;
use host::economy::Preset;
use shared::economy::{Collection, Outcome};
use shared::protocol::{Player, Pocket, RoomState, RunLedger, ShiftClock};
use shared::shift::{Calendar, ShiftPhase, Timings};

fn ledger(world: &mut World) -> RunLedger {
    *world.query_filtered::<&RunLedger, With<RoomState>>().single(world).expect("room ledger")
}

fn clock(world: &mut World) -> ShiftClock {
    *world.query_filtered::<&ShiftClock, With<RoomState>>().single(world).expect("room clock")
}

fn pockets(world: &mut World) -> Vec<i64> {
    world.query_filtered::<&Pocket, With<Player>>().iter(world).map(|p| p.0).collect()
}

fn fast_room(preset: Preset, script: Script) -> LocalRoom {
    // 2 + 9 + 2 + 1 second shifts; the outcome screen shows for 1 second.
    // Customers stay at the bar: gamblers would move the house pool.
    let config = HostConfig {
        timings: Timings::PLAN.scaled_down(60),
        preset,
        tastes: Some(host::customers::BAR_ONLY),
        ..Default::default()
    };
    LocalRoom::with_config(1, config, move |_| script)
}

/// Step until `done` holds or `seconds` of simulated time pass.
fn run_until(room: &mut LocalRoom, seconds: u32, mut done: impl FnMut(&mut LocalRoom) -> bool) -> bool {
    for _ in 0..seconds * shared::TICK_HZ {
        room.step();
        if done(room) {
            return true;
        }
    }
    false
}

#[test]
fn a_bot_deposits_its_pocket_at_the_safe() {
    let mut room = fast_room(Preset::LastWeek, Script::Route { route: ROUTE_TO_SAFE });
    let house0 = ledger(room.host.world_mut()).ledger.house;
    // The bot walks in real time, so step with real-time pacing.
    let mut done = false;
    for _ in 0..(64 * 30) {
        room.step();
        std::thread::sleep(shared::TICK);
        if pockets(room.host.world_mut()) == [0] {
            done = true;
            break;
        }
    }
    let l = ledger(room.host.world_mut());
    let pos = room.host_players();
    assert!(done, "pocket not emptied; pockets {:?}, bot at {pos:?}", pockets(room.host.world_mut()));
    assert_eq!(l.ledger.house - house0, 300, "three deposits of 100");
}

#[test]
fn the_last_payment_wins_and_a_new_run_starts() {
    let mut room = fast_room(Preset::LastWeek, Script::Idle);
    let start = ledger(room.host.world_mut());
    assert_eq!(start.due, 40_000);
    assert_eq!(clock(room.host.world_mut()).calendar, Calendar { week: 6, shift: 0 });

    // Three shifts of 14 seconds, then the collection.
    assert!(run_until(&mut room, 60, |r| ledger(r.host.world_mut()).outcome != Outcome::Playing));
    let won = ledger(room.host.world_mut());
    assert_eq!(won.outcome, Outcome::Won);
    assert_eq!(won.last, Some(Collection::Paid { amount: 40_000 }));
    assert_eq!(won.ledger.paid, 120_000);
    assert_eq!(won.ledger.house, 5_000);
    // The clock stops while the win screen shows (from the next tick: the
    // collection runs after the clock).
    room.step();
    let frozen = clock(room.host.world_mut());
    assert!(!frozen.running);

    // Then a new run: week 1, new game plus 1, no money carried over.
    assert!(run_until(&mut room, 5, |r| ledger(r.host.world_mut()).outcome == Outcome::Playing));
    let next = ledger(room.host.world_mut());
    assert_eq!(next.ledger.ng, 1);
    assert_eq!((next.ledger.house, next.ledger.paid), (0, 0));
    assert_eq!(next.due, 10_000, "week 1 payment at +25%");
    assert_eq!(pockets(room.host.world_mut()), [0]);
    run_until(&mut room, 1, |_| false);
    let c = clock(room.host.world_mut());
    assert_eq!((c.calendar, c.phase, c.running), (Calendar::default(), ShiftPhase::Setup, true));
}

#[test]
fn a_second_missed_payment_loses() {
    let mut room = fast_room(Preset::Broke, Script::Idle);
    assert!(run_until(&mut room, 60, |r| ledger(r.host.world_mut()).outcome != Outcome::Playing));
    let lost = ledger(room.host.world_mut());
    assert_eq!(lost.outcome, Outcome::Lost);
    assert_eq!(lost.last, Some(Collection::Missed { owed: 16_000 }));
    assert_eq!(lost.ledger.missed_in_a_row, 2);
}
