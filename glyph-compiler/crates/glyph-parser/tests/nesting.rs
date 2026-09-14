//! The parser's nesting-depth limit (G229).
//!
//! Recursive descent spends stack per level of nesting, and until this limit
//! existed the input decided how much: `const x = ` followed by 2,000 `[` ended
//! `glyph check` and `glyph fmt` with `fatal runtime error: stack overflow,
//! aborting` and exit 134. An abort carries no span, no code and no chance to
//! recover, and `glyph lsp` and `glyph mcp` read every file under the project
//! root, so one file like that took the server down for the whole workspace.
//!
//! These tests hold the two halves of the fix: a program at the limit still
//! parses, and one past it comes back as `E0011` rather than as a dead process.

use glyph_parser::MAX_NESTING_DEPTH;

const MAX: usize = MAX_NESTING_DEPTH as usize;

fn fixture(name: &str) -> String {
    let path: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// `const x = [[[...1...]]]`, `n` levels of array literal.
fn nested_array(n: usize) -> String {
    format!("const x = {}1{}\n", "[".repeat(n), "]".repeat(n))
}

/// `type Deep = Array<Array<...<int>...>>`, `n` levels of generic application.
fn nested_generic(n: usize) -> String {
    format!("type Deep = {}int{}\n", "Array<".repeat(n), ">".repeat(n))
}

/// A match arm whose pattern is `n` nested constructor patterns.
///
/// Two levels are already open by the time the arm's pattern is read: the
/// function body's block, and the `match` itself. So `n` here is `n + 2` levels
/// deep, which is why the callers subtract two.
fn nested_pattern(n: usize) -> String {
    format!(
        "fn main(argv: Array<string>) -> number {{\n  \
         let v = 0\n  \
         let r = match v {{\n    \
         {}x{} => 1,\n    \
         else => 0,\n  \
         }}\n  \
         return r\n\
         }}\n",
        "Some(".repeat(n),
        ")".repeat(n)
    )
}

fn parse_err_code(source: &str) -> String {
    match glyph_parser::parse(source) {
        Ok(_) => panic!("expected a parse error, got a module"),
        Err(e) => e.code().to_string(),
    }
}

// ---------------------------------------------------------------------------
// The two fuzz inputs
// ---------------------------------------------------------------------------

/// The `parse` target's crashing input, verbatim, 988 bytes of mostly `[`.
///
/// The assertion is only that parsing returns. A panic fails the test; the
/// overflow this input used to cause does not fail a test at all, it aborts the
/// whole harness, which is exactly why the input is worth keeping.
#[test]
fn fuzz_parse_crash_input_returns_an_error() {
    let src = fixture("g229-fuzz-parse-crash.glyph");
    let err = glyph_parser::parse(&src).expect_err("this input is not a valid program");
    assert_eq!(err.code(), "E0011", "got: {err}");
}

/// The `format_idempotent` target's crashing input, 1,187 bytes. It never
/// reaches the formatter: `glyph fmt` parses first, so the file is reported
/// rather than formatted.
#[test]
fn fuzz_format_crash_input_returns_an_error() {
    let src = fixture("g229-fuzz-format-crash.glyph");
    let err = glyph_parser::parse(&src).expect_err("this input is not a valid program");
    assert_eq!(err.code(), "E0011", "got: {err}");
}

/// Recovery must not overflow either. Both inputs carry thousands of unbalanced
/// brackets after the point the limit is crossed, so anything that re-descended
/// from there would abort the same way the original did.
#[test]
fn the_fuzz_inputs_are_rejected_on_a_thin_stack() {
    for name in ["g229-fuzz-parse-crash.glyph", "g229-fuzz-format-crash.glyph"] {
        let src = fixture(name);
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || glyph_parser::parse(&src).is_err())
            .expect("spawn");
        assert!(handle.join().expect("no panic"), "{name} should not parse");
    }
}

// ---------------------------------------------------------------------------
// At the limit, and one past it
// ---------------------------------------------------------------------------

#[test]
fn an_expression_at_the_limit_parses() {
    glyph_parser::parse(&nested_array(MAX)).expect("an expression at the limit parses");
}

#[test]
fn an_expression_past_the_limit_is_e0011() {
    assert_eq!(parse_err_code(&nested_array(MAX + 1)), "E0011");
}

#[test]
fn a_type_at_the_limit_parses() {
    glyph_parser::parse(&nested_generic(MAX)).expect("a type at the limit parses");
}

#[test]
fn a_type_past_the_limit_is_e0011() {
    assert_eq!(parse_err_code(&nested_generic(MAX + 1)), "E0011");
}

#[test]
fn a_pattern_at_the_limit_parses() {
    glyph_parser::parse(&nested_pattern(MAX - 2)).expect("a pattern at the limit parses");
}

#[test]
fn a_pattern_past_the_limit_is_e0011() {
    assert_eq!(parse_err_code(&nested_pattern(MAX - 1)), "E0011");
}

/// The message names both the limit and what was being entered, and the help
/// says what to do about it. A depth error that only said "too deep" would send
/// the reader looking for a setting to raise.
#[test]
fn the_message_names_the_limit_and_the_construct() {
    let err = glyph_parser::parse(&nested_array(MAX + 1)).expect_err("past the limit");
    let text = err.to_string();
    assert!(text.contains("array literal"), "{text}");
    assert!(text.contains(&MAX.to_string()), "{text}");
    let help = err.help().expect("E0011 has help").to_string();
    assert!(help.contains("let"), "{help}");
}

/// The span points at the bracket that would have opened the level the parser
/// refused, not at the whole file: the `[` at index `MAX` of the run, which
/// starts after `const x = `.
#[test]
fn the_span_is_where_the_limit_is_crossed() {
    let src = nested_array(MAX + 1);
    let err = glyph_parser::parse(&src).expect_err("past the limit");
    let start = err.span().start as usize;
    assert_eq!(&src[start..start + 1], "[");
    assert_eq!(start, "const x = ".len() + MAX);
}

// ---------------------------------------------------------------------------
// The limit against the stack it has to fit in
// ---------------------------------------------------------------------------

/// The thinnest stack the parser runs on is a spawned thread's 2 MiB: the
/// language server's tokio workers get that, and so does each test. A limit set
/// too high would not fail anything here in the ordinary way, it would abort
/// the test binary, which is the point.
///
/// Measured cost is about 8,810 bytes a level in this build, so 64 levels is
/// roughly 580 KiB of the 2 MiB.
#[test]
fn the_limit_fits_a_two_megabyte_thread_stack() {
    let src = nested_array(MAX);
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || glyph_parser::parse(&src).is_ok())
        .expect("spawn");
    assert!(handle.join().expect("no panic"), "a program at the limit must parse");
}

/// Depth is per-construct nesting, not a budget spent over the whole file. A
/// thousand sibling arrays each a few levels deep is ordinary generated data
/// and must keep parsing.
#[test]
fn sibling_constructs_do_not_accumulate_depth() {
    let one = "[[[1]]]";
    let src = format!("const x = [{}]\n", vec![one; 1000].join(", "));
    glyph_parser::parse(&src).expect("siblings do not accumulate");
}
