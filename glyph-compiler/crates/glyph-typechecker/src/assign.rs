//! Assign a `Ty` to every expression node in a module.
//!
//! Week-2 acceptance: "every Expr has a Ty (some Unknown is fine)."
//! Concrete types are produced for:
//! - Literals (number, string, template-string, bool, void)
//! - Identifier references whose resolution targets a typed symbol (function
//!   declaration, lambda parameter via the signature, prelude constructor)
//! - Lambdas (the literal's type is its declared signature)
//!
//! Everything else gets `Ty::Unknown` and will be filled in by the week-3
//! bidirectional checker. This walker doesn't propagate types up
//! expressions — `a + b` has type `Unknown` even when both operands are
//! `Number`.

use std::cell::RefCell;
use std::collections::HashMap;

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, JsxAttr, JsxChild, JsxElement, MatchArmBody, Module,
    ObjectField, Param, Stmt, TemplatePart,
};
use glyph_resolver::{Prelude, ResolvedModule, ResolvedRef, SymbolKind};

use crate::lower::Lowerer;
use crate::ty::{Primitive, Ty};
use crate::type_map::TypeMap;

/// Source of per-declaration `Ty` answers. The Assigner queries the resolver
/// every time it needs the type of a `fn`/`component` reference; injecting
/// the lookup as a trait lets the salsa-aware caller in `glyph-db` route
/// these through the memoized `decl_ty(db, file, idx)` query, while the
/// db-less callers in this crate use the local `LocalDeclTy` default.
///
/// **Contract**: an impl MUST return the result of lowering the signature
/// of `module.items[decl_idx]` via `Lowerer::lower_decl_signature` against
/// the same `(resolved, prelude)` that was passed to
/// `assign_types_with_resolver`. Returning anything else produces an
/// internally-inconsistent `TypeMap` with no compile-time error — type
/// inference downstream silently sees `Ty::Unknown` where it should see a
/// concrete `Ty::Fn`. The two shipped impls (`LocalDeclTy` here and
/// `SalsaDeclTy` in `glyph-db`) both delegate to
/// `Lowerer::lower_decl_signature`; new impls should do the same or wrap
/// one of them.
pub trait DeclTyResolver {
    fn decl_ty(&self, decl_idx: u32) -> Ty;
}

/// Default `DeclTyResolver` for callers that don't have a salsa `Db`. Owns
/// a `RefCell<HashMap<decl_idx, Ty>>` cache so each decl is lowered at most
/// once per `assign_types` invocation, matching the pre-day-7 behavior. The
/// cache is `RefCell`-backed (interior mutability) — `LocalDeclTy` is `!Sync`.
///
/// The constructor is `pub(crate)`: building one externally would let a
/// caller pair a `Module` with a `Lowerer` built from an unrelated
/// `(resolved, prelude)`, silently producing wrong `Ty` answers. External
/// crates with their own context should implement `DeclTyResolver`
/// directly (see `SalsaDeclTy` in `glyph-db` for the pattern).
pub struct LocalDeclTy<'a> {
    module: &'a Module,
    lowerer: &'a Lowerer<'a>,
    cache: RefCell<HashMap<u32, Ty>>,
}

impl<'a> LocalDeclTy<'a> {
    pub(crate) fn new(module: &'a Module, lowerer: &'a Lowerer<'a>) -> Self {
        Self {
            module,
            lowerer,
            cache: RefCell::new(HashMap::new()),
        }
    }
}

impl DeclTyResolver for LocalDeclTy<'_> {
    fn decl_ty(&self, decl_idx: u32) -> Ty {
        // Drop the immutable borrow before doing anything else — keeping it
        // alive across `ty.clone()` would block a hypothetical future
        // reentrant `decl_ty` call from inside `Lowerer::lower_decl_signature`.
        let cached = self.cache.borrow().get(&decl_idx).cloned();
        if let Some(ty) = cached {
            return ty;
        }
        let ty = self
            .module
            .items
            .get(decl_idx as usize)
            .map(|d| self.lowerer.lower_decl_signature(d))
            .unwrap_or(Ty::Unknown);
        self.cache.borrow_mut().insert(decl_idx, ty.clone());
        ty
    }
}

/// Assign a `Ty` to every expression node in `module`, using the local
/// `LocalDeclTy` resolver. Direct-call entry point for callers without a
/// salsa `Db`; `glyph-db`'s `type_map` query goes through
/// `assign_types_with_resolver` instead.
pub fn assign_types(module: &Module, resolved: &ResolvedModule, prelude: &Prelude) -> TypeMap {
    let lowerer = Lowerer::new(resolved, prelude);
    let resolver = LocalDeclTy::new(module, &lowerer);
    assign_types_with_resolver(module, resolved, prelude, &resolver)
}

/// Same as `assign_types`, but the caller supplies the `DeclTyResolver`.
/// The salsa-backed `glyph-db` caller passes a resolver whose `decl_ty`
/// method invokes the cached `decl_ty(db, file, k)` query, so each `Ty`
/// answer is shared across the entire database revision instead of being
/// recomputed locally.
pub fn assign_types_with_resolver(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
    decl_ty_resolver: &dyn DeclTyResolver,
) -> TypeMap {
    let mut tm = TypeMap::new();
    let mut assigner = Assigner {
        lowerer: Lowerer::new(resolved, prelude),
        resolved,
        tm: &mut tm,
        decl_ty_resolver,
        local_tys: HashMap::new(),
    };
    for decl in &module.items {
        assigner.walk_decl(decl);
    }
    tm
}

struct Assigner<'a> {
    lowerer: Lowerer<'a>,
    resolved: &'a ResolvedModule,
    tm: &'a mut TypeMap,
    /// Plug-in source of `Ty::Fn` answers for module-level fn/component
    /// references. Each call returns the lowered Ty for the given decl_idx;
    /// the Assigner doesn't keep a local `decl_ty` map any more.
    ///
    /// Per-invocation caching behavior differs by impl:
    /// - `LocalDeclTy` (db-less callers): in-memory `HashMap` short-circuits
    ///   repeated references to the same fn inside one `assign_types` call.
    /// - `SalsaDeclTy` (`glyph-db`): no per-invocation cache — every call
    ///   pays a salsa fetch + a full `Ty::clone()`. The win is the *cross-
    ///   revision* memo, which `LocalDeclTy` doesn't have. For hot paths
    ///   (e.g. fn bodies with many references to the same helper), a layer
    ///   above `SalsaDeclTy` could amortize the per-call cost — day-7
    ///   chose simplicity over this optimization.
    decl_ty_resolver: &'a dyn DeclTyResolver,
    /// Type of each locally-bound name, keyed by the def-site span start the
    /// resolver records in `ResolvedRef::Local`. Populated from typed function
    /// / component / lambda parameters and typed `let` bindings. For-loop
    /// bindings and match-arm payload bindings stay absent (the former share
    /// a def-site span across K/V, the latter need the bidirectional checker
    /// to derive types from the scrutinee).
    local_tys: HashMap<u32, Ty>,
}

impl Assigner<'_> {
    // ----- decls -----

    fn walk_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Import(_) | Decl::Type(_) => {}
            Decl::Fn(f) => {
                self.bind_param_tys(&f.params);
                self.walk_block(&f.body);
            }
            Decl::Component(c) => {
                self.bind_param_tys(&c.params);
                self.walk_block(&c.body);
            }
            Decl::Const(c) => self.walk_expr(&c.value),
        }
    }

    /// Record each param's lowered type under its def-site key. Mirrors the
    /// resolver's `bind_local(name, p.span)` convention so the def-site start
    /// matches what `ResolvedRef::Local` carries.
    fn bind_param_tys(&mut self, params: &[Param]) {
        for p in params {
            let ty = self.lowerer.lower(&p.ty);
            self.local_tys.insert(p.span.start, ty);
        }
    }

    fn walk_block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                self.walk_expr(&l.value);
                if let Some(te) = &l.ty {
                    let ty = self.lowerer.lower(te);
                    self.local_tys.insert(l.span.start, ty);
                }
            }
            Stmt::Mut(m) => match &m.kind {
                glyph_ast::MutKind::Assign { value, .. } => self.walk_expr(value),
                glyph_ast::MutKind::AssignIndex { index, value, .. } => {
                    self.walk_expr(index);
                    self.walk_expr(value);
                }
                glyph_ast::MutKind::AssignField { value, .. } => self.walk_expr(value),
                glyph_ast::MutKind::MethodCall { receiver, call } => {
                    self.walk_expr(receiver);
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
                self.walk_block(&f.body);
            }
            Stmt::Loop(l) => self.walk_block(&l.body),
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => self.walk_expr(e),
        }
    }

    // ----- expressions -----

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            Expr::Number { span, .. } => self.tm.insert(*span, Ty::Prim(Primitive::Number)),
            Expr::String { span, .. } => self.tm.insert(*span, Ty::Prim(Primitive::String)),
            Expr::Bool { span, .. } => self.tm.insert(*span, Ty::Prim(Primitive::Bool)),
            Expr::Void { span } => self.tm.insert(*span, Ty::Prim(Primitive::Void)),
            Expr::TemplateString { parts, span } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        self.walk_expr(value);
                    }
                }
                self.tm.insert(*span, Ty::Prim(Primitive::String));
            }
            Expr::Ident { span, .. } => {
                let ty = self.type_of_ident_ref(*span);
                self.tm.insert(*span, ty);
            }
            Expr::Unary { operand: child, span, .. }
            | Expr::Postfix { operand: child, span, .. }
            | Expr::Member { object: child, span, .. }
            | Expr::Await { expr: child, span } => {
                self.walk_expr(child);
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Binary { left, right, span, .. }
            | Expr::Index {
                object: left,
                index: right,
                span,
                ..
            } => {
                self.walk_expr(left);
                self.walk_expr(right);
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Call {
                callee, args, span, ..
            } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_expr(a);
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Array { elements, span } => {
                for el in elements {
                    let (ArrayElem::Expr(e) | ArrayElem::Spread(e)) = el;
                    self.walk_expr(e);
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Object { fields, span } => {
                for f in fields {
                    let (ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. }) =
                        f;
                    self.walk_expr(value);
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Match { scrutinee, arms, span } => {
                self.walk_expr(scrutinee);
                for arm in arms {
                    match &arm.body {
                        MatchArmBody::Expr(e) => self.walk_expr(e),
                        MatchArmBody::Block(b) => self.walk_block(b),
                    }
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                span,
            } => {
                self.bind_param_tys(params);
                self.walk_block(body);
                let ty = self
                    .lowerer
                    .lower_callable_signature(params, return_ty.as_ref(), false);
                self.tm.insert(*span, ty);
            }
            Expr::Jsx(j) => {
                self.walk_jsx(j);
                self.tm.insert(j.span, Ty::Unknown);
            }
        }
    }

    fn walk_jsx(&mut self, j: &JsxElement) {
        for attr in &j.attrs {
            if let JsxAttr::Expr { value, .. } = attr {
                self.walk_expr(value);
            }
        }
        for child in &j.children {
            match child {
                JsxChild::Element(e) => self.walk_jsx(e),
                JsxChild::Expr(e) => self.walk_expr(e),
                JsxChild::Text { .. } => {}
            }
        }
    }

    // ----- ident reference typing -----

    fn type_of_ident_ref(&mut self, ref_span: glyph_ast::Span) -> Ty {
        let Some(r) = self.resolved.resolutions.get(ref_span) else {
            return Ty::Unknown;
        };
        match r {
            ResolvedRef::Local(def_start) => self
                .local_tys
                .get(&def_start)
                .cloned()
                .unwrap_or(Ty::Unknown),
            ResolvedRef::Module(id) => {
                let sym = self.resolved.symbols.table.get(id).expect("symbol id valid");
                match &sym.kind {
                    SymbolKind::Function { decl_idx }
                    | SymbolKind::Component { decl_idx } => {
                        self.decl_ty_resolver.decl_ty(*decl_idx)
                    }
                    _ => Ty::Unknown,
                }
            }
            // Prelude values (`Ok`, `Err`, etc.) need use-site generic
            // instantiation — week-3 bidirectional checker work.
            ResolvedRef::Prelude(_) => Ty::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    fn type_map_of(src: &str) -> (Module, ResolvedModule, TypeMap) {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let tm = assign_types(&m, &resolved, &prelude);
        (m, resolved, tm)
    }

    /// Convenience: extract the first let statement's value expression from
    /// the first fn decl. Used by every literal-typing test.
    fn first_let_value_span(m: &Module) -> glyph_ast::Span {
        let f = match &m.items[0] {
            Decl::Fn(f) => f,
            _ => panic!("first decl is not a Fn"),
        };
        let l = match &f.body.stmts[0] {
            Stmt::Let(l) => l,
            _ => panic!("first stmt is not a Let"),
        };
        l.value.span()
    }

    #[test]
    fn number_literal_typed() {
        let (m, _, tm) = type_map_of("module x\nfn main() { let x = 42 }\n");
        assert!(matches!(
            tm.get(first_let_value_span(&m)),
            Ty::Prim(Primitive::Number)
        ));
    }

    #[test]
    fn string_literal_typed() {
        let (m, _, tm) = type_map_of(r#"module x
fn main() { let x = "hi" }
"#);
        assert!(matches!(
            tm.get(first_let_value_span(&m)),
            Ty::Prim(Primitive::String)
        ));
    }

    #[test]
    fn template_string_typed() {
        let (m, _, tm) = type_map_of(r#"module x
fn greet(name: string) { let x = "hello ${name}" }
"#);
        assert!(matches!(
            tm.get(first_let_value_span(&m)),
            Ty::Prim(Primitive::String)
        ));
    }

    #[test]
    fn fn_ident_ref_takes_signature() {
        let src = r#"module x
fn helper(a: number) -> string { return "ok" }
fn main() { let f = helper }
"#;
        let (m, _, tm) = type_map_of(src);
        let main = match &m.items[1] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let l = match &main.body.stmts[0] {
            Stmt::Let(l) => l,
            _ => panic!(),
        };
        match tm.get(l.value.span()) {
            Ty::Fn {
                params, return_ty, ..
            } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0].ty, Ty::Prim(Primitive::Number)));
                assert!(matches!(&**return_ty, Ty::Prim(Primitive::String)));
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn typed_param_propagates_to_ident_refs() {
        let (m, _, tm) = type_map_of("module x\nfn id(a: number) -> number { return a }\n");
        let f = match &m.items[0] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ret_val = match &f.body.stmts[0] {
            Stmt::Return(r) => r.value.as_ref().unwrap(),
            _ => panic!(),
        };
        assert!(matches!(tm.get(ret_val.span()), Ty::Prim(Primitive::Number)));
    }

    #[test]
    fn typed_let_propagates_to_later_refs() {
        let src = r#"module x
fn main() -> string {
  let x: string = "hi"
  return x
}
"#;
        let (m, _, tm) = type_map_of(src);
        let f = match &m.items[0] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ret_val = match &f.body.stmts[1] {
            Stmt::Return(r) => r.value.as_ref().unwrap(),
            _ => panic!(),
        };
        assert!(matches!(tm.get(ret_val.span()), Ty::Prim(Primitive::String)));
    }

    #[test]
    fn untyped_let_local_stays_unknown() {
        // `let x = 42` has no annotation; week-2 doesn't infer from the
        // initializer, so refs to `x` are Unknown until the week-3 checker.
        let src = r#"module x
fn main() -> number {
  let x = 42
  return x
}
"#;
        let (m, _, tm) = type_map_of(src);
        let f = match &m.items[0] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ret_val = match &f.body.stmts[1] {
            Stmt::Return(r) => r.value.as_ref().unwrap(),
            _ => panic!(),
        };
        assert!(tm.get(ret_val.span()).is_unknown());
    }

    #[test]
    fn lambda_param_propagates_to_body() {
        let src = r#"module x
fn main() {
  let f = fn(y: number) -> number { return y }
}
"#;
        let (m, _, tm) = type_map_of(src);
        let f = match &m.items[0] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let lambda = match &f.body.stmts[0] {
            Stmt::Let(l) => &l.value,
            _ => panic!(),
        };
        let body = match lambda {
            Expr::Lambda { body, .. } => body,
            _ => panic!(),
        };
        let ret_val = match &body.stmts[0] {
            Stmt::Return(r) => r.value.as_ref().unwrap(),
            _ => panic!(),
        };
        assert!(matches!(tm.get(ret_val.span()), Ty::Prim(Primitive::Number)));
    }

    #[test]
    fn lambda_typed_as_signature() {
        let src = r#"module x
fn main() {
  let f = fn(y: number) -> number { return y }
}
"#;
        let (m, _, tm) = type_map_of(src);
        assert!(matches!(tm.get(first_let_value_span(&m)), Ty::Fn { .. }));
    }
}
