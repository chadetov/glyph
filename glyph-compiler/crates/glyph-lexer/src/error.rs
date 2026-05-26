//! Lexer error type.

use crate::Span;

#[derive(Debug, thiserror::Error)]
pub enum LexError {
    #[error("unexpected character `{ch}` at offset {offset}")]
    UnexpectedChar { ch: char, offset: u32 },

    #[error("unterminated string literal starting at offset {offset}")]
    UnterminatedString { offset: u32 },

    #[error("invalid escape sequence `\\{ch}` at offset {offset}")]
    InvalidEscape { ch: char, offset: u32 },

    #[error("malformed numeric literal at offset {offset}")]
    MalformedNumber { offset: u32 },

    #[error("nested block comment not supported at offset {offset} (D14: `//` only)")]
    NoBlockComments { offset: u32 },
}

impl LexError {
    pub fn span(&self) -> Span {
        let start = match self {
            LexError::UnexpectedChar { offset, .. } => *offset,
            LexError::UnterminatedString { offset } => *offset,
            LexError::InvalidEscape { offset, .. } => *offset,
            LexError::MalformedNumber { offset } => *offset,
            LexError::NoBlockComments { offset } => *offset,
        };
        Span::new(start, start + 1)
    }
}
