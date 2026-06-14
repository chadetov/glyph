//! Formatter correctness against the real example + corpus programs.
//!
//! Two properties per file:
//! - **Stable:** the formatted output re-parses, and formatting it again is a
//!   fixed point (idempotent).
//! - **Semantics-preserving:** the emitter (which is span-insensitive) produces
//!   identical TypeScript from the original and the formatted source — so the
//!   reformat changed layout, not meaning.
//!
//! Plus focused unit checks on the layout rules.

use std::fs;
use std::path::{Path, PathBuf};

use glyph_formatter::format_module;

fn examples_dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "..", "examples"]
        .iter()
        .collect()
}

fn glyph_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            glyph_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path);
        }
    }
}

/// Parse → resolve → assign-types → emit, tolerating resolve/type errors (the
/// emitter consults types only where known). `None` if parse or emit fails.
fn emit_ts(src: &str) -> Option<String> {
    let m = glyph_parser::parse(src).ok()?;
    let syms = glyph_resolver::collect_module_symbols(&m).ok()?;
    let prelude = glyph_resolver::build_prelude();
    let (resolved, _re) = glyph_resolver::resolve_module(&m, syms, &prelude);
    let (tm, _te) = glyph_typechecker::assign_types(&m, &resolved, &prelude);
    glyph_emit::emit_module(&m, &resolved, &tm, &prelude).ok()
}

fn fmt(src: &str) -> String {
    let m = glyph_parser::parse(src).expect("parse");
    format_module(&m, &glyph_lexer::comments(src), src)
}

#[test]
fn examples_format_is_stable_and_semantics_preserving() {
    let mut files = Vec::new();
    glyph_files(&examples_dir(), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no example .glyph files found");

    let mut oracle_ran = 0;
    for f in &files {
        let label = f.strip_prefix(examples_dir()).unwrap_or(f).display();
        let src = fs::read_to_string(f).unwrap();

        // Stable: format → reparse → format is a fixed point.
        let m = glyph_parser::parse(&src).unwrap_or_else(|e| panic!("{label}: parse: {e:?}"));
        let once = format_module(&m, &glyph_lexer::comments(&src), &src);
        let m2 = glyph_parser::parse(&once).unwrap_or_else(|e| {
            panic!("{label}: formatted output did not re-parse: {e:?}\n--- output ---\n{once}")
        });
        let twice = format_module(&m2, &glyph_lexer::comments(&once), &once);
        assert_eq!(once, twice, "{label}: formatting is not idempotent");

        // Comments are preserved: every comment's text survives.
        for c in glyph_lexer::comments(&src) {
            assert!(
                once.contains(&c.text),
                "{label}: dropped comment {:?}\n--- output ---\n{once}",
                c.text
            );
        }

        // Semantics-preserving via the emit oracle.
        if let Some(before) = emit_ts(&src) {
            let after = emit_ts(&once)
                .unwrap_or_else(|| panic!("{label}: formatted source failed to emit"));
            assert_eq!(before, after, "{label}: formatting changed the emitted TypeScript");
            oracle_ran += 1;
        }
    }
    assert!(
        oracle_ran >= 4,
        "expected the emit oracle to run on at least the four hard-case examples, ran on {oracle_ran}"
    );
}

#[test]
fn binary_precedence_uses_minimal_parens() {
    let plain = fmt("module x\nfn f() -> number {\n  return 1 + 2 * 3\n}\n");
    assert!(plain.contains("1 + 2 * 3"), "{plain}");
    let grouped = fmt("module x\nfn f() -> number {\n  return (1 + 2) * 3\n}\n");
    assert!(grouped.contains("(1 + 2) * 3"), "{grouped}");
    // Left-associative: a right-side same-precedence child is parenthesized.
    let right = fmt("module x\nfn f() -> number {\n  return 1 - (2 - 3)\n}\n");
    assert!(right.contains("1 - (2 - 3)"), "{right}");
}

#[test]
fn record_over_two_fields_is_multiline_with_trailing_comma() {
    let two = fmt("module x\ntype P = { a: number, b: number }\n");
    assert!(two.contains("type P = { a: number, b: number }"), "{two}");
    let three = fmt("module x\ntype P = { a: number, b: number, c: number }\n");
    assert!(three.contains("a: number,\n"), "expected one-per-line; got:\n{three}");
    assert!(three.contains("c: number,\n}"), "expected trailing comma; got:\n{three}");
}

#[test]
fn union_renders_in_multiline_bar_form() {
    let u = fmt("module x\ntype Feed = Loading | Loaded | Failed\n");
    assert!(u.contains("type Feed =\n  | Loading\n  | Loaded\n  | Failed\n"), "{u}");
}

#[test]
fn string_escapes_are_preserved_not_corrupted() {
    // G11: a no-op format must not rewrite string contents. A single-line
    // literal with `\n`/`\t` escapes stays single-line and keeps its escapes
    // (it must not be split into raw control bytes).
    let src = "module x\nfn f() -> string {\n  return \"a\\tb\\nc\"\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("\"a\\tb\\nc\""),
        "escapes must round-trip verbatim; got:\n{once}"
    );
    assert!(
        !once.contains("a\tb"),
        "must not emit a raw TAB into the source; got:\n{once:?}"
    );
    assert_eq!(fmt(&once), once, "string formatting is not idempotent");
}

#[test]
fn multiline_d12_string_is_kept_verbatim() {
    // A D12 multi-line string (raw newlines in source) must survive verbatim,
    // not collapse onto one line.
    let src = "module x\nfn f() -> string {\n  return \"line1\nline2\"\n}\n";
    let once = fmt(src);
    assert!(
        once.contains("\"line1\nline2\""),
        "multi-line string must stay multi-line; got:\n{once:?}"
    );
    assert_eq!(fmt(&once), once, "multi-line string formatting is not idempotent");
}

#[test]
fn format_is_idempotent_on_a_reformatted_snippet() {
    // A deliberately badly-spaced source normalizes, then is stable.
    let src = "module x\nfn   f(a:number,b:number,c:number)->number{return a+b+c}\n";
    let once = fmt(src);
    let twice = fmt(&once);
    assert_eq!(once, twice, "not idempotent:\n{once}");
    assert!(
        glyph_parser::parse(&once).is_ok(),
        "normalized output must parse:\n{once}"
    );
}
