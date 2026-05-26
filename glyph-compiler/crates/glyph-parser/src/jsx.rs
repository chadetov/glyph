//! JSX sub-grammar (D6). Entered from `parse_primary` when `<` appears in
//! expression-starting position.
//!
//! Implemented this slice:
//! - Opening tag `<name attrs>` with HTML-like names, component names, and
//!   directive names (`if`, `else`, `for`, `match`, `case`)
//! - Self-closing `<name attrs />`
//! - Closing tag `</name>`
//! - Attributes: `name="literal"`, `name={expr}`, positional `<case Loaded>`
//! - Children: nested elements, `{expr}` blocks, raw text runs reconstructed
//!   by slicing the source string between tag boundaries
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

/// Parse a JSX element starting at `<`. Caller has verified that the next
/// token is `LAngle` and the lookahead disambiguates JSX from comparison.
pub(crate) fn parse_jsx_element(p: &mut Cursor) -> Result<JsxElement, ParseError> {
    let open_span = p.expect(&Token::LAngle, "`<` (JSX)")?;
    let (name, _name_span) = jsx_name(p)?;

    // Parse attributes until `>` or `/>`.
    let mut attrs = Vec::new();
    while !matches!(p.peek(), Token::RAngle | Token::Slash) {
        attrs.push(parse_jsx_attr(p)?);
    }

    // Self-closing or has children?
    if matches!(p.peek(), Token::Slash) {
        p.advance();
        let close = p.expect(&Token::RAngle, "`>` after `/` (self-closing JSX)")?;
        return Ok(JsxElement {
            name,
            attrs,
            children: Vec::new(),
            self_closing: true,
            span: Span::new(open_span.start, close.end),
        });
    }

    // Consume `>` of the opening tag.
    let _gt = p.expect(&Token::RAngle, "`>` (close JSX opening tag)")?;

    // Children until the matching closing tag `</name>`.
    let children = parse_jsx_children(p)?;

    // Closing tag.
    p.expect(&Token::LAngle, "`<` (open closing tag)")?;
    p.expect(&Token::Slash, "`/` (closing tag)")?;
    let (close_name, close_name_span) = jsx_name(p)?;
    if close_name != name {
        return Err(ParseError::Expected {
            expected: "closing tag matching the opening tag",
            found: format!("</{}>", close_name.as_ref()),
            span: close_name_span,
        });
    }
    let close = p.expect(&Token::RAngle, "`>` (end of closing tag)")?;

    Ok(JsxElement {
        name,
        attrs,
        children,
        self_closing: false,
        span: Span::new(open_span.start, close.end),
    })
}

fn parse_jsx_children(p: &mut Cursor) -> Result<Vec<JsxChild>, ParseError> {
    let mut children = Vec::new();
    loop {
        // Skip newlines between children (they're not significant in JSX
        // children flow; whitespace is preserved inside text runs).
        while matches!(p.peek(), Token::Newline) {
            p.advance();
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
            let elem = parse_jsx_element(p)?;
            children.push(JsxChild::Element(elem));
            continue;
        }

        // `{expr}` expression child.
        if matches!(p.peek(), Token::LBrace) {
            let open = p.peek_span();
            p.advance();
            let e = expr::parse_expr(p)?;
            p.expect(&Token::RBrace, "`}` (closing JSX expression child)")?;
            let _ = open; // Span tracked via the expression itself.
            children.push(JsxChild::Expr(e));
            continue;
        }

        // Text run: accumulate tokens until `<` or `{` (or newline/EOF).
        let text_start = p.peek_span().start;
        let mut text_end = text_start;
        let mut consumed_any = false;
        while !matches!(
            p.peek(),
            Token::LAngle | Token::LBrace | Token::Eof | Token::Newline
        ) {
            text_end = p.peek_span().end;
            p.advance();
            consumed_any = true;
        }
        if consumed_any {
            let content = p.slice(text_start, text_end).to_string();
            children.push(JsxChild::Text {
                content,
                span: Span::new(text_start, text_end),
            });
        }
    }
    Ok(children)
}

fn parse_jsx_attr(p: &mut Cursor) -> Result<JsxAttr, ParseError> {
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
                        value: s,
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

/// Parse a JSX name. Identifiers and reserved-keyword-as-name tokens are both
/// accepted, reusing the cursor's field-name helper so directive names like
/// `if`/`else`/`for`/`match`/`case` work uniformly.
fn jsx_name(p: &mut Cursor) -> Result<(Arc<str>, Span), ParseError> {
    p.expect_field_name("JSX element or attribute name")
}
