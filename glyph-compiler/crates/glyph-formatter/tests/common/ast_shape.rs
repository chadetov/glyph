//! What a parse says, with what `glyph fmt` is allowed to change taken out.
//!
//! The formatter's contract is that `parse(fmt(src))` is `parse(src)`: the
//! same program, laid out differently. Text idempotence cannot see a first
//! pass that changes the program, since a wrong output can itself be a fixed
//! point. G335 was one (`let a = 1` then `(load()?).run()` reads as
//! `1(load()?).run()`), and G338 was another (a union's `where` refinement
//! dropped, so an E0300 program checked clean). Comparing the two ASTs catches
//! both on the first pass, and an output that does not parse as well.
//!
//! [`shape`] renders a module for that comparison. It leaves out only what
//! formatting changes by design:
//!
//! - every span, since a layout change moves every offset;
//! - the order of a declaration's annotations, which D27 sorts by name (a
//!   stable sort, so repeated annotations keep their order), and the
//!   whitespace around their argument text, which the printer trims;
//! - how a JSX text child is written: a child is compared as the text it
//!   renders (`glyph_ast::jsx_text_value`), a string child `{"..."}` as its
//!   value, and a child that renders nothing is dropped. The printer writes an
//!   edge-spaced text as a string child and reflows indentation between tags,
//!   and both emit the same TypeScript (record 84).
//!
//! Anything else that differs is a formatter that changed what a file says.
//!
//! One copy, shared: `tests/format.rs` runs it over the corpus and the seeds,
//! and the `format_idempotent` fuzz target includes this file by path.

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, JsxAttr, JsxChild, JsxElement, MatchArmBody, Module, MutKind,
    ObjectField, Stmt, TemplatePart,
};

/// `m` as text for comparison with another parse of the same program. Two
/// modules have the same shape exactly when formatting may turn one into the
/// other.
pub fn shape(m: &Module) -> String {
    let mut m = m.clone();
    for d in &mut m.items {
        let anns = match d {
            Decl::Fn(x) => &mut x.annotations,
            Decl::Type(x) => &mut x.annotations,
            Decl::Const(x) => &mut x.annotations,
            Decl::Component(x) => &mut x.annotations,
            Decl::Interface(x) => &mut x.annotations,
            Decl::Import(_) => continue,
        };
        anns.sort_by(|a, b| a.name.cmp(&b.name));
        for a in anns.iter_mut() {
            a.raw_args = a.raw_args.trim().to_string();
        }
        match d {
            Decl::Fn(x) => block(&mut x.body),
            Decl::Component(x) => block(&mut x.body),
            Decl::Const(x) => expr(&mut x.value),
            Decl::Type(x) => {
                if let Some(r) = &mut x.refinement {
                    expr(r);
                }
            }
            Decl::Interface(_) | Decl::Import(_) => {}
        }
    }
    without_spans(&format!("{m:?}"))
}

/// `dbg` with every `Span { start: N, end: M }` written as `_`.
fn without_spans(dbg: &str) -> String {
    const SPAN: &str = "Span { start: ";
    let mut out = String::with_capacity(dbg.len());
    let mut rest = dbg;
    while let Some(i) = rest.find(SPAN) {
        out.push_str(&rest[..i]);
        out.push('_');
        let tail = &rest[i..];
        let close = tail.find('}').expect("a span's debug form closes");
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

fn block(b: &mut Block) {
    for s in &mut b.stmts {
        match s {
            Stmt::Let(l) => expr(&mut l.value),
            Stmt::Mut(m) => match &mut m.kind {
                MutKind::Assign { target, value } => {
                    expr(target);
                    expr(value);
                }
                MutKind::MethodCall { call } => expr(call),
            },
            Stmt::Return(r) => {
                if let Some(v) = &mut r.value {
                    expr(v);
                }
            }
            Stmt::For(f) => {
                expr(&mut f.iter);
                block(&mut f.body);
            }
            Stmt::Loop(l) => block(&mut l.body),
            Stmt::Expr(e) => expr(e),
            Stmt::Defer(d) => expr(&mut d.expr),
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }
}

fn expr(e: &mut Expr) {
    match e {
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Void { .. }
        | Expr::Ident { .. }
        | Expr::Extern { .. } => {}
        Expr::TemplateString { parts, .. } => {
            for p in parts {
                if let TemplatePart::Expr { value, .. } = p {
                    expr(value);
                }
            }
        }
        Expr::Binary { left, right, .. } => {
            expr(left);
            expr(right);
        }
        Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => expr(operand),
        Expr::Call { callee, args, .. } | Expr::New { callee, args, .. } => {
            expr(callee);
            args.iter_mut().for_each(expr);
        }
        Expr::Member { object, .. } => expr(object),
        Expr::Index { object, index, .. } => {
            expr(object);
            expr(index);
        }
        Expr::Await { expr: inner, .. } => expr(inner),
        Expr::Array { elements, .. } => {
            for el in elements {
                match el {
                    ArrayElem::Expr(v) | ArrayElem::Spread(v) => expr(v),
                }
            }
        }
        Expr::Object { fields, .. } => {
            for f in fields {
                match f {
                    ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => {
                        expr(value)
                    }
                }
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            expr(scrutinee);
            for arm in arms {
                match &mut arm.body {
                    MatchArmBody::Expr(v) => expr(v),
                    MatchArmBody::Block(b) => block(b),
                }
            }
        }
        Expr::Lambda { body, .. } => block(body),
        Expr::Jsx(j) => jsx(j),
    }
}

fn jsx(j: &mut JsxElement) {
    for a in &mut j.attrs {
        match a {
            JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => expr(value),
            JsxAttr::String { .. } | JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } => {}
        }
    }
    let children = std::mem::take(&mut j.children);
    for mut c in children {
        let rendered = match &mut c {
            JsxChild::Element(child) => {
                jsx(child);
                None
            }
            JsxChild::Text { content, .. } => Some(glyph_ast::jsx_text_value(content)),
            JsxChild::Expr(Expr::String { value, .. }) => Some(value.clone()),
            JsxChild::Expr(e) => {
                expr(e);
                None
            }
        };
        match rendered {
            Some(text) if text.is_empty() => {}
            Some(text) => j.children.push(JsxChild::Text {
                content: text,
                span: glyph_lexer::Span::new(0, 0),
            }),
            None => j.children.push(c),
        }
    }
}
