//! Week-2 day-2 acceptance: every expression node in every example file gets
//! a `Ty` entry. Most entries are `Unknown` — that's fine. The point is that
//! the data structure is populated.
//!
//! Also: typed entries should appear for the things day-2 *can* compute —
//! literals (number/string/template/bool/void), lambda signatures, function
//! references whose declared signatures lower successfully.

use std::fs;
use std::path::PathBuf;

use glyph_ast::{
    ArrayElem, Decl, Expr, JsxAttr, JsxChild, JsxElement, MatchArmBody, Module, ObjectField, Span,
    Stmt, TemplatePart,
};
use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};
use glyph_typechecker::{assign_types, Primitive, Ty, TypeMap};

fn example_source(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "..",
        "examples",
        name,
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

fn run_pipeline(name: &str) -> (Module, TypeMap) {
    let source = example_source(name);
    let module = glyph_parser::parse(&source).expect("parse failed");
    let symbols = collect_module_symbols(&module).expect("collect failed");
    let prelude = build_prelude();
    let (resolved, errors) = resolve_module(&module, symbols, &prelude);
    assert!(errors.is_empty(), "{name}: resolve errors: {errors:?}");
    let (tm, _ty_errs) = assign_types(&module, &resolved, &prelude);
    (module, tm)
}

/// Collect every expression span in the module. Used by the acceptance test
/// to verify each one has a `TypeMap` entry.
fn all_expr_spans(module: &Module) -> Vec<Span> {
    let mut out = Vec::new();
    for decl in &module.items {
        match decl {
            Decl::Import(_) | Decl::Type(_) => {}
            Decl::Fn(f) => walk_block_spans(&f.body, &mut out),
            Decl::Component(c) => walk_block_spans(&c.body, &mut out),
            Decl::Const(c) => walk_expr_spans(&c.value, &mut out),
        }
    }
    out
}

fn walk_block_spans(b: &glyph_ast::Block, out: &mut Vec<Span>) {
    for s in &b.stmts {
        walk_stmt_spans(s, out);
    }
}

fn walk_stmt_spans(s: &Stmt, out: &mut Vec<Span>) {
    match s {
        Stmt::Let(l) => walk_expr_spans(&l.value, out),
        Stmt::Mut(m) => match &m.kind {
            glyph_ast::MutKind::Assign { value, .. } => walk_expr_spans(value, out),
            glyph_ast::MutKind::AssignIndex { index, value, .. } => {
                walk_expr_spans(index, out);
                walk_expr_spans(value, out);
            }
            glyph_ast::MutKind::AssignField { value, .. } => walk_expr_spans(value, out),
            glyph_ast::MutKind::MethodCall { receiver, call } => {
                walk_expr_spans(receiver, out);
                walk_expr_spans(call, out);
            }
        },
        Stmt::Return(r) => {
            if let Some(v) = &r.value {
                walk_expr_spans(v, out);
            }
        }
        Stmt::For(f) => {
            walk_expr_spans(&f.iter, out);
            walk_block_spans(&f.body, out);
        }
        Stmt::Loop(l) => walk_block_spans(&l.body, out),
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Expr(e) => walk_expr_spans(e, out),
    }
}

fn walk_expr_spans(e: &Expr, out: &mut Vec<Span>) {
    out.push(e.span());
    match e {
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Void { .. }
        | Expr::Ident { .. } => {}
        Expr::TemplateString { parts, .. } => {
            for p in parts {
                if let TemplatePart::Expr { value, .. } = p {
                    walk_expr_spans(value, out);
                }
            }
        }
        Expr::Binary { left, right, .. } => {
            walk_expr_spans(left, out);
            walk_expr_spans(right, out);
        }
        Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => {
            walk_expr_spans(operand, out);
        }
        Expr::Call { callee, args, .. } => {
            walk_expr_spans(callee, out);
            for a in args {
                walk_expr_spans(a, out);
            }
        }
        Expr::Member { object, .. } => walk_expr_spans(object, out),
        Expr::Index { object, index, .. } => {
            walk_expr_spans(object, out);
            walk_expr_spans(index, out);
        }
        Expr::Await { expr, .. } => walk_expr_spans(expr, out),
        Expr::Array { elements, .. } => {
            for el in elements {
                match el {
                    ArrayElem::Expr(e) | ArrayElem::Spread(e) => walk_expr_spans(e, out),
                }
            }
        }
        Expr::Object { fields, .. } => {
            for f in fields {
                match f {
                    ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => {
                        walk_expr_spans(value, out)
                    }
                }
            }
        }
        Expr::Match { scrutinee, arms, .. } => {
            walk_expr_spans(scrutinee, out);
            for arm in arms {
                match &arm.body {
                    MatchArmBody::Expr(e) => walk_expr_spans(e, out),
                    MatchArmBody::Block(b) => walk_block_spans(b, out),
                }
            }
        }
        Expr::Lambda { body, .. } => walk_block_spans(body, out),
        Expr::Jsx(j) => walk_jsx_spans(j, out),
    }
}

fn walk_jsx_spans(j: &JsxElement, out: &mut Vec<Span>) {
    for attr in &j.attrs {
        if let JsxAttr::Expr { value, .. } = attr {
            walk_expr_spans(value, out);
        }
    }
    for child in &j.children {
        match child {
            JsxChild::Element(e) => walk_jsx_spans(e, out),
            JsxChild::Expr(e) => walk_expr_spans(e, out),
            JsxChild::Text { .. } => {}
        }
    }
}

#[test]
fn every_expression_has_a_type_entry() {
    for name in [
        "01_validator.glyph",
        "02_async_errors.glyph",
        "03_react_component.glyph",
        "04_cli_tool.glyph",
    ] {
        let (module, tm) = run_pipeline(name);
        let spans: Vec<Span> = all_expr_spans(&module);
        let missing = spans.iter().filter(|s| !tm.has_entry(**s)).count();
        println!("{name}: {} expression spans, {missing} without an entry", spans.len());
        assert_eq!(missing, 0, "{name}: {missing} expressions without a Ty entry");
    }
}

#[test]
fn typed_entry_counts_per_example() {
    // Diagnostic: how many entries are concrete (non-Unknown) per example.
    // The number should grow as the typechecker matures.
    for name in [
        "01_validator.glyph",
        "02_async_errors.glyph",
        "03_react_component.glyph",
        "04_cli_tool.glyph",
    ] {
        let (module, tm) = run_pipeline(name);
        let spans: Vec<Span> = all_expr_spans(&module);
        let mut concrete = 0;
        let mut prim_string = 0;
        let mut prim_number = 0;
        let mut prim_bool = 0;
        let mut fns = 0;
        for s in &spans {
            let ty = tm.get(*s);
            if !ty.is_unknown() {
                concrete += 1;
            }
            match ty {
                Ty::Prim(Primitive::String) => prim_string += 1,
                Ty::Prim(Primitive::Number) => prim_number += 1,
                Ty::Prim(Primitive::Bool) => prim_bool += 1,
                Ty::Fn { .. } => fns += 1,
                _ => {}
            }
        }
        println!(
            "{name}: {} exprs, {concrete} concrete ({} string, {} number, {} bool, {} fn)",
            spans.len(),
            prim_string,
            prim_number,
            prim_bool,
            fns
        );
    }
}
