//! Pratt expression parser. Precedence table from `archive/GLYPH.md §2` (D18).
//!
//! Levels (higher number = tighter binding):
//!   1  member/call/index   .  ?.  []  ()         left-assoc
//!   2  postfix try         ?                     postfix
//!   3  prefix              !  -                  right-assoc
//!   4  multiplicative      *  /  %               left-assoc
//!   5  additive            +  -                  left-assoc
//!   6  comparison          <  <=  >  >=          left-assoc
//!   7  equality            ==  !=                left-assoc
//!   8  logical and         &&                    left-assoc
//!   9  logical or          ||                    left-assoc
//!  10  nullish coalesce    ??                    right-assoc
//!  11  prefix await        await                 right-assoc
//!  12  assignment (statement-level, not in expressions)

use glyph_ast::{
    ArrayElem, BinOp, Expr, MatchArm, MatchArmBody, ObjectField, PostfixOp, TemplatePart, UnaryOp,
};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::ParseError;
use crate::jsx;
use crate::pat;
use crate::stmt;

pub(crate) fn parse_expr(p: &mut Cursor) -> Result<Expr, ParseError> {
    parse_await(p)
}

// Level 11 — `await expr`
fn parse_await(p: &mut Cursor) -> Result<Expr, ParseError> {
    if matches!(p.peek(), Token::Await) {
        let kw_span = p.peek_span();
        p.advance();
        let expr = parse_await(p)?; // right-assoc
        let end = expr.span().end;
        return Ok(Expr::Await {
            expr: Box::new(expr),
            span: Span::new(kw_span.start, end),
        });
    }
    parse_nullish(p)
}

// Level 10 — `??` right-associative
fn parse_nullish(p: &mut Cursor) -> Result<Expr, ParseError> {
    let left = parse_or(p)?;
    if matches!(p.peek(), Token::QQ) {
        p.advance();
        let right = parse_nullish(p)?; // right-assoc
        let span = Span::new(left.span().start, right.span().end);
        return Ok(Expr::Binary {
            op: BinOp::NullishCoalesce,
            left: Box::new(left),
            right: Box::new(right),
            span,
        });
    }
    Ok(left)
}

// Level 9 — `||` left-assoc
fn parse_or(p: &mut Cursor) -> Result<Expr, ParseError> {
    left_assoc(p, parse_and, &[(Token::PipePipe, BinOp::LogicalOr)])
}

// Level 8 — `&&` left-assoc
fn parse_and(p: &mut Cursor) -> Result<Expr, ParseError> {
    left_assoc(p, parse_eq, &[(Token::AmpAmp, BinOp::LogicalAnd)])
}

// Level 7 — `==`, `!=`
fn parse_eq(p: &mut Cursor) -> Result<Expr, ParseError> {
    left_assoc(
        p,
        parse_cmp,
        &[(Token::EqEq, BinOp::Eq), (Token::BangEq, BinOp::NotEq)],
    )
}

// Level 6 — `<`, `<=`, `>`, `>=`
fn parse_cmp(p: &mut Cursor) -> Result<Expr, ParseError> {
    left_assoc(
        p,
        parse_add,
        &[
            (Token::LAngle, BinOp::Lt),
            (Token::RAngle, BinOp::Gt),
            (Token::LtEq, BinOp::LtEq),
            (Token::GtEq, BinOp::GtEq),
        ],
    )
}

// Level 5 — `+`, `-`
fn parse_add(p: &mut Cursor) -> Result<Expr, ParseError> {
    left_assoc(p, parse_mul, &[(Token::Plus, BinOp::Add), (Token::Minus, BinOp::Sub)])
}

// Level 4 — `*`, `/`, `%`
fn parse_mul(p: &mut Cursor) -> Result<Expr, ParseError> {
    left_assoc(
        p,
        parse_unary,
        &[
            (Token::Star, BinOp::Mul),
            (Token::Slash, BinOp::Div),
            (Token::Percent, BinOp::Rem),
        ],
    )
}

// Level 3 — prefix `!`, `-` (right-associative).
fn parse_unary(p: &mut Cursor) -> Result<Expr, ParseError> {
    let span = p.peek_span();
    match p.peek() {
        Token::Bang => {
            p.advance();
            let operand = parse_unary(p)?;
            let end = operand.span().end;
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span: Span::new(span.start, end),
            })
        }
        Token::Minus => {
            p.advance();
            let operand = parse_unary(p)?;
            let end = operand.span().end;
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
                span: Span::new(span.start, end),
            })
        }
        _ => parse_postfix(p),
    }
}

// Level 2 (postfix `?`) and Level 1 (member/call/index) both attach here.
// We parse the primary, then loop on the tightest binders.
fn parse_postfix(p: &mut Cursor) -> Result<Expr, ParseError> {
    let mut expr = parse_primary(p)?;
    loop {
        match p.peek() {
            // Level 1: member access
            Token::Dot => {
                p.advance();
                let (name, name_span) = p.expect_ident("identifier after `.`")?;
                let start = expr.span().start;
                expr = Expr::Member {
                    object: Box::new(expr),
                    field: name,
                    optional: false,
                    span: Span::new(start, name_span.end),
                };
            }
            // Level 1: optional chaining (D18)
            Token::QDot => {
                p.advance();
                let (name, name_span) = p.expect_ident("identifier after `?.`")?;
                let start = expr.span().start;
                expr = Expr::Member {
                    object: Box::new(expr),
                    field: name,
                    optional: true,
                    span: Span::new(start, name_span.end),
                };
            }
            // Level 1: call (with optional generic type args `<T1, T2>` before
            // the call args). Heuristic lookahead disambiguates from `<` as a
            // comparison operator; see `looks_like_generic_call`. Cheap
            // receiver-shape filter runs first so the full scan only fires on
            // callable expressions.
            Token::LAngle if is_callable_receiver(&expr) && looks_like_generic_call(p) => {
                let type_args = parse_generic_call_type_args(p)?;
                p.advance(); // `(` (lookahead already confirmed)
                let args = p.parse_comma_separated(&Token::RParen, true, parse_expr)?;
                let close = p.expect(&Token::RParen, "`)`")?;
                let start = expr.span().start;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    type_args,
                    args,
                    span: Span::new(start, close.end),
                };
            }
            // Level 1: call
            Token::LParen => {
                p.advance();
                let args = p.parse_comma_separated(&Token::RParen, true, parse_expr)?;
                let close = p.expect(&Token::RParen, "`)`")?;
                let start = expr.span().start;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    type_args: Vec::new(),
                    args,
                    span: Span::new(start, close.end),
                };
            }
            // Level 1: index
            Token::LBracket => {
                p.advance();
                let index = parse_expr(p)?;
                let close = p.expect(&Token::RBracket, "`]`")?;
                let start = expr.span().start;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span: Span::new(start, close.end),
                };
            }
            // Level 2: postfix `?` (Result-propagation try). Binds tighter than `.`
            // per D18 — but appears at this level because we already consumed
            // member/call/index above and now check for `?` to extend the chain.
            Token::Question => {
                let q_span = p.peek_span();
                p.advance();
                let start = expr.span().start;
                expr = Expr::Postfix {
                    op: PostfixOp::Try,
                    operand: Box::new(expr),
                    span: Span::new(start, q_span.end),
                };
            }
            _ => break,
        }
    }
    Ok(expr)
}

// Primary atoms.
fn parse_primary(p: &mut Cursor) -> Result<Expr, ParseError> {
    let span = p.peek_span();
    match p.peek().clone() {
        Token::Number(raw) => {
            p.advance();
            Ok(Expr::Number { raw, span })
        }
        Token::String(value) => {
            p.advance();
            // D22: split into a TemplateString iff the value contains a `${...}`.
            // `split_template_parts` walks the bytes once; absent any
            // interpolation it returns `None` and we keep the plain String.
            match split_template_parts(&value, span)? {
                Some(parts) => Ok(Expr::TemplateString { parts, span }),
                None => Ok(Expr::String { value, span }),
            }
        }
        Token::True => {
            p.advance();
            Ok(Expr::Bool { value: true, span })
        }
        Token::False => {
            p.advance();
            Ok(Expr::Bool { value: false, span })
        }
        Token::Void => {
            p.advance();
            Ok(Expr::Void { span })
        }
        Token::Identifier(name) => {
            p.advance();
            Ok(Expr::Ident { name, span })
        }
        // "Soft" keywords that can act as identifiers in expression position.
        // These are modifiers (owned, resource, mut, async, as) that don't
        // start expressions, plus a few that the corpus uses as variable
        // names. The full contextual-keyword refactor is a day 4 cleanup.
        ref t if is_soft_keyword_in_expr_position(t) => {
            let text = t.as_field_name().expect("soft keyword has text");
            p.advance();
            Ok(Expr::Ident {
                name: std::sync::Arc::from(text),
                span,
            })
        }
        Token::LParen => {
            // Parenthesized expression; no tuples in v0.
            p.advance();
            p.skip_newlines();
            let inner = parse_expr(p)?;
            p.skip_newlines();
            p.expect(&Token::RParen, "`)`")?;
            Ok(inner)
        }
        Token::LBracket => parse_array_literal(p),
        Token::LBrace => parse_object_literal(p),
        Token::LAngle if is_jsx_lookahead(p) => {
            let elem = jsx::parse_jsx_element(p)?;
            Ok(Expr::Jsx(elem))
        }
        Token::Match => parse_match(p, span),
        Token::Fn => parse_lambda(p, span),
        other => Err(ParseError::Unexpected {
            found: format!("{other:?}"),
            span,
        }),
    }
}

fn parse_array_literal(p: &mut Cursor) -> Result<Expr, ParseError> {
    let open = p.expect(&Token::LBracket, "`[`")?;
    let elements = p.parse_comma_separated(&Token::RBracket, true, |p| {
        if matches!(p.peek(), Token::DotDotDot) {
            p.advance();
            Ok(ArrayElem::Spread(parse_expr(p)?))
        } else {
            Ok(ArrayElem::Expr(parse_expr(p)?))
        }
    })?;
    let close = p.expect(&Token::RBracket, "`]`")?;
    Ok(Expr::Array {
        elements,
        span: Span::new(open.start, close.end),
    })
}

/// Real match parser (D2/D3/D9). Arms separated by trailing-comma.
fn parse_match(p: &mut Cursor, start_span: Span) -> Result<Expr, ParseError> {
    p.expect(&Token::Match, "`match`")?;
    let scrutinee = parse_expr(p)?;
    p.skip_newlines();
    p.expect(&Token::LBrace, "`{` after match scrutinee")?;
    p.skip_newlines();

    let mut arms = Vec::new();
    while !matches!(p.peek(), Token::RBrace) {
        let arm = parse_match_arm(p)?;
        arms.push(arm);
        // D2: trailing comma required on every arm, including the last.
        if matches!(p.peek(), Token::Comma) {
            p.advance();
        } else if !matches!(p.peek(), Token::RBrace) {
            return Err(ParseError::Expected {
                expected: "`,` after match arm (D2)",
                found: format!("{:?}", p.peek()),
                span: p.peek_span(),
            });
        }
        p.skip_newlines();
    }
    let close = p.expect(&Token::RBrace, "`}`")?;
    Ok(Expr::Match {
        scrutinee: Box::new(scrutinee),
        arms,
        span: Span::new(start_span.start, close.end),
    })
}

fn parse_match_arm(p: &mut Cursor) -> Result<MatchArm, ParseError> {
    let arm_start = p.peek_span();
    let pattern = pat::parse_arm_pattern(p)?;
    p.expect(&Token::FatArrow, "`=>` in match arm")?;
    let body = if matches!(p.peek(), Token::LBrace) {
        // Disambiguate "block body" from "object-literal expression body":
        //   Ok(value) => Ok(value),               // expression
        //   FileNotReadable({ path }) => "...",   // expression (NOT a block)
        //   Some(_) => { stmt1 ... stmtN },       // real block
        if looks_like_object_literal(p) {
            MatchArmBody::Expr(parse_expr(p)?)
        } else {
            MatchArmBody::Block(stmt::parse_block(p)?)
        }
    } else if is_statement_arm_body_starter(p.peek()) {
        // Allow a single statement as the arm body, e.g.
        //   Ok(_) => return 0,
        //   Ok(value) => mut result[key] = value,
        // Wrap it in a synthetic one-statement block.
        let stmt = stmt::parse_one_stmt(p)?;
        let span = stmt.span();
        match stmt {
            // Expression statements come through as Stmt::Expr — unwrap.
            glyph_ast::Stmt::Expr(e) => MatchArmBody::Expr(e),
            other => MatchArmBody::Block(glyph_ast::Block {
                stmts: vec![other],
                span,
            }),
        }
    } else {
        MatchArmBody::Expr(parse_expr(p)?)
    };
    let end = match &body {
        MatchArmBody::Expr(e) => e.span().end,
        MatchArmBody::Block(b) => b.span.end,
    };
    Ok(MatchArm {
        pattern,
        body,
        span: Span::new(arm_start.start, end),
    })
}

/// Tokens that mark a match-arm body as a single statement rather than an
/// expression. Per D5 (`mut`) and the corpus's use of bare `return`, `let`,
/// `for`, `loop`, `break`, `continue` in arm-body position.
fn is_statement_arm_body_starter(t: &Token) -> bool {
    matches!(
        t,
        Token::Mut
            | Token::Return
            | Token::Let
            | Token::For
            | Token::Loop
            | Token::Break
            | Token::Continue
    )
}

/// Lookahead heuristic: starting at `{`, is the next non-whitespace structure
/// `key :` (object literal)? Used only to disambiguate match-arm bodies. The
/// key may be an identifier OR a soft keyword (per `Token::as_field_name`).
fn looks_like_object_literal(p: &Cursor) -> bool {
    let is_keyish = matches!(p.peek_at(1), Some(Token::Identifier(_)))
        || p.peek_at(1).is_some_and(|t| t.as_field_name().is_some());
    let is_spread = matches!(p.peek_at(1), Some(Token::DotDotDot));
    let next_is_colon = matches!(p.peek_at(2), Some(Token::Colon));
    (is_keyish && next_is_colon) || is_spread
}

fn parse_object_literal(p: &mut Cursor) -> Result<Expr, ParseError> {
    let open = p.expect(&Token::LBrace, "`{`")?;
    let fields = p.parse_comma_separated(&Token::RBrace, true, |p| {
        if matches!(p.peek(), Token::DotDotDot) {
            let spread_span = p.peek_span();
            p.advance();
            let value = parse_expr(p)?;
            let end = value.span().end;
            Ok(ObjectField::Spread {
                value,
                span: Span::new(spread_span.start, end),
            })
        } else {
            let key_span = p.peek_span();
            let (key, _) = p.expect_field_name("object literal field name")?;
            // D10 forbids shorthand; the colon is required.
            p.expect(&Token::Colon, "`:` after field name (D10: no shorthand)")?;
            let value = parse_expr(p)?;
            let end = value.span().end;
            Ok(ObjectField::KeyValue {
                key,
                value,
                span: Span::new(key_span.start, end),
            })
        }
    })?;
    let close = p.expect(&Token::RBrace, "`}`")?;
    Ok(Expr::Object {
        fields,
        span: Span::new(open.start, close.end),
    })
}

fn parse_lambda(p: &mut Cursor, start_span: Span) -> Result<Expr, ParseError> {
    p.expect(&Token::Fn, "`fn` (lambda)")?;
    p.expect(&Token::LParen, "`(` after `fn`")?;
    // Type annotations are optional inside a lambda (corpus has both
    // `fn(input) { ... }` and `fn(post: Post) { ... }`); missing types fall
    // back to a sentinel `unknown` path.
    let params = p.parse_comma_separated(&Token::RParen, true, |p| {
        let param_start = p.peek_span();
        let (name, _) = p.expect_ident("parameter name")?;
        let ty = if matches!(p.peek(), Token::Colon) {
            p.advance();
            crate::types::parse_type(p)?
        } else {
            glyph_ast::TypeExpr::Path {
                segments: vec![std::sync::Arc::from("unknown")],
                span: param_start,
            }
        };
        let end = ty.span().end;
        Ok(glyph_ast::Param {
            name,
            ty,
            span: Span::new(param_start.start, end),
        })
    })?;
    p.expect(&Token::RParen, "`)`")?;
    let return_ty = if matches!(p.peek(), Token::Arrow) {
        p.advance();
        Some(crate::types::parse_type(p)?)
    } else {
        None
    };
    let body = stmt::parse_block(p)?;
    let end = body.span.end;
    Ok(Expr::Lambda {
        params,
        return_ty,
        body,
        span: Span::new(start_span.start, end),
    })
}

/// D22: split a de-escaped string value into template parts.
///
/// Returns `Ok(None)` when the string contains no `${` — the caller keeps it
/// as a plain `Expr::String`. Returns `Ok(Some(parts))` when interpolation is
/// present; parts alternate `Text` and `Expr` and the expression inside each
/// `${...}` is parsed via `crate::parse_expression`.
///
/// Spans for inner parts are conservatively set to the parent string's span;
/// proper span mapping requires a lexer-level template-literal mode (v1.1).
///
/// Brace nesting inside an interpolation is tracked so `${{a: 1}.a}` works
/// (the outer `{` belongs to the interpolation, the inner `{` is an object
/// literal). String literals inside interpolations are also handled.
fn split_template_parts(
    value: &str,
    span: glyph_lexer::Span,
) -> Result<Option<Vec<TemplatePart>>, ParseError> {
    let bytes = value.as_bytes();
    let mut parts: Vec<TemplatePart> = Vec::new();
    let mut text_buf = String::new();
    let mut saw_interp = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            saw_interp = true;
            if !text_buf.is_empty() {
                parts.push(TemplatePart::Text {
                    content: std::mem::take(&mut text_buf),
                    span,
                });
            }
            i += 2;
            let interp_start = i;
            let mut depth: i32 = 1;
            let mut in_str = false;
            while i < bytes.len() {
                let b = bytes[i];
                if in_str {
                    if b == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if b == b'"' {
                        in_str = false;
                    }
                    i += 1;
                    continue;
                }
                match b {
                    b'"' => in_str = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if depth != 0 {
                return Err(ParseError::Expected {
                    expected: "matching `}` for `${...}` template interpolation",
                    found: "end of string literal".to_string(),
                    span,
                });
            }
            let interp_source = &value[interp_start..i];
            let inner_expr = crate::parse_expression(interp_source).map_err(|e| {
                ParseError::Expected {
                    expected: "valid expression inside `${...}` template interpolation",
                    found: format!("{e}"),
                    span,
                }
            })?;
            parts.push(TemplatePart::Expr {
                value: inner_expr,
                span,
            });
            i += 1;
            continue;
        }
        text_buf.push(bytes[i] as char);
        i += 1;
    }
    if !saw_interp {
        return Ok(None);
    }
    if !text_buf.is_empty() {
        parts.push(TemplatePart::Text {
            content: text_buf,
            span,
        });
    }
    Ok(Some(parts))
}

/// Only `Ident`, `Member`, and `Call` expressions can be the receiver of a
/// generic call. Filtering on the receiver shape skips the full
/// `looks_like_generic_call` scan for the common case of `a < b` comparisons.
fn is_callable_receiver(e: &Expr) -> bool {
    matches!(e, Expr::Ident { .. } | Expr::Member { .. } | Expr::Call { .. })
}

/// Conservative lookahead heuristic: from the current `<` token, scan ahead to
/// see if we have a balanced `<...>` followed immediately by `(`. Tokens that
/// can't appear inside a type expression (most binary operators, statement
/// keywords) abort the scan — preferring the comparison interpretation.
///
/// This is the standard "speculate at `<`" heuristic; TypeScript uses a
/// similar approach. False positives are possible for code like `a < b > (c)`
/// where the parens are unrelated — that's an accepted limitation.
fn looks_like_generic_call(p: &Cursor) -> bool {
    debug_assert!(matches!(p.peek(), Token::LAngle));
    // O(1) first-token filter — a type expression must begin with an
    // identifier, `void`, `fn`, or an inline record `{`. Anything else means
    // the `<` was a comparison.
    match p.peek_at(1) {
        Some(Token::Identifier(_) | Token::Void | Token::Fn | Token::LBrace) => {}
        _ => return false,
    }
    let mut angle: i32 = 0;
    let mut paren: i32 = 0;
    let mut bracket: i32 = 0;
    let mut brace: i32 = 0;
    let mut i: usize = 0;
    loop {
        let tok = match p.peek_at(i) {
            Some(t) => t,
            None => return false,
        };
        match tok {
            Token::Eof | Token::Newline => return false,
            // Tokens that can't appear inside a type expression. Hitting any
            // of these means the `<` was a comparison after all.
            Token::Equals
            | Token::EqEq
            | Token::BangEq
            | Token::AmpAmp
            | Token::PipePipe
            | Token::QQ
            | Token::QDot
            | Token::Plus
            | Token::Star
            | Token::Slash
            | Token::Percent
            | Token::Bang
            | Token::Return
            | Token::Let
            | Token::Const
            | Token::Mut
            | Token::Match
            | Token::For
            | Token::Loop
            | Token::Break
            | Token::Continue
            | Token::If
            | Token::Await
            | Token::FatArrow => return false,
            Token::LParen => paren += 1,
            Token::RParen => {
                paren -= 1;
                if paren < 0 {
                    return false;
                }
            }
            Token::LBracket => bracket += 1,
            Token::RBracket => {
                bracket -= 1;
                if bracket < 0 {
                    return false;
                }
            }
            Token::LBrace => brace += 1,
            Token::RBrace => {
                brace -= 1;
                if brace < 0 {
                    return false;
                }
            }
            Token::LAngle if paren == 0 && bracket == 0 && brace == 0 => angle += 1,
            Token::RAngle if paren == 0 && bracket == 0 && brace == 0 => {
                angle -= 1;
                if angle == 0 {
                    return matches!(p.peek_at(i + 1), Some(Token::LParen));
                }
            }
            _ => {}
        }
        i += 1;
        if i > 200 {
            return false;
        }
    }
}

fn parse_generic_call_type_args(p: &mut Cursor) -> Result<Vec<glyph_ast::TypeExpr>, ParseError> {
    p.expect(&Token::LAngle, "`<` (generic call type args)")?;
    let args = p.parse_comma_separated(&Token::RAngle, false, crate::types::parse_type)?;
    p.expect(&Token::RAngle, "`>` (closing generic call type args)")?;
    Ok(args)
}

/// Disambiguate JSX from less-than at expression-start. JSX is `<` followed
/// by an identifier or a directive keyword (per D6). Anything else (`<` then
/// number, paren, etc.) is a syntax error since `<` cannot start a non-JSX
/// expression in Glyph (D7 reserves `<` for type position outside JSX).
fn is_jsx_lookahead(p: &Cursor) -> bool {
    match p.peek_at(1) {
        Some(Token::Identifier(_)) => true,
        Some(t) if t.as_field_name().is_some() => true,
        _ => false,
    }
}

/// Tokens that are keywords at the lexer level but can also stand in as
/// identifier references inside expression positions. These are *modifiers*
/// and *contextual* keywords — never expression-starters in their own right.
fn is_soft_keyword_in_expr_position(t: &Token) -> bool {
    matches!(
        t,
        Token::Owned
            | Token::Resource
            | Token::Mut
            | Token::As
            | Token::Type
            | Token::Record
            | Token::Component
            | Token::Const
            | Token::Module
            | Token::Import
            | Token::Break
            | Token::Continue
    )
}

/// Generic left-associative binary parser. Used at levels 4–9.
fn left_assoc(
    p: &mut Cursor,
    next: fn(&mut Cursor) -> Result<Expr, ParseError>,
    ops: &[(Token, BinOp)],
) -> Result<Expr, ParseError> {
    let mut left = next(p)?;
    loop {
        let matched_op = ops
            .iter()
            .find(|(t, _)| std::mem::discriminant(p.peek()) == std::mem::discriminant(t))
            .map(|(_, op)| *op);

        let Some(op) = matched_op else { break };
        p.advance();
        let right = next(p)?;
        let span = Span::new(left.span().start, right.span().end);
        left = Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
            span,
        };
    }
    Ok(left)
}
