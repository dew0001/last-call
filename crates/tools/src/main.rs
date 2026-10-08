//! Tool entry point. Subcommands are added per phase (`fetch_assets`,
//! `gen_sfx`, `bake_lighting`, `replay`). Phase 0 lists them only.

const COMMANDS: &[(&str, &str)] = &[
    ("fetch_assets", "download CC0 packs listed in assets/LICENSES.md (Phase 6)"),
    ("gen_sfx", "synthesize placeholder SFX to ogg (Phase 2)"),
    ("bake_lighting", "bake lightmaps for all rooms (Phase 6)"),
    ("replay", "re-derive outcomes from an RNG audit log (Phase 3)"),
];

fn main() {
    match std::env::args().nth(1).as_deref() {
        None | Some("help") => {
            for (name, about) in COMMANDS {
                println!("{name:14} {about}");
            }
        }
        Some(other) => {
            eprintln!("unknown or not yet built: {other}");
            std::process::exit(2);
        }
    }
}
