//! A client drawing at about 13 fps (one frame every 5 host ticks) must stay
//! close to the host: its predicted timeline may lead by a round trip, not by
//! seconds.

use bots::{LocalRoom, Script};

#[test]
fn slow_frame_rate_client_keeps_a_small_lead() {
    let mut room = LocalRoom::new(1, |_| Script::Circle { phase: 0.0 });
    let start = std::time::Instant::now();
    let mut worst: f32 = 0.0;
    for t in 1..=(64 * 12u32) {
        room.host.tick();
        if t % 5 == 0 {
            room.bots[0].update();
        }
        if let Some(wait) = (shared::TICK * t).checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
        if t > 64 * 6 && t % 64 == 0 {
            let id = room.session(0).player_id.unwrap();
            let host = room.host_pos(id).unwrap();
            let pred = room.predicted_pos(0).unwrap();
            let err = host.distance(pred);
            worst = worst.max(err);
            println!("t {t}: prediction leads host by {err:.2} m");
        }
    }
    // At 4 m/s, 1 m is 250 ms of lead.
    assert!(worst < 1.0, "prediction leads the host by {worst} m");
}
