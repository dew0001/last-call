//! Tool entry point. Subcommands are added per phase.
//!
//!   tools protocol_doc [--check]   write docs/PROTOCOL.md from the protocol's doc comments
//!   tools shift_replay [--update]  run the determinism replay; store its golden hash
//!   tools replay FILE...           re-derive every outcome in RNG audit logs

mod protocol_doc;
mod replay;
mod shift_replay;

const COMMANDS: &[(&str, &str)] = &[
    ("protocol_doc", "write docs/PROTOCOL.md from crates/shared/src/protocol.rs (--check: fail if stale)"),
    ("shift_replay", "run the 14-minute determinism replay; --update writes the golden hash"),
    ("fetch_assets", "download CC0 packs listed in assets/LICENSES.md (Phase 6)"),
    ("gen_sfx", "synthesize placeholder SFX to ogg (Phase 2)"),
    ("bake_lighting", "bake lightmaps for all rooms (Phase 6)"),
    ("replay", "re-derive every outcome in RNG audit logs (JSONL from `host --audit` or the browser)"),
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
        Some("replay") => std::process::exit(replay::run(&args[1..])),
        Some(other) => {
            eprintln!("unknown or not yet built: {other}");
            std::process::exit(2);
        }
    }
}
