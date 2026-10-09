//! The shift clock: phases run in order with the configured lengths, the week
//! rolls over after three shifts, clients see the host's clock, and an empty
//! room does not lose time.

use bots::{LocalRoom, Script};
use host::{HostConfig, HostSim};
use shared::protocol::{RoomState, ShiftClock};
use shared::shift::{Calendar, ShiftPhase, Timings};

fn host_clock(world: &mut bevy::ecs::world::World) -> ShiftClock {
    let mut q = world.query_filtered::<&ShiftClock, bevy::ecs::query::With<RoomState>>();
    *q.single(world).expect("one room-state entity")
}

#[test]
fn phases_follow_the_timings_and_weeks_roll_over() {
    // 2 + 9 + 2 + 1 seconds per shift.
    let timings = Timings::PLAN.scaled_down(60);
    let mut room = LocalRoom::with_config(1, HostConfig { timings, ..Default::default() }, |_| Script::Idle);

    // Record each phase start: (host tick, calendar, phase).
    let mut starts: Vec<(u64, Calendar, ShiftPhase)> = Vec::new();
    let mut last = None;
    let shift_ticks = u64::from(timings.shift_seconds()) * u64::from(shared::TICK_HZ);
    for _ in 0..(3 * shift_ticks + 64) {
        // Bots update every tick (no sleep: they just keep up with packets).
        room.step();
        let c = host_clock(room.host.world_mut());
        assert!(c.running, "a player is present, so the clock runs");
        if last != Some((c.calendar, c.phase)) {
            starts.push((room.host.tick_count(), c.calendar, c.phase));
            last = Some((c.calendar, c.phase));
        }
    }

    let phases: Vec<_> = starts.iter().map(|s| (s.1.week, s.1.shift, s.2)).collect();
    use ShiftPhase::*;
    let expected = [
        (1, 0, Setup),
        (1, 0, Open),
        (1, 0, LastCall),
        (1, 0, Payment),
        (1, 1, Setup),
        (1, 1, Open),
        (1, 1, LastCall),
        (1, 1, Payment),
        (1, 2, Setup),
        (1, 2, Open),
        (1, 2, LastCall),
        (1, 2, Payment),
        (2, 0, Setup),
    ];
    assert_eq!(phases[..expected.len()], expected);

    // Each phase lasted its configured length (the first Setup started before
    // the first recorded tick, so skip it).
    for pair in starts.windows(2).skip(1) {
        let (t0, _, phase) = pair[0];
        let len = pair[1].0 - t0;
        assert_eq!(len, u64::from(timings.seconds(phase)) * u64::from(shared::TICK_HZ), "{phase:?}");
    }

    // The bot sees the host's clock (within the usual replication delay).
    let mut matched = false;
    for _ in 0..128 {
        room.step();
        std::thread::sleep(shared::TICK);
        let host = host_clock(room.host.world_mut());
        let seen = room.bots[0]
            .world_mut()
            .query_filtered::<&ShiftClock, bevy::ecs::query::With<RoomState>>()
            .iter(room.bots[0].world())
            .next()
            .copied();
        if seen.is_some_and(|s| {
            (s.calendar, s.phase) == (host.calendar, host.phase) && s.seconds_left.abs_diff(host.seconds_left) <= 1
        }) {
            matched = true;
            break;
        }
    }
    assert!(matched, "the bot never showed the host's clock");
}

#[test]
fn an_empty_room_does_not_lose_time() {
    let mut sim = HostSim::with_config(HostConfig { timings: Timings::PLAN.scaled_down(60), ..Default::default() });
    for _ in 0..(64 * 30) {
        sim.tick();
    }
    let c = host_clock(sim.world_mut());
    assert!(!c.running);
    assert_eq!((c.calendar, c.phase, c.seconds_left), (Calendar::default(), ShiftPhase::Setup, 2));
}
