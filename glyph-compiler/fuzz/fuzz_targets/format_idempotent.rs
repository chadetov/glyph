//! Formatting twice must equal formatting once.
//!
//! This is the only target here that checks a property rather than the absence
//! of a panic, and it is the one worth the most. Diff stability is a pillar: if
//! `glyph fmt` is not idempotent then a file oscillates between two spellings,
//! every save produces a diff, and `glyph fmt --check` in CI passes on one of
//! them and fails on the other. A formatter round-trip bug once moved comments
//! out of the construct they documented (G23) and the mangled output was itself
//! a fixed point, so the check that would have caught it is this one.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    // Only parseable input has a canonical form; the formatter is not asked
    // for one otherwise.
    let Ok(module) = glyph_parser::parse(src) else {
        return;
    };
    let comments = glyph_lexer::comments(src);
    let once = glyph_formatter::format_module(&module, &comments, src);

    let Ok(reparsed) = glyph_parser::parse(&once) else {
        panic!("formatter produced output that does not parse:\n{once}");
    };
    let recomments = glyph_lexer::comments(&once);
    let twice = glyph_formatter::format_module(&reparsed, &recomments, &once);

    assert_eq!(once, twice, "glyph fmt is not idempotent");
});
