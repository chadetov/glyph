//! Drift guard for `docs/reference/stdlib.md`.
//!
//! The stdlib reference is hand-written, so it can fall out of step with the
//! runtime. This test reads every exported name from
//! `glyph-compiler/runtime/std/*.ts` and asserts each one appears in the
//! reference. Adding a stdlib function and forgetting to document it fails the
//! build. (It is a one-directional guard: it catches undocumented additions, not
//! stale entries, and a name that happens to be a common English word could be
//! satisfied by prose. That is an acceptable trade for a dependency-free test.)

use std::fs;
use std::path::PathBuf;

fn runtime_std_dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "runtime", "std"]
        .iter()
        .collect()
}

fn stdlib_doc() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "..",
        "docs",
        "reference",
        "stdlib.md",
    ]
    .iter()
    .collect()
}

/// Pull the exported identifier out of a line like `export function foo<T>(` or
/// `export type Bar =` or `export const Baz:`.
fn exported_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export ")?;
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = ["function ", "type ", "const ", "interface "]
        .iter()
        .find_map(|kw| rest.strip_prefix(kw))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Whole-word membership: `name` appears in `text` not flanked by identifier
/// characters (so `get` matches `record.get` but not `getter`).
fn contains_word(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let start = from + rel;
        let end = start + name.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[test]
fn every_runtime_std_export_is_documented() {
    let doc_path = stdlib_doc();
    let doc = fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("read {doc_path:?}: {e}"));

    let dir = runtime_std_dir();
    let mut modules: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ts"))
        .collect();
    modules.sort();
    assert!(
        modules.len() >= 13,
        "expected the std runtime to have many modules, found {}",
        modules.len()
    );

    let mut missing: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for module in &modules {
        let stem = module.file_stem().unwrap().to_string_lossy().to_string();
        let source = fs::read_to_string(module)
            .unwrap_or_else(|e| panic!("read {module:?}: {e}"));
        for line in source.lines() {
            if let Some(name) = exported_name(line.trim_start()) {
                checked += 1;
                if !contains_word(&doc, &name) {
                    missing.push(format!("std/{stem}: `{name}`"));
                }
            }
        }
    }

    assert!(checked > 0, "no exports found under {dir:?}");
    assert!(
        missing.is_empty(),
        "docs/reference/stdlib.md is missing {} runtime export(s):\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}
