//! The beer tap: a bot pours at the tap and gets a glass, holding too long
//! overflows into a puddle, and a glass put down in front of a waiting
//! customer is served, paid for and tipped.

use bevy::ecs::world::World;
use bevy::math::Vec3;
use bots::{LocalRoom, Script};
use host::HostConfig;
use host::customers::Npc;
use shared::customers::Mood;
use shared::protocol::{Beer, Customer, HeldBy, Player, Pocket, PropKind, Puddle, RoomState, RunLedger};
use shared::shift::Timings;

fn glasses(world: &mut World) -> Vec<(Beer, HeldBy)> {
    world
        .query::<(&PropKind, &Beer, &HeldBy)>()
        .iter(world)
        .filter(|(k, ..)| **k == PropKind::Glass)
        .map(|(_, b, h)| (*b, *h))
        .collect()
}

/// Real-time steps: the bot walks on its own clock.
fn run_realtime(room: &mut LocalRoom, seconds: f32) {
    for _ in 0..(seconds * shared::TICK_HZ as f32) as u32 {
        room.step();
        std::thread::sleep(shared::TICK);
    }
}

#[test]
fn a_perfect_pour_puts_a_glass_in_the_hand() {
    // 147 frames of E at a good tilt: 92% full, little foam.
    let mut room = LocalRoom::new(1, |_| Script::Pour { hold: 147, pitch: -0.4 });
    run_realtime(&mut room, 12.0);
    let id = room.session(0).player_id.unwrap();
    let g = glasses(room.host.world_mut());
    assert_eq!(g.len(), 1, "one glass: {g:?}; bot at {:?}", room.host_players());
    let (beer, held) = g[0];
    assert_eq!(held, HeldBy(Some(id)), "the glass is in the bot's hand");
    assert!((90..=94).contains(&beer.fill), "{beer:?}");
    assert!(beer.perfect);
    assert_eq!(beer.poured_by, id);
}

#[test]
fn holding_too_long_overflows_into_a_puddle() {
    let mut room = LocalRoom::new(1, |_| Script::Pour { hold: 400, pitch: -0.4 });
    run_realtime(&mut room, 14.0);
    assert!(glasses(room.host.world_mut()).is_empty(), "no glass from an overflow");
    let world = room.host.world_mut();
    let puddles: Vec<Puddle> = world.query::<&Puddle>().iter(world).copied().collect();
    assert_eq!(puddles.len(), 1);
    assert!(puddles[0].pos.y < 0.05, "on the floor");
}

#[test]
fn a_glass_in_front_of_a_waiting_customer_is_served() {
    let timings = Timings { setup: 1, open: 120, last_call: 30, payment: 1, outcome: 1, wave: 10_000 };
    let mut room = LocalRoom::with_config(1, HostConfig { timings, ..Default::default() }, |_| Script::Idle);
    for _ in 0..(17 * shared::TICK_HZ) {
        room.step();
    }
    let bot = room.session(0).player_id.unwrap();
    let world = room.host.world_mut();
    // The first waiting customer, and their stool's x.
    let (cust_id, stool_x, cash0) = {
        let mut q = world.query::<(&Customer, &Npc)>();
        let (c, n) = q.iter(world).find(|(c, _)| c.mood == Mood::Waiting).expect("a waiting customer");
        (c.id, n.seat.unwrap().1.x, n.cash)
    };
    let house0 = world.query::<(&RunLedger, &RoomState)>().single(world).unwrap().0.ledger.house;
    // Put a perfect glass on the counter in front of them.
    let (_, cz, _, _) = shared::bar::COUNTER;
    let at = Vec3::new(stool_x + 0.2, shared::bar::COUNTER_HEIGHT + 0.1, cz + 0.2);
    world.spawn(host::beer::glass(at, 0.92, true, bot, None));
    for _ in 0..64 {
        room.step();
    }
    let world = room.host.world_mut();
    let (c, n) = {
        let mut q = world.query::<(&Customer, &Npc)>();
        let (c, n) = q.iter(world).find(|(c, _)| c.id == cust_id).unwrap();
        (*c, n.cash)
    };
    assert_eq!(c.mood, Mood::Drinking);
    assert_eq!(n, cash0 - 8, "the customer paid 8");
    let house = world.query::<(&RunLedger, &RoomState)>().single(world).unwrap().0.ledger.house;
    assert_eq!(house - house0, 8, "into the house pool");
    let pocket = world.query::<(&Player, &Pocket)>().iter(world).find(|(p, _)| p.id == bot).unwrap().1.0;
    assert_eq!(pocket, 2, "a perfect pour tips 2");
    assert!(glasses(room.host.world_mut()).is_empty(), "the customer took the glass");
}
