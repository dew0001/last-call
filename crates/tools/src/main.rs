//! Tool entry point. Subcommands are added per phase.
//!
//!   tools protocol_doc [--check]   write docs/PROTOCOL.md from the protocol's doc comments
//!   tools shift_replay [--update]  run the determinism replay; store its golden hash

mod protocol_doc;
mod shift_replay;

const COMMANDS: &[(&str, &str)] = &[
    ("protocol_doc", "write docs/PROTOCOL.md from crates/shared/src/protocol.rs (--check: fail if stale)"),
    ("shift_replay", "run the 14-minute determinism replay; --update writes the golden hash"),
    ("fetch_assets", "download CC0 packs listed in assets/LICENSES.md (Phase 6)"),
    ("gen_sfx", "synthesize placeholder SFX to ogg (Phase 2)"),
    ("bake_lighting", "bake lightmaps for all rooms (Phase 6)"),
    ("replay", "re-derive outcomes from an RNG audit log (Phase 3)"),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help") => {
            for (name, about) in COMMANDS {
                println!("{name:14} {about}");
            }
        }
        Some("protocol_doc") => std::process::exit(protocol_doc::run(args.iter().any(|a| a == "--check"))),
        Some("shift_replay") => {
            let ticks =
                args.iter().position(|a| a == "--ticks").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
            std::process::exit(shift_replay::run(
                args.iter().any(|a| a == "--update"),
                ticks,
                args.iter().any(|a| a == "--describe"),
            ))
        }
        Some(other) => {
            eprintln!("unknown or not yet built: {other}");
            std::process::exit(2);
        }
    }
}
