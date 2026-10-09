//! The RNG audit log (plan section 3.2): every draw from every stream, with
//! its tick, plus a record of each outcome derived from those draws (a
//! shuffle, a roulette result, slot stops). One JSON object per line.
//!
//! [`verify`] replays a log: it rebuilds every stream from the room seed,
//! checks each logged draw against it, and re-derives each outcome from its
//! draws with the same rule functions the host used. The browser keeps the
//! log in IndexedDB; native runs write a JSONL file. `tools replay` reads either.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cards::{self, DECKS};
use crate::rng::{Draw, Recorded, RngDraw, RngLog, StreamId, TableRng};
use crate::{roulette, slots};

/// What a set of draws decided.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Derived {
    /// A fresh 6-deck shoe, in dealing order.
    Shuffle { cards: Vec<u8> },
    /// Where the roulette ball lands.
    Spin { result: u8 },
    /// Where the slot reels stop.
    Reels { stops: [u8; 3] },
}

impl Derived {
    /// Re-derive this outcome from its draws.
    fn rederive(&self, draws: &[u32]) -> Result<Derived, String> {
        let mut d = Recorded::new(draws);
        let again = match self {
            Derived::Shuffle { .. } => Derived::Shuffle { cards: cards::shuffle(DECKS, &mut d) },
            Derived::Spin { .. } => Derived::Spin { result: roulette::spin(&mut d) },
            Derived::Reels { .. } => Derived::Reels { stops: slots::pull(&mut d) },
        };
        if d.left() != 0 {
            return Err(format!("{} draws left over", d.left()));
        }
        Ok(again)
    }
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "e", rename_all = "snake_case")]
pub enum Entry {
    /// The room started with this seed (hex).
    Start { seed: String },
    /// One draw: the `n`th value of `stream`.
    Draw { stream: u32, tick: u64, n: u64, value: u32 },
    /// Draws `first .. first + count` of `stream` decided `what`.
    Outcome { stream: u32, tick: u64, first: u64, count: u32, what: Derived },
}

impl Entry {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("audit entries serialize")
    }

    pub fn parse(line: &str) -> Result<Self, String> {
        serde_json::from_str(line).map_err(|e| e.to_string())
    }
}

pub fn seed_hex(seed: &[u8; 32]) -> String {
    seed.iter().map(|b| format!("{b:02x}")).collect()
}

fn seed_from_hex(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err("the seed is not 64 hex digits".into());
    }
    let mut seed = [0u8; 32];
    for (i, b) in seed.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(seed)
}

/// Collects the log as the host runs. Implements [`RngLog`], so a stream
/// drawn through it numbers and records each draw.
#[derive(Debug, Default)]
pub struct AuditLog {
    /// Lines not yet handed to storage.
    pending: Vec<Entry>,
    /// Draws so far per stream.
    counts: BTreeMap<u32, u64>,
    /// Every line ever written, when kept (tests).
    pub keep_all: bool,
    pub all: Vec<Entry>,
}

impl AuditLog {
    pub fn new(seed: &[u8; 32]) -> Self {
        let mut log = Self::default();
        log.push(Entry::Start { seed: seed_hex(seed) });
        log
    }

    fn push(&mut self, e: Entry) {
        if self.keep_all {
            self.all.push(e.clone());
        }
        self.pending.push(e);
    }

    /// Draws taken from `stream` so far: the index the next one gets.
    pub fn mark(&self, stream: StreamId) -> u64 {
        self.counts.get(&stream.0).copied().unwrap_or(0)
    }

    /// Record that the draws of `stream` since `first` (from [`Self::mark`]) decided `what`.
    pub fn outcome(&mut self, stream: StreamId, tick: u64, first: u64, what: Derived) {
        let count = (self.mark(stream) - first) as u32;
        self.push(Entry::Outcome { stream: stream.0, tick, first, count, what });
    }

    /// Take the lines written since the last call.
    pub fn drain(&mut self) -> Vec<Entry> {
        std::mem::take(&mut self.pending)
    }

    /// [`Self::drain`] as JSONL text.
    pub fn drain_jsonl(&mut self) -> String {
        self.drain().iter().map(|e| e.to_line() + "\n").collect()
    }
}

impl RngLog for AuditLog {
    fn record(&mut self, draw: &RngDraw) {
        let n = self.counts.entry(draw.stream.0).or_insert(0);
        let entry = Entry::Draw { stream: draw.stream.0, tick: draw.tick, n: *n, value: draw.value };
        *n += 1;
        self.push(entry);
    }
}

/// What a replay of a log found.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub draws: u64,
    pub streams: u32,
    pub shuffles: u32,
    pub spins: u32,
    pub reels: u32,
}

/// Replay a log: every draw must match its stream rebuilt from the seed, and
/// every outcome must follow from its draws. Lines may come from several
/// rooms in a row; each `start` begins a new one.
pub fn verify<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<Report, String> {
    let mut report = Report::default();
    let mut seed: Option<[u8; 32]> = None;
    let mut streams: BTreeMap<u32, (TableRng, Vec<u32>)> = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for (i, line) in lines.into_iter().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let at = |msg: String| format!("line {}: {msg}", i + 1);
        match Entry::parse(line).map_err(at)? {
            Entry::Start { seed: hex } => {
                seed = Some(seed_from_hex(&hex).map_err(at)?);
                streams.clear();
            }
            Entry::Draw { stream, tick: _, n, value } => {
                let seed = seed.ok_or_else(|| at("a draw before the start line".into()))?;
                let (rng, values) =
                    streams.entry(stream).or_insert_with(|| (TableRng::new(seed, StreamId(stream)), Vec::new()));
                if n != values.len() as u64 {
                    return Err(at(format!("stream {stream}: draw {n} out of order (expected {})", values.len())));
                }
                let expected = Logged0(rng).next();
                if value != expected {
                    return Err(at(format!("stream {stream}: draw {n} is {value}, the seed gives {expected}")));
                }
                values.push(value);
                seen.insert(stream);
                report.draws += 1;
            }
            Entry::Outcome { stream, tick: _, first, count, what } => {
                let values = streams.get(&stream).map(|s| &s.1[..]).unwrap_or(&[]);
                let (a, b) = (first as usize, first as usize + count as usize);
                let draws = values.get(a..b).ok_or_else(|| at(format!("stream {stream}: draws {a}..{b} missing")))?;
                let again = what.rederive(draws).map_err(at)?;
                if again != what {
                    return Err(at(format!("stream {stream}: the draws give a different outcome: {again:?}")));
                }
                match what {
                    Derived::Shuffle { .. } => report.shuffles += 1,
                    Derived::Spin { .. } => report.spins += 1,
                    Derived::Reels { .. } => report.reels += 1,
                }
            }
        }
    }
    report.streams = seen.len() as u32;
    Ok(report)
}

/// A bare stream as a [`Draw`], for rebuilding values without logging.
struct Logged0<'a>(&'a mut TableRng);

impl Draw for Logged0<'_> {
    fn next(&mut self) -> u32 {
        self.0.next_u32_logged(0, &mut crate::rng::NoLog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [42; 32];

    fn sample_log() -> Vec<String> {
        let mut log = AuditLog::new(&SEED);
        let mut table = TableRng::new(SEED, StreamId(10));
        let mut wheel = TableRng::new(SEED, StreamId(20));
        let first = log.mark(StreamId(10));
        let shoe = cards::shuffle(DECKS, &mut table.at(5, &mut log));
        log.outcome(StreamId(10), 5, first, Derived::Shuffle { cards: shoe });
        let first = log.mark(StreamId(20));
        let result = roulette::spin(&mut wheel.at(9, &mut log));
        log.outcome(StreamId(20), 9, first, Derived::Spin { result });
        let first = log.mark(StreamId(20));
        let stops = slots::pull(&mut wheel.at(12, &mut log));
        log.outcome(StreamId(20), 12, first, Derived::Reels { stops });
        log.drain().iter().map(Entry::to_line).collect()
    }

    #[test]
    fn a_log_replays() {
        let lines = sample_log();
        assert!(lines[0].starts_with(r#"{"e":"start","seed":"2a2a"#), "{}", lines[0]);
        let report = verify(lines.iter().map(String::as_str)).unwrap();
        assert_eq!(report.shuffles, 1);
        assert_eq!(report.spins, 1);
        assert_eq!(report.reels, 1);
        assert_eq!(report.streams, 2);
        assert!(report.draws >= 311 + 1 + 3);
    }

    #[test]
    fn a_changed_draw_is_caught() {
        let mut lines = sample_log();
        let i = lines.iter().position(|l| l.contains(r#""stream":20"#)).unwrap();
        let mut e = Entry::parse(&lines[i]).unwrap();
        if let Entry::Draw { value, .. } = &mut e {
            *value ^= 1;
        }
        lines[i] = e.to_line();
        let err = verify(lines.iter().map(String::as_str)).unwrap_err();
        assert!(err.contains("the seed gives"), "{err}");
    }

    #[test]
    fn a_changed_outcome_is_caught() {
        let mut lines = sample_log();
        let i = lines.iter().position(|l| l.contains(r#""spin""#)).unwrap();
        let mut e = Entry::parse(&lines[i]).unwrap();
        if let Entry::Outcome { what: Derived::Spin { result }, .. } = &mut e {
            *result = (*result + 1) % 37;
        }
        lines[i] = e.to_line();
        let err = verify(lines.iter().map(String::as_str)).unwrap_err();
        assert!(err.contains("different outcome"), "{err}");
    }
}
