//! Declaration parsing: module, import, fn, type, const.

use std::sync::Arc;

use glyph_ast::{
    Annotation, ComponentDecl, ConstDecl, Decl, FnDecl, GenericParam, ImportDecl, ImportKind,
    InterfaceDecl, InterfaceMember, Module, ModulePath, Param, RecordTypeField, TypeDecl,
};
use glyph_lexer::{Span, Token};

use crate::cursor::Cursor;
use crate::error::ParseError;
use crate::expr;
use crate::stmt;
use crate::types;

pub(crate) fn parse_module(p: &mut Cursor) -> Result<Module, ParseError> {
    let start = p.peek_span().start;
    p.skip_newlines();

    let module_path = if matches!(p.peek(), Token::Module) {
        Some(parse_module_decl(p)?)
    } else {
        None
    };

    let mut items = Vec::new();
    p.skip_newlines();
    while !p.is_at_end() {
        let decl = parse_top_level(p)?;
        items.push(decl);
        p.skip_newlines();
    }

    let end = p.peek_span().end;
    Ok(Module {
        module_path,
        items,
        span: Span::new(start, end),
    })
}

fn parse_module_decl(p: &mut Cursor) -> Result<ModulePath, ParseError> {
    let module_span = p.expect(&Token::Module, "`module`")?;
    let path = parse_dotted_path(p, module_span, /* allow_scope */ false)?;
    // Module decl must be terminated by newline or EOF.
    if matches!(p.peek(), Token::Newline) {
        p.advance();
    } else if !p.is_at_end() {
        return Err(ParseError::Expected {
            expected: "newline after module declaration",
            found: format!("{:?}", p.peek()),
            span: p.peek_span(),
        });
    }
    Ok(path)
}

/// Parse `seg1/seg2/seg3` into a `ModulePath`. The slash is the module
/// separator (D15); we lex it as `Slash`, distinct from path-position usage.
///
/// Segments accept hyphens so npm package specifiers (`react-hook-form`) and
/// hyphenated file names round-trip. When `allow_scope` is set (import paths,
/// not `module` declarations) a leading `@` introduces an npm scope, so
/// `@hookform/resolvers/zod` parses as segments `@hookform`, `resolvers`, `zod`.
fn parse_dotted_path(
    p: &mut Cursor,
    start_span: Span,
    allow_scope: bool,
) -> Result<ModulePath, ParseError> {
    // Path segments accept keyword-spelled names (`std/record`, a file named
    // `type.glyph`, ...): a module/file name is not restricted to non-keywords.
    let mut segments = Vec::new();

    // npm scoped-package prefix (`@scope/pkg/...`), imports only.
    let scoped = allow_scope && matches!(p.peek(), Token::At);
    if scoped {
        p.advance();
    }
    let (first, first_span) = p.expect_hyphenated_name("module path segment")?;
    let first = if scoped {
        std::sync::Arc::from(format!("@{first}").as_str())
    } else {
        first
    };
    segments.push(first);
    let mut end_span = first_span;

    while matches!(p.peek(), Token::Slash) {
        p.advance();
        let (seg, span) = p.expect_hyphenated_name("module path segment")?;
        segments.push(seg);
        end_span = span;
    }

    Ok(ModulePath {
        segments,
        span: Span::new(start_span.start, end_span.end),
    })
}

fn parse_top_level(p: &mut Cursor) -> Result<Decl, ParseError> {
    // D27: collect any leading `@<name> <args>` annotations that decorate
    // this declaration. They attach to the next fn/type/component/const.
    let annotations = parse_annotations(p)?;

    // 0.1.16: optional `pub` visibility prefix. Declarations are module-private
    // by default; `pub` exports them. `pub` sits between the annotations and the
    // declaration keyword (`pub fn`, `pub async fn`, `pub type`, `pub const`,
    // `pub component`, `pub interface`, `pub resource type`).
    let is_public = if matches!(p.peek(), Token::Pub) {
        p.advance();
        true
    } else {
        false
    };

    match p.peek() {
        Token::Import => {
            // Imports do not carry annotations or `pub` in v1.
            if !annotations.is_empty() {
                return Err(ParseError::Unexpected {
                    found: "@annotation on `import`".to_string(),
                    span: annotations[0].span,
                });
            }
            if is_public {
                return Err(ParseError::Unexpected {
                    found: "`pub` on `import` (an import re-binds another module's name)".to_string(),
                    span: p.peek_span(),
                });
            }
            parse_import(p).map(Decl::Import)
        }
        Token::Fn => parse_fn(p, /* is_async */ false, annotations, is_public).map(Decl::Fn),
        Token::Async => {
            p.advance();
            if !matches!(p.peek(), Token::Fn) {
                return Err(ParseError::Expected {
                    expected: "`fn` after `async`",
                    found: format!("{:?}", p.peek()),
                    span: p.peek_span(),
                });
            }
            parse_fn(p, true, annotations, is_public).map(Decl::Fn)
        }
        Token::Type => parse_type_decl(p, annotations, is_public).map(Decl::Type),
        // D25: `resource type X = ...` marks a resource handle type. The
        // `resource` keyword only precedes `type`; anything else is an error.
        Token::Resource => parse_type_decl(p, annotations, is_public).map(Decl::Type),
        Token::Const => parse_const_decl(p, annotations, is_public).map(Decl::Const),
        Token::Component => parse_component(p, annotations, is_public).map(Decl::Component),
        Token::Interface => parse_interface(p, annotations, is_public).map(Decl::Interface),
        // record (top-level `record X { ... }`) — deferred to v1.1 cleanup;
        // for now records are written as `type X = { ... }` per D8 inline.
        other => Err(ParseError::Unexpected {
            found: format!("{other:?}"),
            span: p.peek_span(),
        }),
    }
}

/// 0.1.16: `interface Name<T> { fn method(p: P) -> R  field: T }`. A structural
/// interface: a set of member signatures, usable as a generic bound and as a
/// type. Members are newline-separated; a `fn` member is a method signature, a
/// `name: Type` member is a property.
fn parse_interface(
    p: &mut Cursor,
    annotations: Vec<Annotation>,
    is_public: bool,
) -> Result<InterfaceDecl, ParseError> {
    let kw_span = p.expect(&Token::Interface, "`interface`")?;
    let (name, _) = p.expect_ident("interface name")?;
    let generics = if matches!(p.peek(), Token::LAngle) {
        parse_generic_params(p)?
    } else {
        Vec::new()
    };
    p.expect(&Token::LBrace, "`{` (interface body)")?;
    let mut members = Vec::new();
    p.skip_newlines();
    while !matches!(p.peek(), Token::RBrace) && !p.is_at_end() {
        members.push(parse_interface_member(p)?);
        // Members are separated by a newline or an optional comma.
        if matches!(p.peek(), Token::Comma) {
            p.advance();
        }
        p.skip_newlines();
    }
    let close = p.expect(&Token::RBrace, "`}` (interface body)")?;
    if matches!(p.peek(), Token::Newline) {
        p.advance();
    }
    Ok(InterfaceDecl {
        name,
        annotations,
        is_public,
        generics,
        members,
        span: Span::new(kw_span.start, close.end),
    })
}

fn parse_interface_member(p: &mut Cursor) -> Result<InterfaceMember, ParseError> {
    if matches!(p.peek(), Token::Fn) {
        // `fn name(params) -> ret` — a method signature (no body).
        let fn_span = p.expect(&Token::Fn, "`fn`")?;
        let (name, _) = p.expect_ident("method name")?;
        p.expect(&Token::LParen, "`(`")?;
        let params = p.parse_comma_separated(&Token::RParen, true, parse_param)?;
        let rparen = p.expect(&Token::RParen, "`)`")?;
        let (return_ty, end) = if matches!(p.peek(), Token::Arrow) {
            p.advance();
            let ty = types::parse_type(p)?;
            let end = ty.span().end;
            (Some(ty), end)
        } else {
            (None, rparen.end)
        };
        Ok(InterfaceMember::Method {
            name,
            params,
            return_ty,
            span: Span::new(fn_span.start, end),
        })
    } else {
        // `name: Type` or `name?: Type` — a property signature.
        let (name, name_span) = p.expect_field_name("interface member name")?;
        let optional = if matches!(p.peek(), Token::Question) {
            p.advance();
            true
        } else {
            false
        };
        p.expect(&Token::Colon, "`:` after interface member name")?;
        let ty = types::parse_type(p)?;
        let end = ty.span().end;
        Ok(InterfaceMember::Field(RecordTypeField {
            name,
            ty,
            optional,
            span: Span::new(name_span.start, end),
        }))
    }
}

/// Whether the newline the cursor is sitting on continues the annotation rather
/// than ending it: true when the next line starts with an operator that can only
/// be the middle of an expression.
///
/// `Minus` and `Bang` are deliberately absent. Both can begin an expression
/// (`-1`, `!flag`), so a line starting with one is genuinely ambiguous, and
/// treating it as a continuation would swallow whatever follows the annotation.
fn continues_after_newline(p: &Cursor) -> bool {
    let mut i = 0;
    while matches!(p.peek_at(i), Some(Token::Newline)) {
        i += 1;
    }
    matches!(
        p.peek_at(i),
        Some(
            Token::EqEq
                | Token::BangEq
                | Token::AmpAmp
                | Token::PipePipe
                | Token::LtEq
                | Token::GtEq
                | Token::LAngle
                | Token::RAngle
                | Token::Plus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::Dot
                | Token::QDot
                | Token::QQ
                | Token::Question
                | Token::Amp
                | Token::Pipe
                | Token::Caret
        )
    )
}

/// D27: parse zero or more `@<name> <raw args until newline>` annotations.
/// The raw-args text is captured as a source slice; the typechecker parses
/// it later (per `Annotation.raw_args`).
fn parse_annotations(p: &mut Cursor) -> Result<Vec<Annotation>, ParseError> {
    let mut annotations = Vec::new();
    loop {
        p.skip_newlines();
        if !matches!(p.peek(), Token::At) {
            break;
        }
        let at_span = p.expect(&Token::At, "`@`")?;
        let (name, name_span) = p.expect_field_name("annotation name after `@`")?;
        // Scan to end of line, capturing the raw args source.
        //
        // A newline ends an annotation, except where the next line begins with a
        // binary or postfix operator. An annotation whose argument is a real
        // expression is often long (an `@example` comparing a parsed frame of
        // JSON against a record literal has nowhere sensible to sit on one
        // line), and the alternative was naming a helper function per example
        // purely to get under a line length.
        //
        // Nothing about it is ambiguous: a line starting with `==`, `&&`, `+`,
        // `.` and the rest cannot begin a declaration or another annotation, so
        // it can only be the continuation of this one. A newline *inside*
        // brackets never reaches here at all, because the lexer only emits one
        // at bracket depth zero (D1), so a wrapped argument list already worked.
        let args_start = p.peek_span().start;
        let mut args_end = name_span.end;
        // Each span skipped by a continuation: the newline and the indentation
        // that follows it, which is spliced out when the slice is assembled.
        let mut gaps: Vec<(u32, u32)> = Vec::new();
        loop {
            match p.peek() {
                Token::Eof => break,
                Token::Newline => {
                    if !continues_after_newline(p) {
                        break;
                    }
                    let gap_start = p.peek_span().start;
                    while matches!(p.peek(), Token::Newline) {
                        p.advance();
                    }
                    gaps.push((gap_start, p.peek_span().start));
                }
                _ => {
                    args_end = p.peek_span().end;
                    p.advance();
                }
            }
        }
        // A continued annotation's slice spans the line breaks it continued
        // across. Each is replaced by a single space, which leaves the
        // expression identical: those breaks sit *between tokens*, and Glyph has
        // no significant whitespace there.
        //
        // Only the recorded gaps are touched, never the slice as a whole. A
        // blanket replace would also rewrite anything that looked like a line
        // break inside a string literal, which is the author's data.
        let raw_args = if args_end > args_start {
            let mut out = String::new();
            let mut at = args_start;
            for (gs, ge) in &gaps {
                if *gs >= args_end {
                    break;
                }
                out.push_str(p.slice(at, *gs));
                out.push(' ');
                at = *ge;
            }
            out.push_str(p.slice(at, args_end));
            out.trim().to_string()
        } else {
            String::new()
        };
        annotations.push(Annotation {
            name,
            raw_args,
            span: Span::new(at_span.start, args_end),
        });
    }
    Ok(annotations)
}

/// D19: `component Name<T>(props: P) -> Component { body }`. Grammatically
/// parallel to `fn`; the return type defaults to `Component` if omitted.
fn parse_component(
    p: &mut Cursor,
    annotations: Vec<Annotation>,
    is_public: bool,
) -> Result<ComponentDecl, ParseError> {
    let kw_span = p.expect(&Token::Component, "`component`")?;
    let (name, _) = p.expect_ident("component name")?;
    let sig = parse_callable_signature(p)?;
    Ok(ComponentDecl {
        name,
        annotations,
        is_public,
        generics: sig.generics,
        params: sig.params,
        return_ty: sig.return_ty,
        body: sig.body,
        span: Span::new(kw_span.start, sig.end),
    })
}

fn parse_type_decl(
    p: &mut Cursor,
    annotations: Vec<Annotation>,
    is_public: bool,
) -> Result<TypeDecl, ParseError> {
    // D25: an optional leading `resource` marks the type as a resource handle.
    // The declaration still starts at `resource` when present so the span
    // covers the whole form.
    let (start, is_resource) = if matches!(p.peek(), Token::Resource) {
        let res_span = p.expect(&Token::Resource, "`resource`")?;
        (res_span.start, true)
    } else {
        (p.peek_span().start, false)
    };
    p.expect(&Token::Type, "`type`")?;
    let (name, _) = p.expect_ident("type name")?;
    let generics = if matches!(p.peek(), Token::LAngle) {
        parse_generic_params(p)?
    } else {
        Vec::new()
    };
    p.expect(&Token::Equals, "`=` after type name")?;
    let body = types::parse_type_decl_body(p)?;
    let mut end = body.span().end;
    // D39: an optional `where <predicate>` refinement. The predicate is a boolean
    // expression over a bound `value`; it is woven into the type's runtime
    // descriptor so `.parse` rejects a value that fails it.
    let refinement = if matches!(p.peek(), Token::Where) {
        p.advance();
        let pred = expr::parse_expr(p)?;
        end = pred.span().end;
        Some(Box::new(pred))
    } else {
        None
    };
    if matches!(p.peek(), Token::Newline) {
        p.advance();
    }
    Ok(TypeDecl {
        name,
        annotations,
        is_public,
        generics,
        is_resource,
        body,
        refinement,
        span: Span::new(start, end),
    })
}

fn parse_const_decl(
    p: &mut Cursor,
    annotations: Vec<Annotation>,
    is_public: bool,
) -> Result<ConstDecl, ParseError> {
    let const_span = p.expect(&Token::Const, "`const`")?;
    let (name, _) = p.expect_ident("constant name")?;
    let ty = if matches!(p.peek(), Token::Colon) {
        p.advance();
        Some(types::parse_type(p)?)
    } else {
        None
    };
    p.expect(&Token::Equals, "`=` in const declaration")?;
    let value = expr::parse_expr(p)?;
    let end = value.span().end;
    if matches!(p.peek(), Token::Newline) {
        p.advance();
    }
    Ok(ConstDecl {
        name,
        annotations,
        is_public,
        ty,
        value,
        span: Span::new(const_span.start, end),
    })
}

fn parse_import(p: &mut Cursor) -> Result<ImportDecl, ParseError> {
    let import_span = p.expect(&Token::Import, "`import`")?;
    let path = parse_dotted_path(p, import_span, /* allow_scope */ true)?;

    let kind = if matches!(p.peek(), Token::LBrace) {
        // `import path { Name1, Name2 }`
        p.advance();
        let names = p.parse_comma_separated(&Token::RBrace, true, |p| {
            Ok(p.expect_ident("imported name")?.0)
        })?;
        p.expect(&Token::RBrace, "`}`")?;
        ImportKind::Named(names)
    } else if matches!(p.peek(), Token::As) {
        // `import path as alias`
        p.advance();
        let (alias, _) = p.expect_ident("alias identifier after `as`")?;
        ImportKind::Aliased(alias)
    } else {
        ImportKind::Namespace
    };

    let end_span = p.peek_span();
    if matches!(p.peek(), Token::Newline) {
        p.advance();
    } else if !p.is_at_end() {
        return Err(ParseError::Expected {
            expected: "newline after import",
            found: format!("{:?}", p.peek()),
            span: p.peek_span(),
        });
    }

    Ok(ImportDecl {
        path,
        kind,
        span: Span::new(import_span.start, end_span.end),
    })
}

fn parse_fn(
    p: &mut Cursor,
    is_async: bool,
    annotations: Vec<Annotation>,
    is_public: bool,
) -> Result<FnDecl, ParseError> {
    let fn_span = p.expect(&Token::Fn, "`fn`")?;
    let (name, _) = p.expect_ident("function name")?;
    let sig = parse_callable_signature(p)?;
    Ok(FnDecl {
        name: Arc::from(name.as_ref()),
        annotations,
        is_public,
        is_async,
        generics: sig.generics,
        params: sig.params,
        return_ty: sig.return_ty,
        body: sig.body,
        span: Span::new(fn_span.start, sig.end),
    })
}

/// Shared signature parse for `fn` and `component` (D4 + D19 — grammatically
/// parallel). Consumes optional generics, the `(params)` list, optional
/// `-> ReturnType`, and the block body. Caller supplies the leading keyword
/// span and wraps in the appropriate decl type.
struct CallableSignature {
    generics: Vec<GenericParam>,
    params: Vec<Param>,
    return_ty: Option<glyph_ast::TypeExpr>,
    body: glyph_ast::Block,
    end: u32,
}

fn parse_callable_signature(p: &mut Cursor) -> Result<CallableSignature, ParseError> {
    let generics = if matches!(p.peek(), Token::LAngle) {
        parse_generic_params(p)?
    } else {
        Vec::new()
    };
    p.expect(&Token::LParen, "`(`")?;
    let params = p.parse_comma_separated(&Token::RParen, true, parse_param)?;
    p.expect(&Token::RParen, "`)`")?;
    let return_ty = if matches!(p.peek(), Token::Arrow) {
        p.advance();
        Some(types::parse_type(p)?)
    } else {
        None
    };
    let body = stmt::parse_block(p)?;
    let end = body.span.end;
    Ok(CallableSignature {
        generics,
        params,
        return_ty,
        body,
        end,
    })
}

fn parse_generic_params(p: &mut Cursor) -> Result<Vec<GenericParam>, ParseError> {
    p.expect(&Token::LAngle, "`<` (generic parameters)")?;
    let params = p.parse_comma_separated(&Token::RAngle, false, |p| {
        let (name, span) = p.expect_ident("generic parameter name")?;
        // Optional bound: `<T: Bound>`. v1 supports a single bound (no `+`).
        let bounds = if matches!(p.peek(), Token::Colon) {
            p.advance();
            vec![types::parse_type(p)?]
        } else {
            Vec::new()
        };
        Ok(GenericParam { name, bounds, span })
    })?;
    p.expect(&Token::RAngle, "`>` (generic parameters)")?;
    Ok(params)
}

fn parse_param(p: &mut Cursor) -> Result<Param, ParseError> {
    // D25: an optional leading `owned` marks the parameter as taking
    // ownership of its argument. The span starts at `owned` when present.
    let (start, owned) = if matches!(p.peek(), Token::Owned) {
        let owned_span = p.expect(&Token::Owned, "`owned`")?;
        (owned_span.start, true)
    } else {
        (p.peek_span().start, false)
    };
    let (name, _) = p.expect_ident("parameter name")?;
    p.expect(&Token::Colon, "`:` after parameter name")?;
    let ty = types::parse_type(p)?;
    let end = ty.span().end;
    Ok(Param {
        name,
        owned,
        ty,
        span: Span::new(start, end),
    })
}


#[cfg(test)]
mod annotation_tests {
    /// An annotation continues onto the next line when that line starts with an
    /// operator, so a long `@example` does not have to sit on one line or be
    /// given a helper function purely to fit.
    #[test]
    fn an_annotation_continues_across_a_leading_operator() {
        let src = "module x\n@example f(1)\n  == 1\nfn f(n: int) -> int { return n }\n";
        let m = crate::parse(src).expect("parse");
        let ann = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => &f.annotations[0],
            other => panic!("expected a fn, got {other:?}"),
        };
        assert_eq!(ann.raw_args, "f(1) == 1");
    }

    /// A next line that begins a declaration ends the annotation.
    #[test]
    fn an_annotation_ends_at_a_line_that_is_not_a_continuation() {
        let src = "module x\n@example f(1) == 1\nfn f(n: int) -> int { return n }\n";
        let m = crate::parse(src).expect("parse");
        let ann = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => &f.annotations[0],
            other => panic!("expected a fn, got {other:?}"),
        };
        assert_eq!(ann.raw_args, "f(1) == 1");
        assert_eq!(m.items.len(), 1);
    }
}
