//! Pattern parser (D2/D3/D9 supporting infrastructure).
//!
//! Recognized patterns in this slice:
//! - `_`                                      wildcard (D9)
//! - `else`                                   catch-all arm pattern (D9, position-restricted to whole arm)
//! - identifier                               binding
//! - literal (number, string, true, false, void)
//! - `Constructor(args)`                      with nested arg patterns
//! - `{ field, field: alias }`                object destructure; a field value
//!   only binds, so a PascalCase `alias` is rejected as E0009
//! - `is TypeName`                            type-guard pattern
//!
//! Array patterns (`[]`, `[h, ...r]`, `[_, ...]`) — deferred to day 4.

use glyph_ast::{LiteralPattern, ObjectPatternField, Pattern};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::ParseError;
use crate::types;

/// Parse a match-arm pattern. `else` is legal here (D9 catch-all). Falls
/// through to `parse_pattern_inner` once the `else` case is handled.
pub(crate) fn parse_arm_pattern(p: &mut Cursor) -> Result<Pattern, ParseError> {
    if matches!(p.peek(), Token::Else) {
        let span = p.peek_span();
        p.advance();
        return Ok(Pattern::Else { span });
    }
    parse_pattern_inner(p)
}

/// Parse a non-arm pattern (nested in a constructor or array pattern).
/// `else` is illegal in this position per D9.
pub(crate) fn parse_pattern(p: &mut Cursor) -> Result<Pattern, ParseError> {
    parse_pattern_inner(p)
}

fn parse_pattern_inner(p: &mut Cursor) -> Result<Pattern, ParseError> {
    let pattern = parse_pattern_atom(p)?;
    // Range / comparison patterns (`500..599 =>`) are not supported in v1.
    // The `..` token lexes fine but has no meaning in pattern position, so
    // catch it here with a targeted diagnostic instead of letting the caller
    // report a misleading "expected `=>`" against the `DotDot` token.
    if matches!(p.peek(), Token::DotDot) {
        return Err(ParseError::UnsupportedRangePattern {
            span: p.peek_span(),
        });
    }
    Ok(pattern)
}

fn parse_pattern_atom(p: &mut Cursor) -> Result<Pattern, ParseError> {
    let start_span = p.peek_span();
    match p.peek().clone() {
        Token::Identifier(name) if name.as_ref() == "_" => {
            // The identifier `_` is the wildcard binding (D9). Name
            // resolution does not register a binding.
            p.advance();
            Ok(Pattern::Wildcard { span: start_span })
        }
        Token::Is => {
            p.advance();
            let ty = types::parse_type(p)?;
            let end = ty.span().end;
            Ok(Pattern::IsType {
                ty,
                span: Span::new(start_span.start, end),
            })
        }
        Token::Number(raw) => {
            p.advance();
            Ok(Pattern::Literal {
                value: LiteralPattern::Number(raw),
                span: start_span,
            })
        }
        // A negative-number literal pattern (`-1 => ...`). Only a bare numeric
        // literal may follow the sign; `-x` and `-(...)` are not patterns. The
        // sign folds into the literal's raw text so emit and exhaustiveness see
        // a single `Number` pattern, exactly like its positive counterpart.
        Token::Minus if matches!(p.peek_at(1), Some(Token::Number(_))) => {
            p.advance(); // `-`
            let num_span = p.peek_span();
            let raw = match p.peek().clone() {
                Token::Number(raw) => raw,
                _ => unreachable!("guarded by peek_at(1)"),
            };
            p.advance(); // the numeric literal
            Ok(Pattern::Literal {
                value: LiteralPattern::Number(format!("-{raw}")),
                span: Span::new(start_span.start, num_span.end),
            })
        }
        Token::String(value) => {
            p.advance();
            Ok(Pattern::Literal {
                // `\${` in a string pattern is a literal `${`; resolve the marker.
                value: LiteralPattern::String(
                    glyph_lexer::resolve_escaped_dollars(&value).into_owned(),
                ),
                span: start_span,
            })
        }
        Token::True => {
            p.advance();
            Ok(Pattern::Literal {
                value: LiteralPattern::Bool(true),
                span: start_span,
            })
        }
        Token::False => {
            p.advance();
            Ok(Pattern::Literal {
                value: LiteralPattern::Bool(false),
                span: start_span,
            })
        }
        Token::Void => {
            p.advance();
            Ok(Pattern::Literal {
                value: LiteralPattern::Void,
                span: start_span,
            })
        }
        Token::LBrace => parse_object_pattern(p),
        Token::LBracket => parse_array_pattern(p),
        Token::Identifier(name) => {
            p.advance();
            // Dotted path → multi-segment variant pattern.
            if matches!(p.peek(), Token::Dot) {
                let mut path = vec![name];
                let mut last_end = start_span.end;
                while matches!(p.peek(), Token::Dot) {
                    p.advance();
                    let (next, span) = p.expect_field_name("path segment in variant pattern")?;
                    path.push(next);
                    last_end = span.end;
                }
                // Optional argument list `(...)`.
                let (args, end) = if matches!(p.peek(), Token::LParen) {
                    parse_constructor_args(p)?
                } else {
                    (Vec::new(), last_end)
                };
                return Ok(Pattern::Constructor {
                    path,
                    args,
                    span: Span::new(start_span.start, end),
                });
            }
            // Constructor pattern if followed by `(`.
            if matches!(p.peek(), Token::LParen) {
                let (args, end) = parse_constructor_args(p)?;
                return Ok(Pattern::Constructor {
                    path: vec![name],
                    args,
                    span: Span::new(start_span.start, end),
                });
            }
            // Otherwise a bare identifier binding (typechecker disambiguates
            // "no-payload variant `Foo`" from "binding `foo`" via scrutinee).
            Ok(Pattern::Ident {
                name,
                span: start_span,
            })
        }
        other => Err(ParseError::Unexpected {
            found: format!("{other:?}"),
            span: start_span,
        }),
    }
}

fn parse_object_pattern(p: &mut Cursor) -> Result<Pattern, ParseError> {
    let open = p.expect(&Token::LBrace, "`{`")?;
    let fields = p.parse_comma_separated(&Token::RBrace, false, |p| {
        let key_span = p.peek_span();
        let (key, _) = p.expect_field_name("field name in object pattern")?;
        let binding = if matches!(p.peek(), Token::Colon) {
            p.advance();
            let binding_span = p.peek_span();
            let (binding, _) = p.expect_field_name("binding identifier in object pattern")?;
            // A field value is a pattern position, and D9 reads a PascalCase
            // name there as a variant reference. Glyph object patterns only
            // destructure, so there is no lowering that means "match `Black`";
            // without this check the name became a renamed binding and the arm
            // matched every payload while shadowing the real constructor.
            if is_constructor_shaped(&binding) {
                return Err(ParseError::VariantInObjectPatternField {
                    key: key.to_string(),
                    name: binding.to_string(),
                    span: binding_span,
                });
            }
            Some(binding)
        } else {
            None
        };
        let end = p.peek_span().start;
        Ok(ObjectPatternField {
            key,
            binding,
            span: Span::new(key_span.start, end),
        })
    })?;
    let close = p.expect(&Token::RBrace, "`}`")?;
    Ok(Pattern::Object {
        fields,
        span: Span::new(open.start, close.end),
    })
}

/// Whether a bare ident in pattern position is constructor-shaped: a PascalCase
/// name (`Black`, `Ok`, `None`) denotes a union variant reference, a lowercase
/// or underscore-led name (`c`, `_rest`) is a fresh binding. Mirrors the
/// identically named checks in `glyph_resolver` and `glyph_typechecker` so every
/// stage agrees on what counts as a variant reference (D9).
fn is_constructor_shaped(name: &glyph_ast::Ident) -> bool {
    name.as_ref()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
}

/// Parse `( pattern, pattern, ... )` for a constructor pattern. Returns args
/// and the end offset of the closing `)`.
fn parse_constructor_args(p: &mut Cursor) -> Result<(Vec<Pattern>, u32), ParseError> {
    p.expect(&Token::LParen, "`(`")?;
    let args = p.parse_comma_separated(&Token::RParen, false, |p| {
        parse_pattern(p)
    })?;
    let close = p.expect(&Token::RParen, "`)`")?;
    Ok((args, close.end))
}

/// D9 array pattern. Examples from the corpus:
/// - `[]`                            (empty)
/// - `[head, ...rest]`              (rest binding)
/// - `[_, ...]`                     (rest discard)
/// - `["help", ..._]`               (literal first element + rest discard)
/// - `["done", id_str]`             (two elements, no rest)
/// - `[other, ..._]`                (binding first + rest discard)
fn parse_array_pattern(p: &mut Cursor) -> Result<Pattern, ParseError> {
    let open = p.expect(&Token::LBracket, "`[`")?;
    let mut elements = Vec::new();
    let mut rest: Option<Box<Pattern>> = None;

    while !matches!(p.peek(), Token::RBracket) {
        if matches!(p.peek(), Token::DotDotDot) {
            // Rest element. The pattern after `...` may be a binding ident,
            // a wildcard, or omitted (then `...` alone — corpus uses `..._`).
            let dots_span = p.peek_span();
            p.advance();
            let inner = if matches!(p.peek(), Token::Comma | Token::RBracket) {
                // Bare `...` — treat as wildcard rest.
                Pattern::Wildcard { span: dots_span }
            } else {
                parse_pattern(p)?
            };
            rest = Some(Box::new(inner));
            // After rest, no more elements allowed before `]`.
            if matches!(p.peek(), Token::Comma) {
                p.advance();
            }
            break;
        }
        elements.push(parse_pattern(p)?);
        if matches!(p.peek(), Token::Comma) {
            p.advance();
        } else {
            break;
        }
    }
    let close = p.expect(&Token::RBracket, "`]`")?;
    Ok(Pattern::Array {
        elements,
        rest,
        span: Span::new(open.start, close.end),
    })
}
