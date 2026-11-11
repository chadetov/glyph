//! The parser must not panic on any input, and the error it reports never
//! starts after the source's first lex error.
//!
//! It is the one component reachable from untrusted bytes: an editor, an LSP
//! client, and an agent all hand it text nobody wrote by hand. A rejection is a
//! `ParseError` and a value; a panic is a crashed language server and an
//! unusable editor, so it asserts that the function returns, and what the
//! error it returns says about the source (below).
#![no_main]
use libfuzzer_sys::fuzz_target;

// The parser pulls tokens on demand and reports the first error it reaches
// (record 91): a parse error before the first lex error wins, and a lex error
// is reported only when the parser arrives at it, exactly as the eager lexer
// reported it. Lookahead that reaches a lex error backs off without failing.
// So when `tokenize` fails at offset L, `parse` fails at or before L, and a
// reported lex error is `tokenize`'s.

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let parsed = glyph_parser::parse(src);
    let Err(lex) = glyph_lexer::tokenize(src) else {
        return;
    };
    let Err(err) = parsed else {
        panic!("tokenize fails at {:?} and parse succeeds", lex.span());
    };
    assert!(
        err.span().start <= lex.span().start,
        "parse reports {err:?} after the first lex error at {:?}",
        lex.span()
    );
    if let glyph_parser::ParseError::Lex { message, span } = &err {
        assert_eq!(*span, lex.span(), "a reported lex error has tokenize's span");
        assert_eq!(*message, lex.to_string(), "and tokenize's message");
    }
});
