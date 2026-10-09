//! Run the determinism replay (`host::replay`) and print or store its hash.
//!
//! `tools shift_replay`           print the hash and how long the run took
//! `tools shift_replay --update`  write it to the golden file the tests compare with
//! `tools shift_replay --ticks N` run only N ticks (to find where two builds diverge)
//! `tools shift_replay --describe` also print state parts and positions

use std::fs;
use std::path::Path;

/// The golden hash, compared by the native test and the wasm browser test.
pub const GOLDEN: &str = "crates/host/tests/replay.hash";

pub fn run(update: bool, ticks: Option<u64>, describe: bool) -> i32 {
    let start = std::time::Instant::now();
    let mut text = String::new();
    let r = host::replay::run_with(ticks.unwrap_or(host::replay::SHIFT_TICKS), |w| {
        if describe {
            text = host::replay::describe(w);
        }
    });
    print!("{text}");
    let hex = format!("{:016x}", r.hash);
    println!("{hex}  ({:.1} s; house {}, {} puddles)", start.elapsed().as_secs_f32(), r.house, r.puddles);
    if update && ticks.is_none() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(GOLDEN);
        if let Err(e) = fs::write(&path, format!("{hex}\n")) {
            eprintln!("cannot write {GOLDEN}: {e}");
            return 1;
        }
        println!("wrote {GOLDEN}");
    }
    0
}
