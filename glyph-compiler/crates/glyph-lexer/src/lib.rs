//! Glyph lexer — Phase 1 week 1 (day 1–2 slice).
//!
//! Implements:
//! - D1   significant newlines outside brackets (`bracket_depth` tracking)
//! - D12  one string syntax `"..."` (template interpolation in D22 is opaque for now)
//! - D13  numeric literals with underscore separators
//! - D14  `//` line comments only
//! - D17  trailing commas everywhere (recognized as ordinary commas; parser enforces)
//! - D21  `for` / `loop` / `break` / `continue` keywords
//! - D27  `@<name>` annotation prefix
//!
//! Deferred to week 1 day 3+:
//! - D22  template literal `${expr}` interpolation lexing (current pass treats
//!   `"${x}"` as an opaque string)
//! - JSX `<` recognition (D6 — the parser decides; lexer only emits `LAngle`)
//!
//! Architecture:
//! - `Token` is the closed token enum.
//! - `Spanned<T>` carries a `Span` from `glyph_lexer::Span` (this crate).
//! - `Lexer<'a>` is a char-cursor with bracket-depth state.
//! - `tokenize(source)` is the entry point; returns `Result<Vec<Spanned<Token>>, LexError>`.

#![forbid(unsafe_code)]

mod error;
mod lexer;
mod token;

pub use error::LexError;
pub use lexer::{comments, tokenize, LexState, Lexer};
pub use token::{keywords, Spanned, Token};

/// Internal escape marker in a lexed string's value, so a literal `${`
/// (written `\${`) stays distinguishable from a `${...}` interpolation after
/// escape processing (D22). The marker always comes with the character it
/// escapes: an escaped `\$` is the marker followed by `$`, and the codepoint
/// itself, written raw or as `\u{E000}`, is the marker twice. The parser
/// resolves every pair back to the one character when building the AST, so
/// the marker never leaks past parsing.
///
/// The pairing is what makes the encoding exact. A lone marker standing for
/// `$` once turned a user's own U+E000 into a `$` in the emitted TypeScript.
pub const ESCAPED_DOLLAR: char = '\u{E000}';

/// Push a character from a string's source text onto its lexed value, pairing
/// the marker codepoint with itself (see [`ESCAPED_DOLLAR`]).
pub(crate) fn push_literal(content: &mut String, c: char) {
    if c == ESCAPED_DOLLAR {
        content.push(ESCAPED_DOLLAR);
    }
    content.push(c);
}

/// Replace each [`ESCAPED_DOLLAR`] pair with the character it escapes. Called
/// by the parser wherever a string's lexed value becomes an AST literal, so
/// the marker never escapes the parse phase.
pub fn resolve_escaped_dollars(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains(ESCAPED_DOLLAR) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == ESCAPED_DOLLAR {
            if let Some(escaped) = chars.next() {
                out.push(escaped);
            }
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// A `//` line comment recovered from the source. The lexer skips comments for
/// the token stream (D14), but collects them here so the formatter can preserve
/// them. `text` is the full comment including the leading `//`, trailing
/// whitespace trimmed; `span` is its byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: Span,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Combine two spans into one covering both. Used everywhere a parser
    /// rule starts at one span and ends at another (e.g. `(kw_span | body.span)`).
    pub fn join(self, end: Span) -> Span {
        Span::new(self.start, end.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive a fresh lexer to EOF, keeping the state after every token.
    fn lex_with_states(src: &str) -> Vec<(Spanned<Token>, LexState)> {
        let mut lexer = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lexer.next_token().expect("clean source");
            let eof = matches!(t.token, Token::Eof);
            out.push((t, lexer.state()));
            if eof {
                return out;
            }
        }
    }

    #[test]
    fn lexer_at_resumes_mid_file_with_bracket_depth_carried() {
        // A newline is a token only outside brackets, so resuming inside the
        // call without its depth would emit Newline tokens the uninterrupted
        // lexer does not.
        let src = "fn f() {\n  g(\n    a,\n    b,\n  )\n}\n";
        let full = tokenize(src).unwrap();
        let states = lex_with_states(src);
        let a = full
            .iter()
            .position(|t| matches!(&t.token, Token::Identifier(n) if n.as_ref() == "a"))
            .unwrap();
        assert_eq!(states[a].1, LexState { bracket_depth: 2 });
        let mut resumed = Lexer::at(src, full[a].span.end as usize, states[a].1);
        let mut rest = Vec::new();
        loop {
            let t = resumed.next_token().unwrap();
            let eof = matches!(t.token, Token::Eof);
            rest.push(t);
            if eof {
                break;
            }
        }
        assert_eq!(rest, full[a + 1..]);

        // The same offset with the depth dropped is a different stream.
        let mut wrong = Lexer::at(src, full[a].span.end as usize, LexState::default());
        let _comma = wrong.next_token().unwrap();
        assert!(matches!(wrong.next_token().unwrap().token, Token::Newline));
    }

    #[test]
    fn tokenize_is_the_next_token_loop() {
        let src = "fn f(x: number) -> number {\n  // c\n  x + 1\n}\n";
        let pulled: Vec<_> = lex_with_states(src).into_iter().map(|(t, _)| t).collect();
        assert_eq!(pulled, tokenize(src).unwrap());
    }

    #[test]
    fn next_token_keeps_returning_eof() {
        let mut lexer = Lexer::new("x");
        assert!(matches!(lexer.next_token().unwrap().token, Token::Identifier(_)));
        for _ in 0..3 {
            assert!(matches!(lexer.next_token().unwrap().token, Token::Eof));
        }
    }

    #[test]
    fn empty_source_yields_only_eof() {
        let tokens = tokenize("").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Eof));
    }

    #[test]
    fn comments_are_collected_with_spans() {
        let src = "// header\nfn f() {}\n// trailing\n";
        let cs = comments(src);
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].text, "// header");
        assert_eq!(cs[1].text, "// trailing");
        // Span covers the comment text; trailing whitespace is trimmed from the
        // text but the span runs to end-of-line.
        assert_eq!(&src[cs[0].span.start as usize..cs[0].span.end as usize], "// header");
    }

    #[test]
    fn slashes_inside_a_string_are_not_comments() {
        let cs = comments("const u = \"http://example.com\"\n");
        assert!(cs.is_empty(), "a // inside a string is not a comment: {cs:?}");
    }

    #[test]
    fn triple_quoted_string_is_one_raw_token() {
        let toks = tokenize("\"\"\"line one\nline two\"\"\"\n").unwrap();
        let strings: Vec<&str> = toks
            .iter()
            .filter_map(|t| match &t.token {
                Token::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(strings, vec!["line one\nline two"]);
    }

    #[test]
    fn the_escape_marker_always_comes_in_a_pair() {
        // An escaped `\$` is the marker and `$`; the codepoint itself, raw or
        // escaped, is the marker twice. A lone marker would make the two
        // indistinguishable.
        let m = ESCAPED_DOLLAR;
        let toks = tokenize("\"a\\$b\" \"a\u{E000}b\" \"a\\u{E000}b\" \"\"\"\u{E000}\"\"\"\n").unwrap();
        let strings: Vec<&str> = toks
            .iter()
            .filter_map(|t| match &t.token {
                Token::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        let pair = format!("{m}{m}");
        assert_eq!(
            strings,
            vec![
                format!("a{m}$b").as_str(),
                format!("a{pair}b").as_str(),
                format!("a{pair}b").as_str(),
                pair.as_str(),
            ]
        );
        assert_eq!(resolve_escaped_dollars(strings[0]), "a$b");
        assert_eq!(resolve_escaped_dollars(strings[1]), "a\u{E000}b");
        assert_eq!(resolve_escaped_dollars(strings[3]), "\u{E000}");
    }

    #[test]
    fn empty_string_is_not_triple() {
        let toks = tokenize("\"\" \"x\"\n").unwrap();
        let strings: Vec<&str> = toks
            .iter()
            .filter_map(|t| match &t.token {
                Token::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(strings, vec!["", "x"]);
    }

    #[test]
    fn keyword_recognition() {
        let tokens = tokenize("fn type record component const let mut owned resource").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.token).collect();
        assert!(matches!(kinds[0], Token::Fn));
        assert!(matches!(kinds[1], Token::Type));
        assert!(matches!(kinds[2], Token::Record));
        assert!(matches!(kinds[3], Token::Component));
        assert!(matches!(kinds[4], Token::Const));
        assert!(matches!(kinds[5], Token::Let));
        assert!(matches!(kinds[6], Token::Mut));
        assert!(matches!(kinds[7], Token::Owned));
        assert!(matches!(kinds[8], Token::Resource));
    }

    #[test]
    fn numeric_literal_with_underscores_d13() {
        let tokens = tokenize("1_000_000").unwrap();
        match &tokens[0].token {
            Token::Number(s) => assert_eq!(s, "1_000_000"),
            other => panic!("expected Number, got {other:?}"),
        }
    }

    #[test]
    fn fractional_underscores_lex_d13() {
        let tokens = tokenize("0.000_1").unwrap();
        match &tokens[0].token {
            Token::Number(s) => assert_eq!(s, "0.000_1"),
            other => panic!("expected Number, got {other:?}"),
        }
    }

    #[test]
    fn malformed_underscores_are_rejected_d13() {
        // A `_` must sit between two digits; these leaked a raw tsc TS6188/6189
        // before the lexer validated separator placement.
        for bad in ["1_", "1__0", "1_.5", "1_e5", "1e_5"] {
            assert!(
                matches!(
                    tokenize(bad),
                    Err(crate::error::LexError::MalformedNumber { .. })
                ),
                "expected MalformedNumber for `{bad}`"
            );
        }
    }

    #[test]
    fn bigint_suffix_lexes_on_an_integer() {
        // `123n` and `1_000n` carry the BigInt suffix; it emits verbatim as a TS
        // bigint literal. The suffix is part of the number token's text.
        for (src, want) in [("123n", "123n"), ("1_000n", "1_000n"), ("0n", "0n")] {
            let tokens = tokenize(src).unwrap();
            match &tokens[0].token {
                Token::Number(s) => assert_eq!(s, want, "for `{src}`"),
                other => panic!("expected Number for `{src}`, got {other:?}"),
            }
        }
        // A fractional literal does NOT absorb a following `n` (JS has no
        // `1.5n`): the `n` lexes as a separate identifier.
        let tokens = tokenize("1.5n").unwrap();
        assert!(matches!(&tokens[0].token, Token::Number(s) if s == "1.5"), "1.5 stays a plain number: {:?}", tokens[0].token);
        assert!(matches!(&tokens[1].token, Token::Identifier(s) if s.as_ref() == "n"), "the `n` is a separate ident: {:?}", tokens[1].token);
    }

    #[test]
    fn decimal_literal() {
        let tokens = tokenize("3.14").unwrap();
        match &tokens[0].token {
            Token::Number(s) => assert_eq!(s, "3.14"),
            other => panic!("expected Number, got {other:?}"),
        }
    }

    #[test]
    fn string_literal_with_escapes() {
        let tokens = tokenize(r#""hello\nworld""#).unwrap();
        match &tokens[0].token {
            Token::String(s) => assert_eq!(s, "hello\nworld"),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn line_comment_d14() {
        let tokens = tokenize("// this is a comment\nfn").unwrap();
        // Comment consumed; we expect Newline + Fn + Eof.
        assert!(matches!(tokens[0].token, Token::Newline));
        assert!(matches!(tokens[1].token, Token::Fn));
        assert!(matches!(tokens[2].token, Token::Eof));
    }

    #[test]
    fn significant_newlines_d1_outside_brackets_only() {
        // Inside brackets, newlines are NOT significant.
        let tokens = tokenize("foo(\n  a,\n  b,\n)").unwrap();
        let newline_count = tokens.iter().filter(|t| matches!(t.token, Token::Newline)).count();
        assert_eq!(newline_count, 0, "newlines inside () should be suppressed");

        // Outside brackets, they are.
        let tokens = tokenize("a\nb").unwrap();
        let newline_count = tokens.iter().filter(|t| matches!(t.token, Token::Newline)).count();
        assert_eq!(newline_count, 1, "newline between top-level tokens should be emitted");
    }

    #[test]
    fn multi_char_punctuation() {
        let tokens = tokenize("-> => == != <= >= && || ?? ?. ... ..").unwrap();
        use Token::*;
        let kinds: Vec<_> = tokens.iter().map(|t| t.token.clone()).collect();
        assert_eq!(
            kinds[..12],
            [Arrow, FatArrow, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, QQ, QDot, DotDotDot, DotDot]
        );
    }

    #[test]
    fn annotation_prefix_d27() {
        let tokens = tokenize("@example slugify(\"x\") == \"x\"").unwrap();
        assert!(matches!(tokens[0].token, Token::At));
        // Next token is `example` as an identifier (annotations are `@` + ident at the
        // grammar level; the typechecker dispatches by name).
        match &tokens[1].token {
            Token::Identifier(s) => assert_eq!(s.as_ref(), "example"),
            other => panic!("expected Identifier, got {other:?}"),
        }
    }
}
