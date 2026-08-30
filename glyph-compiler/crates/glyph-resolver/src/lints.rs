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
//! - **Unused import** (E0106): an imported name never referenced. An
//!   `@example` argument is real Glyph source that the build parses,
//!   splices, compiles, and runs (D23) — but it lives in an annotation's
//!   `raw_args`, outside every function body, so the ordinary resolution
//!   walk above never reaches it and an import used only there would
//!   otherwise be misreported as dead (G106). [`example_identifiers`]
//!   parses each `@example`'s expression on its own and folds the names it
//!   references in before the unused-import check runs.
//! - **Unused binding** (E0107): a `let` whose name is never read. Names led by
//!   `_` are exempt (the conventional "intentionally unused" marker).
//! - **Unreachable code** (E0108): the first statement after a `return`,
//!   `break`, or `continue` in the same block.
//!
//! A fourth, [`no_export_surface_lint`], lives here too but is not part of
//! `module_lints`: it needs the whole project's import graph (is this module
//! named in some *sibling* module's `import`?), which this crate's per-module
//! walk does not have. The CLI computes that one external fact and calls it
//! directly (E0112).

use std::collections::HashSet;

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, MatchArmBody, Module, ObjectField, Span, Stmt, Pattern, TemplatePart, TypeExpr,
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

    // Unused imports: every import symbol that no reference resolved to, and
    // that no `@example` on this module references either (G106).
    let example_names = example_identifiers(module);
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
        if is_import
            && !used_modules.contains(&id.0)
            && !example_names.contains(sym.name.as_ref())
        {
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

/// Every identifier referenced by any `@example` annotation in `module`, by
/// name. `@example expr == expr` (D23) is stored as an annotation's raw,
/// unparsed argument string; the CLI parses and resolves it later, against a
/// throwaway copy of the whole project (see `glyph-cli/src/examples.rs`), so
/// nothing here treats the name as bound to a particular symbol the way
/// `resolved.resolutions` does for real code. It only has to answer "does
/// this text mention that name", which is enough to keep an import an
/// example alone uses out of the unused-import check (G106): a malformed
/// `@example` that fails to parse contributes nothing and is left for the
/// CLI to report on its own.
fn example_identifiers(module: &Module) -> HashSet<Box<str>> {
    let mut names = HashSet::new();
    for item in &module.items {
        for ann in decl_annotations(item) {
            if ann.name.as_ref() != "example" {
                continue;
            }
            if let Ok(expr) = glyph_parser::parse_expression(&ann.raw_args) {
                collect_idents(&expr, &mut names);
            }
        }
    }
    names
}

fn decl_annotations(d: &Decl) -> &[glyph_ast::Annotation] {
    match d {
        Decl::Fn(x) => &x.annotations,
        Decl::Type(x) => &x.annotations,
        Decl::Const(x) => &x.annotations,
        Decl::Component(x) => &x.annotations,
        Decl::Interface(x) => &x.annotations,
        Decl::Import(_) => &[],
    }
}

/// Collect every plain identifier reachable from an expression. Mirrors
/// [`LintWalk::expr`]'s shapes (same reasoning: missing a shape here only
/// misses a name, it never invents a false "used"), plus the `Ident` leaf
/// itself and the blocks a `match` arm or lambda body can carry, since an
/// `@example` expression is not confined to the block-free grammar `let`/
/// `return` walks assume. Pattern heads (a `match` arm's own constructor
/// name, e.g. `Some` in `Some(x) => ...`) are not collected; an `@example`
/// that names an import only through a match pattern is not covered by this
/// pass and keeps drawing E0106.
fn collect_idents(e: &Expr, out: &mut HashSet<Box<str>>) {
    match e {
        Expr::Ident { name, .. } => {
            out.insert(name.as_ref().into());
        }
        Expr::Binary { left, right, .. } => {
            collect_idents(left, out);
            collect_idents(right, out);
        }
        Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => {
            collect_idents(operand, out)
        }
        Expr::Call { callee, args, type_args, .. } => {
            collect_idents(callee, out);
            for a in args {
                collect_idents(a, out);
            }
            // `json.parse<TodoFile>(text)` references `TodoFile`. Letting the
            // `..` swallow `type_args` meant a name used only as a type
            // argument inside an `@example` still counted as unused, which is
            // the same false report this lint was just taught not to make
            // about ordinary argument positions.
            for t in type_args {
                collect_type_idents(t, out);
            }
        }
        Expr::New { callee, args, .. } => {
            collect_idents(callee, out);
            for a in args {
                collect_idents(a, out);
            }
        }
        Expr::Member { object, .. } => collect_idents(object, out),
        Expr::Index { object, index, .. } => {
            collect_idents(object, out);
            collect_idents(index, out);
        }
        Expr::Await { expr, .. } => collect_idents(expr, out),
        Expr::Array { elements, .. } => {
            for el in elements {
                match el {
                    ArrayElem::Expr(x) | ArrayElem::Spread(x) => collect_idents(x, out),
                }
            }
        }
        Expr::Object { fields, .. } => {
            for f in fields {
                match f {
                    ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => {
                        collect_idents(value, out)
                    }
                }
            }
        }
        Expr::TemplateString { parts, .. } => {
            for p in parts {
                if let TemplatePart::Expr { value, .. } = p {
                    collect_idents(value, out);
                }
            }
        }
        Expr::Match { scrutinee, arms, .. } => {
            collect_idents(scrutinee, out);
            for arm in arms {
                // An arm's pattern names a variant, and a variant reached
                // through a named import is a use of that import. Walking only
                // the arm bodies reported `Ok` and `Err` unused on a `match`
                // that is entirely built out of them.
                collect_pattern_idents(&arm.pattern, out);
                match &arm.body {
                    MatchArmBody::Expr(x) => collect_idents(x, out),
                    MatchArmBody::Block(b) => collect_block_idents(b, out),
                }
            }
        }
        Expr::Lambda { body, .. } => collect_block_idents(body, out),
        _ => {}
    }
}

/// The block-shaped counterpart to [`collect_idents`], reached only from a
/// `match` arm's block body or a lambda body inside an `@example` expression.
/// The bare names a type expression mentions. A type argument in a call is a
/// reference like any other, and a name used only there was being reported
/// unused.
fn collect_type_idents(t: &TypeExpr, out: &mut HashSet<Box<str>>) {
    match t {
        TypeExpr::Path { segments, .. } => {
            if let Some(first) = segments.first() {
                out.insert(first.as_ref().into());
            }
        }
        TypeExpr::Generic { base, args, .. } => {
            collect_type_idents(base, out);
            for a in args {
                collect_type_idents(a, out);
            }
        }
        _ => {}
    }
}

/// The names a match-arm pattern references. Only the head of a constructor
/// path counts: `Ok(x)` uses `Ok` and binds `x`, and a binding is not a use of
/// anything.
fn collect_pattern_idents(p: &Pattern, out: &mut HashSet<Box<str>>) {
    match p {
        Pattern::Constructor { path, args, .. } => {
            if let Some(head) = path.first() {
                out.insert(head.as_ref().into());
            }
            for a in args {
                collect_pattern_idents(a, out);
            }
        }
        Pattern::Object { fields, .. } => {
            for f in fields {
                if let Some(sub) = &f.pattern {
                    collect_pattern_idents(sub, out);
                }
            }
        }
        Pattern::Array { elements, .. } => {
            for e in elements {
                collect_pattern_idents(e, out);
            }
        }
        _ => {}
    }
}

fn collect_block_idents(b: &Block, out: &mut HashSet<Box<str>>) {
    for s in &b.stmts {
        match s {
            Stmt::Let(l) => collect_idents(&l.value, out),
            Stmt::Mut(m) => match &m.kind {
                glyph_ast::MutKind::Assign { target, value } => {
                    collect_idents(target, out);
                    collect_idents(value, out);
                }
                glyph_ast::MutKind::MethodCall { call } => collect_idents(call, out),
            },
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    collect_idents(v, out);
                }
            }
            Stmt::For(f) => {
                collect_idents(&f.iter, out);
                collect_block_idents(&f.body, out);
            }
            Stmt::Loop(l) => collect_block_idents(&l.body, out),
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Defer(d) => collect_idents(&d.expr, out),
            Stmt::Expr(e) => collect_idents(e, out),
        }
    }
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
            Stmt::Defer(d) => self.expr(&d.expr),
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
            Expr::Call { callee, args, .. } | Expr::New { callee, args, .. } => {
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

/// A module with no `pub` declaration, no `main` (`Decl::is_public` already
/// treats `main` as exported), and no `import` anywhere in the project naming
/// it, is unreachable from anywhere Glyph can see (G124). `imported_elsewhere`
/// is supplied by the caller rather than looked up here: whether *this*
/// module is named in some *other* module's `import` is project-wide
/// information, and this crate's lints otherwise see one module at a time.
/// Kept as a pure predicate (module in, one external fact in, an optional
/// diagnostic out) so it is unit-testable with no project scaffolding.
/// Does this declaration carry an `@example`? Such a declaration is executed by
/// the build, so it is a reachable entry point even without `pub`.
fn has_example_annotation(d: &Decl) -> bool {
    let anns = match d {
        Decl::Fn(x) => &x.annotations,
        Decl::Type(x) => &x.annotations,
        Decl::Const(x) => &x.annotations,
        Decl::Component(x) => &x.annotations,
        Decl::Interface(x) => &x.annotations,
        Decl::Import(_) => return false,
    };
    anns.iter().any(|a| a.name.as_ref() == "example" || a.name.as_ref() == "doc")
}

pub fn no_export_surface_lint(module: &Module, imported_elsewhere: bool) -> Option<ResolveError> {
    if imported_elsewhere || module.items.iter().any(Decl::is_public) {
        return None;
    }
    // A declaration carrying an `@example` is reached: the build runs it and
    // fails on it (D23), so the module is not dead code. Missing this made the
    // new lint report "nothing here is reachable" in the same output that
    // printed "1 example(s) passed", which is the exact false report G106 fixed
    // for the unused-import lint, reintroduced by its sibling in the same cut.
    if module.items.iter().any(has_example_annotation) {
        return None;
    }
    // Anchor on the `module <path>` header when the file has one; a file
    // with no header (legal — the header is optional) has no better anchor
    // than the whole module.
    let span = module
        .module_path
        .as_ref()
        .map(|mp| mp.span)
        .unwrap_or(module.span);
    Some(ResolveError::NoExportSurface { span })
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
        Stmt::Defer(d) => d.span,
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
    fn an_import_used_only_by_an_example_is_clean() {
        // `@example` is a real, compiler-run test (D23): the build parses,
        // splices, compiles, and executes it in the same invocation that
        // reports lints. An import a module's *only* reference to which is
        // inside an `@example` is genuinely used — flagging it E0106 leaves
        // no warning-free way to write the example the docs (`glyph llms`)
        // say to write, since deleting the "unused" import breaks the
        // example with E0103 unresolved name (G106).
        let errs = lints_of(
            "module m\nimport std/option { Option, Some }\n\
             @example identity(Some(1)) == Some(1)\n\
             pub fn identity(o: Option<number>) -> Option<number> { return o }\n",
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

    #[test]
    fn no_pub_no_main_no_importer_warns() {
        let m = glyph_parser::parse(
            "module lib\nfn helper() -> number {\n  return 1\n}\n",
        )
        .expect("parse failed");
        let e = no_export_surface_lint(&m, /* imported_elsewhere */ false);
        assert!(e.is_some(), "expected a warning on a module nothing reaches");
        assert_eq!(e.unwrap().code(), "E0112");
    }

    #[test]
    fn an_imported_module_with_no_pub_is_clean() {
        let m = glyph_parser::parse(
            "module lib\nfn helper() -> number {\n  return 1\n}\n",
        )
        .expect("parse failed");
        assert!(no_export_surface_lint(&m, /* imported_elsewhere */ true).is_none());
    }

    #[test]
    fn a_pub_declaration_is_clean_even_with_no_importer() {
        let m = glyph_parser::parse(
            "module lib\npub fn helper() -> number {\n  return 1\n}\n",
        )
        .expect("parse failed");
        assert!(no_export_surface_lint(&m, false).is_none());
    }

    #[test]
    fn fn_main_is_clean_even_with_no_pub_and_no_importer() {
        // `Decl::is_public` already treats `main` as exported (D33); this
        // lint must not double-diagnose the entrypoint module.
        let m = glyph_parser::parse("module app\nfn main() -> number {\n  return 0\n}\n")
            .expect("parse failed");
        assert!(no_export_surface_lint(&m, false).is_none());
    }
}
