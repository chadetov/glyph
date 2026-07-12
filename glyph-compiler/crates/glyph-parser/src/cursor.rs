//! Token cursor: peek/advance/expect over a `Vec<Spanned<Token>>`.

use glyph_lexer::{Span, Spanned, Token};

use crate::error::ParseError;

pub(crate) struct Cursor<'a> {
    tokens: Vec<Spanned<Token>>,
    pos: usize,
    /// Original source string. Used for JSX text-run reconstruction (D6) —
    /// the parser slices `source[start..end]` between tags to recover the
    /// raw text content that the tokenizer split into multiple tokens.
    source: &'a str,
}

impl<'a> Cursor<'a> {
    pub fn new(tokens: Vec<Spanned<Token>>, source: &'a str) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
        }
    }

    /// Slice the source between two byte offsets. Used by the JSX text-run
    /// reconstructor to recover whitespace and punctuation that the token
    /// stream alone doesn't preserve.
    pub fn slice(&self, start: u32, end: u32) -> &str {
        &self.source[start as usize..end as usize]
    }

    /// Walk `start` left over same-line whitespace (spaces/tabs, never a
    /// newline). Used by the JSX text-run reconstructor to recover a
    /// significant leading space that sits between a preceding `{expr}`/tag and
    /// the first text token but produced no token of its own.
    pub fn extend_left_over_inline_ws(&self, mut start: u32) -> u32 {
        let bytes = self.source.as_bytes();
        while start > 0 && matches!(bytes[start as usize - 1], b' ' | b'\t') {
            start -= 1;
        }
        start
    }

    /// Walk `end` right over same-line whitespace (spaces/tabs, never a
    /// newline). The dual of `extend_left_over_inline_ws`, for a significant
    /// trailing space before a following `{expr}`/tag.
    pub fn extend_right_over_inline_ws(&self, mut end: u32) -> u32 {
        let bytes = self.source.as_bytes();
        while (end as usize) < bytes.len() && matches!(bytes[end as usize], b' ' | b'\t') {
            end += 1;
        }
        end
    }

    pub fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    pub fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset).map(|s| &s.token)
    }

    pub fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    pub fn peek_span_at(&self, offset: usize) -> Option<Span> {
        self.tokens.get(self.pos + offset).map(|s| s.span)
    }

    pub fn advance(&mut self) -> &Spanned<Token> {
        let i = self.pos;
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        &self.tokens[i]
    }

    pub fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    /// Consume any number of leading `Newline` tokens. Used between statements
    /// and between top-level items.
    pub fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    pub fn check(&self, t: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(t)
    }

    pub fn matches(&mut self, t: &Token) -> bool {
        if self.check(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, t: &Token, expected: &'static str) -> Result<Span, ParseError> {
        if self.check(t) {
            Ok(self.advance().span)
        } else {
            Err(ParseError::Expected {
                expected,
                found: format!("{:?}", self.peek()),
                span: self.peek_span(),
            })
        }
    }

    /// Consume an identifier and return its name + span.
    pub fn expect_ident(&mut self, expected: &'static str) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok((name, span))
            }
            other => Err(ParseError::Expected {
                expected,
                found: format!("{other:?}"),
                span,
            }),
        }
    }

    /// Consume an identifier-like name (identifier OR a keyword in field-name
    /// position). Used for record field names, object literal keys, named
    /// import items, etc. — anywhere a keyword may legitimately appear as a
    /// non-keyword identifier.
    pub fn expect_field_name(
        &mut self,
        expected: &'static str,
    ) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok((name, span))
            }
            ref other => {
                if let Some(text) = other.as_field_name() {
                    self.advance();
                    Ok((std::sync::Arc::from(text), span))
                } else {
                    Err(ParseError::Expected {
                        expected,
                        found: format!("{other:?}"),
                        span,
                    })
                }
            }
        }
    }

    /// Consume a field-name and greedily join `-segment` runs when the tokens
    /// are byte-contiguous (no intervening whitespace). This recovers hyphenated
    /// names that the lexer splits on `-` (`Minus`): JSX attribute names like
    /// `aria-label`/`data-testid` and npm package specifiers like
    /// `react-hook-form`. Whitespace on either side of the `-` stops the join, so
    /// subtraction in every other position is unaffected — the join only fires in
    /// name position (JSX names, module/import path segments), where a contiguous
    /// `ident-ident` is unambiguously one hyphenated name.
    pub fn expect_hyphenated_name(
        &mut self,
        expected: &'static str,
    ) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        let (first, first_span) = self.expect_field_name(expected)?;
        let mut name = String::from(first.as_ref());
        let start = first_span.start;
        let mut end = first_span.end;
        loop {
            if !matches!(self.peek(), Token::Minus) {
                break;
            }
            let minus_span = self.peek_span();
            // The `-` must be adjacent to the preceding segment (no whitespace).
            if minus_span.start != end {
                break;
            }
            // The token after `-` must be an adjacent field-name.
            let Some(after) = self.peek_at(1) else { break };
            let is_name =
                matches!(after, Token::Identifier(_)) || after.as_field_name().is_some();
            if !is_name {
                break;
            }
            let Some(after_span) = self.peek_span_at(1) else {
                break;
            };
            if after_span.start != minus_span.end {
                break;
            }
            // Commit: consume `-` and the following segment.
            self.advance();
            let (seg, seg_span) = self.expect_field_name(expected)?;
            name.push('-');
            name.push_str(seg.as_ref());
            end = seg_span.end;
        }
        Ok((std::sync::Arc::from(name.as_str()), Span::new(start, end)))
    }

    /// Parse a comma-separated list ending at `terminator`. Optionally skips
    /// newlines between items (set `skip_newlines` to `true` for things like
    /// argument lists where line breaks are common; false for type argument
    /// lists which are single-line by convention).
    ///
    /// Does NOT consume the terminator — the caller still calls `expect` to
    /// produce a span and a useful error if it's missing.
    pub fn parse_comma_separated<T>(
        &mut self,
        terminator: &Token,
        skip_newlines: bool,
        mut item: impl FnMut(&mut Cursor<'a>) -> Result<T, ParseError>,
    ) -> Result<Vec<T>, ParseError> {
        let mut items = Vec::new();
        if skip_newlines {
            self.skip_newlines();
        }
        while !self.check(terminator) {
            items.push(item(self)?);
            if skip_newlines {
                self.skip_newlines();
            }
            if self.matches(&Token::Comma) {
                if skip_newlines {
                    self.skip_newlines();
                }
            } else {
                break;
            }
        }
        if skip_newlines {
            self.skip_newlines();
        }
        Ok(items)
    }

}
