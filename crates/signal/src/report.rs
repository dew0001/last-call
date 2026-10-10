//! Crash reports (plan section 10, Phase 7): a game tab that panics posts
//! the message to `/report`. Reports go to Workers KV on the free plan,
//! capped at [`MAX_REPORTS`]; the oldest are dropped first.

use serde::{Deserialize, Serialize};

/// Most reports kept.
pub const MAX_REPORTS: usize = 200;
/// Longest body accepted, bytes.
pub const MAX_BODY: usize = 8 * 1024;
/// Longest field kept, characters.
const MAX_FIELD: usize = 4000;

/// What a tab sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub message: String,
    #[serde(default)]
    pub stack: String,
    /// The game build (`build-mode.txt` plus the page's version).
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub agent: String,
}

fn cut(s: &str) -> String {
    s.chars().take(MAX_FIELD).collect()
}

/// Check and trim a posted body.
pub fn parse(body: &[u8]) -> Result<Report, &'static str> {
    if body.len() > MAX_BODY {
        return Err("report too large");
    }
    let r: Report = serde_json::from_slice(body).map_err(|_| "bad report")?;
    if r.message.trim().is_empty() {
        return Err("empty report");
    }
    Ok(Report { message: cut(&r.message), stack: cut(&r.stack), build: cut(&r.build), agent: cut(&r.agent) })
}

/// A key that sorts by time: `r:<ms, 13 digits>:<id>`.
pub fn key(now_ms: u64, id: &str) -> String {
    format!("r:{now_ms:013}:{id}")
}

/// Keys to delete so at most `max` remain (the oldest go first).
pub fn to_drop(mut keys: Vec<String>, max: usize) -> Vec<String> {
    keys.sort();
    let extra = keys.len().saturating_sub(max);
    keys.truncate(extra);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_checked_and_trimmed() {
        let r = parse(br#"{"message":"panicked at x","stack":"at a\nat b"}"#).unwrap();
        assert_eq!(r.message, "panicked at x");
        assert!(parse(br#"{"message":"  "}"#).is_err());
        assert!(parse(b"nope").is_err());
        assert!(parse(&vec![b' '; MAX_BODY + 1]).is_err());
        let long = format!(r#"{{"message":"{}"}}"#, "x".repeat(7000));
        assert!(parse(long.as_bytes()).is_err() || parse(long.as_bytes()).unwrap().message.len() == MAX_FIELD);
    }

    #[test]
    fn the_oldest_reports_go_first() {
        let keys: Vec<String> = [5u64, 1, 3, 2, 4].iter().map(|t| key(*t, "a")).collect();
        assert_eq!(to_drop(keys.clone(), 3), vec![key(1, "a"), key(2, "a")]);
        assert!(to_drop(keys, 10).is_empty());
        assert!(key(9, "a") < key(10, "a"), "keys sort by time");
    }
}
