//! Spec conformance corpus: each `tests/conformance/*.glyph` program pins the
//! exact TypeScript the compiler emits for a language feature, keyed to the
//! D-decision(s) named in its header comment. The emitted output is a committed
//! insta snapshot, so any change to what the language *means* (not just whether
//! it compiles) fails this test and must be acknowledged by regenerating and
//! reviewing the diff.
//!
//! This is the enforcement behind the stability promise: "we don't silently
//! change your emitted code." A green run is proof the emit is byte-for-byte what
//! it was; a red run is a semantic change that a human has to sign off on.
//!
//! Regenerate after an intentional change:
//!   INSTA_UPDATE=always cargo test -p glyph-emit --test conformance
//! then review each `.snap` diff before committing.

use std::fs;
use std::path::PathBuf;

fn conformance_dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "tests", "conformance"]
        .iter()
        .collect()
}

/// Parse -> resolve -> typecheck -> emit, the same pipeline `glyph build` runs
/// (resolve/type errors are tolerated: the corpus pins emit shape, and a few
/// programs reference external names on purpose, e.g. the `new` interop case).
fn emit_ts(src: &str) -> String {
    let m = glyph_parser::parse(src).expect("corpus program must parse");
    let syms = glyph_resolver::collect_module_symbols(&m).expect("collect symbols");
    let prelude = glyph_resolver::build_prelude();
    let (resolved, _re) = glyph_resolver::resolve_module(&m, syms, &prelude);
    let (tm, _te) = glyph_typechecker::assign_types(&m, &resolved, &prelude);
    glyph_emit::emit_module(&m, &resolved, &tm, &prelude, glyph_emit::EmitContext::single())
        .expect("corpus program must emit")
}

#[test]
fn conformance_corpus_emits_pinned_typescript() {
    let dir = conformance_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("glyph"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 10,
        "expected a substantial conformance corpus, found {}",
        files.len()
    );

    for f in &files {
        let stem = f.file_stem().unwrap().to_str().unwrap().to_string();
        let src = fs::read_to_string(f).unwrap_or_else(|e| panic!("read {f:?}: {e}"));
        let ts = emit_ts(&src);
        insta::assert_snapshot!(stem, ts);
    }
}
