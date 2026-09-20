//! Token cursor: peek/advance/expect over a `Vec<Spanned<Token>>`.

use glyph_lexer::{Span, Spanned, Token};

use crate::error::ParseError;

/// How many levels of nested construct the parser will descend through before
/// it stops and reports `E0011`.
///
/// Recursive descent spends stack per level, so an input nested deeply enough
/// ends the process with `fatal runtime error: stack overflow` instead of a
/// diagnostic: 2,000 nested `[` did that to `glyph check`, `glyph fmt`,
/// `glyph lsp` and `glyph mcp` alike, and the language server reads every file
/// under the root, so one pathological file took the server down for the whole
/// workspace (G229).
///
/// The number comes from measuring what one level of the deepest entry point,
/// a nested array literal, costs in stack: 7,792 bytes in the release build
/// and 8,810 in the debug one, on x86-64 macOS. Both were found by
/// binary-searching the smallest thread stack a given depth parses in, where
/// depth 100 needs 790,339 bytes in release and 896,809 in debug and depth 800
/// needs 6,244,879 and 7,063,879. A level is expensive because it runs the
/// whole precedence ladder, twenty-odd frames each carrying an `Expr`
/// temporary.
///
/// The thinnest stack the parser runs on is a spawned thread's, 2 MiB by
/// default: that is what the language server's tokio workers get, and what the
/// test harness gives each test, where the debug build aborts past depth 235.
/// The CLI's main thread has 8 MiB. At 64 levels the release build spends
/// about 515 KiB and the debug build about 580 KiB, so even the costlier build
/// on the thinner stack uses under a third of it.
///
/// A level is one construct the parser descends into, counted once however it
/// is spelled: parentheses around an operand are that operand's level, not a
/// second one, so `-(x)` nests as deep as `-x`. See `nested_operand`.
///
/// It is also far past anything anyone writes. Across the 343 `.glyph` files
/// in this repository the deepest nesting is 16 levels, in
/// `examples/apps/watchrun/main.glyph`; a file that reaches 64 was generated.
pub const MAX_NESTING_DEPTH: u32 = 64;

pub(crate) struct Cursor<'a> {
    tokens: Vec<Spanned<Token>>,
    pos: usize,
    /// Levels of nested construct currently open. Maintained by `nested`, which
    /// is the only thing that touches it, so it cannot leak on an error path.
    depth: u32,
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
            depth: 0,
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

    /// Run `f` one nesting level deeper, or report `E0011` if that would cross
    /// `MAX_NESTING_DEPTH`.
    ///
    /// Every recursion entry point in the parser that can nest goes through
    /// this one helper: an expression operand, a type argument, a pattern, a
    /// block, a JSX child. `construct` names what is being entered and `span`
    /// is where the limit is crossed, so the diagnostic points at the token
    /// that would have opened the level the parser refused to descend into.
    ///
    /// The counter is incremented and decremented around `f` here rather than
    /// by the callers, so an error returned from inside a level still unwinds
    /// the count. Nothing in the parser catches a `ParseError` and continues,
    /// but a recovery path added later would inherit a correct depth either
    /// way.
    pub fn nested<T>(
        &mut self,
        construct: &'static str,
        span: Span,
        f: impl FnOnce(&mut Cursor<'a>) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.depth >= MAX_NESTING_DEPTH {
            return Err(ParseError::NestingTooDeep {
                construct,
                limit: MAX_NESTING_DEPTH,
                span,
            });
        }
        self.depth += 1;
        let out = f(self);
        self.depth -= 1;
        out
    }

    /// Open a level for one operand expression, unless that operand is written
    /// as a grouping, which opens the level itself.
    ///
    /// `(e)` builds no node: `parse_primary` returns `e`. So the operand slot
    /// of a prefix or infix operator and the parentheses a writer may put
    /// around it are the same expression, and charging both counted it twice.
    /// `-x` cost one level and `-(x)` cost two, which made the limit depend on
    /// a spelling that the AST does not record.
    ///
    /// That mattered because the formatter prints every unary operand
    /// parenthesized: a run of 40 minuses parsed as 40 levels, `glyph fmt`
    /// rewrote it as `-(-(-(...`, and the second pass counted 80 and refused
    /// the file the formatter had just written (G241).
    ///
    /// The level is never dropped, only moved: when the operand starts with
    /// `(`, `parse_primary`'s grouping arm charges it through `nested`, and a
    /// grouping directly inside another grouping still charges one each, so
    /// `((((...))))` stays bounded. Both spellings cost one level per level of
    /// real recursion, and that level runs the whole precedence ladder either
    /// way, which is what `MAX_NESTING_DEPTH` was measured against.
    pub fn nested_operand<T>(
        &mut self,
        construct: &'static str,
        span: Span,
        f: impl FnOnce(&mut Cursor<'a>) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.check(&Token::LParen) {
            return f(self);
        }
        self.nested(construct, span, f)
    }
}
