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
}

impl ParseError {
    pub fn span(&self) -> Span {
        match self {
            ParseError::Lex { span, .. }
            | ParseError::Expected { span, .. }
            | ParseError::Unexpected { span, .. }
            | ParseError::ExpectedEof { span }
            | ParseError::NotImplemented { span } => *span,
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
        })
    }
}
