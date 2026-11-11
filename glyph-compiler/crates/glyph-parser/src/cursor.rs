//! Token cursor: peek/advance/expect over tokens pulled from the lexer on
//! demand.
//!
//! The cursor does not lex the file up front. It asks the lexer for the next
//! token only when the parser looks at it, and keeps every token it has
//! pulled in a buffer along with the lexer state after it, so lookahead and
//! a later resume of the lexer both have what they need.
//!
//! A lexical error is held, not raised: it sits in the buffer as an entry the
//! parser sees as end of input, at the error's span. Lookahead that reaches
//! it backs off as it would at the end of the file. It becomes a
//! `ParseError::Lex`, with the span and message `tokenize` reports, only when
//! the parser arrives at it (see `Cursor::lex_error_here`).

use glyph_lexer::{LexState, Lexer, Span, Spanned, Token};

use crate::error::{NameRole, ParseError};

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

/// One pulled token and the lexer state after it. A lex error is an entry
/// whose token is `Eof` at the error's span, with the error's message; it is
/// always the last entry, because the lexer is not asked for more after it.
struct Entry {
    token: Spanned<Token>,
    /// What `Lexer::at` needs to resume right after this token. Nothing reads
    /// it yet; the JSX text scanner resumes from it.
    #[allow(dead_code)]
    after: LexState,
    lex_error: Option<String>,
}

pub(crate) struct Cursor<'a> {
    lexer: Lexer<'a>,
    /// Every token pulled so far, in order. It ends at `Eof` or at a lex
    /// error once the lexer has been driven that far, and `pos` never moves
    /// past that last entry.
    buf: Vec<Entry>,
    /// The lexer produced `Eof` or failed; nothing more will be pulled.
    exhausted: bool,
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
    pub fn new(source: &'a str) -> Self {
        Self {
            lexer: Lexer::new(source),
            buf: Vec::new(),
            exhausted: false,
            pos: 0,
            depth: 0,
            source,
        }
    }

    /// Pull until the buffer holds index `i`, or the lexer is done. Returns
    /// whether index `i` exists.
    fn fill(&mut self, i: usize) -> bool {
        while self.buf.len() <= i && !self.exhausted {
            let entry = match self.lexer.next_token() {
                Ok(token) => {
                    self.exhausted = matches!(token.token, Token::Eof);
                    Entry {
                        token,
                        after: self.lexer.state(),
                        lex_error: None,
                    }
                }
                Err(e) => {
                    self.exhausted = true;
                    Entry {
                        token: Spanned {
                            token: Token::Eof,
                            span: e.span(),
                        },
                        after: self.lexer.state(),
                        lex_error: Some(e.to_string()),
                    }
                }
            };
            self.buf.push(entry);
        }
        i < self.buf.len()
    }

    /// The entry at `pos`. One always exists: the buffer is filled to `pos`
    /// before it is read, and `pos` never passes the last entry.
    fn current(&mut self) -> &Entry {
        let filled = self.fill(self.pos);
        debug_assert!(filled, "the cursor never moves past the last entry");
        &self.buf[self.pos]
    }

    /// The held lex error at the cursor's position, if the parser has arrived
    /// at one, as the `ParseError` the eager lexer's failure produced. The
    /// entry points check this whatever the parse returned: a parse stopped
    /// at the error, or one that took it for the end of input, is reporting
    /// the lex error.
    pub fn lex_error_here(&mut self) -> Option<ParseError> {
        let entry = self.current();
        entry.lex_error.as_ref().map(|message| ParseError::Lex {
            message: message.clone(),
            span: entry.token.span,
        })
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

    pub fn peek(&mut self) -> &Token {
        &self.current().token.token
    }

    pub fn peek_at(&mut self, offset: usize) -> Option<&Token> {
        let i = self.pos + offset;
        if self.fill(i) {
            Some(&self.buf[i].token.token)
        } else {
            None
        }
    }

    pub fn peek_span(&mut self) -> Span {
        self.current().token.span
    }

    pub fn peek_span_at(&mut self, offset: usize) -> Option<Span> {
        let i = self.pos + offset;
        if self.fill(i) {
            Some(self.buf[i].token.span)
        } else {
            None
        }
    }

    pub fn advance(&mut self) -> &Spanned<Token> {
        let i = self.pos;
        self.fill(i + 1);
        if i + 1 < self.buf.len() {
            self.pos += 1;
        }
        &self.buf[i].token
    }

    pub fn is_at_end(&mut self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    /// Consume any number of leading `Newline` tokens. Used between statements
    /// and between top-level items.
    pub fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    pub fn check(&mut self, t: &Token) -> bool {
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

    /// Consume an identifier that introduces a name (a `let`, a parameter, a
    /// `fn` or `type`, a generic, a loop binding) and return it with its span.
    ///
    /// A reserved word here is reported as one (G286), not as a missing
    /// token: `let where = 1` used to say "expected variable name after `let`,
    /// found Where" with a help line about trailing commas, and nothing named
    /// `where` as a keyword. Whether a token is a keyword is read from the
    /// lexer's `KEYWORDS` table through `Token::as_field_name`, so the set this
    /// reports on cannot drift from the set the lexer reserves.
    pub fn expect_ident(&mut self, expected: &'static str) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        self.expect_ident_as(expected, NameRole::Declares)
    }

    /// Consume an identifier that refers to a name declared somewhere else: a
    /// type name or the head of a type path, the head of a `typeof` path, an
    /// item named in an import list. A member or a type path segment after
    /// `.` is not one (`expect_member_name`). A
    /// reserved word is reported with help that does not tell the author to
    /// rename something they may not own.
    pub fn expect_ident_ref(&mut self, expected: &'static str) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        self.expect_ident_as(expected, NameRole::Refers)
    }

    fn expect_ident_as(
        &mut self,
        expected: &'static str,
        role: NameRole,
    ) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok((name, span))
            }
            other => Err(match other.as_field_name() {
                Some(word) => ParseError::ReservedWordAsName { word, role, span },
                None => ParseError::Expected {
                    expected,
                    found: format!("{other:?}"),
                    span,
                },
            }),
        }
    }

    /// Consume a member name after `.` or `?.` (G300). Every keyword is a
    /// member name here, as a field name in a record type or an object
    /// literal is: `{ type: string }` is the most common wire discriminator
    /// there is, and a field that can be declared must be readable as
    /// `e.type`. Nothing else can follow a dot, so this takes no syntax away.
    /// The set is the lexer's `KEYWORDS` table through `expect_field_name`.
    pub fn expect_member_name(
        &mut self,
        expected: &'static str,
    ) -> Result<(std::sync::Arc<str>, Span), ParseError> {
        self.expect_field_name(expected)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn eager_lex_error(src: &str) -> ParseError {
        let e = glyph_lexer::tokenize(src).expect_err("source has a lex error");
        ParseError::Lex {
            message: e.to_string(),
            span: e.span(),
        }
    }

    #[test]
    fn lookahead_past_a_lex_error_backs_off_without_failing() {
        // `"\q"` is an invalid escape. Peeking at it, or past it, sees the
        // end of input and raises nothing while the cursor is short of it.
        let src = "a b \"\\q\" c";
        let mut p = Cursor::new(src);
        assert_eq!(p.peek_at(2), Some(&Token::Eof));
        assert_eq!(p.peek_at(3), None);
        assert!(p.lex_error_here().is_none());
        assert!(matches!(p.peek(), Token::Identifier(_)));
        p.advance();
        assert!(p.lex_error_here().is_none());
        assert!(matches!(p.peek(), Token::Identifier(_)));
    }

    #[test]
    fn arriving_at_a_lex_error_reports_the_eager_span_and_message() {
        let src = "a b \"\\q\" c";
        let mut p = Cursor::new(src);
        p.advance();
        p.advance();
        assert!(p.is_at_end());
        // The cursor does not move past the error entry.
        p.advance();
        assert_eq!(p.lex_error_here(), Some(eager_lex_error(src)));
        assert_eq!(p.peek_span(), eager_lex_error(src).span());
    }

    #[test]
    fn every_lex_error_kind_is_reported_as_the_eager_lexer_reported_it() {
        for src in [
            "fn f() {\n  let x = 1 # 2\n}\n",
            "fn f() {\n  let s = \"open\n}\n",
            "fn f() {\n  let s = \"\\q\"\n}\n",
            "fn f() {\n  let n = 1__0\n}\n",
            "/* no */\n",
        ] {
            assert_eq!(crate::parse(src).unwrap_err(), eager_lex_error(src), "{src:?}");
        }
    }

    #[test]
    fn a_generic_call_scan_into_a_lex_error_still_reports_it() {
        // At `<` the generic-call scan walks forward over `b ,` and reaches
        // the bad escape. It backs off to a comparison, the parser carries on
        // to the string, and the lex error is what the parse reports.
        let src = "fn g() {\n  let x = f(a < b, \"\\q\")\n}\n";
        assert_eq!(crate::parse(src).unwrap_err(), eager_lex_error(src));
    }

    /// The case that reads worse than the eager lexer (G329, record 92). The
    /// scan at `<` reaches the bad escape and backs off, and the comparison
    /// reading `f < a` fails at the `,` before the parser arrives at the
    /// escape, so the error points at valid code. Pinned so the documented
    /// behaviour cannot change unnoticed; a fix for G329 flips this test.
    #[test]
    fn a_generic_call_scan_that_backs_off_can_fail_before_the_lex_error() {
        let src = "fn g() -> number {\n  let x = f<a, \"\\q\">(x)\n}\n";
        let err = crate::parse(src).unwrap_err();
        let lex = eager_lex_error(src);
        assert!(
            matches!(&err, ParseError::Unexpected { found, .. } if found == "Comma"),
            "{err:?}"
        );
        assert!(err.span().start < lex.span().start, "{err:?} before {lex:?}");
    }
}
