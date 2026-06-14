//! Lexer implementation. Char-cursor with bracket-depth state for D1.

use std::sync::Arc;

use crate::error::LexError;
use crate::token::{Spanned, Token};
use crate::{Comment, Span};

/// Public entry point: tokenize a source string into a vector of `Spanned<Token>`,
/// ending with `Token::Eof`. Errors return on the first lexical failure.
pub fn tokenize(source: &str) -> Result<Vec<Spanned<Token>>, LexError> {
    Lexer::new(source).tokenize_all()
}

/// Collect every `//` line comment in `source`, in source order. Drives the
/// lexer to EOF (or the first lexical error) accumulating comments as they are
/// skipped. Intended for sources that parse, where lexing is clean; on a source
/// with a later lexical error this returns the comments found before it.
pub fn comments(source: &str) -> Vec<Comment> {
    let mut lexer = Lexer::new(source);
    loop {
        match lexer.next_token() {
            Ok(Some(s)) if matches!(s.token, Token::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    lexer.comments
}

pub struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    bracket_depth: u32,
    comments: Vec<Comment>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            bracket_depth: 0,
            comments: Vec::new(),
        }
    }

    pub fn tokenize_all(mut self) -> Result<Vec<Spanned<Token>>, LexError> {
        let mut out = Vec::new();
        loop {
            match self.next_token()? {
                Some(spanned) => {
                    let is_eof = matches!(spanned.token, Token::Eof);
                    out.push(spanned);
                    if is_eof {
                        break;
                    }
                }
                None => continue,
            }
        }
        Ok(out)
    }

    fn next_token(&mut self) -> Result<Option<Spanned<Token>>, LexError> {
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
                        b'$' => content.push('$'), // for `\${` literal dollar-brace per D22
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
                            content.push(c);
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
                    content.push_str(&self.source[bytes_start..self.pos]);
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

        // Optional fractional part.
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
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

        let text = self.source[start..self.pos].to_string();
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
            b'?' => Token::Question,
            b'=' => Token::Equals,
            b'@' => Token::At,
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
        if self.pos < self.source.as_bytes().len() {
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
