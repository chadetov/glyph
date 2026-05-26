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

    pub fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    pub fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset).map(|s| &s.token)
    }

    pub fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
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
