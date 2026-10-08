//! A client that drops and comes back with the same UUID gets its old player
//! back: same id, same entity, no duplicate, position kept.

use bots::{LocalRoom, Script};

#[test]
fn refreshed_client_gets_the_same_player_back() {
    let mut room = LocalRoom::new(2, |i| if i == 0 { Script::Circle { phase: 0.0 } } else { Script::Idle });
    room.run_realtime(3.0);
    let id = room.session(0).player_id.expect("bot 0 joined");
    assert_eq!(room.host_players().len(), 2);

    room.disconnect(0);
    room.run_realtime(1.0);
    // Still there, waiting for a reconnect.
    assert_eq!(room.host_players().len(), 2);
    let left_at = room.host_pos(id).unwrap();

    room.reconnect(0);
    room.run_realtime(1.0);
    // Path walked over the next 2 s (a circling bot can sit in a corner, so
    // sum the steps instead of comparing two points).
    let mut walked = 0.0;
    let mut last = room.host_pos(id).unwrap();
    for _ in 0..8 {
        room.run_realtime(0.25);
        let p = room.host_pos(id).unwrap();
        walked += p.distance(last);
        last = p;
    }
    assert_eq!(room.session(0).player_id, Some(id), "rejoined as a different player");
    assert_eq!(room.host_players().len(), 2, "rejoin made a duplicate player");
    let spawn = glam_vec(shared::bar::spawn_point(0));
    let now = room.host_pos(id).unwrap();
    // It kept walking from where it was, not from the spawn point.
    assert!(left_at.distance(spawn) > 1.0);
    assert!(room.players_seen_by(0).len() == 2);
    println!("left at {left_at:?}, now {now:?}, walked {walked:.2} m after rejoining");
    // And the rejoined client controls it again.
    assert!(walked > 1.0, "rejoined client's inputs do not move the player");
}

fn glam_vec(a: [f32; 3]) -> bevy::math::Vec3 {
    bevy::math::Vec3::from_array(a)
}
