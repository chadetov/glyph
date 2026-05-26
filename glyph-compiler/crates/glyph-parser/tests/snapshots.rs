//! Golden snapshot tests via `insta`. Phase 1 week 1 day 1–2 corpus only.
//!
//! Each fixture under `tests/fixtures/` is parsed and the resulting AST is
//! snapshotted. Run `cargo insta review` to accept new/changed snapshots.
//!
//! The acceptance criterion at end of week 1 (per
//! `docs/implementation-plan.md`) is "all 4 example files parse to AST with
//! snapshots checked into git." This file currently snapshots a small
//! hello-world fixture that exercises the parsable subset; the four
//! `examples/*.glyph` files snapshot in once their gating features (JSX,
//! match, tagged unions, mut, for/loop, template interpolation) land later
//! this week.

use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

fn example(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "..",
        "examples",
        name,
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

#[test]
fn hello_world_parses() {
    let source = fixture("hello.glyph");
    let ast = glyph_parser::parse(&source).expect("hello.glyph should parse");
    insta::assert_debug_snapshot!(ast);
}

/// Try parsing the real example files. Each test reports the first parse
/// error if it fails. This is the week-1 acceptance criterion: all 4 example
/// files parse to AST with snapshots checked into git.
///
/// Day 3 status:
/// - 01_validator.glyph: depends on D5 `mut` and D21 `for` (deferred)
/// - 02_async_errors.glyph: depends on D22 template literal interpolation (`/api/${id}`)
/// - 03_react_component.glyph: depends on D6 JSX
/// - 04_cli_tool.glyph: depends on D21 `for`/array patterns and D5 `mut`
///
/// All four are expected to fail at this point; the tests below document the
/// failure mode so day 4+ can resolve them one by one.

#[test]
fn example_01_validator_parses() {
    let source = example("01_validator.glyph");
    let ast = glyph_parser::parse(&source).expect("01_validator.glyph should parse");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn example_02_async_errors_parses() {
    let source = example("02_async_errors.glyph");
    let ast = glyph_parser::parse(&source).expect("02_async_errors.glyph should parse");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn example_03_react_component_parses() {
    let source = example("03_react_component.glyph");
    let ast = glyph_parser::parse(&source).expect("03_react_component.glyph should parse");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn example_04_cli_tool_parses() {
    let source = example("04_cli_tool.glyph");
    let ast = glyph_parser::parse(&source).expect("04_cli_tool.glyph should parse");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn day3_progress_report() {
    // This test runs in the normal suite and tells us how far each example
    // gets through parsing. It always passes; it's a diagnostic.
    for name in [
        "01_validator.glyph",
        "02_async_errors.glyph",
        "03_react_component.glyph",
        "04_cli_tool.glyph",
    ] {
        let source = example(name);
        match glyph_parser::parse(&source) {
            Ok(_) => println!("{name}: PARSE OK"),
            Err(e) => println!("{name}: parse error at byte {}: {e}", e.span().start),
        }
    }
}
