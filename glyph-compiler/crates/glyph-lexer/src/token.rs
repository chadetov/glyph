//! Token enum + `Spanned<T>` carrier.

use std::sync::Arc;

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub token: T,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    // Literals (raw forms; numeric parsing happens later).
    Number(String),
    /// Resolved string content (escapes processed). Template-literal `${...}`
    /// interpolation is not yet parsed; week 1 day 3+ will introduce
    /// `StringStart` / `StringText` / `StringInterp` / `StringEnd` tokens.
    String(String),

    // Identifier (could be a keyword; matched by name).
    Identifier(Arc<str>),

    // -- Keywords --
    Fn,
    Type,
    Record,
    Component,
    Const,
    Let,
    Mut,
    Owned,
    Resource,
    /// D-decision (0.1.16): module-private-by-default visibility. A top-level
    /// declaration is exported only when marked `pub`.
    Pub,
    /// D-decision (0.1.16): a structural interface, usable as a generic bound.
    Interface,
    /// D-decision (0.1.16): `defer <expr>` runs its expression on scope exit
    /// (lowered to `try`/`finally`).
    Defer,
    Module,
    Import,
    As,
    Match,
    Else,
    For,
    Loop,
    Break,
    Continue,
    In,
    Async,
    Await,
    New,
    Return,
    Is,
    True,
    False,
    Void,
    /// Reserved; D3 says no value-level `if`, but `<if>` is a JSX directive.
    If,

    // -- Annotation prefix (D27) --
    At,

    // -- Punctuation, single --
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    /// `<` — may be less-than or generic-arg open. Parser disambiguates per D7.
    LAngle,
    /// `>` — may be greater-than or generic-arg close. Parser disambiguates.
    RAngle,
    Comma,
    Colon,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    /// `|` — tagged union separator (D8) in type/pattern position, bitwise-OR in
    /// expression position.
    Pipe,
    /// `&` — bitwise-AND (expression position). `&&` is the separate `AmpAmp`.
    Amp,
    /// `^` — bitwise-XOR.
    Caret,
    /// `~` — bitwise-NOT (prefix unary).
    Tilde,
    Question,
    Equals,
    /// `$` — has no operator meaning in Glyph. It is emitted only so a bare
    /// `$` inside JSX text (e.g. a price like `$5`) lexes cleanly; JSX text is
    /// reconstructed from the source slice, so the token value is never read.
    /// In any other position the parser rejects it as an unexpected token.
    Dollar,

    // -- Punctuation, multi-char --
    Arrow,      // ->
    FatArrow,   // =>
    EqEq,       // ==
    BangEq,     // !=
    LtEq,       // <=
    GtEq,       // >=
    AmpAmp,     // &&
    PipePipe,   // ||
    QQ,         // ??
    QDot,       // ?.
    DotDot,     // ..  (range; see step-2 corpus)
    DotDotDot,  // ... (spread, D11; rest patterns, D9)

    /// Significant newline (D1). Emitted only when bracket depth is zero.
    Newline,

    /// End of file. Always the last token.
    Eof,
}

impl Token {
    /// Return the source text for tokens that can serve as identifier-like
    /// names in field-name position (object keys, record field names, named
    /// import items, etc.). Returns `None` for tokens that cannot.
    ///
    /// This is what makes `{ resource: string }` and `{ type: "post" }` work
    /// even though `resource` and `type` are Glyph keywords. Property-name
    /// position is contextual: the parser calls this helper when it wants
    /// "any identifier-like token here."
    pub fn as_field_name(&self) -> Option<&'static str> {
        KEYWORDS
            .iter()
            .find(|(_, sample)| std::mem::discriminant(self) == std::mem::discriminant(sample))
            .map(|(text, _)| *text)
    }

    /// Lookup table for keyword identifiers.
    pub(crate) fn keyword(ident: &str) -> Option<Token> {
        KEYWORDS
            .iter()
            .find(|(text, _)| *text == ident)
            .map(|(_, tok)| tok.clone())
    }
}

/// Single source of truth for keyword text ↔ `Token` variant mapping.
/// `as_field_name` walks this list in the variant→text direction;
/// `keyword` walks it in the text→variant direction. Drift between the two
/// is structurally impossible.
///
/// Note: the variant in each pair is just a *sample* used for discriminant
/// comparison — its inner payload (none of these have payloads anyway) is
/// irrelevant.
static KEYWORDS: &[(&str, Token)] = &[
    ("fn", Token::Fn),
    ("type", Token::Type),
    ("record", Token::Record),
    ("component", Token::Component),
    ("const", Token::Const),
    ("let", Token::Let),
    ("mut", Token::Mut),
    ("owned", Token::Owned),
    ("resource", Token::Resource),
    ("pub", Token::Pub),
    ("interface", Token::Interface),
    ("defer", Token::Defer),
    ("module", Token::Module),
    ("import", Token::Import),
    ("as", Token::As),
    ("match", Token::Match),
    ("else", Token::Else),
    ("for", Token::For),
    ("loop", Token::Loop),
    ("break", Token::Break),
    ("continue", Token::Continue),
    ("in", Token::In),
    ("async", Token::Async),
    ("await", Token::Await),
    ("new", Token::New),
    ("return", Token::Return),
    ("is", Token::Is),
    ("true", Token::True),
    ("false", Token::False),
    ("void", Token::Void),
    // `if` is reserved; valid only in JSX directive position.
    ("if", Token::If),
];
