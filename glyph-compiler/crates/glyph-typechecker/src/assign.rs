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
    ArrayElem, Block, Decl, Expr, Ident, InterfaceMember, JsxAttr, JsxChild, JsxElement,
    LiteralPattern, MatchArm, MatchArmBody, Module, ObjectField, ObjectPatternField, Param, Pattern,
    PostfixOp, Span, Stmt, TemplatePart, TypeExpr,
};
use glyph_resolver::{Prelude, ResolvedModule, ResolvedRef, SymbolId, SymbolKind};

use crate::lower::Lowerer;
use crate::ty::{
    ty_display, FnParam, ImportedTypeDecl, ModuleKey, ParamOwner, Primitive, RecordField,
    SymbolRef, Ty, UnionRef, UnionVariant,
};
use crate::type_map::TypeMap;
use crate::{DiagnosticUnion, TypeError};

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
    /// Whether the callable was declared `async` (for the `await` rule,
    /// E0222). A `component` is never async.
    is_async: bool,
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

    /// Cross-module union resolution: for the project module at `module_path`,
    /// find the tagged union that declares `variant_name` and return its
    /// `(type name, variant names)`. `None` with no cross-module context (db-less
    /// callers) or when the module is not a project sibling / has no such union.
    ///
    /// This is what lets a `match` on an *imported* union be checked for
    /// exhaustiveness: an imported union's type lowers to `Ty::Unknown` in a
    /// consuming module (its decl lives elsewhere), so the local variant-set
    /// resolution finds nothing; this reaches across to the source module.
    fn imported_union_of_variant(
        &self,
        _module_path: &str,
        _variant_name: &str,
    ) -> Option<(String, Vec<Ident>)> {
        None
    }

    /// Cross-module string-literal-union resolution: for the project module at
    /// `module_path`, if `type_name` is declared as `type X = "a" | "b"`, return
    /// its literal set. `None` with no cross-module context (db-less callers) or
    /// when the module is not a project sibling / declares no such type.
    ///
    /// The D30 counterpart of `imported_union_of_variant`. Without it an
    /// imported string-literal union lowers to `Ty::Unknown` in the consuming
    /// module, so an exhaustive `match` over it is reported as needing an
    /// `else` — the exact catch-all that destroys the guarantee D30 sells.
    fn imported_string_literal_union(
        &self,
        _module_path: &str,
        _type_name: &str,
    ) -> Option<Vec<String>> {
        None
    }

    /// The general cross-module type query: for the project module at
    /// `module_path`, return the `type <type_name> = ...` declaration lowered on
    /// the *source* side, so its body names types that resolve against the
    /// declaring module rather than the consumer. `None` with no cross-module
    /// context (db-less callers), for a module that is not a project sibling
    /// (`std/fs`), or for a name that module does not declare as a `type`.
    ///
    /// This is what gives an imported record a field set, so `s.rowz` is an
    /// `UnknownField` and `for i, r in s.rows` lowers as an array loop. The two
    /// per-shape queries above (`imported_union_of_variant`,
    /// `imported_string_literal_union`) answer questions this one could also
    /// answer; folding them in is the natural follow-up, deliberately not done
    /// in the same change that introduces this one.
    ///
    /// Answered by `glyph_db::exported_type`, which lowers the declaration on
    /// the source side and wraps it in an `ExportedTypeDecl`. One declaration,
    /// two names: `exported_type` / `ExportedTypeDecl` is the producing side,
    /// `imported_type_decl` / `ImportedTypeDecl` the consuming one.
    fn imported_type_decl(
        &self,
        _module_path: &str,
        _type_name: &str,
    ) -> Option<ImportedTypeDecl> {
        None
    }

    /// Cross-module function-signature resolution: for the project module at
    /// `module_path`, return the lowered `Ty::Fn` of the `pub fn` named
    /// `fn_name`, as the *declaring* module's signature renders on the export
    /// view (so a return type naming a sibling `type` comes back as
    /// `Ty::Imported` rather than a `Ty::Named` carrying a foreign symbol id).
    /// `None` with no cross-module context (db-less callers), for a module
    /// that is not a project sibling (`std/*`), or for a name that module does
    /// not declare as a `fn`.
    ///
    /// Without this, a call into another module (`a.make()`, or `make()`
    /// through a named import) has no signature at all: the call's own type
    /// stays `Ty::Unknown` regardless of what the callee actually returns, so
    /// an inferred `let` over the result loses field checking entirely and any
    /// mistake surfaces only once the emitted TS reaches `tsc` — a typo'd
    /// field degrades from `E0210` (names the type and field) to a bare
    /// `TS2339` pinned to the whole statement.
    ///
    /// Answered by `glyph_db::exported_fn`, the callable counterpart of
    /// `exported_type`.
    fn imported_fn_decl(&self, _module_path: &str, _fn_name: &str) -> Option<Ty> {
        None
    }
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
    let (tm, errors, _coverage) =
        assign_types_with_coverage(module, resolved, prelude, decl_ty_resolver);
    (tm, errors)
}

/// Same as `assign_types_with_resolver`, and additionally the match-coverage
/// relation the exhaustiveness dispatch filled while it ran.
///
/// A side channel, not a second analysis. Every edge is written where the
/// existing ordered dispatch in `check_match_exhaustiveness` already knew
/// something, so there is nothing to keep in step. A query that re-derived
/// coverage by walking the arms again would have to reproduce that dispatch
/// (the literal-arm recovery for `bool` and `number`, and the union the
/// `Expr::Match` handler writes back into the type map before any of it runs)
/// and would be a third copy of the logic the extraction below exists to stop
/// duplicating.
pub fn assign_types_with_coverage(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
    decl_ty_resolver: &dyn DeclTyResolver,
) -> (TypeMap, Vec<TypeError>, FileMatchCoverage) {
    let (tm, errors, coverage, _fields) =
        assign_types_with_relations(module, resolved, prelude, decl_ty_resolver);
    (tm, errors, coverage)
}

/// Same as `assign_types_with_coverage`, and additionally the field-use
/// relation the member-access check filled while it ran.
///
/// The second side channel, written the same way and for the same reason. Every
/// edge is recorded where `walk_expr`'s `Expr::Member` arm already resolved the
/// object's field set, so there is nothing to keep in step: a query that walked
/// the members again would have to reproduce the whole `record_fields_of`
/// dispatch (structural records, local `type` aliases, structural interfaces,
/// the stdlib table, imported declarations, generic applications) and would be a
/// second copy of it that drifts.
pub fn assign_types_with_relations(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
    decl_ty_resolver: &dyn DeclTyResolver,
) -> (TypeMap, Vec<TypeError>, FileMatchCoverage, FileFieldUses) {
    let mut tm = TypeMap::new();
    let mut errors: Vec<TypeError> = Vec::new();
    let mut coverage = FileMatchCoverage::default();
    let mut field_uses = FileFieldUses::default();
    {
        let mut assigner = Assigner {
            module,
            lowerer: Lowerer::with_imports(resolved, prelude, decl_ty_resolver),
            resolved,
            tm: &mut tm,
            errors: &mut errors,
            coverage: &mut coverage,
            field_uses: &mut field_uses,
            assign_target: None,
            decl_ty_resolver,
            return_stack: Vec::new(),
            local_tys: HashMap::new(),
        };
        for decl in &module.items {
            assigner.check_annotations(decl);
            assigner.declare_record_fields(decl);
            assigner.walk_decl(decl);
        }
    }
    // D25: a second pass over the completed `TypeMap`. `owned` single-
    // consumption analysis reads each call site's callee `Ty::Fn` (with its
    // per-parameter `owned` flags), so it must run after assignment fills the
    // map rather than interleaved with it.
    errors.extend(crate::owned::check_owned(module, resolved, prelude, &tm));
    errors.extend(crate::concurrency::check_await_straddle(module));
    (tm, errors, coverage, field_uses)
}

// ============================================================================
// Match coverage: the side channel the exhaustiveness dispatch fills
// ============================================================================

/// Where a match site's scrutinee union is declared, as far as one file can
/// name it.
///
/// A module string plus a name, never a `DeclKey`. A `DeclKey` carries a
/// `ModuleId`, those are issued by the project-level interner in `glyph-db`,
/// and one minted anywhere else is an in-range id for some *other* module: it
/// would answer wrongly rather than fail. This crate holds no interner, so it
/// hands out the strings and the project-wide fold mints the key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverageTypeName {
    /// A union declared in a project module, under the module key that
    /// declaration is reachable by: this file's own module path for a union
    /// declared here, the source module's key for an imported one. Empty for a
    /// file that declares no `module` line, which nothing can key.
    Declared { module: String, name: String },
    /// A prelude or stdlib union (`Result`, `Option`, `fs.ErrorKind`): a name
    /// with a fixed variant table behind it and no declaration to point at.
    Builtin { name: String },
}

impl From<&UnionRef> for CoverageTypeName {
    /// The type end of a site, derived from the one union resolution that
    /// produced its variant set.
    ///
    /// Derived rather than resolved a second time: a site's type end and the
    /// variant set it was counted against have to describe the same
    /// declaration, and the way they stop doing that is two functions walking
    /// the same four paths in an order kept in step by hand. `Local` and
    /// `Imported` collapse here because the relation keys a declaration by its
    /// module either way; which side of the boundary it was reached from is
    /// the consumer's question, not the key's.
    fn from(union: &UnionRef) -> Self {
        match union {
            UnionRef::Local { module, name } | UnionRef::Imported { module, name } => {
                CoverageTypeName::Declared {
                    module: module.clone(),
                    name: name.clone(),
                }
            }
            UnionRef::Builtin { name } => CoverageTypeName::Builtin {
                name: name.clone(),
            },
        }
    }
}

impl From<&UnionRef> for DiagnosticUnion {
    /// The diagnostic view of the same union resolution the coverage edge is
    /// keyed by, derived rather than resolved a second time for the reason
    /// given on `CoverageTypeName`'s conversion above.
    ///
    /// The one difference between the two views is the local module. A
    /// coverage edge is keyed inside this crate and takes the file's own
    /// `module` header; a diagnostic is qualified by the surface that renders
    /// it, from the root that surface counts modules from, which is the root
    /// its `entity` is already counted from.
    fn from(union: &UnionRef) -> Self {
        match union {
            UnionRef::Local { name, .. } => DiagnosticUnion::Local {
                name: name.clone(),
            },
            UnionRef::Imported { module, name } => DiagnosticUnion::Imported {
                module: module.clone(),
                name: name.clone(),
            },
            UnionRef::Builtin { name } => DiagnosticUnion::Builtin {
                name: name.clone(),
            },
        }
    }
}

/// The coverage view of a union a diagnostic named, given the module key the
/// file being checked is known by inside this crate.
///
/// The inverse direction of the pair above, and it exists for the same reason:
/// the string-literal checker resolves its union once, and both the edge it
/// writes and the error it raises are derived from that one answer instead of
/// walking the type a second time.
fn coverage_name(union: &DiagnosticUnion, own_module: &str) -> CoverageTypeName {
    match union {
        DiagnosticUnion::Local { name } => CoverageTypeName::Declared {
            module: own_module.to_string(),
            name: name.clone(),
        },
        DiagnosticUnion::Imported { module, name } => CoverageTypeName::Declared {
            module: module.clone(),
            name: name.clone(),
        },
        DiagnosticUnion::Builtin { name } => CoverageTypeName::Builtin { name: name.clone() },
    }
}

/// One arm naming one variant of one union.
///
/// `mentions`, not `covers`. An arm that reaches a variant through a payload
/// sub-pattern (`Ok(Some(x))`) has not covered `Ok`'s payload, and for most
/// top-level edges the checker draws no conclusion from the arm alone: it
/// concludes about the site, once every arm is in. What this edge records is
/// the thing the checker actually knew at the point it wrote it, which is that
/// the arm named the variant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageMention {
    /// The arm's ordinal within its site, counted from zero in source order.
    /// Order is semantic (D9: first match wins), so this and the site are the
    /// arm's whole identity.
    pub arm: u16,
    /// 0 for the scrutinee's own union, 1 for a payload union the checker
    /// recursed into, and so on.
    pub depth: u16,
    /// The union the variant belongs to: the site's scrutinee type at depth 0,
    /// and at greater depth a payload union, which can live in another module.
    pub union: CoverageTypeName,
    /// The variant named. For a string-literal union, whose members are values
    /// rather than tags, this is the value the arm matched.
    pub variant: String,
}

/// An arm the checker read nothing from, so its site's mentions are not a
/// complete accounting.
///
/// Three shapes decline: a single payload sub-pattern that tests a field's
/// value and can therefore fail (`Node({ colour: Black })`), an `is` guard
/// naming no variant of the union, and a top-level pattern the check does not
/// model (a literal, an array, a record destructure over a union scrutinee).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageDecline {
    pub arm: u16,
    pub depth: u16,
    /// The variant the arm named, when it named one: a value-testing payload
    /// declines a known variant, while an unmodeled top-level shape names
    /// nothing.
    pub variant: Option<String>,
}

/// An arm that absorbs every value the scrutinee can still take, which is
/// where the checker stops counting variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageCatchAll {
    pub arm: u16,
    pub depth: u16,
}

/// A union scope where some variant went unmentioned: the same list E0200
/// reports, recorded where the checker builds it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageGap {
    pub depth: u16,
    pub union: CoverageTypeName,
    /// The unmentioned variants, in declaration order, unquoted.
    pub missing: Vec<String>,
}

/// What the checker concluded about a whole site.
///
/// The state reports the weakest thing true of the site, so `Exhaustive` is
/// only ever the strongest claim: it means this site was counted against the
/// full variant set, nothing was declined, and nothing went unmentioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverageState {
    /// Every variant is mentioned and no arm was declined. Adding a variant to
    /// the union breaks this site, which is the guarantee the checker gives.
    Exhaustive,
    /// One of the site's own arms absorbs the rest, so the mentions are what
    /// the arms name rather than a complete accounting, and adding a variant
    /// leaves this site compiling and silent.
    HasCatchAll,
    /// The checker did not conclude coverage: an arm it declined to read (see
    /// `declines`), or a variant no arm mentions (see `gaps`, which is the
    /// E0200 it reported alongside).
    Declined,
    /// The scrutinee never resolved to a variant set, so there was nothing to
    /// count against. Nothing about this site is checked today.
    ScrutineeUnresolved,
}

/// One match site and every edge the checkers wrote about it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoverageSite {
    scrutinee_type: CoverageTypeName,
    scrutinee_span: Span,
    match_span: Span,
    scrutinee_resolved: bool,
    mentions: Vec<CoverageMention>,
    declines: Vec<CoverageDecline>,
    catch_alls: Vec<CoverageCatchAll>,
    gaps: Vec<CoverageGap>,
}

impl CoverageSite {
    /// The union this site's scrutinee resolved to. For an unresolved site,
    /// the type the scrutinee was *named* by, which is all that was known.
    pub fn scrutinee_type(&self) -> &CoverageTypeName {
        &self.scrutinee_type
    }

    /// The scrutinee expression's span. A location, not an identity: it moves
    /// with the file and is never compared across revisions.
    pub fn scrutinee_span(&self) -> Span {
        self.scrutinee_span
    }

    /// The whole `match` expression's span, which is where E0200 points.
    pub fn match_span(&self) -> Span {
        self.match_span
    }

    pub fn mentions(&self) -> &[CoverageMention] {
        &self.mentions
    }

    pub fn declines(&self) -> &[CoverageDecline] {
        &self.declines
    }

    pub fn catch_alls(&self) -> &[CoverageCatchAll] {
        &self.catch_alls
    }

    pub fn gaps(&self) -> &[CoverageGap] {
        &self.gaps
    }

    /// What the checker concluded here, derived from the edges rather than
    /// stored beside them: a site is written across the whole dispatch,
    /// including its payload recursions, and a state updated in pieces along
    /// the way is a state that can disagree with its own edges.
    pub fn state(&self) -> CoverageState {
        if !self.scrutinee_resolved {
            return CoverageState::ScrutineeUnresolved;
        }
        if !self.declines.is_empty() || !self.gaps.is_empty() {
            return CoverageState::Declined;
        }
        // Only a catch-all among the site's *own* arms makes the site absorb
        // what it does not name. One inside a payload (`Ok(x)` over a
        // `Result<Option<T>, E>`) leaves the site's own accounting complete,
        // which is what the checker concluded and what E0200's silence means.
        if self.catch_alls.iter().any(|c| c.depth == 0) {
            return CoverageState::HasCatchAll;
        }
        CoverageState::Exhaustive
    }

    /// The site as an answer carries it, with no site index anywhere in it.
    ///
    /// The index that routed the writes into this site is a cursor inside one
    /// computation: it is never published and never compared across
    /// revisions. What crosses the boundary is this descriptor, and an agent
    /// relocates the site from it. Turning the span into a line and into the
    /// scrutinee as written is the reader's job, because this crate never sees
    /// the file's bytes.
    pub fn descriptor(&self, module: &str) -> CoverageSiteRef {
        CoverageSiteRef {
            module: module.to_string(),
            scrutinee_span: self.scrutinee_span,
            match_span: self.match_span,
            state: self.state(),
            mentions: self.mentions.clone(),
            declines: self.declines.clone(),
            catch_alls: self.catch_alls.clone(),
            gaps: self.gaps.clone(),
        }
    }
}

/// A site's descriptor: where it is and what the checker concluded, with the
/// type end left to the caller that keys it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoverageSiteRef {
    /// The module key of the file the site is in.
    pub module: String,
    pub scrutinee_span: Span,
    pub match_span: Span,
    pub state: CoverageState,
    pub mentions: Vec<CoverageMention>,
    pub declines: Vec<CoverageDecline>,
    pub catch_alls: Vec<CoverageCatchAll>,
    pub gaps: Vec<CoverageGap>,
}

/// One file's match-coverage relation.
///
/// Sites are in the order the walk reached them, which is source order, and
/// each carries every edge written about it including the ones its payload
/// recursions produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FileMatchCoverage {
    sites: Vec<CoverageSite>,
}

impl FileMatchCoverage {
    pub fn sites(&self) -> &[CoverageSite] {
        &self.sites
    }

    /// Open a site and return the within-file index its writes go through.
    ///
    /// Private, and that is the design: the index is a cursor valid inside the
    /// computation that produced it, so there is no way for it to leave this
    /// file and become an identity that outlives an edit.
    fn open(
        &mut self,
        scrutinee_type: CoverageTypeName,
        scrutinee_span: Span,
        match_span: Span,
        scrutinee_resolved: bool,
    ) -> usize {
        self.sites.push(CoverageSite {
            scrutinee_type,
            scrutinee_span,
            match_span,
            scrutinee_resolved,
            mentions: Vec::new(),
            declines: Vec::new(),
            catch_alls: Vec::new(),
            gaps: Vec::new(),
        });
        self.sites.len() - 1
    }

    fn record_mention(&mut self, site: usize, edge: CoverageMention) {
        if let Some(s) = self.sites.get_mut(site) {
            s.mentions.push(edge);
        }
    }

    fn record_decline(&mut self, site: usize, edge: CoverageDecline) {
        if let Some(s) = self.sites.get_mut(site) {
            s.declines.push(edge);
        }
    }

    fn record_catch_all(&mut self, site: usize, edge: CoverageCatchAll) {
        if let Some(s) = self.sites.get_mut(site) {
            s.catch_alls.push(edge);
        }
    }

    fn record_gap(&mut self, site: usize, edge: CoverageGap) {
        if let Some(s) = self.sites.get_mut(site) {
            s.gaps.push(edge);
        }
    }
}

/// Which site a checker's coverage writes belong to.
#[derive(Debug, Clone, Copy)]
enum CoverageAt {
    /// A `match` (or a JSX `<match>`) the dispatch just reached: open a site
    /// for it, once the scrutinee's union has a name. Carries the scrutinee
    /// expression's span, which is what an answer locates the site by.
    Entry(Span),
    /// A payload an exhaustiveness check recursed into. A recursion is not a
    /// new site: it writes into the one already open, one level deeper.
    Payload { site: usize, depth: u16 },
}

/// The open site, the depth, and the union one check's writes are about.
#[derive(Debug, Clone)]
struct CoverageWriter {
    site: usize,
    depth: u16,
    union: CoverageTypeName,
}

impl CoverageWriter {
    /// Where a recursion into a payload of this scope writes.
    fn payload(&self) -> CoverageAt {
        CoverageAt::Payload {
            site: self.site,
            depth: self.depth + 1,
        }
    }
}

// ============================================================================
// Field uses: the side channel the member-access check fills
// ============================================================================

/// The record a field site was joined to, as far as one file can name it.
///
/// A module string plus a name, never a `DeclKey`, for the reason
/// `CoverageTypeName` gives: a `DeclKey` carries a `ModuleId`, those are issued
/// by the project-level interner in `glyph-db`, and one minted anywhere else is
/// an in-range id for some *other* module. This crate holds no interner, so it
/// hands out strings.
///
/// Unlike `CoverageTypeName`, nothing mints these into keys yet. The one
/// consumer keys a field by comparing `(module, name)` against the identity the
/// address resolved to, which is what the reference tool already does for a
/// symbol. A relation read by a surface that merges several of them wants the
/// minted form instead, and that is where the fold belongs when there is one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldOwner {
    /// A record declared in a project module, under the module key that
    /// declaration is reachable by: this file's own module path for a record
    /// declared here, the source module's key for an imported one. Empty for a
    /// file that declares no `module` line, which nothing can key.
    Declared { module: String, name: String },
    /// A field set with no project declaration behind it: an inline
    /// `{ a: string }` annotation, a variant's record payload, or a stdlib type
    /// whose field table the runtime ships (`fs.FileInfo`). Renaming a field of
    /// one is not a change a project module can make, so there is no
    /// declaration to address. `display` is the name a diagnostic prints.
    Undeclared { display: String },
    /// The object's type never resolved to a field set, so nothing joined this
    /// site to a record. `display` is the type the object was named by, which is
    /// all that was known.
    ///
    /// Recorded rather than skipped, and this is the whole reason the variant
    /// exists. A site like this may well be over the record being asked about,
    /// with a type that stopped resolving for an unrelated reason, and absence
    /// in this relation is reserved for meaning that no relation exists.
    Unresolved { display: String },
}

/// What one site does with the field.
///
/// Four kinds and not one flag, because a caller renaming a field has to edit
/// all four and they do not read alike. Nothing here is a judgement about
/// safety: every one of them stops compiling when the field's name changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldAccess {
    /// The field's own declaration inside its record body.
    Declaration,
    /// `u.email` read in value position.
    Read,
    /// `mut u.email = v`: the member expression is the assignment's target.
    Write,
    /// A `@redact fields: [email]` annotation naming the field (D24).
    Redact,
}

impl FieldAccess {
    /// The word an answer prints for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldAccess::Declaration => "declaration",
            FieldAccess::Read => "read",
            FieldAccess::Write => "write",
            FieldAccess::Redact => "redact",
        }
    }
}

/// One site naming one field of one record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldSite {
    owner: FieldOwner,
    field: String,
    span: Span,
    access: FieldAccess,
}

impl FieldSite {
    /// The record the checker joined this site to.
    pub fn owner(&self) -> &FieldOwner {
        &self.owner
    }

    /// The field as the site spells it. For every access kind but
    /// `Declaration` this is the spelling the source used, which is the same
    /// string as the declaration's when the site resolved.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The site's span: the whole member expression for a read or a write, the
    /// field's own name for a declaration, the `@redact` annotation for a
    /// redaction. A location, not an identity: it moves with the file and is
    /// never compared across revisions.
    pub fn span(&self) -> Span {
        self.span
    }

    pub fn access(&self) -> FieldAccess {
        self.access
    }
}

/// One file's field-use relation: every site that names a record field, in the
/// order the walk reached them, which is source order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FileFieldUses {
    sites: Vec<FieldSite>,
}

impl FileFieldUses {
    pub fn sites(&self) -> &[FieldSite] {
        &self.sites
    }

    pub fn len(&self) -> usize {
        self.sites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    fn record(&mut self, owner: FieldOwner, field: String, span: Span, access: FieldAccess) {
        self.sites.push(FieldSite {
            owner,
            field,
            span,
            access,
        });
    }
}

/// A field set the checker could read, and the record it came from.
///
/// The pair travels together because separating them is how they drift: the
/// field a member access resolves against and the declaration an answer keys
/// that access to have to describe the same record, and the way they stop doing
/// that is two functions walking the same six paths in an order kept in step by
/// hand.
#[derive(Debug, Clone)]
pub(crate) struct RecordShape {
    pub(crate) owner: FieldOwner,
    pub(crate) fields: Vec<RecordField>,
}

/// Every arm's pattern paired with its ordinal. The ordinal is half the arm's
/// identity (the site is the other half), and it is not incidental: `match` is
/// first-match-wins (D9), so an arm's position is part of what it means.
fn arm_patterns(arms: &[MatchArm]) -> Vec<(u16, &Pattern)> {
    arms.iter()
        .enumerate()
        .map(|(i, a)| (arm_ordinal(i), &a.pattern))
        .collect()
}

/// An arm's ordinal, saturating at `u16::MAX`. Nothing in the corpus is within
/// three orders of magnitude of a 65,536-arm `match`, and a saturating count
/// keeps the identity of every arm below that exact.
fn arm_ordinal(index: usize) -> u16 {
    u16::try_from(index).unwrap_or(u16::MAX)
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
    /// The match-coverage relation, filled by the exhaustiveness dispatch as
    /// it runs. Nothing reads it during the walk: it is written at the points
    /// where the checkers already know what an arm named, so the answer costs
    /// one push per edge and cannot drift from the diagnostics beside it.
    coverage: &'a mut FileMatchCoverage,
    field_uses: &'a mut FileFieldUses,
    /// The span of the member expression currently standing as an assignment's
    /// lvalue, so the one `Expr::Member` arm can tell a write from a read.
    ///
    /// A span and not a flag: `mut a.b = f(c.d)` walks the value with the
    /// target's span still set, and a flag would call `c.d` a write. The arm
    /// compares its own span, so only the target itself matches.
    assign_target: Option<Span>,
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

/// The `@<name>` annotations the compiler recognizes (D27). Anything else is a
/// hard error (`UnknownAnnotation`, E0221), so a typo cannot masquerade as a
/// working annotation. Names are stored without the leading `@`.
const RECOGNIZED_ANNOTATIONS: &[&str] = &["example", "doc", "redact", "open", "pure", "public"];

impl Assigner<'_> {
    // ----- decls -----

    /// D27: reject any annotation the compiler does not recognize. Runs for every
    /// top-level declaration before its own checks.
    fn check_annotations(&mut self, decl: &Decl) {
        // The declaration's name travels with the error: `a.span` covers the
        // annotation, which sits *before* the keyword every `Decl` span starts
        // at, so nothing downstream can recover the declaration from the span
        // it is reported at. An `import` has no annotations and no name.
        let (decl_name, annotations) = match decl {
            Decl::Fn(f) => (f.name.as_ref(), &f.annotations),
            Decl::Type(t) => (t.name.as_ref(), &t.annotations),
            Decl::Const(c) => (c.name.as_ref(), &c.annotations),
            Decl::Component(c) => (c.name.as_ref(), &c.annotations),
            Decl::Interface(i) => (i.name.as_ref(), &i.annotations),
            Decl::Import(_) => return,
        };
        for a in annotations {
            if !RECOGNIZED_ANNOTATIONS.contains(&a.name.as_ref()) {
                self.errors.push(TypeError::UnknownAnnotation {
                    name: a.name.to_string(),
                    decl: decl_name.to_string(),
                    span: a.span,
                });
            }
        }
    }

    fn walk_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Import(_) => {}
            // An interface is a set of type-level member signatures; there is no
            // value body to check. Its use as a bound/type is checked by `tsc`.
            Decl::Interface(_) => {}
            Decl::Type(t) => self.check_redact_annotation(t),
            Decl::Fn(f) => {
                let er = self.enclosing_return(f.return_ty.as_ref(), f.is_async);
                let wants_value = ty_requires_value(&er.ty);
                self.return_stack.push(er);
                self.bind_param_tys(&f.params);
                self.walk_block(&f.body);
                // Inside the callable's own return context: the check reads
                // nothing from the stack today, but it is about this callable,
                // so it runs before the frame is popped.
                if wants_value {
                    self.check_value_match_tail(&f.body);
                }
                self.return_stack.pop();
            }
            Decl::Component(c) => {
                // A component lowers to a React function component called
                // props-first, so more than one positional parameter would bind
                // the first to the whole props object and silently drop the rest.
                // Reject it (D19: a single props record, or none).
                if c.params.len() > 1 {
                    self.errors.push(TypeError::ComponentMultipleParams {
                        count: c.params.len(),
                        span: c.params[1].span,
                    });
                }
                // A `component` lowers to a React function component; there is
                // no `async component`, so it is never an async context.
                let er = self.enclosing_return(c.return_ty.as_ref(), false);
                let wants_value = ty_requires_value(&er.ty);
                self.return_stack.push(er);
                self.bind_param_tys(&c.params);
                self.walk_block(&c.body);
                if wants_value {
                    self.check_value_match_tail(&c.body);
                }
                self.return_stack.pop();
            }
            Decl::Const(c) => self.walk_expr(&c.value),
        }
    }

    /// Record a member access nothing joined to a record.
    ///
    /// The object's type did not resolve to a field set, so the compiler never
    /// decided which record this names, and it may well be the one being asked
    /// about. Absence in this relation means no relation exists, so a site the
    /// walk reached and could not key is named here instead of dropped.
    fn unresolved_field_use(&mut self, obj_ty: &Ty, field: &Ident, span: Span, access: FieldAccess) {
        self.field_uses.record(
            FieldOwner::Unresolved {
                display: ty_display(obj_ty),
            },
            field.to_string(),
            span,
            access,
        );
    }

    /// Record each field a record declaration declares, plus each field a
    /// `@redact` annotation on it names.
    ///
    /// The declaration is a site like any other: renaming a field means editing
    /// it, so an impact set that left it out would be missing the one edit the
    /// caller definitely has to make. The span is the field's own name rather
    /// than the whole declaration, because that is what gets rewritten.
    ///
    /// Only a `type` whose body is written as a record inline. An alias
    /// (`type Rows = Sheet`) declares no field of its own, and a field reached
    /// through it is a site over the record the alias names.
    fn declare_record_fields(&mut self, decl: &Decl) {
        let Decl::Type(t) = decl else { return };
        let owner = FieldOwner::Declared {
            module: self.own_module_key(),
            name: t.name.to_string(),
        };
        if let TypeExpr::Record { fields, .. } = &t.body {
            for f in fields {
                self.field_uses.record(
                    owner.clone(),
                    f.name.to_string(),
                    f.span,
                    FieldAccess::Declaration,
                );
            }
        }
        // A `@redact fields: [email]` names the field in a second place, and it
        // is validated against the same record body (E0219), so it is keyed
        // exactly. The span is the annotation's: `redact_fields` parses the
        // names out of the list and the names carry no span of their own.
        let Some(redacted) = glyph_ast::redact_fields(&t.annotations) else {
            return;
        };
        let Some(span) = t
            .annotations
            .iter()
            .find(|a| a.name.as_ref() == "redact")
            .map(|a| a.span)
        else {
            return;
        };
        for f in redacted {
            self.field_uses
                .record(owner.clone(), f, span, FieldAccess::Redact);
        }
    }

    /// D24: validate a `@redact fields: [...]` annotation against the type it
    /// decorates. Every named field must exist on the record; an unknown name is
    /// E0219 (it would silently mask nothing). Only record types have redactable
    /// fields, so a `@redact` on a non-record flags each named field.
    fn check_redact_annotation(&mut self, t: &glyph_ast::TypeDecl) {
        let Some(fields) = glyph_ast::redact_fields(&t.annotations) else {
            return;
        };
        let record_fields: Vec<&str> = match &t.body {
            TypeExpr::Record { fields, .. } => {
                fields.iter().map(|f| f.name.as_ref()).collect()
            }
            _ => Vec::new(),
        };
        let span = t
            .annotations
            .iter()
            .find(|a| a.name.as_ref() == "redact")
            .map(|a| a.span)
            .unwrap_or(t.span);
        for f in &fields {
            if !record_fields.iter().any(|rf| rf == f) {
                self.errors.push(TypeError::RedactUnknownField {
                    field: f.clone(),
                    type_name: t.name.to_string(),
                    // The span is the `@redact` annotation's, which sits
                    // before the `type` keyword the declaration's span starts
                    // at; carry the name so it survives the emission site.
                    decl: t.name.to_string(),
                    span,
                });
            }
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
        let n = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            self.walk_stmt(s);
            // Result-must-use (E0217, a warning): a `Result`-typed expression
            // used as a *non-final* statement discards its value, and with it a
            // possible `Err`. The final statement is skipped deliberately — it
            // may be the value of a match-arm block, which is legitimately used.
            // This never fires on `foo()?` (that types as the unwrapped `T`,
            // not a `Result`) or on a bound/returned value.
            if i + 1 < n {
                if let Stmt::Expr(e) = s {
                    let ty = self.tm.get(e.span()).clone();
                    if self.result_args(&ty).is_some() {
                        self.errors.push(TypeError::UnusedResult { span: e.span() });
                    }
                }
            }
        }
    }

    fn walk_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                self.walk_expr(&l.value);
                if let Expr::Match { arms, .. } = &l.value {
                    self.check_arms_produce_values(arms);
                }
                // Record the binding's type so later references resolve
                // concretely. An explicit annotation wins; otherwise infer
                // from the initializer's type (week-2 task 5, local `let`
                // inference). When unannotated and the initializer types as
                // `Unknown`, record nothing — leaving the binding open rather
                // than pinning it to `Unknown`, mirroring how
                // `collect_type_param_bindings` declines to bind an `Unknown`
                // argument.
                let ty = match &l.ty {
                    Some(te) => self.lowerer.lower(te),
                    None => self.tm.get(l.value.span()).clone(),
                };
                // G149: an explicit annotation that disagrees with the
                // initializer's inferred type must be flagged here, the same
                // way `check_return_type` flags a mismatched `return` value.
                // Without this, `let x: string = 42` records `string` as the
                // binding's type (below) and moves on silently; every later
                // read of `x` is then typed `string` while holding a number,
                // so the lie is load-bearing, not just cosmetic. Judged only
                // when there IS an explicit annotation: unannotated `let`
                // has nothing to disagree with, and `found` is read off the
                // initializer's own span so the diagnostic underlines the
                // value, not the annotation.
                if l.ty.is_some() {
                    let found = self.tm.get(l.value.span()).clone();
                    if self.assign_incompatible(&found, &ty) {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: ty_display(&ty),
                            found: ty_display(&found),
                            span: l.value.span(),
                        });
                    }
                }
                if l.ty.is_some() || !ty.is_unknown() {
                    self.local_tys.insert(l.span.start, ty);
                }
            }
            Stmt::Mut(m) => match &m.kind {
                glyph_ast::MutKind::Assign { target, value } => {
                    // D20: a `const` is immutable; `mut N = ...` reassigning one
                    // is rejected. Only a bare-name target rebinds; a field/index
                    // target mutates contents (a separate value-semantics
                    // question). The resolver records the target name at its own
                    // span when it walks the lvalue.
                    if let Expr::Ident { name, span } = target {
                        if let Some(ResolvedRef::Module(id)) = self.resolved.resolutions.get(*span) {
                            if let Some(sym) = self.resolved.symbols.table.get(id) {
                                if matches!(sym.kind, SymbolKind::Const { .. }) {
                                    self.errors.push(TypeError::MutateConst {
                                        name: name.to_string(),
                                        span: *span,
                                    });
                                }
                            }
                        }
                    }
                    // The target is an lvalue, so an index on it is a *write*.
                    // Writing a key into a map is how a map is built and is
                    // always safe; only reading one is a guess (E0224). Walking
                    // the parts rather than the whole keeps every other check
                    // on them while skipping that judgement.
                    match target {
                        Expr::Index { object, index, span } => {
                            self.walk_expr(object);
                            self.walk_expr(index);
                            // The node still needs a type: every expression
                            // carries one, and the lvalue is an expression.
                            // Only the E0224 judgement is skipped.
                            self.tm.insert(*span, Ty::Unknown);
                        }
                        other => {
                            // `mut u.email = v` writes the field. The member
                            // arm is the one place that resolves it, so what
                            // makes this a write rather than a read is which
                            // node it is, and that is what travels down.
                            let outer = self.assign_target.replace(other.span());
                            self.walk_expr(other);
                            self.assign_target = outer;
                        }
                    }
                    self.walk_expr(value);
                    if let Expr::Match { arms, .. } = value {
                        self.check_arms_produce_values(arms);
                    }
                }
                glyph_ast::MutKind::MethodCall { call } => {
                    self.walk_expr(call);
                }
            },
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.walk_expr(v);
                    self.check_return_type(v);
                    // `return match ...` in a `void` callable returns nothing
                    // of consequence, so only a callable that decidably needs a
                    // value has arms that must produce one.
                    let wants_value = self
                        .return_stack
                        .last()
                        .is_some_and(|er| ty_requires_value(&er.ty));
                    if wants_value {
                        if let Expr::Match { arms, .. } = v {
                            self.check_arms_produce_values(arms);
                        }
                    }
                }
            }
            Stmt::For(f) => {
                self.walk_expr(&f.iter);
                // Give the loop's element binding the iterand's element type.
                //
                // Without it the binding is `Unknown`, so every judgement that
                // depends on the element's type evaporates the moment the value
                // is iterated: a `match` over a string-literal union (D30) went
                // from "you have not handled `pro`" to "a string match can never
                // be exhaustive, add an `else`", which is advice to switch off
                // the check rather than to satisfy it.
                //
                // Each binding now has its own def-site span (G37), so the
                // two-binding form (`for i, v in xs`) can type `v` without also
                // mistyping the index `i` as the element — the two used to
                // share one key (the statement's span) and only the
                // single-binding form had a key to give a type to.
                //
                // Only the `Array<T>` iterand shape is modeled: `array_elem_ty`
                // returns `None` for `Record<K, V>`, so a two-binding loop over
                // a record's entries still leaves both bindings `Unknown`. That
                // matches the single-binding form's existing scope; typing a
                // record's key/value pair is a separate, unstarted piece of
                // work.
                let elem_binding = match f.bindings.as_slice() {
                    [v] => Some(v),
                    [_, v] => Some(v),
                    _ => None,
                };
                if let Some(v) = elem_binding {
                    if let Some(elem) = array_elem_ty(&self.tm.get(f.iter.span()).clone()) {
                        self.local_tys.insert(v.span.start, elem);
                    }
                }
                self.walk_block(&f.body);
            }
            Stmt::Loop(l) => self.walk_block(&l.body),
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Defer(d) => self.walk_expr(&d.expr),
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
            // The escape hatch is opaque to Glyph's checker; `tsc` checks its
            // raw TypeScript. Type it `unknown` so any use must narrow first.
            Expr::Extern { span, .. } => self.tm.insert(*span, Ty::Unknown),
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
                let ty = if matches!(op, PostfixOp::Try) {
                    let operand_ty = self.tm.get(operand.span()).clone();
                    self.check_question_operator(*span, &operand_ty)
                } else {
                    Ty::Unknown
                };
                self.tm.insert(*span, ty);
            }
            Expr::Unary { operand: child, span, .. } => {
                self.walk_expr(child);
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Member { object, field, span, .. } => {
                self.walk_expr(object);
                let obj_ty = self.tm.get(object.span()).clone();
                let access = if self.assign_target == Some(*span) {
                    FieldAccess::Write
                } else {
                    FieldAccess::Read
                };
                // When the object's type is decidably a record, the field must
                // exist; report a typo'd/renamed field and propagate the field's
                // type so chained accesses keep checking. A non-record or
                // undecidable object type is left unchecked (no false positive).
                let member_ty = match self.record_shape_of(&obj_ty) {
                    Some(shape) => match shape.fields.iter().find(|f| &f.name == field) {
                        Some(f) => {
                            // The one place the field-use edge is written, and
                            // it is written from the resolution that just
                            // decided the access is legal. There is no second
                            // walk to keep in step and no way for the edge to
                            // name a record the check did not use.
                            self.field_uses.record(
                                shape.owner,
                                field.to_string(),
                                *span,
                                access,
                            );
                            f.ty.clone()
                        }
                        None => {
                            // Deliberately no edge. The checker resolved the
                            // record and the record does not declare this
                            // field, so there is no relation here: E0210 says
                            // so, and recording one would put a site in the
                            // impact set of a field it provably does not name.
                            self.errors.push(TypeError::UnknownField {
                                field: field.to_string(),
                                type_name: ty_display(&obj_ty),
                                span: *span,
                            });
                            Ty::Unknown
                        }
                    },
                    // Not a decidable record. Try the stdlib namespace path
                    // (`http.get`, `fs.read_text`, ...) so a hand-written
                    // TS-wrapper function still gets a Glyph-level signature,
                    // then the runtime descriptor a type declaration of our own
                    // emits (`WireLedger.parse`); otherwise stay `Unknown`
                    // (permissive).
                    // A `Record<K, V>` map has arbitrary keys, so the compiler
                    // cannot know this one is there. Typing the access as `V`
                    // states something unchecked, and the value is `undefined`
                    // when the key is absent: a mistyped column read off a
                    // database row compiled clean and rendered as the text
                    // "undefined". `record.get` is the same lookup with the
                    // absent case in the type.
                    None if self.resolves_to_map(&obj_ty) => {
                        // A map key is not a record field, so there is no
                        // record to join this to. Named all the same: the
                        // spelling is the one being asked about, and a caller
                        // has to see that this site was reached and not keyed.
                        self.unresolved_field_use(&obj_ty, field, *span, access);
                        self.errors.push(TypeError::MapFieldAccess {
                            field: field.to_string(),
                            type_name: ty_display(&obj_ty),
                            span: *span,
                        });
                        Ty::Unknown
                    }
                    None => {
                        self.unresolved_field_use(&obj_ty, field, *span, access);
                        self.stdlib_member_ty(object, field)
                            .or_else(|| self.descriptor_member_ty(object, field))
                            .unwrap_or(Ty::Unknown)
                    }
                };
                self.tm.insert(*span, member_ty);
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
                // E0222: `await` needs an `async` enclosing callable. The
                // innermost one decides, so a synchronous lambda inside an
                // `async fn` is flagged (as it is in TypeScript). An empty
                // stack is a `const` initializer at module scope; the emitted
                // module is ESM, where top-level `await` is legal, so that
                // case stays permissive.
                if let Some(er) = self.return_stack.last() {
                    if !er.is_async {
                        self.errors
                            .push(TypeError::AwaitOutsideAsyncFn { span: *span });
                    }
                }
            }
            Expr::Binary { left, right, span, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
                self.tm.insert(*span, Ty::Unknown);
            }
            // Indexing. `xs[i]` on an array stays unchecked: the bound is a
            // value, and a program that has just measured `array.len` is not
            // making a mistake. A `Record<K, V>` map is different, because
            // there is no bound to check against and the key is a guess, so
            // the same reasoning as the dot form applies (E0224).
            Expr::Index {
                object,
                index,
                span,
                ..
            } => {
                self.walk_expr(object);
                self.walk_expr(index);
                let obj_ty = self.tm.get(object.span()).clone();
                if self.resolves_to_map(&obj_ty) {
                    self.errors.push(TypeError::MapFieldAccess {
                        field: index_key_display(index),
                        type_name: ty_display(&obj_ty),
                        span: *span,
                    });
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            // A `new` interop constructor. Glyph has no class definitions, so
            // the callee is always an imported/external value the checker sees
            // as opaque; the instance type is therefore `Unknown` on the Glyph
            // side and `tsc` checks the construction against the real
            // constructor. We still walk the callee and args so their names
            // resolve and their subexpressions get typed.
            Expr::New {
                callee, args, span, ..
            } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_expr(a);
                }
                self.tm.insert(*span, Ty::Unknown);
            }
            Expr::Call {
                callee,
                args,
                span,
                type_args,
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
                // Clone the callee's signature so the borrow of `self.tm` ends
                // before the per-argument checks (which read `self.tm` and push
                // to `self.errors`).
                let callee_ty = self.tm.get(callee.span()).clone();
                // `T.parse<A>(v)` on a generic record: the member alone has no
                // signature (its descriptor arity depends on the parameters),
                // so the instantiation is read from the call's own type args.
                if matches!(callee_ty, Ty::Unknown) {
                    if let Some(ty) = self.generic_descriptor_parse_ty(callee, type_args) {
                        self.tm.insert(*span, ty);
                        return;
                    }
                }
                let call_ty = if let Ty::Fn { params, return_ty, .. } = &callee_ty {
                    // Arity check. A Glyph `fn`/`component` has no optional or
                    // variadic parameter and a call carries no spread, so for
                    // user code this is still an exact comparison. The standard
                    // library is the exception: several of its functions take a
                    // trailing argument TypeScript declares optional, and
                    // comparing one number against one number reported a false
                    // error on every call that omitted it, which is why they
                    // were left unmodeled and their results were `Unknown`
                    // (G39). Required parameters set the floor, the whole list
                    // the ceiling.
                    let required = params.iter().filter(|p| !p.optional).count();
                    if args.len() < required || args.len() > params.len() {
                        self.errors.push(TypeError::ArgumentCountMismatch {
                            expected: if args.len() < required {
                                required
                            } else {
                                params.len()
                            },
                            found: args.len(),
                            span: *span,
                        });
                    }
                    let mut subst: HashMap<Ident, Ty> = HashMap::new();
                    for (p, a) in params.iter().zip(args.iter()) {
                        collect_type_param_bindings(&p.ty, self.tm.get(a.span()), &mut subst);
                    }
                    // Check each argument against its parameter type (with any
                    // inferred generics substituted in). Reports only a provable
                    // mismatch; an undecidable argument or parameter is silent.
                    for (p, a) in params.iter().zip(args.iter()) {
                        let expected = substitute_type_params(&p.ty, &subst);
                        let found = self.tm.get(a.span()).clone();
                        if self.assign_incompatible(&found, &expected) {
                            self.errors.push(TypeError::ArgumentTypeMismatch {
                                expected: ty_display(&expected),
                                found: ty_display(&found),
                                span: a.span(),
                            });
                        }
                    }
                    substitute_type_params(return_ty, &subst)
                } else {
                    Ty::Unknown
                };
                self.tm.insert(*span, call_ty);
            }
            Expr::Array { elements, span } => {
                for el in elements {
                    let (ArrayElem::Expr(e) | ArrayElem::Spread(e)) = el;
                    self.walk_expr(e);
                }
                // Synthesize `Array<T>` so a `let`-bound array literal carries a
                // concrete container type downstream. This is what lets the
                // emitter distinguish an array (numeric-index `.entries()`) from
                // a record (string-key `Object.entries`) in a two-binding `for`
                // — an inferred, unannotated array previously typed `Unknown` and
                // was misclassified as a record. The element type is inferred
                // only when every plain element shares one decidable type;
                // otherwise (mixed elements, an empty literal, or any spread)
                // it stays `Array<Unknown>`, which is enough for the container
                // discrimination and never triggers a false argument mismatch.
                let elem_ty = self.infer_array_elem_ty(elements);
                let ty = self.array_ty(elem_ty);
                self.tm.insert(*span, ty);
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
                let mut scrutinee_ty = self.tm.get(scrutinee.span()).clone();
                // When the scrutinee's static type is undecidable — an
                // ambient/external value such as a member access through a
                // `.d.ts` generic (`state.value`, where the Glyph checker
                // cannot see `StateHandle<T>`) — try to recover a module-local
                // tagged union from the arm patterns. Without this the
                // scrutinee type stays `Unknown`, exhaustiveness is skipped,
                // and the emitter misclassifies every nullary-variant arm as a
                // binding catch-all (silently wrong at runtime, or an E0300 for
                // 2+ arms). Recording the recovered union at the scrutinee's
                // span makes both the check below and the emitter's
                // variant/binding classifier key off a concrete union.
                if matches!(scrutinee_ty, Ty::Unknown) {
                    if let Some(recovered) = self.recover_union_from_arms(arms) {
                        self.tm.insert(scrutinee.span(), recovered.clone());
                        scrutinee_ty = recovered;
                    }
                }
                self.check_match_exhaustiveness(&scrutinee_ty, scrutinee.span(), arms, *span);
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
                // `match` is Glyph's only branching construct (D: no `if`), so
                // leaving it `Unknown` starves everything downstream: a
                // `let`-bound match had no type, which silently misclassified a
                // two-binding `for` over one of its fields as a record
                // (string-key `Object.entries`) and pushed field-existence
                // checking out to `tsc`. Join the arms by equality: when every
                // non-divergent arm produces the same decidable type, the whole
                // expression has it. Anything else keeps the old `Unknown`.
                let ty = self.join_match_arms(arms);
                self.tm.insert(*span, ty);
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                is_async,
                span,
            } => {
                let er = self.enclosing_return(return_ty.as_ref(), *is_async);
                let wants_value = ty_requires_value(&er.ty);
                self.return_stack.push(er);
                self.bind_param_tys(params);
                self.walk_block(body);
                if wants_value {
                    self.check_value_match_tail(body);
                }
                self.return_stack.pop();
                let ty = self
                    .lowerer
                    .lower_callable_signature(params, return_ty.as_ref(), *is_async);
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
            match attr {
                JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => self.walk_expr(value),
                JsxAttr::String { .. } | JsxAttr::Positional { .. } => {}
            }
        }
        // A JSX `<match value={s}>` directive is a conditional over a tagged
        // union exactly like a value-level `match`: each `<case Variant>` covers
        // one variant, and the emitter lowers the whole thing to a `switch` with
        // a `default: throw new Error("non-exhaustive match")`. Run the same
        // exhaustiveness check here so a missing `<case>` is a compile-time
        // E0200 rather than a runtime throw. Walk the attrs above FIRST so the
        // scrutinee's type is recorded before we look it up.
        if j.name.as_ref() == "match" {
            self.check_jsx_match_exhaustiveness(j);
        }
        for child in &j.children {
            match child {
                JsxChild::Element(e) => self.walk_jsx(e),
                JsxChild::Expr(e) => self.walk_expr(e),
                JsxChild::Text { .. } => {}
            }
        }
    }

    /// Exhaustiveness for a `<match value={s}>` JSX directive. Mirrors the
    /// value-level `Expr::Match` path: resolve the scrutinee (`value={..}`) to a
    /// tagged union and require the `<case Variant>` children to cover every
    /// variant. Each `<case>` names its variant as a positional attribute and
    /// covers that variant wholesale (a `bind={x}` binds the whole payload), so
    /// synthetic `Pattern::Ident` patterns feed the shared exhaustiveness core.
    fn check_jsx_match_exhaustiveness(&mut self, j: &JsxElement) {
        let Some(value) = j.attrs.iter().find_map(|a| match a {
            JsxAttr::Expr { name, value, .. } if name.as_ref() == "value" => Some(value),
            _ => None,
        }) else {
            return;
        };
        let mut scrutinee_ty = self.tm.get(value.span()).clone();

        // The variant named by each `<case Variant ...>` child's first
        // positional attribute.
        let case_variants: Vec<Ident> = j
            .children
            .iter()
            .filter_map(|child| match child {
                JsxChild::Element(el) if el.name.as_ref() == "case" => {
                    el.attrs.iter().find_map(|a| match a {
                        JsxAttr::Positional { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .collect();

        // When the scrutinee's static type is undecidable (an ambient/external
        // value whose Glyph type stays `Unknown`), recover a module-local union
        // from a `<case>` variant name, matching `Expr::Match`'s arm recovery.
        if matches!(scrutinee_ty, Ty::Unknown) {
            for v in &case_variants {
                if let Some(recovered) = self.union_ty_of_variant(v) {
                    scrutinee_ty = recovered;
                    break;
                }
            }
        }

        let patterns: Vec<Pattern> = case_variants
            .into_iter()
            .map(|name| Pattern::Ident { name, span: j.span })
            .collect();
        // A `<case>`'s position among the cases is its ordinal, the role an
        // arm's position plays in a value-level `match`.
        let pattern_refs: Vec<(u16, &Pattern)> = patterns
            .iter()
            .enumerate()
            .map(|(i, p)| (arm_ordinal(i), p))
            .collect();
        self.check_patterns_exhaustive(
            &scrutinee_ty,
            &pattern_refs,
            j.span,
            Some(CoverageAt::Entry(value.span())),
        );
    }

    // ----- ident reference typing -----

    /// The prelude `Array` type applied to `elem` (`Array<elem>`), built as an
    /// `App` over the prelude `Array` `Ty::Named` so it matches the shape a
    /// declared `Array<T>` parameter lowers to. Falls back to `Unknown` only if
    /// the prelude has no `Array` symbol (never, in practice).
    fn array_ty(&self, elem: Ty) -> Ty {
        match self.lowerer.prelude.lookup("Array") {
            Some(id) => Ty::App {
                base: Arc::new(Ty::Named {
                    symbol: id.into(),
                    path: vec![Ident::from("Array")],
                }),
                args: vec![elem],
            },
            None => Ty::Unknown,
        }
    }

    /// Infer the element type of an array literal from its already-walked
    /// elements. Returns a concrete type only when every element is a plain
    /// (non-spread) expression with the same decidable type; a spread element,
    /// an empty literal, or any type disagreement yields `Unknown`, keeping the
    /// literal at `Array<Unknown>`.
    fn infer_array_elem_ty(&self, elements: &[ArrayElem]) -> Ty {
        let mut inferred: Option<Ty> = None;
        for el in elements {
            let e = match el {
                ArrayElem::Expr(e) => e,
                // A spread contributes its *element* type, not its own type;
                // rather than unwrap the container here, stay conservative.
                ArrayElem::Spread(_) => return Ty::Unknown,
            };
            let ty = self.tm.get(e.span());
            if !ty_is_decidable(ty) {
                return Ty::Unknown;
            }
            match &inferred {
                None => inferred = Some(ty.clone()),
                Some(prev) if prev == ty => {}
                Some(_) => return Ty::Unknown,
            }
        }
        inferred.unwrap_or(Ty::Unknown)
    }

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
                    // A named import of a modeled stdlib function (`import
                    // std/http { header }`) gets the same signature the
                    // namespace path (`http.header`) does, so a modeled
                    // `Option`/`Result` return is enforced regardless of import
                    // style. Unmodeled members stay `Unknown` (permissive).
                    //
                    // A named import of a *project sibling's* `pub fn`
                    // (`import a { make }`) is tried first: `stdlib_fn_ty` only
                    // ever answers for a `std/*` key, so it would silently miss
                    // a project module's own function every time, and G133 is
                    // exactly that gap — a cross-module call typing as
                    // `Unknown` regardless of what the callee returns.
                    SymbolKind::ImportNamed { path, original } => {
                        let key = path
                            .segments
                            .iter()
                            .map(|s| s.as_ref())
                            .collect::<Vec<_>>()
                            .join("/");
                        self.decl_ty_resolver
                            .imported_fn_decl(&key, original.as_ref())
                            .or_else(|| self.stdlib_fn_ty(&key, original.as_ref()))
                            .unwrap_or(Ty::Unknown)
                    }
                    _ => Ty::Unknown,
                }
            }
            // Prelude values (`Ok`, `Err`, etc.) need use-site generic
            // instantiation — week-3 bidirectional checker work.
            ResolvedRef::Prelude(_) => Ty::Unknown,
        }
    }

    // ----- stdlib TS-wrapper signatures -----

    /// Synthesize the type of a member access `ns.field` when `ns` is a
    /// namespace import of a stdlib module (`import std/http`, `import std/fs
    /// as f`) and `field` is one of the hand-written TS-wrapper functions the
    /// Glyph runtime ships.
    ///
    /// v1 models exactly the Result-returning stdlib functions whose error type
    /// is a concrete, named type (`HttpError`, `FsError`). Modeling them gives
    /// those functions a decidable `Result<T, E>` at the `?` operator, so the
    /// exact-error-type rule (E0203) fires for `http.get(url)?` at parity with
    /// a local record/union error type, instead of falling through to the `tsc`
    /// backstop. The runtime has no `.d.ts` the checker parses, so this small
    /// table stands in until the stdlib is modeled from real sources (Q21/Q40);
    /// every unmodeled member stays `Unknown` (permissive), exactly as before.
    fn stdlib_member_ty(&self, object: &Expr, field: &Ident) -> Option<Ty> {
        let Expr::Ident { span, .. } = object else {
            return None;
        };
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(*span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        let path = match &sym.kind {
            SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. } => path,
            _ => return None,
        };
        let key = path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        // The namespace spelling of the same G133 gap: `a.make()` reaches a
        // project sibling's `pub fn` through `ImportNamespace`/`ImportAlias`
        // rather than `ImportNamed`, so it needs the identical fallback the
        // by-name arm in `type_of_ident_ref` tries first.
        self.decl_ty_resolver
            .imported_fn_decl(&key, field.as_ref())
            .or_else(|| self.stdlib_fn_ty(&key, field.as_ref()))
    }

    /// The signature of `T.parse` for a module-local type `T` that emits a Q8
    /// runtime descriptor: `parse(value) -> Result<T, Array<Issue>>`.
    ///
    /// This is the entry point for untrusted input, so leaving it `Unknown`
    /// undoes the work of every downstream inference: the `match` over its
    /// result has an undecidable scrutinee, the `Ok(w)` arm binds nothing, and
    /// the checked value is back to being an opaque blob that only `tsc` sees.
    /// The signature is read off the same shape the emitter writes, so the two
    /// agree by construction.
    ///
    /// Eligibility mirrors `emit_type_decl` exactly, because typing a `parse`
    /// the emitter does not write would be a signature for a member that is not
    /// there. A descriptor exists for a non-generic record, a non-generic
    /// tagged union whose name no variant shadows, and a refined primitive.
    /// A plain alias (`type Cents = int`) has none. A *generic* record's
    /// descriptor takes one runtime checker per type parameter, so its arity
    /// differs and it is left `Unknown`.
    fn descriptor_member_ty(&self, object: &Expr, field: &Ident) -> Option<Ty> {
        if field.as_ref() != "parse" {
            return None;
        }
        let Expr::Ident { span, .. } = object else {
            return None;
        };
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(*span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        let SymbolKind::Type { decl_idx } = sym.kind else {
            return None;
        };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        if !td.generics.is_empty() {
            return None;
        }
        let has_descriptor = match &td.body {
            TypeExpr::Record { .. } => true,
            TypeExpr::Union { variants, .. } => {
                variants.iter().all(|v| v.name.as_ref() != td.name.as_ref())
            }
            _ => td.refinement.is_some(),
        };
        if !has_descriptor {
            return None;
        }
        let parsed = Ty::Named {
            symbol: SymbolRef(id.0),
            path: vec![td.name.clone()],
        };
        let issue_id = self.lowerer.prelude.lookup("Issue")?;
        let issues = self.stdlib_array_ty(Ty::Named {
            symbol: SymbolRef(issue_id.0),
            path: vec![Ident::from("Issue")],
        })?;
        Some(Ty::Fn {
            params: vec![FnParam {
                name: None,
                owned: false,
                ty: Ty::Unknown,
                optional: false,
            }],
            return_ty: Arc::new(self.stdlib_result_ty(parsed, issues)?),
            is_async: false,
        })
    }

    /// The type of `T.parse<A, B>(value)` for a **generic** record `T`:
    /// `Result<T<A, B>, Array<Issue>>`.
    ///
    /// `descriptor_member_ty` handles the non-generic case and stops at a
    /// generic one, because a generic descriptor takes one runtime checker per
    /// type parameter, so `T.parse` has no single signature to give the member.
    /// That is still true, and it is why this reads the *call* instead: the
    /// instantiation only exists once the explicit type arguments are written.
    ///
    /// Leaving it `Unknown` was not a missing convenience. The parsed value's
    /// fields were invisible, so a typo on one produced no Glyph diagnostic (a
    /// `tsc` TS2339 mapped to the whole enclosing function, where the
    /// non-generic path gives E0210 at the field), and the emitter could not
    /// tell an `Array` field from a record, which made a `for k, v` over one
    /// bind a string index in a build reporting no diagnostics (G109).
    ///
    /// Requires the type arguments to be written explicitly and to match the
    /// declaration's arity. `parse` takes an `unknown`, so there is nothing to
    /// infer them from, and guessing would put a wrong shape behind a boundary
    /// check, which is worse than leaving it opaque.
    fn generic_descriptor_parse_ty(&self, callee: &Expr, type_args: &[TypeExpr]) -> Option<Ty> {
        if type_args.is_empty() {
            return None;
        }
        let Expr::Member { object, field, .. } = callee else {
            return None;
        };
        if field.as_ref() != "parse" {
            return None;
        }
        let Expr::Ident { span, .. } = object.as_ref() else {
            return None;
        };
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(*span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        let SymbolKind::Type { decl_idx } = sym.kind else {
            return None;
        };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        if td.generics.len() != type_args.len() {
            return None;
        }
        // Same eligibility as the non-generic path: a descriptor exists for a
        // record, for a tagged union no variant shadows, and for a refined
        // primitive. Typing a `parse` the emitter does not write would be a
        // signature for a member that is not there.
        let has_descriptor = match &td.body {
            TypeExpr::Record { .. } => true,
            TypeExpr::Union { variants, .. } => {
                variants.iter().all(|v| v.name.as_ref() != td.name.as_ref())
            }
            _ => td.refinement.is_some(),
        };
        if !has_descriptor {
            return None;
        }
        let args: Vec<Ty> = type_args.iter().map(|a| self.lowerer.lower(a)).collect();
        let parsed = Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(id.0),
                path: vec![td.name.clone()],
            }),
            args,
        };
        let issue_id = self.lowerer.prelude.lookup("Issue")?;
        let issues = self.stdlib_array_ty(Ty::Named {
            symbol: SymbolRef(issue_id.0),
            path: vec![Ident::from("Issue")],
        })?;
        self.stdlib_result_ty(parsed, issues)
    }

    /// The signature of a modeled stdlib TS-wrapper function, or `None` for any
    /// function not in the v1 table. Parameter types are left `Unknown` (only
    /// the arity is modeled) so this never introduces a new argument-type
    /// diagnostic; the value it adds is the decidable `Result<T, E>` return.
    fn stdlib_fn_ty(&self, module_key: &str, field: &str) -> Option<Ty> {
        // The CLDR plural category is the reason `std/intl` exists. Modeling the
        // return as the closed six-member literal union is what makes a `match`
        // over it exhaustive without a catch-all (D30); as a bare `string` it
        // would be E0218, whose advice is to add an `else`, and an `else` over a
        // plural category is how a locale's `few` silently renders as `other`.
        if module_key == "std/intl"
            && matches!(field, "plural_category" | "ordinal_category")
        {
            return Some(Ty::Fn {
                params: vec![required(Ty::Unknown), required(Ty::Unknown)],
                return_ty: Arc::new(Ty::StringLiteralUnion(vec![
                    "zero".to_string(),
                    "one".to_string(),
                    "two".to_string(),
                    "few".to_string(),
                    "many".to_string(),
                    "other".to_string(),
                ])),
                is_async: false,
            });
        }

        // `json.stringify(value, options?)` -> string. The sixth and last of the
        // trailing-optional functions G39 named: modelable now that the arity
        // check understands a minimum and a maximum, so its result stops being
        // `Unknown` and a program that matches or concatenates it is checked.
        if (module_key, field) == ("std/json", "stringify") {
            return Some(Ty::Fn {
                params: vec![required(Ty::Unknown), optional(Ty::Unknown)],
                return_ty: Arc::new(Ty::Prim(Primitive::String)),
                is_async: false,
            });
        }

        // Option-returning accessors for untrusted request input. Modeling the
        // return as `Option<string>` gives the caller a `match` the
        // exhaustiveness checker understands, so a missing header or query
        // parameter can't be read as if it were present.
        if let Some(inner) = match (module_key, field) {
            ("std/http", "header") | ("std/http", "query_param") => Some(Ty::Prim(Primitive::String)),
            ("std/json", "discriminant") => Some(Ty::Prim(Primitive::String)),
            _ => None,
        } {
            let return_ty = self.stdlib_option_ty(inner)?;
            let params = (0..2)
                .map(|_| FnParam {
                    name: None,
                    owned: false,
                    ty: Ty::Unknown,
                optional: false,
                })
                .collect();
            return Some(Ty::Fn {
                params,
                return_ty: Arc::new(return_ty),
                is_async: false,
            });
        }

        // `segments(req) -> Array<string>`: modeled so a router's array-pattern
        // match (`["tasks", id]`) binds `id` as a `string`.
        if (module_key, field) == ("std/http", "segments") {
            let return_ty = self.stdlib_array_ty(Ty::Prim(Primitive::String))?;
            return Some(Ty::Fn {
                params: vec![FnParam {
                    name: None,
                    owned: false,
                    ty: Ty::Unknown,
                optional: false,
                }],
                return_ty: Arc::new(return_ty),
                is_async: false,
            });
        }

        // `range(count)` / `range_from(start, end) -> Array<number>`: the
        // counted loop. Modeled so `for i in array.range(n)` binds `i` as a
        // `number` instead of falling back to `Unknown` — a hand-rolled `upto(n)
        // -> Array<int>` is typed today, so without this the stdlib replacement
        // would be a typing regression. `int` lowers to `Primitive::Number`, so
        // `Array<number>` also satisfies an `Array<int>` annotation.
        if let Some(arity) = match (module_key, field) {
            ("std/array", "range") => Some(1),
            ("std/array", "range_from") => Some(2),
            _ => None,
        } {
            let return_ty = self.stdlib_array_ty(Ty::Prim(Primitive::Number))?;
            let params = (0..arity)
                .map(|_| FnParam {
                    name: None,
                    owned: false,
                    ty: Ty::Prim(Primitive::Number),
                optional: false,
                })
                .collect();
            return Some(Ty::Fn {
                params,
                return_ty: Arc::new(return_ty),
                is_async: false,
            });
        }

        // `fetch_of(url, method)` builds the request record `send` takes. Modeled
        // so the record a program threads through carries its type, and a
        // misspelled field on it is a Glyph error rather than a `tsc` one.
        if let ("std/http", "fetch_of") = (module_key, field) {
            let params = (0..2)
                .map(|_| FnParam {
                    name: None,
                    owned: false,
                    ty: Ty::Prim(Primitive::String),
                optional: false,
                })
                .collect();
            return Some(Ty::Fn {
                params,
                return_ty: Arc::new(stdlib_named("http", "Fetch")),
                is_async: false,
            });
        }

        // Response constructors that do not return a `Result`. Modeled so a
        // handler's `Ok(http.html(...))` is checked against its declared
        // `Result<Response, string>` here, rather than only by `tsc` on the
        // emitted TypeScript.
        if let Some(arity) = match (module_key, field) {
            ("std/http", "html") | ("std/http", "redirect") => Some(2),
            ("std/http", "with_header") => Some(3),
            _ => None,
        } {
            let params = (0..arity)
                .map(|_| FnParam {
                    name: None,
                    owned: false,
                    ty: Ty::Unknown,
                optional: false,
                })
                .collect();
            return Some(Ty::Fn {
                params,
                return_ty: Arc::new(stdlib_named("http", "Response")),
                is_async: false,
            });
        }

        if let Some(sig) = self.stdlib_string_fn_ty(module_key, field) {
            return Some(sig);
        }
        if let Some(sig) = self.stdlib_array_fn_ty(module_key, field) {
            return Some(sig);
        }
        if let Some(sig) = self.stdlib_record_fn_ty(module_key, field) {
            return Some(sig);
        }

        // (arity, ok, err, is_async)
        let (arity, ok, err, is_async): (usize, Ty, Ty, bool) = match (module_key, field) {
            // Without an entry here the return type is unknown, D30
            // exhaustiveness never fires, and a `match` with only an `Ok` arm
            // builds clean, passes `tsc --strict`, and throws at run time. The
            // accessor exists so a failure is a value you must handle, so the
            // checker has to know its shape.
            ("std/http", "to_text") => (
                1,
                Ty::Prim(Primitive::String),
                Ty::Prim(Primitive::String),
                false,
            ),
            ("std/http", "get") => (
                1,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            // The bounded form: one `Fetch` record carrying the timeout and the
            // redirect policy, rather than optional trailing arguments the
            // checker cannot model.
            ("std/http", "send") => (
                1,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            ("std/http", "head") => (
                1,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            ("std/http", "post") => (
                2,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            ("std/http", "put") => (
                2,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            ("std/http", "patch") => (
                2,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            ("std/http", "del") => (
                1,
                stdlib_named("http", "Response"),
                stdlib_named("http", "HttpError"),
                true,
            ),
            ("std/fs", "read_text") => (
                1,
                Ty::Prim(Primitive::String),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "write_text") => (
                2,
                Ty::Prim(Primitive::Void),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "append_text") => (
                2,
                Ty::Prim(Primitive::Void),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "make_dir") => (
                1,
                Ty::Prim(Primitive::Void),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "remove") => (
                1,
                Ty::Prim(Primitive::Void),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "read_dir") => (
                1,
                self.stdlib_array_ty(Ty::Prim(Primitive::String))?,
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "stat") => (
                1,
                stdlib_named("fs", "FileInfo"),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "read_bytes") => (
                1,
                stdlib_named("bytes", "Bytes"),
                stdlib_named("fs", "FsError"),
                false,
            ),
            ("std/fs", "write_bytes") | ("std/fs", "append_bytes") => (
                2,
                Ty::Prim(Primitive::Void),
                stdlib_named("fs", "FsError"),
                false,
            ),
            // Every `std/bytes` entry that can fail does so for the same reason:
            // the input is not the thing it claims to be. `from_array` over a
            // 256, `to_text` over a PNG, `from_hex` over a typo. A silent
            // truncation is what the alternative would be, so each is a
            // `Result` and Glyph holds the caller to matching it.
            ("std/bytes", "from_array")
            | ("std/bytes", "from_hex")
            | ("std/bytes", "from_base64")
            | ("std/bytes", "from_base64url")
            | ("std/bytes", "from_base32") => (
                1,
                stdlib_named("bytes", "Bytes"),
                stdlib_named("bytes", "BytesError"),
                false,
            ),
            // Async, and it resolves when the server stops rather than when it
            // starts, so `Err` is how a port already in use arrives. Modeled so
            // a caller that forgets to match the failure is E0200 rather than a
            // silently ignored bind error.
            // Resolves when the socket is bound, so `Ok` means the port is yours.
            ("std/websocket", "listen") => (
                3,
                stdlib_named("websocket", "Server"),
                stdlib_named("net", "ServerError"),
                true,
            ),
            // Resolves when the socket is bound, so `Ok` means the port is yours.
            // The error is structured: `in_use` and `denied` lead to different
            // decisions, and scraping that out of a message string is what
            // `ServerError` exists to avoid.
            // Node's HTTP server is a TCP server, so this hands back the same
            // `net.Server` and is stopped by the same `net.stop`.
            ("std/http", "listen") => (
                3,
                stdlib_named("net", "Server"),
                stdlib_named("net", "ServerError"),
                true,
            ),
            ("std/net", "listen") => (
                3,
                stdlib_named("net", "Server"),
                stdlib_named("net", "ServerError"),
                true,
            ),
            ("std/url", "parse") => (
                1,
                stdlib_named("url", "Url"),
                Ty::Prim(Primitive::String),
                false,
            ),
            // `join(base, relative)`, so two.
            ("std/url", "join") => (
                2,
                stdlib_named("url", "Url"),
                Ty::Prim(Primitive::String),
                false,
            ),
            ("std/url", "decode_component") => (
                1,
                Ty::Prim(Primitive::String),
                Ty::Prim(Primitive::String),
                false,
            ),
            // Every lookup is async and every one fails for ordinary reasons, so
            // the caller is held to matching them rather than being handed a
            // throw from a name resolution.
            ("std/dns", "lookup") => (1, Ty::Prim(Primitive::String), Ty::Prim(Primitive::String), true),
            ("std/dns", "ipv4") | ("std/dns", "ipv6") | ("std/dns", "text") => (
                1,
                self.stdlib_array_ty(Ty::Prim(Primitive::String))?,
                Ty::Prim(Primitive::String),
                true,
            ),
            ("std/dns", "mail") => (
                1,
                self.stdlib_array_ty(stdlib_named("dns", "MailHost"))?,
                Ty::Prim(Primitive::String),
                true,
            ),
            // Resolves after the handshake, so an `Ok` means the peer's
            // certificate was accepted. Three arguments: the deadline is
            // required, because a dial with no bound can hang forever with no
            // handle to abort it.
            ("std/tls", "connect") => (
                3,
                stdlib_named("net", "Socket"),
                Ty::Prim(Primitive::String),
                true,
            ),
            ("std/bytes", "to_text") => (
                1,
                Ty::Prim(Primitive::String),
                stdlib_named("bytes", "BytesError"),
                false,
            ),
            _ => return None,
        };
        let return_ty = self.stdlib_result_ty(ok, err)?;
        let params = (0..arity)
            .map(|_| FnParam {
                name: None,
                owned: false,
                ty: Ty::Unknown,
                optional: false,
            })
            .collect();
        Some(Ty::Fn {
            params,
            return_ty: Arc::new(return_ty),
            is_async,
        })
    }

    /// The signature of a `std/string` function whose arity is fixed.
    ///
    /// Returns only: every parameter stays `Unknown`, matching the invariant the
    /// rest of this table keeps, so modeling `std/string` introduces no new
    /// argument-type diagnostic. The value is the return — `string.split(s,
    /// ",")` is now decidably an `Array<string>`, which is what lets a
    /// two-binding `for` over it bind a numeric index instead of falling back to
    /// the record (`Object.entries`) lowering, and what lets a `let` bound to it
    /// carry an element type forward without a hand-written annotation.
    ///
    /// Deliberately absent: `slice`, `index_of`, `pad_start`, `pad_end`. Each
    /// takes an optional trailing argument, and `Expr::Call` reports E0213
    /// whenever `params.len() != args.len()`, so modeling them here would report
    /// a false arity error on every call that omits the last argument. They are
    /// modeled once that check understands a minimum and a maximum.
    fn stdlib_string_fn_ty(&self, module_key: &str, field: &str) -> Option<Ty> {
        if module_key != "std/string" {
            return None;
        }
        let string = || Ty::Prim(Primitive::String);
        let (arity, ret): (usize, Ty) = match field {
            "from" => (1, string()),
            "join" => (2, string()),
            "split" => (2, self.stdlib_array_ty(string())?),
            "len" => (1, Ty::Prim(Primitive::Number)),
            "trim" | "trim_start" | "trim_end" | "lower" | "upper" => (1, string()),
            "contains" | "starts_with" | "ends_with" => (2, Ty::Prim(Primitive::Bool)),
            "repeat" => (2, string()),
            "replace_all" => (3, string()),
            // The three with a trailing optional argument. `index_of` is the one
            // G39 was really about: unmodeled, its `Option<number>` was
            // `Unknown`, so a `match` over it skipped D9 exhaustiveness and a
            // missing `None` arm threw at run time on a clean build.
            "index_of" => {
                return Some(Ty::Fn {
                    params: vec![
                        required(Ty::Unknown),
                        required(Ty::Unknown),
                        optional(Ty::Unknown),
                    ],
                    return_ty: Arc::new(
                        self.stdlib_option_ty(Ty::Prim(Primitive::Number))?,
                    ),
                    is_async: false,
                })
            }
            "slice" => {
                return Some(Ty::Fn {
                    params: vec![
                        required(Ty::Unknown),
                        required(Ty::Unknown),
                        optional(Ty::Unknown),
                    ],
                    return_ty: Arc::new(string()),
                    is_async: false,
                })
            }
            "pad_start" | "pad_end" => {
                return Some(Ty::Fn {
                    params: vec![
                        required(Ty::Unknown),
                        required(Ty::Unknown),
                        optional(Ty::Unknown),
                    ],
                    return_ty: Arc::new(string()),
                    is_async: false,
                })
            }
            _ => return None,
        };
        Some(Ty::Fn {
            params: unknown_params(arity),
            return_ty: Arc::new(ret),
            is_async: false,
        })
    }

    /// The signature of a `std/array` function whose arity is fixed.
    ///
    /// The element type travels as a `Ty::Param("T")`: `collect_type_param_bindings`
    /// binds it from the argument (`Array<string>` against `Array<T>` gives `T =
    /// string`) and `substitute_type_params` rewrites the return, so
    /// `array.filter(names, is_short)` is an `Array<string>` with no new
    /// machinery. `T` is placed on a parameter only where the *return* needs it;
    /// every other parameter stays `Unknown`, so this adds no argument-type
    /// diagnostic beyond "the first argument of an array function is an array".
    /// An `Unknown` argument leaves `T` unbound, which still leaves the return an
    /// `Array` — enough for the `for` lowering.
    ///
    /// `map`, `flat_map`, and `zip` carry a *second* parameter `U`, which comes
    /// from the callback's return rather than from any argument's own type.
    /// `collect_type_param_bindings` walks into `Ty::Fn` on both sides, so
    /// `array.map(names, dup)` binds `T = string` from parameter 0 and `U =
    /// string` from the callback's return.
    ///
    /// Writing the callback as a *synchronous* `fn(T) -> U` is the point of
    /// modeling them, not a limitation of it. D40 holds `fn` and `async fn`
    /// apart, and `xs.map(async_f)` is an `Array<Promise<U>>` in JavaScript, so
    /// an unmodeled `map` let `array.map(xs, some_async_fn)` compile clean, pass
    /// `tsc --strict`, and print `[object Promise]` — the result was `Unknown`
    /// and `string.from` takes an `unknown`. That silent green is what an
    /// unmodeled signature bought, and it is G99.
    ///
    /// This paragraph described those three arms as present for eight releases
    /// while none of them existed, which is how the gap survived: whoever
    /// checked read the comment and stopped. If you remove an arm, remove its
    /// sentence in the same edit.
    ///
    /// The async spelling is `std/task`: map to an `Array<async fn() -> T>` and
    /// hand it to `task.all`, which is the example D40 itself uses.
    fn stdlib_array_fn_ty(&self, module_key: &str, field: &str) -> Option<Ty> {
        if module_key != "std/array" {
            return None;
        }
        let elem = || Ty::Param {
            name: Ident::from("T"),
            owner: ParamOwner::Unresolved,
        };
        let xs = self.stdlib_array_ty(elem())?;
        let unknown = || FnParam {
            name: None,
            owned: false,
            ty: Ty::Unknown,
                optional: false,
        };
        let of = |ty: Ty| FnParam {
            name: None,
            owned: false,
            ty,
            optional: false,
        };
        // A trailing argument the caller may omit. Modeling these is what lets
        // the six stdlib functions that take one be modeled at all (G39).
        #[allow(unused)]
        let opt = |ty: Ty| FnParam {
            name: None,
            owned: false,
            ty,
            optional: true,
        };
        // `fn(T) -> bool`, the shape `filter`, `find`, and `any` all take.
        let pred = || Ty::Fn {
            params: vec![FnParam {
                name: None,
                owned: false,
                ty: elem(),
                optional: false,
            }],
            return_ty: Arc::new(Ty::Prim(Primitive::Bool)),
            is_async: false,
        };
        // `U`, the callback's own return type, for the three functions whose
        // result element differs from their input's.
        let out = || Ty::Param {
            name: Ident::from("U"),
            owner: ParamOwner::Unresolved,
        };
        // `fn(T) -> U`. Synchronous on purpose: see the note above about what an
        // `async fn` passed here used to do.
        let mapper = |from: Ty, to: Ty| Ty::Fn {
            params: vec![FnParam {
                name: None,
                owned: false,
                ty: from,
                optional: false,
            }],
            return_ty: Arc::new(to),
            is_async: false,
        };
        let ys = self.stdlib_array_ty(out())?;
        let (params, ret): (Vec<FnParam>, Ty) = match field {
            "len" => (unknown_params(1), Ty::Prim(Primitive::Number)),
            // The three the comment above has described as modeled since before
            // 0.1.72 while none of them was. An `async fn` callback is now
            // rejected at the argument instead of producing an `Array<Promise<U>>`
            // that `string.from` renders as `[object Promise]` (G99).
            "map" => (vec![of(xs.clone()), of(mapper(elem(), out()))], ys),
            "flat_map" => (
                vec![of(xs.clone()), of(mapper(elem(), ys.clone()))],
                ys,
            ),
            "any" => (vec![of(xs.clone()), of(pred())], Ty::Prim(Primitive::Bool)),
            "contains" => (unknown_params(2), Ty::Prim(Primitive::Bool)),
            "index_of" => (
                unknown_params(2),
                self.stdlib_option_ty(Ty::Prim(Primitive::Number))?,
            ),
            "reverse" => (vec![of(xs.clone())], xs),
            "push" | "concat" => (vec![of(xs.clone()), unknown()], xs),
            "filter" => (vec![of(xs.clone()), of(pred())], xs),
            "sort" => (
                vec![
                    of(xs.clone()),
                    of(Ty::Fn {
                        params: vec![of(elem()), of(elem())],
                        return_ty: Arc::new(Ty::Prim(Primitive::Number)),
                        is_async: false,
                    }),
                ],
                xs,
            ),
            "find" => (
                vec![of(xs.clone()), of(pred())],
                self.stdlib_option_ty(elem())?,
            ),
            // `get(xs, i) -> Option<T>`: the element type rides on parameter 0
            // the same way `find`'s does, so the `Some(x)` binding of a match
            // over it carries a real type instead of `Unknown`.
            // The trailing-optional member of `std/array`, modelable now that the
            // arity check understands a minimum and a maximum (G39).
            "slice" => {
                return Some(Ty::Fn {
                    params: vec![
                        of(xs.clone()),
                        required(Ty::Prim(Primitive::Number)),
                        optional(Ty::Prim(Primitive::Number)),
                    ],
                    return_ty: Arc::new(xs),
                    is_async: false,
                })
            }
            "get" => (
                vec![of(xs.clone()), of(Ty::Prim(Primitive::Number))],
                self.stdlib_option_ty(elem())?,
            ),
            "fold" => {
                let acc = Ty::Param {
                    name: Ident::from("A"),
                    owner: ParamOwner::Unresolved,
                };
                (
                    vec![
                        of(xs.clone()),
                        of(acc.clone()),
                        of(Ty::Fn {
                            params: vec![of(acc.clone()), of(elem())],
                            return_ty: Arc::new(acc.clone()),
                            is_async: false,
                        }),
                    ],
                    acc,
                )
            }
            // The five reductions (G100). `max`/`min`/`max_by`/`min_by` are
            // `Option`-returning because an empty array has no maximum, so
            // modeling them is what turns the empty case into a `None` arm the
            // exhaustiveness checker requires instead of something the caller
            // can forget. `sum` is a plain `number`: the sum of no numbers is 0.
            //
            // `max_by`/`min_by` take the element type from parameter 0 the way
            // `find` does, so the `Some(x)` binding is the array's element and
            // not `Unknown`, and the key callback is a synchronous `fn(T) ->
            // number` for the same reason `map`'s is (see the note above).
            "sum" => (
                vec![of(self.stdlib_array_ty(Ty::Prim(Primitive::Number))?)],
                Ty::Prim(Primitive::Number),
            ),
            "max" | "min" => (
                vec![of(self.stdlib_array_ty(Ty::Prim(Primitive::Number))?)],
                self.stdlib_option_ty(Ty::Prim(Primitive::Number))?,
            ),
            "max_by" | "min_by" => (
                vec![
                    of(xs.clone()),
                    of(mapper(elem(), Ty::Prim(Primitive::Number))),
                ],
                self.stdlib_option_ty(elem())?,
            ),
            _ => return None,
        };
        Some(Ty::Fn {
            params,
            return_ty: Arc::new(ret),
            is_async: false,
        })
    }

    /// The signature of a `std/record` function. All six are fixed-arity.
    ///
    /// The value type travels as a `Ty::Param("V")` on parameter 0, the same
    /// mechanism `stdlib_array_fn_ty` uses for `T`: the argument's
    /// `Record<string, Array<string>>` binds `V = Array<string>`, so
    /// `record.get(t, k)` is decidably an `Option<Array<string>>` and the
    /// `Some(p)` binding of a `match` over it carries an element type. Without
    /// that, a `for i, hop in p` reads an `Unknown` iterable and silently takes
    /// the `Object.entries` lowering, binding `i` to the string `"0"`.
    ///
    /// The key is always `string`, so it is not a parameter. Every parameter
    /// slot that is not `V` stays `Unknown`, per this table's rule, so modeling
    /// `std/record` introduces no new argument-type diagnostic.
    fn stdlib_record_fn_ty(&self, module_key: &str, field: &str) -> Option<Ty> {
        if module_key != "std/record" {
            return None;
        }
        let value = || Ty::Param {
            name: Ident::from("V"),
            owner: ParamOwner::Unresolved,
        };
        let rec = self.stdlib_record_ty(Ty::Prim(Primitive::String), value())?;
        let unknown = || FnParam {
            name: None,
            owned: false,
            ty: Ty::Unknown,
                optional: false,
        };
        let of = |ty: Ty| FnParam {
            name: None,
            owned: false,
            ty,
            optional: false,
        };
        // A trailing argument the caller may omit. Modeling these is what lets
        // the six stdlib functions that take one be modeled at all (G39).
        #[allow(unused)]
        let opt = |ty: Ty| FnParam {
            name: None,
            owned: false,
            ty,
            optional: true,
        };
        let (params, ret): (Vec<FnParam>, Ty) = match field {
            "get" => (
                vec![of(rec), unknown()],
                self.stdlib_option_ty(value())?,
            ),
            "has" => (unknown_params(2), Ty::Prim(Primitive::Bool)),
            "keys" => (
                unknown_params(1),
                self.stdlib_array_ty(Ty::Prim(Primitive::String))?,
            ),
            "values" => (
                vec![of(rec)],
                self.stdlib_array_ty(value())?,
            ),
            "set" => (vec![of(rec.clone()), unknown(), unknown()], rec),
            "remove" => (vec![of(rec.clone()), unknown()], rec),
            _ => return None,
        };
        Some(Ty::Fn {
            params,
            return_ty: Arc::new(ret),
            is_async: false,
        })
    }

    /// Build `Result<ok, err>` as a prelude `App` the `?` checker recognizes
    /// (`prelude_app` keys off the prelude `Result` symbol id). Returns `None`
    /// only if the prelude somehow lacks `Result`, which never happens.
    fn stdlib_result_ty(&self, ok: Ty, err: Ty) -> Option<Ty> {
        let result_id = self.lowerer.prelude.lookup("Result")?;
        Some(Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(result_id.0),
                path: vec![Ident::from("Result")],
            }),
            args: vec![ok, err],
        })
    }

    /// Build `Option<inner>` as a prelude `App` the exhaustiveness checker
    /// recognizes (it keys off the prelude `Option` symbol id). Mirrors
    /// `stdlib_result_ty`.
    fn stdlib_option_ty(&self, inner: Ty) -> Option<Ty> {
        let option_id = self.lowerer.prelude.lookup("Option")?;
        Some(Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(option_id.0),
                path: vec![Ident::from("Option")],
            }),
            args: vec![inner],
        })
    }

    /// Build `Array<inner>` as a prelude `App`. Mirrors `stdlib_option_ty`.
    fn stdlib_array_ty(&self, inner: Ty) -> Option<Ty> {
        let array_id = self.lowerer.prelude.lookup("Array")?;
        Some(Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(array_id.0),
                path: vec![Ident::from("Array")],
            }),
            args: vec![inner],
        })
    }

    /// Build `Record<key, value>` as a prelude `App`. Mirrors `stdlib_array_ty`.
    fn stdlib_record_ty(&self, key: Ty, value: Ty) -> Option<Ty> {
        let record_id = self.lowerer.prelude.lookup("Record")?;
        Some(Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(record_id.0),
                path: vec![Ident::from("Record")],
            }),
            args: vec![key, value],
        })
    }

    // ----- day-15: `?` operator typing rule -----

    /// Build the `EnclosingReturn` for a declared return type: its
    /// `ReturnClass` (for the `?` rule) and its lowered `Ty` (for
    /// return-type mismatch checking). Both err toward *permissive* — a
    /// missing annotation (legal under D4) or one that can't be resolved
    /// yields `ReturnClass::Unknown` and `Ty::Unknown`, so neither check
    /// fires on a type it can't judge.
    fn enclosing_return(&self, return_ty: Option<&TypeExpr>, is_async: bool) -> EnclosingReturn {
        let Some(te) = return_ty else {
            return EnclosingReturn { class: ReturnClass::Unknown, ty: Ty::Unknown, is_async };
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
        EnclosingReturn { class, ty, is_async }
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
            Some(ResolvedRef::Module(id)) => {
                match self.resolved.symbols.table.get(id).map(|s| &s.kind) {
                    Some(SymbolKind::ImportNamed { original, .. }) => {
                        original.as_ref() == "Result"
                    }
                    // `result.Result<T, E>`, and the same through an alias. The
                    // lowerer now gives this the prelude `Ty::Named`, which makes
                    // it decidable, so without this arm the `?` rule reads it as a
                    // decidably non-`Result` return and rejects every `?` in the
                    // function body.
                    Some(SymbolKind::ImportNamespace { path })
                    | Some(SymbolKind::ImportAlias { path, .. }) => {
                        segments.len() == 2
                            && path.segments.iter().map(|s| s.as_ref()).eq(["std", "result"])
                    }
                    _ => false,
                }
            }
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
            // A sibling's `pub type Res = Result<A, B>` *is* a `Result`, and
            // this module cannot see through the alias to know it. Judging it
            // non-`Result` here would reject every `?` in a function declared
            // with that return type. Same reasoning as `ty_is_decidable`:
            // having an identity is not the same as having a definition.
            Ty::Imported { .. } => false,
            Ty::App { base, .. } => !matches!(base.as_ref(), Ty::Unknown),
            Ty::Param { .. } => false,
            _ => true,
        }
    }

    /// Check a `?` expression and return the type it evaluates to: the
    /// operand's success type `T` when the operand is a `Result<T, E>`,
    /// else `Unknown`. Three rules (week-3 task 2), each erring toward
    /// permissive so none fires on a type it cannot judge:
    /// - enclosing-fn side: the innermost callable must (provably) return
    ///   `Result`, else `QuestionOutsideResultFn`. An empty stack is a `?`
    ///   in a `const` initializer with no enclosing callable, always an
    ///   error.
    /// - operand side: a decidably non-`Result` operand is
    ///   `QuestionOnNonResult`.
    /// - error-type side: the operand's `E` must equal the enclosing
    ///   function's `E` exactly (no `From` in v1), else
    ///   `QuestionErrorTypeMismatch`. Judged only when the enclosing return
    ///   is a decidable `Result` and both `E`s are decidable.
    fn check_question_operator(&mut self, span: Span, operand_ty: &Ty) -> Ty {
        let enclosing = self.return_stack.last().cloned();
        let permitted = matches!(
            enclosing.as_ref().map(|e| e.class),
            Some(ReturnClass::Result | ReturnClass::Unknown)
        );
        if !permitted {
            self.errors.push(TypeError::QuestionOutsideResultFn { span });
        }

        match self.result_args(operand_ty) {
            Some((ok_ty, err_ty)) => {
                // Operand IS a `Result`: its `E` must match the enclosing
                // function's `E` exactly. Only judged when the enclosing
                // return is a decidable `Result` and both error types are
                // decidable, so an undecidable side never produces a false
                // positive.
                if let Some(EnclosingReturn { class: ReturnClass::Result, ty, .. }) = &enclosing {
                    if let Some((_, fn_err)) = self.result_args(ty) {
                        if ty_is_decidable(&err_ty) && ty_is_decidable(&fn_err) && err_ty != fn_err {
                            self.errors.push(TypeError::QuestionErrorTypeMismatch {
                                expected: ty_display(&fn_err),
                                found: ty_display(&err_ty),
                                span,
                            });
                        }
                    }
                }
                ok_ty
            }
            None => {
                if ty_is_decidable(operand_ty) {
                    self.errors.push(TypeError::QuestionOnNonResult {
                        found: ty_display(operand_ty),
                        span,
                    });
                }
                Ty::Unknown
            }
        }
    }

    /// If `ty` is a prelude `Result<T, E>`, return `(T, E)` (cloned). The
    /// `?`-specific reader over `prelude_union`: `Option` and every other
    /// type return None, since only `Result` is `?`-compatible. A missing
    /// type argument (an under-applied `Result`) reads as `Unknown`.
    fn result_args(&self, ty: &Ty) -> Option<(Ty, Ty)> {
        match self.prelude_union(ty)? {
            ("Result", args) => Some((
                args.first().cloned().unwrap_or(Ty::Unknown),
                args.get(1).cloned().unwrap_or(Ty::Unknown),
            )),
            _ => None,
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
        if self.assign_incompatible(&found, &expected) {
            self.errors.push(TypeError::TypeMismatch {
                expected: ty_display(&expected),
                found: ty_display(&found),
                span: value.span(),
            });
        }
    }

    // ----- match coverage: the writes the dispatch makes as it goes -----

    /// Enter a coverage scope: the site `at` names, opening one when this is a
    /// top-level entry whose union can be named.
    ///
    /// `None` switches every write below it off, which is the answer for a
    /// union with neither a declaration nor a builtin name: the relation is
    /// keyed by type, so a site with no type end has nowhere to be filed and
    /// is better absent than filed under an invented name.
    fn cover_enter(
        &mut self,
        at: Option<CoverageAt>,
        union: Option<CoverageTypeName>,
        match_span: Span,
    ) -> Option<CoverageWriter> {
        let (at, union) = (at?, union?);
        match at {
            CoverageAt::Payload { site, depth } => Some(CoverageWriter { site, depth, union }),
            CoverageAt::Entry(scrutinee_span) => {
                let site = self
                    .coverage
                    .open(union.clone(), scrutinee_span, match_span, true);
                Some(CoverageWriter {
                    site,
                    depth: 0,
                    union,
                })
            }
        }
    }

    fn cover_mention(&mut self, w: Option<&CoverageWriter>, arm: u16, variant: &str) {
        let Some(w) = w else { return };
        self.coverage.record_mention(
            w.site,
            CoverageMention {
                arm,
                depth: w.depth,
                union: w.union.clone(),
                variant: variant.to_string(),
            },
        );
    }

    fn cover_decline(&mut self, w: Option<&CoverageWriter>, arm: u16, variant: Option<&str>) {
        let Some(w) = w else { return };
        self.coverage.record_decline(
            w.site,
            CoverageDecline {
                arm,
                depth: w.depth,
                variant: variant.map(str::to_string),
            },
        );
    }

    fn cover_catch_all(&mut self, w: Option<&CoverageWriter>, arm: u16) {
        let Some(w) = w else { return };
        self.coverage.record_catch_all(
            w.site,
            CoverageCatchAll {
                arm,
                depth: w.depth,
            },
        );
    }

    fn cover_gap(&mut self, w: Option<&CoverageWriter>, missing: Vec<String>) {
        let Some(w) = w else { return };
        self.coverage.record_gap(
            w.site,
            CoverageGap {
                depth: w.depth,
                union: w.union.clone(),
                missing,
            },
        );
    }

    /// The declaration a string-literal-union site was reached through,
    /// resolved the way `string_literal_union_values` resolved the values: the
    /// alias declared here, the alias imported from a sibling, or nothing at
    /// all for a literal set written inline, which is declared nowhere.
    ///
    /// Answers in the diagnostic view because that is the one that keeps local
    /// and imported apart; `coverage_name` derives the edge's key from it.
    fn string_literal_union_ref(&self, ty: &Ty) -> Option<DiagnosticUnion> {
        match ty {
            Ty::Imported { module, name } => Some(DiagnosticUnion::Imported {
                module: module.as_str().to_string(),
                name: name.to_string(),
            }),
            Ty::Named { symbol, .. } => {
                let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
                Some(DiagnosticUnion::Local {
                    name: sym.name.to_string(),
                })
            }
            _ => None,
        }
    }

    /// The module key this file is known by, or the empty string when it
    /// declares no `module` line. Empty is not a key anything resolves, which
    /// is the honest answer for that file: its types have names and no
    /// address.
    fn own_module_key(&self) -> String {
        self.module
            .module_path
            .as_ref()
            .map(|p| crate::lower::module_key(p).as_str().to_string())
            .unwrap_or_default()
    }

    /// Record the one fact a union-shaped scrutinee this module cannot read
    /// leaves behind: there is a match over it and the checker concluded
    /// nothing. Only an imported scrutinee gets a site, because only it has a
    /// module and a name; a scrutinee whose type never resolved at all has
    /// nothing to key a site to.
    fn cover_unresolved_site(&mut self, ty: &Ty, scrutinee_span: Span, match_span: Span) {
        let Ty::Imported { module, name } = union_base(ty) else {
            return;
        };
        let named = CoverageTypeName::Declared {
            module: module.as_str().to_string(),
            name: name.to_string(),
        };
        self.coverage.open(named, scrutinee_span, match_span, false);
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
    /// **Bare-head classification**: a `Pattern::Ident { name }` is resolved
    /// by shape. A lowercase/underscore-led name is a binding (an irrefutable
    /// catch-all); a PascalCase name is a variant reference. A PascalCase head
    /// that names a variant covers it; one that names no variant of the union
    /// is a typo (`Loadign` for `Loading`) or a wrong-union variant, and is
    /// escalated to `UnknownVariantPattern` (E0220) with a nearest-variant
    /// suggestion rather than being read as a silent catch-all. This is the
    /// module-local, decidable-scrutinee case; cross-module/imported unions
    /// are out of scope (see `docs/dogfooding-gaps.md`).
    ///
    /// Each branch below that resolves a union-shaped scrutinee also opens a
    /// coverage site and fills it as it counts. `scrutinee_span` is carried for
    /// that and nothing else: it is what an answer locates a site by, since the
    /// within-file site index never leaves the computation that produced it.
    fn check_match_exhaustiveness(
        &mut self,
        scrutinee_ty: &Ty,
        scrutinee_span: glyph_ast::Span,
        arms: &[MatchArm],
        match_span: glyph_ast::Span,
    ) {
        // Reachability runs first, independent of the scrutinee's kind: an arm
        // after an irrefutable arm is dead code under first-match-wins (D9),
        // and the emitter's `default`-based lowering of a leading binding
        // catch-all would otherwise let a shadowed later `case` win at runtime.
        self.check_arm_reachability(scrutinee_ty, arms);
        if self.is_prelude_array(scrutinee_ty) {
            self.check_array_exhaustiveness(arms, match_span);
            return;
        }
        // A `bool` match, either by the scrutinee's statically-known type or —
        // when the scrutinee is a comparison/boolean expression that types as
        // `Unknown` — recovered from the arms: a `true`/`false` literal pattern
        // only type-checks over a bool, so its presence pins this as a bool
        // match. Mirrors the arm-driven union recovery in the `Expr::Match`
        // handler; without it `match n > 0 { true => .. }` (missing `false`)
        // slipped past exhaustiveness and threw at run time.
        let arms_are_bool = arms.iter().any(|a| {
            matches!(
                a.pattern,
                Pattern::Literal {
                    value: LiteralPattern::Bool(_),
                    ..
                }
            )
        });
        if matches!(scrutinee_ty, Ty::Prim(Primitive::Bool)) || arms_are_bool {
            self.check_bool_exhaustiveness(arms, match_span);
            return;
        }
        // A string-literal union (`"free" | "pro"`, D30) is a *bounded* string
        // domain, so a `match` covering every literal is exhaustive without a
        // catch-all (unlike an unbounded `string` below). Resolves a named alias
        // (`type Tier = "free" | "pro"`) to its literal set.
        if let Some(values) = self.string_literal_union_values(scrutinee_ty) {
            self.check_string_literal_union_exhaustiveness(
                &values,
                arms,
                match_span,
                scrutinee_ty,
                Some(CoverageAt::Entry(scrutinee_span)),
            );
            return;
        }
        // A `number`/`string` match: those domains are unbounded, so literal arms
        // can never be exhaustive without a catch-all. Detect by the scrutinee's
        // static type, or — when it types as `Unknown` — recover from a literal
        // arm (a number/string literal pattern only type-checks over that
        // primitive), mirroring the bool recovery just above.
        let value_kind = match scrutinee_ty {
            Ty::Prim(Primitive::Number) => Some("number"),
            Ty::Prim(Primitive::String) => Some("string"),
            _ => arms.iter().find_map(|a| match &a.pattern {
                Pattern::Literal {
                    value: LiteralPattern::Number(_),
                    ..
                } => Some("number"),
                Pattern::Literal {
                    value: LiteralPattern::String(_),
                    ..
                } => Some("string"),
                _ => None,
            }),
        };
        if let Some(kind) = value_kind {
            self.check_value_exhaustiveness(kind, arms, match_span);
            return;
        }
        // Imported union: the scrutinee's union decl lives in another module, so
        // every local resolution above found nothing — it is either `Unknown`
        // (no annotation to lower) or an imported type whose declaration this
        // module never sees. If an arm names an imported variant, resolve the
        // union's full variant set cross-module and hold the match to the same
        // exhaustiveness bar as a module-local one. (Reachability, above,
        // already treats a PascalCase bare ident as a refutable variant rather
        // than a catch-all.)
        //
        // The test is on the *base* of the scrutinee: an imported union at a
        // concrete instantiation (`Tree<string>`) lowers to `Ty::App` over the
        // `Ty::Imported`, and reading the application itself found neither
        // arm of the pattern. Adding a type parameter to the declaration then
        // turned this E0200 into a runtime `non-exhaustive match`. A union's
        // arity is not something the exhaustiveness bar may depend on, the
        // same rule `resolve_named_union` applies on the module-local side.
        if matches!(union_base(scrutinee_ty), Ty::Unknown | Ty::Imported { .. }) {
            if let Some((module, type_name, required)) =
                self.imported_union_variants_from_arms(arms)
            {
                let patterns = arm_patterns(arms);
                self.check_imported_union_coverage(
                    &module,
                    &type_name,
                    &required,
                    &patterns,
                    match_span,
                    Some(CoverageAt::Entry(scrutinee_span)),
                );
                return;
            }
        }

        // Nothing above claimed this scrutinee, so there is no variant set,
        // no length set and no bounded value domain to count against. One
        // thing is still decidable without a usefulness algorithm, and it is
        // the one that matters: if every arm can fail and no arm is a
        // catch-all, the chain the emitter builds falls off its end.
        if self.required_variants(scrutinee_ty).is_none() {
            self.check_refutable_arms_have_a_catch_all(arms, match_span);
            // Union-shaped and unresolvable: an imported scrutinee whose
            // declaration this module cannot read as a union, and whose arms
            // named no variant of it either. It still has a name, so the
            // relation records the site with the one thing the checker
            // concluded, which is nothing. The alternative is a match that
            // nobody can see is going unchecked.
            self.cover_unresolved_site(scrutinee_ty, scrutinee_span, match_span);
        }

        let patterns = arm_patterns(arms);
        self.check_patterns_exhaustive(
            scrutinee_ty,
            &patterns,
            match_span,
            Some(CoverageAt::Entry(scrutinee_span)),
        );
    }

    /// A `match` over a scrutinee with no variant set: `{ x: 0, y: y }` over a
    /// record, or over an imported type this module cannot resolve. Such a
    /// match is exhaustive only if some arm always matches, so an arm set that
    /// is refutable throughout is `E0226`.
    ///
    /// Scoped to a match that contains a refutable object pattern, which is the
    /// only way (D44) to write a top-level arm here that can fail: an array,
    /// bool, literal or union scrutinee is claimed by one of the checks above,
    /// and widening this to every refutable arm shape would start reporting
    /// matches whose scrutinee simply went unresolved.
    ///
    /// It asks for a catch-all where a reader can sometimes see there is
    /// nothing left (`{ flag: true, .. }` beside `{ flag: false, .. }`). That is
    /// the same conservative reading D44 already takes one level down, and it
    /// relaxes the day coverage is proved over a product of fields rather than
    /// a set of tags. The alternative is accepting a match that throws.
    fn check_refutable_arms_have_a_catch_all(
        &mut self,
        arms: &[MatchArm],
        match_span: glyph_ast::Span,
    ) {
        let tests_a_field = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Object { .. }) && a.pattern.is_refutable());
        if !tests_a_field {
            return;
        }
        // `Scrutinee::Record`: the guard above established that these arms
        // destructure a record, which is the one scrutinee an all-binding
        // object pattern absorbs.
        if arms
            .iter()
            .any(|a| is_catch_all_pattern(&a.pattern, Scrutinee::Record))
        {
            return;
        }
        self.errors
            .push(TypeError::NonExhaustiveFieldMatch { span: match_span });
    }

    /// Resolve an imported union's `(type name, variant set)` from the match
    /// arms: find the first arm naming an imported variant, follow its
    /// `ImportNamed` symbol to the source module, and ask the resolver for the
    /// union that owns it. `None` when no arm names a resolvable imported variant.
    ///
    /// A qualified arm (`model.Yes(_)`, or `m.Yes(_)` through
    /// `import model as m`) is resolved through its *head* instead: the variant
    /// name is not a symbol under a namespace import, so the `ImportNamed`
    /// lookup below can never find it and the whole match went unchecked.
    /// Returns the source module alongside the type name and variant set so a
    /// caller that needs to resolve a payload deeper (`check_imported_union_coverage`'s
    /// nested-pattern recursion) can ask the same module for a different type
    /// without re-deriving it from the arms a second time.
    fn imported_union_variants_from_arms(
        &self,
        arms: &[MatchArm],
    ) -> Option<(ModuleKey, String, Vec<Ident>)> {
        for arm in arms {
            if let Pattern::Constructor { path, .. } = &arm.pattern {
                if path.len() >= 2 {
                    if let Some(found) = self.qualified_union_variants(path) {
                        return Some(found);
                    }
                }
            }
            let variant = match &arm.pattern {
                Pattern::Constructor { path, .. } => path.last(),
                // The shared shape check, not a hand-rolled copy of it. There
                // is no variant set to consult here — finding it is what this
                // function is for — so shape is the whole test.
                Pattern::Ident { name, .. } if is_constructor_shaped(name) => Some(name),
                _ => None,
            };
            let Some(variant) = variant else { continue };
            let Some(&sym_id) = self.resolved.symbols.by_name.get(variant) else {
                continue;
            };
            let Some(sym) = self.resolved.symbols.table.get(sym_id) else {
                continue;
            };
            let SymbolKind::ImportNamed { path, original } = &sym.kind else {
                continue;
            };
            let module_path = crate::lower::module_key(path);
            if let Some((type_name, variants)) = self
                .decl_ty_resolver
                .imported_union_of_variant(module_path.as_str(), original)
            {
                return Some((module_path, type_name, variants));
            }
        }
        None
    }

    /// The `(module, type name, variant set)` a qualified constructor pattern
    /// names, when its head is a namespace import of a project sibling:
    /// `model.Yes(_)` resolves `model` to its `ImportNamespace`/`ImportAlias`
    /// path and asks the resolver which union in that module declares `Yes`.
    /// `None` for a head that is not a namespace import, or a module that
    /// declares no such variant (a stdlib namespace such as `option` has no
    /// project file, so it falls through here and is typed by the lowerer
    /// instead).
    fn qualified_union_variants(&self, path: &[Ident]) -> Option<(ModuleKey, String, Vec<Ident>)> {
        let head = path.first()?;
        let variant = path.last()?;
        let &sym_id = self.resolved.symbols.by_name.get(head)?;
        let sym = self.resolved.symbols.table.get(sym_id)?;
        let import_path = match &sym.kind {
            SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. } => path,
            _ => return None,
        };
        let module_path = crate::lower::module_key(import_path);
        let (type_name, variants) = self
            .decl_ty_resolver
            .imported_union_of_variant(module_path.as_str(), variant)?;
        Some((module_path, type_name, variants))
    }

    /// Coverage check for a `match` on an imported union: every required variant
    /// must be covered by an arm, or a catch-all must absorb the rest. A variant
    /// covered only by a single-payload constructor sub-pattern (`B(X)`) is not
    /// enough on its own — the payload might itself be a tagged union with
    /// variants the arm never names — so that sub-pattern is checked
    /// recursively against the payload's own variant set, exactly as
    /// `check_patterns_exhaustive` recurses on the module-local side. The
    /// payload's declaration is resolved cross-module the same way the outer
    /// union was (`DeclTyResolver::imported_type_decl`), so nesting composes
    /// across a chain of imported unions of arbitrary depth, whether the inner
    /// union is a sibling declaration in the same source module or itself
    /// re-imported from a third one.
    ///
    /// Also emits `UnknownVariantPattern` for a constructor head that names no
    /// variant of the union — that arm used to be inserted into `covered`
    /// unexamined, so a misspelling reached `tsc` and came back as a raw
    /// `TS2678` instead of a Glyph diagnostic pointing at the arm.
    fn check_imported_union_coverage(
        &mut self,
        module: &ModuleKey,
        type_name: &str,
        required: &[Ident],
        patterns: &[(u16, &Pattern)],
        match_span: glyph_ast::Span,
        at: Option<CoverageAt>,
    ) {
        // The site these writes belong to: this entry's own, or the one a
        // payload recursion is already inside.
        let cov = self.cover_enter(
            at,
            Some(CoverageTypeName::Declared {
                module: module.as_str().to_string(),
                name: type_name.to_string(),
            }),
            match_span,
        );
        let mut covered: std::collections::HashSet<&str> = std::collections::HashSet::new();
        // Variants covered only via a nested constructor sub-pattern
        // (`B(X)`): the payload's own exhaustiveness is a separate question,
        // checked below by recursion, once it's known the payload's variant
        // set can even be resolved.
        let mut nested: HashMap<&str, Vec<(u16, &Pattern)>> = HashMap::new();
        let mut has_catch_all = false;
        let mut unknown: Vec<(String, glyph_ast::Span)> = Vec::new();
        for &(arm, pat) in patterns {
            // The same classification the module-local side runs, with the
            // imported union's own variant set as the context. What differs is
            // only what this side does with an unknown head, below.
            match classify_arm(pat, required) {
                ArmCoverage::CatchAll => {
                    has_catch_all = true;
                    self.cover_catch_all(cov.as_ref(), arm);
                }
                ArmCoverage::Mentions(v) => {
                    covered.insert(v.as_ref());
                    self.cover_mention(cov.as_ref(), arm, v.as_ref());
                }
                // A single payload sub-pattern names the *variant* here, but
                // whether it covers the payload's own variant set is a
                // question for the payload's type — deferred to the recursive
                // check below. Folding this straight into `covered` was the
                // shallow bug: `B(X)` over `Inner = X | Y` marked `B` fully
                // handled without ever looking at `Y`. The mention is still
                // recorded, because the arm did name `B`.
                ArmCoverage::Nests { variant, sub } => {
                    nested.entry(variant.as_ref()).or_default().push((arm, sub));
                    self.cover_mention(cov.as_ref(), arm, variant.as_ref());
                }
                ArmCoverage::UnknownVariant { name, span, bare } => {
                    if bare {
                        // A bare head naming no variant of this union has
                        // always been credited on this side, where the
                        // module-local twin reports E0220 for it. The insert
                        // cannot change the outcome (the name is not in
                        // `required`, so nothing it covers can be missing) and
                        // the diagnostic asymmetry is not this change's to
                        // fix, so it is left exactly as it stood — and the
                        // relation records no mention for it, because the arm
                        // named nothing this union declares.
                        covered.insert(name.as_ref());
                    } else {
                        unknown.push((name.to_string(), span));
                    }
                }
                ArmCoverage::Declined { variant } => {
                    let variant = variant.map(|v| v.to_string());
                    self.cover_decline(cov.as_ref(), arm, variant.as_deref());
                }
            }
        }
        for (name, span) in unknown {
            let suggestion = nearest_variant(&name, required);
            self.errors.push(TypeError::UnknownVariantPattern {
                union: type_name.to_string(),
                name,
                suggestion,
                span,
            });
        }
        if has_catch_all {
            return;
        }
        let mut missing: Vec<&str> = Vec::new();
        for v in required {
            let v = v.as_ref();
            if covered.contains(v) {
                continue;
            }
            match nested.get(v) {
                Some(subs) => {
                    // The variant IS present (a `V(...)` arm exists); recurse
                    // into its payload, when the payload is itself an
                    // imported union, to check the nested patterns. A payload
                    // this module can't resolve to a union (not exported as
                    // one, or not a union at all) makes this a no-op — the
                    // same conservative "skip, don't false-positive" the
                    // module-local recursion takes when `variant_payload`
                    // comes back empty.
                    if let Some((inner_module, inner_type_name, inner_required)) =
                        self.imported_variant_payload_union(module, type_name, v)
                    {
                        self.check_imported_union_coverage(
                            &inner_module,
                            &inner_type_name,
                            &inner_required,
                            subs,
                            match_span,
                            cov.as_ref().map(CoverageWriter::payload),
                        );
                    }
                }
                None => missing.push(v),
            }
        }
        if !missing.is_empty() {
            self.cover_gap(
                cov.as_ref(),
                missing.iter().map(|v| v.to_string()).collect(),
            );
            // Backticked, exactly like the module-local path in
            // `check_patterns_exhaustive`: E0200 has one shape whether the
            // union was declared here or imported (greppability — one rule,
            // one rendering).
            self.errors.push(TypeError::NonExhaustiveMatch {
                type_name: type_name.to_string(),
                missing: missing
                    .iter()
                    .map(|v| format!("`{v}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                union: Some(DiagnosticUnion::Imported {
                    module: module.as_str().to_string(),
                    name: type_name.to_string(),
                }),
                missing_variants: missing.iter().map(|v| (*v).to_string()).collect(),
                span: match_span,
            });
        }
    }

    /// The payload of `variant` (a variant of the imported union `type_name`
    /// declared in `module`), when that payload is itself resolvable as a
    /// tagged union: its own `(module, type name, required variants)`. This is
    /// the type-driven counterpart of `imported_union_variants_from_arms`'s
    /// arm-driven top-level discovery — here the variant is already known, so
    /// only its payload's declaration needs resolving, via the same
    /// `imported_type_decl` query the outer union itself was resolved
    /// through.
    ///
    /// `None` when the union or the variant can't be found, the variant is
    /// nullary, or the payload doesn't resolve to a union (a record payload,
    /// a primitive, or a type this module has no cross-module view of).
    fn imported_variant_payload_union(
        &self,
        module: &ModuleKey,
        type_name: &str,
        variant: &str,
    ) -> Option<(ModuleKey, String, Vec<Ident>)> {
        let decl = self.decl_ty_resolver.imported_type_decl(module.as_str(), type_name)?;
        let Ty::Union { variants } = &decl.body else {
            return None;
        };
        let payload = variants
            .iter()
            .find(|v| v.name.as_ref() == variant)?
            .payload
            .as_ref()?;
        // A type named inside an exported union's body lowers to
        // `Ty::Imported` off the export view (`Lowerer::for_export`)
        // regardless of whether it's a sibling declaration in this same
        // source module or itself re-imported from a third one, so this one
        // match handles both. `union_base` first unwraps a generic
        // application (`B(Tree<K>)`) to its base.
        let Ty::Imported {
            module: inner_module,
            name: inner_name,
        } = union_base(payload)
        else {
            return None;
        };
        let inner_decl = self
            .decl_ty_resolver
            .imported_type_decl(inner_module.as_str(), inner_name)?;
        let Ty::Union {
            variants: inner_variants,
        } = &inner_decl.body
        else {
            return None;
        };
        Some((
            inner_module.clone(),
            inner_decl.name.to_string(),
            inner_variants.iter().map(|v| v.name.clone()).collect(),
        ))
    }

    /// A `match` over a `number`/`string` is exhaustive only if it has a
    /// catch-all arm (`_`, `else`, or a bare-identifier binding). Literal arms
    /// alone leave the rest of the unbounded domain uncovered, which the emitter
    /// lowers to a throwing `switch` `default`.
    fn check_value_exhaustiveness(
        &mut self,
        type_name: &str,
        arms: &[MatchArm],
        match_span: glyph_ast::Span,
    ) {
        let has_catch_all = arms
            .iter()
            .any(|a| is_catch_all_pattern(&a.pattern, Scrutinee::Opaque));
        if !has_catch_all {
            self.errors.push(TypeError::NonExhaustiveValueMatch {
                type_name: type_name.to_string(),
                span: match_span,
            });
        }
    }

    /// A `match` over a string-literal union (`"free" | "pro"`) is exhaustive if
    /// it either has a catch-all or covers every literal in the set. A missing
    /// literal is a compile error listing the gaps, so adding a value to the
    /// union type forces every match to handle it (the enum-exhaustiveness
    /// guarantee that a bare `string` match cannot give).
    fn check_string_literal_union_exhaustiveness(
        &mut self,
        values: &[String],
        arms: &[MatchArm],
        match_span: glyph_ast::Span,
        scrutinee_ty: &Ty,
        at: Option<CoverageAt>,
    ) {
        // The type end is the alias the literal set was reached through. A set
        // written inline into a signature has no declaration, so it has
        // nothing to key a site to and gets none.
        let union_ref = self.string_literal_union_ref(scrutinee_ty);
        let union = union_ref
            .as_ref()
            .map(|u| coverage_name(u, &self.own_module_key()));
        let cov = self.cover_enter(at, union, match_span);
        // A string-literal union is a bounded set of *values*, not a set of
        // variants: `Scrutinee::Opaque`, so a PascalCase head stays the tag
        // test it is instead of covering the rest of the set.
        let has_catch_all = arms
            .iter()
            .any(|a| is_catch_all_pattern(&a.pattern, Scrutinee::Opaque));
        // The arms name what they name whether or not a catch-all absorbs the
        // rest, so the edges go in before the early return below. There is no
        // head classification and no nesting here: the members of this union
        // are values, so an arm either matches one, absorbs everything, or is
        // read by nothing.
        for (i, arm) in arms.iter().enumerate() {
            let ordinal = arm_ordinal(i);
            match &arm.pattern {
                Pattern::Literal {
                    value: LiteralPattern::String(s),
                    ..
                } => self.cover_mention(cov.as_ref(), ordinal, s),
                p if is_catch_all_pattern(p, Scrutinee::Opaque) => {
                    self.cover_catch_all(cov.as_ref(), ordinal)
                }
                _ => self.cover_decline(cov.as_ref(), ordinal, None),
            }
        }
        if has_catch_all {
            return;
        }
        let covered: std::collections::HashSet<&str> = arms
            .iter()
            .filter_map(|a| match &a.pattern {
                Pattern::Literal {
                    value: LiteralPattern::String(s),
                    ..
                } => Some(s.as_str()),
                _ => None,
            })
            .collect();
        let missing: Vec<&String> = values
            .iter()
            .filter(|v| !covered.contains(v.as_str()))
            .collect();
        if !missing.is_empty() {
            self.cover_gap(cov.as_ref(), missing.iter().map(|v| (*v).clone()).collect());
            let type_name = values
                .iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<_>>()
                .join(" | ");
            self.errors.push(TypeError::NonExhaustiveMatch {
                type_name,
                missing: missing
                    .iter()
                    .map(|v| format!("\"{v}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
                union: union_ref,
                missing_variants: missing.iter().map(|v| (*v).clone()).collect(),
                span: match_span,
            });
        }
    }

    /// Flag every arm that follows an irrefutable arm as unreachable. Glyph's
    /// `match` is first-match-wins (D9): once an arm matches every value
    /// (`_`, `else`, or a bare-identifier binding that is not a variant of the
    /// scrutinee's union), no later arm can ever run, so the later arm is dead
    /// code. This is also a soundness fix — the emitter lowers a leading
    /// binding catch-all to a `switch` `default`, and a JS `switch` prefers a
    /// matching `case` over `default` regardless of source order, silently
    /// reordering the arms. Rejecting the dead arm removes that hazard.
    ///
    /// Whether a bare identifier is a variant (refutable) or a binding
    /// (irrefutable) depends on the scrutinee's type; when it resolves to a
    /// tagged union its variant set decides, otherwise every ident is a
    /// binding. Constructor, literal, array, and `is`-type arms are always
    /// refutable and never mark a catch-all. Object patterns are left out of
    /// the irrefutable set here — they are not the reported hazard and skipping
    /// them keeps the check free of false positives.
    fn check_arm_reachability(&mut self, scrutinee_ty: &Ty, arms: &[MatchArm]) {
        // Resolve the scrutinee's variant set, if it has one. `required_variants`
        // covers a `Ty::Named` user union, a *generic* user union (`Tree<T>`
        // arrives as `Ty::App` over a user `Ty::Named`, which
        // `resolve_named_union` unwraps), and the prelude `Result`/`Option`.
        let variants: Vec<Ident> = self
            .required_variants(scrutinee_ty)
            .map(|(_, vs)| vs)
            .unwrap_or_default();
        // `is_catch_all_pattern` is the shared predicate; irrefutable is the
        // same question this pass asks, so it asks it the same way. The
        // prelude names (`Ok`, `Err`, `Some`, `None`) need no clause of their
        // own: all four are PascalCase, so the shape half already calls them
        // references even when the scrutinee's type is undecidable (`match
        // array.find(..) { None => .., Some(_) => .. }`, whose `Option` the
        // checker cannot see through a `.d.ts` method). The same shape half
        // carries an *imported* union, whose variant set is empty here, so a
        // nullary variant like `EmptyOctet` is not misread as an irrefutable
        // catch-all drawing a false E0216 on every later arm. Object patterns
        // stay out of the irrefutable set (`Scrutinee::Union`, not `Record`):
        // they are not the reported hazard and skipping them keeps this check
        // free of false positives.
        let mut seen_irrefutable = false;
        for arm in arms {
            if seen_irrefutable {
                self.errors
                    .push(TypeError::UnreachableMatchArm { span: arm.span });
                continue;
            }
            if is_catch_all_pattern(&arm.pattern, Scrutinee::Union(&variants)) {
                seen_irrefutable = true;
            }
        }
    }

    /// Exhaustiveness for a `match` over a `bool` scrutinee: both `true` and
    /// `false` must be covered, or a catch-all (`_`, `else`, or a binding)
    /// must absorb the rest. D3 makes `match` the only conditional, so an
    /// open boolean match (`match b { true => .. }`) is a real gap. Only
    /// fires when the scrutinee has statically-known `bool` type — a boolean
    /// *expression* (a comparison, say) types as `Unknown` and reaches this
    /// path not at all.
    fn check_bool_exhaustiveness(&mut self, arms: &[MatchArm], match_span: glyph_ast::Span) {
        let mut has_true = false;
        let mut has_false = false;
        for arm in arms {
            // A binding or catch-all absorbs every value. A `bool` has no
            // variants and no fields, so `Scrutinee::Opaque`.
            if is_catch_all_pattern(&arm.pattern, Scrutinee::Opaque) {
                return;
            }
            // Other pattern shapes over a bool scrutinee are not modeled
            // (and don't normally type-check); skip without crediting.
            if let Pattern::Literal {
                value: LiteralPattern::Bool(b),
                ..
            } = &arm.pattern
            {
                if *b {
                    has_true = true;
                } else {
                    has_false = true;
                }
            }
        }
        if has_true && has_false {
            return;
        }
        let missing = match (has_true, has_false) {
            (true, false) => "`false`",
            (false, true) => "`true`",
            (false, false) => "`true` and `false`",
            (true, true) => unreachable!("covered both branches returns above"),
        };
        self.errors.push(TypeError::NonExhaustiveBoolMatch {
            missing: missing.to_string(),
            span: match_span,
        });
    }

    /// If `ty` is an application of the named prelude container type, return
    /// its type arguments. The single collision-guarded prelude-app detector:
    /// prelude and module symbol tables both number ids from 0, so an id match
    /// alone could collide with an unrelated module symbol — require BOTH the
    /// lexical name on the base path AND the prelude id. Shared by
    /// `is_prelude_array` and `prelude_union`.
    fn prelude_app<'a>(&self, ty: &'a Ty, name: &str) -> Option<&'a [Ty]> {
        let Ty::App { base, args } = ty else { return None };
        let Ty::Named { symbol, path } = base.as_ref() else { return None };
        if path.last().map(|n| n.as_ref()) != Some(name) {
            return None;
        }
        (self.lowerer.prelude.lookup(name) == Some(SymbolId(symbol.0))).then_some(args.as_slice())
    }

    /// True if `ty` is an application of the prelude `Array` type
    /// (`Array<T>` → `App(Array, [T])`).
    fn is_prelude_array(&self, ty: &Ty) -> bool {
        self.prelude_app(ty, "Array").is_some()
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
            // A whole-array binding or catch-all covers every length.
            if is_catch_all_pattern(&arm.pattern, Scrutinee::Opaque) {
                return;
            }
            // Other pattern shapes over an array scrutinee are not modeled.
            let Pattern::Array { elements, rest, .. } = &arm.pattern else {
                continue;
            };
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
        patterns: &[(u16, &Pattern)],
        match_span: glyph_ast::Span,
        at: Option<CoverageAt>,
    ) {
        // Resolve the scrutinee to a tagged union (user-defined, imported, or
        // a prelude/stdlib one) and its required variant set.
        let Some((union, variants)) = self.required_variants(scrutinee_ty) else {
            return;
        };
        // The name the diagnostics below render. `union` carries the declaring
        // module as well, which is what the coverage edge is keyed by; the
        // messages have always printed the bare name and still do.
        let type_name = union.display().to_string();
        // The site these writes belong to: this entry's own, opened now that
        // the union is resolved, or the one a payload recursion is inside.
        let cov = self.cover_enter(at, Some(CoverageTypeName::from(&union)), match_span);

        // `covered`: variants whose whole payload is matched (a binding,
        // wildcard, object/array destructure, or no-payload form) — no deeper
        // check needed. `nested`: variants covered ONLY by a constructor
        // sub-pattern, mapped to those sub-patterns for a recursive check.
        let mut covered: HashSet<Ident> = HashSet::new();
        let mut nested: HashMap<Ident, Vec<(u16, &Pattern)>> = HashMap::new();
        let mut has_catch_all = false;
        for &(arm, pat) in patterns {
            // `is TypeName` (D9) is read here rather than in the shared
            // classifier: only this side ever credited one, and crediting it
            // for the imported twin as well would change what E0200 says about
            // a match this change is not about. The asymmetry stays at the one
            // call site that has it.
            //
            // Not collapsed into a nested pattern on purpose: a non-`Path`
            // `ty` has to fall through to the conservative skip, and matching
            // `ty: TypeExpr::Path { .. }` in the arm would need a second arm
            // to say the same thing.
            #[allow(clippy::collapsible_match)]
            if let Pattern::IsType { ty, .. } = pat {
                // The inner TypeExpr is typically a `Path` — extract the last
                // segment as the variant name when possible.
                if let TypeExpr::Path { segments, .. } = ty {
                    if let Some(name) = segments.last().filter(|n| variants.iter().any(|v| v == *n))
                    {
                        covered.insert(name.clone());
                        self.cover_mention(cov.as_ref(), arm, name.as_ref());
                        continue;
                    }
                }
                // Non-Path TypeExpr (e.g., `is fn(x) -> y`) or a path that
                // doesn't name a variant of this union — conservative: skip
                // without marking catch-all.
                self.cover_decline(cov.as_ref(), arm, None);
                continue;
            }
            match classify_arm(pat, &variants) {
                ArmCoverage::CatchAll => {
                    has_catch_all = true;
                    self.cover_catch_all(cov.as_ref(), arm);
                }
                ArmCoverage::Mentions(variant) => {
                    covered.insert(variant.clone());
                    self.cover_mention(cov.as_ref(), arm, variant.as_ref());
                }
                // A single payload sub-pattern is collected for a recursive
                // check. Whether it actually covers the payload (a binding
                // `Ok(x)`) or only part of it (a nested variant `Ok(Some(x))`,
                // or the no-arg variant `Ok(None)` which parses as an ident)
                // is decided by the recursion, which knows the payload's
                // variants. The arm still named the variant here, so the
                // mention is recorded: this bucket, not `covered`, is where
                // most of the corpus's sites keep their outer edge.
                ArmCoverage::Nests { variant, sub } => {
                    nested.entry(variant.clone()).or_default().push((arm, sub));
                    self.cover_mention(cov.as_ref(), arm, variant.as_ref());
                }
                // A head that names no variant of this union is the
                // silent-swallow class this escalates: E0220 with a
                // nearest-variant hint, before the arm is dropped. It is
                // neither covered nor a catch-all, so a genuinely missing
                // variant still surfaces as E0200 alongside it.
                ArmCoverage::UnknownVariant { name, span, .. } => {
                    self.errors.push(TypeError::UnknownVariantPattern {
                        union: type_name.clone(),
                        name: name.to_string(),
                        suggestion: nearest_variant(name.as_ref(), &variants),
                        span,
                    });
                }
                // A value-testing record payload, or a top-level shape this
                // check does not model. Recorded as declined rather than
                // dropped: the site's mentions are not a complete accounting,
                // and a reader has to be able to tell that from a site where
                // they are.
                ArmCoverage::Declined { variant } => {
                    let variant = variant.map(|v| v.to_string());
                    self.cover_decline(cov.as_ref(), arm, variant.as_deref());
                }
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
                    // return None — but that None hides two very different
                    // cases the recursion cannot tell apart from inside: the
                    // payload is genuinely uninspectable (imported, or its
                    // type never resolved), where staying silent is the only
                    // safe answer, or the payload resolved all the way to a
                    // concrete declaration that simply has no variants (a
                    // record, most often), where the compiler knows exactly
                    // what shape values take. In the second case a
                    // variant-shaped sub-pattern (`Ok(Point)` against `type
                    // Point = { .. }`) can never match anything; left
                    // unflagged here it passed Glyph's typecheck clean and
                    // only failed downstream at the tsc backend, on a `.tag`
                    // property the emitter's own PascalCase shape fallback
                    // (`is_variant_shaped`) invented for a value the author
                    // never asked to be tag-tested (G146). `type_name` here
                    // is the *enclosing* union's display name (`Result`, not
                    // `Point`), which is what actually reads correctly in
                    // "`Point` is not a variant of `Result`" — recursing
                    // normally would instead have `Point` resolve its own
                    // (empty) variant set and report against itself.
                    if let Some(payload_ty) = self.variant_payload(scrutinee_ty, v) {
                        if self.required_variants(&payload_ty).is_some() {
                            self.check_patterns_exhaustive(
                                &payload_ty,
                                subs,
                                match_span,
                                cov.as_ref().map(CoverageWriter::payload),
                            );
                        } else if self.resolves_to_non_union_decl(&payload_ty) {
                            for &(_arm, sub) in subs {
                                let (name, span) = match sub {
                                    Pattern::Ident { name, span }
                                        if is_constructor_shaped(name) =>
                                    {
                                        (name.clone(), *span)
                                    }
                                    Pattern::Constructor {
                                        path, span, ..
                                    } if path.last().is_some_and(is_constructor_shaped) => {
                                        (path.last().unwrap().clone(), *span)
                                    }
                                    // A lowercase binding of the whole payload
                                    // (`Ok(p)`), or a destructure of it
                                    // (`Ok({ x, y })`), is legitimate against a
                                    // record payload and covers nothing to flag.
                                    _ => continue,
                                };
                                self.errors.push(TypeError::UnknownVariantPattern {
                                    union: type_name.clone(),
                                    name: name.to_string(),
                                    suggestion: None,
                                    span,
                                });
                            }
                        }
                    }
                }
                None => missing.push(v),
            }
        }

        if missing.is_empty() {
            return;
        }

        self.cover_gap(
            cov.as_ref(),
            missing.iter().map(|n| n.to_string()).collect(),
        );
        let missing_str = missing
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        self.errors.push(TypeError::NonExhaustiveMatch {
            type_name,
            missing: missing_str,
            union: Some(DiagnosticUnion::from(&union)),
            missing_variants: missing.iter().map(|n| n.to_string()).collect(),
            span: match_span,
        });
    }

    /// The type of a `match` expression: an equality join over the arms.
    ///
    /// Every arm must be walked before this runs, since each arm's value type
    /// is read back out of the type map. An arm whose body block ends in
    /// `return`/`break`/`continue` diverges and contributes nothing to the join
    /// (`Err(_) => return 1` parses as a block holding a single `Stmt::Return`).
    /// A block ending in anything other than an expression (a `let`, a `mut`,
    /// an empty block) has no value type and stops the join.
    ///
    /// The join is equality at the head: no widening, no union, no subtyping.
    /// An arm whose value type is entirely undecidable, or all arms diverging,
    /// still yields `Ty::Unknown`. Underneath an already-agreeing head,
    /// `join_ty` joins type arguments with `Unknown ∨ T = T`, which is what
    /// lets `None => []` (an `Array<Unknown>`, per `infer_array_elem_ty`) agree
    /// with `Some(p) => p`'s `Array<string>` on the container head. The head is
    /// what the emitter's `iter_is_array` reads to pick the `for` lowering, so
    /// leaving a hole there ships a wrong program; the element type stays
    /// best-effort with `tsc --strict` as the backstop.
    fn join_match_arms(&self, arms: &[MatchArm]) -> Ty {
        let mut joined: Option<Ty> = None;
        for arm in arms {
            let ty = match &arm.body {
                MatchArmBody::Expr(e) => self.tm.get(e.span()).clone(),
                MatchArmBody::Block(b) => match b.stmts.last() {
                    Some(Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_)) => continue,
                    Some(Stmt::Expr(e)) => self.tm.get(e.span()).clone(),
                    _ => return Ty::Unknown,
                },
            };
            if ty.is_unknown() {
                return Ty::Unknown;
            }
            match &joined {
                None => joined = Some(ty),
                Some(prev) => joined = Some(join_ty(prev, &ty)),
            }
        }
        joined.unwrap_or(Ty::Unknown)
    }

    /// E0223: every arm of a `match` that is used as a value must produce one.
    ///
    /// The emitter lowers a value-position `match` to a `switch` whose cases
    /// assign to (or `return`) the matched value. An arm body that yields
    /// nothing emits `case X: { break; }`, so the binding is never assigned and
    /// the value is `undefined` at run time — and `tsc` does not catch it,
    /// because the emitted binding is untyped at that point. This is exactly
    /// the set `join_match_arms` swallows into `Ty::Unknown`.
    ///
    /// An arm diverges (and so needs no value) when its block ends in
    /// `return`/`break`/`continue`. A nested `match` in tail position inherits
    /// the value position, mirroring `emit_arm_body`, so its arms are checked
    /// under the same rule.
    fn check_arms_produce_values(&mut self, arms: &[MatchArm]) {
        for arm in arms {
            match &arm.body {
                MatchArmBody::Expr(Expr::Match { arms: inner, .. }) => {
                    self.check_arms_produce_values(inner)
                }
                MatchArmBody::Expr(_) => {}
                MatchArmBody::Block(b) => match b.stmts.last() {
                    Some(Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_)) => {}
                    Some(Stmt::Expr(Expr::Match { arms: inner, .. })) => {
                        self.check_arms_produce_values(inner)
                    }
                    Some(Stmt::Expr(_)) => {}
                    _ => self
                        .errors
                        .push(TypeError::MatchArmProducesNoValue { span: arm.span }),
                },
            }
        }
    }

    /// Run `check_arms_produce_values` over a callable body whose tail is a
    /// bare `match`. Called only when the callable declares a return type that
    /// decidably needs a value, so an unannotated callable (D4 makes the
    /// annotation optional) stays permissive.
    fn check_value_match_tail(&mut self, body: &Block) {
        if let Some(Stmt::Expr(Expr::Match { arms, .. })) = body.stmts.last() {
            self.check_arms_produce_values(arms);
        }
    }

    /// Recover a module-local tagged union from a `match`'s arm patterns, used
    /// only when the scrutinee's static type is undecidable (see the call site
    /// in the `Expr::Match` handler). An arm whose head names an in-scope union
    /// variant pins the scrutinee to that variant's union; the first such arm
    /// wins. Returns the union's `Ty::Named` (with `path` set to the union's
    /// name so the emitter's collision guard accepts it), or None when no arm
    /// names a known variant.
    fn recover_union_from_arms(&self, arms: &[MatchArm]) -> Option<Ty> {
        for arm in arms {
            let name = match &arm.pattern {
                Pattern::Constructor { path, .. } => path.last()?,
                Pattern::Ident { name, .. } => name,
                _ => continue,
            };
            if let Some(ty) = self.union_ty_of_variant(name) {
                return Some(ty);
            }
        }
        None
    }

    /// If `name` is a variant of a module-local tagged union, return that
    /// union's `Ty::Named` (pointing at the union's `Type` symbol, not the
    /// variant symbol, so `resolve_named_union` accepts it). None otherwise.
    fn union_ty_of_variant(&self, name: &Ident) -> Option<Ty> {
        let table = &self.resolved.symbols.table;
        for i in 0..table.len() as u32 {
            let sym = table.get(SymbolId(i))?;
            if &sym.name != name {
                continue;
            }
            let SymbolKind::Variant { decl_idx } = sym.kind else {
                continue;
            };
            let Some(Decl::Type(td)) = self.module.items.get(decl_idx as usize) else {
                continue;
            };
            if !matches!(&td.body, TypeExpr::Union { .. }) {
                continue;
            }
            // `Ty::Named` must reference the union's `Type` symbol; scan for the
            // one owning this decl.
            let type_symbol = self.type_symbol_for_decl(decl_idx)?;
            return Some(Ty::Named {
                symbol: type_symbol.into(),
                path: vec![td.name.clone()],
            });
        }
        None
    }

    /// The `SymbolId` of the `Type` symbol for the module item at `decl_idx`.
    fn type_symbol_for_decl(&self, decl_idx: u32) -> Option<SymbolId> {
        let table = &self.resolved.symbols.table;
        for i in 0..table.len() as u32 {
            let sym = table.get(SymbolId(i))?;
            if matches!(sym.kind, SymbolKind::Type { decl_idx: d } if d == decl_idx) {
                return Some(SymbolId(i));
            }
        }
        None
    }

    /// If `ty` is a module-local tagged-union `type X = | A | B | ...`, return
    /// that declaration together with the type arguments it was applied to. The
    /// shared resolution chain behind `named_union_variants` and
    /// `union_variant_payload`.
    ///
    /// A generic union applied via `Ty::App` (`Tree<K>`) resolves through its
    /// base and reports the arguments; an unapplied `Ty::Named` reports none.
    /// The unwrap lives here rather than in the callers because a union's arity
    /// is not something a caller should have to know about: every question the
    /// checker asks of a union (its variant set for exhaustiveness, a variant's
    /// payload for the sub-pattern types) has the same answer whether or not
    /// the declaration takes parameters. Splitting the unwrap across the
    /// callers is what left `Tree<K>` with no exhaustiveness checking at all
    /// while `Tree` was checked.
    ///
    /// The resolved symbol's name has to match the type's lexical path, the
    /// same prelude/module symbol-id collision guard `named_record_fields` and
    /// `interface_member_fields` carry. It matters here because of that unwrap:
    /// a prelude `Result<T, E>` arrives as an application over a `Ty::Named`
    /// whose sentinel symbol id could otherwise index an unrelated
    /// module-local union and answer for it, and `variant_payload` consults
    /// this function *before* the prelude branch, so a collision would shadow
    /// the right answer rather than merely add noise. `prelude_app` carries a
    /// stronger form of the same guard (it checks the prelude table directly);
    /// name-matching is what the neighbouring resolvers here do, and matching
    /// them was the deliberate choice, at the cost of letting a module that
    /// shadows `Result` locally answer for the prelude one.
    fn resolve_named_union<'t>(
        &self,
        ty: &'t Ty,
    ) -> Option<(&glyph_ast::TypeDecl, &'t [Ty])> {
        let (base, args) = split_type_app(ty);
        let Ty::Named { symbol, path } = base else { return None };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return None;
        }
        let SymbolKind::Type { decl_idx } = sym.kind else { return None };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        matches!(&td.body, TypeExpr::Union { .. }).then_some((td, args))
    }

    /// Whether `ty` resolves all the way to a module-local type declaration
    /// that is concretely *not* a tagged union (most often a record). Walks
    /// the same symbol/decl resolution as `resolve_named_union`, but answers
    /// the opposite question: not "is this a union", but "does the compiler
    /// know for certain it has no variants at all", as distinct from a type
    /// it simply cannot see into (imported, or unresolved). G146's nested
    /// exhaustiveness recursion needs exactly that distinction: a payload the
    /// compiler cannot resolve must stay silently unchecked (it might be a
    /// real union the resolver just can't reach across a module boundary),
    /// while a payload that resolves right down to a record declaration is
    /// provably not a union and a variant-shaped pattern against it can be
    /// flagged on the spot.
    fn resolves_to_non_union_decl(&self, ty: &Ty) -> bool {
        let (base, _args) = split_type_app(ty);
        let Ty::Named { symbol, path } = base else { return false };
        let Some(sym) = self.resolved.symbols.table.get(SymbolId(symbol.0)) else {
            return false;
        };
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return false;
        }
        let SymbolKind::Type { decl_idx } = sym.kind else { return false };
        let Some(Decl::Type(td)) = self.module.items.get(decl_idx as usize) else {
            return false;
        };
        // Only a body that is structurally incapable of being a union counts.
        //
        // The earlier form asked `!matches!(body, Union { .. })`, which is a
        // strictly weaker question: it answers "is this body syntactically a
        // union" and calls everything else provably variant-free. A
        // `TypeExpr::Path` is the counterexample, because an alias to a union
        // is a path. `type MaybeAge = Option<int>` was therefore ruled
        // variant-free, and `Ok(Some(n))` over it drew two false `E0220`s on a
        // program 0.1.95 compiled and emitted correctly.
        //
        // Following the alias would widen this, and is worth doing, but not
        // here: this value decides whether to accuse, and an accusation needs
        // certainty while silence costs only a missed diagnostic. A record and
        // a function type cannot alias a union no matter what they point at,
        // which is exactly the shape G146 is about (`Ok(Point)` over a record).
        matches!(&td.body, TypeExpr::Record { .. } | TypeExpr::Fn { .. })
    }

    /// The literal set of a string-literal-union type, resolving a named alias
    /// (`type Tier = "free" | "pro"`) to its declaration body. Returns `None` for
    /// any other type, so a `match` scrutinee that is not a literal union falls
    /// through to the ordinary (unbounded) string/value handling.
    fn string_literal_union_values(&self, ty: &Ty) -> Option<Vec<String>> {
        if let Ty::StringLiteralUnion(values) = ty {
            return Some(values.clone());
        }
        // A string-literal union reached through an imported record's *field*
        // (`match sheet.kind { ... }`). The direct spelling is already answered
        // by `imported_string_literal_union` at lowering; the field type comes
        // from the sibling's own lowering, so it arrives as a `Ty::Imported`
        // and needs the same resolution D30 promises for the direct case.
        if let Ty::Imported { module, name } = ty {
            let mut seen: HashSet<(String, String)> = HashSet::new();
            let decl = self.imported_type_body(module.as_str(), name, &mut seen)?;
            let Ty::StringLiteralUnion(values) = decl.body else {
                return None;
            };
            return Some(values);
        }
        let Ty::Named { symbol, .. } = ty else { return None };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        let SymbolKind::Type { decl_idx } = sym.kind else { return None };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        match &td.body {
            TypeExpr::StringLiteralUnion { values, .. } => Some(values.clone()),
            _ => None,
        }
    }

    /// The field set of `ty` when it is decidably a record: a structural
    /// `Ty::Record`, a `Ty::Named` pointing at a module-local `type X = { ... }`
    /// record declaration, or a generic record application `Ty::App` (whose type
    /// arguments are substituted into the field types). Returns `None` for any
    /// non-record or undecidable type, so callers (member-access checking) never
    /// flag a field on a type they cannot resolve.
    /// Whether `ty` is a `Record<K, V>` map, seeing through one level of local
    /// alias.
    ///
    /// The alias is the case that matters. Nobody annotates a parameter
    /// `Record<string, unknown>` twice; they name it (`type Headers =
    /// Record<string, string>`) and pass the name around, so a check that only
    /// recognised the literal spelling would miss every real program.
    ///
    /// One level, deliberately: a chain is rare and stopping here needs no cycle
    /// guard, which is the same reasoning `named_record_fields` uses.
    fn resolves_to_map(&self, ty: &Ty) -> bool {
        if is_map_ty(ty) {
            return true;
        }
        let Ty::Named { symbol, path } = ty else {
            return false;
        };
        let Some(sym) = self.resolved.symbols.table.get(SymbolId(symbol.0)) else {
            return false;
        };
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return false;
        }
        let SymbolKind::Type { decl_idx } = sym.kind else {
            return false;
        };
        let Some(Decl::Type(td)) = self.module.items.get(decl_idx as usize) else {
            return false;
        };
        is_map_ty(&self.lowerer.lower(&td.body))
    }

    fn record_fields_of(&self, ty: &Ty) -> Option<Vec<RecordField>> {
        self.record_shape_of(ty).map(|s| s.fields)
    }

    /// `record_fields_of`, and additionally the record the fields came from.
    ///
    /// The dispatch lives here and `record_fields_of` is the projection, so the
    /// field set a member access is checked against and the declaration the
    /// field-use relation keys that access to are one answer rather than two.
    fn record_shape_of(&self, ty: &Ty) -> Option<RecordShape> {
        let undeclared = |fields: Vec<RecordField>| RecordShape {
            owner: FieldOwner::Undeclared {
                display: ty_display(ty),
            },
            fields,
        };
        match ty {
            // A structural record: an inline `{ a: string }` annotation or a
            // variant's record payload. It has a field set and no name, so
            // there is no declaration for a rename to land on.
            Ty::Record { fields } => Some(undeclared(fields.clone())),
            // A `Ty::Named` is a `type` record alias, a structural interface, or
            // a stdlib type the runtime ships (`fs.FsError`); all three expose a
            // member/field set for access and assignability. The stdlib table
            // goes first: its types carry a sentinel symbol that resolves to
            // nothing, so the resolver-backed paths below can never see them.
            //
            // The stdlib table's types are `Undeclared`: `fs.FileInfo` has a
            // fixed field set behind a name and no project module declares it.
            // The other two resolve to a declaration in *this* file, so the
            // owner is this file's own module key, which is the same rule a
            // locally declared union's coverage edge uses.
            Ty::Named { .. } => match stdlib_type_fields(self, ty) {
                Some(fields) => Some(undeclared(fields)),
                None => self
                    .named_record_shape(ty, &[])
                    .or_else(|| self.interface_member_shape(ty)),
            },
            // A type declared in a sibling module. Resolving it here, rather
            // than at lowering, is what keeps the representation free of cycle
            // guards: nothing expands until a field set is actually asked for.
            Ty::Imported { module, name } => {
                self.imported_record_shape(module.as_str(), name, &[])
            }
            // Dispatch on the base so a generic sibling record
            // (`type Box<T> = { value: T }` used as `Box<string>`) substitutes
            // its arguments the same way a local one does.
            Ty::App { base, args } => match base.as_ref() {
                Ty::Imported { module, name } => {
                    self.imported_record_shape(module.as_str(), name, args)
                }
                _ => self.named_record_shape(base, args),
            },
            _ => None,
        }
    }

    /// `named_record_fields` with the declaration it read them from. A record
    /// declared in this file, so the owning module is this file's own key.
    fn named_record_shape(&self, ty: &Ty, args: &[Ty]) -> Option<RecordShape> {
        let fields = self.named_record_fields(ty, args)?;
        Some(RecordShape {
            owner: self.local_owner(ty)?,
            fields,
        })
    }

    /// `interface_member_fields` with the declaration it read them from.
    fn interface_member_shape(&self, ty: &Ty) -> Option<RecordShape> {
        let fields = self.interface_member_fields(ty)?;
        Some(RecordShape {
            owner: self.local_owner(ty)?,
            fields,
        })
    }

    /// The owner of a `Ty::Named` this file declares: this file's module key
    /// and the declaration's own name.
    ///
    /// The name comes from the resolved symbol rather than from the type's
    /// lexical path, because the path is what the *use site* wrote and the
    /// symbol is what the declaration is called. The two paths that reach here
    /// have both already required them to match, so this is the same string
    /// either way; reading it off the declaration keeps it that way if one ever
    /// stops.
    fn local_owner(&self, ty: &Ty) -> Option<FieldOwner> {
        let Ty::Named { symbol, .. } = ty else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        Some(FieldOwner::Declared {
            module: self.own_module_key(),
            name: sym.name.to_string(),
        })
    }

    /// `imported_record_fields` with the declaration it read them from.
    ///
    /// The owner is the record the alias chain *ends* at, not the name the use
    /// site reached it by. `pub type Rows = Sheet` re-exported from a third
    /// module gives `Rows` no fields of its own, so a site reading `r.header`
    /// through it is a site over `catalog::Sheet.header`, and keying it under
    /// `Rows` would put it in the impact set of a rename that cannot touch it.
    fn imported_record_shape(
        &self,
        module: &str,
        name: &str,
        args: &[Ty],
    ) -> Option<RecordShape> {
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let (owner_module, owner_name) = self.imported_record_decl(module, name, &mut seen)?;
        let fields = self.imported_record_fields(&owner_module, &owner_name, args)?;
        Some(RecordShape {
            owner: FieldOwner::Declared {
                module: owner_module,
                name: owner_name,
            },
            fields,
        })
    }

    /// Follow a cross-module alias chain to the module and name the record is
    /// actually declared under. The same walk `imported_type_body` does, and it
    /// reuses it: the chain is followed once and this reports where it stopped.
    fn imported_record_decl(
        &self,
        module: &str,
        name: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> Option<(String, String)> {
        let decl = self.decl_ty_resolver.imported_type_decl(module, name)?;
        if let Ty::Imported {
            module: next_module,
            name: next_name,
        } = &decl.body
        {
            let (next_module, next_name) = (next_module.as_str().to_string(), next_name.to_string());
            if !seen.insert((next_module.clone(), next_name.clone())) {
                return None;
            }
            return self.imported_record_decl(&next_module, &next_name, seen);
        }
        Some((module.to_string(), name.to_string()))
    }

    /// The field set of an imported record type, with the declaration's generic
    /// parameters substituted by `args`. `None` when the sibling declares the
    /// name as something other than a record, or when it cannot be resolved at
    /// all (no cross-module context, a stdlib module, a cyclic alias chain) —
    /// which leaves member access exactly as permissive as it is today.
    fn imported_record_fields(
        &self,
        module: &str,
        name: &str,
        args: &[Ty],
    ) -> Option<Vec<RecordField>> {
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let decl = self.imported_type_body(module, name, &mut seen)?;
        let Ty::Record { fields } = decl.body else {
            return None;
        };
        if decl.generics.is_empty() || args.is_empty() {
            return Some(fields);
        }
        let mut subst: HashMap<Ident, Ty> = HashMap::new();
        for (g, a) in decl.generics.iter().zip(args.iter()) {
            subst.insert(g.clone(), a.clone());
        }
        Some(
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: substitute_type_params(&f.ty, &subst),
                    optional: f.optional,
                })
                .collect(),
        )
    }

    /// Resolve `(module, name)` to the sibling's lowered `type` declaration,
    /// following a cross-module alias chain (`pub type Rows = Sheet` in the
    /// sibling lowers to a `Ty::Imported` body, which is another hop). The one
    /// place cross-module type resolution happens. A cycle returns `None`
    /// rather than looping: permissive, never a hang.
    fn imported_type_body(
        &self,
        module: &str,
        name: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> Option<ImportedTypeDecl> {
        if !seen.insert((module.to_string(), name.to_string())) {
            return None;
        }
        let decl = self.decl_ty_resolver.imported_type_decl(module, name)?;
        if let Ty::Imported {
            module: next_module,
            name: next_name,
        } = &decl.body
        {
            let (next_module, next_name) = (next_module.clone(), next_name.clone());
            return self.imported_type_body(next_module.as_str(), &next_name, seen);
        }
        Some(decl)
    }

    /// The field set of a `Ty::Named` record declaration, with any generic
    /// parameters substituted by `args`. Guards against the prelude/module
    /// symbol-id collision (a prelude `Ty::Named` like `Array` could otherwise
    /// index an unrelated module record) by requiring the resolved symbol's name
    /// to match the type's lexical path — the same guard the emitter uses.
    fn named_record_fields(&self, ty: &Ty, args: &[Ty]) -> Option<Vec<RecordField>> {
        let Ty::Named { symbol, path } = ty else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return None;
        }
        let SymbolKind::Type { decl_idx } = sym.kind else {
            return None;
        };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        if !matches!(&td.body, TypeExpr::Record { .. }) {
            return None;
        }
        let Ty::Record { fields } = self.lowerer.lower(&td.body) else {
            return None;
        };
        if td.generics.is_empty() || args.is_empty() {
            return Some(fields);
        }
        // Substitute the declaration's generic parameters with the application's
        // type arguments (`type Box<T> = { value: T }` applied to `<number>`).
        let mut subst: HashMap<Ident, Ty> = HashMap::new();
        for (g, a) in td.generics.iter().zip(args.iter()) {
            subst.insert(g.name.clone(), a.clone());
        }
        Some(
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: substitute_type_params(&f.ty, &subst),
                    optional: f.optional,
                })
                .collect(),
        )
    }

    /// The member set of a `Ty::Named` pointing at a **structural interface**
    /// (D34), rendered as `RecordField`s: a `fn m(p: P) -> R` method member
    /// becomes a field `m: fn(p: P) -> R`, and a `name: T` / `name?: T`
    /// property member becomes a field of the same type and optionality.
    /// Returns `None` for any type that is not a decidable interface, so
    /// callers stay permissive on everything else.
    ///
    /// An interface is structural: a value satisfies it when it carries every
    /// member, with no nominal identity (no `impl`). Assignability against an
    /// interface used as an ordinary type therefore compares these member
    /// fields, not the interface's name.
    fn interface_member_fields(&self, ty: &Ty) -> Option<Vec<RecordField>> {
        let Ty::Named { symbol, path } = ty else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        // Same prelude/module symbol-id collision guard `named_record_fields`
        // uses: the resolved symbol's name must match the type's lexical path.
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return None;
        }
        let SymbolKind::Type { decl_idx } = sym.kind else {
            return None;
        };
        let Decl::Interface(iface) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        Some(
            iface
                .members
                .iter()
                .map(|m| match m {
                    InterfaceMember::Method {
                        name,
                        params,
                        return_ty,
                        ..
                    } => RecordField {
                        name: name.clone(),
                        ty: Ty::Fn {
                            params: params
                                .iter()
                                .map(|p| FnParam {
                                    name: Some(p.name.clone()),
                                    owned: false,
                                    ty: self.lowerer.lower(&p.ty),
                optional: false,
                                })
                                .collect(),
                            return_ty: Arc::new(
                                return_ty
                                    .as_ref()
                                    .map(|rt| self.lowerer.lower(rt))
                                    .unwrap_or(Ty::Prim(Primitive::Void)),
                            ),
                            is_async: false,
                        },
                        optional: false,
                    },
                    InterfaceMember::Field(f) => RecordField {
                        name: f.name.clone(),
                        ty: self.lowerer.lower(&f.ty),
                        optional: f.optional,
                    },
                })
                .collect(),
        )
    }

    /// True when `found` is provably not assignable to `expected`, with
    /// structural handling of an interface on the expected side. An interface
    /// used as an ordinary parameter or return type is satisfied by any value
    /// carrying its members, so it is matched by member shape rather than by
    /// the nominal `Named`-vs-`Named` name check `definitely_incompatible`
    /// applies to `type` aliases (Q15). Non-interface expectations delegate to
    /// `definitely_incompatible` unchanged; the recursion also reaches an
    /// interface nested one level inside a generic application (`Array<Iface>`).
    fn assign_incompatible(&self, found: &Ty, expected: &Ty) -> bool {
        if let Some(members) = self.interface_member_fields(expected) {
            return match self.record_fields_of(found) {
                // A value carries its members structurally: the same
                // required-member / shared-field-type logic the record branch
                // of `definitely_incompatible` already implements.
                Some(found_fields) => definitely_incompatible(
                    &Ty::Record {
                        fields: found_fields,
                    },
                    &Ty::Record { fields: members },
                ),
                // `found` is undecidable (an open generic, an unresolved value):
                // stay permissive, exactly as the nominal path does.
                None => false,
            };
        }
        if let (Ty::App { base: fb, args: fa }, Ty::App { base: eb, args: ea }) = (found, expected) {
            return fa.len() != ea.len()
                || self.assign_incompatible(fb, eb)
                || fa
                    .iter()
                    .zip(ea.iter())
                    .any(|(f, e)| self.assign_incompatible(f, e));
        }
        definitely_incompatible(found, expected)
    }

    /// The declaration and ordered variant list of a union declared in another
    /// module, read off the export view through the same query an imported
    /// union's payload uses. Otherwise None.
    fn imported_union_variants(&self, ty: &Ty) -> Option<(UnionRef, Vec<Ident>)> {
        let (base, _args) = split_type_app(ty);
        let Ty::Imported { module, name } = base else { return None };
        let decl = self.decl_ty_resolver.imported_type_decl(module.as_str(), name)?;
        let Ty::Union { variants } = &decl.body else { return None };
        let names: Vec<Ident> = variants.iter().map(|v| v.name.clone()).collect();
        // The module comes off the type, which is the declaring module rather
        // than this one. Both halves were already destructured here and both
        // used to be dropped at the return.
        let union = UnionRef::Imported {
            module: module.as_str().to_string(),
            name: name.to_string(),
        };
        Some((union, names))
    }

    /// If `ty` is a module-local tagged union, return its declaration and the
    /// ordered list of variant names. Otherwise None. A generic union's
    /// application (`Tree<K>`) answers the same as its bare form: the variant
    /// set does not depend on the arguments, and `resolve_named_union` unwraps
    /// the application for us.
    fn named_union_variants(&self, ty: &Ty) -> Option<(UnionRef, Vec<Ident>)> {
        let (td, _) = self.resolve_named_union(ty)?;
        let TypeExpr::Union { variants, .. } = &td.body else { return None };
        let names: Vec<Ident> = variants.iter().map(|v| v.name.clone()).collect();
        // `resolve_named_union` only ever reaches `self.module.items`, so a hit
        // here is declared in the file being checked and the module is this
        // file's own key.
        let union = UnionRef::Local {
            module: self.own_module_key(),
            name: td.name.to_string(),
        };
        Some((union, names))
    }

    /// The exhaustiveness target for `ty`: a module-local tagged union, one
    /// declared in another module, a stdlib union, or a prelude `Result`
    /// (`Ok`/`Err`) / `Option` (`Some`/`None`). Returns the declaration the
    /// variant set came from and the required variant names. Otherwise None.
    ///
    /// The declaration, not a display name: this answer is the type end of a
    /// match-coverage edge as well as the string E0200 prints, and the corpus
    /// holds eleven unrelated declarations named `Command`, so the name on its
    /// own would file all eleven under one key. `UnionRef::display` is the name
    /// every diagnostic still renders.
    fn required_variants(&self, ty: &Ty) -> Option<(UnionRef, Vec<Ident>)> {
        if let Some(found) = stdlib_union_variants(ty) {
            return Some(found);
        }
        if let Some(found) = self.named_union_variants(ty) {
            return Some(found);
        }
        // A union declared in another module. `named_union_variants` resolves
        // only `Ty::Named`, which is this module's own declarations, so without
        // this a *module-local* union whose variant payload is an imported
        // union had no variant list to require and its inner match was never
        // checked: it built clean, passed `tsc --strict`, and threw
        // `non-exhaustive match` at run time.
        //
        // This is the sibling of the payload resolution added for G140 and the
        // same shape as G143. Both directions across the boundary now resolve
        // through the same `imported_type_decl` query, which is the point:
        // whether the union or the payload is the imported half should not
        // decide whether the compiler checks it.
        if let Some(found) = self.imported_union_variants(ty) {
            return Some(found);
        }
        // A prelude union has a fixed variant table and no declaration in any
        // project module, so there is nothing to address and it is not a
        // `Declared` case under an invented module.
        match self.prelude_union(ty)? {
            ("Result", _) => Some((
                UnionRef::Builtin {
                    name: "Result".to_string(),
                },
                vec!["Ok".into(), "Err".into()],
            )),
            ("Option", _) => Some((
                UnionRef::Builtin {
                    name: "Option".to_string(),
                },
                vec!["Some".into(), "None".into()],
            )),
            _ => None,
        }
    }

    /// If `ty` is an application of the prelude `Result`/`Option` type,
    /// return its display name and type arguments. The shared detector behind
    /// `required_variants` and `variant_payload`; the collision-guarded
    /// prelude-app match lives in `prelude_app`.
    fn prelude_union<'a>(&self, ty: &'a Ty) -> Option<(&'static str, &'a [Ty])> {
        if let Some(args) = self.prelude_app(ty, "Result") {
            return Some(("Result", args));
        }
        if let Some(args) = self.prelude_app(ty, "Option") {
            return Some(("Option", args));
        }
        None
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
    /// Prelude unions are included: `variant_payload` reads `Ok`/`Err`/`Some`
    /// payloads off the scrutinee's `Ty::App` arguments, so `match r { Ok(v) =>
    /// v, ... }` binds `v` to the success type. Without that binding the arm
    /// body types as `Unknown` and the match's arm join has nothing to join.
    ///
    /// Deferred: nested constructor payloads and array payloads.
    fn bind_arm_payloads(&mut self, scrutinee_ty: &Ty, pattern: &Pattern) {
        self.record_pattern_tys(scrutinee_ty, pattern);
        let Pattern::Constructor { path, args, .. } = pattern else {
            return;
        };
        let Some(variant_name) = path.last() else {
            return;
        };
        let Some(payload_ty) = self.variant_payload(scrutinee_ty, variant_name) else {
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
    ///
    /// A field carrying a structured sub-pattern binds through that pattern
    /// instead, at the sub-pattern's own spans: a nested object destructure
    /// recurses on the field's record type, and a nested constructor
    /// (`{ left: Node({ value: v }) }`) goes back through `bind_arm_payloads`,
    /// which resolves the variant's payload before descending again.
    fn bind_object_pattern_fields(&mut self, fields: &[ObjectPatternField], payload_ty: &Ty) {
        let Ty::Record { fields: rec_fields } = payload_ty else {
            return;
        };
        for pf in fields {
            let Some(rf) = rec_fields.iter().find(|rf| rf.name == pf.key) else {
                continue;
            };
            if pf.bound_name().is_some() {
                self.local_tys.insert(pf.span.start, rf.ty.clone());
                continue;
            }
            match &pf.pattern {
                Some(Pattern::Object { fields: inner, .. }) => {
                    self.bind_object_pattern_fields(inner, &rf.ty)
                }
                Some(sub @ Pattern::Constructor { .. }) => self.bind_arm_payloads(&rf.ty, sub),
                _ => {}
            }
        }
    }

    /// Record, for every node of `pattern`, the type of the value that node is
    /// matched against, keyed by the node's own span.
    ///
    /// The emitter reads these back. A variant declared with a record payload
    /// has that payload spread flat into the tag object while every other
    /// payload sits under `value`, so a pattern reached *through* a payload has
    /// to know which, and the type is the only thing that answers it. The
    /// resolution lives here rather than in the emitter because this is the
    /// side that holds the lowerer and the cross-module declaration resolver;
    /// an emitter-local answer would be a second, weaker copy of a question
    /// already answered once.
    ///
    /// A node whose type cannot be resolved is simply not recorded, and the
    /// emitter falls back to what it can decide from the variant name alone.
    fn record_pattern_tys(&mut self, ty: &Ty, pattern: &Pattern) {
        self.tm.insert(pattern.span(), ty.clone());
        match pattern {
            Pattern::Constructor { path, args, .. } => {
                let Some(variant) = path.last() else { return };
                let Some(payload_ty) = self.variant_payload(ty, variant) else {
                    return;
                };
                if let [sub] = args.as_slice() {
                    self.record_pattern_tys(&payload_ty, sub);
                }
            }
            Pattern::Object { fields, .. } => {
                let Ty::Record { fields: rec_fields } = ty else {
                    return;
                };
                let rec_fields = rec_fields.clone();
                for pf in fields {
                    let Some(sub) = &pf.pattern else { continue };
                    let Some(rf) = rec_fields.iter().find(|rf| rf.name == pf.key) else {
                        continue;
                    };
                    self.record_pattern_tys(&rf.ty, sub);
                }
            }
            Pattern::Array { elements, rest, .. } => {
                let Some(elem) = self.prelude_app(ty, "Array").and_then(|a| a.first()).cloned()
                else {
                    return;
                };
                for el in elements {
                    self.record_pattern_tys(&elem, el);
                }
                // A rest binding holds the tail, which is another array of the
                // same element type.
                if let Some(r) = rest.as_deref() {
                    self.record_pattern_tys(ty, r);
                }
            }
            _ => {}
        }
    }

    /// The lowered payload type of `variant_name` in the tagged union `ty`
    /// refers to, or None if `ty` isn't such a union, the variant doesn't
    /// exist, or it carries no payload.
    ///
    /// A generic union applied via `Ty::App` (`Tree<K>`) resolves through its
    /// base and substitutes the declaration's parameters into the payload, the
    /// same way `record_fields_of` sends an application to `named_record_fields`.
    /// A union's arity is not something an arm should be able to feel: without
    /// this, a scrutinee of `Tree<K>` recorded no payload type, so nothing under
    /// the payload got a type and a nested field pattern was refused as
    /// undecidable while the identical arm over a non-generic `Tree` compiled.
    ///
    /// A union declared in another module lowers to `Ty::Imported { module,
    /// name }` rather than `Ty::Named` — resolved through
    /// `DeclTyResolver::imported_type_decl` instead of `resolve_named_union`,
    /// which only ever sees this module's own AST. This is the caller behind
    /// `record_pattern_tys`, which is what a *nested* pattern reached through a
    /// payload field reads its own type from (`emit::payload_shape`'s registry
    /// lookup only covers the outermost constructor of an arm). Without this
    /// branch, an imported union's own type never resolved here, so nothing
    /// nested under a payload-carrying variant's field got a recorded type at
    /// all: the named-import spelling still built because a separate,
    /// name-based fallback in the emitter happens to see the variant's own
    /// symbol, but the namespace spelling (`import tree` / `tree.Node(...)`)
    /// binds no such symbol, so it had no fallback and fell through to
    /// `E0300`'s "cannot be decided here" refusal (G140). Resolving the same
    /// declaration the same way regardless of import spelling is the fix, not
    /// widening the emitter's fallback further.
    fn union_variant_payload(&self, ty: &Ty, variant_name: &Ident) -> Option<Ty> {
        let (base, args) = split_type_app(ty);
        if let Ty::Imported { module, name } = base {
            let decl = self.decl_ty_resolver.imported_type_decl(module.as_str(), name)?;
            let Ty::Union { variants } = &decl.body else { return None };
            let variant = variants.iter().find(|v| &v.name == variant_name)?;
            let payload = variant.payload.clone()?;
            if decl.generics.is_empty() || args.is_empty() {
                return Some(payload);
            }
            let subst: HashMap<Ident, Ty> =
                decl.generics.iter().cloned().zip(args.iter().cloned()).collect();
            return Some(substitute_type_params(&payload, &subst));
        }
        let (td, args) = self.resolve_named_union(ty)?;
        let TypeExpr::Union { variants, .. } = &td.body else { return None };
        let variant = variants.iter().find(|v| &v.name == variant_name)?;
        let payload_te = variant.payload.as_ref()?;
        let payload = self.lowerer.lower(payload_te);
        if td.generics.is_empty() || args.is_empty() {
            return Some(payload);
        }
        let subst: HashMap<Ident, Ty> = td
            .generics
            .iter()
            .map(|g| g.name.clone())
            .zip(args.iter().cloned())
            .collect();
        Some(substitute_type_params(&payload, &subst))
    }

    /// The payload type of `variant` in the tagged union `ty`, for both
    /// module-local unions (via `union_variant_payload`) and the prelude
    /// `Result`/`Option` — whose payloads are the `Ty::App` type arguments
    /// (`Ok` → arg 0, `Err` → arg 1, `Some` → arg 0). Drives nested
    /// exhaustiveness recursion. None when there is no such payload-carrying
    /// variant. A generic module-local union applied via `Ty::App` resolves
    /// through `union_variant_payload`, which unwraps the application and
    /// substitutes the declaration's parameters into the payload.
    fn variant_payload(&self, ty: &Ty, variant: &Ident) -> Option<Ty> {
        if let Some(p) = stdlib_variant_payload(ty, variant) {
            return Some(p);
        }
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

/// An array-pattern element/rest that matches any value of its position. Used
/// by array exhaustiveness: only irrefutable elements let a pattern fully cover
/// its length(s).
///
/// One definition, `Pattern::is_refutable`, and no second one here. It used to
/// count a bare `Ident` element as irrefutable unconditionally, which made
/// `[Black]` a binding in an array position and a variant tag everywhere else —
/// two readings of D9's capitalization rule in the same checker. The reading
/// that says a PascalCase element tests a tag is the one the rest of the
/// compiler uses, so `match xs { [] => .., [Black] => .. }` is now reported
/// non-exhaustive rather than counted as covering length 1.
///
/// That does not close G130: the top-level array chain still *lowers* `[Black]`
/// to a binding, so an arm set that covers the lengths some other way still
/// miscompiles. It stops one spelling of it from being certified exhaustive on
/// the strength of a disagreement.
fn is_irrefutable_pattern(p: &Pattern) -> bool {
    !p.is_refutable()
}

// ----- assignability (conservative) -----

/// True only when `found` is *provably* not assignable to `expected`. Used for
/// return-type and call-argument checking. The relation stays conservative — it
/// returns false whenever either side is undecidable (`Unknown`, an open generic
/// `Param`, an `App` over an unresolved base) or for shape pairs it does not
/// judge — so it never produces a false positive:
///
/// - `unknown` (the top type) as the expected type accepts any value;
/// - two primitives are incompatible iff they differ;
/// - two named types are incompatible iff their lexical paths differ (nominal,
///   Q15) — comparing paths rather than symbol ids sidesteps the prelude/module
///   id collision and matches what the diagnostic shows;
/// - two generic applications are incompatible if their arity, base, or any
///   argument is incompatible;
/// - a concrete scalar (`string`/`number`/`bool`) is incompatible with a record
///   or function type in either direction (a number is never an object or a
///   function); `void` is excluded, its assignability being subtler;
/// - two function types are incompatible when their return types are (returns
///   are covariant) or when one is `async` and the other is not (D40: an
///   `async fn` emits `Promise<T>`, which is not the same value a sync `fn`
///   returns); parameter variance stays permissive;
/// - two structural records are incompatible when a shared field's types are, or
///   when `found` lacks a required field of `expected`; extra fields in `found`
///   are fine (width subtyping);
/// - a `Named` type against a differently-shaped type stays permissive — a
///   newtype alias may resolve to that shape — as does every other pair.
fn definitely_incompatible(found: &Ty, expected: &Ty) -> bool {
    if matches!(expected, Ty::UnknownTop) {
        return false;
    }
    // `never` is the bottom type (D43): no value has it, so an expression of it
    // fits wherever a value is wanted, and nothing but itself fits into it.
    // Without the first arm a call to a non-returning function could not sit in
    // a `match` arm beside arms that produce a value, which is the whole point
    // of naming the type.
    if matches!(found, Ty::Never) {
        return false;
    }
    if matches!(expected, Ty::Never) {
        return true;
    }
    if !ty_is_decidable(found) || !ty_is_decidable(expected) {
        return false;
    }
    match (found, expected) {
        (Ty::Prim(a), Ty::Prim(b)) => a != b,
        (Ty::Named { path: a, .. }, Ty::Named { path: b, .. }) => {
            !a.is_empty() && !b.is_empty() && a != b
        }
        (Ty::App { base: fb, args: fa }, Ty::App { base: eb, args: ea }) => {
            fa.len() != ea.len()
                || definitely_incompatible(fb, eb)
                || fa
                    .iter()
                    .zip(ea.iter())
                    .any(|(f, e)| definitely_incompatible(f, e))
        }
        // A concrete scalar is never a record or a function (or vice versa).
        (Ty::Prim(p), Ty::Record { .. } | Ty::Fn { .. })
        | (Ty::Record { .. } | Ty::Fn { .. }, Ty::Prim(p)) => is_concrete_scalar(*p),
        // Function return types are covariant: found is not assignable when its
        // return can't stand in for expected's, regardless of the parameters.
        // `void` on either side is skipped: a value-returning function is
        // assignable where a `void`-returning one is expected (callback
        // contravariance), and an un-annotated lambda's return currently infers
        // to the `void` stub, which must not be trusted as a real return type.
        //
        // `is_async` is compared under the same guards (D40). An `async fn(A) ->
        // T` emits `(a: A) => Promise<T>`, so a sync function of the same
        // parameters and return is a different value, and the annotation is what
        // says which one a position wants. Without this the distinction D40
        // introduced would be enforced only by `tsc` (TS2322), never by Glyph.
        // The `void` guard matters here too: TypeScript lets any function stand
        // where a `void`-returning one is expected, so an `async` mismatch is not
        // judged when either side returns `void`.
        (
            Ty::Fn {
                return_ty: fr,
                is_async: fa,
                ..
            },
            Ty::Fn {
                return_ty: er,
                is_async: ea,
                ..
            },
        ) => {
            !matches!(**fr, Ty::Prim(Primitive::Void))
                && !matches!(**er, Ty::Prim(Primitive::Void))
                && (fa != ea || definitely_incompatible(fr, er))
        }
        // Structural records: a shared field with incompatible types, or a
        // required field of `expected` that `found` lacks.
        (Ty::Record { fields: ff }, Ty::Record { fields: ef }) => ef.iter().any(|e| {
            match ff.iter().find(|f| f.name == e.name) {
                Some(f) => definitely_incompatible(&f.ty, &e.ty),
                None => !e.optional,
            }
        }),
        _ => false,
    }
}

/// True for a concrete scalar primitive (`string`/`number`/`bool`) — the ones
/// that are never a record or function. `void` is deliberately excluded; its
/// assignability in return and callback positions is subtler.
fn is_concrete_scalar(p: Primitive) -> bool {
    matches!(p, Primitive::String | Primitive::Number | Primitive::Bool)
}

/// True when `ty` is resolved enough to compare for equality or to judge
/// "not a `Result`" with certainty: not the `Unknown` placeholder, not an
/// open generic `Param` (which could instantiate to anything), and not an
/// `App` over an unresolved (`Unknown`) base. The `?` operand and
/// error-type checks gate on this so neither fires on a type it cannot
/// decide.
/// A synthetic named type for a stdlib type (`http.Response`, `fs.FsError`).
/// The runtime ships these as TypeScript types with no `.d.ts` the checker
/// parses, so they have no resolver `SymbolId`; a sentinel `u32::MAX` symbol
/// keeps them out of the dense id space, and the lexical `path` both renders
/// the diagnostic (`ty_display` joins it with `.`) and distinguishes one
/// stdlib type from another under `Ty` equality (two names with the same
/// sentinel symbol are unequal when their paths differ).
fn stdlib_named(module: &str, name: &str) -> Ty {
    Ty::Named {
        symbol: SymbolRef(u32::MAX),
        path: vec![Ident::from(module), Ident::from(name)],
    }
}

/// The `Ty` for a stdlib type whose *shape* the tables below model, or `None`
/// for every other stdlib type.
///
/// This is what `Lowerer` consults for a written `fs.FsError` annotation, and
/// the restriction is the point: a stdlib type with no modeled shape keeps
/// lowering to `Ty::Unknown`, so nothing the checker cannot judge becomes newly
/// How to name the key in an E0224 about `map[k]`. A literal is quoted as
/// written; anything computed is described rather than rendered, since the
/// point of the diagnostic is that its value is not known here.
fn index_key_display(index: &Expr) -> String {
    match index {
        Expr::String { value, .. } => value.clone(),
        _ => "that key".to_string(),
    }
}

/// The element type of an `Array<T>`, for a `for` binding.
///
/// Only a literal array application. A map, a string, and anything undecidable
/// return `None` and leave the binding as it was, so this never invents a type
/// for an iterand the checker does not understand.
fn array_elem_ty(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::App { base, args } if args.len() == 1 => match base.as_ref() {
            Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Array") => {
                Some(args[0].clone())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Whether a type is a `Record<K, V>` map: a value whose keys are arbitrary
/// rather than a record with a declared field set.
///
/// Only the map form counts. A record type (`{ id: int }`) has known fields and
/// is checked by `record_fields_of`, and anything undecidable stays permissive,
/// so this never turns an unknown receiver into an error.
fn is_map_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::App { base, args }
        if args.len() == 2
            && matches!(base.as_ref(), Ty::Named { path, .. }
                if path.last().map(|s| s.as_ref()) == Some("Record")))
}

/// checked by this table growing an entry it has no fields for.
pub(crate) fn stdlib_modeled_type(module: &str, name: &str) -> Option<Ty> {
    matches!(
        (module, name),
        ("fs", "FsError")
            | ("fs", "FileInfo")
            | ("fs", "ErrorKind")
            | ("http", "HttpError")
            | ("bytes", "BytesError")
            | ("url", "Url")
            | ("url", "Param")
            | ("dns", "MailHost")
            | ("net", "ServerError")
    )
    .then(|| stdlib_named(module, name))
}

/// The two lexical segments of a stdlib type name (`fs.FsError` → `("fs",
/// "FsError")`), or `None` for anything that is not one. Keys the three tables
/// below off the `path` rather than the symbol, because `stdlib_named` gives
/// every stdlib type the same `u32::MAX` sentinel.
fn stdlib_type_path(ty: &Ty) -> Option<(&str, &str)> {
    let Ty::Named { symbol, path } = ty else {
        return None;
    };
    if symbol.0 != u32::MAX || path.len() != 2 {
        return None;
    }
    Some((path[0].as_ref(), path[1].as_ref()))
}

/// The field set of a stdlib named type, read off the TypeScript the runtime
/// ships. Without it a value of a stdlib type is an opaque blob: `e.kind` on an
/// `fs.FsError` typed `Unknown`, so the `match` over it had an undecidable
/// scrutinee and needed an `else` arm to be safe.
///
/// A field here that `runtime/std/fs.ts` does not declare would be a signature
/// for a member that is not there, so the two are kept in step by hand — the
/// same contract `descriptor_member_ty` keeps with the emitter.
fn stdlib_type_fields(a: &Assigner<'_>, ty: &Ty) -> Option<Vec<RecordField>> {
    let fields: Vec<(&str, Ty)> = match stdlib_type_path(ty)? {
        ("fs", "FsError") => vec![
            ("kind", stdlib_named("fs", "ErrorKind")),
            ("message", Ty::Prim(Primitive::String)),
        ],
        ("fs", "FileInfo") => vec![
            ("is_dir", Ty::Prim(Primitive::Bool)),
            ("is_file", Ty::Prim(Primitive::Bool)),
            ("size", Ty::Prim(Primitive::Number)),
            ("modified", Ty::Prim(Primitive::Number)),
        ],
        // `index` is a position in the rejected input, so a decode failure can
        // be reported against the source rather than as "somewhere in here".
        // There is no `kind`: every failure in this module is one shape of the
        // same thing, and a one-variant union would be a match with one arm.
        ("url", "Url") => vec![
            ("scheme", Ty::Prim(Primitive::String)),
            ("host", Ty::Prim(Primitive::String)),
            ("port", a.stdlib_option_ty(Ty::Prim(Primitive::Number))?),
            ("path", Ty::Prim(Primitive::String)),
            ("query", Ty::Prim(Primitive::String)),
            ("fragment", a.stdlib_option_ty(Ty::Prim(Primitive::String))?),
        ],
        ("url", "Param") => vec![
            ("key", Ty::Prim(Primitive::String)),
            ("value", Ty::Prim(Primitive::String)),
        ],
        // `kind` is a string-literal union, so a `match` over it is exhaustive
        // under D30 with no catch-all: three named reasons that lead to three
        // different decisions, and `other` keeps the raw errno reachable.
        ("net", "ServerError") => vec![
            (
                "kind",
                Ty::StringLiteralUnion(vec![
                    "in_use".to_string(),
                    "denied".to_string(),
                    "unavailable".to_string(),
                    "other".to_string(),
                ]),
            ),
            ("message", Ty::Prim(Primitive::String)),
            ("code", Ty::Prim(Primitive::String)),
        ],
        ("dns", "MailHost") => vec![
            ("priority", Ty::Prim(Primitive::Number)),
            ("host", Ty::Prim(Primitive::String)),
        ],
        ("bytes", "BytesError") => vec![
            ("message", Ty::Prim(Primitive::String)),
            ("index", Ty::Prim(Primitive::Number)),
        ],
        // `kind` is a string-literal union rather than a tagged one, so it
        // needs no variant table: D30 exhaustiveness reads the members straight
        // off the type. Without this a caller matching `e.kind` gets E0218
        // ("a string match can never be exhaustive") and is told to add a
        // catch-all, which is advice to switch the check off.
        ("http", "HttpError") => vec![
            ("status", Ty::Prim(Primitive::Number)),
            ("message", Ty::Prim(Primitive::String)),
            (
                "kind",
                Ty::StringLiteralUnion(vec![
                    "timeout".to_string(),
                    "network".to_string(),
                    "status".to_string(),
                ]),
            ),
        ],
        _ => return None,
    };
    Some(
        fields
            .into_iter()
            .map(|(name, ty)| RecordField {
                name: Ident::from(name),
                ty,
                optional: false,
            })
            .collect(),
    )
}

/// The variant set of a stdlib tagged union, in declaration order so the E0200
/// message is reproducible. Feeds `required_variants`, which is what
/// exhaustiveness (E0200), arm reachability (E0216) and the unknown-variant hint
/// (E0220) all read, so one entry here makes `match e.kind { ... }` a checked
/// match instead of a run-time throw.
fn stdlib_union_variants(ty: &Ty) -> Option<(UnionRef, Vec<Ident>)> {
    match stdlib_type_path(ty)? {
        ("fs", "ErrorKind") => Some((
            UnionRef::Builtin {
                name: "fs.ErrorKind".to_string(),
            },
            vec![
                "NotFound".into(),
                "IsADirectory".into(),
                "NotADirectory".into(),
                "PermissionDenied".into(),
                "AlreadyExists".into(),
                "Other".into(),
            ],
        )),
        _ => None,
    }
}

/// The payload of a stdlib union variant. `fs.ErrorKind.Other` carries the raw
/// errno, so `Other({ code })` binds `code` as a `string`.
fn stdlib_variant_payload(ty: &Ty, variant: &Ident) -> Option<Ty> {
    match (stdlib_type_path(ty)?, variant.as_ref()) {
        (("fs", "ErrorKind"), "Other") => Some(Ty::Record {
            fields: vec![RecordField {
                name: Ident::from("code"),
                ty: Ty::Prim(Primitive::String),
                optional: false,
            }],
        }),
        _ => None,
    }
}

/// `n` parameters of unmodeled type — the arity-only shape most of the stdlib
/// table uses, so a modeled return never drags a new argument-type diagnostic
/// in with it.
/// A required parameter of the given type.
fn required(ty: Ty) -> FnParam {
    FnParam { name: None, owned: false, ty, optional: false }
}

/// A parameter the caller may omit. Only the standard library has these; a
/// Glyph `fn` cannot declare one.
fn optional(ty: Ty) -> FnParam {
    FnParam { name: None, owned: false, ty, optional: true }
}

fn unknown_params(n: usize) -> Vec<FnParam> {
    (0..n)
        .map(|_| FnParam {
            name: None,
            owned: false,
            ty: Ty::Unknown,
                optional: false,
        })
        .collect()
}

/// Split a type into the base it applies and the arguments it applies to it:
/// `Tree<string>` is `(Tree, [string])`, and an unapplied `Tree` is
/// `(Tree, [])`. The one place the exhaustiveness path unwraps the
/// `Ty::App`-versus-base distinction, so a question about a union answers the
/// same whether or not the declaration takes parameters. The emitter keeps
/// its own copies of the same unwrap at `glyph-emit/src/lib.rs:2530, 2568,
/// 2687, 3812`, and `record_fields_of` and `owned.rs:477` each have one too;
/// none of those are this function's call to consolidate.
///
/// Three separate checks were written against a bare base and silently stopped
/// applying the moment a type parameter appeared: the module-local variant set,
/// a variant's payload type, and the imported-union exhaustiveness gate. Each
/// one turned a compile-time error into a runtime throw for the generic
/// spelling of a program the non-generic spelling rejected. A caller that asks
/// this instead of matching `Ty::App` itself cannot regress that way.
fn split_type_app(ty: &Ty) -> (&Ty, &[Ty]) {
    match ty {
        Ty::App { base, args } => (base.as_ref(), args.as_slice()),
        other => (other, &[]),
    }
}

/// The base a union type applies, seeing through one `Ty::App`. `Tree<string>`
/// and `Tree` both answer with the union's own type.
fn union_base(ty: &Ty) -> &Ty {
    split_type_app(ty).0
}

fn ty_is_decidable(ty: &Ty) -> bool {
    match ty {
        Ty::Unknown => false,
        Ty::Param { .. } => false,
        // Cross-module assignability stays exactly as permissive as it was when
        // an imported type was `Ty::Unknown`. The identity is now available, so
        // `definitely_incompatible` *could* decide it, but that would be a new
        // error class at a module boundary — language surface, not
        // implementation, and it is not part of this change. Without this arm
        // the `_ => true` fallback would draw a false `QuestionOnNonResult` on
        // `g()?` where `g` returns an imported `type Res = Result<A, B>`.
        Ty::Imported { .. } => false,
        Ty::App { base, .. } => ty_is_decidable(base),
        _ => true,
    }
}

/// Whether a declared return type decidably requires the body to produce a
/// value (E0223). `Unknown` (no annotation, or one that could not be resolved)
/// and `void` both mean "no value is owed", so a body whose tail `match` has a
/// valueless arm is left alone.
fn ty_requires_value(ty: &Ty) -> bool {
    // `never` owes no value for the opposite reason `void` does not: not that
    // the caller wants nothing back, but that the body never gets to the point
    // of returning. A `-> never` body is a loop or a call to another `-> never`.
    !ty.is_unknown()
        && !matches!(ty, Ty::Prim(Primitive::Void))
        && !matches!(ty, Ty::Never)
}


/// The least upper bound of two types, used to join `match` arms.
///
/// Equal types join to themselves. Two `Ty::App`s over the same base with the
/// same arity join argument-wise, where `Unknown ∨ T = T`; everything else
/// joins to `Unknown`. The `Unknown ∨ T` rule is what makes an empty array
/// literal (`Array<Unknown>`) agree with an `Array<string>` on the container
/// head, which is the only part of the result the emitter's lowering choice
/// reads. Nothing here promotes an entirely-`Unknown` arm: that case is
/// rejected by the caller before this runs.
fn join_ty(a: &Ty, b: &Ty) -> Ty {
    if a == b {
        return a.clone();
    }
    // An arm that does not return (it calls `process.exit`, or a `-> never` of
    // your own) contributes nothing to the join, so the other arm's type is the
    // match's type. This is what deletes the dead arm kept only to keep a match
    // exhaustive.
    if matches!(a, Ty::Never) {
        return b.clone();
    }
    if matches!(b, Ty::Never) {
        return a.clone();
    }
    if let (
        Ty::App { base: abase, args: aargs },
        Ty::App { base: bbase, args: bargs },
    ) = (a, b)
    {
        if abase == bbase && aargs.len() == bargs.len() {
            let args = aargs
                .iter()
                .zip(bargs.iter())
                .map(|(x, y)| {
                    if x == y {
                        x.clone()
                    } else if x.is_unknown() {
                        y.clone()
                    } else if y.is_unknown() {
                        x.clone()
                    } else {
                        Ty::Unknown
                    }
                })
                .collect();
            return Ty::App {
                base: abase.clone(),
                args,
            };
        }
    }
    Ty::Unknown
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
        // A callback's declared return type binds a parameter the same way a
        // container's element type does. `array.map(xs, fn(x: T) -> U { .. })`
        // could not bind `U` before, so `map` was left unmodeled and its result
        // was `Unknown`: the third of G39's three callback cases.
        (
            Ty::Fn { params: pparams, return_ty: pret, .. },
            Ty::Fn { params: aparams, return_ty: aret, .. },
        ) => {
            for (pp, ap) in pparams.iter().zip(aparams.iter()) {
                collect_type_param_bindings(&pp.ty, &ap.ty, out);
            }
            collect_type_param_bindings(pret, aret, out);
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
                    optional: p.optional,
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

/// What one arm's head says about a union's variant set: the per-pattern
/// classification both coverage checkers run, lifted out of the two loops
/// that used to hold a copy each.
///
/// It decides and does not report. The callers keep their own diagnostics, at
/// the points they already pushed them, and they keep the two places they
/// disagree: the module-local checker escalates an unknown head to E0220
/// immediately, while the imported one collects constructor heads for the same
/// diagnostic after its loop and has always credited a bare one instead.
enum ArmCoverage<'p> {
    /// Absorbs every value the scrutinee can still take, so no later arm runs
    /// and no earlier gap remains.
    CatchAll,
    /// Names this variant and covers its whole payload: a bare variant head, a
    /// no-payload constructor, or a multi-argument one.
    Mentions(&'p Ident),
    /// Names this variant through exactly one payload sub-pattern. Whether the
    /// payload is itself covered is a question for the payload's type, which is
    /// what the caller's recursion answers.
    Nests {
        variant: &'p Ident,
        sub: &'p Pattern,
    },
    /// A constructor-shaped head naming no variant of this union: a typo, or a
    /// variant of a different union. `bare` distinguishes `Loadign` from
    /// `Loadign(x)`, which the two callers treat differently.
    UnknownVariant {
        name: &'p Ident,
        span: Span,
        bare: bool,
    },
    /// The checker reads nothing from this arm: a payload sub-pattern that
    /// tests a field's value and can fail, or a top-level shape (literal,
    /// array, record) this check does not model.
    Declined { variant: Option<&'p Ident> },
}

/// Classify one arm head against `variants`. See `ArmCoverage`.
///
/// `is TypeName` is deliberately not handled here: only the module-local
/// checker ever credited one, and reading it for the imported checker too
/// would change what E0200 says about a match this change is not about. That
/// asymmetry stays at the one call site that has it.
fn classify_arm<'p>(pat: &'p Pattern, variants: &[Ident]) -> ArmCoverage<'p> {
    match pat {
        Pattern::Wildcard { .. } | Pattern::Else { .. } => ArmCoverage::CatchAll,
        Pattern::Ident { name, span } => {
            // The shared predicate decides first: a head that is not a variant
            // reference is a fresh binding, so it absorbs everything. A head
            // that is one either names a variant of this union or names none
            // of them, and the second case is a typo rather than a licence to
            // swallow every value.
            if is_catch_all_pattern(pat, Scrutinee::Union(variants)) {
                ArmCoverage::CatchAll
            } else if variants.iter().any(|v| v == name) {
                ArmCoverage::Mentions(name)
            } else {
                ArmCoverage::UnknownVariant {
                    name,
                    span: *span,
                    bare: true,
                }
            }
        }
        Pattern::Constructor {
            path, args, span, ..
        } => {
            // The LAST segment is the variant name: bare `Loading` and
            // qualified `Feed.Loading` both name `Loading`.
            let Some(variant) = path.last() else {
                return ArmCoverage::Declined { variant: None };
            };
            if !variants.iter().any(|v| v == variant) {
                // A constructor form can never be an irrefutable binding (a
                // binding takes no payload and no qualifier), so it is neither
                // covered nor a catch-all and a genuinely missing variant
                // still surfaces alongside whatever the caller reports here.
                return if is_constructor_shaped(variant) {
                    ArmCoverage::UnknownVariant {
                        name: variant,
                        span: *span,
                        bare: false,
                    }
                } else {
                    ArmCoverage::Declined { variant: None }
                };
            }
            match args.as_slice() {
                // A record destructure sub-pattern that tests a field's value
                // (`Node({ colour: Black })`) can fail, so the arm covers
                // nothing on its own and is recorded in neither map. The
                // variant is reported missing unless another arm or a
                // catch-all takes it, which is the safe direction: the
                // alternative is accepting a match that falls off its end.
                [sub] if sub.is_refutable() && matches!(sub, Pattern::Object { .. }) => {
                    ArmCoverage::Declined {
                        variant: Some(variant),
                    }
                }
                // One payload sub-pattern names the variant here; whether it
                // covers the payload (a binding `Ok(x)`) or only part of it (a
                // nested variant `Ok(Some(x))`) is for the recursion, which
                // knows the payload's variants.
                [sub] => ArmCoverage::Nests { variant, sub },
                // No-arg (`fs.ErrorKind.NotFound`) or multi-arg payloads cover
                // the variant at this level.
                _ => ArmCoverage::Mentions(variant),
            }
        }
        // Literal, object, array and `is` patterns over a union scrutinee are
        // not modeled here. Conservative: read nothing from them rather than
        // report a variant missing that an unread arm may well handle.
        _ => ArmCoverage::Declined { variant: None },
    }
}

/// What the catch-all question needs to know about the scrutinee, and all it
/// needs to know.
///
/// "Does this arm absorb every remaining value" has one answer, but reaching
/// it takes context, because two pattern shapes mean different things over
/// different scrutinees. A lowercase head is a fresh binding everywhere
/// except over a union that declares it as a variant (Glyph allows a
/// lowercase variant name). An all-binding object pattern destructures a
/// record and absorbs every value of it, while over a `bool` or a number it
/// tests a shape the value does not have and absorbs nothing.
///
/// So the context is unified rather than the answer flattened: every
/// exhaustiveness check names which of these three it is looking at, and
/// `is_catch_all_pattern` decides from there.
#[derive(Clone, Copy)]
enum Scrutinee<'a> {
    /// A tagged union with this variant set: module-local, the prelude
    /// `Result`/`Option`, or an imported one resolved cross-module. Empty
    /// when the union's variants could not be resolved, which costs nothing:
    /// shape alone still classifies a PascalCase head.
    Union(&'a [Ident]),
    /// A record. The only scrutinee an object pattern can be irrefutable
    /// against.
    Record,
    /// A `bool`, a number, a string, a string-literal union, an array: no
    /// variants to name and no fields to destructure, so only `_`, `else`,
    /// and a binding absorb anything.
    Opaque,
}

/// Whether a bare `Pattern::Ident` arm head is a *variant reference* rather
/// than a fresh binding.
///
/// Shape answers first, and it answers for every scrutinee kind: D9 fixes a
/// PascalCase head as a variant reference before any type is known, which is
/// why `glyph_resolver` resolves it as a name reference instead of binding it
/// (an unknown one is E0103) and why every emitter path lowers it to a
/// `{access}.tag === "Foo"` test rather than a `default:`. A head that binds
/// nothing and tests a tag cannot be a catch-all over a `bool` any more than
/// over a union; the checker calling it one there was claiming a binding the
/// rest of the compiler never makes.
///
/// The variant set answers the other half. Glyph accepts a lowercase variant
/// name, so `blank` is a reference over a union that declares it and a
/// binding anywhere else.
fn is_variant_reference(name: &Ident, scrutinee: Scrutinee<'_>) -> bool {
    if is_constructor_shaped(name) {
        return true;
    }
    match scrutinee {
        Scrutinee::Union(variants) => variants.iter().any(|v| v == name),
        Scrutinee::Record | Scrutinee::Opaque => false,
    }
}

/// **The** catch-all predicate: whether this arm matches every value the
/// scrutinee can still take, so no later arm can run and no earlier gap can
/// remain.
///
/// Every exhaustiveness and reachability check in this file routes through
/// here. They used to each carry their own version and disagree: four of them
/// read a PascalCase `Pattern::Ident` as a catch-all while the reachability
/// pass and `check_patterns_exhaustive` read it as a variant reference, so
/// `match b { true => .., Red => .. }` type-checked as exhaustive and then
/// failed in the emitter. One question, one answer.
///
/// Where two readings were defensible the reporting one wins, which is why an
/// object pattern is a catch-all only over `Scrutinee::Record`: crediting it
/// over a `bool` would swallow E0209.
fn is_catch_all_pattern(pat: &Pattern, scrutinee: Scrutinee<'_>) -> bool {
    match pat {
        Pattern::Wildcard { .. } | Pattern::Else { .. } => true,
        Pattern::Ident { name, .. } => !is_variant_reference(name, scrutinee),
        // `{ x, y }` over a record binds both fields and cannot fail;
        // `{ x: 0, y }` tests one and can. `is_refutable` already recurses on
        // the fields, so only the scrutinee's shape is decided here.
        Pattern::Object { .. } => matches!(scrutinee, Scrutinee::Record) && !pat.is_refutable(),
        // A literal, a constructor, an array, and an `is` guard each test
        // something, so each can fail. An array pattern that is nothing but a
        // rest binding is the one arguable case; `check_array_exhaustiveness`
        // credits it through its length algebra instead, where the answer is
        // exact rather than conservative.
        Pattern::Literal { .. }
        | Pattern::Constructor { .. }
        | Pattern::Array { .. }
        | Pattern::IsType { .. } => false,
    }
}

/// Whether a bare ident used as a `match` arm head is constructor-shaped: a
/// PascalCase name (`Idle`, `Ok`, `None`) denotes a union variant reference;
/// a lowercase or underscore-led name (`x`, `_rest`) is a fresh binding. This
/// is the single predicate shared by the reachability pass and
/// `check_patterns_exhaustive`, and it mirrors the resolver's identically
/// named check (`glyph_resolver::resolve`) so the three stages agree on what
/// counts as a variant reference.
fn is_constructor_shaped(name: &Ident) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// The nearest variant name to `name` by Levenshtein distance, when one is
/// close enough to be a plausible typo. Powers the `did you mean` hint on
/// `UnknownVariantPattern` (E0220). Returns `None` when every variant is too
/// far to be a likely misspelling, so the diagnostic never guesses wildly.
fn nearest_variant(name: &str, variants: &[Ident]) -> Option<String> {
    let name_len = name.chars().count();
    // A short name tolerates fewer edits than a long one; at least 2 so a
    // single transposition (`Loadign` vs `Loading`, distance 2) still matches.
    let threshold = (name_len / 3).max(2);
    variants
        .iter()
        .map(|v| (levenshtein(name, v.as_ref()), v))
        .filter(|(d, _)| *d <= threshold)
        .min_by_key(|(d, _)| *d)
        .map(|(_, v)| v.to_string())
}

/// Classic two-row Levenshtein edit distance over Unicode scalar values.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
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

    fn errors_of(src: &str) -> Vec<TypeError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        ty_errs
    }

    #[test]
    fn a_generic_records_parse_is_typed_from_the_calls_type_arguments() {
        // `T.parse` on a generic record has no signature of its own (its
        // descriptor takes one runtime checker per type parameter), so the
        // instantiation is read from the call. Leaving it `Unknown` made the
        // parsed value's fields invisible: a typo produced no Glyph diagnostic
        // at all, only a `tsc` TS2339 pointed at the whole enclosing function,
        // while the non-generic spelling gave E0210 at the field.
        let errs = errors_of(
            "module x\n\
             import std/result { Ok, Err }\n\
             type Wire<V> = { keys: Array<string>, values: Array<V> }\n\
             fn f(raw: unknown) -> string {\n\
             \x20 return match Wire.parse<number>(raw) {\n\
             \x20\x20\x20 Ok(w) => w.keyz,\n\
             \x20\x20\x20 Err(_e) => \"\",\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::UnknownField { .. })),
            "a typo on a generic-parsed value must be a Glyph error, not left to \
             `tsc`: {errs:?}"
        );
    }

    #[test]
    fn a_generic_parse_without_explicit_type_arguments_stays_unknown() {
        // There is nothing to infer them from: `parse` takes an `unknown`.
        // Guessing an instantiation would put a wrong shape behind a boundary
        // check, which is worse than staying opaque, so an un-annotated call
        // reports nothing rather than reporting something wrong.
        let errs = errors_of(
            "module x\n\
             import std/result { Ok, Err }\n\
             type Wire<V> = { keys: Array<string>, values: Array<V> }\n\
             fn f(raw: unknown) -> string {\n\
             \x20 return match Wire.parse(raw) {\n\
             \x20\x20\x20 Ok(w) => w.keyz,\n\
             \x20\x20\x20 Err(_e) => \"\",\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::UnknownField { .. })),
            "no type arguments means no instantiation to check against: {errs:?}"
        );
    }

    #[test]
    fn a_match_over_a_plural_category_is_exhaustive_over_the_six_cldr_names() {
        // The reason `std/intl` wraps `Intl` rather than exposing it: as a bare
        // `string` this match would be E0218, whose advice is to add an `else`,
        // and an `else` over a plural category is how a locale's `few` silently
        // renders as `other`.
        let all_six = "module x\n\
             import std/intl\n\
             fn label(n: number) -> string {\n\
             \x20 return match intl.plural_category(\"pl\", n) {\n\
             \x20\x20\x20 \"zero\" => \"z\",\n    \"one\" => \"o\",\n    \"two\" => \"t\",\n\
             \x20\x20\x20 \"few\" => \"f\",\n    \"many\" => \"m\",\n    \"other\" => \"x\",\n\
             \x20 }\n\
             }\n";
        let errs = errors_of(all_six);
        assert!(
            !errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveMatch { .. } | TypeError::NonExhaustiveValueMatch { .. }
            )),
            "covering all six categories needs no catch-all: {errs:?}"
        );

        // And a missing one is still reported, by name.
        let missing_zero = all_six.replace("    \"zero\" => \"z\",\n", "");
        let errs = errors_of(&missing_zero);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveMatch { .. } | TypeError::NonExhaustiveValueMatch { .. }
            )),
            "a missing category must be reported: {errs:?}"
        );
    }

    #[test]
    fn string_literal_union_match_is_exhaustive_without_else() {
        // Covering every literal of a string-literal union needs no `else` (it is
        // a bounded domain), unlike a bare `string` match.
        let errs = errors_of(
            "module x\ntype Tier = \"free\" | \"pro\"\n\
             fn label(t: Tier) -> string {\n  return match t {\n    \"free\" => \"F\",\n    \"pro\" => \"P\",\n  }\n}\n",
        );
        assert!(
            !errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveMatch { .. } | TypeError::NonExhaustiveValueMatch { .. }
            )),
            "a fully-covered literal union match should be exhaustive: {errs:?}"
        );
    }

    #[test]
    fn string_literal_union_match_missing_a_literal_errors() {
        let errs = errors_of(
            "module x\ntype Tier = \"free\" | \"pro\" | \"team\"\n\
             fn label(t: Tier) -> string {\n  return match t {\n    \"free\" => \"F\",\n    \"pro\" => \"P\",\n  }\n}\n",
        );
        let missing = errs.iter().find_map(|e| match e {
            TypeError::NonExhaustiveMatch { missing, .. } => Some(missing.clone()),
            _ => None,
        });
        assert_eq!(missing.as_deref(), Some("\"team\""), "errs: {errs:?}");
    }

    /// The value span of the first `let` in the first `fn` that has one.
    /// Unlike `first_let_value_span` this skips leading type declarations and
    /// functions whose body starts with something else.
    fn first_let_value_span_anywhere(m: &Module) -> glyph_ast::Span {
        for item in &m.items {
            let Decl::Fn(f) = item else { continue };
            for s in &f.body.stmts {
                if let Stmt::Let(l) = s {
                    return l.value.span();
                }
            }
        }
        panic!("no let statement in any fn");
    }

    #[test]
    fn match_with_arms_of_one_type_takes_that_type() {
        let (m, _, tm) = type_map_of(
            "module x\ntype Shape = Circle(number) | Square(number)\n\
             fn area(s: Shape) -> number {\n  let a = match s {\n    Circle(r) => r,\n    Square(w) => w,\n  }\n  return a\n}\n",
        );
        assert!(
            matches!(
                tm.get(first_let_value_span_anywhere(&m)),
                Ty::Prim(Primitive::Number)
            ),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
    }

    #[test]
    fn diverging_match_arm_does_not_block_the_join() {
        // `Err(_) => return 1` parses as a block holding a single `Stmt::Return`.
        // It contributes nothing to the join, so the match takes `Ok`'s type.
        let (m, _, tm) = type_map_of(
            "module x\nfn get() -> Result<number, string> {\n  return Ok(1)\n}\n\
             fn main() -> number {\n  let w = match get() {\n    Ok(v) => v,\n    Err(_) => return 1,\n  }\n  return w\n}\n",
        );
        assert!(
            matches!(
                tm.get(first_let_value_span_anywhere(&m)),
                Ty::Prim(Primitive::Number)
            ),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
    }

    #[test]
    fn match_with_arms_of_differing_types_stays_unknown_and_silent() {
        // The join is equality only: no widening, no union. Disagreeing arms
        // keep the pre-join behavior exactly, including reporting no error.
        let src = "module x\ntype Tier = \"free\" | \"pro\"\n\
             fn pick(t: Tier) -> number {\n  let v = match t {\n    \"free\" => 1,\n    \"pro\" => \"two\",\n  }\n  return 0\n}\n";
        let (m, _, tm) = type_map_of(src);
        assert!(
            tm.get(first_let_value_span_anywhere(&m)).is_unknown(),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
        assert!(
            errors_of(src).is_empty(),
            "differing arms must not error: {:?}",
            errors_of(src)
        );
    }

    #[test]
    fn match_whose_arms_all_diverge_stays_unknown() {
        // No `Never`/bottom type exists in Glyph; an all-divergent match keeps
        // `Unknown` rather than inventing one.
        let (m, _, tm) = type_map_of(
            "module x\ntype Tier = \"free\" | \"pro\"\n\
             fn pick(t: Tier) -> number {\n  let v = match t {\n    \"free\" => return 1,\n    \"pro\" => return 2,\n  }\n  return 0\n}\n",
        );
        assert!(
            tm.get(first_let_value_span_anywhere(&m)).is_unknown(),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
    }

    #[test]
    fn descriptor_parse_result_flows_through_a_match() {
        // `T.parse` is the boundary between untrusted input and typed data. Its
        // `Result<T, Array<Issue>>` signature is what lets the `Ok` arm bind a
        // real `Wire`, which is what the arm join then hands to the `let`.
        let (m, _, tm) = type_map_of(
            "module x\ntype Wire = { rows: Array<number> }\n\
             fn main() -> number {\n  let w = match Wire.parse(0) {\n    Ok(v) => v,\n    Err(_) => return 1,\n  }\n  return 0\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::Named { path, .. } if path.last().map(|n| n.as_ref()) == Some("Wire")),
            "got {ty:?}"
        );
    }

    #[test]
    fn plain_alias_has_no_descriptor_parse() {
        // A `type Cents = int` alias emits no runtime descriptor, so claiming a
        // `parse` signature for it would describe a member that is not there.
        let (m, _, tm) = type_map_of(
            "module x\ntype Cents = int\nfn main() -> number {\n  let c = Cents.parse(0)\n  return 0\n}\n",
        );
        assert!(
            tm.get(first_let_value_span_anywhere(&m)).is_unknown(),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
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
    fn untyped_let_infers_from_initializer() {
        // Week-2 task 5: `let x = 42` (no annotation) infers `number` from the
        // initializer, so later refs to `x` resolve concretely.
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
        assert!(matches!(tm.get(ret_val.span()), Ty::Prim(Primitive::Number)));
    }

    #[test]
    fn untyped_let_infers_through_call() {
        // Inference reads the initializer's synthesized type, so a call to a
        // string-returning fn makes the binding `string`.
        let src = r#"module x
fn greet() -> string { return "hi" }
fn main() -> string {
  let g = greet()
  return g
}
"#;
        let (m, _, tm) = type_map_of(src);
        let main = match &m.items[1] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ret_val = match &main.body.stmts[1] {
            Stmt::Return(r) => r.value.as_ref().unwrap(),
            _ => panic!(),
        };
        assert!(matches!(tm.get(ret_val.span()), Ty::Prim(Primitive::String)));
    }

    #[test]
    fn untyped_let_from_unknown_initializer_stays_open() {
        // When the initializer types as Unknown (a member access here), the
        // binding records nothing and refs stay Unknown — no false pinning.
        let src = r#"module x
fn main(s: string) -> number {
  let n = s.length
  return n
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
    fn nested_bare_ident_naming_a_record_type_is_e0220() {
        // `Point` is a record, not a union: a bare `Point` as `Ok`'s payload
        // sub-pattern reads (by shape, D9) as a variant reference, but the
        // payload it is tested against has no variants at all. Before this
        // fix `check_patterns_exhaustive`'s recursive call resolved
        // `payload_ty` to `Point` fine, then `required_variants(payload_ty)`
        // came back `None` for it the same way it does for an imported or
        // unresolvable type, and the whole check silently returned. The arm
        // then reached the tsc backend clean and failed there instead, on a
        // `.tag` property the emitter's PascalCase shape fallback
        // (`is_variant_shaped`) invented for a value nobody asked to be
        // tag-tested (G146). This must be caught here, at Glyph typecheck
        // time, distinctly from a genuinely unresolvable/imported payload
        // (which must stay silent) and from a wrong variant of a real union
        // (already covered by `typo_constructor_form_with_payload_is_e0220`).
        let src = r#"module x
type Point = { x: int, y: int }
fn f(r: Result<Point, string>) -> int {
  return match r {
    Ok(Point) => 1,
    Err(e) => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        let e0220 = errs
            .iter()
            .find(|e| e.code() == "E0220")
            .unwrap_or_else(|| {
                panic!("expected E0220 for the record-shaped nested payload; got {errs:?}")
            });
        let TypeError::UnknownVariantPattern {
            name,
            union,
            suggestion,
            ..
        } = e0220
        else {
            panic!("expected UnknownVariantPattern, got {e0220:?}");
        };
        assert_eq!(name, "Point");
        assert_eq!(union, "Result");
        assert_eq!(suggestion.as_deref(), None);
        assert!(
            !errs.iter().any(|e| e.code() == "E0200"),
            "`Ok` has a real arm; it must not also be reported as a missing variant: {errs:?}"
        );
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
    fn jsx_match_missing_variant_is_flagged() {
        // A JSX `<match value={s}>` directive runs the same tagged-union
        // exhaustiveness as a value-level `match`: a missing `<case>` is E0200,
        // not a silently-emitted `default: throw`.
        let src = r#"module x
type Status = | Idle | Loading | Done
component View(s: Status) -> Component {
  return <match value={s}>
    <case Idle><span>idle</span></case>
    <case Loading><span>loading</span></case>
  </match>
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Status");
                assert!(missing.contains("Done"), "missing: {missing}");
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn jsx_match_all_variants_covered_passes() {
        // The complement: every variant has a `<case>` (a `bind={x}` binds the
        // whole payload and covers the variant), so no exhaustiveness error.
        let src = r#"module x
type Status = | Idle | Loading | Done(number)
component View(s: Status) -> Component {
  return <match value={s}>
    <case Idle><span>idle</span></case>
    <case Loading><span>loading</span></case>
    <case Done bind={n}><span>{n}</span></case>
  </match>
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn undecidable_scrutinee_recovers_union_from_arms_and_flags_missing() {
        // BUG-01: a `match` whose scrutinee has an undecidable static type (a
        // member access through a value the checker cannot resolve — here
        // `state.value` on an `unknown`, standing in for a `.d.ts`
        // `StateHandle<Filter>`) must still be checked for exhaustiveness by
        // recovering the union from the arm patterns. Before the fix this
        // silently passed (and miscompiled the nullary arm as a binding).
        let src = r#"module x
type Filter = | All | Active | Done
fn run(state: unknown) -> string {
  return match state.value {
    All => "all",
  }
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                assert_eq!(type_name, "Filter");
                assert!(missing.contains("Active"), "missing: {missing}");
                assert!(missing.contains("Done"), "missing: {missing}");
            }
            other => panic!("expected NonExhaustiveMatch, got {other:?}"),
        }
    }

    #[test]
    fn undecidable_scrutinee_exhaustive_over_recovered_union_passes() {
        // The complement: once every variant of the recovered union is covered,
        // the undecidable-scrutinee match type-checks clean.
        let src = r#"module x
type Filter = | All | Active | Done
fn run(state: unknown) -> string {
  return match state.value {
    All => "all",
    Active => "active",
    Done => "done",
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
    fn array_match_with_object_element_rest_is_exhaustive() {
        // An object-destructure element binds any record value, so
        // `[{id}, ...rest]` covers all non-empty arrays — together with `[]`
        // the match is exhaustive and must not be flagged.
        let src = r#"module x
type Row = { id: number }
fn f(rows: Array<Row>) -> number {
  return match rows {
    [] => 0,
    [{ id }, ...rest] => id,
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
    fn typo_constructor_in_arm_is_rejected_not_bound_as_catchall() {
        // A typo'd bare variant name (`Loadign` vs `Loading`) in a match arm
        // used to be silently treated as a binding, acting as an irrefutable
        // catch-all: it masked non-exhaustiveness and misrouted variants at
        // runtime. A constructor-shaped (PascalCase) arm head that names no
        // known variant now resolves as a name reference and is rejected by
        // the resolver with E0103, mirroring the JSX `<case Variant>` path.
        // (The disambiguation lives in the resolver, so this asserts against
        // its errors rather than the exhaustiveness checker.)
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
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(
            errs.iter()
                .any(|e| format!("{e:?}").contains("Loadign")),
            "typo'd constructor must raise an unresolved-name error; got: {errs:?}"
        );
        // The typechecker independently escalates the typo to E0220 with a
        // union-scoped `did you mean` suggestion, and — because the typo is no
        // longer read as a catch-all — the genuinely missing `Failed` variant
        // now surfaces as E0200 alongside it.
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        let e0220 = ty_errs
            .iter()
            .find(|e| e.code() == "E0220")
            .expect("expected E0220 for the PascalCase typo");
        let TypeError::UnknownVariantPattern {
            name,
            union,
            suggestion,
            ..
        } = e0220
        else {
            panic!("expected UnknownVariantPattern, got {e0220:?}");
        };
        assert_eq!(name, "Loadign");
        assert_eq!(union, "Feed");
        assert_eq!(suggestion.as_deref(), Some("Loading"));
        assert!(
            format!("{e0220}").contains("did you mean `Loading`?"),
            "message should carry the suggestion: {e0220}"
        );
        assert!(
            ty_errs.iter().any(|e| e.code() == "E0200"),
            "the previously-swallowed missing-variant error should surface: {ty_errs:?}"
        );
    }

    #[test]
    fn pascal_arm_that_resolves_to_a_foreign_variant_is_e0220() {
        // A PascalCase arm head can *resolve* cleanly (here `Red` is a real
        // variant of another union) yet still not be a variant of the
        // scrutinee's union. The resolver is happy, so this is the case the
        // typechecker must catch: without E0220 the arm was read as a silent
        // catch-all, masking the missing `Failed`. `Red` is too far from any
        // `Feed` variant to suggest, so no `did you mean`.
        let src = r#"module x
type Color = | Red | Green | Blue
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loaded => 2,
    Red => 3,
  }
}
"#;
        let errs = ty_errors_of(src);
        let e0220 = errs
            .iter()
            .find(|e| e.code() == "E0220")
            .unwrap_or_else(|| panic!("expected E0220; got {errs:?}"));
        let TypeError::UnknownVariantPattern {
            name, suggestion, ..
        } = e0220
        else {
            panic!("expected UnknownVariantPattern, got {e0220:?}");
        };
        assert_eq!(name, "Red");
        assert_eq!(suggestion.as_deref(), None, "no near variant to suggest");
        assert!(
            errs.iter().any(|e| e.code() == "E0200"),
            "the missing `Failed` variant must still surface: {errs:?}"
        );
    }

    #[test]
    fn typo_constructor_form_with_payload_is_e0220() {
        // The payload-bearing shape of the same typo must escalate to E0220 too,
        // not slip through as a silently-dropped arm. `Loadign(x)` is the common
        // mistake shape for the prelude unions an agent touches most (`Errr(e)`,
        // `Somee(x)`); before this fix only the bare `Ident` head was caught and
        // the parenthesized form walked straight through. The genuinely missing
        // `Failed` still surfaces as E0200 because the dropped arm covers nothing.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loaded => 2,
    Loadign(x) => x,
  }
}
"#;
        // The resolver independently rejects the unresolved head; this test pins
        // the typechecker's E0220, so it drives resolve + assign directly rather
        // than through `ty_errors_of` (which asserts a clean resolve).
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        let e0220 = ty_errs
            .iter()
            .find(|e| e.code() == "E0220")
            .unwrap_or_else(|| panic!("expected E0220 for the constructor-form typo; got {ty_errs:?}"));
        let TypeError::UnknownVariantPattern {
            name,
            union,
            suggestion,
            ..
        } = e0220
        else {
            panic!("expected UnknownVariantPattern, got {e0220:?}");
        };
        assert_eq!(name, "Loadign");
        assert_eq!(union, "Feed");
        assert_eq!(suggestion.as_deref(), Some("Loading"));
        assert!(
            ty_errs.iter().any(|e| e.code() == "E0200"),
            "the missing `Failed` variant must still surface: {ty_errs:?}"
        );
    }

    #[test]
    fn typo_qualified_head_is_e0220() {
        // A qualified head `Feed.Loadign` parses as a `Constructor` with a
        // 2-segment path and no args; the last segment is the variant name and
        // must escalate identically to the bare and payload-bearing forms.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Feed.Loading => 1,
    Feed.Loaded => 2,
    Feed.Loadign => 3,
  }
}
"#;
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        let e0220 = ty_errs
            .iter()
            .find(|e| e.code() == "E0220")
            .unwrap_or_else(|| panic!("expected E0220 for the qualified typo; got {ty_errs:?}"));
        let TypeError::UnknownVariantPattern {
            name,
            union,
            suggestion,
            ..
        } = e0220
        else {
            panic!("expected UnknownVariantPattern, got {e0220:?}");
        };
        assert_eq!(name, "Loadign");
        assert_eq!(union, "Feed");
        assert_eq!(suggestion.as_deref(), Some("Loading"));
        assert!(
            ty_errs.iter().any(|e| e.code() == "E0200"),
            "the missing `Failed` variant must still surface: {ty_errs:?}"
        );
    }

    #[test]
    fn lowercase_binding_catch_all_stays_exhaustive() {
        // A lowercase bare head is a binding, an irrefutable catch-all — it
        // absorbs the rest of the union with no error, unchanged by E0220.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    rest => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "a lowercase binding catch-all is exhaustive: {errs:?}"
        );
    }

    #[test]
    fn all_correct_variants_draw_no_e0220() {
        // The happy path: every arm names a real variant. No spurious E0220.
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
            !errs.iter().any(|e| e.code() == "E0220"),
            "correct variant names must not draw E0220: {errs:?}"
        );
    }

    #[test]
    fn number_match_without_a_catch_all_is_non_exhaustive() {
        // `number`/`string` are unbounded, so literal arms alone can never be
        // exhaustive: an open value match is E0218 (not a silent runtime throw).
        let src = r#"module x
fn main(n: number) -> number {
  return match n {
    0 => 1,
    1 => 2,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| e.code() == "E0218"),
            "an open number match should be E0218; got: {errs:?}"
        );
    }

    #[test]
    fn number_match_with_an_else_is_exhaustive() {
        let src = r#"module x
fn main(n: number) -> number {
  return match n {
    0 => 1,
    else => 2,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn string_match_with_a_binding_catch_all_is_exhaustive() {
        // A bare-identifier binding absorbs the rest of the domain.
        let src = r#"module x
fn f(s: string) -> number {
  return match s {
    "a" => 1,
    other => 2,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    // ----- bool match exhaustiveness (week-3, D3) -----

    #[test]
    fn bool_match_covering_both_passes() {
        let src = r#"module x
fn f(b: bool) -> number {
  return match b {
    true => 1,
    false => 0,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn bool_match_missing_false_is_flagged() {
        let src = r#"module x
fn f(b: bool) -> number {
  return match b {
    true => 1,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveBoolMatch { missing, .. }] if missing.contains("false")),
            "expected NonExhaustiveBoolMatch missing false; got {errs:?}"
        );
    }

    #[test]
    fn bool_match_missing_true_is_flagged() {
        let src = r#"module x
fn f(b: bool) -> number {
  return match b {
    false => 0,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveBoolMatch { missing, .. }] if missing.contains("true")),
            "expected NonExhaustiveBoolMatch missing true; got {errs:?}"
        );
    }

    #[test]
    fn bool_match_with_wildcard_passes() {
        let src = r#"module x
fn f(b: bool) -> number {
  return match b {
    true => 1,
    _ => 0,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn bool_match_with_binding_passes() {
        // A bare-ident arm over a bool scrutinee is a binding catch-all.
        let src = r#"module x
fn f(b: bool) -> number {
  return match b {
    other => 0,
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn bool_comparison_scrutinee_missing_arm_is_flagged() {
        // `n > 0` is a comparison that types as Unknown, but a `true`/`false`
        // literal arm only type-checks over a bool, so the match is recovered as
        // a bool match from its arms and an incomplete one is flagged. (Before
        // the fix this slipped through and threw `non-exhaustive match` at run
        // time.)
        let src = r#"module x
fn f(n: number) -> number {
  return match n > 0 {
    true => 1,
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveBoolMatch { missing, .. }] if missing.contains("false")),
            "comparison scrutinee with a `true`-only match should be flagged non-exhaustive; got: {errs:?}"
        );
    }

    #[test]
    fn bool_comparison_scrutinee_exhaustive_passes() {
        // The complement: both arms present over a comparison type-checks clean.
        let src = r#"module x
fn f(n: number) -> number {
  return match n > 0 {
    true => 1,
    false => 0,
  }
}
"#;
        assert!(
            ty_errors_of(src).is_empty(),
            "exhaustive bool match over a comparison should pass; got: {:?}",
            ty_errors_of(src)
        );
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

    // ----- `?` operand rule (week-3 task 2): operand must be a Result -----

    #[test]
    fn question_on_non_result_operand_is_flagged() {
        // `n?` where `n: number`. The enclosing fn returns Result (so the
        // enclosing-fn rule passes), but the operand is decidably not a
        // Result, so the operand rule fires.
        let src = r#"module x
fn f(n: number) -> Result<number, string> {
  let x = n?
  return Ok(x)
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(&errs[0], TypeError::QuestionOnNonResult { found, .. } if found == "number"),
            "expected QuestionOnNonResult on number; got {:?}",
            errs[0]
        );
    }

    #[test]
    fn question_on_unknown_operand_is_permissive() {
        // `s.parse()?` types as Unknown (a member-access call), so the
        // operand rule cannot prove it isn't a Result and stays silent.
        let src = r#"module x
fn f(s: string) -> Result<number, string> {
  let v = s.parse()?
  return Ok(1)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`?` on an Unknown-typed operand must not be flagged; got: {errs:?}"
        );
    }

    #[test]
    fn question_unwraps_to_success_type() {
        // `inner()?` evaluates to the operand's success type (`number`), not
        // Unknown, so the bound `v` is `number` and downstream typing sees it.
        let src = r#"module x
fn inner() -> Result<number, string> { return Ok(1) }
fn outer() -> Result<number, string> {
  let v = inner()?
  return Ok(v)
}
"#;
        let (m, _, tm) = type_map_of(src);
        let outer = match &m.items[1] {
            Decl::Fn(f) => f,
            _ => panic!(),
        };
        let q_span = match &outer.body.stmts[0] {
            Stmt::Let(l) => l.value.span(),
            _ => panic!("first stmt is not a let"),
        };
        assert!(
            matches!(tm.get(q_span), Ty::Prim(Primitive::Number)),
            "`inner()?` should unwrap to number; got {:?}",
            tm.get(q_span)
        );
    }

    #[test]
    fn question_error_type_mismatch_is_flagged() {
        // The operand propagates `Err(A)`, but the enclosing fn returns
        // `Result<_, B>`. v1 has no `From`, so the mismatched error types are
        // flagged.
        let src = r#"module x
type A = | X
type B = | Y
fn inner() -> Result<number, A> { return Err(X) }
fn outer() -> Result<number, B> {
  let v = inner()?
  return Ok(v)
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(&errs[0], TypeError::QuestionErrorTypeMismatch { expected, found, .. }
                if expected == "B" && found == "A"),
            "expected QuestionErrorTypeMismatch B vs A; got {:?}",
            errs[0]
        );
    }

    #[test]
    fn question_matching_error_types_pass() {
        // Same error type on both sides: no mismatch.
        let src = r#"module x
type E = | X
fn inner() -> Result<number, E> { return Err(X) }
fn outer() -> Result<string, E> {
  let v = inner()?
  return Ok("ok")
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "matching `?` error types must pass; got: {errs:?}"
        );
    }

    #[test]
    fn question_on_stdlib_result_enforces_error_type() {
        // A stdlib TS-wrapper (`http.get -> Result<Response, HttpError>`) now
        // carries a Glyph-level signature, so its `E` (`HttpError`) is decidable
        // at `?`. Propagating it into a function that returns `Result<_, string>`
        // is an exact-error-type mismatch (E0203) at parity with a local error
        // type — previously it fell through to the `tsc` backstop with zero
        // diagnostics under `--no-check`.
        let src = r#"module x
import std/http
import std/result { Ok }
async fn f(url: string) -> Result<http.Response, string> {
  let r = await http.get(url)?
  return Ok(r)
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(
            matches!(&errs[0], TypeError::QuestionErrorTypeMismatch { expected, found, .. }
                if expected == "string" && found == "http.HttpError"),
            "expected QuestionErrorTypeMismatch string vs http.HttpError; got {:?}",
            errs[0]
        );
    }

    #[test]
    fn http_response_constructors_type_as_response() {
        // `html`/`redirect`/`with_header` build an `http.Response` and do not go
        // through a `Result`, so they need their own entry in the stdlib table.
        // Without it they type `Unknown` and a handler's `Ok(http.html(...))` is
        // checked only by `tsc` on the emitted TypeScript.
        let src = r#"module x
import std/http
fn f() {
  let a = http.html(200, "<p>hi</p>")
  let b = http.redirect(302, "/next")
  let c = http.with_header(a, "cache-control", "no-store")
}
"#;
        let (m, _, tm) = type_map_of(src);
        let f = match &m.items[1] {
            Decl::Fn(f) => f,
            other => panic!("second decl is not a Fn: {other:?}"),
        };
        for (idx, name) in ["html", "redirect", "with_header"].iter().enumerate() {
            let span = match &f.body.stmts[idx] {
                Stmt::Let(l) => l.value.span(),
                other => panic!("stmt {idx} is not a Let: {other:?}"),
            };
            assert_eq!(
                ty_display(tm.get(span)),
                "http.Response",
                "`http.{name}` should type as http.Response; got {:?}",
                tm.get(span)
            );
        }
    }

    #[test]
    fn question_on_map_erred_stdlib_result_is_permissive() {
        // The idiomatic conversion (`http.get(url).map_err(...)?`) makes the `?`
        // operand a method-call result typed `Unknown`, so the error-type rule
        // stays permissive and does not fire — mirrors `examples/02`. Guards
        // against the stdlib signatures introducing a false positive on the
        // correct idiom.
        let src = r#"module x
import std/http
import std/result { Ok }
type FeedError = | NetworkError
async fn f(url: string) -> Result<http.Response, FeedError> {
  let r = await http.get(url).map_err(fn(e) { NetworkError })?
  return Ok(r)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "`?` on a map_err'd stdlib Result must not be flagged; got: {errs:?}"
        );
    }

    #[test]
    fn question_error_mismatch_against_imported_result_returns_self() {
        // Regression guard for the example shape: when the enclosing fn and
        // the operand share the same module-local error type (via an imported
        // `Result`), no mismatch fires even though both `E`s are user Named
        // types.
        let src = r#"module x
import std/result { Result, Ok, Err }
type FeedError = | Boom
fn inner() -> Result<number, FeedError> { return Err(Boom) }
fn outer() -> Result<number, FeedError> {
  let v = inner()?
  return Ok(v)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.is_empty(),
            "same user error type via imported Result must pass; got: {errs:?}"
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
    fn let_string_annotation_with_number_initializer_is_flagged() {
        // G149: `let x: string = 42` must draw the same TypeMismatch a
        // mismatched `return` already draws (`check_return_type`, above).
        // Left unflagged, a `let` annotation is a lie the compiler prints
        // but never checks: an agent editing an initializer past its
        // declared type gets nothing from `glyph check` (only tsc's TS2322,
        // reported against the generated `.ts`) and nothing from `glyph lsp`
        // (a stale-annotation edit surfaces no diagnostic at all).
        let errs = ty_errors_of("module x\nfn f() -> void {\n  let x: string = 42\n}\n");
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        match &errs[0] {
            TypeError::TypeMismatch { expected, found, .. } => {
                assert_eq!(expected, "string");
                assert_eq!(found, "number");
            }
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
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

    // ----- G6a: member-access field checking -----

    /// A `for` binding carries the iterand's element type, so D30
    /// exhaustiveness survives a loop.
    ///
    /// Without it the binding was `Unknown` and degraded to `string`, so the
    /// diagnostic changed from "you have not handled `pro`" (E0200) to "a string
    /// match can never be exhaustive, add an `else`" (E0218): advice to switch
    /// the check off rather than to satisfy it.
    #[test]
    fn a_for_binding_keeps_the_element_type() {
        let errs = errors_of(
            "module x\ntype Tier = \"free\" | \"pro\"\n\
             pub fn f(ts: Array<Tier>) -> void {\n\
             \x20 for t in ts {\n\
             \x20   let _v = match t {\n\
             \x20     \"free\" => 1,\n\
             \x20   }\n\
             \x20 }\n\
             \x20 return void\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| e.code() == "E0200"),
            "expected the missing-variant error, got {errs:?}"
        );
    }

    /// The two-binding form (`for i, t in ts`) must keep the element's type
    /// too, not just the single-binding form (G37).
    ///
    /// Before per-binding spans, every name in a `for` statement was keyed by
    /// the whole statement's span, so a two-binding loop had no per-binding
    /// slot to hang a type on and both `i` and `t` stayed `Unknown`. The same
    /// D30 exhaustiveness match that reports E0200 for `for t in ts` degraded
    /// to E0218 ("a string match can never be exhaustive, add an `else`") the
    /// moment a caller wrote `for i, t in ts` instead, even though the body
    /// is identical: advice to switch the check off rather than to satisfy
    /// it, and silent in a clean `tsc --strict` build.
    #[test]
    fn a_two_binding_for_keeps_the_element_type() {
        let errs = errors_of(
            "module x\ntype Tier = \"free\" | \"pro\"\n\
             pub fn f(ts: Array<Tier>) -> void {\n\
             \x20 for i, t in ts {\n\
             \x20   let _v = match t {\n\
             \x20     \"free\" => 1,\n\
             \x20   }\n\
             \x20 }\n\
             \x20 return void\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| e.code() == "E0200"),
            "expected the missing-variant error, got {errs:?}"
        );
        assert!(
            !errs.iter().any(|e| e.code() == "E0218"),
            "the string-exhaustiveness fallback should not fire once `t` \
             carries the element type, got {errs:?}"
        );
    }

    /// Reading a key out of a map is a guess, so it is E0224.
    ///
    /// The compiler cannot know the key is there, and typing the read as `V`
    /// states something it has not checked: the value is `undefined` when the
    /// key is absent, under a type saying otherwise. A mistyped column name
    /// read off a database row compiled clean, passed `tsc --strict`, and
    /// rendered as the text "undefined".
    #[test]
    fn reading_a_key_from_a_map_is_flagged() {
        for src in [
            "module x\npub fn f(m: Record<string, int>) -> unknown {\n  return m.naem\n}\n",
            "module x\npub fn f(m: Record<string, int>) -> unknown {\n  return m[\"naem\"]\n}\n",
            // Through an alias, which is how a map is actually spelled in a
            // program: nobody annotates `Record<string, unknown>` twice.
            "module x\ntype M = Record<string, int>\npub fn f(m: M) -> unknown {\n  return m.naem\n}\n",
        ] {
            let errs = errors_of(src);
            assert!(
                errs.iter().any(|e| e.code() == "E0224"),
                "expected E0224 for:\n{src}\ngot {errs:?}"
            );
        }
    }

    /// Writing a key is how a map is built, and is always safe.
    ///
    /// The first cut flagged `mut m[k] = v` and lit up twenty call sites across
    /// the examples, every one of them a write. An lvalue index is not a read.
    #[test]
    fn writing_a_key_into_a_map_is_not_flagged() {
        let errs = errors_of(
            "module x\npub fn f() -> void {\n\
             \x20 let m: Record<string, int> = {}\n\
             \x20 mut m[\"a\"] = 1\n\
             \x20 return void\n\
             }\n",
        );
        assert!(
            !errs.iter().any(|e| e.code() == "E0224"),
            "a write must not be flagged, got {errs:?}"
        );
    }

    /// An array index is not a map read. The bound is a value, and a program
    /// that has just measured `array.len` is not guessing.
    #[test]
    fn indexing_an_array_is_not_flagged() {
        let errs = errors_of(
            "module x\npub fn f(xs: Array<int>) -> int {\n  return xs[0]\n}\n",
        );
        assert!(
            !errs.iter().any(|e| e.code() == "E0224"),
            "array indexing must stay unchecked, got {errs:?}"
        );
    }

    #[test]
    fn member_typo_on_a_record_is_flagged() {
        // `u.naem` on a `User` record (no such field) is an UnknownField error.
        let src = r#"module x
type User = { name: string }
fn label(u: User) -> string {
  return u.naem
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::UnknownField { field, type_name, .. }]
                if field == "naem" && type_name == "User"
        ), "errs: {errs:?}");
    }

    #[test]
    fn valid_member_access_is_not_flagged() {
        // `u.name` exists; no error, and the member types as the field's type.
        let src = r#"module x
type User = { name: string }
fn label(u: User) -> string {
  return u.name
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    // ----- structural interfaces as ordinary types (D34) -----

    #[test]
    fn record_satisfying_an_interface_type_is_accepted() {
        // `Widget` carries `key: number` and a `label` method, so it satisfies
        // the structural interface `Labeled` used as an ordinary parameter type.
        // The nominal `Named`-vs-`Named` name check must not fire here.
        let src = r#"module x
interface Labeled {
  key: number
  fn label() -> string
}
type Widget = { key: number, label: fn() -> string }
fn describe(x: Labeled) -> number {
  return x.key
}
fn use(w: Widget) -> number {
  return describe(w)
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn record_missing_an_interface_member_is_rejected() {
        // `Bare` lacks the `label` method the interface requires, so passing it
        // where a `Labeled` is expected is a provable argument mismatch.
        let src = r#"module x
interface Labeled {
  key: number
  fn label() -> string
}
type Bare = { key: number }
fn describe(x: Labeled) -> number {
  return x.key
}
fn use(b: Bare) -> number {
  return describe(b)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "Labeled" && found == "Bare"
            )),
            "a record missing an interface member should mismatch: {errs:?}"
        );
    }

    #[test]
    fn member_access_on_an_interface_typed_value_is_checked() {
        // Accessing a member the interface does not declare is an UnknownField
        // error: an interface exposes exactly its declared members.
        let src = r#"module x
interface Labeled {
  fn label() -> string
}
fn f(x: Labeled) -> string {
  return x.missing()
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::UnknownField { field, .. } if field == "missing"
            )),
            "an undeclared interface member access should be flagged: {errs:?}"
        );
    }

    #[test]
    fn member_on_a_non_record_is_not_flagged() {
        // `.length` on an `Array` and a member on an unknown-typed value must
        // not false-positive: only a decidable record's fields are checked.
        let src = r#"module x
fn count(xs: Array<number>) -> number {
  return xs.length
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn nested_member_access_is_checked_through_field_types() {
        // `fridge.items` types as `Array<Item>`; `.bogus` on the Item record
        // (reached via a further access) is still checked once the field type
        // is a record. Here the typo is on the outer record's field.
        let src = r#"module x
type Item = { name: string }
type Bag = { items: Item }
fn f(b: Bag) -> string {
  return b.items.naem
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::UnknownField { field, type_name, .. }]
                if field == "naem" && type_name == "Item"
        ), "errs: {errs:?}");
    }

    // ----- G6b: call-argument type checking -----

    #[test]
    fn argument_type_mismatch_is_flagged() {
        // Passing a `string` where a `number` is expected.
        let src = r#"module x
fn takes_number(n: number) -> number {
  return n
}
fn f() -> number {
  return takes_number("hi")
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::ArgumentTypeMismatch { expected, found, .. }]
                if expected == "number" && found == "string"
        ), "errs: {errs:?}");
    }

    #[test]
    fn correct_argument_is_not_flagged() {
        let src = r#"module x
fn takes_number(n: number) -> number {
  return n
}
fn f() -> number {
  return takes_number(5)
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn generic_argument_is_not_flagged() {
        // A generic parameter accepts any concrete argument — no false positive.
        let src = r#"module x
fn id<T>(x: T) -> T {
  return x
}
fn f() -> number {
  return id(5)
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    #[test]
    fn named_type_argument_mismatch_is_flagged() {
        // Distinct named types are nominally incompatible (Q15).
        let src = r#"module x
type A = { x: number }
type B = { y: number }
fn takes_a(a: A) -> number {
  return a.x
}
fn f(b: B) -> number {
  return takes_a(b)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "errs: {errs:?}"
        );
    }

    // ----- BUG-03: call arity -----

    #[test]
    fn too_few_arguments_is_flagged() {
        // BUG-03: `add(1)` for a two-param `fn` must be rejected by Glyph's own
        // checker (previously the `zip` truncated to the shorter side and only
        // `tsc` caught it; `glyph run --no-check` printed `NaN`).
        let src = r#"module x
fn add(a: number, b: number) -> number {
  return a + b
}
fn f() -> number {
  return add(1)
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::ArgumentCountMismatch { expected, found, .. }]
                if *expected == 2 && *found == 1
        ), "errs: {errs:?}");
    }

    #[test]
    fn component_with_multiple_params_is_rejected() {
        // A component lowers to a props-first React call, so >1 positional
        // parameter would silently bind the first to the whole props object.
        let src = "module x\ncomponent M(a: string, b: number) -> Component { return <p>{a}</p> }\n";
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::ComponentMultipleParams { count: 2, .. }]),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn component_with_single_props_record_passes() {
        let src = "module x\ntype P = { a: string }\ncomponent M(props: P) -> Component { return <p>{props.a}</p> }\n";
        assert!(
            ty_errors_of(src).is_empty(),
            "single props-record component should pass; got: {:?}",
            ty_errors_of(src)
        );
    }

    #[test]
    fn too_many_arguments_is_flagged() {
        // BUG-03: extra trailing arguments must also be rejected.
        let src = r#"module x
fn add(a: number, b: number) -> number {
  return a + b
}
fn f() -> number {
  return add(1, 2, 99)
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::ArgumentCountMismatch { expected, found, .. }]
                if *expected == 2 && *found == 3
        ), "errs: {errs:?}");
    }

    #[test]
    fn correct_arity_is_not_flagged() {
        let src = r#"module x
fn add(a: number, b: number) -> number {
  return a + b
}
fn f() -> number {
  return add(1, 2)
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn zero_arg_call_with_no_params_is_not_flagged() {
        let src = r#"module x
fn zero() -> number {
  return 0
}
fn f() -> number {
  return zero()
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    // ----- G15: mut on a const -----

    #[test]
    fn reassigning_a_const_is_flagged() {
        let src = r#"module x
const N = 5
fn f() -> void {
  mut N = 6
  return void
}
"#;
        let errs = ty_errors_of(src);
        assert!(matches!(
            errs.as_slice(),
            [TypeError::MutateConst { name, .. }] if name == "N"
        ), "errs: {errs:?}");
    }

    #[test]
    fn reassigning_a_let_is_not_flagged() {
        let src = r#"module x
fn f() -> number {
  let x = 1
  mut x = 2
  return x
}
"#;
        assert!(ty_errors_of(src).is_empty());
    }

    // ----- arm reachability: an irrefutable arm shadows every later arm -----

    #[test]
    fn binding_catch_all_before_a_variant_arm_is_unreachable() {
        // A leading binding catch-all (`other`) matches every value, so the
        // later `Idle` arm is dead code. Without this check the emitter lowers
        // `other` to a `switch` `default` and the `Idle` `case` silently wins
        // at runtime (first-match-wins violation).
        let src = r#"module x
type Status = | Idle | Loading | Done
fn label(s: Status) -> string {
  return match s {
    other => "other",
    Idle => "idle",
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::UnreachableMatchArm { .. }]),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn wildcard_before_a_variant_arm_is_unreachable() {
        let src = r#"module x
type Status = | Idle | Loading | Done
fn label(s: Status) -> string {
  return match s {
    _ => "other",
    Idle => "idle",
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::UnreachableMatchArm { .. }]),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn binding_catch_all_as_the_last_arm_is_fine() {
        // The catch-all in final position is the normal, legal shape and must
        // not be flagged.
        let src = r#"module x
type Status = | Idle | Loading | Done
fn label(s: Status) -> string {
  return match s {
    Idle => "idle",
    other => "other",
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn a_variant_arm_before_another_variant_arm_is_reachable() {
        // Two specific variant arms don't shadow each other; only the trailing
        // catch-all absorbs the rest.
        let src = r#"module x
type Status = | Idle | Loading | Done
fn label(s: Status) -> string {
  return match s {
    Idle => "idle",
    Loading => "loading",
    Done => "done",
  }
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    #[test]
    fn definitely_incompatible_strengthened_cases() {
        let num = || Ty::Prim(Primitive::Number);
        let string = || Ty::Prim(Primitive::String);
        let void = || Ty::Prim(Primitive::Void);
        let rec = |ty: Ty| Ty::Record {
            fields: vec![RecordField {
                name: "a".into(),
                ty,
                optional: false,
            }],
        };
        let func = |ret: Ty| Ty::Fn {
            params: vec![],
            return_ty: Arc::new(ret),
            is_async: false,
        };

        // A concrete scalar is never a record or a function, in either direction.
        assert!(definitely_incompatible(&num(), &rec(num())));
        assert!(definitely_incompatible(&rec(num()), &num()));
        assert!(definitely_incompatible(&num(), &func(num())));
        assert!(definitely_incompatible(&func(num()), &num()));
        // `void` is excluded (subtler assignability), so it is not flagged.
        assert!(!definitely_incompatible(&void(), &rec(num())));

        // Function return covariance: a `string`-returning fn is not assignable
        // where a `number`-returning one is expected.
        assert!(definitely_incompatible(&func(string()), &func(num())));
        // A `void` return on either side is skipped (callback contravariance and
        // the un-annotated-lambda `void` stub must not be trusted).
        assert!(!definitely_incompatible(&func(void()), &func(num())));
        assert!(!definitely_incompatible(&func(num()), &func(void())));

        // Structural records: a field-type mismatch, or a required field of the
        // expected type the found type lacks.
        assert!(definitely_incompatible(&rec(string()), &rec(num())));
        let empty = Ty::Record { fields: vec![] };
        assert!(definitely_incompatible(&empty, &rec(num())));
        // Extra fields in `found` are fine (width subtyping).
        assert!(!definitely_incompatible(&rec(num()), &empty));
        // Identical records are compatible.
        assert!(!definitely_incompatible(&rec(num()), &rec(num())));
    }

    #[test]
    fn an_async_function_type_is_incompatible_with_a_sync_one() {
        // D40: `async fn() -> T` emits `() => Promise<T>`. Comparing only the
        // return type let a sync function stand where an async one was declared,
        // leaving the distinction to `tsc`.
        let num = || Ty::Prim(Primitive::Number);
        let void = || Ty::Prim(Primitive::Void);
        let func = |ret: Ty, is_async: bool| Ty::Fn {
            params: vec![],
            return_ty: Arc::new(ret),
            is_async,
        };

        assert!(definitely_incompatible(
            &func(num(), false),
            &func(num(), true)
        ));
        assert!(definitely_incompatible(
            &func(num(), true),
            &func(num(), false)
        ));
        // Same asyncness, same return: compatible.
        assert!(!definitely_incompatible(
            &func(num(), true),
            &func(num(), true)
        ));
        // A `void` return on either side keeps the whole arm permissive, so an
        // asyncness mismatch is not judged there either.
        assert!(!definitely_incompatible(
            &func(void(), false),
            &func(num(), true)
        ));
        assert!(!definitely_incompatible(
            &func(num(), true),
            &func(void(), false)
        ));
    }

    #[test]
    fn returning_a_sync_fn_where_an_async_fn_type_is_declared_is_flagged() {
        let errs = errors_of(
            "module x\nasync fn slow() -> number { return 1 }\n\
             fn fast() -> number { return 1 }\n\
             fn pick() -> async fn() -> number {\n  return fast\n}\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "expected a return-type mismatch: {errs:?}"
        );

        let ok = errors_of(
            "module x\nasync fn slow() -> number { return 1 }\n\
             fn pick() -> async fn() -> number {\n  return slow\n}\n",
        );
        assert!(ok.is_empty(), "errs: {ok:?}");
    }

    #[test]
    fn passing_a_sync_fn_into_an_async_fn_parameter_is_flagged() {
        // The argument position, which reports E0211 rather than the return's
        // E0204. Both directions are wrong: an async value does not fit a plain
        // `fn` parameter either.
        let src = "module x\nfn fast() -> number { return 1 }\n\
                   async fn slow() -> number { return 2 }\n\
                   fn takes_async(f: async fn() -> number) -> string { return \"ok\" }\n\
                   fn takes_sync(f: fn() -> number) -> string { return \"ok\" }\n";
        let bad = errors_of(&format!(
            "{src}fn go() -> string {{\n  return takes_async(fast)\n}}\n"
        ));
        assert!(
            bad.iter()
                .any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "expected an argument-type mismatch: {bad:?}"
        );
        let reverse = errors_of(&format!(
            "{src}fn go() -> string {{\n  return takes_sync(slow)\n}}\n"
        ));
        assert!(
            reverse
                .iter()
                .any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "expected an argument-type mismatch: {reverse:?}"
        );
        let ok = errors_of(&format!(
            "{src}fn go() -> string {{\n  return takes_async(slow)\n}}\n"
        ));
        assert!(ok.is_empty(), "errs: {ok:?}");
    }

    #[test]
    fn an_async_lambda_satisfies_an_async_fn_type() {
        // A lambda carries its own `async`, so the new comparison must not
        // report a mismatch on the shape a caller actually writes.
        let errs = errors_of(
            "module x\nfn pick() -> async fn() -> number {\n  \
             return async fn() { return 1 }\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    // ----- E0222: `await` outside an `async fn` -----

    #[test]
    fn await_in_a_sync_fn_is_flagged() {
        let errs = errors_of(
            "module x\nasync fn slow() -> number { return 1 }\n\
             fn nope() -> number {\n  return await slow()\n}\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::AwaitOutsideAsyncFn { .. })),
            "expected E0222: {errs:?}"
        );
    }

    #[test]
    fn await_in_an_async_fn_is_fine() {
        let errs = errors_of(
            "module x\nasync fn slow() -> number { return 1 }\n\
             async fn ok() -> number {\n  return await slow()\n}\n",
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, TypeError::AwaitOutsideAsyncFn { .. })),
            "an `await` in an `async fn` must not be flagged: {errs:?}"
        );
    }

    #[test]
    fn await_in_a_sync_lambda_inside_an_async_fn_is_flagged() {
        // The innermost enclosing callable decides, as it does in TypeScript.
        let sync = errors_of(
            "module x\nasync fn slow(n: number) -> number { return n }\n\
             async fn run(xs: Array<number>) -> Array<number> {\n  \
               return array.map(xs, fn(x: number) -> number { return await slow(x) })\n}\n",
        );
        assert!(
            sync.iter()
                .any(|e| matches!(e, TypeError::AwaitOutsideAsyncFn { .. })),
            "expected E0222 for the sync lambda: {sync:?}"
        );

        let asyncy = errors_of(
            "module x\nasync fn slow(n: number) -> number { return n }\n\
             async fn run(xs: Array<number>) -> Array<number> {\n  \
               return array.map(xs, async fn(x: number) -> number { return await slow(x) })\n}\n",
        );
        assert!(
            !asyncy
                .iter()
                .any(|e| matches!(e, TypeError::AwaitOutsideAsyncFn { .. })),
            "an `async fn` lambda may await: {asyncy:?}"
        );
    }

    // ----- E0223: a value-position `match` arm that produces no value -----

    #[test]
    fn empty_arm_in_a_let_bound_match_is_flagged() {
        let errs = errors_of(
            "module x\ntype Status = | Loading | Ready(string)\n\
             fn label(s: Status) -> string {\n  \
               let out = match s {\n    Loading => {},\n    Ready(v) => v,\n  }\n  \
               return out\n}\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::MatchArmProducesNoValue { .. })),
            "expected E0223: {errs:?}"
        );
    }

    #[test]
    fn empty_arm_in_a_tail_match_of_a_typed_fn_is_flagged() {
        let errs = errors_of(
            "module x\ntype Status = | Loading | Ready(string)\n\
             fn label(s: Status) -> string {\n  \
               match s {\n    Loading => {},\n    Ready(v) => v,\n  }\n}\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::MatchArmProducesNoValue { .. })),
            "expected E0223: {errs:?}"
        );
    }

    #[test]
    fn statement_position_empty_arm_stays_legal() {
        // `X => {}` as a deliberate no-op is used across the corpus; a `void`
        // function's tail match is a statement, not a value.
        let errs = errors_of(
            "module x\ntype Status = | Loading | Ready(string)\n\
             fn note(s: Status) -> void {\n  \
               match s {\n    Loading => {},\n    Ready(v) => {},\n  }\n}\n",
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, TypeError::MatchArmProducesNoValue { .. })),
            "a statement-position `=> {{}}` must stay legal: {errs:?}"
        );
    }

    #[test]
    fn diverging_and_block_valued_arms_are_not_flagged() {
        let errs = errors_of(
            "module x\ntype Status = | Loading | Ready(string)\n\
             fn label(s: Status) -> string {\n  \
               let out = match s {\n    \
                 Loading => { return \"none\" },\n    \
                 Ready(v) => { let t = v\n t },\n  }\n  \
               return out\n}\n",
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, TypeError::MatchArmProducesNoValue { .. })),
            "a diverging arm and a block ending in an expression both produce values: {errs:?}"
        );
    }

    #[test]
    fn a_valueless_arm_in_a_nested_tail_match_is_flagged() {
        let errs = errors_of(
            "module x\ntype Status = | Loading | Ready(string)\n\
             fn label(s: Status, n: number) -> string {\n  \
               let out = match s {\n    \
                 Loading => \"none\",\n    \
                 Ready(v) => match n {\n      1 => v,\n      else => {},\n    },\n  }\n  \
               return out\n}\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::MatchArmProducesNoValue { .. })),
            "a nested value-position arm inherits the position: {errs:?}"
        );
    }

    // ----- stdlib return types and stdlib named-type shapes -----

    #[test]
    fn string_split_types_as_an_array_of_string() {
        // The headline of the modeled `std/string` table: without it the call
        // typed `Unknown`, so a `let` bound to it needed a hand-written
        // `Array<string>` annotation to iterate with an index.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/string\nfn f(text: string) -> void {\n  let parts = string.split(text, \",\")\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { base, args }
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Array"))
                    && args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn array_filter_carries_the_element_type_through() {
        // The element type travels as a `Ty::Param` bound from the argument, so
        // `array.filter(names, ...)` over an `Array<string>` is an
        // `Array<string>` with no annotation.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/array\nfn short(s: string) -> bool { return true }\n             fn f(names: Array<string>) -> void {\n  let kept = array.filter(names, short)\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { args, .. } if args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn an_async_callback_to_an_array_predicate_is_a_glyph_error() {
        // `array.filter(xs, some_async_fn)` was a back-end TS2322 talking about
        // `Promise<boolean>` and pointing at the whole `let`. The callback
        // parameters of the five predicate-taking array functions are modeled as
        // synchronous, so the mismatch is E0211 at the argument instead.
        for call in [
            "array.filter(names, slow)",
            "array.find(names, slow)",
            "array.any(names, slow)",
        ] {
            let errs = errors_of(&format!(
                "module x\nimport std/array\n\
                 async fn slow(s: string) -> bool {{ return true }}\n\
                 fn f(names: Array<string>) -> void {{\n  let out = {call}\n  return void\n}}\n"
            ));
            assert!(
                errs.iter()
                    .any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
                "{call} should be an argument-type mismatch: {errs:?}"
            );
        }
    }

    #[test]
    fn a_synchronous_predicate_still_satisfies_an_array_function() {
        // The other direction of the same modeling: the shape everyone writes
        // must keep compiling, and `filter` must keep returning the element type.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/array\nfn short(s: string) -> bool { return true }\n\
             fn f(names: Array<string>) -> void {\n  let kept = array.filter(names, short)\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { args, .. } if args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn array_map_takes_its_element_type_from_the_callback() {
        // `array.map`'s result element comes from the callback's *return*, not
        // from any argument, which is why it needs the second type parameter
        // `U`. This asserted `Ty::Unknown` for eight releases, describing the
        // signature as "deliberately absent"; that absence is what let an
        // `async fn` callback through to print `[object Promise]` (G99).
        let (m, _, tm) = type_map_of(
            "module x\nimport std/array\nfn dup(s: string) -> string { return s }\n             fn f(names: Array<string>) -> void {\n  let out = array.map(names, dup)\n  return void\n}\n",
        );
        assert!(
            matches!(tm.get(first_let_value_span_anywhere(&m)),
                     Ty::App { args, .. } if args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
    }

    #[test]
    fn array_max_and_min_type_as_an_option_of_number() {
        // G100. The `Option` is the point: an empty array has no maximum, so
        // the empty case becomes a `None` arm the exhaustiveness checker asks
        // for rather than a 0 the caller has to remember to distrust. Left
        // unmodeled the call types `Unknown` and that `match` goes unchecked.
        for call in ["array.max(xs)", "array.min(xs)"] {
            let (m, _, tm) = type_map_of(&format!(
                "module x\nimport std/array\nfn f(xs: Array<number>) -> void {{\n  let best = {call}\n  return void\n}}\n"
            ));
            let ty = tm.get(first_let_value_span_anywhere(&m));
            assert!(
                matches!(ty, Ty::App { base, args }
                    if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Option"))
                        && args.first() == Some(&Ty::Prim(Primitive::Number))),
                "{call}: got {:?}",
                tm.get(first_let_value_span_anywhere(&m))
            );
        }
    }

    #[test]
    fn array_sum_types_as_a_bare_number() {
        // The one reduction of the five with nothing to unwrap: the sum of no
        // numbers is 0, which is an answer rather than a stand-in for one.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/array\nfn f(xs: Array<number>) -> void {\n  let total = array.sum(xs)\n  return void\n}\n",
        );
        assert!(
            matches!(
                tm.get(first_let_value_span_anywhere(&m)),
                Ty::Prim(Primitive::Number)
            ),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
    }

    #[test]
    fn array_max_by_and_min_by_carry_the_element_type_through() {
        // The element rides on parameter 0 the way `find`'s does, so the
        // `Some(x)` binding of a `match` over `max_by` is the array's element
        // and a field typo on it is E0210 here instead of a `tsc` error later.
        for call in ["array.max_by(names, size)", "array.min_by(names, size)"] {
            let (m, _, tm) = type_map_of(&format!(
                "module x\nimport std/array\nfn size(s: string) -> number {{ return 1 }}\n\
                 fn f(names: Array<string>) -> void {{\n  let best = {call}\n  return void\n}}\n"
            ));
            let ty = tm.get(first_let_value_span_anywhere(&m));
            assert!(
                matches!(ty, Ty::App { base, args }
                    if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Option"))
                        && args.first() == Some(&Ty::Prim(Primitive::String))),
                "{call}: got {:?}",
                tm.get(first_let_value_span_anywhere(&m))
            );
        }
    }

    #[test]
    fn an_async_key_to_an_array_reduction_is_a_glyph_error() {
        // Same reason the predicate-taking array functions model a synchronous
        // callback (G99). An `async fn` key returns a `Promise<number>`, every
        // `>` against it is false, and `max_by` would hand back the first
        // element out of a build `tsc --strict` passed.
        for call in ["array.max_by(names, slow)", "array.min_by(names, slow)"] {
            let errs = errors_of(&format!(
                "module x\nimport std/array\n\
                 async fn slow(s: string) -> number {{ return 1 }}\n\
                 fn f(names: Array<string>) -> void {{\n  let out = {call}\n  return void\n}}\n"
            ));
            assert!(
                errs.iter()
                    .any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
                "{call} should be an argument-type mismatch: {errs:?}"
            );
        }
    }

    #[test]
    fn a_reduction_called_with_the_wrong_arity_errors() {
        // `sum` takes the array and nothing else; `max_by` takes the array and
        // a key. Modeling the arity is what makes a stray second argument
        // E0213 at the call rather than a `tsc` message about the emitted TS.
        for call in ["array.sum(xs, 1)", "array.max(xs, 1)", "array.max_by(xs)"] {
            let errs = errors_of(&format!(
                "module x\nimport std/array\nfn f(xs: Array<number>) -> void {{\n  let out = {call}\n  return void\n}}\n"
            ));
            assert!(
                errs.iter()
                    .any(|e| matches!(e, TypeError::ArgumentCountMismatch { .. })),
                "{call} should be an argument-count mismatch: {errs:?}"
            );
        }
    }

    #[test]
    fn record_get_carries_the_value_type_through() {
        // `std/record` was entirely unmodeled, so `record.get(t, k)` typed
        // `Unknown`, the `Some(p)` binding of a `match` over it bound nothing,
        // and a two-binding `for` over the result took the `Object.entries`
        // lowering: `i` bound the string `"0"` on a build `tsc --strict`
        // passed. `V` binds off the argument, so this is `Option<Array<string>>`.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/record\nfn f(t: Record<string, Array<string>>, k: string) -> void {\n  let hit = record.get(t, k)\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        let inner = match ty {
            Ty::App { base, args }
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Option")) =>
            {
                args.first().cloned()
            }
            _ => None,
        };
        assert!(
            matches!(inner, Some(Ty::App { ref base, ref args })
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Array"))
                    && args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn record_keys_types_as_an_array_of_string() {
        // The ordered-walk idiom is `array.sort(record.keys(t), cmp)`. With
        // `keys` unmodeled, `sort` bound its `T` from `Unknown` and the whole
        // walk was untyped.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/record\nfn f(t: Record<string, number>) -> void {\n  let ks = record.keys(t)\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { base, args }
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Array"))
                    && args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn record_set_returns_the_record_type() {
        let (m, _, tm) = type_map_of(
            "module x\nimport std/record\nfn f(t: Record<string, number>) -> void {\n  let next = record.set(t, \"a\", 1)\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { base, args }
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Record"))
                    && args.as_slice() == [Ty::Prim(Primitive::String), Ty::Prim(Primitive::Number)]),
            "got {ty:?}"
        );
    }

    #[test]
    fn a_match_arm_returning_an_empty_array_keeps_the_array_head() {
        // `[]` is `Array<Unknown>` (`infer_array_elem_ty`), which under a plain
        // equality join disagreed with the other arm's `Array<string>` and sank
        // the whole `match` to `Unknown`. The emitter reads only the head to
        // pick the `for` lowering, so that hole shipped a wrong program.
        let (m, _, tm) = type_map_of(
            "module x\nfn f(o: Option<Array<string>>) -> void {\n  let path = match o {\n    Some(p) => p,\n    None => [],\n  }\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { base, args }
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Array"))
                    && args.first() == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn a_match_over_two_different_heads_still_joins_to_unknown() {
        // The join is still equality at the head: only type *arguments* under an
        // already-agreeing head absorb `Unknown`.
        let (m, _, tm) = type_map_of(
            "module x\nfn f(o: Option<Array<string>>) -> void {\n  let v = match o {\n    Some(p) => p,\n    None => 0,\n  }\n  return void\n}\n",
        );
        assert!(
            matches!(tm.get(first_let_value_span_anywhere(&m)), Ty::Unknown),
            "got {:?}",
            tm.get(first_let_value_span_anywhere(&m))
        );
    }

    #[test]
    fn a_modeled_stdlib_call_with_the_wrong_arity_errors() {
        let errs = errors_of(
            "module x\nimport std/string\nfn f(text: string) -> void {\n  let n = string.len(text, text)\n  return void\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::ArgumentCountMismatch { .. })),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn fs_error_kind_resolves_to_the_closed_union() {
        // `e.kind` on a declared `fs.FsError` was `Unknown`, which is why a
        // `match` over it needed an `else` arm. Covering every kind is now
        // exhaustive on its own.
        let errs = errors_of(
            "module x\nimport std/fs\nfn reason(e: fs.FsError) -> string {\n  return match e.kind {\n             \x20   fs.ErrorKind.NotFound => \"missing\",\n             \x20   fs.ErrorKind.IsADirectory => \"dir\",\n             \x20   fs.ErrorKind.NotADirectory => \"not dir\",\n             \x20   fs.ErrorKind.PermissionDenied => \"denied\",\n             \x20   fs.ErrorKind.AlreadyExists => \"exists\",\n             \x20   fs.ErrorKind.Other({ code }) => code,\n  }\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_match_on_fs_error_kind_missing_a_variant_is_not_exhaustive() {
        let errs = errors_of(
            "module x\nimport std/fs\nfn reason(e: fs.FsError) -> string {\n  return match e.kind {\n             \x20   fs.ErrorKind.NotFound => \"missing\",\n             \x20   fs.ErrorKind.IsADirectory => \"dir\",\n             \x20   fs.ErrorKind.NotADirectory => \"not dir\",\n             \x20   fs.ErrorKind.AlreadyExists => \"exists\",\n             \x20   fs.ErrorKind.Other({ code }) => code,\n  }\n}\n",
        );
        let missing = errs.iter().find_map(|e| match e {
            TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                Some((type_name.clone(), missing.clone()))
            }
            _ => None,
        });
        assert_eq!(
            missing,
            Some(("fs.ErrorKind".to_string(), "`PermissionDenied`".to_string())),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_typo_on_an_fs_error_field_is_reported() {
        // The field model makes member access on a stdlib type checked, instead
        // of leaving it to `tsc` on the emitted TypeScript.
        let errs = errors_of(
            "module x\nimport std/fs\nfn reason(e: fs.FsError) -> string {\n  return e.mesage\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::UnknownField { field, .. } if field == "mesage")),
            "errs: {errs:?}"
        );
    }
    // ---------------------------------------------------------------------
    // One catch-all predicate. A bare `Pattern::Ident` arm head is a variant
    // *reference* when it is PascalCase (D9): the resolver resolves it as a
    // name rather than binding it, and the emitter lowers it to a
    // `.tag === "Foo"` test. It therefore absorbs nothing, whatever the
    // scrutinee is. These pin that every exhaustiveness check reads it the
    // same way; four of them used to read it as a catch-all and report
    // nothing.
    // ---------------------------------------------------------------------

    #[test]
    fn a_variant_shaped_head_is_not_a_catch_all_over_a_bool() {
        let errs = errors_of(
            "module x\n\
             type Colour = Red | Green\n\
             fn f(b: bool) -> number {\n\
             \x20 return match b {\n\
             \x20\x20\x20 true => 1,\n\
             \x20\x20\x20 Red => 2,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveBoolMatch { missing, .. } if missing == "`false`"
            )),
            "`Red` tests a tag, so `false` is still uncovered: {errs:?}"
        );
    }

    #[test]
    fn a_variant_shaped_head_is_not_a_catch_all_over_a_number() {
        let errs = errors_of(
            "module x\n\
             type Colour = Red | Green\n\
             fn f(n: number) -> number {\n\
             \x20 return match n {\n\
             \x20\x20\x20 0 => 1,\n\
             \x20\x20\x20 Red => 2,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveValueMatch { type_name, .. } if type_name == "number"
            )),
            "the rest of `number` is still uncovered: {errs:?}"
        );
    }

    #[test]
    fn a_variant_shaped_head_is_not_a_catch_all_over_a_string_literal_union() {
        let errs = errors_of(
            "module x\n\
             type Colour = Red | Green\n\
             type Tier = \"free\" | \"pro\"\n\
             fn f(t: Tier) -> number {\n\
             \x20 return match t {\n\
             \x20\x20\x20 \"free\" => 1,\n\
             \x20\x20\x20 Red => 2,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveMatch { missing, .. } if missing == "\"pro\""
            )),
            "`\"pro\"` is still uncovered: {errs:?}"
        );
    }

    #[test]
    fn a_variant_shaped_head_is_not_a_catch_all_over_an_array() {
        let errs = errors_of(
            "module x\n\
             type Colour = Red | Green\n\
             fn f(xs: Array<number>) -> number {\n\
             \x20 return match xs {\n\
             \x20\x20\x20 [a, b] => a + b,\n\
             \x20\x20\x20 Red => 2,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveArrayMatch { .. })),
            "every length but 2 is still uncovered: {errs:?}"
        );
    }

    #[test]
    fn a_binding_head_is_still_a_catch_all_over_every_scrutinee_kind() {
        // The other direction, so the unified predicate cannot be tightened
        // into rejecting the ordinary spelling: a lowercase head binds and
        // absorbs everything, whatever is being matched.
        let errs = errors_of(
            "module x\n\
             type Tier = \"free\" | \"pro\"\n\
             fn b(v: bool) -> number {\n\
             \x20 return match v {\n\
             \x20\x20\x20 true => 1,\n\
             \x20\x20\x20 other => 2,\n\
             \x20 }\n\
             }\n\
             fn n(v: number) -> number {\n\
             \x20 return match v {\n\
             \x20\x20\x20 0 => 1,\n\
             \x20\x20\x20 other => 2,\n\
             \x20 }\n\
             }\n\
             fn t(v: Tier) -> number {\n\
             \x20 return match v {\n\
             \x20\x20\x20 \"free\" => 1,\n\
             \x20\x20\x20 other => 2,\n\
             \x20 }\n\
             }\n\
             fn a(v: Array<number>) -> number {\n\
             \x20 return match v {\n\
             \x20\x20\x20 [x, y] => x + y,\n\
             \x20\x20\x20 other => 2,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            !errs.iter().any(|e| matches!(
                e,
                TypeError::NonExhaustiveBoolMatch { .. }
                    | TypeError::NonExhaustiveValueMatch { .. }
                    | TypeError::NonExhaustiveArrayMatch { .. }
                    | TypeError::NonExhaustiveMatch { .. }
            )),
            "a binding absorbs every value: {errs:?}"
        );
    }

    #[test]
    fn an_all_binding_object_arm_is_still_a_catch_all_over_a_record() {
        // The context half of the predicate. `{ x, y }` tests nothing, so over
        // a record it absorbs every value and E0226 must stay silent. Over a
        // `bool` the same shape absorbs nothing, which the case below pins.
        let errs = errors_of(
            "module x\n\
             type Point = { x: number, y: number }\n\
             fn f(p: Point) -> string {\n\
             \x20 return match p {\n\
             \x20\x20\x20 { x: 0, y: 0 } => \"origin\",\n\
             \x20\x20\x20 { x, y } => \"other\",\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveFieldMatch { .. })),
            "the second arm cannot fail: {errs:?}"
        );
    }

    #[test]
    fn an_object_arm_is_not_a_catch_all_over_a_bool() {
        let errs = errors_of(
            "module x\n\
             fn f(b: bool) -> number {\n\
             \x20 return match b {\n\
             \x20\x20\x20 { } => 1,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveBoolMatch { .. })),
            "an object pattern destructures a record, it does not absorb a bool: {errs:?}"
        );
    }

    // ------------------------------------------------------------------
    // Match coverage: the side channel the exhaustiveness dispatch fills
    // ------------------------------------------------------------------

    /// The coverage sink and the diagnostics from one run of the same
    /// dispatch. Both come out of one call on purpose: the sink's claim about
    /// a site is only worth anything if it agrees with what the checker
    /// reported while filling it.
    fn coverage_and_errors(src: &str) -> (FileMatchCoverage, Vec<TypeError>) {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let lowerer = Lowerer::new(&resolved, &prelude);
        let resolver = LocalDeclTy::new(&m, &lowerer);
        let (_tm, errs, cov) = assign_types_with_coverage(&m, &resolved, &prelude, &resolver);
        (cov, errs)
    }

    /// A stand-in for the cross-module resolver: one two-variant union in a
    /// sibling module, which is all it takes to reach
    /// `check_imported_union_coverage` without a salsa database.
    struct OneImportedUnion;

    impl DeclTyResolver for OneImportedUnion {
        fn decl_ty(&self, _decl_idx: u32) -> Ty {
            Ty::Unknown
        }

        fn imported_union_of_variant(
            &self,
            module_path: &str,
            variant_name: &str,
        ) -> Option<(String, Vec<Ident>)> {
            if module_path != "model" || !matches!(variant_name, "Yes" | "No") {
                return None;
            }
            Some(("Answer".to_string(), vec!["Yes".into(), "No".into()]))
        }
    }

    fn imported_coverage_and_errors(src: &str) -> (FileMatchCoverage, Vec<TypeError>) {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs, cov) =
            assign_types_with_coverage(&m, &resolved, &prelude, &OneImportedUnion);
        (cov, errs)
    }

    /// `(arm ordinal, depth, variant)` for every mention edge, in the order
    /// the dispatch wrote them.
    fn mentions_of(site: &CoverageSite) -> Vec<(u16, u16, String)> {
        site.mentions()
            .iter()
            .map(|m| (m.arm, m.depth, m.variant.clone()))
            .collect()
    }

    fn declared(module: &str, name: &str) -> CoverageTypeName {
        CoverageTypeName::Declared {
            module: module.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn a_single_payload_arm_mentions_the_variant_it_nests_into() {
        // The trap this side channel is easiest to get wrong on: a
        // constructor arm with exactly one sub-pattern is recorded in
        // `nested` and never in `covered`, so a sink reading `covered` alone
        // loses the arm's mention of `Cd` while the checker itself counts
        // it as present.
        let src = r#"module app
type Command =
  | Up
  | Cd({ name: string })
fn run(c: Command) -> string {
  return match c {
    Up => "up",
    Cd(x) => x.name,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        assert_eq!(cov.sites().len(), 1, "sites: {:?}", cov.sites());
        let site = &cov.sites()[0];
        assert_eq!(
            mentions_of(site),
            vec![(0, 0, "Up".to_string()), (1, 0, "Cd".to_string())]
        );
        assert_eq!(site.scrutinee_type(), &declared("app", "Command"));
        assert_eq!(site.state(), CoverageState::Exhaustive);
    }

    #[test]
    fn a_value_testing_record_payload_declines_its_arm() {
        // The third bucket. `Node({ colour: Red })` can fail, so the checker
        // records it in neither map and concludes nothing from it; the sink
        // says so instead of quietly counting it as coverage.
        let src = r#"module app
type Colour =
  | Red
  | Black
type Tree =
  | Leaf
  | Node({ colour: Colour })
fn f(t: Tree) -> number {
  return match t {
    Leaf => 0,
    Node({ colour: Red }) => 1,
    Node(n) => 2,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let site = &cov.sites()[0];
        assert_eq!(
            mentions_of(site),
            vec![(0, 0, "Leaf".to_string()), (2, 0, "Node".to_string())]
        );
        assert_eq!(site.declines().len(), 1, "declines: {:?}", site.declines());
        assert_eq!(site.declines()[0].arm, 1);
        assert_eq!(site.declines()[0].variant.as_deref(), Some("Node"));
        assert_eq!(site.state(), CoverageState::Declined);
    }

    #[test]
    fn a_catch_all_arm_is_recorded_with_its_ordinal() {
        let src = r#"module app
type Feed =
  | Loading
  | Ready
  | Failed
fn f(x: Feed) -> number {
  return match x {
    Loading => 0,
    _ => 1,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let site = &cov.sites()[0];
        assert_eq!(mentions_of(site), vec![(0, 0, "Loading".to_string())]);
        assert_eq!(site.catch_alls().len(), 1);
        assert_eq!(site.catch_alls()[0].arm, 1);
        assert_eq!(site.catch_alls()[0].depth, 0);
        assert_eq!(site.state(), CoverageState::HasCatchAll);
    }

    #[test]
    fn a_variant_no_arm_mentions_is_a_gap_and_the_site_is_not_exhaustive() {
        let src = r#"module app
type Feed =
  | Loading
  | Ready
  | Failed
fn f(x: Feed) -> number {
  return match x {
    Loading => 0,
    Ready => 1,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })),
            "errs: {errs:?}"
        );
        let site = &cov.sites()[0];
        assert_eq!(site.gaps().len(), 1, "gaps: {:?}", site.gaps());
        assert_eq!(site.gaps()[0].depth, 0);
        assert_eq!(site.gaps()[0].missing, vec!["Failed".to_string()]);
        assert_eq!(site.state(), CoverageState::Declined);
    }

    #[test]
    fn a_payload_recursion_writes_deeper_edges_into_the_same_site() {
        // A recursion into a payload is not a new site: the edges land in the
        // same one, one level deeper, and the arm ordinals pass through
        // unchanged so `Ok(Some(x))` is still arm 0.
        let src = r#"module app
fn run(r: Result<Option<number>, string>) -> number {
  return match r {
    Ok(Some(x)) => x,
    Ok(None) => 0,
    Err(_e) => 1,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        assert_eq!(cov.sites().len(), 1);
        let site = &cov.sites()[0];
        assert_eq!(
            mentions_of(site),
            vec![
                (0, 0, "Ok".to_string()),
                (1, 0, "Ok".to_string()),
                (2, 0, "Err".to_string()),
                (0, 1, "Some".to_string()),
                (1, 1, "None".to_string()),
            ]
        );
        assert_eq!(
            site.scrutinee_type(),
            &CoverageTypeName::Builtin {
                name: "Result".to_string()
            }
        );
        let deep = site
            .mentions()
            .iter()
            .find(|m| m.depth == 1)
            .expect("a depth-1 edge");
        assert_eq!(
            deep.union,
            CoverageTypeName::Builtin {
                name: "Option".to_string()
            }
        );
        assert_eq!(site.state(), CoverageState::Exhaustive);
    }

    #[test]
    fn a_string_literal_union_site_records_the_values_its_arms_name() {
        let src = r#"module app
type Tier = "free" | "pro"
fn f(t: Tier) -> number {
  return match t {
    "free" => 0,
    "pro" => 1,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let site = &cov.sites()[0];
        assert_eq!(
            mentions_of(site),
            vec![(0, 0, "free".to_string()), (1, 0, "pro".to_string())]
        );
        assert_eq!(site.scrutinee_type(), &declared("app", "Tier"));
        assert_eq!(site.state(), CoverageState::Exhaustive);
    }

    #[test]
    fn an_inline_string_literal_union_has_no_declaration_to_key_a_site_to() {
        // Every edge's type end is a declaration or a builtin name. A literal
        // set written into a signature is neither, so it gets no site rather
        // than a site keyed to something invented.
        let src = r#"module app
fn f(t: "free" | "pro") -> number {
  return match t {
    "free" => 0,
    "pro" => 1,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        assert!(cov.sites().is_empty(), "sites: {:?}", cov.sites());
    }

    #[test]
    fn an_imported_union_site_carries_its_source_module_and_its_gap() {
        let src = r#"module app
import model { Answer, Yes, No }
fn f(a: Answer) -> number {
  return match a {
    Yes => 1,
  }
}
"#;
        let (cov, errs) = imported_coverage_and_errors(src);
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })),
            "errs: {errs:?}"
        );
        assert_eq!(cov.sites().len(), 1, "sites: {:?}", cov.sites());
        let site = &cov.sites()[0];
        assert_eq!(site.scrutinee_type(), &declared("model", "Answer"));
        assert_eq!(mentions_of(site), vec![(0, 0, "Yes".to_string())]);
        assert_eq!(site.gaps()[0].missing, vec!["No".to_string()]);
        assert_eq!(site.state(), CoverageState::Declined);
    }

    #[test]
    fn an_imported_union_covered_by_every_arm_is_exhaustive() {
        let src = r#"module app
import model { Answer, Yes, No }
fn f(a: Answer) -> number {
  return match a {
    Yes => 1,
    No => 0,
  }
}
"#;
        let (cov, errs) = imported_coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let site = &cov.sites()[0];
        assert_eq!(
            mentions_of(site),
            vec![(0, 0, "Yes".to_string()), (1, 0, "No".to_string())]
        );
        assert_eq!(site.state(), CoverageState::Exhaustive);
    }

    #[test]
    fn an_imported_arm_with_a_payload_mentions_its_variant_too() {
        // The imported checker is a near-clone of the module-local one, and
        // the two buckets have to be read on both sides or the pair gains a
        // second divergence. `Yes(_v)` lands in `nested` here exactly as it
        // would there, and the catch-all is recorded with its own ordinal.
        let src = r#"module app
import model { Answer, Yes, No }
fn f(a: Answer) -> number {
  return match a {
    Yes(_v) => 1,
    _ => 0,
  }
}
"#;
        let (cov, errs) = imported_coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let site = &cov.sites()[0];
        assert_eq!(site.scrutinee_type(), &declared("model", "Answer"));
        assert_eq!(mentions_of(site), vec![(0, 0, "Yes".to_string())]);
        assert_eq!(site.catch_alls().len(), 1);
        assert_eq!(site.catch_alls()[0].arm, 1);
        assert_eq!(site.state(), CoverageState::HasCatchAll);
    }

    #[test]
    fn an_unresolvable_imported_scrutinee_is_a_site_with_no_conclusion() {
        // The fourth state. The scrutinee is named (`model::Sheet`) but this
        // module cannot read it as a union, so the checker concludes nothing
        // and the sink says exactly that rather than reporting coverage.
        let src = r#"module app
import model { Sheet }
fn f(s: Sheet) -> number {
  return match s {
    { kind: "a" } => 1,
    else => 0,
  }
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
        assert_eq!(cov.sites().len(), 1, "sites: {:?}", cov.sites());
        let site = &cov.sites()[0];
        assert_eq!(site.scrutinee_type(), &declared("model", "Sheet"));
        assert!(site.mentions().is_empty());
        assert_eq!(site.state(), CoverageState::ScrutineeUnresolved);
    }

    #[test]
    fn a_jsx_match_directive_is_a_site_with_one_mention_per_case() {
        let src = r#"module x
type Status =
  | Idle
  | Loading
  | Done
component View(s: Status) -> Component {
  return <match value={s}>
    <case Idle><span>idle</span></case>
    <case Loading><span>loading</span></case>
  </match>
}
"#;
        let (cov, errs) = coverage_and_errors(src);
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })),
            "errs: {errs:?}"
        );
        assert_eq!(cov.sites().len(), 1, "sites: {:?}", cov.sites());
        let site = &cov.sites()[0];
        assert_eq!(
            mentions_of(site),
            vec![(0, 0, "Idle".to_string()), (1, 0, "Loading".to_string())]
        );
        assert_eq!(site.gaps()[0].missing, vec!["Done".to_string()]);
        assert_eq!(site.state(), CoverageState::Declined);
    }

    // ------------------------------------------------------------------
    // The type end of a resolved union: which declaration, not which name
    // ------------------------------------------------------------------

    /// A cross-module resolver that declares one union, `Answer`, in `model`.
    ///
    /// It answers `imported_type_decl`, which is the query `required_variants`
    /// reaches an imported union through. `OneImportedUnion` above answers a
    /// different one (`imported_union_of_variant`, resolved from an arm rather
    /// than from the scrutinee's type), so the two are not interchangeable.
    struct ImportedAnswerDecl;

    impl DeclTyResolver for ImportedAnswerDecl {
        fn decl_ty(&self, _decl_idx: u32) -> Ty {
            Ty::Unknown
        }

        fn imported_type_decl(
            &self,
            module_path: &str,
            type_name: &str,
        ) -> Option<ImportedTypeDecl> {
            if module_path != "model" || type_name != "Answer" {
                return None;
            }
            Some(ImportedTypeDecl {
                name: Ident::from("Answer"),
                generics: Vec::new(),
                body: Ty::Union {
                    variants: vec![
                        UnionVariant {
                            name: Ident::from("Yes"),
                            payload: None,
                        },
                        UnionVariant {
                            name: Ident::from("No"),
                            payload: None,
                        },
                    ],
                },
            })
        }
    }

    /// Run `f` against an `Assigner` wired the way `assign_types_with_coverage`
    /// wires one.
    ///
    /// `required_variants` is what all four exhaustiveness callers read, and
    /// the walk publishes only its display name, through a diagnostic. Asking
    /// it directly is the only way to see which of the three declaration cases
    /// it resolved, which is the whole point of the answer it returns.
    fn with_assigner<R>(
        src: &str,
        resolver: &dyn DeclTyResolver,
        f: impl FnOnce(&Assigner<'_>) -> R,
    ) -> R {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let mut tm = TypeMap::new();
        let mut errors = Vec::new();
        let mut coverage = FileMatchCoverage::default();
        let mut field_uses = FileFieldUses::default();
        let assigner = Assigner {
            module: &m,
            lowerer: Lowerer::with_imports(&resolved, &prelude, resolver),
            resolved: &resolved,
            tm: &mut tm,
            errors: &mut errors,
            coverage: &mut coverage,
            field_uses: &mut field_uses,
            assign_target: None,
            decl_ty_resolver: resolver,
            return_stack: Vec::new(),
            local_tys: HashMap::new(),
        };
        f(&assigner)
    }

    /// The lowered type of the first parameter of the first `fn` in the file.
    fn first_param_ty(a: &Assigner<'_>) -> Ty {
        for d in &a.module.items {
            if let Decl::Fn(f) = d {
                return a.lowerer.lower(&f.params[0].ty);
            }
        }
        panic!("no fn declaration in the source");
    }

    #[test]
    fn a_local_unions_type_end_carries_the_module_it_is_declared_in() {
        // A display name is not an address. The dogfood corpus holds eleven
        // unrelated declarations named `Command`, so an edge whose type end is
        // the string `"Command"` names all eleven. The module this file is
        // known by is the other half, and it comes from the file being checked
        // rather than from the type, which is why local is its own case.
        let src = r#"module app
type Command =
  | Up
  | Down
fn run(c: Command) -> string {
  return "x"
}
"#;
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            a.required_variants(&ty)
        });
        let (union, variants) = got.expect("a module-local union resolves");
        assert_eq!(
            union,
            UnionRef::Local {
                module: "app".to_string(),
                name: "Command".to_string(),
            }
        );
        assert_eq!(variants, vec![Ident::from("Up"), Ident::from("Down")]);
        // The diagnostics render this and nothing else, unchanged.
        assert_eq!(union.display(), "Command");
    }

    #[test]
    fn a_local_unions_type_end_has_an_empty_module_without_a_module_line() {
        // A file that declares no `module` line has no key anything resolves.
        // Empty is the honest answer for it: the declaration has a name and no
        // address. Inventing one would file the site under a module that does
        // not exist.
        let src = r#"type Command =
  | Up
  | Down
fn run(c: Command) -> string {
  return "x"
}
"#;
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            a.required_variants(&ty)
        });
        let (union, _variants) = got.expect("a module-local union resolves");
        assert_eq!(
            union,
            UnionRef::Local {
                module: String::new(),
                name: "Command".to_string(),
            }
        );
    }

    #[test]
    fn an_imported_unions_type_end_carries_the_module_it_came_from() {
        // The module comes off the type here, not off the file: this is the
        // half a single `(module, name)` pair would have collapsed into the
        // consumer's own module and answered wrongly for.
        let src = "module app\nfn f(n: number) -> number {\n  return n\n}\n";
        let ty = Ty::Imported {
            module: ModuleKey::from("model"),
            name: Ident::from("Answer"),
        };
        let got = with_assigner(src, &ImportedAnswerDecl, |a| a.required_variants(&ty));
        let (union, variants) = got.expect("an imported union resolves");
        assert_eq!(
            union,
            UnionRef::Imported {
                module: "model".to_string(),
                name: "Answer".to_string(),
            }
        );
        assert_eq!(variants, vec![Ident::from("Yes"), Ident::from("No")]);
        assert_eq!(union.display(), "Answer");
    }

    #[test]
    fn a_prelude_unions_type_end_is_a_builtin_with_no_declaration() {
        // `Result` has a fixed variant table and no declaration in any project
        // module. There is nothing to mint a key for, so it is not a `Declared`
        // case with an invented module: it is its own.
        let src = "module app\nfn f(r: Result<number, string>) -> number {\n  return 0\n}\n";
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            a.required_variants(&ty)
        });
        let (union, variants) = got.expect("a prelude Result resolves");
        assert_eq!(
            union,
            UnionRef::Builtin {
                name: "Result".to_string(),
            }
        );
        assert_eq!(variants, vec![Ident::from("Ok"), Ident::from("Err")]);
        assert_eq!(union.display(), "Result");
    }

    #[test]
    fn a_stdlib_unions_type_end_is_a_builtin_under_its_display_name() {
        // `fs.ErrorKind` is published by the stdlib stubs and declared nowhere
        // in the project. Its display name is the only name it has in Glyph
        // source, and E0200 has always printed it with the dot.
        let src = "module app\nfn f(n: number) -> number {\n  return n\n}\n";
        let ty = stdlib_named("fs", "ErrorKind");
        let got = with_assigner(src, &ImportedAnswerDecl, |a| a.required_variants(&ty));
        let (union, variants) = got.expect("a stdlib union resolves");
        assert_eq!(
            union,
            UnionRef::Builtin {
                name: "fs.ErrorKind".to_string(),
            }
        );
        assert_eq!(variants.first(), Some(&Ident::from("NotFound")));
        assert_eq!(union.display(), "fs.ErrorKind");
    }

    #[test]
    fn the_coverage_type_end_comes_from_the_one_union_resolution() {
        // The relation's type end and the variant set a site was counted
        // against must describe the same declaration. They do because there is
        // one resolution: `required_variants` answers with the declaration and
        // the coverage name is derived from that answer, rather than a second
        // function re-walking the same four paths in an order kept in step by
        // hand.
        let local = UnionRef::Local {
            module: "app".to_string(),
            name: "Command".to_string(),
        };
        assert_eq!(
            CoverageTypeName::from(&local),
            CoverageTypeName::Declared {
                module: "app".to_string(),
                name: "Command".to_string(),
            }
        );
        let imported = UnionRef::Imported {
            module: "model".to_string(),
            name: "Answer".to_string(),
        };
        assert_eq!(
            CoverageTypeName::from(&imported),
            CoverageTypeName::Declared {
                module: "model".to_string(),
                name: "Answer".to_string(),
            }
        );
        let builtin = UnionRef::Builtin {
            name: "Result".to_string(),
        };
        assert_eq!(
            CoverageTypeName::from(&builtin),
            CoverageTypeName::Builtin {
                name: "Result".to_string(),
            }
        );
    }

    // ---- G195: the union and the missing variants come off the error ----

    /// The repair loop is grep-free at every hop except the one that starts
    /// it. An agent that gets E0200 needs the union's name to make the next
    /// query and the variant names to write the arms, and until now both
    /// lived only in the message, in backticks. A message is meant to be
    /// rewritten; pinning a machine contract to its wording makes every
    /// improvement to a sentence a breaking change nobody notices.
    #[test]
    fn a_non_exhaustive_match_carries_its_union_and_its_missing_variants() {
        let errs = errors_of(
            "module billing\n\
             type PaymentResult =\n  | Settled\n  | Failed\n  | Pending\n\
             fn settle(r: PaymentResult) -> string {\n\
             \x20 return match r {\n    Settled => \"s\",\n    Failed => \"f\",\n  }\n\
             }\n",
        );
        let e = errs
            .iter()
            .find(|e| e.code() == "E0200")
            .unwrap_or_else(|| panic!("expected E0200: {errs:?}"));
        assert_eq!(
            e.union(),
            Some(&DiagnosticUnion::Local {
                name: "PaymentResult".to_string()
            }),
            "errs: {errs:?}"
        );
        assert_eq!(
            e.missing_variants(),
            Some(["Pending".to_string()].as_slice()),
            "errs: {errs:?}"
        );
        // The prose is unchanged: this adds fields beside the message, it does
        // not move anything out of it.
        assert_eq!(
            format!("{e}"),
            "non-exhaustive match on `PaymentResult`: missing variants `Pending`"
        );
    }

    /// A union declared in another module keeps that module on the error, so
    /// the identity addresses the declaration rather than this file.
    #[test]
    fn an_imported_unions_gap_names_the_module_it_is_declared_in() {
        let src = r#"module app
import model { Answer, Yes, No }
fn f(a: Answer) -> number {
  return match a {
    Yes => 1,
  }
}
"#;
        let (_cov, errs) = imported_coverage_and_errors(src);
        let e = errs
            .iter()
            .find(|e| e.code() == "E0200")
            .unwrap_or_else(|| panic!("expected E0200: {errs:?}"));
        assert_eq!(
            e.union(),
            Some(&DiagnosticUnion::Imported {
                module: "model".to_string(),
                name: "Answer".to_string(),
            }),
            "errs: {errs:?}"
        );
        assert_eq!(e.missing_variants(), Some(["No".to_string()].as_slice()));
    }

    /// A prelude union has a fixed variant table and no declaration anywhere,
    /// so it is a builtin rather than a declaration under an invented module.
    #[test]
    fn a_prelude_unions_gap_is_a_builtin_with_no_declaration() {
        let errs = errors_of(
            "module app\n\
             import std/result { Ok, Err }\n\
             fn f(r: Result<number, string>) -> number {\n\
             \x20 return match r {\n    Ok(n) => n,\n  }\n\
             }\n",
        );
        let e = errs
            .iter()
            .find(|e| e.code() == "E0200")
            .unwrap_or_else(|| panic!("expected E0200: {errs:?}"));
        assert_eq!(
            e.union(),
            Some(&DiagnosticUnion::Builtin {
                name: "Result".to_string()
            }),
            "errs: {errs:?}"
        );
        assert_eq!(e.missing_variants(), Some(["Err".to_string()].as_slice()));
    }

    /// A string-literal union's members are values rather than tags, and the
    /// list carries them unquoted, the same way the coverage relation records
    /// them. The alias it was reached through is the declaration.
    #[test]
    fn a_string_literal_unions_gap_names_the_alias_and_the_missing_values() {
        let errs = errors_of(
            "module app\n\
             type Tier = \"free\" | \"pro\" | \"team\"\n\
             fn label(t: Tier) -> string {\n\
             \x20 return match t {\n    \"free\" => \"F\",\n    \"pro\" => \"P\",\n  }\n\
             }\n",
        );
        let e = errs
            .iter()
            .find(|e| e.code() == "E0200")
            .unwrap_or_else(|| panic!("expected E0200: {errs:?}"));
        assert_eq!(
            e.union(),
            Some(&DiagnosticUnion::Local {
                name: "Tier".to_string()
            }),
            "errs: {errs:?}"
        );
        assert_eq!(e.missing_variants(), Some(["team".to_string()].as_slice()));
    }

    /// A literal set written inline into a signature is declared nowhere, so
    /// there is no union to name. Absent, not guessed: the missing values are
    /// still known and still reported.
    #[test]
    fn an_inline_literal_sets_gap_has_no_union_and_still_lists_its_values() {
        let errs = errors_of(
            "module app\n\
             fn label(t: \"free\" | \"pro\" | \"team\") -> string {\n\
             \x20 return match t {\n    \"free\" => \"F\",\n    \"pro\" => \"P\",\n  }\n\
             }\n",
        );
        let e = errs
            .iter()
            .find(|e| e.code() == "E0200")
            .unwrap_or_else(|| panic!("expected E0200: {errs:?}"));
        assert_eq!(e.union(), None, "errs: {errs:?}");
        assert_eq!(e.missing_variants(), Some(["team".to_string()].as_slice()));
    }

    /// Absence means absence of a relation. An error that is not about a
    /// union's variants answers nothing for either field rather than an empty
    /// list, which would read as "no variants are missing".
    #[test]
    fn an_error_that_is_not_about_a_union_carries_neither_field() {
        let errs = errors_of(
            "module app\n\
             type Account = { email: string }\n\
             fn f(a: Account) -> string {\n  return a.emial\n}\n",
        );
        let e = errs
            .iter()
            .find(|e| e.code() == "E0210")
            .unwrap_or_else(|| panic!("expected E0210: {errs:?}"));
        assert_eq!(e.union(), None);
        assert_eq!(e.missing_variants(), None);
    }
}
