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
}
