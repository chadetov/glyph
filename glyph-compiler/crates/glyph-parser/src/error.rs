//! Parser error type. Phase 0 / Phase 1 week 1 has minimal recovery; week 7
//! is the Elm-quality error-message audit (Q6 resolution).

use glyph_lexer::Span;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("lex error: {message}")]
    Lex { message: String, span: Span },

    #[error("expected {expected}, found {found}")]
    Expected {
        expected: &'static str,
        found: String,
        span: Span,
    },

    #[error("unexpected token: {found}")]
    Unexpected { found: String, span: Span },

    #[error("expected end of file, but more tokens remain")]
    ExpectedEof { span: Span },

    #[error("not yet implemented in this slice")]
    NotImplemented { span: Span },

    /// A TypeScript conditional keyword (`if`/`else`) used where Glyph has none.
    /// D3 makes `match` the only conditional. Carried as its own variant so the
    /// highest-traffic mistake a TS-trained author makes gets a targeted fix
    /// instead of a generic "unexpected token".
    #[error("Glyph has no `{keyword}`")]
    NoConditionalKeyword { keyword: &'static str, span: Span },

    /// A range / comparison pattern (`500..599 =>`) in a match arm. The `..`
    /// token lexes but has no meaning in pattern position in v1. Carried as
    /// its own variant so the author gets "range patterns aren't supported"
    /// instead of a misleading "expected `=>`" against the `DotDot` token.
    #[error("range patterns (e.g. `500..599`) are not supported in v1")]
    UnsupportedRangePattern { span: Span },
}

impl ParseError {
    pub fn span(&self) -> Span {
        match self {
            ParseError::Lex { span, .. }
            | ParseError::Expected { span, .. }
            | ParseError::Unexpected { span, .. }
            | ParseError::ExpectedEof { span }
            | ParseError::NotImplemented { span }
            | ParseError::NoConditionalKeyword { span, .. }
            | ParseError::UnsupportedRangePattern { span } => *span,
        }
    }

    /// Stable diagnostic code (parser range `E000x`; see
    /// `docs/error-codes.md`).
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::Lex { .. } => "E0001",
            ParseError::Expected { .. } => "E0002",
            ParseError::Unexpected { .. } => "E0003",
            ParseError::ExpectedEof { .. } => "E0004",
            ParseError::NotImplemented { .. } => "E0005",
            ParseError::NoConditionalKeyword { .. } => "E0006",
            ParseError::UnsupportedRangePattern { .. } => "E0007",
        }
    }

    /// A one-line, actionable fix.
    pub fn help(&self) -> Option<&'static str> {
        Some(match self {
            ParseError::Lex { .. } => {
                "Check for an unterminated string, an invalid escape, or a stray character."
            }
            ParseError::Expected { .. } => {
                "Add the expected token. Glyph is deliberately stricter than TypeScript (e.g. trailing commas required, no `if`/`else`)."
            }
            ParseError::Unexpected { .. } => "Remove or correct this token; it can't appear here.",
            ParseError::ExpectedEof { .. } => {
                "Only declarations appear at the top level. Check for a missing brace or an extra token."
            }
            ParseError::NotImplemented { .. } => "This construct is not supported yet.",
            ParseError::NoConditionalKeyword { .. } => {
                "Glyph has no `if`/`else` (D3); `match` is the only conditional — e.g. `match cond { true => a, false => b }`."
            }
            ParseError::UnsupportedRangePattern { .. } => {
                "Range and comparison patterns aren't in v1. Enumerate the values as separate arms (`429 => ..., 500 => ...,`) or match a guard-less scrutinee, e.g. a boolean derived from a comparison."
            }
        })
    }
}
