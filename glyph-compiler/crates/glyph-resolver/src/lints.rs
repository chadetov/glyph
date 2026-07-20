//! Warning-tier lints, computed after a module resolves cleanly.
//!
//! These are advisory (severity `Warning`): they surface but never fail the
//! build. They are computed *outside* `resolve_module` so the resolver's hard
//! `Vec<ResolveError>` (and every test asserting it is empty) is unaffected;
//! the CLI runs `module_lints` only on a module that produced no errors, since
//! an unresolved name would leave gaps in the resolution map and could turn a
//! genuinely-used binding into a false "unused" report.
//!
//! Everything here reads the authoritative resolution map for *usage*, so the
//! walk that finds candidate bindings only needs to be complete enough to reach
//! them: a missed block yields at worst a missed lint (a false negative), never
//! a wrong warning. Usage is never guessed.
//!
//! Three lints:
//! - **Unused import** (E0106): an imported name never referenced.
//! - **Unused binding** (E0107): a `let` whose name is never read. Names led by
//!   `_` are exempt (the conventional "intentionally unused" marker).
//! - **Unreachable code** (E0108): the first statement after a `return`,
//!   `break`, or `continue` in the same block.

use std::collections::HashSet;

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, MatchArmBody, Module, ObjectField, Span, Stmt, TemplatePart,
};

use crate::error::ResolveError;
use crate::resolve::{ResolvedModule, ResolvedRef};
use crate::symbol::{SymbolId, SymbolKind};

/// Compute the warning-tier lints for a cleanly-resolved module.
pub fn module_lints(module: &Module, resolved: &ResolvedModule) -> Vec<ResolveError> {
    let mut used_modules: HashSet<u32> = HashSet::new();
    let mut used_locals: HashSet<u32> = HashSet::new();
    for (_, r) in resolved.resolutions.iter() {
        match r {
            ResolvedRef::Module(id) => {
                used_modules.insert(id.0);
            }
            ResolvedRef::Local(start) => {
                used_locals.insert(start);
            }
            ResolvedRef::Prelude(_) => {}
        }
    }

    let mut out = Vec::new();

    // Unused imports: every import symbol that no reference resolved to.
    let table = &resolved.symbols.table;
    for i in 0..table.len() {
        let id = SymbolId(i as u32);
        let Some(sym) = table.get(id) else { continue };
        let is_import = matches!(
            sym.kind,
            SymbolKind::ImportNamespace { .. }
                | SymbolKind::ImportAlias { .. }
                | SymbolKind::ImportNamed { .. }
        );
        if is_import && !used_modules.contains(&id.0) {
            out.push(ResolveError::UnusedImport {
                name: sym.name.to_string(),
                span: sym.span,
            });
        }
    }

    // Unused bindings and unreachable code: walk the executable blocks.
    let mut walk = LintWalk {
        used_locals: &used_locals,
        out,
    };
    for item in &module.items {
        match item {
            Decl::Fn(f) => walk.block(&f.body),
            Decl::Component(c) => walk.block(&c.body),
            _ => {}
        }
    }
    walk.out
}

struct LintWalk<'a> {
    used_locals: &'a HashSet<u32>,
    out: Vec<ResolveError>,
}

impl LintWalk<'_> {
    fn block(&mut self, b: &Block) {
        // Unreachable: the first statement after the first unconditional
        // terminator in this block. Reported once per block.
        if let Some(ti) = b.stmts.iter().position(is_terminal) {
            if let Some(dead) = b.stmts.get(ti + 1) {
                self.out.push(ResolveError::UnreachableCode {
                    span: stmt_span(dead),
                });
            }
        }
        for s in &b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                let name = l.name.as_ref();
                if !name.starts_with('_') && !self.used_locals.contains(&l.span.start) {
                    self.out.push(ResolveError::UnusedBinding {
                        name: name.to_string(),
                        span: l.span,
                    });
                }
                self.expr(&l.value);
            }
            Stmt::Mut(m) => match &m.kind {
                glyph_ast::MutKind::Assign { target, value } => {
                    self.expr(target);
                    self.expr(value);
                }
                glyph_ast::MutKind::MethodCall { call } => self.expr(call),
            },
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.expr(v);
                }
            }
            Stmt::For(f) => {
                self.expr(&f.iter);
                self.block(&f.body);
            }
            Stmt::Loop(l) => self.block(&l.body),
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => self.expr(e),
        }
    }

    /// Descend into the block-bearing corners of an expression (lambda bodies,
    /// match-arm blocks) so nested `let`s and dead code are still seen. Leaf
    /// expressions and JSX internals are not walked; missing one only forgoes a
    /// lint, never invents one.
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Binary { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => self.expr(operand),
            Expr::Call { callee, args, .. } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::Member { object, .. } => self.expr(object),
            Expr::Index { object, index, .. } => {
                self.expr(object);
                self.expr(index);
            }
            Expr::Await { expr, .. } => self.expr(expr),
            Expr::Array { elements, .. } => {
                for el in elements {
                    match el {
                        ArrayElem::Expr(x) | ArrayElem::Spread(x) => self.expr(x),
                    }
                }
            }
            Expr::Object { fields, .. } => {
                for f in fields {
                    match f {
                        ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => {
                            self.expr(value)
                        }
                    }
                }
            }
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.expr(value);
                    }
                }
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.expr(scrutinee);
                for arm in arms {
                    match &arm.body {
                        MatchArmBody::Expr(x) => self.expr(x),
                        MatchArmBody::Block(b) => self.block(b),
                    }
                }
            }
            Expr::Lambda { body, .. } => self.block(body),
            _ => {}
        }
    }
}

/// A statement that unconditionally leaves the enclosing block, so anything
/// after it in the same block cannot run.
fn is_terminal(s: &Stmt) -> bool {
    matches!(s, Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_))
}

fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Let(l) => l.span,
        Stmt::Mut(m) => m.span,
        Stmt::Return(r) => r.span,
        Stmt::For(f) => f.span,
        Stmt::Loop(l) => l.span,
        Stmt::Break(b) => b.span,
        Stmt::Continue(c) => c.span,
        Stmt::Expr(e) => e.span(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::collect_module_symbols;
    use crate::prelude::build_prelude;
    use crate::resolve::resolve_module;

    fn lints_of(src: &str) -> Vec<ResolveError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).expect("collect failed");
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "resolve errors: {errs:?}");
        module_lints(&m, &resolved)
    }

    fn has(errs: &[ResolveError], code: &str) -> bool {
        errs.iter().any(|e| e.code() == code)
    }

    #[test]
    fn flags_an_unused_import() {
        let errs = lints_of("module m\nimport std/array\nfn f() -> number { return 1 }\n");
        assert!(has(&errs, "E0106"), "{errs:?}");
    }

    #[test]
    fn a_used_import_is_clean() {
        let errs = lints_of(
            "module m\nimport std/array\nfn f(xs: Array<number>) -> number { return array.len(xs) }\n",
        );
        assert!(!has(&errs, "E0106"), "{errs:?}");
    }

    #[test]
    fn flags_an_unused_let_but_not_an_underscore_one() {
        let errs = lints_of(
            "module m\nfn f() -> number {\n  let unused = 1\n  let _ignored = 2\n  return 3\n}\n",
        );
        let unused: Vec<_> = errs.iter().filter(|e| e.code() == "E0107").collect();
        assert_eq!(unused.len(), 1, "only `unused`, not `_ignored`: {errs:?}");
    }

    #[test]
    fn flags_code_after_return() {
        let errs = lints_of("module m\nfn f() -> number {\n  return 1\n  let x = 2\n}\n");
        assert!(has(&errs, "E0108"), "{errs:?}");
    }

    #[test]
    fn a_used_binding_is_clean() {
        let errs = lints_of("module m\nfn f() -> number {\n  let x = 1\n  return x\n}\n");
        assert!(!has(&errs, "E0107"), "{errs:?}");
    }
}
