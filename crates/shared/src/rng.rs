//! Seeded RNG streams. One stream per table so one game never drains another.

use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// Identifier of an RNG stream (one per table, plus one for the room).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamId(pub u32);

/// The room-level stream (customers, chaos picks).
pub const ROOM_STREAM: StreamId = StreamId(0);

/// One recorded draw, for the audit log and the replay tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngDraw {
    pub stream: StreamId,
    pub tick: u64,
    pub value: u32,
}

/// Sink for RNG draws. IndexedDB in the browser, JSONL natively.
pub trait RngLog {
    fn record(&mut self, draw: &RngDraw);
}

/// A log that keeps draws in memory. Used by tests.
#[derive(Default, Debug)]
pub struct MemoryLog(pub Vec<RngDraw>);

impl RngLog for MemoryLog {
    fn record(&mut self, draw: &RngDraw) {
        self.0.push(draw.clone());
    }
}

/// A log that drops every draw.
#[derive(Default, Debug, Clone, Copy)]
pub struct NoLog;

impl RngLog for NoLog {
    fn record(&mut self, _draw: &RngDraw) {}
}

/// A source of random `u32`s. Game rules take `&mut impl Draw`, so the host
/// feeds them from a logged [`TableRng`] and the replay tool feeds them the
/// logged values, and both derive the same outcome.
pub trait Draw {
    fn next(&mut self) -> u32;

    /// Uniform integer in `0..n` without modulo bias. `n` must be above zero.
    fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "range must not be empty");
        let zone = u32::MAX - (u32::MAX % n);
        loop {
            let v = self.next();
            if v < zone {
                return v % n;
            }
        }
    }

    /// True with probability `percent` / 100.
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
}

/// A [`TableRng`] that logs each draw at a tick.
pub struct Logged<'a> {
    pub rng: &'a mut TableRng,
    pub tick: u64,
    pub log: &'a mut dyn RngLog,
}

impl Draw for Logged<'_> {
    fn next(&mut self) -> u32 {
        self.rng.next_u32_logged(self.tick, self.log)
    }
}

/// Replays recorded values in order. Panics when they run out.
pub struct Recorded<'a> {
    values: core::slice::Iter<'a, u32>,
}

impl<'a> Recorded<'a> {
    pub fn new(values: &'a [u32]) -> Self {
        Self { values: values.iter() }
    }

    /// Values not used yet.
    pub fn left(&self) -> usize {
        self.values.len()
    }
}

impl Draw for Recorded<'_> {
    fn next(&mut self) -> u32 {
        *self.values.next().expect("ran out of recorded draws")
    }
}

/// A ChaCha20 stream derived from the room seed and a stream id.
pub struct TableRng {
    id: StreamId,
    inner: ChaCha20Rng,
}

impl TableRng {
    /// Derive a stream. Each id gets its own ChaCha stream number from one seed.
    pub fn new(room_seed: [u8; 32], id: StreamId) -> Self {
        let mut inner = ChaCha20Rng::from_seed(room_seed);
        inner.set_stream(u64::from(id.0));
        Self { id, inner }
    }

    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Draw a `u32` and record it.
    pub fn next_u32_logged(&mut self, tick: u64, log: &mut dyn RngLog) -> u32 {
        let value = self.inner.next_u32();
        log.record(&RngDraw { stream: self.id, tick, value });
        value
    }

    /// Uniform integer in `0..n` without modulo bias. `n` must be above zero.
    pub fn below(&mut self, n: u32, tick: u64, log: &mut dyn RngLog) -> u32 {
        self.at(tick, log).below(n)
    }

    /// This stream as a [`Draw`] that logs at `tick`.
    pub fn at<'a>(&'a mut self, tick: u64, log: &'a mut dyn RngLog) -> Logged<'a> {
        Logged { rng: self, tick, log }
    }
}

/// An unlogged stream for simulations (house edge tests).
pub struct SimRng(pub ChaCha20Rng);

impl SimRng {
    pub fn new(seed: u64) -> Self {
        Self(ChaCha20Rng::seed_from_u64(seed))
    }
}

impl Draw for SimRng {
    fn next(&mut self) -> u32 {
        self.0.next_u32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream_is_deterministic() {
        let mut log = MemoryLog::default();
        let mut a = TableRng::new([7; 32], StreamId(3));
        let mut b = TableRng::new([7; 32], StreamId(3));
        for t in 0..100 {
            assert_eq!(a.next_u32_logged(t, &mut log), b.next_u32_logged(t, &mut log));
        }
        assert_eq!(log.0.len(), 200);
    }

    #[test]
    fn streams_are_independent() {
        let mut log = MemoryLog::default();
        let mut a = TableRng::new([7; 32], StreamId(1));
        let mut b = TableRng::new([7; 32], StreamId(2));
        let va: Vec<u32> = (0..8).map(|t| a.next_u32_logged(t, &mut log)).collect();
        let vb: Vec<u32> = (0..8).map(|t| b.next_u32_logged(t, &mut log)).collect();
        assert_ne!(va, vb);
    }

    #[test]
    fn below_stays_in_range() {
        let mut log = MemoryLog::default();
        let mut r = TableRng::new([1; 32], ROOM_STREAM);
        for t in 0..10_000 {
            assert!(r.below(37, t, &mut log) < 37);
        }
    }
}
