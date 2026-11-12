//! Lexer implementation. Char-cursor with bracket-depth state for D1.

use std::sync::Arc;

use crate::error::{JsxTextError, JsxTextRefusal, LexError};
use crate::token::{Spanned, Token};
use crate::{Comment, Span};

/// Public entry point: tokenize a source string into a vector of `Spanned<Token>`,
/// ending with `Token::Eof`. Errors return on the first lexical failure.
///
/// This is the eager form of the same lexer the parser pulls from one token
/// at a time: it is a loop over [`Lexer::next_token`], so the two cannot
/// produce different streams.
pub fn tokenize(source: &str) -> Result<Vec<Spanned<Token>>, LexError> {
    Lexer::new(source).tokenize_all()
}

/// Everything the lexer carries from one token to the next, apart from its
/// byte offset. Saved after a token, it is what [`Lexer::at`] needs to resume
/// lexing exactly where that token ended and produce the same tokens the
/// uninterrupted lexer would have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LexState {
    /// Open `(`, `[` and `{` not yet closed. A newline is a token only at
    /// depth 0 (D1).
    pub bracket_depth: u32,
}

pub struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    bracket_depth: u32,
    comments: Vec<Comment>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self::at(source, 0, LexState::default())
    }

    /// A lexer that starts at byte `offset` of `source` in `state`. Given the
    /// end of a token and the [`LexState`] the lexer was in after producing
    /// it, the tokens that follow are the ones lexing from the start of the
    /// file would have produced. Comments are collected from `offset` on.
    pub fn at(source: &'a str, offset: usize, state: LexState) -> Self {
        Self {
            source,
            pos: offset.min(source.len()),
            bracket_depth: state.bracket_depth,
            comments: Vec::new(),
        }
    }

    /// The state after the last token produced, to resume from with
    /// [`Lexer::at`] at that token's end.
    pub fn state(&self) -> LexState {
        LexState {
            bracket_depth: self.bracket_depth,
        }
    }

    /// The `//` comments skipped since the last call, in source order. The
    /// parser's cursor takes them after each token, so every comment belongs
    /// to the token it preceded, and a comment the parser later discards
    /// with that token (a rescan as JSX text) is discarded with it.
    pub fn take_comments(&mut self) -> Vec<Comment> {
        std::mem::take(&mut self.comments)
    }

    /// Scan one run of JSX text from the current position (D6): every byte
    /// up to the next `{` or `<`, or the end of the source, newlines
    /// included. The parser calls this where its grammar says a text run
    /// starts, after a tag or an `{expr}` child; the lexer never decides that
    /// on its own (record 85).
    ///
    /// The run is raw source. No `Newline` token is produced, no comment is
    /// recorded and the bracket depth is untouched, so normal lexing resumes
    /// after it in the state it was in before. An empty run is `None`.
    ///
    /// Four things are refused rather than read. `}` and `>` would end an
    /// `{expr}` or a tag that is not open, as TSX refuses them. `//` is read
    /// as a comment everywhere else in Glyph and as text by TSX, and was a
    /// comment in JSX text before 0.1.135, so either reading would change
    /// what some program means; refusing it means nobody has to know which
    /// one applies. An entity shape (`&amp;`, `&#38;`) is decoded by TSX and
    /// would be emitted as written here, so it is refused for the same reason
    /// (record 93). On a refusal the position is left at the refused bytes.
    pub fn scan_jsx_text(&mut self) -> Result<Option<Spanned<Token>>, JsxTextError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            let refusal = match c {
                b'{' | b'<' => break,
                b'}' => Some((JsxTextRefusal::CloseBrace, 1)),
                b'>' => Some((JsxTextRefusal::CloseAngle, 1)),
                b'/' if self.peek_at(1) == Some(b'/') => Some((JsxTextRefusal::LineComment, 2)),
                b'&' => self.entity_len().map(|len| (JsxTextRefusal::Entity, len)),
                _ => None,
            };
            if let Some((refusal, len)) = refusal {
                return Err(JsxTextError {
                    refusal,
                    span: Span::new(self.pos as u32, (self.pos + len) as u32),
                });
            }
            self.advance();
        }
        if self.pos == start {
            return Ok(None);
        }
        let text = self.source[start..self.pos].to_string();
        Ok(Some(self.spanned(Token::JsxText(text), start, self.pos)))
    }

    /// At an `&`: the length of the entity shape it starts, `&` then one or
    /// more of `[A-Za-z0-9_#]` then `;`, or `None` for an `&` that is text.
    /// The shape is wider than the entities TSX decodes, unknown names
    /// included, so no list of names decides what is refused (record 93).
    fn entity_len(&self) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let mut i = self.pos + 1;
        while bytes.get(i).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'#') {
            i += 1;
        }
        (i > self.pos + 1 && bytes.get(i) == Some(&b';')).then_some(i + 1 - self.pos)
    }

    pub fn tokenize_all(mut self) -> Result<Vec<Spanned<Token>>, LexError> {
        let mut out = Vec::new();
        loop {
            let spanned = self.next_token()?;
            let is_eof = matches!(spanned.token, Token::Eof);
            out.push(spanned);
            if is_eof {
                break;
            }
        }
        Ok(out)
    }

    /// The next token, skipping whitespace, comments and newlines that are
    /// not significant. At the end of the source this returns `Token::Eof`,
    /// and keeps returning it. After an error the lexer's position is
    /// unspecified and it should not be asked for more.
    pub fn next_token(&mut self) -> Result<Spanned<Token>, LexError> {
        loop {
            if let Some(spanned) = self.next_raw()? {
                return Ok(spanned);
            }
        }
    }

    /// One step: a token, or `None` when the step only skipped something.
    fn next_raw(&mut self) -> Result<Option<Spanned<Token>>, LexError> {
        self.skip_inline_whitespace();

        let start = self.pos;
        let Some(ch) = self.peek() else {
            return Ok(Some(self.spanned(Token::Eof, start, start)));
        };

        // Newlines: significant only at bracket_depth == 0 (D1).
        if ch == b'\n' {
            self.advance();
            if self.bracket_depth == 0 {
                return Ok(Some(self.spanned(Token::Newline, start, self.pos)));
            }
            return Ok(None);
        }

        // Line comments (D14): // to end of line, not consuming the newline.
        if ch == b'/' && self.peek_at(1) == Some(b'/') {
            self.skip_line_comment();
            return Ok(None);
        }

        // Block comment guard (D14 forbids them).
        if ch == b'/' && self.peek_at(1) == Some(b'*') {
            return Err(LexError::NoBlockComments { offset: start as u32 });
        }

        // String literals (D12; template interpolation parsing deferred).
        if ch == b'"' {
            return self.lex_string(start).map(Some);
        }

        // Numeric literals (D13).
        if ch.is_ascii_digit() {
            return self.lex_number(start).map(Some);
        }

        // Identifiers and keywords.
        if is_ident_start(ch) {
            return Ok(Some(self.lex_identifier_or_keyword(start)));
        }

        // Multi-char and single-char punctuation.
        self.lex_punctuation(start).map(Some)
    }

    // -- Atoms ----------------------------------------------------------------

    fn lex_string(&mut self, start: usize) -> Result<Spanned<Token>, LexError> {
        // Triple-quoted string: raw content, no escape processing, read verbatim
        // until the closing `"""`. Introduced for D26 `@doc """ ... """` blocks
        // but accepted in ANY string position, which D12 now writes down. A
        // plain empty string `""` is not affected — the third `"` must be
        // present.
        if self.peek_at(1) == Some(b'"') && self.peek_at(2) == Some(b'"') {
            return self.lex_triple_string(start);
        }
        self.advance(); // opening "
        let mut content = String::new();

        loop {
            let Some(ch) = self.peek() else {
                return Err(LexError::UnterminatedString { offset: start as u32 });
            };
            match ch {
                b'"' => {
                    self.advance();
                    let end = self.pos;
                    return Ok(self.spanned(Token::String(content), start, end));
                }
                b'\\' => {
                    self.advance();
                    let escape_offset = self.pos as u32 - 1;
                    let Some(esc) = self.peek() else {
                        return Err(LexError::UnterminatedString { offset: start as u32 });
                    };
                    self.advance();
                    match esc {
                        b'n' => content.push('\n'),
                        b't' => content.push('\t'),
                        b'r' => content.push('\r'),
                        b'"' => content.push('"'),
                        b'\\' => content.push('\\'),
                        // `\$` is a literal dollar (D22): the internal marker
                        // before it keeps a literal `${` distinct from a `${...}`
                        // interpolation. The parser resolves the pair back to
                        // `$` when building the AST.
                        b'$' => {
                            content.push(crate::ESCAPED_DOLLAR);
                            content.push('$');
                        }
                        b'u' => {
                            // \u{HEX}
                            if self.peek() != Some(b'{') {
                                return Err(LexError::InvalidEscape {
                                    ch: 'u',
                                    offset: escape_offset,
                                });
                            }
                            self.advance(); // {
                            let hex_start = self.pos;
                            while let Some(c) = self.peek() {
                                if c.is_ascii_hexdigit() {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            let hex = &self.source[hex_start..self.pos];
                            if hex.is_empty() || self.peek() != Some(b'}') {
                                return Err(LexError::InvalidEscape {
                                    ch: 'u',
                                    offset: escape_offset,
                                });
                            }
                            self.advance(); // }
                            let code = u32::from_str_radix(hex, 16).map_err(|_| {
                                LexError::InvalidEscape { ch: 'u', offset: escape_offset }
                            })?;
                            let c = char::from_u32(code).ok_or(LexError::InvalidEscape {
                                ch: 'u',
                                offset: escape_offset,
                            })?;
                            crate::push_literal(&mut content, c);
                        }
                        other => {
                            return Err(LexError::InvalidEscape {
                                ch: other as char,
                                offset: escape_offset,
                            });
                        }
                    }
                }
                _ => {
                    // D12: embedded raw newlines preserved verbatim.
                    let bytes_start = self.pos;
                    while let Some(c) = self.peek() {
                        if c == b'"' || c == b'\\' {
                            break;
                        }
                        self.advance();
                    }
                    for c in self.source[bytes_start..self.pos].chars() {
                        crate::push_literal(&mut content, c);
                    }
                }
            }
        }
    }

    fn lex_number(&mut self, start: usize) -> Result<Spanned<Token>, LexError> {
        // D13: /-?\d+(_\d+)*(\.\d+(_\d+)*)?/ with optional e exponent.
        // Leading - is handled by the parser as a prefix unary op, not by the lexer.
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'_' {
                self.advance();
            } else {
                break;
            }
        }

        // Track whether this is an integer: a `123n` bigint suffix is only legal
        // on an integer (JS rejects `1.5n` and `1e5n`).
        let mut is_integer = true;

        // Optional fractional part.
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_integer = false;
            self.advance(); // .
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == b'_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Optional exponent.
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_integer = false;
            self.advance();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            let exp_start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            if self.pos == exp_start {
                return Err(LexError::MalformedNumber { offset: start as u32 });
            }
        }

        // Optional `n` bigint suffix (`123n`, `1_000n`). Emitted verbatim as a
        // TypeScript BigInt literal; the value is typed `bigint` by `tsc`. Only
        // an integer may carry it.
        if is_integer && self.peek() == Some(b'n') {
            self.advance();
        }

        let text = self.source[start..self.pos].to_string();
        // D13 digit separators: every `_` must sit between two digits. The digit
        // runs above consume `_` loosely (so `1_`, `1__0`, `1_.5`, `1e_5` all
        // lex), which would otherwise leak a raw `tsc` TS6188/TS6189 at the seam.
        // Reject them here with a Glyph-level diagnostic instead.
        let bytes = text.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'_' {
                let prev_ok = i > 0 && bytes[i - 1].is_ascii_digit();
                let next_ok = bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
                if !prev_ok || !next_ok {
                    return Err(LexError::MalformedNumber { offset: start as u32 });
                }
            }
        }
        Ok(self.spanned(Token::Number(text), start, self.pos))
    }

    fn lex_identifier_or_keyword(&mut self, start: usize) -> Spanned<Token> {
        while let Some(c) = self.peek() {
            if is_ident_cont(c) {
                self.advance();
            } else {
                break;
            }
        }
        let text = &self.source[start..self.pos];
        let token = match Token::keyword(text) {
            Some(kw) => kw,
            None => Token::Identifier(Arc::from(text)),
        };
        self.spanned(token, start, self.pos)
    }

    fn lex_punctuation(&mut self, start: usize) -> Result<Spanned<Token>, LexError> {
        let ch = self.peek().expect("caller verified non-empty");

        // Multi-char first (longest match).
        macro_rules! two {
            ($second:expr, $tok:expr) => {
                if self.peek_at(1) == Some($second) {
                    self.advance();
                    self.advance();
                    return Ok(self.spanned($tok, start, self.pos));
                }
            };
        }

        match ch {
            b'-' => two!(b'>', Token::Arrow),
            b'=' => {
                two!(b'>', Token::FatArrow);
                two!(b'=', Token::EqEq);
            }
            b'!' => {
                two!(b'=', Token::BangEq);
            }
            b'<' => {
                two!(b'=', Token::LtEq);
            }
            b'>' => {
                two!(b'=', Token::GtEq);
            }
            b'&' => {
                two!(b'&', Token::AmpAmp);
            }
            b'|' => {
                two!(b'|', Token::PipePipe);
            }
            b'?' => {
                two!(b'?', Token::QQ);
                two!(b'.', Token::QDot);
            }
            b'.' => {
                if self.peek_at(1) == Some(b'.') && self.peek_at(2) == Some(b'.') {
                    self.advance();
                    self.advance();
                    self.advance();
                    return Ok(self.spanned(Token::DotDotDot, start, self.pos));
                }
                two!(b'.', Token::DotDot);
            }
            _ => {}
        }

        // Single-char.
        self.advance();
        let token = match ch {
            b'(' => {
                self.bracket_depth += 1;
                Token::LParen
            }
            b')' => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                Token::RParen
            }
            b'[' => {
                self.bracket_depth += 1;
                Token::LBracket
            }
            b']' => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                Token::RBracket
            }
            b'{' => {
                self.bracket_depth += 1;
                Token::LBrace
            }
            b'}' => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                Token::RBrace
            }
            // D1 note: `<>` are NOT counted toward bracket depth at the lexer level.
            // The parser tracks generic-arg vs less-than. Newline behavior is
            // unaffected.
            b'<' => Token::LAngle,
            b'>' => Token::RAngle,
            b',' => Token::Comma,
            b':' => Token::Colon,
            b'.' => Token::Dot,
            b'+' => Token::Plus,
            b'-' => Token::Minus,
            b'*' => Token::Star,
            b'/' => Token::Slash,
            b'%' => Token::Percent,
            b'!' => Token::Bang,
            b'|' => Token::Pipe,
            b'&' => Token::Amp,
            b'^' => Token::Caret,
            b'~' => Token::Tilde,
            b'?' => Token::Question,
            b'=' => Token::Equals,
            b'@' => Token::At,
            b'$' => Token::Dollar,
            other => {
                return Err(LexError::UnexpectedChar {
                    ch: other as char,
                    offset: start as u32,
                });
            }
        };
        Ok(self.spanned(token, start, self.pos))
    }

    // -- Cursor utilities -----------------------------------------------------

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.source.as_bytes().get(self.pos + offset).copied()
    }

    fn advance(&mut self) {
        if self.pos < self.source.len() {
            self.pos += 1;
        }
    }

    fn skip_inline_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\r' => self.advance(),
                _ => break,
            }
        }
    }

    /// Lex a `""" ... """` triple-quoted string (D12's raw form). Content is raw
    /// — newlines and backslashes verbatim — which suits Markdown `@doc` bodies
    /// and any other text that would otherwise need escaping. The token's value
    /// is the inner text; its span covers both delimiters.
    ///
    /// The result is an ordinary `Token::String`, so nothing downstream can tell
    /// which spelling produced it: `${...}` inside a `"""` literal interpolates
    /// exactly as it does in the `"..."` form, because the parser runs the same
    /// interpolation split over both. "Raw" here means escapes are not decoded,
    /// not that the content is inert.
    fn lex_triple_string(&mut self, start: usize) -> Result<Spanned<Token>, LexError> {
        self.advance();
        self.advance();
        self.advance(); // opening """
        let content_start = self.pos;
        loop {
            let Some(ch) = self.peek() else {
                return Err(LexError::UnterminatedString { offset: start as u32 });
            };
            if ch == b'"' && self.peek_at(1) == Some(b'"') && self.peek_at(2) == Some(b'"') {
                let mut content = String::new();
                for c in self.source[content_start..self.pos].chars() {
                    crate::push_literal(&mut content, c);
                }
                self.advance();
                self.advance();
                self.advance(); // closing """
                let end = self.pos;
                return Ok(self.spanned(Token::String(content), start, end));
            }
            self.advance();
        }
    }

    fn skip_line_comment(&mut self) {
        // We are positioned at the first `/`. Skip until newline (do not consume it).
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'\n' {
                break;
            }
            self.advance();
        }
        let text = self.source[start..self.pos].trim_end().to_string();
        self.comments.push(Comment {
            span: Span::new(start as u32, self.pos as u32),
            text,
        });
    }

    fn spanned(&self, token: Token, start: usize, end: usize) -> Spanned<Token> {
        Spanned {
            token,
            span: Span::new(start as u32, end as u32),
        }
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
