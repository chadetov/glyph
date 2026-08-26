//! Parser error type. Phase 0 / Phase 1 week 1 has minimal recovery; week 7
//! is the Elm-quality error-message audit (Q6 resolution).

use glyph_lexer::Span;
use std::borrow::Cow;

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

    /// A bare `x = e` assignment with no `mut` (D5). Every mutation in Glyph
    /// is marked, so the assignment form is `mut x = e`. Carried as its own
    /// variant because the generic path reports "unexpected token: Equals",
    /// which names a token instead of the rule the author broke — the same
    /// reason `NoConditionalKeyword` exists for `if`/`else`.
    #[error("assignment requires `mut`")]
    MissingMutOnAssignment { span: Span },

    /// A tagged-union variant given more than one positional payload field
    /// (`Node(Color, Tree<K, V>, K, V, int, Tree<K, V>)`). D8 gives a variant
    /// one payload, and the manifesto's abstraction pillar spells a multi-field
    /// payload as a record: "named records over positional tuples." The parser
    /// always rejected the tuple form, but by falling off `expect(")")` at the
    /// first comma, so the author was told a token was missing rather than that
    /// the construct does not exist. Carried as its own variant so the arity is
    /// counted and the record form is named.
    ///
    /// `fields` holds each positional field as the author wrote it, in order,
    /// and `count` is its length: the message and the help both come from that
    /// one list, so they cannot disagree about how many fields there are.
    #[error("a union variant carries one payload, but `{name}` lists {count} positional fields")]
    MultiFieldVariantPayload {
        name: String,
        count: usize,
        fields: Vec<String>,
        span: Span,
    },
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
            | ParseError::UnsupportedRangePattern { span }
            | ParseError::MissingMutOnAssignment { span }
            | ParseError::MultiFieldVariantPayload { span, .. } => *span,
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
            ParseError::MissingMutOnAssignment { .. } => "E0008",
            ParseError::MultiFieldVariantPayload { .. } => "E0010",
        }
    }

    /// A one-line, actionable fix.
    ///
    /// Most are fixed strings. `Cow` is here for the ones that have to name
    /// what the author actually wrote: an example built from someone else's
    /// program is worse than no example, and a fix that points at a variant the
    /// file does not contain is not actionable.
    pub fn help(&self) -> Option<Cow<'static, str>> {
        Some(match self {
            ParseError::Lex { .. } => Cow::Borrowed(
                "Check for an unterminated string, an invalid escape, or a stray character.",
            ),
            ParseError::Expected { .. } => Cow::Borrowed(
                "Add the expected token. Glyph is deliberately stricter than TypeScript (e.g. trailing commas required, no `if`/`else`).",
            ),
            ParseError::Unexpected { .. } => {
                Cow::Borrowed("Remove or correct this token; it can't appear here.")
            }
            ParseError::ExpectedEof { .. } => Cow::Borrowed(
                "Only declarations appear at the top level. Check for a missing brace or an extra token.",
            ),
            ParseError::NotImplemented { .. } => {
                Cow::Borrowed("This construct is not supported yet.")
            }
            ParseError::NoConditionalKeyword { .. } => Cow::Borrowed(
                "Glyph has no `if`/`else` (D3); `match` is the only conditional — e.g. `match cond { true => a, false => b }`.",
            ),
            ParseError::UnsupportedRangePattern { .. } => Cow::Borrowed(
                "Range and comparison patterns aren't in v1. Enumerate the values as separate arms (`429 => ..., 500 => ...,`) or match a guard-less scrutinee, e.g. a boolean derived from a comparison.",
            ),
            ParseError::MissingMutOnAssignment { .. } => Cow::Borrowed(
                "Glyph marks every mutation (D5): write `mut x = ...` to reassign an existing binding, or `let x = ...` to introduce a new one.",
            ),
            // Built from the author's own variant name and field types. The
            // field names are the one thing the parser cannot supply, so they
            // stay as placeholders; a wrong name is worse than an obvious hole.
            ParseError::MultiFieldVariantPayload { name, fields, .. } => {
                let record = fields
                    .iter()
                    .map(|ty| format!("/* name */: {ty}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                Cow::Owned(format!(
                    "Glyph has no tuple payload. Put the fields in one record and name them: `{name}({{ {record} }})`, and destructure it by those names in a match arm."
                ))
            }
        })
    }
}
