//! The drunk meter: drinking costs money and fills the meter, the meter
//! decays, Wasted players stumble, puddles trip Sloppy walkers, and at 100 a
//! player passes out, can be dragged, and wakes up 45 seconds later.

use avian3d::prelude::{Position, RigidBody};
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bots::{LocalRoom, Script};
use host::HostConfig;
use host::drunk::{PassedOut, Stumble};
use host::economy::Preset;
use shared::protocol::{Drunk, Player, PlayerPos, Pocket, Puddle};

/// These tests run bots in real time. Run in parallel, they starve each
/// other of CPU and the bots' inputs reach the host late, so take turns.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn player(world: &mut World, id: u64) -> Entity {
    world.query::<(Entity, &Player)>().iter(world).find(|(_, p)| p.id == id).map(|(e, _)| e).expect("player")
}

fn realtime(room: &mut LocalRoom, seconds: f32) {
    for _ in 0..(seconds * shared::TICK_HZ as f32) as u32 {
        room.step();
        std::thread::sleep(shared::TICK);
    }
}

fn fast(room: &mut LocalRoom, seconds: f32) {
    for _ in 0..(seconds * shared::TICK_HZ as f32) as u32 {
        room.step();
    }
}

#[test]
fn a_beer_costs_five_and_fills_the_meter() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let config = HostConfig { preset: Preset::LastWeek, ..Default::default() };
    let mut room = LocalRoom::with_config(1, config, |_| Script::Pour { hold: 165, pitch: -0.4, drink: true });
    realtime(&mut room, 12.0);
    let id = room.session(0).player_id.unwrap();
    let world = room.host.world_mut();
    let e = player(world, id);
    let drunk = *world.get::<Drunk>(e).unwrap();
    // One beer (+20), drunk several seconds ago: a few points have decayed.
    assert!((14..=20).contains(&drunk.level), "{drunk:?}");
    assert_eq!(world.get::<Pocket>(e).unwrap().0, 295);
}

#[test]
fn the_meter_decays_a_point_every_two_seconds() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut room = LocalRoom::new(1, |_| Script::Idle);
    fast(&mut room, 1.0);
    let id = room.session(0).player_id.unwrap();
    let e = player(room.host.world_mut(), id);
    room.host.world_mut().get_mut::<Drunk>(e).unwrap().level = 50;
    fast(&mut room, 10.0);
    let level = room.host.world_mut().get::<Drunk>(e).unwrap().level;
    assert!((44..=46).contains(&level), "{level}");
}

#[test]
fn wasted_players_stumble() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut room = LocalRoom::new(1, |_| Script::Idle);
    fast(&mut room, 1.0);
    let id = room.session(0).player_id.unwrap();
    let e = player(room.host.world_mut(), id);
    room.host.world_mut().get_mut::<Drunk>(e).unwrap().level = 90;
    let start = room.host.world_mut().get::<PlayerPos>(e).unwrap().0;
    let mut stumbled = false;
    for _ in 0..(9 * shared::TICK_HZ) {
        room.step();
        stumbled |= room.host.world_mut().get::<Stumble>(e).is_some();
    }
    assert!(stumbled, "a stumble within 9 seconds");
    let moved = room.host.world_mut().get::<PlayerPos>(e).unwrap().0.distance(start);
    assert!(moved > 0.5, "standing still, yet moved {moved} m");
}

#[test]
fn sloppy_walkers_slip_on_puddles() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // The bot walks from its spawn toward the tap; a puddle lies on the way.
    let mut room = LocalRoom::new(1, |_| Script::Route { route: bots::ROUTE_TO_TAP });
    fast(&mut room, 0.5);
    let id = room.session(0).player_id.unwrap();
    let e = player(room.host.world_mut(), id);
    let spawn = room.host.world_mut().get::<PlayerPos>(e).unwrap().0;
    let target = Vec3::new(4.0, 0.0, -1.5);
    let on_path = spawn.lerp(target, 0.5);
    room.host.world_mut().spawn(Puddle { pos: Vec3::new(on_path.x, 0.01, on_path.z) });
    room.host.world_mut().get_mut::<Drunk>(e).unwrap().level = 50;
    let mut slipped = false;
    for _ in 0..(8 * shared::TICK_HZ) {
        room.step();
        std::thread::sleep(shared::TICK);
        slipped |= room.host.world_mut().get::<Stumble>(e).is_some();
    }
    assert!(slipped, "a Sloppy walker slips on the puddle");
}

#[test]
fn passing_out_dragging_and_waking_up() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // Bot 0 pours (up to 103%, so a late start still leaves a drinkable glass)
    // and drinks at the tap with the meter at 85: that beer makes 100. Bot 1
    // waits for a body on the floor, then drags it away.
    let config = HostConfig { preset: Preset::LastWeek, ..Default::default() };
    let mut room = LocalRoom::with_config(2, config, |i| {
        if i == 0 { Script::Pour { hold: 165, pitch: -0.4, drink: true } } else { Script::Drag }
    });
    realtime(&mut room, 1.0);
    let id = room.session(0).player_id.unwrap();
    let e = player(room.host.world_mut(), id);
    room.host.world_mut().get_mut::<Drunk>(e).unwrap().level = 85;

    let mut out = false;
    for _ in 0..(10 * shared::TICK_HZ) {
        room.step();
        std::thread::sleep(shared::TICK);
        if room.host.world_mut().get::<Drunk>(e).unwrap().passed_out {
            out = true;
            break;
        }
    }
    assert!(out, "passed out after the fifth beer's worth");
    realtime(&mut room, 1.0);
    let world = room.host.world_mut();
    assert!(world.get::<PassedOut>(e).is_some());
    assert!(world.get::<RigidBody>(e).unwrap().is_dynamic());
    assert!(world.get::<Position>(e).unwrap().0.y < 0.5, "lying on the floor");
    let lying_at = world.get::<PlayerPos>(e).unwrap().0;

    // Bot 1 sees the body, walks over, grabs it, and backs away toward +Z.
    let mut dragged_to = lying_at;
    for _ in 0..(12 * shared::TICK_HZ) {
        room.step();
        std::thread::sleep(shared::TICK);
        dragged_to = room.host.world_mut().get::<PlayerPos>(e).unwrap().0;
        if dragged_to.z - lying_at.z > 1.0 {
            break;
        }
    }
    assert!(dragged_to.z - lying_at.z > 1.0, "dragged from {lying_at} to {dragged_to}");

    // 45 seconds after passing out, the player gets up where they lie.
    let mut slept = 0;
    while room.host.world_mut().get::<Drunk>(e).unwrap().passed_out && slept < 50 * shared::TICK_HZ {
        room.step();
        slept += 1;
    }
    let world = room.host.world_mut();
    let drunk = *world.get::<Drunk>(e).unwrap();
    assert!(!drunk.passed_out, "awake: {drunk:?}");
    assert!(world.get::<PassedOut>(e).is_none());
    assert!(world.get::<RigidBody>(e).unwrap().is_kinematic());
    assert!(drunk.level < 100);
}
