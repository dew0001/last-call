//! Host plus 8 native bots over byte pipes, in real time (Phase 1 netcode).

use bots::{LocalRoom, Script};

#[test]
fn eight_bots_join_move_and_stay_in_budget() {
    let mut room = LocalRoom::new(8, |i| Script::Circle { phase: f32::from(i) * 0.7 });

    // Warm up: join, sync timelines, receive the first snapshots.
    room.run_realtime(3.0);
    for i in 0..8 {
        let s = room.session(i);
        assert!(s.player_id.is_some(), "bot {i} not welcomed: {s:?}");
        assert_eq!(room.players_seen_by(i).len(), 8, "bot {i} does not see everyone");
    }
    assert_eq!(room.host_players().len(), 8);

    // Measure traffic over a steady window.
    let before: Vec<[u64; 4]> = room.bot_stats.iter().map(|s| s.snapshot()).collect();
    let window = 4.0;
    room.run_realtime(window);
    let mut host_up = 0.0;
    for (i, stats) in room.bot_stats.iter().enumerate() {
        let now = stats.snapshot();
        let up = (now[0] - before[i][0]) as f32 / window;
        let down = (now[1] - before[i][1]) as f32 / window;
        host_up += down;
        let pk = (now[2] - before[i][2]) as f32 / window;
        println!(
            "bot {i}: up {:.0} B/s in {:.1} pkt/s, down {:.0} B/s in {:.1} pkt/s",
            up,
            pk,
            down,
            (now[3] - before[i][3]) as f32 / window
        );
        assert!(up < 8_000.0, "bot {i} up {up} B/s over the 8 KB/s budget");
        assert!(down < 40_000.0, "bot {i} down {down} B/s over the 40 KB/s budget");
    }
    println!("host up total: {host_up:.0} B/s");
    assert!(host_up < 300_000.0);

    // Prediction: each bot's predicted position is close to the host's.
    for i in 0..8 {
        let id = room.session(i).player_id.unwrap();
        let host = room.host_pos(id).unwrap();
        let predicted = room.predicted_pos(i).expect("bot has a predicted player");
        let err = host.distance(predicted);
        println!("bot {i}: host {host:?} predicted {predicted:?} err {err:.3} m");
        // The prediction runs ahead of the host by about one round trip of
        // ticks; at walk speed that is well under a meter.
        assert!(err < 1.0, "bot {i} prediction off by {err} m");
    }
}
