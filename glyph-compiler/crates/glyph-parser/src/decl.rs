//! Declaration parsing: module, import, fn, type, const.

use std::sync::Arc;

use glyph_ast::{
    Annotation, ComponentDecl, ConstDecl, Decl, FnDecl, GenericParam, ImportDecl, ImportKind,
    Module, ModulePath, Param, TypeDecl,
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

    match p.peek() {
        Token::Import => {
            // Imports do not carry annotations in v1; reject if any preceded.
            if !annotations.is_empty() {
                return Err(ParseError::Unexpected {
                    found: "@annotation on `import`".to_string(),
                    span: annotations[0].span,
                });
            }
            parse_import(p).map(Decl::Import)
        }
        Token::Fn => parse_fn(p, /* is_async */ false, annotations).map(Decl::Fn),
        Token::Async => {
            p.advance();
            if !matches!(p.peek(), Token::Fn) {
                return Err(ParseError::Expected {
                    expected: "`fn` after `async`",
                    found: format!("{:?}", p.peek()),
                    span: p.peek_span(),
                });
            }
            parse_fn(p, true, annotations).map(Decl::Fn)
        }
        Token::Type => parse_type_decl(p, annotations).map(Decl::Type),
        // D25: `resource type X = ...` marks a resource handle type. The
        // `resource` keyword only precedes `type`; anything else is an error.
        Token::Resource => parse_type_decl(p, annotations).map(Decl::Type),
        Token::Const => parse_const_decl(p, annotations).map(Decl::Const),
        Token::Component => parse_component(p, annotations).map(Decl::Component),
        // record (top-level `record X { ... }`) — deferred to v1.1 cleanup;
        // for now records are written as `type X = { ... }` per D8 inline.
        other => Err(ParseError::Unexpected {
            found: format!("{other:?}"),
            span: p.peek_span(),
        }),
    }
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
        let args_start = p.peek_span().start;
        let mut args_end = name_span.end;
        while !matches!(p.peek(), Token::Newline | Token::Eof) {
            args_end = p.peek_span().end;
            p.advance();
        }
        let raw_args = if args_end > args_start {
            p.slice(args_start, args_end).trim().to_string()
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
) -> Result<ComponentDecl, ParseError> {
    let kw_span = p.expect(&Token::Component, "`component`")?;
    let (name, _) = p.expect_ident("component name")?;
    let sig = parse_callable_signature(p)?;
    Ok(ComponentDecl {
        name,
        annotations,
        generics: sig.generics,
        params: sig.params,
        return_ty: sig.return_ty,
        body: sig.body,
        span: Span::new(kw_span.start, sig.end),
    })
}

fn parse_type_decl(p: &mut Cursor, annotations: Vec<Annotation>) -> Result<TypeDecl, ParseError> {
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
    let end = body.span().end;
    if matches!(p.peek(), Token::Newline) {
        p.advance();
    }
    Ok(TypeDecl {
        name,
        annotations,
        generics,
        is_resource,
        body,
        span: Span::new(start, end),
    })
}

fn parse_const_decl(p: &mut Cursor, annotations: Vec<Annotation>) -> Result<ConstDecl, ParseError> {
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
) -> Result<FnDecl, ParseError> {
    let fn_span = p.expect(&Token::Fn, "`fn`")?;
    let (name, _) = p.expect_ident("function name")?;
    let sig = parse_callable_signature(p)?;
    Ok(FnDecl {
        name: Arc::from(name.as_ref()),
        annotations,
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
        Ok(GenericParam {
            name,
            bounds: Vec::new(),
            span,
        })
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

