//! Property-based robustness fuzzing (Phase 1 week 8).
//!
//! The lexer and parser must be *total*: on any input they return `Ok` or a
//! proper error, and never panic, loop forever, or corrupt state. These
//! properties don't assert what the result is — only that producing one is
//! safe. Token-soup inputs drive the recursive-descent parser deeper than
//! random Unicode (which mostly exercises the lexer).

use proptest::prelude::*;

/// Random sequences drawn from Glyph's token vocabulary — far likelier to reach
/// deep parser states than arbitrary text.
fn token_soup() -> impl Strategy<Value = String> {
    let tok = prop::sample::select(vec![
        "module", "fn", "component", "type", "const", "let", "mut", "return",
        "import", "match", "for", "in", "loop", "if", "else", "owned", "resource",
        "async", "await", "is", "->", "=>", "{", "}", "(", ")", "[", "]", "<", ">",
        ",", ":", ";", "=", "|", "?", ".", "...", "&&", "||", "==", "+", "-",
        "x", "Foo", "main", "0", "42", "\"s\"", "true", "false", "void", "number", " ", "\n",
    ]);
    prop::collection::vec(tok, 0..60).prop_map(|v| v.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Tokenizing arbitrary text never panics.
    #[test]
    fn tokenize_never_panics(s in any::<String>()) {
        let _ = glyph_lexer::tokenize(&s);
    }

    /// Parsing arbitrary text never panics.
    #[test]
    fn parse_arbitrary_text_never_panics(s in any::<String>()) {
        let _ = glyph_parser::parse(&s);
    }

    /// Parsing token soup never panics (drives the parser, not just the lexer).
    #[test]
    fn parse_token_soup_never_panics(s in token_soup()) {
        let _ = glyph_parser::parse(&s);
    }

    /// A failed parse always carries an in-bounds span (the renderer clamps,
    /// but a span past end-of-input signals a bookkeeping bug worth catching).
    #[test]
    fn parse_error_span_is_in_bounds(s in token_soup()) {
        if let Err(e) = glyph_parser::parse(&s) {
            let span = e.span();
            prop_assert!(span.start <= span.end, "inverted span: {span:?}");
            prop_assert!(span.end as usize <= s.len(), "span past end: {span:?} for len {}", s.len());
        }
    }
}
