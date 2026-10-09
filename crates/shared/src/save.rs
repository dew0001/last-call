//! Saved runs: the host stores one at the start of every shift's Setup, so a
//! group can stop and pick the run up later in a new room.
//!
//! A save holds what carries from shift to shift: the ledger (house pool,
//! debt paid, misses, new game plus level), the calendar, the upgrades, and
//! each player's pocket by player id. Props, customers and table rounds reset at every
//! shift anyway, and a resumed room draws a fresh RNG seed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::economy::Ledger;
use crate::shift::Calendar;

/// Bump when the save format changes; older saves are refused.
pub const SAVE_VERSION: u16 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSave {
    pub version: u16,
    pub ledger: Ledger,
    /// The shift whose Setup the run resumes at.
    pub calendar: Calendar,
    /// Pockets by player id (hex, so JSON keeps all 64 bits).
    pub pockets: BTreeMap<String, i64>,
    /// Upgrades bought this run.
    #[serde(default)]
    pub upgrades: crate::upgrades::Upgrades,
}

impl RunSave {
    pub fn new(ledger: Ledger, calendar: Calendar, pockets: impl IntoIterator<Item = (u64, i64)>) -> Self {
        Self {
            version: SAVE_VERSION,
            ledger,
            calendar,
            pockets: pockets.into_iter().map(|(id, m)| (format!("{id:016x}"), m)).collect(),
            upgrades: crate::upgrades::Upgrades::default(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("saves serialize")
    }

    /// Parse a save; refuse another version.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let save: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // Version 1 had no upgrades; they default to none.
        if !(1..=SAVE_VERSION).contains(&save.version) {
            return Err(format!("save version {} (this build reads {SAVE_VERSION})", save.version));
        }
        Ok(save)
    }

    /// Pockets keyed by numeric player id.
    pub fn pockets_by_id(&self) -> BTreeMap<u64, i64> {
        self.pockets.iter().filter_map(|(k, v)| u64::from_str_radix(k, 16).ok().map(|id| (id, *v))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunSave {
        let ledger = Ledger { house: 4_200, paid: 8_000, carried: 0, missed_in_a_row: 0, ng: 1 };
        RunSave::new(ledger, Calendar { week: 2, shift: 1 }, [(u64::MAX, 55), (7, 0)])
    }

    #[test]
    fn a_save_round_trips() {
        let s = sample();
        let back = RunSave::from_json(&s.to_json()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.pockets_by_id().get(&u64::MAX), Some(&55), "all 64 bits survive JSON");
    }

    #[test]
    fn other_versions_and_junk_are_refused() {
        let mut s = sample();
        s.version = SAVE_VERSION + 1;
        assert!(RunSave::from_json(&s.to_json()).unwrap_err().contains("version"));
        s.version = 0;
        assert!(RunSave::from_json(&s.to_json()).unwrap_err().contains("version"));
        assert!(RunSave::from_json("{").is_err());
    }
}
