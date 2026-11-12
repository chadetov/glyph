//! Lexer error type.

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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

/// What JSX text cannot hold (D6, record 85). The parser reports each as
/// E0012 with a help that names the spelling to use instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsxTextRefusal {
    /// `}`: it would close an `{expr}` child that is not open.
    CloseBrace,
    /// `>`: it would end a tag that is not open.
    CloseAngle,
    /// `//`: a comment everywhere else in Glyph and text in TSX.
    LineComment,
    /// `&name;`, `&#38;`, `&#x26;`: an entity TSX would decode and Glyph
    /// would emit as written.
    Entity,
}

/// A refused character in a run of JSX text, with the span of the
/// characters refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsxTextError {
    pub refusal: JsxTextRefusal,
    pub span: Span,
}
