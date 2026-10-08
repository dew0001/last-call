//! Clock-independent pacing: decides how many ticks are due for elapsed time.

use shared::TICK_HZ;

/// Most ticks a runner simulates in one wake-up. Prevents a spiral of death
/// after a long stall (for example a throttled tab).
pub const MAX_CATCH_UP: u32 = 8;

/// Converts wall-clock milliseconds into due ticks and measures the tick rate.
#[derive(Debug, Clone)]
pub struct Pacer {
    tick_ms: f64,
    next_tick_at: f64,
    window_start: f64,
    window_ticks: u32,
    last_rate: f32,
}

impl Pacer {
    pub fn new(now_ms: f64) -> Self {
        Self {
            tick_ms: 1000.0 / f64::from(TICK_HZ),
            next_tick_at: now_ms,
            window_start: now_ms,
            window_ticks: 0,
            last_rate: 0.0,
        }
    }

    /// Number of ticks to run now. Drops backlog beyond [`MAX_CATCH_UP`].
    pub fn due(&mut self, now_ms: f64) -> u32 {
        let mut n = 0;
        while self.next_tick_at <= now_ms && n < MAX_CATCH_UP {
            self.next_tick_at += self.tick_ms;
            n += 1;
        }
        if self.next_tick_at <= now_ms {
            self.next_tick_at = now_ms + self.tick_ms;
        }
        self.window_ticks += n;
        n
    }

    /// Milliseconds until the next tick is due.
    pub fn wait_ms(&self, now_ms: f64) -> f64 {
        (self.next_tick_at - now_ms).max(0.0)
    }

    /// Returns a new ticks-per-second figure once per second of wall time.
    pub fn poll_rate(&mut self, now_ms: f64) -> Option<f32> {
        let span = now_ms - self.window_start;
        if span < 1000.0 {
            return None;
        }
        self.last_rate = (f64::from(self.window_ticks) * 1000.0 / span) as f32;
        self.window_start = now_ms;
        self.window_ticks = 0;
        Some(self.last_rate)
    }

    pub fn last_rate(&self) -> f32 {
        self.last_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_clock_gives_64_hz() {
        let mut p = Pacer::new(0.0);
        let mut total = 0;
        let mut t = 0.0;
        while t < 10_000.0 {
            total += p.due(t);
            t += 1.0;
        }
        assert!((639..=641).contains(&total), "got {total}");
    }

    #[test]
    fn long_stall_caps_catch_up() {
        let mut p = Pacer::new(0.0);
        assert_eq!(p.due(5_000.0), MAX_CATCH_UP);
        assert!(p.due(5_000.0) == 0);
    }

    #[test]
    fn rate_reported_each_second() {
        let mut p = Pacer::new(0.0);
        let mut t = 0.0;
        let mut rate = None;
        while rate.is_none() {
            t += 1.0;
            p.due(t);
            rate = p.poll_rate(t);
        }
        assert!((rate.unwrap() - 64.0).abs() < 1.5);
    }
}
