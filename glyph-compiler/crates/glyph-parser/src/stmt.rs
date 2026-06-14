//! Statement and block parsing.

use glyph_ast::{
    Block, BreakStmt, ContinueStmt, Expr, ForStmt, LetStmt, LoopStmt, MutKind, MutStmt,
    ReturnStmt, Stmt, TypeExpr,
};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::ParseError;
use crate::expr;
use crate::types;

pub(crate) fn parse_block(p: &mut Cursor) -> Result<Block, ParseError> {
    let open = p.expect(&Token::LBrace, "`{`")?;
    let mut stmts = Vec::new();
    loop {
        p.skip_newlines();
        if matches!(p.peek(), Token::RBrace) {
            break;
        }
        let s = parse_stmt(p)?;
        stmts.push(s);
        p.skip_newlines();
    }
    let close = p.expect(&Token::RBrace, "`}`")?;
    Ok(Block {
        stmts,
        span: Span::new(open.start, close.end),
    })
}

/// Parse a single statement. Exposed for the match-arm parser, which allows
/// statement-shaped arm bodies like `Ok(_) => return 0,` and
/// `Ok(value) => mut result[key] = value,`.
pub(crate) fn parse_one_stmt(p: &mut Cursor) -> Result<Stmt, ParseError> {
    parse_stmt(p)
}

fn parse_stmt(p: &mut Cursor) -> Result<Stmt, ParseError> {
    match p.peek() {
        Token::Let => parse_let(p).map(Stmt::Let),
        Token::Mut => parse_mut(p).map(Stmt::Mut),
        Token::Return => parse_return(p).map(Stmt::Return),
        Token::For => parse_for(p).map(Stmt::For),
        Token::Loop => parse_loop(p).map(Stmt::Loop),
        Token::Break => {
            let span = p.peek_span();
            p.advance();
            Ok(Stmt::Break(BreakStmt { span }))
        }
        Token::Continue => {
            let span = p.peek_span();
            p.advance();
            Ok(Stmt::Continue(ContinueStmt { span }))
        }
        _ => {
            let e = expr::parse_expr(p)?;
            Ok(Stmt::Expr(e))
        }
    }
}

/// D5: `mut` statement. Grammar restricts to these four shapes; anything else
/// is a syntax error. The typechecker does NOT verify that called methods
/// actually mutate (Q7).
fn parse_mut(p: &mut Cursor) -> Result<MutStmt, ParseError> {
    let mut_span = p.expect(&Token::Mut, "`mut`")?;
    // The target parses as an expression up to `=`. Since `=` is not an
    // operator, a postfix lvalue chain (`x`, `x.f`, `x[i]`, `x.items[0].name`,
    // `r.a.b`) parses and stops at the `=`; a method-call statement
    // (`mut xs.push(v)`) parses as a call with no following `=`.
    let target = expr::parse_expr(p)?;
    match p.peek() {
        Token::Equals => {
            p.advance();
            let value = expr::parse_expr(p)?;
            let end = value.span().end;
            if !is_lvalue(&target) {
                return Err(ParseError::Expected {
                    expected: "an assignable target before `=` (a name, field access, or index)",
                    found: format!("{target:?}"),
                    span: target.span(),
                });
            }
            Ok(MutStmt {
                kind: MutKind::Assign { target, value },
                span: Span::new(mut_span.start, end),
            })
        }
        // `mut <method call>` — D5's method-call mutation form.
        _ if matches!(target, Expr::Call { .. }) => {
            let end = target.span().end;
            Ok(MutStmt {
                kind: MutKind::MethodCall { call: target },
                span: Span::new(mut_span.start, end),
            })
        }
        other => Err(ParseError::Expected {
            expected: "`=` after the assignment target, or a method call (D5)",
            found: format!("{other:?}"),
            span: p.peek_span(),
        }),
    }
}

/// Whether `e` is an assignable place (an lvalue): a name, or a chain of field
/// accesses and index subscripts bottoming out at one. Calls, literals, and
/// operator expressions are not assignable.
fn is_lvalue(e: &Expr) -> bool {
    match e {
        Expr::Ident { .. } => true,
        Expr::Member { object, .. } => is_lvalue(object),
        Expr::Index { object, .. } => is_lvalue(object),
        _ => false,
    }
}

/// D21: `for X in expr { body }` and `for K, V in expr { body }`.
fn parse_for(p: &mut Cursor) -> Result<ForStmt, ParseError> {
    let for_span = p.expect(&Token::For, "`for`")?;
    let (first, _) = p.expect_ident("loop binding name")?;
    let mut bindings = vec![first];
    while matches!(p.peek(), Token::Comma) {
        p.advance();
        let (next, _) = p.expect_ident("additional loop binding name")?;
        bindings.push(next);
    }
    p.expect(&Token::In, "`in` between bindings and iterator")?;
    let iter = expr::parse_expr(p)?;
    let body = parse_block(p)?;
    let end = body.span.end;
    Ok(ForStmt {
        bindings,
        iter,
        body,
        span: Span::new(for_span.start, end),
    })
}

/// D21: `loop { body }`.
fn parse_loop(p: &mut Cursor) -> Result<LoopStmt, ParseError> {
    let loop_span = p.expect(&Token::Loop, "`loop`")?;
    let body = parse_block(p)?;
    let end = body.span.end;
    Ok(LoopStmt {
        body,
        span: Span::new(loop_span.start, end),
    })
}

fn parse_let(p: &mut Cursor) -> Result<LetStmt, ParseError> {
    let let_span = p.expect(&Token::Let, "`let`")?;

    // `owned` modifier (D25) — accepted at parse, body of analysis deferred.
    let owned = if matches!(p.peek(), Token::Owned) {
        p.advance();
        true
    } else {
        false
    };

    let (name, _) = p.expect_ident("variable name after `let`")?;

    // Optional `: Type` annotation.
    let ty: Option<TypeExpr> = if matches!(p.peek(), Token::Colon) {
        p.advance();
        Some(types::parse_type(p)?)
    } else {
        None
    };

    p.expect(&Token::Equals, "`=` in let binding")?;
    let value: Expr = expr::parse_expr(p)?;
    let end = value.span().end;

    Ok(LetStmt {
        name,
        owned,
        ty,
        value,
        span: Span::new(let_span.start, end),
    })
}

fn parse_return(p: &mut Cursor) -> Result<ReturnStmt, ParseError> {
    let ret_span = p.expect(&Token::Return, "`return`")?;
    let (value, end) = if matches!(p.peek(), Token::Newline | Token::RBrace) {
        (None, ret_span.end)
    } else {
        let e = expr::parse_expr(p)?;
        let end = e.span().end;
        (Some(e), end)
    };
    Ok(ReturnStmt {
        value,
        span: Span::new(ret_span.start, end),
    })
}
