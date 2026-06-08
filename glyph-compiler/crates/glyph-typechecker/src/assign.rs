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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glyph_ast::{
    ArrayElem, Block, Decl, Expr, Ident, JsxAttr, JsxChild, JsxElement, MatchArm, MatchArmBody,
    Module, ObjectField, ObjectPatternField, Param, Pattern, PostfixOp, Span, Stmt, TemplatePart,
    TypeExpr,
};
use glyph_resolver::{Prelude, ResolvedModule, ResolvedRef, SymbolId, SymbolKind};

use crate::lower::Lowerer;
use crate::ty::{ty_display, FnParam, Primitive, RecordField, Ty, UnionVariant};
use crate::type_map::TypeMap;
use crate::TypeError;

/// How the innermost enclosing callable's declared return type relates to
/// the `?` operator's requirement (D + week-3 task 2). Pushed onto
/// `Assigner::return_stack` when entering a `fn`/`component`/lambda body
/// and popped on exit, so a `?` inside a nested lambda is checked against
/// the lambda's return type rather than the outer function's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnClass {
    /// Declared `-> Result<_, _>`. `?` is legal.
    Result,
    /// Declared a concrete non-`Result` type (e.g. `-> number`, `-> void`,
    /// `-> Component`). `?` is an error.
    NonResult,
    /// No return annotation, or one whose type couldn't be resolved
    /// (multi-segment path, generic parameter). Permissive: `?` is not
    /// flagged here because we can't prove the return type isn't a
    /// `Result`. D4 makes the return annotation optional, so this case is
    /// common and must not produce false positives.
    Unknown,
}

/// The innermost enclosing callable's declared return type, tracked on a
/// stack so nested lambdas check against their own return. Bundles the
/// `ReturnClass` (for the `?` rule) with the lowered `Ty` (for return-type
/// mismatch checking) so the two can never desync across push/pop sites.
#[derive(Debug, Clone)]
struct EnclosingReturn {
    class: ReturnClass,
    /// The lowered declared return type, or `Ty::Unknown` when there is no
    /// annotation or it could not be resolved.
    ty: Ty,
}

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
///
/// Returns the `TypeMap` plus any `TypeError`s the walker collected (as
/// of day 14: non-exhaustive `match` on tagged unions).
pub fn assign_types(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
) -> (TypeMap, Vec<TypeError>) {
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
) -> (TypeMap, Vec<TypeError>) {
    let mut tm = TypeMap::new();
    let mut errors: Vec<TypeError> = Vec::new();
    {
        let mut assigner = Assigner {
            module,
            lowerer: Lowerer::new(resolved, prelude),
            resolved,
            tm: &mut tm,
            errors: &mut errors,
            decl_ty_resolver,
            return_stack: Vec::new(),
            local_tys: HashMap::new(),
        };
        for decl in &module.items {
            assigner.walk_decl(decl);
        }
    }
    // D25: a second pass over the completed `TypeMap`. `owned` single-
    // consumption analysis reads each call site's callee `Ty::Fn` (with its
    // per-parameter `owned` flags), so it must run after assignment fills the
    // map rather than interleaved with it.
    errors.extend(crate::owned::check_owned(module, resolved, prelude, &tm));
    (tm, errors)
}

struct Assigner<'a> {
    /// The parsed module — needed to chase `Ty::Named` symbols back to
    /// their `TypeDecl` for the day-14 match-exhaustiveness check.
    module: &'a Module,
    lowerer: Lowerer<'a>,
    resolved: &'a ResolvedModule,
    tm: &'a mut TypeMap,
    /// Diagnostics collected during the walk. Day 14 ships the first
    /// real consumer: non-exhaustive match on tagged unions.
    errors: &'a mut Vec<TypeError>,
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
    /// Return-type classification of each enclosing callable, innermost
    /// last. Drives the `?`-operator check (`QuestionOutsideResultFn`).
    /// Empty when walking a `const` initializer (no enclosing callable),
    /// which makes a bare `?` there an error.
    return_stack: Vec<EnclosingReturn>,
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
                let er = self.enclosing_return(f.return_ty.as_ref());
                self.return_stack.push(er);
                self.bind_param_tys(&f.params);
                self.walk_block(&f.body);
                self.return_stack.pop();
            }
            Decl::Component(c) => {
                let er = self.enclosing_return(c.return_ty.as_ref());
                self.return_stack.push(er);
                self.bind_param_tys(&c.params);
                self.walk_block(&c.body);
                self.return_stack.pop();
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
                    self.check_return_type(v);
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
            Expr::Postfix { op, operand, span } => {
                self.walk_expr(operand);
                if matches!(op, PostfixOp::Try) {
                    self.check_question_operator(*span);
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Unary { operand: child, span, .. }
            | Expr::Member { object: child, span, .. } => {
                self.walk_expr(child);
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Await { expr, span } => {
                // A Glyph `async fn -> T` is awaited to a `T` (the declared
                // return type is the awaited type; there is no user-visible
                // `Promise<T>` wrapper). So `await e` synthesizes the same
                // type as `e`. This lets `match await fetch() { .. }` see
                // through to the callee's return type for exhaustiveness.
                self.walk_expr(expr);
                let ty = self.tm.get(expr.span()).clone();
                self.tm.insert(*span, ty);
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
                // Synthesize the call's type from the callee's signature.
                // When the callee resolves to a `Ty::Fn` (a module-level
                // fn/component reference or a typed lambda binding), the
                // call has that fn's return type, with type parameters
                // instantiated from the argument types. A generic
                // `fn id<T>(x: T) -> T` called with a `number` argument
                // types as `number`. Any non-`Fn` callee (member-access
                // method, an unresolved name) leaves the call `Unknown`.
                let call_ty = match self.tm.get(callee.span()) {
                    Ty::Fn { params, return_ty, .. } => {
                        let mut subst: HashMap<Ident, Ty> = HashMap::new();
                        for (p, a) in params.iter().zip(args.iter()) {
                            collect_type_param_bindings(&p.ty, self.tm.get(a.span()), &mut subst);
                        }
                        substitute_type_params(return_ty, &subst)
                    }
                    _ => Ty::Unknown,
                };
                self.tm.insert(*span, call_ty);
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
                // Day-14 exhaustiveness check: when the scrutinee's type
                // resolves to a user-defined tagged union, verify the
                // arms cover every variant. Walk the scrutinee FIRST so
                // its type is in `tm`; then look it up.
                let scrutinee_ty = self.tm.get(scrutinee.span()).clone();
                self.check_match_exhaustiveness(&scrutinee_ty, arms, *span);
                for arm in arms {
                    // Type the arm's payload binding from the matched
                    // variant before walking the body, so refs to it inside
                    // the body resolve to the payload type.
                    self.bind_arm_payloads(&scrutinee_ty, &arm.pattern);
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
                let er = self.enclosing_return(return_ty.as_ref());
                self.return_stack.push(er);
                self.bind_param_tys(params);
                self.walk_block(body);
                self.return_stack.pop();
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

    // ----- day-15: `?` operator typing rule -----

    /// Build the `EnclosingReturn` for a declared return type: its
    /// `ReturnClass` (for the `?` rule) and its lowered `Ty` (for
    /// return-type mismatch checking). Both err toward *permissive* — a
    /// missing annotation (legal under D4) or one that can't be resolved
    /// yields `ReturnClass::Unknown` and `Ty::Unknown`, so neither check
    /// fires on a type it can't judge.
    fn enclosing_return(&self, return_ty: Option<&TypeExpr>) -> EnclosingReturn {
        let Some(te) = return_ty else {
            return EnclosingReturn { class: ReturnClass::Unknown, ty: Ty::Unknown };
        };
        let ty = self.lowerer.lower(te);
        let class = if self.type_expr_is_result(te) {
            ReturnClass::Result
        } else if self.is_decidably_non_result(&ty) {
            // A concrete, fully-resolved non-`Result` type. Anything that
            // lowers to `Unknown` — including a generic application over an
            // unresolved base (e.g. an imported non-`Result` type) — stays
            // permissive so we never emit a false positive.
            ReturnClass::NonResult
        } else {
            ReturnClass::Unknown
        };
        EnclosingReturn { class, ty }
    }

    /// True if `te` names the `Result` type, applied (`Result<T, E>`) or
    /// bare. Recognizes both the prelude `Result` and an `import std/result
    /// { Result }` named import — the latter lowers to `Ty::Unknown` (imports
    /// aren't resolved to `Ty::Named` yet), so this works from the syntactic
    /// `TypeExpr` and consults the resolver directly rather than the lowered
    /// `Ty`. A locally-declared `type Result` (a `Module`/`Type` resolution)
    /// is intentionally NOT treated as the `?`-compatible `Result`.
    fn type_expr_is_result(&self, te: &TypeExpr) -> bool {
        let base = match te {
            TypeExpr::Generic { base, .. } => base.as_ref(),
            other => other,
        };
        let TypeExpr::Path { segments, span } = base else {
            return false;
        };
        if segments.last().map(|s| s.as_ref()) != Some("Result") {
            return false;
        }
        match self.resolved.resolutions.get(*span) {
            Some(ResolvedRef::Prelude(id)) => self.lowerer.prelude.lookup("Result") == Some(id),
            Some(ResolvedRef::Module(id)) => matches!(
                self.resolved.symbols.table.get(id).map(|s| &s.kind),
                Some(SymbolKind::ImportNamed { original, .. }) if original.as_ref() == "Result"
            ),
            _ => false,
        }
    }

    /// True only when `ty` is a fully-resolved type that is definitively not
    /// a `Result`. `Ty::Unknown`, an `App` over an `Unknown` base, and a
    /// generic `Ty::Param` (which could instantiate to `Result`) are all
    /// undecidable and return false.
    fn is_decidably_non_result(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Unknown => false,
            Ty::App { base, .. } => !matches!(base.as_ref(), Ty::Unknown),
            Ty::Param { .. } => false,
            _ => true,
        }
    }

    /// Flag a `?` whose innermost enclosing callable does not (provably)
    /// return `Result`. An empty stack means the `?` sits in a `const`
    /// initializer with no enclosing callable, which is always an error.
    fn check_question_operator(&mut self, span: Span) {
        let permitted = matches!(
            self.return_stack.last().map(|e| e.class),
            Some(ReturnClass::Result | ReturnClass::Unknown)
        );
        if !permitted {
            self.errors.push(TypeError::QuestionOutsideResultFn { span });
        }
    }

    // ----- day-21: return-type mismatch -----

    /// Flag a `return value` whose value type is provably incompatible with
    /// the enclosing function's declared return type. Day-21 only judges
    /// primitive-vs-primitive mismatches (see `definitely_incompatible`),
    /// so it never fires on a type it can't decide — including every
    /// `Unknown`, generic, named, or structural type.
    fn check_return_type(&mut self, value: &Expr) {
        let Some(expected) = self.return_stack.last().map(|e| e.ty.clone()) else {
            return;
        };
        let found = self.tm.get(value.span()).clone();
        if definitely_incompatible(&found, &expected) {
            self.errors.push(TypeError::TypeMismatch {
                expected: ty_display(&expected),
                found: ty_display(&found),
                span: value.span(),
            });
        }
    }

    // ----- match exhaustiveness for tagged unions -----

    /// If the scrutinee resolves to a tagged union — a user-defined
    /// `type X = | A | B | ...` decl (day 14) or the prelude `Result`
    /// (`Ok`/`Err`) and `Option` (`Some`/`None`) types (day 19) — check
    /// that the arms cover every variant. Scope:
    /// - User unions: `Ty::Named` pointing at a `Decl::Type` whose body is
    ///   a `TypeExpr::Union`. Prelude unions: `Ty::App` over the prelude
    ///   `Result`/`Option` symbol. The top-level variant set is checked, and
    ///   a variant covered ONLY by a nested constructor pattern recurses into
    ///   its payload (e.g. `Ok(Some(x))` forces a check of `Ok(None)`) — see
    ///   `check_patterns_exhaustive`.
    /// - Patterns recognized: `Variant(...)` (constructor, single- or
    ///   multi-segment path — last segment is the variant name),
    ///   bare `Variant` ident, `is TypeName` guard, `_` wildcard,
    ///   `else` catch-all, and arbitrarily-deep single-payload nesting
    ///   (`Ok(Some(x))`, `Ok(Some(Ok(y)))`).
    /// - Patterns NOT recognized (silently skipped at the top level): object
    ///   destructure, array patterns, literal patterns. A single-payload arm
    ///   whose sub-pattern is a binding fully covers its variant.
    ///
    /// **Known trade-off**: a `Pattern::Ident { name }` whose name
    /// doesn't match a variant is treated as a binding (catch-all).
    /// This means a typo like `Loadign` (vs `Loading`) silently passes
    /// exhaustiveness AND catches every input at runtime as a binding.
    /// Fixing this properly needs scrutinee-aware resolver
    /// disambiguation — when an ident's name shadows a hoisted Variant
    /// of the scrutinee's type, the resolver could warn or escalate to
    /// an error per the Glyph 'stricter-than-TS' posture. Deferred to
    /// week 3.
    fn check_match_exhaustiveness(
        &mut self,
        scrutinee_ty: &Ty,
        arms: &[MatchArm],
        match_span: glyph_ast::Span,
    ) {
        if self.is_prelude_array(scrutinee_ty) {
            self.check_array_exhaustiveness(arms, match_span);
            return;
        }
        let patterns: Vec<&Pattern> = arms.iter().map(|a| &a.pattern).collect();
        self.check_patterns_exhaustive(scrutinee_ty, &patterns, match_span);
    }

    /// True if `ty` is an application of the prelude `Array` type
    /// (`Array<T>` → `App(Array, [T])`).
    fn is_prelude_array(&self, ty: &Ty) -> bool {
        let Ty::App { base, .. } = ty else { return false };
        let Ty::Named { symbol, path } = base.as_ref() else { return false };
        path.last().map(|n| n.as_ref()) == Some("Array")
            && self.lowerer.prelude.lookup("Array") == Some(SymbolId(symbol.0))
    }

    /// Exhaustiveness for a `match` over an array scrutinee: every length in
    /// `[0, ∞)` must be covered. A pattern credits coverage only when all its
    /// fixed elements (and its rest, if any) are irrefutable bindings or
    /// wildcards — a literal element like `["help"]` matches only some arrays
    /// of its length, so it is not counted. `[]` covers length 0, `[a, b]`
    /// covers exactly length 2, and `[a, ...rest]` covers every length ≥ 1.
    /// The smallest uncovered length is reported.
    fn check_array_exhaustiveness(&mut self, arms: &[MatchArm], match_span: glyph_ast::Span) {
        let mut covered_lengths: HashSet<usize> = HashSet::new();
        // The smallest fixed-prefix length of an irrefutable rest pattern; it
        // covers every length at or above that value.
        let mut rest_min: Option<usize> = None;
        for arm in arms {
            match &arm.pattern {
                Pattern::Wildcard { .. } | Pattern::Else { .. } | Pattern::Ident { .. } => {
                    // A whole-array binding or catch-all covers every length.
                    return;
                }
                Pattern::Array { elements, rest, .. } => {
                    if !elements.iter().all(is_irrefutable_pattern) {
                        continue;
                    }
                    match rest {
                        None => {
                            covered_lengths.insert(elements.len());
                        }
                        Some(r) if is_irrefutable_pattern(r) => {
                            let k = elements.len();
                            rest_min = Some(rest_min.map_or(k, |m| m.min(k)));
                        }
                        // A refutable rest (unusual) credits nothing.
                        Some(_) => {}
                    }
                }
                // Other pattern shapes over an array scrutinee are not modeled.
                _ => {}
            }
        }

        // Find the smallest length that is neither an exactly-covered fixed
        // length nor at/above the rest threshold.
        let mut len = 0usize;
        loop {
            if covered_lengths.contains(&len) {
                len += 1;
                continue;
            }
            if rest_min.is_some_and(|k| len >= k) {
                // Everything from here up is covered by a rest pattern.
                return;
            }
            break;
        }

        let missing = if len == 0 {
            "the empty array".to_string()
        } else if rest_min.is_none() && covered_lengths.iter().all(|&c| c < len) {
            format!("arrays of length {len} or longer")
        } else {
            format!("arrays of length {len}")
        };
        self.errors.push(TypeError::NonExhaustiveArrayMatch {
            missing,
            span: match_span,
        });
    }

    /// Recursive core of exhaustiveness. Given the scrutinee type and the
    /// patterns matched against it, verify the tagged-union variant set is
    /// covered, then recurse into the payload of any variant covered ONLY by
    /// a nested constructor pattern. `match r { Ok(Some(x)) => .., Err(e) =>
    /// .. }` over `Result<Option<T>, E>` reaches `Ok` via `Some(x)` alone, so
    /// the payload `Option<T>` is checked too and `Ok(None)` is reported
    /// missing. Recursion is arbitrary-depth and reuses the same payload
    /// resolution for module-local unions and the prelude `Result`/`Option`.
    fn check_patterns_exhaustive(
        &mut self,
        scrutinee_ty: &Ty,
        patterns: &[&Pattern],
        match_span: glyph_ast::Span,
    ) {
        // Resolve the scrutinee to a tagged union (user-defined or a
        // prelude Result/Option) and its required variant set.
        let Some((type_name, variants)) = self.required_variants(scrutinee_ty) else {
            return;
        };

        // `covered`: variants whose whole payload is matched (a binding,
        // wildcard, object/array destructure, or no-payload form) — no deeper
        // check needed. `nested`: variants covered ONLY by a constructor
        // sub-pattern, mapped to those sub-patterns for a recursive check.
        let mut covered: HashSet<Ident> = HashSet::new();
        let mut nested: HashMap<Ident, Vec<&Pattern>> = HashMap::new();
        let mut has_catch_all = false;
        for pat in patterns {
            match pat {
                Pattern::Wildcard { .. } | Pattern::Else { .. } => {
                    has_catch_all = true;
                }
                Pattern::Constructor { path, args, .. } if !path.is_empty() => {
                    // Take the LAST segment as the variant name. Bare
                    // `Loading` → ["Loading"] → "Loading". Qualified
                    // `Feed.Loading` → ["Feed", "Loading"] → "Loading".
                    let variant = path.last().unwrap();
                    if !variants.iter().any(|v| v == variant) {
                        continue;
                    }
                    match args.as_slice() {
                        // A single payload sub-pattern is collected for a
                        // recursive check. Whether it actually covers the
                        // payload (a binding `Ok(x)`) or only part of it (a
                        // nested variant `Ok(Some(x))`, or the no-arg variant
                        // `Ok(None)` which parses as an ident) is decided by
                        // the recursion, which knows the payload's variants.
                        [sub] => {
                            nested.entry(variant.clone()).or_default().push(sub);
                        }
                        // No-arg (`fs.ErrorKind.NotFound`) or multi-arg
                        // payloads fully cover the variant at this level.
                        _ => {
                            covered.insert(variant.clone());
                        }
                    }
                }
                Pattern::Ident { name, .. } => {
                    // See the function-level docstring for the
                    // typo-vs-binding trade-off. If `name` matches a
                    // known variant, cover it; otherwise treat as a
                    // binding (universal match).
                    if variants.iter().any(|v| v == name) {
                        covered.insert(name.clone());
                    } else {
                        has_catch_all = true;
                    }
                }
                Pattern::IsType { ty, .. } => {
                    // `is TypeName` (D9) guard. The inner TypeExpr is
                    // typically a `Path` — extract the last segment as
                    // the variant name when possible.
                    if let TypeExpr::Path { segments, .. } = ty {
                        if let Some(name) = segments.last() {
                            if variants.iter().any(|v| v == name) {
                                covered.insert(name.clone());
                                continue;
                            }
                        }
                    }
                    // Non-Path TypeExpr (e.g., `is fn(x) -> y`) or a
                    // path that doesn't name a variant of this union —
                    // conservative: skip without marking catch-all.
                }
                // Top-level literal/object/array patterns over a union
                // scrutinee are not modeled. Conservative assumption: skip —
                // don't flag false-positive missing variants.
                _ => {}
            }
        }

        if has_catch_all {
            return;
        }

        // A variant covered by a binding/wildcard wins over any nested arms.
        // Recurse into the rest; collect variants no arm mentions at all, in
        // declaration order so the diagnostic is reproducible.
        let mut missing: Vec<&Ident> = Vec::new();
        for v in &variants {
            if covered.contains(v) {
                continue;
            }
            match nested.get(v) {
                Some(subs) => {
                    // The variant IS present (a `V(...)` arm exists); recurse
                    // into its payload to check the nested patterns. A payload
                    // that isn't a tagged union makes `required_variants`
                    // return None and the recursion a no-op.
                    if let Some(payload_ty) = self.variant_payload(scrutinee_ty, v) {
                        self.check_patterns_exhaustive(&payload_ty, subs, match_span);
                    }
                }
                None => missing.push(v),
            }
        }

        if missing.is_empty() {
            return;
        }

        let missing_str = missing
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        self.errors.push(TypeError::NonExhaustiveMatch {
            type_name,
            missing: missing_str,
            span: match_span,
        });
    }

    /// If `ty` is a `Ty::Named` pointing at a module-local tagged-union
    /// `type X = | A | B | ...` declaration, return that declaration. The
    /// shared resolution chain behind `named_union_variants` and
    /// `union_variant_payload`.
    fn resolve_named_union(&self, ty: &Ty) -> Option<&glyph_ast::TypeDecl> {
        let Ty::Named { symbol, .. } = ty else { return None };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        let SymbolKind::Type { decl_idx } = sym.kind else { return None };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        matches!(&td.body, TypeExpr::Union { .. }).then_some(td)
    }

    /// If `ty` is a module-local tagged union, return the type's name and
    /// the ordered list of variant names. Otherwise None.
    fn named_union_variants(&self, ty: &Ty) -> Option<(String, Vec<Ident>)> {
        let td = self.resolve_named_union(ty)?;
        let TypeExpr::Union { variants, .. } = &td.body else { return None };
        let names: Vec<Ident> = variants.iter().map(|v| v.name.clone()).collect();
        Some((td.name.to_string(), names))
    }

    /// The exhaustiveness target for `ty`: a module-local tagged union, or
    /// a prelude `Result` (`Ok`/`Err`) / `Option` (`Some`/`None`). Returns
    /// the display name and the required variant names. Otherwise None.
    fn required_variants(&self, ty: &Ty) -> Option<(String, Vec<Ident>)> {
        if let Some(found) = self.named_union_variants(ty) {
            return Some(found);
        }
        match self.prelude_union(ty)? {
            ("Result", _) => Some(("Result".to_string(), vec!["Ok".into(), "Err".into()])),
            ("Option", _) => Some(("Option".to_string(), vec!["Some".into(), "None".into()])),
            _ => None,
        }
    }

    /// If `ty` is an application of the prelude `Result`/`Option` type,
    /// return its display name and type arguments. Prelude unions appear as
    /// `Ty::App` over the prelude symbol (e.g. `Result<T, E>`). The prelude
    /// and module symbol tables both number ids from 0, so an id match alone
    /// could collide with an unrelated module symbol; require BOTH the
    /// lexical name on the base path AND the prelude id. The shared detector
    /// behind `required_variants` and `variant_payload`.
    fn prelude_union<'a>(&self, ty: &'a Ty) -> Option<(&'static str, &'a [Ty])> {
        let Ty::App { base, args } = ty else { return None };
        let Ty::Named { symbol, path } = base.as_ref() else { return None };
        let name = path.last()?.as_ref();
        let is_prelude = |n: &str| self.lowerer.prelude.lookup(n) == Some(SymbolId(symbol.0));
        match name {
            "Result" if is_prelude("Result") => Some(("Result", args.as_slice())),
            "Option" if is_prelude("Option") => Some(("Option", args.as_slice())),
            _ => None,
        }
    }

    // ----- day-17: match-arm payload binding typing -----

    /// Type a match arm's payload binding from the matched variant. For a
    /// `Variant(x)` pattern over a module-local tagged union, bind `x` to
    /// the variant's payload type so references to `x` in the arm body
    /// resolve concretely (via the resolver's `Local` def-site key).
    ///
    /// Two payload shapes are typed:
    /// - whole payload bound to one identifier (`Full(n)` → `n: payload`);
    /// - a record payload destructured by an object pattern
    ///   (`NetworkError({ url, status })` → each field bound to its record
    ///   field type).
    ///
    /// Deferred: nested constructor payloads and array payloads. Prelude
    /// unions (`Ok(x)`, `Some(x)`) aren't handled either: their scrutinee
    /// lowers to `Ty::App`, not the `Ty::Named` `union_variant_payload`
    /// requires.
    fn bind_arm_payloads(&mut self, scrutinee_ty: &Ty, pattern: &Pattern) {
        let Pattern::Constructor { path, args, .. } = pattern else {
            return;
        };
        let Some(variant_name) = path.last() else {
            return;
        };
        let Some(payload_ty) = self.union_variant_payload(scrutinee_ty, variant_name) else {
            return;
        };
        match args.as_slice() {
            // `Full(n)` — the whole payload binds to one name.
            [Pattern::Ident { span, .. }] => {
                self.local_tys.insert(span.start, payload_ty);
            }
            // `NetworkError({ url, status })` — destructure a record payload.
            [Pattern::Object { fields, .. }] => {
                self.bind_object_pattern_fields(fields, &payload_ty);
            }
            _ => {}
        }
    }

    /// Bind each field of an object pattern to its type from the payload
    /// record. The resolver binds `{ name }` and `{ name: alias }` at the
    /// field's span, so the type is keyed by `field.span.start`. A field
    /// the record doesn't declare is left untyped (a separate
    /// unknown-field diagnostic is the bidirectional checker's job).
    fn bind_object_pattern_fields(&mut self, fields: &[ObjectPatternField], payload_ty: &Ty) {
        let Ty::Record { fields: rec_fields } = payload_ty else {
            return;
        };
        for pf in fields {
            if let Some(rf) = rec_fields.iter().find(|rf| rf.name == pf.key) {
                self.local_tys.insert(pf.span.start, rf.ty.clone());
            }
        }
    }

    /// The lowered payload type of `variant_name` in the module-local
    /// tagged union `ty` refers to, or None if `ty` isn't such a union, the
    /// variant doesn't exist, or it carries no payload.
    fn union_variant_payload(&self, ty: &Ty, variant_name: &Ident) -> Option<Ty> {
        let td = self.resolve_named_union(ty)?;
        let TypeExpr::Union { variants, .. } = &td.body else { return None };
        let variant = variants.iter().find(|v| &v.name == variant_name)?;
        let payload_te = variant.payload.as_ref()?;
        Some(self.lowerer.lower(payload_te))
    }

    /// The payload type of `variant` in the tagged union `ty`, for both
    /// module-local unions (via `union_variant_payload`) and the prelude
    /// `Result`/`Option` — whose payloads are the `Ty::App` type arguments
    /// (`Ok` → arg 0, `Err` → arg 1, `Some` → arg 0). Drives nested
    /// exhaustiveness recursion. None when there is no such payload-carrying
    /// variant. A generic module-local union applied via `Ty::App` is not
    /// substituted here (conservative: no recursion), since `resolve_named_union`
    /// requires a bare `Ty::Named`.
    fn variant_payload(&self, ty: &Ty, variant: &Ident) -> Option<Ty> {
        if let Some(p) = self.union_variant_payload(ty, variant) {
            return Some(p);
        }
        match (self.prelude_union(ty)?, variant.as_ref()) {
            (("Result", args), "Ok") => args.first().cloned(),
            (("Result", args), "Err") => args.get(1).cloned(),
            (("Option", args), "Some") => args.first().cloned(),
            _ => None,
        }
    }
}

/// An array-pattern element/rest that matches any value of its position: a
/// binding or `_`. Used by array exhaustiveness — only irrefutable elements
/// let a pattern fully cover its length(s).
fn is_irrefutable_pattern(p: &Pattern) -> bool {
    matches!(p, Pattern::Ident { .. } | Pattern::Wildcard { .. })
}

// ----- day-21: assignability (conservative, primitives only) -----

/// True only when `found` is *provably* not assignable to `expected`. The
/// day-21 relation judges primitive-vs-primitive pairs (`string`, `number`,
/// `bool`, `void`), which are unambiguous, and returns false for every other
/// pairing — `Unknown`, `UnknownTop`, `Param`, named, generic, record, and
/// function types — so the check never produces a false positive on a type
/// it has not learned to compare. Assignability over those is a later day.
fn definitely_incompatible(found: &Ty, expected: &Ty) -> bool {
    matches!(
        (found, expected),
        (Ty::Prim(a), Ty::Prim(b)) if a != b
    )
}


// ----- day-20: generic instantiation (a minimal unifier) -----

/// Infer type-parameter bindings by structurally matching a declared
/// parameter type against the concrete argument type. `fn id<T>(x: T)`
/// called with `5: number` binds `T → number`; `xs: Array<T>` against
/// `Array<number>` binds the same. The first binding for a name wins, and
/// `Unknown` arguments bind nothing (leaving the parameter open rather than
/// pinning it to `Unknown`). This is not full unification: it only walks
/// `Param` positions and zips `App` arguments — enough for the common
/// generic call shapes.
fn collect_type_param_bindings(param: &Ty, arg: &Ty, out: &mut HashMap<Ident, Ty>) {
    match (param, arg) {
        (Ty::Param { name, .. }, concrete) if !concrete.is_unknown() => {
            out.entry(name.clone()).or_insert_with(|| concrete.clone());
        }
        (Ty::App { base: pbase, args: pargs }, Ty::App { base: abase, args: aargs }) => {
            collect_type_param_bindings(pbase, abase, out);
            for (p, a) in pargs.iter().zip(aargs.iter()) {
                collect_type_param_bindings(p, a, out);
            }
        }
        _ => {}
    }
}

/// Replace every `Ty::Param` named in `subst` with its bound type, walking
/// the type structurally. An empty substitution (the non-generic call case)
/// returns a clone unchanged, so this is a no-op for ordinary calls.
fn substitute_type_params(ty: &Ty, subst: &HashMap<Ident, Ty>) -> Ty {
    if subst.is_empty() {
        return ty.clone();
    }
    match ty {
        Ty::Param { name, .. } => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Ty::App { base, args } => Ty::App {
            base: Arc::new(substitute_type_params(base, subst)),
            args: args.iter().map(|a| substitute_type_params(a, subst)).collect(),
        },
        Ty::Fn { params, return_ty, is_async } => Ty::Fn {
            params: params
                .iter()
                .map(|p| FnParam {
                    name: p.name.clone(),
                    owned: p.owned,
                    ty: substitute_type_params(&p.ty, subst),
                })
                .collect(),
            return_ty: Arc::new(substitute_type_params(return_ty, subst)),
            is_async: *is_async,
        },
        Ty::Record { fields } => Ty::Record {
            fields: fields
                .iter()
                .map(|f| RecordField {
                    name: f.name.clone(),
                    ty: substitute_type_params(&f.ty, subst),
                    optional: f.optional,
                })
                .collect(),
        },
        Ty::Union { variants } => Ty::Union {
            variants: variants
                .iter()
                .map(|v| UnionVariant {
                    name: v.name.clone(),
                    payload: v.payload.as_ref().map(|p| substitute_type_params(p, subst)),
                })
                .collect(),
        },
        other => other.clone(),
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
        let (tm, _ty_errs) = assign_types(&m, &resolved, &prelude);
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

    /// Helper for day-14 exhaustiveness tests: run assign_types and
    /// return the collected `TypeError`s.
    fn ty_errors_of(src: &str) -> Vec<TypeError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        ty_errs
    }

    #[test]
    fn nested_missing_inner_variant_is_flagged() {
        // `Ok(Some(n))` covers Ok only through the `Some` arm, so the payload
        // `Option<number>` must also be exhaustive — `Ok(None)` is missing.
        let src = r#"module x
fn run(r: Result<Option<number>, string>) -> number {
  return match r {
    Ok(Some(n)) => n,
    Err(e) => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Option");
                assert!(missing.contains("None"), "missing: {missing}");
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn nested_all_inner_variants_covered_passes() {
        let src = r#"module x
fn run(r: Result<Option<number>, string>) -> number {
  return match r {
    Ok(Some(n)) => n,
    Ok(None) => 0,
    Err(e) => 1,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn nested_no_arg_variant_does_not_over_cover() {
        // `Ok(None)` must not be mistaken for a payload binding: the `Some`
        // arm of the inner `Option` is still missing.
        let src = r#"module x
fn run(r: Result<Option<number>, string>) -> number {
  return match r {
    Ok(None) => 0,
    Err(e) => 1,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Option");
                assert!(missing.contains("Some"), "missing: {missing}");
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn whole_variant_cover_wins_over_a_nested_arm() {
        // `Ok` (bare) fully covers the variant; a sibling `Ok(Some(y))` arm
        // also classifies it as nested. The whole-variant cover must win, so
        // no inner `Option` check runs and `Ok(None)` is not reported missing.
        let src = r#"module x
fn run(r: Result<Option<number>, string>) -> number {
  return match r {
    Ok => 0,
    Ok(Some(y)) => y,
    Err(e) => 1,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn binding_payload_does_not_trigger_nested_check() {
        // `Ok(opt)` binds the whole `Option` payload, so no inner check runs.
        let src = r#"module x
fn run(r: Result<Option<number>, string>) -> number {
  return match r {
    Ok(opt) => 0,
    Err(e) => 1,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn array_match_empty_and_rest_is_exhaustive() {
        let src = r#"module x
fn f(xs: Array<string>) -> number {
  return match xs {
    [] => 0,
    [head, ...rest] => 1,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn array_match_missing_empty_is_flagged() {
        let src = r#"module x
fn f(xs: Array<string>) -> number {
  return match xs {
    [head, ...rest] => 1,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveArrayMatch { missing, .. }] if missing.contains("empty")),
            "got {errs:?}"
        );
    }

    #[test]
    fn array_match_missing_long_arrays_is_flagged() {
        let src = r#"module x
fn f(xs: Array<string>) -> number {
  return match xs {
    [] => 0,
    [a] => 1,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveArrayMatch { missing, .. }] if missing.contains("length 2")),
            "got {errs:?}"
        );
    }

    #[test]
    fn array_match_with_literal_arms_still_needs_a_catch_all() {
        // Literal-element patterns do not cover their whole length; without an
        // irrefutable rest or catch-all, the empty array is uncovered.
        let src = r#"module x
fn f(xs: Array<string>) -> number {
  return match xs {
    ["help"] => 0,
    ["version"] => 1,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveArrayMatch { .. }]),
            "got {errs:?}"
        );
    }

    #[test]
    fn array_match_cli_idiom_is_exhaustive() {
        // The `04_cli_tool` shape: literal-first arms are not credited, but a
        // trailing binding-first rest arm `[other, ..._]` covers all non-empty
        // lengths, and `[]` covers the empty case.
        let src = r#"module x
fn f(argv: Array<string>) -> number {
  return match argv {
    [] => 0,
    ["help", ..._] => 1,
    ["add", ...rest] => 2,
    [other, ..._] => 3,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn array_match_with_catch_all_is_exhaustive() {
        let src = r#"module x
fn f(xs: Array<string>) -> number {
  return match xs {
    [] => 0,
    other => 1,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn exhaustive_match_on_tagged_union_passes() {
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loaded => 2,
    Failed => 3,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "exhaustive match should not error; got: {errs:?}"
        );
    }

    #[test]
    fn non_exhaustive_match_on_tagged_union_is_flagged() {
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loaded => 2,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Feed");
                assert!(
                    missing.contains("Failed"),
                    "missing list should mention Failed; got: {missing}"
                );
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn wildcard_arm_makes_match_exhaustive() {
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    _ => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "wildcard should cover; got: {errs:?}");
    }

    #[test]
    fn else_arm_makes_match_exhaustive() {
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    else => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "else should cover; got: {errs:?}");
    }

    #[test]
    fn missing_variants_listed_in_declaration_order() {
        // Reproducibility: the diagnostic lists missing variants in the
        // order they appear in the type declaration, not arm-walk order.
        let src = r#"module x
type Tri = | A | B | C
fn x(t: Tri) -> number {
  return match t {
    B => 2,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        let TypeError::NonExhaustiveMatch { missing, .. } = &errs[0] else {
            panic!("expected NonExhaustiveMatch, got {:?}", errs[0]);
        };
        // `A` appears before `C` in the type decl, so the diagnostic
        // mentions them in that order.
        let a_pos = missing.find("A").expect("A in missing");
        let c_pos = missing.find("C").expect("C in missing");
        assert!(a_pos < c_pos, "missing should be in decl order: {missing}");
    }

    #[test]
    fn is_type_arms_cover_variants() {
        // Day-14 review fix #1: `is TypeName` guard patterns previously
        // fell through to the wildcard arm, producing a false-positive
        // non-exhaustive diagnostic on syntactically-valid exhaustive
        // code. After the fix, `is Loading | is Loaded | is Failed`
        // covers the same set as bare variant arms.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    is Loading => 1,
    is Loaded => 2,
    is Failed => 3,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`is Variant` arms should cover; got: {errs:?}"
        );
    }

    #[test]
    fn typo_ident_is_treated_as_binding_and_passes_exhaustiveness() {
        // Day-14 review finding #2: a typo'd bare variant name
        // (`Loadign` vs `Loading`) is treated as a binding, which acts
        // as a catch-all. The typechecker silently accepts the match.
        // This test LOCKS that behavior so a future change to the
        // ident-vs-variant disambiguation rule is deliberate (it will
        // need to update this test along with the trade-off doc).
        //
        // Fixing this properly requires scrutinee-aware resolver
        // disambiguation — when an ident's name lexically matches a
        // hoisted Variant of the scrutinee's type, the resolver could
        // warn or error. Out of day-14 scope.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loaded => 2,
    Loadign => 999,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "current behavior: typo'd ident binds and acts as catch-all; \
             see the function-level docstring on `check_match_exhaustiveness`. \
             got: {errs:?}"
        );
    }

    #[test]
    fn non_tagged_union_scrutinee_is_not_checked() {
        // Number scrutinees aren't tagged unions; day-14 scope skips
        // them. Verify no false-positive diagnostic.
        let src = r#"module x
fn main(n: number) -> number {
  return match n {
    0 => 1,
    1 => 2,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "non-union scrutinee should not be flagged; got: {errs:?}");
    }

    // ----- day-15: `?` operator typing rule -----

    // The `?` operand is a parameter so it resolves cleanly (the
    // `ty_errors_of` helper asserts the resolve pass is error-free). The
    // operand's *type* doesn't matter to the day-15 check — only the
    // enclosing function's return type does.

    #[test]
    fn question_in_result_returning_fn_passes() {
        let src = r#"module x
fn read(r: Result<string, string>) -> Result<string, string> {
  let data = r?
  return Ok(data)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`?` inside a Result-returning fn should not error; got: {errs:?}"
        );
    }

    #[test]
    fn question_in_non_result_fn_is_flagged() {
        let src = r#"module x
fn read(r: Result<string, string>) -> number {
  let data = r?
  return 1
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(errs[0], TypeError::QuestionOutsideResultFn { .. }),
            "expected QuestionOutsideResultFn, got {:?}",
            errs[0]
        );
    }

    #[test]
    fn question_in_void_returning_fn_is_flagged() {
        // Explicit `-> void` is a concrete non-Result return; `?` is illegal.
        let src = r#"module x
fn run(r: Result<string, string>) -> void {
  let data = r?
  return void
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(matches!(errs[0], TypeError::QuestionOutsideResultFn { .. }));
    }

    #[test]
    fn question_in_unannotated_fn_is_permissive() {
        // D4 makes the return annotation optional. Without one we can't
        // prove the function doesn't return Result, so `?` is not flagged.
        let src = r#"module x
fn read(r: Result<string, string>) {
  let data = r?
  return data
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`?` in an unannotated fn must not produce a false positive; got: {errs:?}"
        );
    }

    #[test]
    fn question_in_const_initializer_is_flagged() {
        // A `const` initializer has no enclosing callable, so the `?`
        // cannot propagate anywhere — always an error.
        let src = r#"module x
const FALLIBLE: Result<number, string> = Ok(1)
const VALUE = FALLIBLE?
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(matches!(errs[0], TypeError::QuestionOutsideResultFn { .. }));
    }

    #[test]
    fn question_checked_against_innermost_lambda() {
        // The `?` sits inside a lambda that returns `number`, NOT the
        // outer Result-returning fn. The innermost frame governs, so it is
        // flagged even though an enclosing fn returns Result.
        let src = r#"module x
fn outer(r: Result<string, string>) -> Result<number, string> {
  let f = fn() -> number { r? }
  return Ok(1)
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(matches!(errs[0], TypeError::QuestionOutsideResultFn { .. }));
    }

    #[test]
    fn question_passes_when_result_is_imported() {
        // Regression: the four example files `import std/result { Result }`,
        // so the return type's `Result` resolves to an `ImportNamed` symbol
        // and lowers to `Ty::App { base: Unknown }`. The naive "base is the
        // prelude Result symbol" check produced a false positive on every
        // `?` in those files. `type_expr_is_result` recognizes the imported
        // name syntactically and keeps the `?` legal.
        let src = r#"module x
import std/result { Result, Ok, Err }
async fn fetch(r: Result<string, string>) -> Result<string, string> {
  let v = r?
  return Ok(v)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`?` with an imported Result return type must not be flagged; got: {errs:?}"
        );
    }

    #[test]
    fn question_in_result_returning_lambda_passes() {
        // Inverse of the previous test: a Result-returning lambda nested in
        // a non-Result fn. The innermost frame (the lambda) permits `?`.
        let src = r#"module x
fn outer(r: Result<string, string>) -> number {
  let f = fn() -> Result<string, string> { r? }
  return 1
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`?` in a Result-returning lambda should pass; got: {errs:?}"
        );
    }

    // ----- day-16: synthesize Call types from the callee signature -----

    #[test]
    fn call_takes_callee_return_type() {
        // `helper()` synthesizes `number` from `fn helper() -> number`.
        let (m, _, tm) = type_map_of(
            "module x\nfn helper() -> number { return 1 }\nfn main() { let x = helper() }\n",
        );
        // The `let x = ...` is the first stmt of the SECOND decl (`main`).
        let main = match &m.items[1] {
            Decl::Fn(f) => f,
            _ => panic!("second decl is not a Fn"),
        };
        let call_span = match &main.body.stmts[0] {
            Stmt::Let(l) => l.value.span(),
            _ => panic!("first stmt is not a Let"),
        };
        assert!(
            matches!(tm.get(call_span), Ty::Prim(Primitive::Number)),
            "call should take the callee's return type; got {:?}",
            tm.get(call_span)
        );
    }

    #[test]
    fn match_on_call_returning_union_checks_exhaustiveness() {
        // Day-16: the scrutinee is a call, not a bound name. Synthesizing
        // the call's return type (`Feed`) lets the day-14 exhaustiveness
        // check fire — previously the call typed as Unknown and was skipped.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn current() -> Feed { return Loading }
fn show() -> number {
  return match current() {
    Loading => 1,
    Loaded => 2,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Feed");
                assert!(missing.contains("Failed"), "missing: {missing}");
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn exhaustive_match_on_call_returning_union_passes() {
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn current() -> Feed { return Loading }
fn show() -> number {
  return match current() {
    Loading => 1,
    Loaded => 2,
    Failed => 3,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "exhaustive match on a call should pass; got: {errs:?}");
    }

    #[test]
    fn match_on_awaited_call_sees_through_to_union() {
        // `await current()` synthesizes the same type as `current()`, so
        // exhaustiveness still fires through the `await`.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
async fn current() -> Feed { return Loading }
async fn show() -> number {
  return match await current() {
    Loading => 1,
    Loaded => 2,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(&errs[0], TypeError::NonExhaustiveMatch { type_name, .. } if type_name == "Feed"),
            "expected NonExhaustiveMatch on Feed; got {:?}",
            errs[0]
        );
    }

    #[test]
    fn match_on_call_returning_prelude_result_covering_both_arms_passes() {
        // A call returning a prelude `Result` types as `Ty::App` over the
        // prelude Result symbol. Day-19 checks it for exhaustiveness; this
        // match covers both `Ok` and `Err`, so it passes.
        let src = r#"module x
fn current() -> Result<number, string> { return Ok(1) }
fn show() -> number {
  return match current() {
    Ok(n) => n,
    Err(_) => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "exhaustive prelude-Result match must pass; got: {errs:?}");
    }

    // ----- day-17: match-arm payload binding typing -----

    /// Navigate to the `arm_idx`-th match arm's body expression span,
    /// assuming `decl_idx` is a fn whose first statement is
    /// `return match ... { ... }`.
    fn match_arm_body_expr_span(
        m: &Module,
        decl_idx: usize,
        arm_idx: usize,
    ) -> glyph_ast::Span {
        let f = match &m.items[decl_idx] {
            Decl::Fn(f) => f,
            _ => panic!("decl {decl_idx} is not a Fn"),
        };
        let ret = match &f.body.stmts[0] {
            Stmt::Return(r) => r,
            _ => panic!("first stmt is not a return"),
        };
        let value = ret.value.as_ref().expect("return has a value");
        let arms = match value {
            Expr::Match { arms, .. } => arms,
            _ => panic!("return value is not a match"),
        };
        match &arms[arm_idx].body {
            MatchArmBody::Expr(e) => e.span(),
            _ => panic!("arm {arm_idx} body is not an expr"),
        }
    }

    #[test]
    fn primitive_payload_binding_is_typed() {
        // `Full(n) => n` binds `n` to the variant's `number` payload, so
        // the body reference to `n` types as number.
        let src = r#"module x
type Box = | Empty | Full(number)
fn get(b: Box) -> number {
  return match b {
    Empty => 0,
    Full(n) => n,
  }
}
"#;
        let (m, _, tm) = type_map_of(src);
        let body_span = match_arm_body_expr_span(&m, 1, 1);
        assert!(
            matches!(tm.get(body_span), Ty::Prim(Primitive::Number)),
            "Full(n) body should type as number; got {:?}",
            tm.get(body_span)
        );
    }

    #[test]
    fn record_payload_binding_is_typed() {
        // `Data(p) => p` binds `p` to the variant's `Payload` record type.
        let src = r#"module x
type Payload = { size: number }
type Msg = | Ping | Data(Payload)
fn handle(m: Msg, fallback: Payload) -> Payload {
  return match m {
    Ping => fallback,
    Data(p) => p,
  }
}
"#;
        let (m, _, tm) = type_map_of(src);
        let body_span = match_arm_body_expr_span(&m, 2, 1);
        assert!(
            matches!(tm.get(body_span), Ty::Named { .. }),
            "Data(p) body should type as the Payload named type; got {:?}",
            tm.get(body_span)
        );
    }

    #[test]
    fn no_payload_variant_binds_nothing() {
        // A bare-ident arm (`other`) over a union is a binding, not a
        // payload destructure. It must not pick up a phantom payload type;
        // the scrutinee type itself is the most we could say, and we don't
        // claim even that here — the binding stays Unknown.
        let src = r#"module x
type Box = | Empty | Full(number)
fn get(b: Box) -> number {
  return match b {
    Full(n) => n,
    other => 0,
  }
}
"#;
        let (m, _, tm) = type_map_of(src);
        // Arm 1 is `other => 0`; its body is the literal `0` (number), and
        // crucially the bind of `other` did not crash or mistype. Assert the
        // typed payload arm still works and the catch-all body is number.
        let payload_body = match_arm_body_expr_span(&m, 1, 0);
        assert!(matches!(tm.get(payload_body), Ty::Prim(Primitive::Number)));
        let catch_all_body = match_arm_body_expr_span(&m, 1, 1);
        assert!(matches!(tm.get(catch_all_body), Ty::Prim(Primitive::Number)));
    }

    // ----- day-18: object-pattern payload destructuring -----

    #[test]
    fn object_pattern_payload_string_field_typed() {
        // `Info({ text }) => text` binds `text` to the record payload's
        // `text: string` field. Mirrors example 04's `format_parse_error`.
        let src = r#"module x
type Log = | Info({ text: string }) | Code({ n: number })
fn render(l: Log) -> string {
  return match l {
    Info({ text }) => text,
    Code({ n }) => "x",
  }
}
"#;
        let (m, _, tm) = type_map_of(src);
        let body = match_arm_body_expr_span(&m, 1, 0);
        assert!(
            matches!(tm.get(body), Ty::Prim(Primitive::String)),
            "Info({{ text }}) body should type as string; got {:?}",
            tm.get(body)
        );
    }

    #[test]
    fn object_pattern_payload_number_field_typed() {
        // Same union, the other field: `Code({ n }) => n` binds `n: number`.
        let src = r#"module x
type Log = | Info({ text: string }) | Code({ n: number })
fn pick(l: Log) -> number {
  return match l {
    Code({ n }) => n,
    Info({ text }) => 0,
  }
}
"#;
        let (m, _, tm) = type_map_of(src);
        let body = match_arm_body_expr_span(&m, 1, 0);
        assert!(
            matches!(tm.get(body), Ty::Prim(Primitive::Number)),
            "Code({{ n }}) body should type as number; got {:?}",
            tm.get(body)
        );
    }

    #[test]
    fn aliased_object_pattern_field_typed() {
        // `Boom({ code: c }) => c` binds the alias `c` to the type of the
        // record's `code` field, not the alias name.
        let src = r#"module x
type E = | Boom({ code: number })
fn f(e: E) -> number {
  return match e {
    Boom({ code: c }) => c,
  }
}
"#;
        let (m, _, tm) = type_map_of(src);
        let body = match_arm_body_expr_span(&m, 1, 0);
        assert!(
            matches!(tm.get(body), Ty::Prim(Primitive::Number)),
            "aliased binding `c` should take the `code` field type; got {:?}",
            tm.get(body)
        );
    }

    // ----- day-19: exhaustiveness for prelude Result / Option -----

    #[test]
    fn non_exhaustive_prelude_result_match_is_flagged() {
        // `Result` resolves to the prelude; a match missing `Err` is flagged.
        let src = r#"module x
fn run(r: Result<number, string>) -> number {
  return match r {
    Ok(n) => n,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Result");
                assert!(missing.contains("Err"), "missing: {missing}");
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn non_exhaustive_imported_result_match_is_flagged() {
        // The example files `import std/result { Result }`, so the imported
        // name must be recognized too (it lowers to the prelude Named).
        let src = r#"module x
import std/result { Result, Ok, Err }
fn run(r: Result<number, string>) -> number {
  return match r {
    Err(_) => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(&errs[0], TypeError::NonExhaustiveMatch { type_name, missing, .. }
                if type_name == "Result" && missing.contains("Ok")),
            "expected missing Ok on Result; got {:?}",
            errs[0]
        );
    }

    #[test]
    fn exhaustive_prelude_result_passes() {
        let src = r#"module x
fn run(r: Result<number, string>) -> number {
  return match r {
    Ok(n) => n,
    Err(_) => 0,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn prelude_result_with_wildcard_passes() {
        // A wildcard covers the rest, so `Ok` alone + `_` is exhaustive.
        let src = r#"module x
fn run(r: Result<number, string>) -> number {
  return match r {
    Ok(n) => n,
    _ => 0,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn non_exhaustive_prelude_option_match_is_flagged() {
        let src = r#"module x
fn run(o: Option<number>) -> number {
  return match o {
    Some(n) => n,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(&errs[0], TypeError::NonExhaustiveMatch { type_name, missing, .. }
                if type_name == "Option" && missing.contains("None")),
            "expected missing None on Option; got {:?}",
            errs[0]
        );
    }

    #[test]
    fn exhaustive_prelude_option_passes() {
        let src = r#"module x
fn run(o: Option<number>) -> number {
  return match o {
    Some(n) => n,
    None => 0,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn generic_user_union_is_not_mistaken_for_prelude() {
        // A generic user union appears as `Ty::App { base: Named(user) }`,
        // the same shape as a prelude `Result`. The name guard in
        // `required_variants` must keep them distinct: this match covers
        // `Tree`'s own variants and must NOT be checked against `Ok`/`Err`
        // (nor flagged as missing prelude variants), even if the user
        // type's symbol id collides numerically with a prelude id.
        let src = r#"module x
type Tree<T> = | Leaf | Node(T)
fn size(t: Tree<number>) -> number {
  return match t {
    Leaf => 0,
    Node(n) => n,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "generic user union (App over a user Named) must not be treated as prelude Result; got: {errs:?}"
        );
    }

    #[test]
    fn result_match_with_nested_err_arms_is_exhaustive() {
        // Mirrors example 02: the outer Result variants are `Ok` and `Err`,
        // even though the `Err` arms carry nested user-variant patterns.
        // Only the top-level Result variant set is checked.
        let src = r#"module x
type E = | A | B
fn run(r: Result<number, E>) -> number {
  return match r {
    Ok(n) => n,
    Err(A) => 1,
    Err(B) => 2,
  }
}
"#;
        assert!(
            ty_errors_of(src).is_empty(),
            "Ok + (multiple Err arms) covers the Result variant set"
        );
    }

    // ----- day-20: generic instantiation -----

    /// The value span of the first `let` in the `decl_idx`-th decl.
    fn nth_decl_first_let_span(m: &Module, decl_idx: usize) -> glyph_ast::Span {
        let f = match &m.items[decl_idx] {
            Decl::Fn(f) => f,
            _ => panic!("decl {decl_idx} is not a Fn"),
        };
        match &f.body.stmts[0] {
            Stmt::Let(l) => l.value.span(),
            _ => panic!("first stmt is not a Let"),
        }
    }

    #[test]
    fn generic_identity_call_instantiates_return() {
        // `id(5)` infers `T = number` from the argument, so the call types
        // as `number` rather than the uninstantiated `Ty::Param`.
        let (m, _, tm) = type_map_of(
            "module x\nfn id<T>(x: T) -> T { return x }\nfn main() { let n = id(5) }\n",
        );
        let call = nth_decl_first_let_span(&m, 1);
        assert!(
            matches!(tm.get(call), Ty::Prim(Primitive::Number)),
            "id(5) should instantiate T = number; got {:?}",
            tm.get(call)
        );
    }

    #[test]
    fn generic_call_instantiates_through_container() {
        // `first(arr)` with `arr: Array<number>` against `xs: Array<T>`
        // binds `T = number` by zipping the `App` arguments.
        let (m, _, tm) = type_map_of(
            "module x\n\
             fn first<T>(xs: Array<T>) -> T { return xs[0] }\n\
             fn main(arr: Array<number>) { let x = first(arr) }\n",
        );
        let call = nth_decl_first_let_span(&m, 1);
        assert!(
            matches!(tm.get(call), Ty::Prim(Primitive::Number)),
            "first(arr: Array<number>) should instantiate T = number; got {:?}",
            tm.get(call)
        );
    }

    #[test]
    fn non_generic_call_return_is_unchanged() {
        // Regression: a non-generic call still takes its concrete return
        // type; the empty substitution is a no-op.
        let (m, _, tm) = type_map_of(
            "module x\nfn area(w: number, h: number) -> number { return w }\nfn main() { let a = area(2, 3) }\n",
        );
        let call = nth_decl_first_let_span(&m, 1);
        assert!(matches!(tm.get(call), Ty::Prim(Primitive::Number)));
    }

    #[test]
    fn generic_call_with_unknown_argument_leaves_param_open() {
        // When the argument type is Unknown nothing is bound, so the return
        // stays the open `Ty::Param` (no worse than before instantiation,
        // and not falsely pinned to Unknown). Here `pick`'s argument is a
        // call through a member access, which types as Unknown.
        let (m, _, tm) = type_map_of(
            "module x\n\
             fn pick<T>(x: T) -> T { return x }\n\
             fn main(obj: number) { let y = pick(obj.missing()) }\n",
        );
        let call = nth_decl_first_let_span(&m, 1);
        assert!(
            matches!(tm.get(call), Ty::Param { .. }),
            "unknown arg should leave T open as Ty::Param; got {:?}",
            tm.get(call)
        );
    }

    #[test]
    fn substitute_is_identity_without_bindings() {
        let subst = HashMap::new();
        let t = Ty::App {
            base: Arc::new(Ty::Param {
                name: "T".into(),
                owner: crate::ty::ParamOwner::Unresolved,
            }),
            args: vec![Ty::Prim(Primitive::Number)],
        };
        assert_eq!(substitute_type_params(&t, &subst), t);
    }

    // ----- day-21: return-type mismatch -----

    #[test]
    fn return_string_in_number_fn_is_flagged() {
        let errs = ty_errors_of("module x\nfn f() -> number { return \"hi\" }\n");
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::TypeMismatch { expected, found, .. } => {
                assert_eq!(expected, "number");
                assert_eq!(found, "string");
            }
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn return_number_in_string_fn_is_flagged() {
        let errs = ty_errors_of("module x\nfn f() -> string { return 5 }\n");
        assert!(matches!(
            errs.as_slice(),
            [TypeError::TypeMismatch { expected, found, .. }]
                if expected == "string" && found == "number"
        ), "errs: {errs:?}");
    }

    #[test]
    fn return_number_in_void_fn_is_flagged() {
        let errs = ty_errors_of("module x\nfn f() -> void { return 5 }\n");
        assert!(matches!(
            errs.as_slice(),
            [TypeError::TypeMismatch { expected, .. }] if expected == "void"
        ), "errs: {errs:?}");
    }

    #[test]
    fn matching_primitive_return_passes() {
        assert!(ty_errors_of("module x\nfn f() -> number { return 5 }\n").is_empty());
    }

    #[test]
    fn return_unknown_typed_value_is_not_flagged() {
        // `x.length` is a member access, which types as Unknown. A mismatch
        // can't be proven, so nothing is flagged.
        let src = "module x\nfn f(x: string) -> number { return x.length }\n";
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn return_in_unannotated_fn_is_not_flagged() {
        // No return annotation (legal under D4) => expected Unknown => the
        // check stays silent regardless of the value's type.
        assert!(ty_errors_of("module x\nfn f() { return 5 }\n").is_empty());
    }

    #[test]
    fn return_primitive_against_named_type_is_not_flagged() {
        // Conservative boundary: a primitive value against a named return
        // type is not (yet) judged — assignability over named types is a
        // later day. This locks the documented scope so a future change is
        // a deliberate one.
        let src = "module x\ntype U = { x: number }\nfn f() -> U { return 5 }\n";
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn return_mismatch_uses_innermost_lambda_return_type() {
        // The inner lambda returns `number` but yields `"x"` (string) — one
        // mismatch. The outer fn returns `string` and yields `"y"` — fine.
        let src = r#"module x
fn outer() -> string {
  let f = fn() -> number { return "x" }
  return "y"
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::TypeMismatch { expected, found, .. }]
                if expected == "number" && found == "string"
        ), "errs: {errs:?}");
    }
}
