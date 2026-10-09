//! `tools replay FILE...`: re-derive every outcome in RNG audit logs.
//!
//! A log is JSONL from a native host (`host --audit FILE`) or exported from
//! the browser (the host page's "RNG log" button, IndexedDB). Every draw is
//! checked against its stream rebuilt from the room seed, and every shuffle,
//! spin and slot pull is derived again from its draws.

pub fn run(files: &[String]) -> i32 {
    if files.is_empty() {
        eprintln!("usage: tools replay FILE...");
        return 2;
    }
    let mut failed = false;
    for path in files {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{path}: {e}");
                failed = true;
                continue;
            }
        };
        match shared::audit::verify(text.lines()) {
            Ok(r) => println!(
                "{path}: OK. {} draws on {} streams; {} shuffles, {} spins, {} slot pulls re-derived",
                r.draws, r.streams, r.shuffles, r.spins, r.reels
            ),
            Err(e) => {
                eprintln!("{path}: FAILED: {e}");
                failed = true;
            }
        }
    }
    i32::from(failed)
}
