//! Native host runner. Runs the simulation at 64 Hz on the wall clock and
//! prints the measured tick rate once per second.
//!
//! Usage: `host [--seconds N]` (default 3).

use std::time::{Duration, Instant};

use host::HostSim;
use host::runner::Pacer;

fn main() {
    let seconds: u64 =
        std::env::args().skip_while(|a| a != "--seconds").nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);

    let start = Instant::now();
    let now_ms = || start.elapsed().as_secs_f64() * 1000.0;
    let mut sim = HostSim::new();
    let mut pacer = Pacer::new(now_ms());

    while start.elapsed() < Duration::from_secs(seconds) {
        for _ in 0..pacer.due(now_ms()) {
            sim.tick();
        }
        if let Some(rate) = pacer.poll_rate(now_ms()) {
            println!("host tick={} tps={rate:.1}", sim.tick_count());
        }
        std::thread::sleep(Duration::from_secs_f64(pacer.wait_ms(now_ms()) / 1000.0));
    }
}
