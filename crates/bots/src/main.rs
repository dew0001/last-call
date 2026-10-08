//! Run a local room: one host and N bots in real time, then print what each
//! bot sees. Usage: `bots [bots] [seconds]` (defaults 8 and 5).

use std::time::{Duration, Instant};

use bots::{LocalRoom, Script};

fn main() {
    let mut args = std::env::args().skip(1);
    let n: u8 = args.next().and_then(|s| s.parse().ok()).unwrap_or(8);
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut room = LocalRoom::new(n, |i| Script::Circle { phase: f32::from(i) });
    let start = Instant::now();
    let mut ticks = 0u64;
    while start.elapsed() < Duration::from_secs(secs) {
        room.step();
        ticks += 1;
        let next = shared::TICK * ticks as u32;
        if let Some(wait) = next.checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    println!("host ticks: {}, players on host: {}", room.host.tick_count(), room.host_players().len());
    for i in 0..usize::from(n) {
        println!("bot {i}: session {:?}, sees {} players", room.session(i), room.players_seen_by(i).len());
    }
}
