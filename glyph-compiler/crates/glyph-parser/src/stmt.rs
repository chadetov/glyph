//! Statement and block parsing.

use glyph_ast::{
    Block, BreakStmt, ContinueStmt, Expr, ForBinding, ForStmt, LetBinding, LetStmt, LiteralPattern,
    LoopStmt, MutKind, MutStmt, Pattern, ReturnStmt, Stmt, TypeExpr,
};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::{LetPatternRefusal, NameRole, ParseError, SameLineStart};
use crate::expr;
use crate::pat;
use crate::types;

pub(crate) fn parse_block(p: &mut Cursor) -> Result<Block, ParseError> {
    let open = p.expect(&Token::LBrace, "`{`")?;
    let stmts = p.nested("block", open, |p| {
        let mut stmts = Vec::new();
        loop {
            p.skip_newlines();
            if matches!(p.peek(), Token::RBrace) {
                break;
            }
            let s = parse_stmt(p)?;
            // G351, G370: a statement does not begin on the line where the
            // previous one ended. Greedy parsing (D1) ends `let x = h()`
            // before `g` and `return h()` before `{ k: 2 }`, and reads what
            // follows as a separate statement, so a value the author wrote
            // was dropped or never ran without an error.
            if let Some(start) = same_line_start(p) {
                if !p.line_break_before_current() && !keeps_older_error(&s, start) {
                    let stmt = s.span();
                    let text = p.slice(stmt.start, stmt.end);
                    // Quoted only when it reads as one short line; a statement
                    // that spans lines is named, not reprinted.
                    let before = (!text.contains('\n') && text.chars().count() <= 60)
                        .then(|| text.to_string());
                    return Err(ParseError::StatementOnSameLine {
                        before,
                        start,
                        span: p.peek_span(),
                    });
                }
            }
            stmts.push(s);
            p.skip_newlines();
        }
        Ok(stmts)
    })?;
    let close = p.expect(&Token::RBrace, "`}`")?;
    Ok(Block {
        stmts,
        span: Span::new(open.start, close.end),
    })
}

/// TypeScript keywords that lex as identifiers in Glyph, and `const`, which
/// demotes to one in expression position. None of them can be bound (`const`
/// is a Glyph keyword, the rest are E0109 in the resolver, whose
/// `is_reserved_ts_word` holds every name here; a resolver test checks it),
/// so a bare one is always an unresolved name. `null`, `this`, `super`,
/// `eval` and `arguments` are left out: they are values in TypeScript, not
/// the start of a statement form.
pub const TS_STATEMENT_WORDS: &[&str] = &[
    "const",
    "var",
    "throw",
    "typeof",
    "delete",
    "yield",
    "function",
    "try",
    "catch",
    "finally",
    "while",
    "do",
    "switch",
    "case",
    "default",
    "class",
    "enum",
    "export",
    "extends",
    "implements",
    "instanceof",
    "debugger",
    "with",
    "package",
    "private",
    "protected",
    "public",
    "static",
];

/// Whether E0013 stands aside for `stmt` so the error 0.1.148 gave stays
/// (record 117, B1). `const y = 1` and `var y = 1` read as the name `const`
/// and then `y = 1`, which is E0008 with help to write `let`; `throw new
/// Error("x")` reads as the name `throw`, which is E0103. Both say what is
/// wrong, where E0013's "start the statement on its own line" would split
/// `const` from `y = 1` and lead nowhere.
///
/// Standing aside drops nothing silently: the word cannot be bound, so the
/// program fails to resolve even when nothing after it is an error. A `{`
/// keeps E0013, as G351 refused it in 0.1.146 (`try { ... }`).
fn keeps_older_error(stmt: &Stmt, start: SameLineStart) -> bool {
    start != SameLineStart::Brace
        && matches!(stmt, Stmt::Expr(Expr::Ident { name, .. })
            if TS_STATEMENT_WORDS.contains(&name.as_ref()))
}

/// What the token after a statement would begin, for E0013, or `None` when
/// another error says more. That is a token that cannot begin a statement at
/// all (`}`, `)`, `,`, `=`, `@`, `$`, a binary operator the expression did not
/// take), which keeps the error the next step gives it, naming the token
/// rather than a second statement. It is also a token whose statement already
/// has a dedicated error: `if` (E0006, record 117 N1), `const [` (E0230), and
/// a `name:` label (E0003 on the `:`, record 117 N2).
///
/// After a postfix `?` the token is a TypeScript `? :` conditional, or a
/// statement after error propagation, whatever it is (record 117, B2).
fn same_line_start(p: &mut Cursor) -> Option<SameLineStart> {
    let start = token_start(p.peek())?;
    if matches!(p.previous_token(), Some(Token::Question)) {
        return Some(SameLineStart::Ternary);
    }
    let next_is = |p: &mut Cursor, t: Token| p.peek_at(1) == Some(&t);
    let has_own_error = match p.peek() {
        Token::Identifier(_) => next_is(p, Token::Colon),
        Token::Const => next_is(p, Token::LBracket),
        _ => false,
    };
    (!has_own_error).then_some(start)
}

fn token_start(t: &Token) -> Option<SameLineStart> {
    Some(match t {
        Token::LBrace => SameLineStart::Brace,
        Token::As => SameLineStart::As,
        Token::String(_) | Token::Number(_) | Token::True | Token::False => SameLineStart::Literal,
        Token::Let
        | Token::Mut
        | Token::Return
        | Token::For
        | Token::Loop
        | Token::Break
        | Token::Continue
        | Token::Defer
        | Token::Const => SameLineStart::Keyword,
        Token::RBrace
        | Token::RParen
        | Token::RBracket
        | Token::RAngle
        | Token::Comma
        | Token::Colon
        | Token::Dot
        | Token::QDot
        | Token::Question
        | Token::Equals
        | Token::Arrow
        | Token::FatArrow
        | Token::EqEq
        | Token::BangEq
        | Token::LtEq
        | Token::GtEq
        | Token::AmpAmp
        | Token::PipePipe
        | Token::QQ
        | Token::DotDot
        | Token::DotDotDot
        | Token::Plus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::Pipe
        | Token::Amp
        | Token::Caret
        | Token::If
        | Token::Else
        | Token::At
        | Token::Dollar
        | Token::In
        | Token::Is
        | Token::Where
        | Token::JsxText(_)
        | Token::Newline
        | Token::Eof => return None,
        _ => SameLineStart::Other,
    })
}

/// Parse a single statement. Exposed for the match-arm parser, which allows
/// statement-shaped arm bodies like `Ok(_) => return 0,` and
/// `Ok(value) => mut result[key] = value,`.
pub(crate) fn parse_one_stmt(p: &mut Cursor) -> Result<Stmt, ParseError> {
    parse_stmt(p)
}

fn parse_stmt(p: &mut Cursor) -> Result<Stmt, ParseError> {
    let const_destructure =
        matches!(p.peek(), Token::Const) && matches!(p.peek_at(1), Some(Token::LBracket));
    match p.peek() {
        Token::Let => parse_let(p).map(Stmt::Let),
        // `const [a, b] = ...` in a body: name the rule rather than let `const`
        // demote to an identifier and the line fail as an index assignment.
        Token::Const if const_destructure => {
            Err(const_destructure_error(p))
        }
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
        // 0.1.16: `defer <expr>` runs its expression on exit of the enclosing
        // block (lowered to `try`/`finally`).
        Token::Defer => {
            let defer_span = p.expect(&Token::Defer, "`defer`")?;
            let expr = expr::parse_expr(p)?;
            let end = expr.span().end;
            Ok(Stmt::Defer(glyph_ast::DeferStmt {
                expr,
                span: Span::new(defer_span.start, end),
            }))
        }
        // `if`/`else` are keywords with no statement form (D3). Point at `match`
        // rather than emitting a generic "unexpected token" for the single most
        // common mistake a TypeScript-trained author makes.
        Token::If | Token::Else => Err(ParseError::NoConditionalKeyword {
            keyword: if matches!(p.peek(), Token::If) { "if" } else { "else" },
            span: p.peek_span(),
        }),
        _ => {
            let e = expr::parse_expr(p)?;
            // `x = e` with no `mut`. `=` is not an operator, so the expression
            // stops here and the generic path would report "unexpected token:
            // Equals" on the next statement. Name the rule instead (D5).
            if matches!(p.peek(), Token::Equals) {
                return Err(ParseError::MissingMutOnAssignment {
                    span: Span::new(e.span().start, p.peek_span().end),
                });
            }
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
        // `mut <method call>` — D5's method-call mutation form: the call must be
        // a *method* call (`x.method(args)`), not a free function or an
        // index-element call.
        _ if is_method_call(&target) => {
            let end = target.span().end;
            Ok(MutStmt {
                kind: MutKind::MethodCall { call: target },
                span: Span::new(mut_span.start, end),
            })
        }
        other => Err(ParseError::Expected {
            expected: "`=` after the assignment target, or a method call `x.method(...)` (D5)",
            found: format!("{other:?}"),
            span: p.peek_span(),
        }),
    }
}

/// Whether `e` is an assignable place (an lvalue): a name, or a chain of plain
/// (non-optional) field accesses and index subscripts bottoming out at one.
/// Calls, literals, operator expressions, and optional-chain (`?.`) accesses are
/// not assignable.
fn is_lvalue(e: &Expr) -> bool {
    match e {
        Expr::Ident { .. } => true,
        Expr::Member { object, optional, .. } => !optional && is_lvalue(object),
        Expr::Index { object, .. } => is_lvalue(object),
        _ => false,
    }
}

/// Whether `e` is a method call `<lvalue>.method(args)` — a call whose callee is
/// a plain field access. Excludes free-function calls and calls on an index
/// element, which D5 does not permit as `mut` statements.
fn is_method_call(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Call { callee, .. }
            if matches!(callee.as_ref(), Expr::Member { optional: false, .. })
    )
}

/// D21: `for X in expr { body }` and `for K, V in expr { body }`.
fn parse_for(p: &mut Cursor) -> Result<ForStmt, ParseError> {
    let for_span = p.expect(&Token::For, "`for`")?;
    let (first, first_span) = p.expect_ident("loop binding name")?;
    let mut bindings = vec![ForBinding { name: first, span: first_span }];
    while matches!(p.peek(), Token::Comma) {
        p.advance();
        let (next, next_span) = p.expect_ident("additional loop binding name")?;
        bindings.push(ForBinding { name: next, span: next_span });
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
    let owned_span = matches!(p.peek(), Token::Owned).then(|| p.peek_span());
    let owned = if owned_span.is_some() {
        p.advance();
        true
    } else {
        false
    };

    let binding = match p.peek() {
        // D48: `let [a, b] = expr`.
        Token::LBracket => {
            let pattern = pat::parse_pattern(p)?;
            if let Some(span) = owned_span {
                return Err(ParseError::DestructuringLet {
                    refusal: LetPatternRefusal::Owned,
                    span: span.join(pattern.span()),
                });
            }
            check_let_pattern(&pattern)?;
            if matches!(p.peek(), Token::Colon) {
                return Err(ParseError::DestructuringLet {
                    refusal: LetPatternRefusal::Annotated,
                    span: p.peek_span(),
                });
            }
            LetBinding::Pattern(pattern)
        }
        Token::LBrace => {
            let open = p.peek_span();
            let pattern = pat::parse_pattern(p)?;
            return Err(ParseError::ObjectDestructuringLet {
                span: open.join(pattern.span()),
            });
        }
        // `let owned = 1`: the modifier was taken, and what follows is the
        // `=` or `:` of a binding with no name, so the author meant `owned` as
        // the name. Say it is reserved, at the word they wrote (G286).
        Token::Equals | Token::Colon if owned => {
            return Err(ParseError::ReservedWordAsName {
                word: Token::Owned.as_field_name().unwrap_or("owned"),
                role: NameRole::Declares,
                span: owned_span.unwrap_or(let_span),
            });
        }
        _ => {
            let (name, _) = p.expect_ident("variable name after `let`")?;
            LetBinding::Name(name)
        }
    };

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
        binding,
        owned,
        ty,
        value,
        span: Span::new(let_span.start, end),
    })
}

/// `const [a, b] = ...`, with the cursor on `const` (D20, D48): a `const` binds
/// one name. The span covers `const` and the pattern, which is parsed only to
/// find its end.
pub(crate) fn const_destructure_error(p: &mut Cursor) -> ParseError {
    let start = p.peek_span();
    p.advance();
    let span = match pat::parse_pattern(p) {
        Ok(pattern) => start.join(pattern.span()),
        Err(_) => start,
    };
    ParseError::DestructuringLet {
        refusal: LetPatternRefusal::Const,
        span,
    }
}

/// D48: a destructuring `let` always matches. Its pattern holds lowercase
/// bindings, `_`, nested array patterns and a trailing rest that is a name or
/// `_`; anything that tests the value is refused here, where no type is needed
/// to know it can fail. An object pattern is refused as the construct that is
/// not designed yet (E0005), wherever it sits in the pattern.
fn check_let_pattern(pattern: &Pattern) -> Result<(), ParseError> {
    let refuse = |what: String, span: Span| ParseError::DestructuringLet {
        refusal: LetPatternRefusal::Refutable { what },
        span,
    };
    match pattern {
        Pattern::Wildcard { .. } => Ok(()),
        Pattern::Ident { name, span } => match glyph_ast::is_variant_shaped(name) {
            true => Err(refuse(format!("the variant name `{name}`"), *span)),
            false => Ok(()),
        },
        Pattern::Array { elements, rest, .. } => {
            for e in elements {
                check_let_pattern(e)?;
            }
            match rest.as_deref() {
                None | Some(Pattern::Wildcard { .. }) => Ok(()),
                Some(r @ Pattern::Ident { .. }) => check_let_pattern(r),
                Some(other) => Err(refuse(
                    "a rest that is not a name".to_string(),
                    other.span(),
                )),
            }
        }
        Pattern::Object { span, .. } => Err(ParseError::ObjectDestructuringLet { span: *span }),
        Pattern::Literal { value, span } => {
            let written = match value {
                LiteralPattern::Number(raw) => raw.clone(),
                LiteralPattern::String(s) => format!("\"{s}\""),
                LiteralPattern::Bool(b) => b.to_string(),
                LiteralPattern::Void => "void".to_string(),
            };
            Err(refuse(format!("the literal `{written}`"), *span))
        }
        Pattern::Constructor { path, args, span } => {
            let head = path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".");
            let what = match args.is_empty() {
                true => format!("the variant name `{head}`"),
                false => format!("the constructor pattern `{head}(..)`"),
            };
            Err(refuse(what, *span))
        }
        Pattern::IsType { span, .. } => Err(refuse("an `is` guard".to_string(), *span)),
        // `else` never reaches here: `parse_pattern` refuses it outside an arm.
        Pattern::Else { span } => Err(refuse("`else`".to_string(), *span)),
    }
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
