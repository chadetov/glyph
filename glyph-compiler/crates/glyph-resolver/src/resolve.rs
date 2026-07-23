//! Intra-module name resolution.
//!
//! Walks every expression and pattern in a `Module`, mapping each identifier
//! reference to either a local binding, a module-level symbol, an imported
//! symbol, or a prelude built-in. Produces a `ResolvedModule` that owns:
//! - the original `ModuleSymbols` (top-level symbol table)
//! - a `ResolutionMap` keyed by ident span → `ResolvedRef`
//!
//! Local scopes (function parameters, `let`s inside blocks, match-arm
//! bindings, for-loop bindings, lambda params) are managed via a stack of
//! `HashMap<Ident, LocalId>` frames during the walk. Locals are not interned
//! into the symbol table — they're transient and don't need stable ids
//! beyond the walk itself.
//!
//! Cross-module resolution (imports → foreign module exports) is deferred to
//! week 2 day 3+. In this slice, an `ImportNamed` symbol resolves "in the
//! local module" (the importing module knows the name exists); the
//! typechecker doesn't yet check the target export.

use std::collections::HashMap;

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, GenericParam, Ident, JsxAttr, JsxChild, JsxElement, MatchArmBody,
    Module, ObjectField, Param, Pattern, Span, Stmt, TemplatePart, TypeExpr,
};

use crate::collect::ModuleSymbols;
use crate::error::ResolveError;
use crate::prelude::Prelude;
use crate::symbol::SymbolId;

/// What a name resolved to. Stored once per ident reference (keyed by the
/// reference span, not the definition span).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedRef {
    /// Module-level symbol — top-level decl, or import.
    Module(SymbolId),
    /// Prelude built-in.
    Prelude(SymbolId),
    /// A local binding introduced by the function body: param, `let`, `mut`,
    /// match binding, `for` binding, lambda param. The `u32` is the binding's
    /// definition-site span start — stable for the lifetime of the AST.
    Local(u32),
}

/// The output of `resolve_module`: the symbol table + a span-indexed
/// resolution map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModule {
    pub symbols: ModuleSymbols,
    pub resolutions: ResolutionMap,
}

impl ResolvedModule {
    /// Return a new `ResolvedModule` with the same symbols but the
    /// `resolutions` map restricted to entries whose span passes `keep`.
    /// Used by `glyph-db` to produce a per-declaration resolution slice —
    /// when a file edit doesn't change a particular decl's spans, the
    /// slice is content-equal across the edit and salsa can backdate
    /// downstream queries that depend on it.
    ///
    /// **Limitation**: the cloned `symbols` table contains a `Symbol` for
    /// every top-level decl, each with a `Symbol.span` that covers the
    /// entire declaration (including the body for `fn`/`component`). So
    /// any edit that *shifts byte positions* — a non-equal-length change
    /// to an earlier decl's body, a newline insertion, etc. — changes
    /// every later symbol's span, which makes the cloned `symbols`
    /// compare unequal even when the slice's `resolutions` are stable.
    /// Callers relying on the day-8 per-decl invalidation win should
    /// either use equal-length edits or eventually move to a
    /// span-insensitive `Symbol` equality.
    pub fn sliced(&self, mut keep: impl FnMut(Span) -> bool) -> Self {
        let mut out = ResolutionMap::new();
        for (span, r) in self.resolutions.iter() {
            if keep(span) {
                out.insert(span, r);
            }
        }
        ResolvedModule {
            symbols: self.symbols.clone(),
            resolutions: out,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolutionMap {
    by_span: HashMap<(u32, u32), ResolvedRef>,
}

impl ResolutionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, span: Span, r: ResolvedRef) {
        self.by_span.insert((span.start, span.end), r);
    }

    pub fn get(&self, span: Span) -> Option<ResolvedRef> {
        self.by_span.get(&(span.start, span.end)).copied()
    }

    pub fn len(&self) -> usize {
        self.by_span.len()
    }

    /// Iterate over every recorded resolution. Used by tests and by the
    /// typechecker, which needs to know "what each name in scope actually
    /// pointed at."
    pub fn iter(&self) -> impl Iterator<Item = (Span, ResolvedRef)> + '_ {
        self.by_span
            .iter()
            .map(|((s, e), v)| (Span::new(*s, *e), *v))
    }
}

/// Resolve every identifier reference in `module`. Local bindings are scoped
/// per function body; top-level decls and imports use `symbols`; everything
/// else falls through to `prelude`.
///
/// Returns the resolved module plus any errors encountered. Errors do not
/// abort the walk — the resolver records `Unresolved` references and keeps
/// going, so a single pass reports every unresolved name at once.
pub fn resolve_module(
    module: &Module,
    symbols: ModuleSymbols,
    prelude: &Prelude,
) -> (ResolvedModule, Vec<ResolveError>) {
    let mut walker = Resolver {
        symbols: &symbols,
        prelude,
        scopes: Vec::new(),
        resolutions: ResolutionMap::new(),
        errors: Vec::new(),
    };

    for item in &module.items {
        walker.walk_decl(item);
    }

    let Resolver {
        resolutions, errors, ..
    } = walker;
    (
        ResolvedModule {
            symbols,
            resolutions,
        },
        errors,
    )
}

// ============================================================================
// JSX classifier
// ============================================================================

/// What kind of element a JSX tag is. Drives directive-specific name
/// resolution: `<for>` and `<case>` introduce bindings; intrinsics like `<div>`
/// don't resolve as references; component-shaped names (uppercase) do.
///
/// The directive name list is duplicated from the parser's `jsx.rs`. If a new
/// directive lands, both lists need updating — same drift risk the `KEYWORDS`
/// table fixed for the lexer. Worth promoting to a shared `glyph-ast` helper
/// if the count grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsxKind {
    Intrinsic,
    Component,
    Fragment,
    If,
    Else,
    For,
    Match,
    Case,
}

impl JsxKind {
    fn classify(name: &Ident) -> Self {
        match name.as_ref() {
            "" => JsxKind::Fragment,
            "if" => JsxKind::If,
            "else" => JsxKind::Else,
            "for" => JsxKind::For,
            "match" => JsxKind::Match,
            "case" => JsxKind::Case,
            other => {
                if other
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_lowercase())
                    .unwrap_or(false)
                {
                    JsxKind::Intrinsic
                } else {
                    JsxKind::Component
                }
            }
        }
    }
}

/// Whether a bare ident used in constructor position (a `Pattern::Ident`
/// arm head) is constructor-shaped: PascalCase names (`Idle`, `Ok`, `None`)
/// denote a union variant and must resolve as a reference; lowercase or
/// underscore-led names (`x`, `_rest`) are fresh bindings. Same capitalization
/// rule the JSX classifier uses to tell a `<Component>` from an intrinsic tag.
fn is_constructor_shaped(name: &Ident) -> bool {
    name.as_ref()
        .chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false)
}

fn find_expr_attr<'a>(attrs: &'a [JsxAttr], name: &str) -> Option<&'a Expr> {
    attrs.iter().find_map(|a| match a {
        JsxAttr::Expr { name: n, value, .. } if n.as_ref() == name => Some(value),
        _ => None,
    })
}

fn first_positional(attrs: &[JsxAttr]) -> Option<(&Ident, Span)> {
    attrs.iter().find_map(|a| match a {
        JsxAttr::Positional { name, span } => Some((name, *span)),
        _ => None,
    })
}

// ============================================================================
// Walker
// ============================================================================

struct Resolver<'a> {
    symbols: &'a ModuleSymbols,
    prelude: &'a Prelude,
    /// Stack of local scopes. Each scope maps name → defining-site span start.
    scopes: Vec<HashMap<Ident, u32>>,
    resolutions: ResolutionMap,
    errors: Vec<ResolveError>,
}

impl Resolver<'_> {
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind_local(&mut self, name: Ident, def_span: Span) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name, def_span.start);
        }
    }

    fn lookup_local(&self, name: &str) -> Option<u32> {
        for scope in self.scopes.iter().rev() {
            if let Some(start) = scope.get(name).copied() {
                return Some(start);
            }
        }
        None
    }

    /// Resolve a name *reference* at `ref_span`. Records the resolution into
    /// `resolutions` and emits an unresolved-name error if no match is found.
    fn resolve_name_ref(&mut self, name: &Ident, ref_span: Span) {
        self.resolve_name_ref_ctx(name, ref_span, false);
    }

    /// As `resolve_name_ref`, but when `mut_target` is set the emitted
    /// unresolved-name error records that this name is the whole left-hand side
    /// of a `mut x = e` reassignment, so the diagnostic can offer the
    /// let-vs-mut hint (D: `mut` reassigns an existing binding; it is not
    /// `let mut`).
    fn resolve_name_ref_ctx(&mut self, name: &Ident, ref_span: Span, mut_target: bool) {
        if let Some(start) = self.lookup_local(name) {
            self.resolutions
                .insert(ref_span, ResolvedRef::Local(start));
            return;
        }
        if let Some(id) = self.symbols.lookup(name) {
            self.resolutions.insert(ref_span, ResolvedRef::Module(id));
            return;
        }
        if let Some(id) = self.prelude.lookup(name) {
            self.resolutions.insert(ref_span, ResolvedRef::Prelude(id));
            return;
        }
        self.errors.push(ResolveError::UnresolvedName {
            name: name.to_string(),
            span: ref_span,
            mut_target,
        });
    }

    /// Shared body for `fn` and `component` declarations (D4 + D19): push a
    /// scope, bind generics + params, walk the signature types and the body.
    fn walk_callable(
        &mut self,
        generics: &[GenericParam],
        params: &[Param],
        return_ty: Option<&TypeExpr>,
        body: &Block,
    ) {
        self.push_scope();
        // D7 generics are in scope for the whole signature and body.
        for g in generics {
            self.bind_local(g.name.clone(), g.span);
        }
        for p in params {
            self.bind_local(p.name.clone(), p.span);
            self.walk_type_expr(&p.ty);
        }
        if let Some(rt) = return_ty {
            self.walk_type_expr(rt);
        }
        self.walk_block(body);
        self.pop_scope();
    }

    // ----- decls -----

    fn walk_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Import(_) => {}
            Decl::Fn(f) => self.walk_callable(&f.generics, &f.params, f.return_ty.as_ref(), &f.body),
            Decl::Component(c) => {
                self.walk_callable(&c.generics, &c.params, c.return_ty.as_ref(), &c.body)
            }
            Decl::Type(t) => {
                // Type-decl generic params are in scope for the body of the
                // type declaration only. `type Schema<T> = { parse: fn(...) -> Result<T, ...> }`
                // needs `T` visible inside the body.
                self.push_scope();
                for g in &t.generics {
                    self.bind_local(g.name.clone(), g.span);
                }
                self.walk_type_expr(&t.body);
                self.pop_scope();
            }
            Decl::Const(c) => {
                if let Some(ty) = &c.ty {
                    self.walk_type_expr(ty);
                }
                // `const` bodies are module-level expressions; no function
                // scope, but expression resolution still needs to traverse
                // them. Push an empty scope so `lookup_local` finds nothing.
                self.push_scope();
                self.walk_expr(&c.value);
                self.pop_scope();
            }
        }
    }

    // ----- blocks / stmts -----

    fn walk_block(&mut self, block: &Block) {
        // Each block gets a nested scope so `let` bindings inside don't leak.
        self.push_scope();
        for s in &block.stmts {
            self.walk_stmt(s);
        }
        self.pop_scope();
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let(l) => {
                if let Some(ty) = &l.ty {
                    self.walk_type_expr(ty);
                }
                self.walk_expr(&l.value);
                // Binding is visible *after* the initializer (sequential let).
                self.bind_local(l.name.clone(), l.span);
            }
            Stmt::Mut(m) => match &m.kind {
                glyph_ast::MutKind::Assign { target, value } => {
                    // The target is an lvalue expression (a name or a field/index
                    // chain); walking it resolves the base binding and any index
                    // subexpressions. When the target is a *bare name*, an
                    // unresolved reference is the classic `let mut` mistake —
                    // `mut x = e` reassigns an existing binding, so tag it so the
                    // diagnostic can point at the missing preceding `let`.
                    match target {
                        Expr::Ident { name, span } => {
                            self.resolve_name_ref_ctx(name, *span, true)
                        }
                        _ => self.walk_expr(target),
                    }
                    self.walk_expr(value);
                }
                glyph_ast::MutKind::MethodCall { call } => {
                    self.walk_expr(call);
                }
            },
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.walk_expr(v);
                }
            }
            Stmt::For(f) => {
                self.walk_expr(&f.iter);
                self.push_scope();
                for b in &f.bindings {
                    // For-binding spans are the statement's span — we don't
                    // have per-binding spans in `ForStmt`. Use the for span as
                    // the def-site marker. (Two bindings share a marker, but
                    // local refs only need *some* def-site key.)
                    self.bind_local(b.clone(), f.span);
                }
                self.walk_block(&f.body);
                self.pop_scope();
            }
            Stmt::Loop(l) => self.walk_block(&l.body),
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => self.walk_expr(e),
        }
    }

    // ----- expressions -----

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Number { .. }
            | Expr::String { .. }
            | Expr::Bool { .. }
            | Expr::Void { .. } => {}
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.walk_expr(value);
                    }
                }
            }
            Expr::Ident { name, span } => self.resolve_name_ref(name, *span),
            Expr::Binary { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::Unary { operand, .. } => self.walk_expr(operand),
            Expr::Postfix { operand, .. } => self.walk_expr(operand),
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                self.walk_expr(callee);
                for t in type_args {
                    self.walk_type_expr(t);
                }
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::Member { object, .. } => self.walk_expr(object),
            Expr::Index { object, index, .. } => {
                self.walk_expr(object);
                self.walk_expr(index);
            }
            Expr::Await { expr, .. } => self.walk_expr(expr),
            Expr::Array { elements, .. } => {
                for e in elements {
                    match e {
                        ArrayElem::Expr(e) | ArrayElem::Spread(e) => self.walk_expr(e),
                    }
                }
            }
            Expr::Object { fields, .. } => {
                for f in fields {
                    match f {
                        ObjectField::KeyValue { value, .. } => self.walk_expr(value),
                        ObjectField::Spread { value, .. } => self.walk_expr(value),
                    }
                }
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.walk_expr(scrutinee);
                for arm in arms {
                    self.push_scope();
                    self.walk_pattern(&arm.pattern);
                    match &arm.body {
                        MatchArmBody::Expr(e) => self.walk_expr(e),
                        // Pattern bindings must be visible in the block body,
                        // so we walk stmts directly instead of `walk_block`
                        // (which would push a fresh scope and hide them).
                        MatchArmBody::Block(b) => {
                            for s in &b.stmts {
                                self.walk_stmt(s);
                            }
                        }
                    }
                    self.pop_scope();
                }
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                ..
            } => {
                self.push_scope();
                for p in params {
                    self.bind_local(p.name.clone(), p.span);
                    self.walk_type_expr(&p.ty);
                }
                if let Some(rt) = return_ty {
                    self.walk_type_expr(rt);
                }
                self.walk_block(body);
                self.pop_scope();
            }
            Expr::Jsx(j) => self.walk_jsx(j),
        }
    }

    fn walk_jsx(&mut self, j: &JsxElement) {
        let kind = JsxKind::classify(&j.name);
        if kind == JsxKind::Component {
            // A member-expression name (`Ctx.Provider`) resolves its base
            // segment only; the `.Provider` part is a property access, not a
            // separate binding.
            match j.name.split_once('.') {
                Some((base, _)) => {
                    let base_ident: Ident = std::sync::Arc::from(base);
                    self.resolve_name_ref(&base_ident, j.span);
                }
                None => self.resolve_name_ref(&j.name, j.span),
            }
        }

        // Directive bindings introduce locals scoped to the element:
        // - `<for X in={iter} ...>` — first positional attr `X` is the loop
        //   variable. `in={...}` evaluates in the *outer* scope so the
        //   iterable can't accidentally see the binding.
        // - `<case Variant bind={X}>` — `bind={X}` introduces `X` as a binding
        //   visible to the element's children.
        if kind == JsxKind::For {
            if let Some(iter_expr) = find_expr_attr(&j.attrs, "in") {
                self.walk_expr(iter_expr);
            }
        }

        self.push_scope();

        if kind == JsxKind::For {
            if let Some((name, span)) = first_positional(&j.attrs) {
                self.bind_local(name.clone(), span);
            }
        }
        if kind == JsxKind::Case {
            if let Some(Expr::Ident {
                name: bind_name,
                span: bind_span,
            }) = find_expr_attr(&j.attrs, "bind")
            {
                self.bind_local(bind_name.clone(), *bind_span);
            }
        }

        for attr in &j.attrs {
            match attr {
                JsxAttr::String { .. } => {}
                JsxAttr::Expr { name, value, .. } => {
                    // Skip the directive-binding attrs already handled.
                    let consumed = (kind == JsxKind::For && name.as_ref() == "in")
                        || (kind == JsxKind::Case && name.as_ref() == "bind");
                    if !consumed {
                        self.walk_expr(value);
                    }
                }
                JsxAttr::Positional { name, span } => {
                    if kind == JsxKind::For {
                        // Already consumed as the loop binding.
                        continue;
                    }
                    self.resolve_name_ref(name, *span);
                }
                JsxAttr::Spread { value, .. } => self.walk_expr(value),
            }
        }

        for child in &j.children {
            match child {
                JsxChild::Element(e) => self.walk_jsx(e),
                JsxChild::Expr(e) => self.walk_expr(e),
                JsxChild::Text { .. } => {}
            }
        }

        self.pop_scope();
    }

    // ----- patterns -----

    fn walk_pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Wildcard { .. } | Pattern::Else { .. } | Pattern::Literal { .. } => {}
            Pattern::Ident { name, span } => {
                // A single-segment ident pattern is ambiguous by shape: a
                // lowercase name (`x`, `_rest`) is a fresh binding; an
                // uppercase name (`Idle`, `Ok`, `None`) is a no-payload
                // constructor of the scrutinee's union. Mirror the JSX
                // `<case Variant>` path: resolve a constructor-shaped name as
                // a reference so a misspelled or unknown variant (`Loded`)
                // raises E0103 instead of being silently bound as an
                // irrefutable catch-all (which masks non-exhaustiveness and
                // misroutes variants at runtime).
                if is_constructor_shaped(name) {
                    self.resolve_name_ref(name, *span);
                } else {
                    self.bind_local(name.clone(), *span);
                }
            }
            Pattern::Constructor { path, args, span } => {
                // Constructor path: resolve the first segment as a name
                // reference (the rest is dotted lookup, handled by the
                // typechecker). E.g. `Ok(x)` resolves `Ok`; `fs.ErrorKind.NotFound`
                // resolves `fs`.
                if let Some(first) = path.first() {
                    self.resolve_name_ref(first, *span);
                }
                for arg in args {
                    self.walk_pattern(arg);
                }
            }
            Pattern::Object { fields, .. } => {
                for f in fields {
                    // `{ name }` binds `name`; `{ name: alias }` binds `alias`.
                    let binding_name = f.binding.clone().unwrap_or_else(|| f.key.clone());
                    self.bind_local(binding_name, f.span);
                }
            }
            Pattern::Array { elements, rest, .. } => {
                for el in elements {
                    self.walk_pattern(el);
                }
                if let Some(r) = rest {
                    self.walk_pattern(r);
                }
            }
            Pattern::IsType { ty, .. } => {
                self.walk_type_expr(ty);
            }
        }
    }

    // ----- type expressions -----

    fn walk_type_expr(&mut self, te: &TypeExpr) {
        match te {
            TypeExpr::Path { segments, span } => {
                if let Some(first) = segments.first() {
                    // Single-segment paths resolve against the local module
                    // and prelude. Multi-segment paths' first segment resolves
                    // similarly; subsequent segments are members handled by
                    // the typechecker.
                    self.resolve_name_ref(first, *span);
                }
            }
            TypeExpr::Generic { base, args, .. } => {
                self.walk_type_expr(base);
                for a in args {
                    self.walk_type_expr(a);
                }
            }
            TypeExpr::Fn { params, return_ty, .. } => {
                for p in params {
                    self.walk_type_expr(&p.ty);
                }
                if let Some(rt) = return_ty {
                    self.walk_type_expr(rt);
                }
            }
            TypeExpr::Record { fields, .. } => {
                for f in fields {
                    self.walk_type_expr(&f.ty);
                }
            }
            TypeExpr::Union { variants, .. } => {
                for v in variants {
                    if let Some(p) = &v.payload {
                        self.walk_type_expr(p);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::collect_module_symbols;
    use crate::prelude::build_prelude;
    use glyph_parser::parse;

    fn resolve(src: &str) -> (ResolvedModule, Vec<ResolveError>) {
        let m = parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).expect("collect failed");
        let prelude = build_prelude();
        resolve_module(&m, syms, &prelude)
    }

    #[test]
    fn fn_param_resolves_to_local() {
        let (rm, errs) = resolve("module x\nfn id(a: number) -> number { return a }\n");
        assert!(errs.is_empty(), "errors: {errs:?}");
        assert!(rm.resolutions.len() > 0, "should have resolved `a` ref");
        // Find the resolved entry that's a Local.
        let any_local = rm
            .resolutions
            .iter()
            .any(|(_, r)| matches!(r, ResolvedRef::Local(_)));
        assert!(any_local);
    }

    #[test]
    fn module_decl_resolves_call_site() {
        let src = r#"module x
fn helper() -> number { return 1 }
fn main() -> number { return helper() }
"#;
        let (rm, errs) = resolve(src);
        assert!(errs.is_empty(), "errors: {errs:?}");
        let any_module = rm
            .resolutions
            .iter()
            .any(|(_, r)| matches!(r, ResolvedRef::Module(_)));
        assert!(any_module);
    }

    #[test]
    fn prelude_ok_resolves() {
        let src = r#"module x
fn one() -> number { return 1 }
fn main() { let _r = Ok(one()) }
"#;
        let (rm, errs) = resolve(src);
        assert!(errs.is_empty(), "errors: {errs:?}");
        let any_prelude = rm
            .resolutions
            .iter()
            .any(|(_, r)| matches!(r, ResolvedRef::Prelude(_)));
        assert!(any_prelude, "expected at least one prelude resolution");
    }

    #[test]
    fn unresolved_name_errors() {
        let (_, errs) = resolve("module x\nfn main() { return missing }\n");
        assert!(
            errs.iter()
                .any(|e| matches!(e, ResolveError::UnresolvedName { name, .. } if name == "missing")),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn mut_on_never_bound_name_flags_mut_target() {
        // `mut total = 0` as the first mention of `total` is the classic
        // `let mut` mistake: `mut` reassigns an existing binding, so the LHS
        // is unresolved. The error must carry `mut_target: true` so the CLI
        // can offer the let-vs-mut hint instead of the generic one.
        let src = r#"module main
fn sum() -> number {
  mut total = 0
  return total
}
"#;
        let (_, errs) = resolve(src);
        let mut_err = errs.iter().find(
            |e| matches!(e, ResolveError::UnresolvedName { name, .. } if name == "total"),
        );
        assert!(mut_err.is_some(), "expected an unresolved `total`: {errs:?}");
        assert!(
            matches!(
                mut_err.unwrap(),
                ResolveError::UnresolvedName { mut_target: true, .. }
            ),
            "mut-target flag not set: {errs:?}"
        );
        assert!(
            mut_err.unwrap().help().unwrap().contains("`let`"),
            "help should mention `let`: {:?}",
            mut_err.unwrap().help()
        );
    }

    #[test]
    fn match_arm_binding_visible_in_body() {
        let src = r#"module x
fn handle(r: number) -> number {
  return match r {
    Ok(v) => v,
    Err(_) => 0,
  }
}
"#;
        let (_, errs) = resolve(src);
        // `v` should resolve — it's bound by the Ok arm and used as the body.
        assert!(
            !errs.iter()
                .any(|e| matches!(e, ResolveError::UnresolvedName { name, .. } if name == "v")),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn known_variant_arm_head_resolves_not_binds() {
        // A correctly-spelled no-payload variant (`Idle`, `Done`) in an arm
        // head resolves against the hoisted Variant symbols; no unresolved
        // error, and a lowercase catch-all binding still works.
        let src = r#"module x
type Status = | Idle | Done
fn label(s: Status) -> string {
  return match s {
    Idle => "idle",
    other => "other",
  }
}
"#;
        let (_, errs) = resolve(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn typo_constructor_arm_head_is_unresolved() {
        // A PascalCase arm head that names no known variant (`Loded` vs
        // `Loaded`) is constructor-shaped, so it must resolve as a name
        // reference and fail with an unresolved-name error rather than being
        // silently bound as an irrefutable catch-all. Mirrors the JSX
        // `<case Variant>` path; keeps the value-match path from being
        // strictly weaker than the JSX one.
        let src = r#"module x
type Status = | Idle | Loaded | Done
fn label(s: Status) -> string {
  return match s {
    Idle => "idle",
    Loded => "loaded",
  }
}
"#;
        let (_, errs) = resolve(src);
        assert!(
            errs.iter()
                .any(|e| matches!(e, ResolveError::UnresolvedName { name, .. } if name == "Loded")),
            "expected unresolved `Loded`: {errs:?}"
        );
    }

    #[test]
    fn for_binding_visible_in_body() {
        let src = r#"module x
fn sum(xs: number) -> number {
  for item in xs {
    return item
  }
  return 0
}
"#;
        let (_, errs) = resolve(src);
        assert!(
            !errs.iter()
                .any(|e| matches!(e, ResolveError::UnresolvedName { name, .. } if name == "item")),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn lambda_param_visible_in_body() {
        let src = r#"module x
fn make() -> number {
  let f = fn(y: number) -> number { return y + 1 }
  return 0
}
"#;
        let (_, errs) = resolve(src);
        assert!(
            !errs.iter()
                .any(|e| matches!(e, ResolveError::UnresolvedName { name, .. } if name == "y")),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn type_decl_named_ref_resolves() {
        let src = r#"module x
type Issue = { message: string }
type Bundle = { issue: Issue }
"#;
        let (_, errs) = resolve(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }
}
