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
//!        `"${x}"` as an opaque string)
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
pub use lexer::{comments, tokenize, Lexer};
pub use token::{Spanned, Token};

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
