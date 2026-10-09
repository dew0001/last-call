//! Customers: a wave walks in, sits on the stools, orders, gives up when
//! nobody serves them, leaves at Last call, and leaves when their stool goes.
//! These rooms send every customer to the bar; `host/tests/casino.rs` covers
//! the tables.

use avian3d::prelude::Position;
use bevy::ecs::world::World;
use bots::{LocalRoom, Script};
use host::HostConfig;
use host::customers::Npc;
use shared::customers::{DOOR, Mood, PATIENCE_SECS};
use shared::protocol::{Customer, NpcPose};
use shared::shift::Timings;

/// One second of Setup, then a long Open with a single wave.
fn timings(open: u32, last_call: u32) -> Timings {
    Timings { setup: 1, open, last_call, payment: 1, outcome: 1, wave: 10_000 }
}

fn room(timings: Timings, seed: u8) -> LocalRoom {
    LocalRoom::with_config(
        1,
        HostConfig { timings, seed: [seed; 32], tastes: Some(host::customers::BAR_ONLY), ..Default::default() },
        |_| Script::Idle,
    )
}

fn customers(world: &mut World) -> Vec<(Customer, NpcPose)> {
    let mut v: Vec<_> = world.query::<(&Customer, &NpcPose)>().iter(world).map(|(c, p)| (*c, *p)).collect();
    v.sort_by_key(|(c, _)| c.id);
    v
}

fn run(room: &mut LocalRoom, seconds: f32) {
    for _ in 0..(seconds * shared::TICK_HZ as f32) as u32 {
        room.step();
    }
}

#[test]
fn a_wave_sits_down_orders_and_leaves_when_nobody_serves() {
    let mut room = room(timings(120, 30), 1);
    run(&mut room, 1.5);
    let entering = customers(room.host.world_mut());
    assert_eq!(entering.len(), 6, "week 1 wave: 4 + 2 * 1");
    assert!(
        entering
            .iter()
            .all(|(c, p)| c.mood == Mood::Entering && p.pos.distance(bevy::math::Vec3::new(DOOR.0, 0.0, DOOR.1)) < 1.0)
    );

    // Within 15 seconds everyone is on a stool, facing the counter, waiting.
    run(&mut room, 15.0);
    let seated = customers(room.host.world_mut());
    for (c, p) in &seated {
        assert_eq!(c.mood, Mood::Waiting, "customer {} at {:?}", c.id, p.pos);
        assert!(p.pos.y > 0.4 && p.pos.z > -3.3 && p.pos.z < -2.0, "seated at the bar: {:?}", p.pos);
        assert!(c.patience > 0 && u32::from(c.patience) <= PATIENCE_SECS);
    }
    // No two on one stool.
    for (i, a) in seated.iter().enumerate() {
        for b in &seated[i + 1..] {
            assert!(a.1.pos.distance(b.1.pos) > 0.5);
        }
    }

    // Nobody serves them: after 20 seconds of waiting they walk out.
    run(&mut room, PATIENCE_SECS as f32);
    assert!(customers(room.host.world_mut()).iter().all(|(c, _)| c.mood == Mood::Leaving));
    run(&mut room, 15.0);
    assert!(customers(room.host.world_mut()).is_empty(), "everyone went out the door");

    // The bot saw them come and go through replication.
    let seen = room.bots[0].world_mut().query::<&Customer>().iter(room.bots[0].world()).count();
    assert_eq!(seen, 0);
}

#[test]
fn last_call_sends_everyone_home() {
    // Open ends 12 seconds in: everyone is seated and still patient.
    let mut room = room(timings(12, 30), 2);
    run(&mut room, 12.5);
    assert!(customers(room.host.world_mut()).iter().all(|(c, _)| c.mood == Mood::Waiting));
    run(&mut room, 1.0);
    assert!(customers(room.host.world_mut()).iter().all(|(c, _)| c.mood == Mood::Leaving));
    run(&mut room, 20.0);
    assert!(customers(room.host.world_mut()).is_empty());
}

#[test]
fn a_customer_whose_stool_is_moved_leaves() {
    let mut room = room(timings(120, 30), 3);
    run(&mut room, 16.5);
    let world = room.host.world_mut();
    let (id, stool) = {
        let mut q = world.query::<(&Customer, &Npc)>();
        let (c, npc) = q.iter(world).next().expect("a customer");
        (c.id, npc.seat.expect("seated").0)
    };
    // Shove the stool half a meter.
    world.get_mut::<Position>(stool).unwrap().0.x += 0.5;
    run(&mut room, 0.1);
    let moods: Vec<_> = customers(room.host.world_mut()).into_iter().map(|(c, _)| (c.id, c.mood)).collect();
    assert!(moods.contains(&(id, Mood::Leaving)), "{moods:?}");
    assert_eq!(moods.iter().filter(|(_, m)| *m == Mood::Leaving).count(), 1, "only that customer leaves");
}

#[test]
fn the_same_seed_brings_the_same_customers() {
    let trace = |seed: u8| {
        let mut room = room(timings(120, 30), seed);
        run(&mut room, 10.0);
        let world = room.host.world_mut();
        let mut q = world.query::<(&Customer, &Npc, &NpcPose)>();
        let mut v: Vec<_> =
            q.iter(world).map(|(c, n, p)| (c.id, n.cash, p.pos.x.to_bits(), p.pos.z.to_bits())).collect();
        v.sort();
        v
    };
    assert_eq!(trace(7), trace(7));
    assert_ne!(trace(7), trace(8));
}

#[test]
fn walking_customers_stay_inside_the_download_budget() {
    // A wave every second fills all ten stools.
    let t = Timings { setup: 1, open: 60, last_call: 30, payment: 1, outcome: 1, wave: 1 };
    let mut room = room(t, 4);
    run(&mut room, 1.5);
    let before = room.bot_stats[0].snapshot();
    run(&mut room, 8.0);
    let after = room.bot_stats[0].snapshot();
    let walking = customers(room.host.world_mut()).len();
    assert!(walking >= 10, "{walking} customers");
    let down_per_s = (after[1] - before[1]) as f64 / 8.0;
    println!("{walking} customers walking: {down_per_s:.0} B/s down");
    assert!(down_per_s < 40_000.0);
}
