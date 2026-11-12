//! JSX sub-grammar (D6). Entered from `parse_primary` when `<` appears in
//! expression-starting position.
//!
//! Implemented this slice:
//! - Opening tag `<name attrs>` with HTML-like names, component names, and
//!   directive names (`if`, `else`, `for`, `match`, `case`)
//! - Self-closing `<name attrs />`
//! - Closing tag `</name>`
//! - Attributes: `name="literal"`, `name={expr}`, positional `<for x in={xs}>`,
//!   and the pattern a `<case>` opens with (`<case Loaded(items)>`), which is
//!   parsed as a `match` arm's pattern
//! - Children: nested elements, `{expr}` blocks, comment-only `{ // note }`
//!   blocks, and raw text runs the lexer scans on the parser's request
//!   (`Cursor::rescan_jsx_text`, record 85)
//!
//! Per D6, directive element names are reserved at the typechecker level, not
//! the parser. The parser treats `<if>`, `<for>`, `<match>`, `<case>`, `<else>`
//! exactly like any other named element.

use std::sync::Arc;

use glyph_ast::{JsxAttr, JsxChild, JsxElement};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::ParseError;
use crate::expr;
use crate::pat;

/// Where an element sits, which decides what may follow its last `>`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// In expression position: what follows its end is an expression
    /// continuing, or nothing.
    Outermost,
    /// Among another element's children: a text run follows its end.
    Child,
}

/// Parse a JSX element starting at `<`. Caller has verified that the next
/// token is `LAngle` and the lookahead disambiguates JSX from comparison.
///
/// A `>=` that ends a tag is split into the tag's `>` and an `=` only where a
/// text run follows the `>` (record 93): after the opening tag, the opening
/// `<>` of a fragment, and the last `>` of a child element. The outermost
/// element's last `>` is never split, so `<p></p>== y` and `<br/>=x` stay
/// refused as they were.
pub(crate) fn parse_jsx_element(p: &mut Cursor, placement: Placement) -> Result<JsxElement, ParseError> {
    let open_span = p.expect(&Token::LAngle, "`<` (JSX)")?;
    let split_end = placement == Placement::Child;

    // Fragment `<>...</>`: `<` immediately followed by `>`, no name or attrs.
    // Represented by an empty element name (a real JSX name can never be empty).
    // `is_jsx_lookahead` admits `<>=` for this split.
    p.split_glued_tag_end();
    if matches!(p.peek(), Token::RAngle) {
        p.advance(); // `>`
        let children = p.nested("JSX fragment", open_span, parse_jsx_children)?;
        p.expect(&Token::LAngle, "`<` (open fragment closing `</>`)")?;
        p.expect(&Token::Slash, "`/` (fragment closing `</>`)")?;
        let close = expect_tag_end(p, split_end, "`>` (end of fragment `</>`)")?;
        return Ok(JsxElement {
            name: Arc::from(""),
            attrs: Vec::new(),
            children,
            self_closing: false,
            name_span: open_span,
            name_segments: Vec::new(),
            close_name_segments: Vec::new(),
            span: Span::new(open_span.start, close.end),
        });
    }

    let (name, name_span, name_segments) = jsx_element_name(p)?;

    // Parse attributes until `>` or `/>`. A `<case>` opens with the pattern
    // it matches, the same pattern a `match` arm takes (D6).
    let mut attrs = Vec::new();
    if name.as_ref() == "case" && starts_case_pattern(p) {
        let pattern = pat::parse_arm_pattern(p)?;
        attrs.push(JsxAttr::Pattern {
            span: pattern.span(),
            pattern,
        });
    }
    p.split_glued_tag_end();
    while !matches!(p.peek(), Token::RAngle | Token::Slash) {
        attrs.push(parse_jsx_attr(p)?);
        p.split_glued_tag_end();
    }

    // Self-closing or has children?
    if matches!(p.peek(), Token::Slash) {
        p.advance();
        let close = expect_tag_end(p, split_end, "`>` after `/` (self-closing JSX)")?;
        return Ok(JsxElement {
            name,
            attrs,
            children: Vec::new(),
            self_closing: true,
            name_span,
            name_segments,
            close_name_segments: Vec::new(),
            span: Span::new(open_span.start, close.end),
        });
    }

    // Consume `>` of the opening tag.
    expect_tag_end(p, true, "`>` (close JSX opening tag)")?;

    // Children until the matching closing tag `</name>`.
    let children = p.nested("JSX element", open_span, parse_jsx_children)?;

    // Closing tag.
    p.expect(&Token::LAngle, "`<` (open closing tag)")?;
    p.expect(&Token::Slash, "`/` (closing tag)")?;
    let (close_name, close_name_span, close_name_segments) = jsx_element_name(p)?;
    if close_name != name {
        return Err(ParseError::Expected {
            expected: "closing tag matching the opening tag",
            found: format!("</{}>", close_name.as_ref()),
            span: close_name_span,
        });
    }
    let close = expect_tag_end(p, split_end, "`>` (end of closing tag)")?;

    Ok(JsxElement {
        name,
        attrs,
        children,
        self_closing: false,
        name_span,
        name_segments,
        close_name_segments,
        span: Span::new(open_span.start, close.end),
    })
}

/// The `>` that ends a tag. Where a text run follows it (`split`), a `>=`
/// there is the tag's `>` and an `=` that begins the text (`<p>=</p>`),
/// which normal lexing reads as one token.
fn expect_tag_end(p: &mut Cursor, split: bool, expected: &'static str) -> Result<Span, ParseError> {
    if split {
        p.split_glued_tag_end();
    }
    p.expect(&Token::RAngle, expected)
}

/// The children of an element, up to its closing tag (D6).
///
/// A text run starts after the opening tag's `>`, after each child element and
/// after each `{expr}`, and runs to the next `{` or `<`. The parser asks the
/// lexer to scan it there (`rescan_jsx_text`): the run is raw source, across
/// lines, so it is one child, its span is exactly its source range, and what
/// it renders is `jsx_text_value`'s business (record 85, Q3). An empty run is
/// no child.
fn parse_jsx_children(p: &mut Cursor) -> Result<Vec<JsxChild>, ParseError> {
    let mut children = Vec::new();
    loop {
        if let Some((content, span)) = p.rescan_jsx_text()? {
            children.push(JsxChild::Text { content, span });
        }

        if matches!(p.peek(), Token::Eof) {
            return Err(ParseError::Expected {
                expected: "closing JSX tag (reached EOF)",
                found: "EOF".to_string(),
                span: p.peek_span(),
            });
        }

        // Closing tag of the parent element.
        if matches!(p.peek(), Token::LAngle) && matches!(p.peek_at(1), Some(Token::Slash)) {
            break;
        }

        // Nested JSX element.
        if matches!(p.peek(), Token::LAngle) {
            let elem = parse_jsx_element(p, Placement::Child)?;
            children.push(JsxChild::Element(elem));
            continue;
        }

        // The run stops only at `{` or `<`, so this is an `{...}` child.
        let open = p.expect(&Token::LBrace, "`{` (JSX expression child)")?;
        if let Some(child) = parse_jsx_brace_child(p, open)? {
            children.push(child);
        }
    }
    Ok(children)
}

/// The rest of an `{...}` child after its `{`: an expression child, or a
/// comment-only child (Q2 rule 4), which is no child: the comment is in the
/// comment list like any other, and the formatter writes it back as one.
/// `{}` with no comment is refused, and so is a block comment, with help that
/// names the comment-only form.
fn parse_jsx_brace_child(p: &mut Cursor, open: Span) -> Result<Option<JsxChild>, ParseError> {
    if matches!(p.peek(), Token::RBrace) {
        let close = p.advance().span;
        if p.previous_had_comments() {
            return Ok(None);
        }
        return Err(ParseError::EmptyJsxExpression {
            span: open.join(close),
        });
    }
    let parsed = expr::parse_expr(p)
        .and_then(|e| p.expect(&Token::RBrace, "`}` (closing JSX expression child)").map(|_| e));
    match parsed {
        Ok(e) => Ok(Some(JsxChild::Expr(e))),
        Err(err) => Err(block_comment_hint(p).unwrap_or(err)),
    }
}

/// A block comment the parser stopped at inside an `{...}` child, reported
/// with the JSX hint instead of the bare lex error.
fn block_comment_hint(p: &mut Cursor) -> Option<ParseError> {
    if !matches!(p.peek(), Token::Eof) {
        return None;
    }
    let span = p.peek_span();
    match p.take_lex_error_here()? {
        e @ glyph_lexer::LexError::NoBlockComments { .. } => Some(ParseError::JsxBlockComment {
            message: e.to_string(),
            span,
        }),
        other => {
            // Not ours to rephrase: leave it to be reported as itself.
            Some(ParseError::Lex {
                message: other.to_string(),
                span,
            })
        }
    }
}

fn parse_jsx_attr(p: &mut Cursor) -> Result<JsxAttr, ParseError> {
    // Prop spread `{...expr}`: an attribute that starts with `{` rather than a
    // name (the react-hook-form `{...register("name")}` idiom).
    if matches!(p.peek(), Token::LBrace) {
        let open = p.peek_span();
        p.advance();
        p.expect(&Token::DotDotDot, "`...` (JSX prop spread)")?;
        let value = expr::parse_expr(p)?;
        let close = p.expect(&Token::RBrace, "`}` (closing JSX prop spread)")?;
        return Ok(JsxAttr::Spread {
            value,
            span: Span::new(open.start, close.end),
        });
    }

    let (name, name_span) = jsx_name(p)?;

    // `name="string"` or `name={expr}` or positional (no `=`).
    match p.peek() {
        Token::Equals => {
            p.advance();
            match p.peek().clone() {
                Token::String(s) => {
                    let str_span = p.peek_span();
                    p.advance();
                    Ok(JsxAttr::String {
                        name,
                        // `\${` in a JSX attribute string is a literal `${`.
                        value: glyph_lexer::resolve_escaped_dollars(&s).into_owned(),
                        span: Span::new(name_span.start, str_span.end),
                    })
                }
                Token::LBrace => {
                    p.advance();
                    let value = expr::parse_expr(p)?;
                    let close = p.expect(&Token::RBrace, "`}` (closing JSX attribute expression)")?;
                    Ok(JsxAttr::Expr {
                        name,
                        value,
                        span: Span::new(name_span.start, close.end),
                    })
                }
                other => Err(ParseError::Expected {
                    expected: "string literal or `{expr}` after `=` in JSX attribute",
                    found: format!("{other:?}"),
                    span: p.peek_span(),
                }),
            }
        }
        _ => Ok(JsxAttr::Positional {
            name,
            span: name_span,
        }),
    }
}

/// Whether a `<case>`'s first attribute is its pattern. It is unless the tag
/// ends there or the next thing is a named attribute (`bind={x}`, `key="k"`,
/// `data-x="1"`) or a prop spread (`{...p}`). A pattern never continues with
/// `=` or `-` after a leading name, and an object pattern opens `{` with a
/// field name where a spread opens it with `...`, so neither test can take
/// a pattern for an attribute.
fn starts_case_pattern(p: &mut Cursor) -> bool {
    match p.peek() {
        Token::RAngle | Token::Slash => false,
        Token::LBrace => !matches!(p.peek_at(1), Some(Token::DotDotDot)),
        _ => !matches!(p.peek_at(1), Some(Token::Equals | Token::Minus)),
    }
}

/// Parse a JSX name. Identifiers and reserved-keyword-as-name tokens are both
/// accepted, reusing the cursor's field-name helper so directive names like
/// `if`/`else`/`for`/`match`/`case` work uniformly. Hyphenated names are joined
/// (`aria-label`, `data-testid`, web-component element names), which the lexer
/// would otherwise split into `ident - ident`.
fn jsx_name(p: &mut Cursor) -> Result<(Arc<str>, Span), ParseError> {
    p.expect_hyphenated_name("JSX element or attribute name")
}

/// Parse a JSX *element* name, which may be a member-expression path
/// (`Ctx.Provider`, `Ns.Sub.Comp`) for a namespaced component such as a React
/// Context provider. Stored as the dotted string; the emitter uses it verbatim
/// as the `createElement` type, and the resolver resolves only the base
/// segment. A `.` never follows an intrinsic tag or directive, so this is a
/// no-op for those.
///
/// Returns the dotted name, the span of the whole path, and the span of each
/// segment. The segments are separate tokens, so whitespace may sit between
/// them and only their own spans say where each one is.
fn jsx_element_name(p: &mut Cursor) -> Result<(Arc<str>, Span, Vec<Span>), ParseError> {
    let (first, first_span) = jsx_name(p)?;
    if !matches!(p.peek(), Token::Dot) {
        return Ok((first, first_span, vec![first_span]));
    }
    let mut name = first.to_string();
    let mut segments = vec![first_span];
    while matches!(p.peek(), Token::Dot) {
        p.advance(); // `.`
        let (seg, seg_span) = jsx_name(p)?;
        name.push('.');
        name.push_str(&seg);
        segments.push(seg_span);
    }
    let span = first_span.join(*segments.last().expect("one segment at least"));
    Ok((Arc::from(name.as_str()), span, segments))
}
