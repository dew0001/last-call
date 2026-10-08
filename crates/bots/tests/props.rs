//! Physics props: they rest on the counter, a bot can pick one up and throw
//! it, nothing leaves the room, and the awake-body budget holds.

use bots::{LocalRoom, Script};
use shared::protocol::{HeldBy, PropKind};

#[test]
fn props_rest_and_a_bot_throws_a_bottle() {
    // Bot slot 2 spawns at x = -3, in front of bottles on the counter.
    let mut room = LocalRoom::new(3, |i| if i == 2 { Script::GrabAndThrow } else { Script::Idle });
    let props = room.host_props();
    assert_eq!(props.iter().filter(|p| p.1 == PropKind::Bottle).count(), 20);
    assert_eq!(props.iter().filter(|p| p.1 == PropKind::Chip).count(), 100);
    assert_eq!(props.iter().filter(|p| p.1 == PropKind::Stool).count(), 10);

    // Follow the bot from the first tick: it walks to the counter, picks up a
    // bottle and throws it. Bot scripts run on real time while the host
    // advances one tick per step, so on a slow machine the bot gets further
    // per host tick. Watching from the start (with a generous deadline) keeps
    // the test independent of machine speed.
    let mut id = None;
    let mut held_seen = false;
    let mut thrown = None;
    let mut start = None;
    let mut released_at = None;
    let mut worst_tick = std::time::Duration::ZERO;
    for tick in 0..(64 * 40) {
        let t = std::time::Instant::now();
        room.host.tick();
        worst_tick = worst_tick.max(t.elapsed());
        for bot in &mut room.bots {
            bot.update();
        }
        if id.is_none() {
            id = room.session(2).player_id;
        }
        let Some(id) = id else {
            std::thread::sleep(shared::TICK);
            continue;
        };
        for (e, kind, pose, held) in room.host_props() {
            if held == HeldBy(Some(id)) {
                held_seen = true;
                thrown = Some(e);
                start.get_or_insert(pose.pos);
                assert_eq!(kind, PropKind::Bottle);
            }
        }
        if released_at.is_none()
            && let Some(e) = thrown
            && room.host_prop(e).unwrap().2 == HeldBy(None)
        {
            released_at = Some(tick);
        }
        // Two seconds after the release, the bottle has landed.
        if released_at.is_some_and(|r| tick > r + 128) {
            break;
        }
        std::thread::sleep(shared::TICK);
    }
    assert!(held_seen, "bot never picked anything up");
    let (_, pose, held) = room.host_prop(thrown.unwrap()).unwrap();
    let flew = pose.pos.distance(start.unwrap());
    println!("thrown bottle flew {flew:.2} m to {:?}", pose.pos);
    assert_eq!(held, HeldBy(None), "bottle should be released");
    assert!(flew > 2.0, "bottle only moved {flew} m");

    for (_, kind, pose, _) in room.host_props() {
        let p = pose.pos;
        assert!(p.y > -0.2 && p.x.abs() < 10.5 && p.z.abs() < 7.5, "{kind:?} left the room at {p:?}");
    }
    let awake = host::physics::awake_bodies(room.host.world_mut());
    println!("awake bodies: {awake}, worst host tick: {worst_tick:?}");
    assert!(awake <= 300);
}

#[test]
fn a_dropped_prop_falls_to_the_floor() {
    let mut room = LocalRoom::new(3, |i| if i == 2 { Script::GrabAndDrop } else { Script::Idle });
    // Watch from the first tick, as in the throw test above.
    let mut id = None;
    let mut carried = None;
    for _ in 0..(64 * 40) {
        room.step();
        std::thread::sleep(shared::TICK);
        if id.is_none() {
            id = room.session(2).player_id;
        }
        if carried.is_none()
            && let Some(id) = id
        {
            carried = room.host_props().into_iter().find(|p| p.3 == HeldBy(Some(id))).map(|p| p.0);
        }
        if let Some(e) = carried {
            let (kind, pose, held) = room.host_prop(e).unwrap();
            // On the floor: below the counter top (a stool's center rests at 0.38 m).
            if held == HeldBy(None) && pose.pos.y < 0.5 {
                println!("dropped {kind:?} fell below the counter top: {:?}", pose.pos);
                return;
            }
        }
    }
    panic!("no prop was picked up and dropped to the floor (carried: {carried:?})");
}
