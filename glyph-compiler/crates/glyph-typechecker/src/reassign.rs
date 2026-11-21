//! Which bindings a `mut` reassigns, anywhere in the module.
//!
//! One predicate, "a binding no `mut` reassigns", for every rule that needs a
//! name to keep the value it was bound to. `is`-arm narrowing (record 130, F3)
//! is the first: a name narrowed inside an arm must not be rebound while the
//! arm runs, and a lambda defined earlier in the function can rebind it from a
//! call inside the arm (record 134, B1), so looking inside the arm alone is not
//! enough. Constness rules ("a `let` that no `mut` reassigns") are meant to
//! read the same answer, so the two cannot drift apart.
//!
//! The walk is by resolution, not by spelling. A `mut v = ..` counts against
//! the binding its `v` resolves to, so a shadowing `let v` is a different
//! binding, and a reassignment from any nested lambda is seen because a local
//! can only be reassigned from inside its own scope, which this walk covers
//! in full.

use std::collections::HashSet;

use glyph_ast::{visit, Block, Decl, Expr, JsxAttr, JsxChild, JsxElement, Module, MutKind, Stmt};
use glyph_resolver::{ResolvedModule, ResolvedRef, SymbolKind};

/// The local bindings (keyed by definition-site span start, as
/// `ResolvedRef::Local` keys them) that some `mut name = ..` in the module
/// reassigns.
#[derive(Debug, Default, Clone)]
pub struct Reassigned {
    locals: HashSet<u32>,
}

impl Reassigned {
    /// Walk every body in `module`, nested lambdas included.
    pub fn of(module: &Module, resolved: &ResolvedModule) -> Self {
        let mut w = Walk {
            resolved,
            locals: HashSet::new(),
        };
        for decl in &module.items {
            match decl {
                Decl::Fn(f) => w.block(&f.body),
                Decl::Component(c) => w.block(&c.body),
                Decl::Const(c) => w.expr(&c.value),
                Decl::Type(_) | Decl::Interface(_) | Decl::Import(_) => {}
            }
        }
        Reassigned { locals: w.locals }
    }

    /// True when the name `r` resolves to is never reassigned: a local or
    /// parameter that no `mut` rebinds, or a module `const` (which cannot be,
    /// E0212). False for anything else, whose value this module cannot vouch
    /// for.
    pub fn never_reassigned(&self, r: ResolvedRef, resolved: &ResolvedModule) -> bool {
        match r {
            ResolvedRef::Local(def) => !self.locals.contains(&def),
            ResolvedRef::Module(id) => resolved
                .symbols
                .table
                .get(id)
                .is_some_and(|s| matches!(s.kind, SymbolKind::Const { .. })),
            ResolvedRef::Prelude(_) => false,
        }
    }
}

struct Walk<'a> {
    resolved: &'a ResolvedModule,
    locals: HashSet<u32>,
}

impl Walk<'_> {
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            if let Stmt::Mut(m) = s {
                if let MutKind::Assign {
                    target: Expr::Ident { span, .. },
                    ..
                } = &m.kind
                {
                    if let Some(ResolvedRef::Local(def)) = self.resolved.resolutions.get(*span) {
                        self.locals.insert(def);
                    }
                }
            }
            visit::stmt_exprs(s, &mut |e| self.expr(e));
            visit::stmt_blocks(s, &mut |b| self.block(b));
        }
    }

    fn expr(&mut self, e: &Expr) {
        // The shared visitor treats JSX as a leaf, and a JSX attribute is
        // where a component's event-handler lambdas live, so it is walked
        // here: a `mut` in an `onClick={fn() { .. }}` reassigns as surely as
        // one anywhere else.
        if let Expr::Jsx(j) = e {
            self.jsx(j);
            return;
        }
        visit::child_exprs(e, &mut |x| self.expr(x));
        visit::child_blocks(e, &mut |b| self.block(b));
    }

    fn jsx(&mut self, j: &JsxElement) {
        for a in &j.attrs {
            match a {
                JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => self.expr(value),
                JsxAttr::String { .. } | JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } => {}
            }
        }
        for c in &j.children {
            match c {
                JsxChild::Element(el) => self.jsx(el),
                JsxChild::Expr(e) => self.expr(e),
                JsxChild::Text { .. } => {}
            }
        }
    }
}
