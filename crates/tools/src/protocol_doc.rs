//! Generate `docs/PROTOCOL.md` from the doc comments in
//! `crates/shared/src/protocol.rs` (plan section 3.3).
//!
//! It reads the source as text: every `pub struct`, `pub enum` and enum
//! variant with `///` docs becomes an entry, grouped under the file's
//! `// ---------- Section ----------` markers.

use std::fs;
use std::path::Path;

const SOURCE: &str = "crates/shared/src/protocol.rs";
const OUTPUT: &str = "docs/PROTOCOL.md";

/// Render the markdown for a protocol source file.
pub fn render(source: &str) -> String {
    let mut out = String::from(
        "# Protocol\n\nGenerated from `crates/shared/src/protocol.rs` by `cargo run -p last_call_tools -- protocol_doc`. Do not edit by hand.\n",
    );
    let mut docs: Vec<String> = Vec::new();
    let mut in_enum = false;
    for line in source.lines() {
        let t = line.trim();
        if let Some(section) = t.strip_prefix("// ----------").and_then(|r| r.strip_suffix("----------")) {
            out.push_str(&format!("\n## {}\n", section.trim()));
            docs.clear();
            continue;
        }
        if let Some(doc) = t.strip_prefix("///") {
            docs.push(doc.trim().to_string());
            continue;
        }
        if t.starts_with("#[") {
            continue;
        }
        let item = t
            .strip_prefix("pub struct ")
            .map(|r| ("struct", r))
            .or_else(|| t.strip_prefix("pub enum ").map(|r| ("enum", r)))
            .or_else(|| t.strip_prefix("pub const ").map(|r| ("const", r)));
        if let Some((kind, rest)) = item {
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            out.push_str(&format!("\n### `{name}` ({kind})\n\n"));
            if !docs.is_empty() {
                out.push_str(&docs.join(" "));
                out.push('\n');
            }
            in_enum = kind == "enum" && t.ends_with('{');
            docs.clear();
            continue;
        }
        if in_enum {
            if t == "}" {
                in_enum = false;
            } else if let Some(c) = t.chars().next()
                && c.is_ascii_uppercase()
            {
                let variant: String = t.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let detail = if docs.is_empty() { String::new() } else { format!(": {}", docs.join(" ")) };
                out.push_str(&format!("- `{variant}`{detail}\n"));
            }
        }
        docs.clear();
    }
    out
}

/// Write (or with `check`, verify) the generated file. Returns an exit code.
pub fn run(check: bool) -> i32 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = match fs::read_to_string(root.join(SOURCE)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {SOURCE}: {e}");
            return 1;
        }
    };
    let rendered = render(&source);
    let path = root.join(OUTPUT);
    if check {
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current != rendered {
            eprintln!("{OUTPUT} is out of date; run: cargo run -p last_call_tools -- protocol_doc");
            return 1;
        }
        println!("{OUTPUT} is up to date");
        return 0;
    }
    match fs::write(&path, rendered) {
        Ok(()) => {
            println!("wrote {OUTPUT}");
            0
        }
        Err(e) => {
            eprintln!("cannot write {OUTPUT}: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sections_items_and_variants() {
        let src = "// ---------- Messages ----------\n/// Hello there.\n#[derive(Clone)]\npub enum Reply {\n    /// It worked.\n    Welcome { id: u64 },\n    Refused,\n}\n";
        let md = render(src);
        assert!(md.contains("## Messages"));
        assert!(md.contains("### `Reply` (enum)\n\nHello there."));
        assert!(md.contains("- `Welcome`: It worked."));
        assert!(md.contains("- `Refused`\n"));
    }
}
