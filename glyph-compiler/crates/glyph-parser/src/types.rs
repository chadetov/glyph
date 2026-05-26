//! Type expression parsing. Day 3 scope:
//! - `path.with.dots` paths
//! - `Generic<T1, T2>`
//! - `fn(name: T, T) -> U` function types
//! - `{ field: T, optional?: T }` inline record types
//! - Tagged unions `A | B | C` (single line) and `\n  | A | B` (multi-line) per D8

use glyph_ast::{FnTypeParam, RecordTypeField, TypeExpr, UnionVariant};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::ParseError;

/// Public entry. Parses a type expression and, if a `|` follows, treats the
/// result as the first variant of a tagged union (D8 single-line form).
pub(crate) fn parse_type(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    let first = parse_atom_type(p)?;

    // Single-line tagged union: `A | B | C`. The first atom must be a
    // path-shape that can act as a variant name with optional payload.
    if matches!(p.peek(), Token::Pipe) {
        return parse_union_continuation(p, first);
    }
    Ok(first)
}

/// Parse the right-hand side of a `type X = ...` declaration. Recognizes the
/// multi-line tagged union form (`type Y =\n  | A\n  | B`) per D8 in addition
/// to whatever `parse_type` accepts.
pub(crate) fn parse_type_decl_body(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    p.skip_newlines();
    if matches!(p.peek(), Token::Pipe) {
        return parse_union_multiline(p);
    }
    parse_type(p)
}

// ---------------------------------------------------------------------------
// Atom types
// ---------------------------------------------------------------------------

fn parse_atom_type(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    match p.peek() {
        Token::LBrace => parse_record_type(p),
        Token::Fn => parse_fn_type(p),
        Token::Identifier(_) | Token::Void => parse_path_type(p),
        other => Err(ParseError::Expected {
            expected: "type expression",
            found: format!("{other:?}"),
            span: p.peek_span(),
        }),
    }
}

fn parse_path_type(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    let start = p.peek_span();
    let mut segments = Vec::new();
    let mut end = start.end;

    // `void` is a reserved word; treat it as a one-segment path so the rest
    // of the compiler doesn't need a special case.
    if matches!(p.peek(), Token::Void) {
        p.advance();
        segments.push(std::sync::Arc::from("void"));
    } else {
        let (name, span) = p.expect_ident("type name")?;
        segments.push(name);
        end = span.end;
    }

    while matches!(p.peek(), Token::Dot) {
        p.advance();
        let (next, span) = p.expect_ident("type path segment")?;
        segments.push(next);
        end = span.end;
    }

    let base = TypeExpr::Path {
        segments,
        span: Span::new(start.start, end),
    };

    if matches!(p.peek(), Token::LAngle) {
        return parse_generic_args(p, base, start.start);
    }
    Ok(base)
}

fn parse_generic_args(
    p: &mut Cursor,
    base: TypeExpr,
    start: u32,
) -> Result<TypeExpr, ParseError> {
    p.expect(&Token::LAngle, "`<`")?;
    let args = p.parse_comma_separated(&Token::RAngle, false, parse_type)?;
    let end_span = p.expect(&Token::RAngle, "`>`")?;
    Ok(TypeExpr::Generic {
        base: Box::new(base),
        args,
        span: Span::new(start, end_span.end),
    })
}

fn parse_record_type(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    let open = p.expect(&Token::LBrace, "`{`")?;
    let fields = p.parse_comma_separated(&Token::RBrace, true, |p| {
        let field_start = p.peek_span();
        let (name, _) = p.expect_field_name("record field name")?;
        let optional = matches!(p.peek(), Token::Question);
        if optional {
            p.advance();
        }
        p.expect(&Token::Colon, "`:` after field name")?;
        let ty = parse_type(p)?;
        let end = ty.span().end;
        Ok(RecordTypeField {
            name,
            ty,
            optional,
            span: Span::new(field_start.start, end),
        })
    })?;
    let close = p.expect(&Token::RBrace, "`}`")?;
    Ok(TypeExpr::Record {
        fields,
        span: Span::new(open.start, close.end),
    })
}

fn parse_fn_type(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    let start = p.expect(&Token::Fn, "`fn`")?;
    p.expect(&Token::LParen, "`(` after `fn` in type")?;
    // Each param is `name: T` or bare `T`. Decided by lookahead at `peek_at(1)`.
    let params = p.parse_comma_separated(&Token::RParen, false, |p| {
        let param_start = p.peek_span();
        let (name, ty) = if matches!(p.peek_at(1), Some(Token::Colon)) {
            let (n, _) = p.expect_ident("parameter name in fn type")?;
            p.advance();
            (Some(n), parse_type(p)?)
        } else {
            (None, parse_type(p)?)
        };
        let end = ty.span().end;
        Ok(FnTypeParam {
            name,
            ty,
            span: Span::new(param_start.start, end),
        })
    })?;
    let close = p.expect(&Token::RParen, "`)`")?;
    let (return_ty, end) = if matches!(p.peek(), Token::Arrow) {
        p.advance();
        let t = parse_type(p)?;
        let e = t.span().end;
        (Some(Box::new(t)), e)
    } else {
        (None, close.end)
    };
    Ok(TypeExpr::Fn {
        params,
        return_ty,
        span: Span::new(start.start, end),
    })
}

// ---------------------------------------------------------------------------
// Tagged unions (D8)
// ---------------------------------------------------------------------------

fn parse_union_continuation(p: &mut Cursor, first: TypeExpr) -> Result<TypeExpr, ParseError> {
    let start = first.span().start;
    let first_variant = type_to_variant(first)?;
    let mut variants = vec![first_variant];

    while matches!(p.peek(), Token::Pipe) {
        p.advance();
        p.skip_newlines();
        variants.push(parse_variant(p)?);
    }

    let end = variants.last().unwrap().span.end;
    Ok(TypeExpr::Union {
        variants,
        span: Span::new(start, end),
    })
}

fn parse_union_multiline(p: &mut Cursor) -> Result<TypeExpr, ParseError> {
    let start_span = p.peek_span();
    let mut variants = Vec::new();
    while matches!(p.peek(), Token::Pipe) {
        p.advance();
        p.skip_newlines();
        variants.push(parse_variant(p)?);
        p.skip_newlines();
    }
    let end = variants.last().map(|v| v.span.end).unwrap_or(start_span.start);
    Ok(TypeExpr::Union {
        variants,
        span: Span::new(start_span.start, end),
    })
}

fn parse_variant(p: &mut Cursor) -> Result<UnionVariant, ParseError> {
    let start = p.peek_span();
    let (name, _) = p.expect_ident("variant name")?;
    let payload = if matches!(p.peek(), Token::LParen) {
        p.advance();
        let ty = parse_type(p)?;
        p.expect(&Token::RParen, "`)` after variant payload")?;
        Some(ty)
    } else {
        None
    };
    let end = payload.as_ref().map(|t| t.span().end).unwrap_or(start.end);
    Ok(UnionVariant {
        name,
        payload,
        span: Span::new(start.start, end),
    })
}

/// Convert an arbitrary `TypeExpr` into a `UnionVariant` if possible (first
/// variant in a single-line union: `A | B | C` — `A` was parsed as a path or
/// generic, and we promote it to a variant).
fn type_to_variant(ty: TypeExpr) -> Result<UnionVariant, ParseError> {
    match ty {
        TypeExpr::Path { ref segments, span } if segments.len() == 1 => Ok(UnionVariant {
            name: segments[0].clone(),
            payload: None,
            span,
        }),
        TypeExpr::Generic { span, .. } => {
            // `Name<T>` is syntactically possible but the corpus uses `Name(T)`
            // for variant payloads. Reject generics in variant-head position.
            Err(ParseError::Unexpected {
                found: "generic head in union variant position".to_string(),
                span,
            })
        }
        TypeExpr::Path { span, .. } => Err(ParseError::Unexpected {
            found: "dotted path in union variant position".to_string(),
            span,
        }),
        other => Err(ParseError::Unexpected {
            found: format!("non-variant type {:?} in union", other),
            span: other.span(),
        }),
    }
}
