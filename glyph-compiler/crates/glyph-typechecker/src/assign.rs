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
    jsx_member_name, ArrayElem, Block, Decl, Expr, Ident, InterfaceMember, JsxAttr, JsxChild,
    JsxElement, JsxKind, LetBinding, LiteralPattern, MatchArm, MatchArmBody, Module, ObjectField,
    ObjectPatternField, Param, Pattern, PostfixOp, Span, Stmt, TemplatePart, TypeExpr,
};
use glyph_resolver::{
    Prelude, PreludeKind, ResolvedModule, ResolvedRef, SymbolId, SymbolKind,
};

use crate::lower::Lowerer;
use crate::ty::{
    builtin_string_literal_union_decl, builtin_union, field_owner_display, ty_display, FnParam, ImportedTypeDecl, ModuleKey, Primitive, RecordField,
    SymbolRef, Ty, UnionRef, UnionVariant,
};
use crate::builtin_members::{MethodSig, MethodTy};
use crate::type_map::{IdentPattern, TypeMap};
use crate::{
    Accepted, AlternativesKind, BuiltinReceiver, CaseBindShape, DestructureRefusal, DiagnosticDecl, PropsRefusal,
    TypeError,
};

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

/// What a component's one parameter is as a props value (G243). Three answers
/// rather than a bool, because "not proved to be a record" and "proved not to
/// be one" are different claims and only the second is E0229.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropsParam {
    /// A record the element's attributes are checked against.
    Record,
    /// A type that cannot be a props object the component reads, and why.
    NotRecord(PropsRefusal),
    /// A type this checker cannot see into, or one that describes any object.
    Undecided,
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

    /// The general cross-module type query: for the project module at
    /// `module_path`, return the `type <type_name> = ...` declaration lowered on
    /// the *source* side, so its body names types that resolve against the
    /// declaring module rather than the consumer. `None` with no cross-module
    /// context (db-less callers), for a module that is not a project sibling
    /// (`std/fs`), or for a name that module does not declare as a `type`.
    ///
    /// This is what gives an imported record a field set, so `s.rowz` is an
    /// `UnknownField` and `for i, r in s.rows` lowers as an array loop. It is
    /// also where an imported string-literal union's literal set comes from
    /// since G233, through `imported_string_literal_union_values`; the
    /// per-shape query that used to answer that separately is gone.
    /// `imported_union_of_variant` is the one still beside it.
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

    /// Cross-module const resolution: for the project module at `module_path`,
    /// return the lowered annotation of the `pub const` named `const_name`, on
    /// the export view (so an annotation naming a sibling `type` comes back
    /// as `Ty::Imported`). `None` with no cross-module context, for a module
    /// that is not a project sibling, for a name the module does not declare
    /// as a `const`, and for a const with no annotation, which stays `Unknown`
    /// exactly as it does in its own module.
    ///
    /// Without this an imported const had no type at all (G205): `ORIGIN.rowz`
    /// was silent through `import lib { ORIGIN }` and through `lib.ORIGIN`
    /// alike, where the same read in the declaring module has been E0210
    /// since the annotated-const rule landed.
    ///
    /// Answered by `glyph_db::exported_const`, the sibling of `exported_fn`.
    fn imported_const_decl(&self, _module_path: &str, _const_name: &str) -> Option<Ty> {
        None
    }

    /// The type parameter names of the `fn` or `component` the project
    /// module at `module_path` declares under `fn_name`, in declaration
    /// order. `None` wherever `imported_fn_decl` would answer `None`.
    ///
    /// The signature `imported_fn_decl` returns carries each parameter by
    /// name only, so a written type argument (`lib.id<number>(x)`) has no
    /// position to bind to without the declaration's list. Without this a
    /// call through an import read its type arguments by nothing, while the
    /// same call to a local function read them (G292).
    fn imported_fn_generics(&self, _module_path: &str, _fn_name: &str) -> Option<Vec<Ident>> {
        None
    }

    /// Whether the project module at `module_path` exports `name`. `None` when
    /// no module at that path is part of this compile: an npm package, a
    /// sibling outside the project being built, or any import at all for a
    /// caller with no cross-module context (the one-file playground).
    ///
    /// It separates the two reasons `imported_type_decl` can answer `None`. A
    /// module the compile cannot see leaves a `match` over its type undecided
    /// (E0232). A module it can see that has no such export has already been
    /// reported at the import (E0105), and a second error on the `match` about
    /// a declaration being out of sight would send the reader the wrong way.
    ///
    /// Answered by `glyph_db::module_exports`.
    fn module_exports_name(&self, _module_path: &str, _name: &str) -> Option<bool> {
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
            lowerer: Lowerer::new(resolved, prelude),
            resolved,
            tm: &mut tm,
            errors: &mut errors,
            coverage: &mut coverage,
            field_uses: &mut field_uses,
            assign_target: None,
            call_callee: None,
            member_object: None,
            relating: RefCell::new(Vec::new()),
            decl_ty_resolver,
            return_stack: Vec::new(),
            local_tys: HashMap::new(),
            stdlib_values: HashMap::new(),
            local_stdlib_values: HashMap::new(),
            null_chains: NullChains::default(),
        };
        for decl in &module.items {
            assigner.check_annotations(decl);
            assigner.declare_record_fields(decl);
            assigner.walk_decl(decl);
            assigner.report_unhandled_null_chains();
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

/// What the checker's assignability relation says about one pairing, three
/// ways where `assign_incompatible` says two.
///
/// The relation refuses only what it can prove wrong, so its `false` means
/// "not refused" and not "accepted". A surface that reported the two as one
/// word would be answering a question the compiler never asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assignability {
    /// A rule read this pairing and refused it. This is exactly what the
    /// checker reports at the site: E0204 at a `let`, a `const` or a `return`,
    /// E0211 at a call argument.
    Incompatible,
    /// A rule read this pairing and accepted it. `rule` says which rule, in
    /// the words the relation's own documentation uses.
    Compatible { rule: &'static str },
    /// No rule covers the pairing. Nothing in Glyph reports a mismatch here;
    /// only `tsc` reading the emitted TypeScript on a full `glyph build`
    /// would.
    NoRule,
}

/// Ask the checker's own assignability relation whether a value of type
/// `found` can go where `expected` is declared, in the scope of `module`.
///
/// **This is `assign_incompatible`, not `definitely_incompatible`.** The free
/// function underneath is only the part of the relation decidable from two
/// `Ty` values with no declaration in hand, and three of the rules that matter
/// most are decided above it, by reading a declaration it cannot reach: a
/// declared union or record against a primitive (G201), a prelude container
/// against a primitive (G216), and an imported declaration against a
/// primitive, a local declaration or another imported one (G215, G226). A
/// surface built on `definitely_incompatible` would report "no rule" for every
/// one of those and be wrong in the direction that matters.
///
/// **What the `Compatible` half is, and is not.** `Incompatible` is the
/// relation's own answer, unchanged. `Compatible` is not its complement: it is
/// claimed only where a *total* rule covers the pairing, one that refuses
/// whenever the two sides differ in the way it reads, so a refusal that did
/// not happen is a positive acceptance rather than a rule that never looked.
/// Those rules are enumerated in `Assigner::accepting_rule` and nowhere else,
/// and every pairing outside them is `NoRule` however permissive the relation
/// was. A function type against a function type is the shape of the
/// distinction: the relation compares the returns and skips the parameters, so
/// its silence there is not an acceptance and this answers `NoRule`.
///
/// It is not a full assignability decision procedure, and it is not `tsc`. A
/// `NoRule` answer says Glyph has nothing to say, not that the program is
/// correct.
pub fn assignability(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
    decl_ty_resolver: &dyn DeclTyResolver,
    found: &Ty,
    expected: &Ty,
) -> Assignability {
    // The relation reads the module, the resolver and the lowerer; the four
    // side channels below are written by the walk, and no walk runs here.
    let mut tm = TypeMap::new();
    let mut errors: Vec<TypeError> = Vec::new();
    let mut coverage = FileMatchCoverage::default();
    let mut field_uses = FileFieldUses::default();
    let assigner = Assigner {
        module,
        lowerer: Lowerer::new(resolved, prelude),
        resolved,
        tm: &mut tm,
        errors: &mut errors,
        coverage: &mut coverage,
        field_uses: &mut field_uses,
        assign_target: None,
        call_callee: None,
        member_object: None,
        relating: RefCell::new(Vec::new()),
        decl_ty_resolver,
        return_stack: Vec::new(),
        local_tys: HashMap::new(),
        stdlib_values: HashMap::new(),
        local_stdlib_values: HashMap::new(),
        null_chains: NullChains::default(),
    };
    if assigner.assign_incompatible(found, expected) {
        return Assignability::Incompatible;
    }
    match assigner.accepting_rule(found, expected) {
        Some(rule) => Assignability::Compatible { rule },
        None => Assignability::NoRule,
    }
}

/// The fields the checker compares a structural record against where
/// `declared` is the declared type, or `None` when it reads no field set
/// there. Two readers rest on it being the checker's own read, through one
/// method (`Assigner::props_record_of`): the props record a component element
/// is compared against (G240), and the record `glyph_assignable` names when it
/// says which of a value's fields the declaration does not have. It answers
/// for a structural record, a local or imported record and a second name for
/// one, a generic application of one, an interface and a stdlib record; a
/// generic parameter, `unknown`, a `Record<K, V>` map and every type that is
/// not a record answer `None`.
pub fn declared_record_fields(
    module: &Module,
    resolved: &ResolvedModule,
    prelude: &Prelude,
    decl_ty_resolver: &dyn DeclTyResolver,
    declared: &Ty,
) -> Option<Vec<RecordField>> {
    let mut tm = TypeMap::new();
    let mut errors: Vec<TypeError> = Vec::new();
    let mut coverage = FileMatchCoverage::default();
    let mut field_uses = FileFieldUses::default();
    let assigner = Assigner {
        module,
        lowerer: Lowerer::new(resolved, prelude),
        resolved,
        tm: &mut tm,
        errors: &mut errors,
        coverage: &mut coverage,
        field_uses: &mut field_uses,
        assign_target: None,
        call_callee: None,
        member_object: None,
        relating: RefCell::new(Vec::new()),
        decl_ty_resolver,
        return_stack: Vec::new(),
        local_tys: HashMap::new(),
        stdlib_values: HashMap::new(),
        local_stdlib_values: HashMap::new(),
        null_chains: NullChains::default(),
    };
    assigner.props_record_of(declared).map(|shape| shape.fields)
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
    /// A prelude or stdlib union (`Result`, `Option`, `fs.ErrorKind`), keyed by
    /// the stdlib module that declares it (G231). `declared` is the name inside
    /// that module; `name` is the spelling a program writes.
    Builtin {
        module: String,
        declared: String,
        name: String,
    },
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
            UnionRef::Builtin {
                module,
                declared,
                name,
            } => CoverageTypeName::Builtin {
                module: module.clone(),
                declared: declared.clone(),
                name: name.clone(),
            },
        }
    }
}

impl From<&UnionRef> for DiagnosticDecl {
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
            UnionRef::Local { name, .. } => DiagnosticDecl::Local {
                name: name.clone(),
            },
            UnionRef::Imported { module, name } => DiagnosticDecl::Imported {
                module: module.clone(),
                name: name.clone(),
            },
            UnionRef::Builtin {
                module,
                declared,
                name,
            } => DiagnosticDecl::Builtin {
                module: module.clone(),
                declared: declared.clone(),
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
fn coverage_name(union: &DiagnosticDecl, own_module: &str) -> CoverageTypeName {
    match union {
        DiagnosticDecl::Local { name } => CoverageTypeName::Declared {
            module: own_module.to_string(),
            name: name.clone(),
        },
        DiagnosticDecl::Imported { module, name } => CoverageTypeName::Declared {
            module: module.clone(),
            name: name.clone(),
        },
        DiagnosticDecl::Builtin {
            module,
            declared,
            name,
        } => CoverageTypeName::Builtin {
            module: module.clone(),
            declared: declared.clone(),
            name: name.clone(),
        },
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

/// The declaration `ty` names as written, or `None`: a module-local `Ty::Named`
/// whose symbol is a type declaration of this module. The one place the symbol
/// table is read for a module-local type, so the collision guard (a prelude
/// `SymbolId` numbers from 0 independently of the module table, and must not
/// index an unrelated declaration) lives here once. Shared by the assigner and
/// the owned checker so both read the same declaration for the same name.
pub fn direct_type_decl<'m>(
    module: &'m Module,
    resolved: &ResolvedModule,
    ty: &Ty,
) -> Option<&'m glyph_ast::TypeDecl> {
    let Ty::Named { symbol, path } = ty else { return None };
    let sym = resolved.symbols.table.get(SymbolId(symbol.0))?;
    if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
        return None;
    }
    let SymbolKind::Type { decl_idx } = sym.kind else { return None };
    let Decl::Type(td) = module.items.get(decl_idx as usize)? else {
        return None;
    };
    Some(td)
}

/// The type `td` is a second name for, when it is one (D46): its body is a
/// name, it declares no generic parameters, and it carries no `where`.
/// Lowering that body gives the next hop: the `Ty::Named` of a declaration in
/// this module, or the `Ty::Imported` of one in a sibling. The name is
/// followed however it is spelled: bare (`Doc`, under `import a { Doc }`) or
/// qualified by a namespace import (`a.Doc` under `import a`, `aa.Doc` under
/// `import a as aa`, `http.RedirectPolicy` under `import std/http`). The
/// qualified spelling lowers to the type the named one does, and following
/// only the bare one made the guarantees on `Page` depend on how its import
/// was written (G302).
/// The rule does not stop at a file boundary, because D46 says the
/// guarantees on a type do not depend on which file its name was declared
/// in: `type Local = Shape` with `Shape` imported is a second name for the
/// sibling's declaration, and answering `Local`'s own `Ty::Named` here left
/// a match over it unchecked (G224) and made `Local` and a second alias of
/// the same import incompatible with each other by their spelling.
///
/// `None` for every other body, which is the declaration's own type rather
/// than another's: a record, a union, a string-literal union, an
/// `extern_ts` or `typeof` body, a function type, a generic application
/// (`Record<string, T>`, `Wrapper<T>`), a refinement, and a bare name that
/// lowers to a primitive (`type Id = string`). No incompatibility is decided
/// from a body not being followed; the corpus holds six `Record<string, T>`
/// and `Array<..>` aliases, and reading those as nominal would reject
/// programs `tsc` accepts.
pub fn alias_target(lowerer: &Lowerer<'_>, td: &glyph_ast::TypeDecl) -> Option<Ty> {
    if !td.generics.is_empty() || td.refinement.is_some() {
        return None;
    }
    if !matches!(&td.body, TypeExpr::Path { .. }) {
        return None;
    }
    match lowerer.lower(&td.body) {
        next @ (Ty::Named { .. } | Ty::Imported { .. }) => Some(next),
        _ => None,
    }
}

/// Follow `ty` through second names to the declaration the chain ends at
/// (D46). This is the one chain walker: every judgement that depends on which
/// declaration a name reaches, field lookup, descriptor lookup, and whether the
/// declaration is `resource`-marked (G212), and which variants a match must
/// cover (G224), goes through it, so no judgement can answer by which of two
/// names a site reached the type through. A chain that leaves the module ends
/// at the `Ty::Imported` it reaches, which every consumer reads exactly as it
/// reads a direct import. A cycle answers the type as written; anything that
/// is not a module-local alias answers unchanged.
pub fn resolve_alias_chain(
    module: &Module,
    resolved: &ResolvedModule,
    lowerer: &Lowerer<'_>,
    ty: &Ty,
) -> Ty {
    let mut current = ty.clone();
    let mut seen: HashSet<u32> = HashSet::new();
    loop {
        let Ty::Named { symbol, .. } = &current else {
            return current;
        };
        if !seen.insert(symbol.0) {
            return ty.clone();
        }
        let Some(td) = direct_type_decl(module, resolved, &current) else {
            return current;
        };
        match alias_target(lowerer, td) {
            Some(next) => current = next,
            None => return current,
        }
    }
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
    /// The span of the identifier currently standing as a member access's
    /// object, so the one `Expr::Ident` arm can tell a type name used as a
    /// value from a type name used as its own descriptor's receiver (G217).
    ///
    /// `Order.parse(json)` and `Order.is(v)` are the two forms where naming a
    /// type in an expression is the point, and both reach the identifier arm
    /// through the member arm's walk of its object. A span rather than a flag,
    /// for the reason `assign_target` is one: the object's subexpressions are
    /// walked with it still set, and a flag would exempt every identifier
    /// underneath.
    member_object: Option<Span>,
    /// The span of the expression currently standing as a call's callee, so
    /// the one `Expr::Member` arm can tell `s.toUpperCase()` from
    /// `s.toUpperCase` read alone (E0234, G312). A span rather than a flag
    /// for the reason `assign_target` is one: the callee's object is walked
    /// with it still set, and a flag would take the `s.trim` in
    /// `s.trim.bind(s)()` for a call.
    call_callee: Option<Span>,
    /// The interface pairings the relation is in the middle of deciding, so a
    /// type that names itself through a member does not recurse without end.
    /// See the interface arm of `assign_incompatible_at`.
    relating: RefCell<Vec<(Ty, Ty)>>,
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
    /// Record 88, 4a. Every expression, and every `let` binding (by its
    /// definition start, as `local_tys` keys it), whose value is a stdlib
    /// function read without being called whose type parameter only a type
    /// argument on a direct call fills: `json.parse` in `let p = json.parse`.
    /// The value is the function as the program spelled it, so an E0233 on
    /// `p<User>(s)` can say which direct call to write instead.
    stdlib_values: HashMap<Span, String>,
    local_stdlib_values: HashMap<u32, String>,
    /// The optional chains over a `Nullable` whose value has not yet been
    /// handled (G304, D45). See `NullChains`.
    null_chains: NullChains,
}

/// The optional chains over a `Nullable` (G304, D45), tracked by span.
///
/// `n?.f` on `n: Nullable<R>` is an `F` or JavaScript's `undefined`, and Glyph
/// has no type for `undefined`, so the chain's value may stand only where the
/// absence is dealt with on the spot: the left operand of `??`, or an operand
/// of `==`/`!=`. Rather than a `Ty` for "an `F` or `undefined`", which every
/// match on `Ty` would then have to answer for, the walk keeps the obligation
/// beside the type map. The member arm opens one at each `?.` over a
/// `Nullable`, a member, index or call on top of the chain moves it outward
/// (JavaScript short-circuits the whole chain, so the outermost link is the
/// value), the binary arm discharges it, and whatever is still open when a
/// declaration's walk ends is E0235.
#[derive(Default)]
struct NullChains {
    /// Open chains, in the order they were met: the span of the chain's value,
    /// the field read through the `Nullable`, and the `Nullable`'s display.
    open: Vec<(Span, String, String)>,
    /// Chains already discharged, so a subexpression the walk reaches a second
    /// time is not opened again after its parent handled it.
    handled: HashSet<Span>,
}

impl NullChains {
    /// Open an obligation at `span`, unless that chain was already handled.
    fn open(&mut self, span: Span, root: (String, String)) {
        if !self.handled.contains(&span) && !self.open.iter().any(|(s, ..)| *s == span) {
            self.open.push((span, root.0, root.1));
        }
    }

    /// Take the open chain at `span`, to move it to an enclosing link.
    fn take(&mut self, span: Span) -> Option<(String, String)> {
        let i = self.open.iter().position(|(s, ..)| *s == span)?;
        let (_, field, ty) = self.open.remove(i);
        Some((field, ty))
    }

    /// Discharge the chain at `span`. True when `span` is a chain over a
    /// `Nullable`, open or already handled.
    fn handle(&mut self, span: Span) -> bool {
        if self.take(span).is_some() {
            self.handled.insert(span);
            return true;
        }
        self.handled.contains(&span)
    }
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

    /// D45: every type written in a declaration's signature or body is checked
    /// for `Nullable<T>` with a `T` that is itself `Nullable` or `Option`
    /// (E0227). Runs for every top-level declaration; the annotations inside a
    /// body (`let`, lambdas, call type arguments, `is` arms) are reached from
    /// the walk that visits them.
    fn check_decl_nullable_args(&mut self, decl: &Decl) {
        match decl {
            Decl::Import(_) => {}
            Decl::Type(t) => self.check_nullable_args(&t.body),
            Decl::Fn(f) => {
                for p in &f.params {
                    self.check_nullable_args(&p.ty);
                }
                if let Some(rt) = &f.return_ty {
                    self.check_nullable_args(rt);
                }
            }
            Decl::Component(c) => {
                for p in &c.params {
                    self.check_nullable_args(&p.ty);
                }
                if let Some(rt) = &c.return_ty {
                    self.check_nullable_args(rt);
                }
            }
            Decl::Const(c) => {
                if let Some(te) = &c.ty {
                    self.check_nullable_args(te);
                }
            }
            Decl::Interface(i) => {
                for m in &i.members {
                    match m {
                        glyph_ast::InterfaceMember::Method { params, return_ty, .. } => {
                            for p in params {
                                self.check_nullable_args(&p.ty);
                            }
                            if let Some(rt) = return_ty {
                                self.check_nullable_args(rt);
                            }
                        }
                        glyph_ast::InterfaceMember::Field(f) => self.check_nullable_args(&f.ty),
                    }
                }
            }
        }
    }

    /// The recursive half of the D45 rule: find every `Nullable<..>` inside
    /// `te`, however deeply it is nested, and reject one whose argument
    /// resolves to `Nullable` or `Option`. The argument is judged by what it
    /// resolves to rather than how it is spelled, so an alias for `Option<int>`
    /// is refused like the direct spelling and `import std/option { Option }`
    /// changes nothing. The message carries the argument as written.
    fn check_nullable_args(&mut self, te: &TypeExpr) {
        match te {
            TypeExpr::Generic { base, args, span } => {
                if let Some(expected) = self.type_param_count(base) {
                    if expected != args.len() {
                        self.errors.push(TypeError::TypeArgumentCountMismatch {
                            name: type_expr_source(base),
                            expected,
                            found: args.len(),
                            value_of: None,
                            span: *span,
                        });
                    }
                }
                if self.is_prelude_named(base, "Nullable") {
                    if let [arg] = args.as_slice() {
                        let lowered = self.lowerer.lower(arg);
                        if self.resolves_to_nullable_or_option(&lowered, 0) {
                            self.errors.push(TypeError::NullableNested {
                                inner: type_expr_source(arg),
                                span: *span,
                            });
                        }
                    }
                }
                // The base is not walked: it is the name the arguments above
                // were counted against, not a bare use of it.
                for a in args {
                    self.check_nullable_args(a);
                }
            }
            TypeExpr::Fn { params, return_ty, .. } => {
                for p in params {
                    self.check_nullable_args(&p.ty);
                }
                if let Some(rt) = return_ty {
                    self.check_nullable_args(rt);
                }
            }
            TypeExpr::Record { fields, .. } => {
                for f in fields {
                    self.check_nullable_args(&f.ty);
                }
            }
            TypeExpr::Union { variants, .. } => {
                for v in variants {
                    if let Some(p) = &v.payload {
                        self.check_nullable_args(p);
                    }
                }
            }
            // G292. A generic declaration written with no type arguments takes
            // fewer than it has parameters, which is E0233 like any other
            // wrong count (`tsc` says TS2314). The count follows resolution,
            // so a name that resolves to a non-generic declaration (a local
            // `type Array`, an imported `Result`) takes 0 and passes.
            TypeExpr::Path { span, .. } => {
                if let Some(expected) = self.type_param_count(te).filter(|n| *n > 0) {
                    self.errors.push(TypeError::TypeArgumentCountMismatch {
                        name: type_expr_source(te),
                        expected,
                        found: 0,
                        value_of: None,
                        span: *span,
                    });
                }
            }
            TypeExpr::Extern { .. }
            | TypeExpr::StringLiteralUnion { .. }
            | TypeExpr::TypeOf { .. } => {}
        }
    }

    /// How many type parameters the declaration a written type's base names
    /// has, for the E0233 count of a type application (`Box<number, string>`
    /// on `type Box<T>`). Read from a `type` or `interface` of this module, a
    /// project sibling's `type` under either import spelling, and the prelude,
    /// whose containers take the counts TypeScript gives them and whose other
    /// types take none. `None` for anything whose list the checker does not
    /// read: a type parameter, an npm package's type, a stdlib-modeled type.
    fn type_param_count(&self, base: &TypeExpr) -> Option<usize> {
        match self.lowerer.lower(base) {
            Ty::Imported { module, name } => self
                .decl_ty_resolver
                .imported_type_decl(module.as_str(), name.as_ref())
                .map(|d| d.generics.len()),
            Ty::Named { symbol, path } => {
                let name = path.last()?;
                let local = self
                    .resolved
                    .symbols
                    .table
                    .get(SymbolId(symbol.0))
                    .filter(|sym| sym.name == *name)
                    .and_then(|sym| match sym.kind {
                        SymbolKind::Type { decl_idx } => {
                            match self.module.items.get(decl_idx as usize)? {
                                Decl::Type(td) => Some(td.generics.len()),
                                Decl::Interface(id) => Some(id.generics.len()),
                                _ => None,
                            }
                        }
                        _ => None,
                    });
                local.or_else(|| {
                    let id = self.lowerer.prelude.lookup(name.as_ref())?;
                    if id != SymbolId(symbol.0) {
                        return None;
                    }
                    match self.lowerer.prelude.table.get(id)?.kind {
                        SymbolKind::Prelude { kind } => prelude_type_param_count(kind),
                        _ => None,
                    }
                })
            }
            Ty::Prim(_) | Ty::UnknownTop | Ty::Never => Some(0),
            _ => None,
        }
    }

    /// Whether a written type resolves to the prelude container `name`
    /// (`Nullable`, `Option`), whatever spelling brought it into scope.
    fn is_prelude_named(&self, te: &TypeExpr, name: &str) -> bool {
        let Ty::Named { symbol, path } = self.lowerer.lower(te) else {
            return false;
        };
        path.last().map(|n| n.as_ref()) == Some(name)
            && self.lowerer.prelude.lookup(name) == Some(SymbolId(symbol.0))
    }

    /// Whether `ty` is `Nullable<..>` or `Option<..>`, following a local alias
    /// whose body is one (`type O = Option<int>`) up to a fixed depth, which
    /// is also the cycle guard.
    fn resolves_to_nullable_or_option(&self, ty: &Ty, depth: usize) -> bool {
        if self.prelude_app(ty, "Nullable").is_some() || self.prelude_app(ty, "Option").is_some() {
            return true;
        }
        if depth >= 8 {
            return false;
        }
        match self.local_type_decl(ty) {
            Some(td) if td.generics.is_empty() => {
                let body = self.lowerer.lower(&td.body);
                self.resolves_to_nullable_or_option(&body, depth + 1)
            }
            _ => false,
        }
    }

    fn walk_decl(&mut self, decl: &Decl) {
        self.check_decl_nullable_args(decl);
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
                // The one parameter is that props object, so a type the
                // checker can decide is not a record can never be what it
                // holds (G243, E0229).
                if let [param] = c.params.as_slice() {
                    if let PropsParam::NotRecord(why) = self.component_props_param(c, param) {
                        self.errors.push(TypeError::ComponentParamNotRecord {
                            component: c.name.to_string(),
                            param: param.name.to_string(),
                            ty: type_expr_source(&param.ty),
                            why,
                            span: param.span,
                        });
                    }
                }
                // A `component` lowers to a React function component; there is
                // no `async component`, so it is never an async context. One
                // written without a return type returns the prelude
                // `Component` (D19), and its body is held to exactly what
                // `-> Component` would hold it to.
                let er = match c.return_ty.as_ref() {
                    Some(te) => self.enclosing_return(Some(te), false),
                    None => self.enclosing_return_ty(self.lowerer.prelude_component(), false),
                };
                let wants_value = ty_requires_value(&er.ty);
                self.return_stack.push(er);
                self.bind_param_tys(&c.params);
                self.walk_block(&c.body);
                if wants_value {
                    self.check_value_match_tail(&c.body);
                }
                self.return_stack.pop();
            }
            Decl::Const(c) => {
                self.walk_expr(&c.value);
                // An annotated module `const` gets the check an annotated `let`
                // gets (G149). Lowering the const to its annotation made every
                // read of it trust the annotation, so an initializer that
                // disagrees with it is the same load-bearing lie the `let` case
                // is: `const X: number = "hi"` typed `X` as `number` while it
                // held a string, and only `tsc` (TS2322) said so. Judged only
                // when there is an annotation, and `found` is read off the
                // initializer's span so the diagnostic underlines the value.
                if let Some(te) = &c.ty {
                    let expected = self.lowerer.lower(te);
                    let found = self.tm.get(c.value.span()).clone();
                    if let Some(r) = self.value_refusal(&c.value, &found, &expected) {
                        let accepted = self.accepted_values(&r.expected);
                        self.errors.push(TypeError::TypeMismatch {
                            expected: ty_display(&r.expected),
                            found: r.found,
                            accepted,
                            missing_fields: r.missing_fields,
                            no_common_field: r.no_common_field,
                            span: r.span,
                        });
                    }
                }
            }
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
                if let Some(te) = &l.ty {
                    self.check_nullable_args(te);
                }
                self.walk_expr(&l.value);
                if let Expr::Match { arms, .. } = &l.value {
                    self.check_arms_produce_values(arms);
                }
                // D48: a pattern takes no annotation (the parser refuses one),
                // and gives each name its own element's type.
                if let LetBinding::Pattern(p) = &l.binding {
                    let value_ty = self.tm.get(l.value.span()).clone();
                    self.bind_let_pattern(p, &l.value, &value_ty);
                    return;
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
                    None => self
                        .widen_fresh_literals(&l.value, self.tm.get(l.value.span()).clone()),
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
                    if let Some(r) = self.value_refusal(&l.value, &found, &ty) {
                        let accepted = self.accepted_values(&r.expected);
                        self.errors.push(TypeError::TypeMismatch {
                            expected: ty_display(&r.expected),
                            found: r.found,
                            accepted,
                            missing_fields: r.missing_fields,
                            no_common_field: r.no_common_field,
                            span: r.span,
                        });
                    }
                }
                if l.ty.is_some() || !ty.is_unknown() {
                    self.local_tys.insert(l.span.start, ty);
                }
                if let Some(f) = self.stdlib_values.get(&l.value.span()).cloned() {
                    self.local_stdlib_values.insert(l.span.start, f);
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
                        // A written array widens here the way it widens at a
                        // `let` (G237): `for m in ["read", "write"]` emits a
                        // TypeScript loop over a `string[]`, so the binding is
                        // a `string` in the program that runs.
                        let elem = self.widen_fresh_literals(&f.iter, elem);
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

    /// D48: prove a destructuring `let`'s initializer has the pattern's
    /// length, and give each name the type of its element.
    ///
    /// Four readings, the table in the spec:
    /// - a written array literal with no spread is counted here, and each name
    ///   takes its element's type widened the way a `let` widens (D30), so
    ///   `let [m] = ["read"]` holds a `string` as `let m = "read"` does;
    /// - a value whose type the checker cannot see (`Ty::Unknown`, an npm
    ///   import) is left to `tsc`, which the emitted `satisfies` makes prove
    ///   the length, and its names stay open;
    /// - an `Array<T>` is refused: nothing knows its length before run time;
    /// - anything else is refused as not an array.
    ///
    /// A nested pattern is read against its element by the same rules, so
    /// `let [[a, b], c] = [[1, 2], 3]` is counted at both levels. Each name's
    /// type is keyed at its own span, the def site the resolver bound it at,
    /// and recorded in the type map there for inlay hints.
    fn bind_let_pattern(&mut self, pattern: &Pattern, value: &Expr, ty: &Ty) {
        let Pattern::Array { elements, rest, .. } = pattern else {
            return;
        };
        if let Expr::Array { elements: written, .. } = value {
            let exprs: Option<Vec<&Expr>> = written
                .iter()
                .map(|el| match el {
                    ArrayElem::Expr(e) => Some(e),
                    ArrayElem::Spread(_) => None,
                })
                .collect();
            // A literal with a spread has no length written down; it falls
            // through to its type, an `Array<T>`.
            if let Some(exprs) = exprs {
                let fits = match rest {
                    Some(_) => exprs.len() >= elements.len(),
                    None => exprs.len() == elements.len(),
                };
                if !fits {
                    self.errors.push(TypeError::DestructureUnproven {
                        why: DestructureRefusal::Count {
                            pattern: elements.len(),
                            at_least: rest.is_some(),
                            literal: exprs.len(),
                        },
                        span: value.span(),
                    });
                    return;
                }
                for (sub, e) in elements.iter().zip(exprs.iter()) {
                    self.bind_let_slot(sub, e);
                }
                if let Some(Pattern::Ident { span, .. }) = rest.as_deref() {
                    // The rest holds the elements left over, as an array of
                    // their joined type; with none left, or none that join,
                    // the binding stays open rather than claim an element type.
                    let mut joined: Option<Ty> = None;
                    for e in &exprs[elements.len()..] {
                        let t = self.widen_fresh_literals(e, self.tm.get(e.span()).clone());
                        joined = match joined {
                            None if ty_is_decidable(&t) => Some(t),
                            Some(prev) if prev == t => Some(prev),
                            Some(prev) if ty_is_decidable(&t) => match join_ty(&prev, &t) {
                                Ty::Unknown => None,
                                j => Some(j),
                            },
                            _ => None,
                        };
                        if joined.is_none() {
                            break;
                        }
                    }
                    if let Some(elem) = joined {
                        let arr = self.array_ty(elem);
                        self.tm.insert(*span, arr.clone());
                        self.local_tys.insert(span.start, arr);
                    }
                }
                return;
            }
        }
        if ty.is_unknown() {
            return;
        }
        let canonical = self.canonicalize_aliases(ty);
        // Shown as a `let` would hold it: `"ab"` is a `string` there.
        let shown = ty_display(&self.widen_fresh_literals(value, ty.clone()));
        let why = match self.prelude_app(&canonical, "Array") {
            Some(_) => DestructureRefusal::UnknownLength {
                ty: shown,
                spread: matches!(value, Expr::Array { .. }),
            },
            None => DestructureRefusal::NotAnArray { ty: shown },
        };
        self.errors.push(TypeError::DestructureUnproven {
            why,
            span: value.span(),
        });
    }

    /// One element of a destructuring `let` over a written literal: a name
    /// takes the element's widened type, a nested pattern is read against the
    /// element, and `_` reads nothing.
    fn bind_let_slot(&mut self, sub: &Pattern, e: &Expr) {
        let ty = self.tm.get(e.span()).clone();
        match sub {
            Pattern::Ident { span, .. } => {
                let widened = self.widen_fresh_literals(e, ty);
                if !widened.is_unknown() {
                    self.tm.insert(*span, widened.clone());
                    self.local_tys.insert(span.start, widened);
                }
            }
            Pattern::Array { .. } => self.bind_let_pattern(sub, e, &ty),
            _ => {}
        }
    }

    // ----- expressions -----

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            Expr::Number { span, .. } => self.tm.insert(*span, Ty::Prim(Primitive::Number)),
            // A written string literal is typed as the one-literal union it
            // spells (G237), which is what TypeScript reads it as. The rule
            // that widens it back to `string` is `widen_fresh_literals`, and
            // it runs where TypeScript's own widening runs: at a binding whose
            // type is inferred from the value.
            Expr::String { span, value } => self
                .tm
                .insert(*span, Ty::StringLiteralUnion(vec![value.clone()])),
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
                let ty = self.value_read_ty(e, ty);
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
            Expr::Member { object, field, span, optional } => {
                // The object of a member access is the one expression position
                // a type name belongs in: `Order.parse(json)` and `Order.is(v)`
                // are the descriptor forms (G217). Marked before the walk and
                // restored after, so nothing outside this object is exempt.
                let outer_member_object = match object.as_ref() {
                    Expr::Ident { span, .. } => self.member_object.replace(*span),
                    _ => self.member_object,
                };
                self.walk_expr(object);
                self.member_object = outer_member_object;
                let mut obj_ty = self.tm.get(object.span()).clone();
                // G304, D45. A chain this link continues moves out to it, and
                // a `?.` over a `Nullable<R>` starts one and reads the field
                // off `R`. The plain dot over one may read null: E0235.
                let chain = self.null_chains.take(object.span());
                // E0236. One refusal at the link the chain starts from, and
                // the node is `Unknown`, so the links after it read nothing.
                if *optional && self.refuse_optional_chain_on_tagged(object, &obj_ty, field, *span) {
                    if let Some(c) = chain {
                        self.null_chains.open(*span, c);
                    }
                    self.tm.insert(*span, Ty::Unknown);
                    return;
                }
                let mut root = None;
                if let Some(inner) = self.nullable_inner(&obj_ty) {
                    let through = ty_display(&obj_ty);
                    if !*optional {
                        self.errors.push(TypeError::NullableRead {
                            kind: crate::NullableReadKind::Dot,
                            field: field.to_string(),
                            type_name: through,
                            span: *span,
                        });
                        if let Some(c) = chain {
                            self.null_chains.open(*span, c);
                        }
                        self.tm.insert(*span, Ty::Unknown);
                        return;
                    }
                    root = Some((field.to_string(), through));
                    obj_ty = inner;
                }
                if let Some(c) = chain.or(root) {
                    self.null_chains.open(*span, c);
                }
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
                            let record = self.field_owner_decl(&shape.owner);
                            self.errors.push(TypeError::UnknownField {
                                field: field.to_string(),
                                type_name: field_owner_display(&obj_ty),
                                second_name_for: self.second_name_target(&obj_ty, &shape.owner),
                                fields: shape
                                    .fields
                                    .iter()
                                    .map(|f| f.name.to_string())
                                    .collect(),
                                record,
                                builtin: None,
                                suggestion: nearest_name(
                                    field.as_ref(),
                                    shape.fields.iter().map(|f| f.name.as_ref()),
                                ),
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
                    // A built-in receiver (G288). An `Array<T>`, a `string` or a
                    // `number` is a JavaScript value whose member set the TypeScript library
                    // fixes, so a name outside it is the same mistake as a field
                    // a record does not declare, and gets the same code.
                    //
                    // `length` on an array or a string is a `number`. It is
                    // the only data property either has (every other member
                    // is a method), so this is their whole property set. An
                    // `int` wants it as well, since D31 makes the two
                    // mutually assignable.
                    //
                    // A method is typed from the table generated out of
                    // TypeScript's library (G312), with the array's element
                    // type for its `T`, so the call arm checks the arguments
                    // and gives the call its return type. Read without being
                    // called, it is E0234: the bare function loses its
                    // receiver and throws when called later, and `tsc` lets
                    // the read through.
                    None => {
                        self.unresolved_field_use(&obj_ty, field, *span, access);
                        let (receiver, elem) = match self.builtin_receiver_elem(&obj_ty) {
                            Some((r, elem)) => (Some(r), elem),
                            None => (None, Ty::Unknown),
                        };
                        let method = receiver.and_then(|r| Some((r, r.method(field.as_ref())?)));
                        match receiver.filter(|r| !r.has(field.as_ref())) {
                            None if field.as_ref() == "length"
                                && matches!(
                                    receiver,
                                    Some(BuiltinReceiver::Array | BuiltinReceiver::String)
                                ) =>
                            {
                                Ty::Prim(Primitive::Number)
                            }
                            Some(receiver) => {
                                let members = receiver.members().iter().copied();
                                self.errors.push(TypeError::UnknownField {
                                    field: field.to_string(),
                                    type_name: ty_display(&obj_ty),
                                    second_name_for: None,
                                    fields: members.clone().map(str::to_string).collect(),
                                    record: None,
                                    builtin: Some(receiver),
                                    suggestion: nearest_name(field.as_ref(), members),
                                    span: *span,
                                });
                                Ty::Unknown
                            }
                            None => match method {
                                // A write (`mut xs.push = f`) detaches
                                // nothing, so only a read is refused.
                                Some((receiver, m))
                                    if self.call_callee != Some(*span)
                                        && matches!(access, FieldAccess::Read) =>
                                {
                                    // The help writes the call back, and the
                                    // lambda that wraps it when the method
                                    // takes no argument and has a type.
                                    let recv = receiver_source(object);
                                    let lambda = match builtin_method_ty(&m.sig, &elem, |t| {
                                        self.array_ty(t)
                                    }) {
                                        Ty::Fn { params, return_ty, .. }
                                            if params.is_empty()
                                                && !return_ty.is_unknown() =>
                                        {
                                            Some(format!(
                                                "fn() -> {} {{ return {recv}.{}() }}",
                                                ty_display(&return_ty),
                                                m.name
                                            ))
                                        }
                                        _ => None,
                                    };
                                    let call = match lambda {
                                        Some(_) => format!("{recv}.{}()", m.name),
                                        None => format!("{recv}.{}(...)", m.name),
                                    };
                                    self.errors.push(TypeError::UncalledBuiltinMethod {
                                        method: m.name.to_string(),
                                        type_name: ty_display(&obj_ty),
                                        builtin: receiver,
                                        call,
                                        lambda,
                                        span: *span,
                                    });
                                    Ty::Unknown
                                }
                                Some((_, m)) => builtin_method_ty(&m.sig, &elem, |t| self.array_ty(t)),
                                None => self
                                    .stdlib_member_ty(object, field)
                                    .or_else(|| self.descriptor_member_ty(object, field))
                                    .unwrap_or(Ty::Unknown),
                            },
                        }
                    }
                };
                let member_ty = self.value_read_ty(e, member_ty);
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
            Expr::Binary { left, right, span, op } => {
                self.walk_expr(left);
                self.walk_expr(right);
                let ty = match op {
                    glyph_ast::BinOp::NullishCoalesce => self.coalesce_ty(left, right),
                    // A comparison reads the chain's value without holding
                    // it, so `n?.f == "x"` handles the `undefined` (D45).
                    glyph_ast::BinOp::Eq | glyph_ast::BinOp::NotEq => {
                        self.null_chains.handle(left.span());
                        self.null_chains.handle(right.span());
                        Ty::Unknown
                    }
                    _ => Ty::Unknown,
                };
                self.tm.insert(*span, ty);
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
                // `n?.xs[0]` is one chain (G304): the index is its value now.
                if let Some(c) = self.null_chains.take(object.span()) {
                    self.null_chains.open(*span, c);
                }
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
                for t in type_args {
                    self.check_nullable_args(t);
                }
                let outer = self.call_callee.replace(callee.span());
                self.walk_expr(callee);
                self.call_callee = outer;
                // `n?.f()` is one chain (G304): the call is its value now.
                if let Some(c) = self.null_chains.take(callee.span()) {
                    self.null_chains.open(*span, c);
                }
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
                let call_ty = if let Ty::Fn {
                    params,
                    return_ty,
                    type_params,
                    ..
                } = &callee_ty
                {
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
                    // A stdlib parameter a type argument can name
                    // (`json.parse`'s `T`, G289; `json.parse_with`'s, G298) is
                    // the written type argument, else what the arguments
                    // bound, else the row's own answer for an unwritten one.
                    if let Some((key, field)) = self.stdlib_callee(callee) {
                        let names = crate::stdlib::explicit_type_params(&key, &field);
                        // E0233: the row lists every type parameter the
                        // function has, so a written count that is not its
                        // own binds nothing, the rule G292 keeps below.
                        let written = if names.is_empty()
                            || type_args.is_empty()
                            || names.len() == type_args.len()
                        {
                            type_args.as_slice()
                        } else {
                            self.errors.push(TypeError::TypeArgumentCountMismatch {
                                name: callee_source(callee),
                                expected: names.len(),
                                found: type_args.len(),
                                value_of: None,
                                span: *span,
                            });
                            &[]
                        };
                        for (i, (name, unwritten)) in names.iter().enumerate() {
                            match written.get(i) {
                                Some(t) => {
                                    subst.insert(Ident::from(*name), self.lowerer.lower(t));
                                }
                                None => {
                                    subst.entry(Ident::from(*name)).or_insert(unwritten.ty());
                                }
                            }
                        }
                    }
                    // G292. A written type argument is the binding of the
                    // parameter it is written for, whatever the arguments
                    // would have inferred, so it replaces the inferred one.
                    // Only when the count is the declaration's. Any other
                    // count is E0233 (TS2558 under `tsc`), and it binds
                    // nothing: pairing them by position would report a
                    // mismatch about a binding the program never made. A
                    // declaration with no type parameters takes 0, so any
                    // written type argument on it is the same mistake.
                    //
                    // The declaration's list comes first, then the callee's
                    // type: a function bound to a value (`let f = id`) is not
                    // a declaration the resolver can name, and its type
                    // carries the list so `f<number>(x)` is read as
                    // `id<number>(x)` is. A type whose list is not known (a
                    // stdlib signature) stays unchecked, as it always was.
                    if !type_args.is_empty() {
                        if let Some(names) =
                            self.callee_generics(callee).or_else(|| type_params.clone())
                        {
                            if names.len() == type_args.len() {
                                for (name, t) in names.into_iter().zip(type_args.iter()) {
                                    subst.insert(name, self.lowerer.lower(t));
                                }
                            } else {
                                // A value read off `json.parse` says how to
                                // write the validating call instead.
                                let value_of =
                                    self.stdlib_values.get(&callee.span()).map(|f| {
                                        let written: Vec<String> =
                                            type_args.iter().map(type_expr_source).collect();
                                        (f.clone(), written.join(", "))
                                    });
                                self.errors.push(TypeError::TypeArgumentCountMismatch {
                                    name: callee_source(callee),
                                    expected: names.len(),
                                    found: type_args.len(),
                                    value_of,
                                    span: *span,
                                });
                            }
                        }
                    }
                    // Check each argument against its parameter type (with any
                    // inferred generics substituted in). Reports only a provable
                    // mismatch; an undecidable argument or parameter is silent.
                    for (p, a) in params.iter().zip(args.iter()) {
                        let expected = substitute_type_params(&p.ty, &subst);
                        let found = self.tm.get(a.span()).clone();
                        if let Some(r) = self.value_refusal(a, &found, &expected) {
                            let accepted = self.accepted_values(&r.expected);
                            self.errors.push(TypeError::ArgumentTypeMismatch {
                                expected: ty_display(&r.expected),
                                found: r.found,
                                accepted,
                                missing_fields: r.missing_fields,
                                no_common_field: r.no_common_field,
                                children_between_tags: false,
                                span: r.span,
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
                // Synthesize the record type the literal writes, so the value
                // is compared against what it was written for (G232). Until
                // this, an object literal was `Ty::Unknown`: `let g: string =
                // { x: 1 }` drew nothing from `glyph check --no-tsc` while
                // `tsc` refused it, and the same silence covered a `return`, a
                // call argument and a record field. Every rule the type now
                // reaches was already written — the record recursion in
                // `definitely_incompatible`, the record-against-a-primitive arm
                // and the width-subtyping comparison — and had no value to
                // read.
                //
                // Only when every field is written out. A spread contributes
                // fields this walk cannot enumerate, so a record built from
                // one would claim a field set the value does not have, and
                // "missing required field" would fire on a literal that
                // carries it. That is a false refusal, which costs more than
                // the silence, so a literal holding a spread stays `Unknown`
                // exactly as before.
                let ty = self.object_literal_ty(fields);
                self.tm.insert(*span, ty);
            }
            Expr::Match { scrutinee, arms, span } => {
                for arm in arms {
                    // A bare name in an `is` arm is a runtime test of the
                    // declaration (D9), not an annotation, so the bare count
                    // (G292) does not apply to it: `is S` on a generic `S` is
                    // still E0300 from the emitter. An applied one is walked.
                    if let Pattern::IsType { ty, .. } = &arm.pattern {
                        if !matches!(ty, TypeExpr::Path { .. }) {
                            self.check_nullable_args(ty);
                        }
                    }
                }
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
                for p in params {
                    self.check_nullable_args(&p.ty);
                }
                if let Some(rt) = return_ty {
                    self.check_nullable_args(rt);
                }
                let er = self.enclosing_return(return_ty.as_ref(), *is_async);
                let wants_value = ty_requires_value(&er.ty);
                self.return_stack.push(er);
                self.bind_param_tys(params);
                self.walk_block(body);
                if wants_value {
                    self.check_value_match_tail(body);
                }
                self.return_stack.pop();
                let ty = self.lowerer.lower_callable_signature(
                    &[],
                    params,
                    return_ty.as_ref(),
                    *is_async,
                );
                self.tm.insert(*span, ty);
            }
            Expr::Jsx(j) => {
                self.walk_jsx(j);
                self.tm.insert(j.span, Ty::Unknown);
            }
        }
    }

    /// `T` when `ty` is a `Nullable<T>`, read through second names and
    /// through a `type` whose body is one (`type MaybeNote = Nullable<Note>`),
    /// local or a sibling's under either import spelling, so the guarantee
    /// does not depend on which name a parameter wrote. A generic one
    /// (`type Maybe<T> = Nullable<T>`) is read with the application's
    /// arguments substituted into its body, so `Maybe<Note>` is
    /// `Nullable<Note>`.
    fn nullable_inner(&self, ty: &Ty) -> Option<Ty> {
        match self.container_through_aliases(ty, 0)? {
            ("Nullable", args) => args.first().cloned(),
            _ => None,
        }
    }

    /// The prelude container `ty` is (`Option`, `Result`, `Array`, `Record`
    /// or `Nullable`) with its arguments, read through aliases the way
    /// `nullable_inner` reads a `Nullable`: second names, a local or a
    /// sibling's `type` whose body is one, and a generic one with the
    /// application's arguments substituted. A user type that shadows a
    /// prelude name is not the prelude's, since `prelude_container` checks
    /// the resolved symbol and not the spelling.
    fn container_through_aliases(&self, ty: &Ty, depth: usize) -> Option<(&'static str, Vec<Ty>)> {
        let canonical = self.canonicalize_aliases(ty);
        if let Some((name, args)) = self.prelude_container(&canonical) {
            return Some((name, args.to_vec()));
        }
        if depth >= 8 {
            return None;
        }
        let (base, args) = split_type_app(&canonical);
        if let Ty::Imported { module, name } = base {
            let (_, _, decl) =
                imported_decl_chain_end(self.decl_ty_resolver, module.as_str(), name.as_ref())?;
            let body = alias_body_applied(&decl.body, &decl.generics, args)?;
            return self.container_through_aliases(&body, depth + 1);
        }
        match self.local_type_decl(&canonical) {
            Some(td) if td.refinement.is_none() => {
                let generics: Vec<Ident> = td.generics.iter().map(|g| g.name.clone()).collect();
                let body = alias_body_applied(&self.lowerer.lower(&td.body), &generics, args)?;
                self.container_through_aliases(&body, depth + 1)
            }
            _ => None,
        }
    }

    /// E0236 (G311, record 87): `?.` written on a receiver that resolves to
    /// the prelude `Option` or `Result`. Answers whether it was refused.
    fn refuse_optional_chain_on_tagged(
        &mut self,
        object: &Expr,
        obj_ty: &Ty,
        field: &Ident,
        span: Span,
    ) -> bool {
        let (container, args) = match self.container_through_aliases(obj_ty, 0) {
            Some(("Option", args)) => (crate::TaggedContainer::Option, args),
            Some(("Result", args)) => (crate::TaggedContainer::Result, args),
            _ => return false,
        };
        let name = match container {
            crate::TaggedContainer::Option => "Option",
            crate::TaggedContainer::Result => "Result",
        };
        let shown: Vec<String> = args.iter().map(ty_display).collect();
        let payload = shown.first().cloned().unwrap_or_else(|| "T".to_string());
        let receiver = receiver_source(object);
        // The help binds the payload to the first letter of its type's name
        // (`Note` is `n`), unless that would shadow the receiver it opens.
        let binding = payload
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase().to_string())
            .filter(|b| *b != receiver)
            .unwrap_or_else(|| "v".to_string());
        self.errors.push(TypeError::OptionalChainOnTagged {
            container,
            type_name: format!("{name}<{}>", shown.join(", ")),
            field: field.to_string(),
            method: self.call_callee == Some(span),
            receiver,
            binding,
            payload,
            span,
        });
        true
    }

    /// The type of `left ?? right` (G304, D45).
    ///
    /// Two lefts have a type here, and each removes an absence: an optional
    /// chain over a `Nullable` (its `undefined`, and also the field's own null
    /// when the field is a `Nullable<T>`), and a `Nullable<T>` value itself.
    /// The expression is the field's type, or `T`, when the default is
    /// accepted there by a rule of the relation. Anything else stays `Unknown`
    /// and `tsc` decides, because the union TypeScript would give it has no
    /// Glyph spelling.
    fn coalesce_ty(&mut self, left: &Expr, right: &Expr) -> Ty {
        let left_ty = self.tm.get(left.span()).clone();
        let target = if self.null_chains.handle(left.span()) {
            self.nullable_inner(&left_ty).unwrap_or(left_ty)
        } else {
            match self.nullable_inner(&left_ty) {
                Some(inner) => inner,
                None => return Ty::Unknown,
            }
        };
        let default = self.tm.get(right.span()).clone();
        if ty_is_decidable(&target) && self.accepting_rule(&default, &target).is_some() {
            target
        } else {
            Ty::Unknown
        }
    }

    /// E0235 for every optional chain over a `Nullable` the walk opened and
    /// nothing handled. Called after each declaration's walk.
    fn report_unhandled_null_chains(&mut self) {
        let mut open = std::mem::take(&mut self.null_chains.open);
        open.sort_by_key(|(s, ..)| (s.start, s.end));
        for (span, field, type_name) in open {
            self.errors.push(TypeError::NullableRead {
                kind: crate::NullableReadKind::ChainEscapes,
                field,
                type_name,
                span,
            });
        }
    }

    fn walk_jsx(&mut self, j: &JsxElement) {
        if j.kind() == JsxKind::Match {
            self.walk_jsx_match(j);
            return;
        }
        for attr in &j.attrs {
            match attr {
                JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => self.walk_expr(value),
                // A `<case>` outside a `<match>` is the emitter's to refuse
                // (E0300); its pattern has no scrutinee to be checked against.
                JsxAttr::String { .. } | JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } => {}
            }
        }
        self.check_jsx_component_attrs(j);
        self.walk_jsx_children(&j.children);
    }

    fn walk_jsx_children(&mut self, children: &[JsxChild]) {
        for child in children {
            match child {
                JsxChild::Element(e) => self.walk_jsx(e),
                JsxChild::Expr(e) => self.walk_expr(e),
                JsxChild::Text { .. } => {}
            }
        }
    }

    /// A JSX `<match value={s}>` is a value-level `match` written in JSX (D6),
    /// and is checked as one: `as_value_match` turns each `<case>` into an arm
    /// holding the case's pattern and what it renders, and the arm goes
    /// through `Expr::Match`'s own walk. So a case's bindings are typed by
    /// `bind_arm_payloads`, its pattern is checked against the payload there
    /// (G279), coverage and reachability go through `check_match_exhaustiveness`
    /// (a refutable `<case Loaded({ status: 404 })>` alone does not cover
    /// `Loaded`, E0200), and an untyped scrutinee is recovered from the case
    /// patterns exactly as from arm patterns.
    ///
    /// What the arm walk does not see is walked here: a `<case>` with no
    /// pattern, and a child that is not a `<case>` (the emitter refuses both,
    /// E0300, but the names inside are still typed). `bind={x}` on a case is
    /// E0231, raised after the walk so the help can read the payload's shape
    /// off the scrutinee's type.
    fn walk_jsx_match(&mut self, j: &JsxElement) {
        for attr in &j.attrs {
            match attr {
                JsxAttr::Expr { name, .. } if name.as_ref() == "value" => {}
                JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => self.walk_expr(value),
                JsxAttr::String { .. } | JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } => {}
            }
        }
        let value_span = match j.as_value_match() {
            Some(m) => {
                let Expr::Match { scrutinee, .. } = &m else {
                    unreachable!("as_value_match builds a match")
                };
                let span = scrutinee.span();
                self.walk_expr(&m);
                Some(span)
            }
            None => None,
        };
        let scrutinee_ty = value_span.map(|s| self.tm.get(s).clone()).unwrap_or(Ty::Unknown);
        for child in &j.children {
            match child {
                JsxChild::Element(el) if el.kind() == JsxKind::Case => {
                    if value_span.is_none() || el.case_pattern().is_none() {
                        self.walk_jsx_children(&el.children);
                    }
                    self.check_case_bind(el, &scrutinee_ty);
                }
                JsxChild::Element(e) => self.walk_jsx(e),
                JsxChild::Expr(e) => self.walk_expr(e),
                JsxChild::Text { .. } => {}
            }
        }
    }

    /// `bind={x}` on a `<case>` is E0231. The help writes the pattern that
    /// replaces it, by what the variant carries: a record payload's field by
    /// an object pattern (which reads the field `bind` read), one value by a
    /// name in parentheses, a variant with no payload by its bare name.
    fn check_case_bind(&mut self, case: &JsxElement, scrutinee_ty: &Ty) {
        let Some((value, span)) = case.attrs.iter().find_map(|a| match a {
            JsxAttr::Expr { name, value, span } if name.as_ref() == "bind" => Some((value, *span)),
            _ => None,
        }) else {
            return;
        };
        let variant = match case.case_pattern() {
            Some(Pattern::Ident { name, .. }) => Some(name.clone()),
            Some(Pattern::Constructor { path, .. }) => path.last().cloned(),
            _ => None,
        };
        let name = match value {
            Expr::Ident { name, .. } => name.to_string(),
            _ => "name".to_string(),
        };
        let shape = match &variant {
            None => CaseBindShape::Unknown,
            Some(v) => match self.variant_payload(scrutinee_ty, v) {
                Some(payload) => {
                    let is_record = self.record_shape_of(&payload).is_some()
                        || self.record_shape_of(&self.canonicalize_aliases(&payload)).is_some();
                    match is_record {
                        true => CaseBindShape::Record,
                        false => CaseBindShape::Positional,
                    }
                }
                None => match self.required_variants(scrutinee_ty) {
                    Some((_, vs)) if vs.contains(v) => CaseBindShape::Nullary,
                    _ => CaseBindShape::Unknown,
                },
            },
        };
        self.errors.push(TypeError::CaseBind {
            variant: variant.map(|v| v.to_string()).unwrap_or_else(|| "Variant".to_string()),
            name,
            shape,
            span,
        });
    }

    /// Every attribute of a component element, against the props record the
    /// component declares (G240).
    ///
    /// `<Button variant="nope" />` lowers to
    /// `React.createElement(Button, { variant: "nope" })`, so an attribute is
    /// an argument and the props record is the parameter it is passed to. The
    /// rule is the call-argument relation reached through the same two
    /// helpers, `value_refusal` and `accepted_values`, which is why a wrong
    /// attribute draws the `E0211` a wrong argument draws, carries the same
    /// `expected`/`found` pair, and carries a string-literal union's members
    /// as its `alternatives` so `glyph fix` and `check --agent` can act on it
    /// (D30). Before this, nothing in the front end read a JSX attribute at
    /// all: `tsc` on the emitted TypeScript was the only thing that saw one,
    /// which left `--no-tsc`, the language server, the MCP tools and the
    /// playground blind.
    ///
    /// Three readings the rule is built on, each established by running the
    /// program under `tsc --strict`:
    ///
    /// - **Children are not a prop.** The emitter passes them as
    ///   `React.createElement(tag, props, ...children)` varargs, never inside
    ///   the props object, and `tsc` accepts children on a component whose
    ///   props record declares no `children` field. So nothing here reads
    ///   `j.children`. The converse holds too: written children do not
    ///   supply a `children` field the props record requires, because `tsc`
    ///   checks the props object alone against the record and reports
    ///   `children` missing from it. A required `children` is therefore asked
    ///   for like any other required prop, as an attribute, and a
    ///   `children?:` field is never asked for.
    /// - **A spread supplies what it supplies.** `<Button {...p} />` may carry
    ///   every required prop, and `tsc` accepts it, so a missing attribute is
    ///   not decidable next to a spread. An explicitly written attribute is
    ///   still wrong when it is wrong, and `tsc` refuses that one with the
    ///   spread present, so every attribute below is checked whether or not
    ///   the element also spreads.
    /// - **An optional field may be omitted.** `hint?: string` is
    ///   `RecordField::optional`, and an element that omits it compiles.
    ///   A written one is checked against the field's type like any other.
    /// - **A required field may not be omitted.** `<Card title="x" />` lowers
    ///   to `createElement(Card, { title: "x" })`, and `tsc` refuses that
    ///   object when `Card` requires a `count`: the props object is the
    ///   argument, so the element draws the `E0211` an argument record missing
    ///   a field draws, at the element's name, with the missing names as
    ///   `missing_fields`. `<Card />` is the same question, because the
    ///   emitter writes an attribute-less component's props as `{}`. It used
    ///   to write `null`, which React's declaration accepts for any component,
    ///   so `<Card />` compiled under `tsc --strict` and crashed at render.
    ///
    /// `key` is React's own attribute (it lives on `Attributes`, not on the
    /// props type) and `tsc` accepts it on every component, so it is never an
    /// unknown one. The emitter also injects it itself on a `<for>`-mapped
    /// element.
    fn check_jsx_component_attrs(&mut self, j: &JsxElement) {
        let Some((props_ty, shape)) = self.jsx_component_props_shape(j) else {
            return;
        };
        for attr in &j.attrs {
            // A positional attribute belongs to a directive, and a spread's
            // object is checked where it is written, not against one field.
            let (name, attr_span, written, found) = match attr {
                JsxAttr::String { name, value, span } => (
                    name,
                    *span,
                    None,
                    Ty::StringLiteralUnion(vec![value.clone()]),
                ),
                JsxAttr::Expr { name, value, span } => (
                    name,
                    *span,
                    Some(value),
                    self.tm.get(value.span()).clone(),
                ),
                JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } | JsxAttr::Spread { .. } => continue,
            };
            if name.as_ref() == "key" {
                continue;
            }
            let Some(field) = shape.fields.iter().find(|f| f.name == *name) else {
                self.errors.push(TypeError::UnknownField {
                    field: name.to_string(),
                    type_name: field_owner_display(&props_ty),
                    second_name_for: self.second_name_target(&props_ty, &shape.owner),
                    fields: shape.fields.iter().map(|f| f.name.to_string()).collect(),
                    record: self.field_owner_decl(&shape.owner),
                    builtin: None,
                    suggestion: nearest_name(name.as_ref(), shape.fields.iter().map(|f| f.name.as_ref())),
                    span: attr_span,
                });
                continue;
            };
            // A written `name={expr}` goes through the expression form of the
            // relation, so an array or object literal is named by the element
            // that is wrong rather than as a whole. A `name="literal"` has no
            // expression to name: the attribute is its own span, and the
            // literal it spells is the one-literal union a written string
            // types as anywhere else.
            let refusal = match written {
                Some(value) => self.value_refusal(value, &found, &field.ty),
                None => self.assign_incompatible(&found, &field.ty).then(|| {
                    let display = self.found_display(&found, &field.ty);
                    self.refusal(&field.ty, display, attr_span)
                }),
            };
            if let Some(r) = refusal {
                let accepted = self.accepted_values(&r.expected);
                self.errors.push(TypeError::ArgumentTypeMismatch {
                    expected: ty_display(&r.expected),
                    found: r.found,
                    accepted,
                    missing_fields: r.missing_fields,
                    no_common_field: r.no_common_field,
                    children_between_tags: false,
                    span: r.span,
                });
            }
        }
        self.check_jsx_required_props(j, &props_ty, &shape);
    }

    /// The required props a component element does not write (see the last
    /// reading on `check_jsx_component_attrs`).
    ///
    /// Asked of every component element without a spread, since a spread
    /// may carry anything. An element with no attribute at all is asked too:
    /// the emitter writes its props as `{}`, never `null`, so `tsc` checks it
    /// against the props type like any other object. That covers the element
    /// a `<for ... key={x}>` maps, whose injected `key` is React's attribute
    /// and not a field of the props record. `found` is `record`, the name a
    /// written object literal has on every other refusal of one.
    fn check_jsx_required_props(&mut self, j: &JsxElement, props_ty: &Ty, shape: &RecordShape) {
        let mut written: Vec<&Ident> = Vec::new();
        for attr in &j.attrs {
            match attr {
                JsxAttr::String { name, .. } | JsxAttr::Expr { name, .. } => written.push(name),
                JsxAttr::Spread { .. } => return,
                JsxAttr::Positional { .. } | JsxAttr::Pattern { .. } => {}
            }
        }
        let missing_fields: Vec<String> = shape
            .fields
            .iter()
            .filter(|f| !f.optional && !written.iter().any(|w| **w == f.name))
            .map(|f| f.name.to_string())
            .collect();
        if missing_fields.is_empty() {
            return;
        }
        // Children written between the tags are `createElement` arguments
        // after the props object, never a field of it, so they do not fill a
        // required `children`. The refusal stands (`tsc` makes the same one);
        // the flag is what lets the help say why.
        let children_between_tags =
            missing_fields.iter().any(|f| f == "children") && jsx_writes_children(&j.children);
        self.errors.push(TypeError::ArgumentTypeMismatch {
            expected: ty_display(props_ty),
            found: "record".to_string(),
            accepted: None,
            missing_fields,
            no_common_field: false,
            children_between_tags,
            span: j.name_span,
        });
    }

    /// The props record a component element's attributes are checked against,
    /// or `None` for every element this rule has nothing to say about.
    ///
    /// Which elements are components is `JsxKind::classify`'s answer, the
    /// one the resolver and the emitter use too. An intrinsic (`<div>`) is
    /// React's own element table, which Glyph does not model, and a directive
    /// (`<if>`, `<else>`, `<for>`, `<match>`, `<case>`) is compiler-owned with
    /// attributes of its own. A component is reached one of two ways. A plain
    /// name (`<Button>`) is the reference the resolver recorded. A dotted
    /// name is a member of its base, and the member is in hand when the base
    /// is a namespace import (`<ui.Button>` under `import ui` or
    /// `import ui as u`): it is read through the same function the member
    /// expression `ui.Button` is, so the component and its props are the
    /// ones a named import would give. Any other dotted name
    /// (`<Ctx.Provider>`, `<a.b.C>`) is a property of a value whose type is
    /// not in hand, and is left to `tsc`.
    ///
    /// A component qualifies when it is a callable taking one parameter
    /// whose type is a record: the props-record form `E0214` already pushes
    /// every component toward. A component with no parameter takes no props,
    /// and `tsc` accepts any attribute on one, so it is left alone. The type
    /// is the one the component's parameter is *declared* with, not the alias
    /// chain's end, so a diagnostic names `Props` where the author wrote
    /// `Props` — the rule `E0210` already follows at a field access.
    fn jsx_component_props_shape(&mut self, j: &JsxElement) -> Option<(Ty, RecordShape)> {
        if j.kind() != JsxKind::Component {
            return None;
        }
        // The resolver keys the element's reference at the segment that
        // names a binding, which `binding_span` is the one definition of.
        let at = j.binding_span()?;
        let callee = match jsx_member_name(&j.name) {
            None => self.type_of_ident_ref(at),
            Some((_, member)) if !member.contains('.') => self.namespace_member_ty(at, member)?,
            Some(_) => return None,
        };
        let Ty::Fn { params, .. } = callee else {
            return None;
        };
        let [param] = params.as_slice() else {
            return None;
        };
        let declared = param.ty.clone();
        let shape = self.props_record_of(&declared)?;
        Some((declared, shape))
    }

    /// What a component's one parameter is as a props value, with its own type
    /// parameters judged by whether they are bounded (G246).
    ///
    /// `component Show<T>(p: T)` cannot be rendered with props: `tsc` refuses
    /// `<Show name="x" />` (TS2769), since an element cannot fill `T`, and a
    /// direct caller may fill it with a primitive. A bound is what can make it
    /// a props record, and `tsc --strict` with React 18's types accepts
    /// `<Show label="x" />` against `<T: HasLabel>` and against `<T: L>` for a
    /// record `L`. The resolver does not resolve a bound (it is emitted as a
    /// TypeScript `extends` clause for `tsc` to enforce), so this checker
    /// cannot read one, and a bounded parameter is undecided whatever the
    /// bound says.
    fn component_props_param(&self, c: &glyph_ast::ComponentDecl, param: &Param) -> PropsParam {
        if let TypeExpr::Path { segments, .. } = &param.ty {
            if let [name] = segments.as_slice() {
                if let Some(g) = c.generics.iter().find(|g| g.name == *name) {
                    return if g.bounds.is_empty() {
                        PropsParam::NotRecord(PropsRefusal::TypeParam)
                    } else {
                        PropsParam::Undecided
                    };
                }
            }
        }
        self.props_param(&self.lowerer.lower(&param.ty), 0)
    }

    /// The record a component's one parameter, declared as `declared`, holds
    /// its props in: the declaration's second names followed to the end, then
    /// read as a record. One method, because `declared_record_fields` answers
    /// for surfaces that report what an element was compared against, and
    /// two reads of the same thing is how they come to disagree.
    fn props_record_of(&self, declared: &Ty) -> Option<RecordShape> {
        let canonical = self.canonicalize_aliases(declared);
        self.record_shape_of(&canonical)
    }

    /// What a component's one parameter is as a props value (G243): a record,
    /// provably not one, or a type this checker cannot decide.
    ///
    /// React calls a component with the single object its element builds, so
    /// the only declarations that can hold it are a record and the types that
    /// describe any object. `NotRecord` is answered only where the value's
    /// emitted form is settled whatever fills it: a primitive, a string-literal
    /// union, a tagged union (`Option` and `Result` included), an `Array`, a
    /// `Nullable` (which says the object may be `null`, and it never is) and a
    /// function type, each seen through any chain of second names, local or
    /// imported.
    ///
    /// `unknown` is `NotRecord` too (G246): it describes the props object
    /// truthfully, and gives the component nothing it can read.
    ///
    /// Everything else is `Undecided`, because a refusal has to be proved.
    /// `Record<K, V>` describes a props object truthfully, and `tsc` accepts
    /// an element against it with attributes or without. An `extern_ts` or `typeof`
    /// body, an imported `interface`, an opaque stdlib handle and the interop
    /// types (`Component<P>`, `Schema<T>`) are shapes this checker does not
    /// read. `depth` guards a cycle of second names.
    fn props_param(&self, ty: &Ty, depth: usize) -> PropsParam {
        if depth > 8 {
            return PropsParam::Undecided;
        }
        match ty {
            Ty::Record { .. } => return PropsParam::Record,
            Ty::Prim(_) | Ty::StringLiteralUnion(_) | Ty::Union { .. } | Ty::Fn { .. } => {
                return PropsParam::NotRecord(PropsRefusal::NotAnObject);
            }
            Ty::UnknownTop => return PropsParam::NotRecord(PropsRefusal::Unknown),
            // A `Param` reached here is a generic declaration's own parameter,
            // read through its unsubstituted body (`type Same<T> = T`), and the
            // argument decides it. The component's own type parameters are
            // judged by their bounds in `component_props_param`.
            Ty::Unknown | Ty::Never | Ty::Param { .. } => {
                return PropsParam::Undecided;
            }
            Ty::Named { .. } | Ty::App { .. } | Ty::Imported { .. } => {}
        }
        let canonical = self.canonicalize_aliases(ty);
        // A named record, local or imported, an interface (whose members the
        // checker reads as a record, and which the element's attributes are
        // already checked against), and a stdlib record such as `fs.FileInfo`.
        if self.record_shape_of(&canonical).is_some() {
            return PropsParam::Record;
        }
        if let Some((name, _)) = self.prelude_container(&canonical) {
            return match name {
                "Record" => PropsParam::Undecided,
                _ => PropsParam::NotRecord(PropsRefusal::NotAnObject),
            };
        }
        if self.string_literal_union_values(&canonical).is_some()
            || self.required_variants(&canonical).is_some()
        {
            return PropsParam::NotRecord(PropsRefusal::NotAnObject);
        }
        // A declaration whose body is some other form, read through the body:
        // `type Name = string` is a second name for a primitive. A generic
        // declaration's body is read unsubstituted, which can only move the
        // answer to `Undecided`: a body that is its own parameter lowers to a
        // `Ty::Param`, and every other form is decided by its outermost
        // constructor, which the arguments do not change.
        let (base, _) = split_type_app(&canonical);
        if let Some(td) = self.local_type_decl(base) {
            let body = self.lowerer.lower(&td.body);
            return self.props_param(&body, depth + 1);
        }
        if let Ty::Imported { module, name } = base {
            if let Some((_, _, decl)) =
                imported_decl_chain_end(self.decl_ty_resolver, module.as_str(), name.as_ref())
            {
                return self.props_param(&decl.body, depth + 1);
            }
        }
        PropsParam::Undecided
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
    /// (non-spread) expression with a decidable type that joins with the rest;
    /// a spread element, an empty literal, or a disagreement the join cannot
    /// close yields `Unknown`, keeping the literal at `Array<Unknown>`.
    ///
    /// The join is `join_ty`'s, so `["read", "write"]` is an
    /// `Array<"read" | "write">` rather than the `Array<Unknown>` an equality
    /// test made of two different literal types (G237). That is what
    /// `Array<Mode>` is compared against, and the comparison is a subset test
    /// on the two literal sets.
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
                Some(prev) => match join_ty(prev, ty) {
                    Ty::Unknown => return Ty::Unknown,
                    joined => inferred = Some(joined),
                },
            }
        }
        inferred.unwrap_or(Ty::Unknown)
    }

    /// The structural record type a written object literal has (G232), or
    /// `Ty::Unknown` for a literal whose field set this walk cannot enumerate.
    ///
    /// One field per written `key: value`, in the order they are written, each
    /// carrying the type the walk already recorded for its value and none of
    /// them optional: a key written in a literal is a key the value has.
    ///
    /// A field whose value is itself undecidable keeps `Ty::Unknown` as its
    /// type rather than sinking the whole record. The record recursion in
    /// `definitely_incompatible` declines an undecidable field on its own, so
    /// a literal holding one still decides the fields around it and still
    /// answers the "is a field missing" question, which is the half that does
    /// not depend on any field's type.
    ///
    /// `Ty::Unknown` for a literal with a spread in it, for the reason stated
    /// at the call site: the spread's fields are not enumerable here, so a
    /// record built without them would be missing fields the value carries.
    ///
    /// Nothing about a `const` changes. An unannotated `const` lowers its
    /// declaration to `Ty::Unknown` on purpose (G39) and reads its type from
    /// the annotation or from nowhere; this types the *expression*, which is
    /// what the four value sites compare, and leaves what a binding infers to
    /// the rules that already decide it.
    fn object_literal_ty(&self, fields: &[ObjectField]) -> Ty {
        let mut out: Vec<RecordField> = Vec::with_capacity(fields.len());
        for f in fields {
            match f {
                ObjectField::KeyValue { key, value, .. } => out.push(RecordField {
                    name: key.clone(),
                    ty: self.tm.get(value.span()).clone(),
                    optional: false,
                }),
                ObjectField::Spread { .. } => return Ty::Unknown,
            }
        }
        Ty::Record { fields: out }
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
                    // G39. A `const` joins the two callable kinds here: its
                    // declaration lowers to its annotation, or to `Unknown`
                    // when it carries none, so reading it through the same
                    // resolver states exactly what the author wrote and
                    // nothing more.
                    SymbolKind::Function { decl_idx }
                    | SymbolKind::Component { decl_idx }
                    | SymbolKind::Const { decl_idx } => {
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
                        //
                        // A sibling's `pub const` is the third kind a named
                        // import can bind (G205); its annotation is read the
                        // same way, so the guarantee does not depend on
                        // whether the value was declared as a function or a
                        // constant.
                        let value = self
                            .decl_ty_resolver
                            .imported_fn_decl(&key, original.as_ref())
                            .or_else(|| {
                                self.decl_ty_resolver
                                    .imported_const_decl(&key, original.as_ref())
                            })
                            .or_else(|| self.stdlib_fn_ty(&key, original.as_ref()));
                        match value {
                            Some(ty) => ty,
                            // G217 across the module boundary. A named import
                            // binds one name to whatever the sibling declares,
                            // and when none of the three value kinds answered
                            // it may be a type. The declaration is one query
                            // away, and asking it is what keeps the diagnostic
                            // from depending on whether the record was
                            // declared here or imported.
                            None => {
                                if let Some(decl) = self
                                    .decl_ty_resolver
                                    .imported_type_decl(&key, original.as_ref())
                                {
                                    let construction = self.construction_of_ty(&decl.body);
                                    self.report_type_name_as_value(
                                        ref_span,
                                        original.as_ref(),
                                        construction,
                                    );
                                }
                                Ty::Unknown
                            }
                        }
                    }
                    // G217. The name is a type declared in this module (a
                    // `type` or an `interface`, which collect interns in the
                    // same namespace), standing where a value is wanted.
                    SymbolKind::Type { decl_idx } => {
                        let construction = self.construction_of_local_type(*decl_idx);
                        self.report_type_name_as_value(
                            ref_span,
                            sym.name.as_ref(),
                            construction,
                        );
                        Ty::Unknown
                    }
                    _ => Ty::Unknown,
                }
            }
            // Prelude values (`Ok`, `Err`, etc.) need use-site generic
            // instantiation — week-3 bidirectional checker work. A prelude
            // *type* is G217's error, the same as a declared one: `string` and
            // `Option` name types, and neither is a value.
            ResolvedRef::Prelude(id) => {
                if let Some(sym) = self.lowerer.prelude.table.get(id) {
                    if let SymbolKind::Prelude { kind } = sym.kind {
                        if let Some(sig) = crate::stdlib::prelude_fn_ty(kind) {
                            return sig;
                        }
                        if let Some(construction) = prelude_type_construction(kind) {
                            let name = sym.name.as_ref().to_string();
                            self.report_type_name_as_value(
                                ref_span,
                                &name,
                                construction.to_string(),
                            );
                        }
                    }
                }
                Ty::Unknown
            }
        }
    }

    /// Record a type's name standing where a value is wanted (G217, E0228).
    ///
    /// Silent for the one legal expression use of a type name, its own
    /// descriptor's receiver: `member_object` carries the span of the
    /// identifier the enclosing member access is reading through, so
    /// `Order.parse(json)` and `Order.is(v)` report nothing while `Order` on
    /// its own does.
    fn report_type_name_as_value(&mut self, span: Span, name: &str, construction: String) {
        if self.member_object == Some(span) {
            return;
        }
        self.errors.push(TypeError::TypeNameAsValue {
            name: name.to_string(),
            construction,
            span,
        });
    }

    /// How a value of the type declared at `decl_idx` is written, for E0228's
    /// message. An `interface` answers for itself; every other declaration
    /// answers from its lowered body, so one reading of "what shape is this"
    /// serves the local and the imported case alike.
    fn construction_of_local_type(&self, decl_idx: u32) -> String {
        match self.module.items.get(decl_idx as usize) {
            Some(Decl::Interface(_)) => {
                "an `interface` is a shape a record satisfies, not a value to construct".to_string()
            }
            Some(Decl::Type(td)) => self.construction_of_ty(&self.lowerer.lower(&td.body)),
            _ => GENERIC_CONSTRUCTION.to_string(),
        }
    }

    /// How a value of `ty` is written, in one clause. The record and the
    /// tagged union are the two an agent reaches for by name, so both list
    /// what they actually hold: the field names for the literal, the variants
    /// with their payload shapes for the constructor call. Anything the
    /// checker cannot see into (an `extern_ts` body, a `typeof`, an
    /// unresolved name) gets the general sentence rather than a guess.
    fn construction_of_ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Record { fields } if !fields.is_empty() => {
                let shape = fields
                    .iter()
                    .map(|f| format!("{}: ...", f.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("a value is the record literal `{{ {shape} }}`, with the type on the annotation")
            }
            Ty::Union { variants } if !variants.is_empty() => {
                let shape = variants
                    .iter()
                    .map(|v| match &v.payload {
                        Some(p) => format!("`{}({})`", v.name, ty_display(p)),
                        None => format!("`{}`", v.name),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("a value is one of its variants: {shape}")
            }
            Ty::StringLiteralUnion(values) if !values.is_empty() => {
                let shape = values
                    .iter()
                    .map(|v| format!("`\"{v}\"`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("a value is one of its literals: {shape}")
            }
            Ty::Prim(_) => format!(
                "a value is a `{}` literal, with the type on the annotation",
                ty_display(ty)
            ),
            Ty::Fn { .. } => {
                "a value is a function, with the type on the annotation".to_string()
            }
            _ => GENERIC_CONSTRUCTION.to_string(),
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
        if let Some(ResolvedRef::Prelude(id)) = self.resolved.resolutions.get(*span) {
            let sym = self.lowerer.prelude.table.get(id)?;
            let SymbolKind::Prelude { kind } = sym.kind else {
                return None;
            };
            return crate::stdlib::prelude_member_ty(self.lowerer.prelude, kind, field);
        }
        self.namespace_member_ty(*span, field)
    }

    /// The type of `field` read through the namespace import whose reference
    /// the resolver recorded at `base_span`: `ui.Button` under `import ui` or
    /// `import ui as u`, `http.get` under `import std/http`. `None` when the
    /// reference is not a namespace import or the module declares no such
    /// value.
    ///
    /// The member expression `ui.Button` and the JSX element `<ui.Button>`
    /// both read the member through this one function, and it asks the same
    /// declaration queries a named import does, so the component a
    /// diagnostic is about is the same declaration whichever import spelling
    /// reached it.
    fn namespace_member_ty(&self, base_span: Span, field: &str) -> Option<Ty> {
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(base_span)? else {
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
        // And the namespace spelling of G205: `l.ORIGIN` reaches a sibling's
        // `pub const` here, with the same fallback order as the by-name arm.
        self.decl_ty_resolver
            .imported_fn_decl(&key, field)
            .or_else(|| self.decl_ty_resolver.imported_const_decl(&key, field))
            .or_else(|| self.stdlib_fn_ty(&key, field))
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
        let Decl::Type(_) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        // `T.parse` is a member of the descriptor `T` names. Under D46 a second
        // name names the same descriptor, so `B.parse` with `type B = A` is
        // `A.parse` and its result is checked as an `A`; the emitter binds the
        // value alias that makes the call exist at run time.
        let named = Ty::Named {
            symbol: SymbolRef(id.0),
            path: vec![sym.name.clone()],
        };
        let td = self.local_type_decl(&named)?;
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
        // The parsed value is a `T`, which is the type the author wrote; under
        // the alias hop it compares equal to the declaration it names.
        let parsed = named;
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
            type_params: None,
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

    // ----- the stdlib signature tables (`crate::stdlib`) -----
    //
    // The tables themselves are free functions of the prelude: nothing in them
    // reads a program. These five wrappers keep the receiver spelling the rest
    // of this file uses.

    /// The `(module key, export name)` a call's callee names in the standard
    /// library, under either import spelling: `json.parse` through `import
    /// std/json` (or an alias of it) and `parse` through `import std/json {
    /// parse }`. `None` for anything else, a project sibling's function
    /// included, so a stdlib table is consulted only for a stdlib callee.
    fn stdlib_callee(&self, callee: &Expr) -> Option<(String, String)> {
        let (span, field) = match callee {
            Expr::Member {
                object,
                field,
                optional: false,
                ..
            } => match object.as_ref() {
                Expr::Ident { span, .. } => (*span, Some(field.to_string())),
                _ => return None,
            },
            Expr::Ident { span, .. } => (*span, None),
            _ => return None,
        };
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        let (path, name) = match (&sym.kind, field) {
            (
                SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. },
                Some(field),
            ) => (path, field),
            (SymbolKind::ImportNamed { path, original }, None) => (path, original.to_string()),
            _ => return None,
        };
        let key = path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        key.starts_with("std/").then_some((key, name))
    }

    /// The type of `e`, a name or a member read, once its position is
    /// known: unchanged in a callee's position, and for a stdlib function
    /// read as a value, the value's own type (record 88, 4a).
    ///
    /// `json.parse`'s `T` appears in no parameter. A direct call fills it from
    /// the written type argument, and the emitter makes that call validate
    /// its input. A value read off the function is the function itself, and a
    /// call through it is the casting `parse<T>`, so the value gets no type
    /// parameters (`Some([])`: a type argument on it is E0233) and `T` is
    /// `unknown`, the top type. `Ty::Unknown` there would let `let r:
    /// Result<User, _> = p(s)` pass, and `tsc` would then infer `T = User`
    /// from the annotation: a cast again.
    ///
    /// A parameter an argument carries (`json.parse_with`'s `T` rides on the
    /// schema) is still bound through the value, so only a function every one
    /// of whose explicit parameters is `unknown` when unwritten is narrowed.
    fn value_read_ty(&mut self, e: &Expr, ty: Ty) -> Ty {
        // A name bound to such a value carries where it came from, in a
        // callee's position too: that is where the E0233 is reported.
        if let Expr::Ident { span, .. } = e {
            if let Some(ResolvedRef::Local(def)) = self.resolved.resolutions.get(*span) {
                if let Some(f) = self.local_stdlib_values.get(&def).cloned() {
                    self.stdlib_values.insert(*span, f);
                }
                return ty;
            }
        }
        if self.call_callee == Some(e.span()) {
            return ty;
        }
        let Some((key, field)) = self.stdlib_callee(e) else {
            return ty;
        };
        let params = crate::stdlib::explicit_type_params(&key, &field);
        if params.is_empty()
            || params
                .iter()
                .any(|(_, u)| *u != crate::stdlib::Unwritten::UnknownTop)
        {
            return ty;
        }
        let Ty::Fn {
            params: fn_params,
            return_ty,
            is_async,
            ..
        } = &ty
        else {
            return ty;
        };
        let subst: HashMap<Ident, Ty> = params
            .iter()
            .map(|(name, unwritten)| (Ident::from(*name), unwritten.ty()))
            .collect();
        let value = Ty::Fn {
            params: fn_params
                .iter()
                .map(|p| FnParam {
                    ty: substitute_type_params(&p.ty, &subst),
                    ..p.clone()
                })
                .collect(),
            return_ty: Arc::new(substitute_type_params(return_ty, &subst)),
            is_async: *is_async,
            type_params: Some(Vec::new()),
        };
        self.stdlib_values.insert(e.span(), callee_source(e));
        value
    }

    /// The type parameter names, in declaration order, of the user `fn` or
    /// `component` a call's callee names: one declared in this module, or a
    /// project sibling's under either import spelling (`import lib { id }`
    /// and `lib.id` through `import lib`). `None` for anything else: a stdlib
    /// function, whose written arguments `crate::stdlib` binds; a lambda or a
    /// function-typed value, which has no declaration to name here, so its
    /// list is read off the value's `Ty::Fn` instead (`type_params`, where a
    /// declaration, a lambda and a written function type all carry it); and a
    /// name this compile cannot see.
    fn callee_generics(&self, callee: &Expr) -> Option<Vec<Ident>> {
        let (span, field) = match callee {
            Expr::Member {
                object,
                field,
                optional: false,
                ..
            } => match object.as_ref() {
                Expr::Ident { span, .. } => (*span, Some(field.to_string())),
                _ => return None,
            },
            Expr::Ident { span, .. } => (*span, None),
            _ => return None,
        };
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        let (path, name) = match (&sym.kind, field) {
            (SymbolKind::Function { decl_idx } | SymbolKind::Component { decl_idx }, None) => {
                return match self.module.items.get(*decl_idx as usize)? {
                    Decl::Fn(fd) => Some(fd.generics.iter().map(|g| g.name.clone()).collect()),
                    Decl::Component(cd) => {
                        Some(cd.generics.iter().map(|g| g.name.clone()).collect())
                    }
                    _ => None,
                };
            }
            (
                SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. },
                Some(field),
            ) => (path, field),
            (SymbolKind::ImportNamed { path, original }, None) => (path, original.to_string()),
            _ => return None,
        };
        let key = path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        if key.starts_with("std/") {
            return None;
        }
        self.decl_ty_resolver.imported_fn_generics(&key, &name)
    }

    fn stdlib_fn_ty(&self, module_key: &str, field: &str) -> Option<Ty> {
        crate::stdlib::fn_ty(self.lowerer.prelude, module_key, field)
    }

    fn stdlib_result_ty(&self, ok: Ty, err: Ty) -> Option<Ty> {
        crate::stdlib::result_ty(self.lowerer.prelude, ok, err)
    }

    fn stdlib_option_ty(&self, inner: Ty) -> Option<Ty> {
        crate::stdlib::option_ty(self.lowerer.prelude, inner)
    }

    #[allow(dead_code)]
    fn stdlib_nullable_ty(&self, inner: Ty) -> Option<Ty> {
        crate::stdlib::nullable_ty(self.lowerer.prelude, inner)
    }

    fn stdlib_array_ty(&self, inner: Ty) -> Option<Ty> {
        crate::stdlib::array_ty(self.lowerer.prelude, inner)
    }

    #[allow(dead_code)]
    fn stdlib_record_ty(&self, key: Ty, value: Ty) -> Option<Ty> {
        crate::stdlib::record_ty(self.lowerer.prelude, key, value)
    }


    // ----- day-15: `?` operator typing rule -----

    /// Build the `EnclosingReturn` for a declared return type: its
    /// `ReturnClass` (for the `?` rule) and its lowered `Ty` (for
    /// return-type mismatch checking). Both err toward *permissive* — a
    /// missing annotation (legal under D4) or one that can't be resolved
    /// yields `ReturnClass::Unknown` and `Ty::Unknown`, so neither check
    /// fires on a type it can't judge.
    /// The return context of a callable whose return type was not written
    /// but is known anyway: an unannotated `component` returns the prelude
    /// `Component`. Classified as `enclosing_return` classifies a written
    /// type that is not a `Result`.
    fn enclosing_return_ty(&self, ty: Ty, is_async: bool) -> EnclosingReturn {
        let class = if self.is_decidably_non_result(&ty) {
            ReturnClass::NonResult
        } else {
            ReturnClass::Unknown
        };
        EnclosingReturn { class, ty, is_async }
    }

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
        if let Some(r) = self.value_refusal(value, &found, &expected) {
            let accepted = self.accepted_values(&r.expected);
            self.errors.push(TypeError::TypeMismatch {
                expected: ty_display(&r.expected),
                found: r.found,
                accepted,
                missing_fields: r.missing_fields,
                no_common_field: r.no_common_field,
                span: r.span,
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
    fn string_literal_union_ref(&self, ty: &Ty) -> Option<DiagnosticDecl> {
        match ty {
            Ty::Imported { module, name } => Some(DiagnosticDecl::Imported {
                module: module.as_str().to_string(),
                name: name.to_string(),
            }),
            Ty::Named { symbol, .. } => {
                let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
                Some(DiagnosticDecl::Local {
                    name: sym.name.to_string(),
                })
            }
            _ => None,
        }
    }

    /// What a value of `ty` may be, when `ty` accepts a finite set the checker
    /// holds: a string-literal union's values, or a tagged union's variant
    /// names. `None` for every other type, whose accepted set is not
    /// enumerable, where an invented list would be a claim rather than a fact.
    ///
    /// This is what goes beside a mismatch as the alternatives. An agent told
    /// only "expected `OrderStatus`, found `string`" has to go read the
    /// declaration to write the repair; the checker resolved that declaration
    /// to decide the mismatch (G220).
    ///
    /// The two shapes are told apart by the `kind` on the answer, decided here
    /// where the checker knows which branch it took. On the wire they are both
    /// lists of bare names, so a consumer that has to write one of them cannot
    /// recover the difference from the names themselves.
    fn accepted_values(&self, ty: &Ty) -> Option<Accepted> {
        if let Some(values) = self.string_literal_union_values(ty) {
            return Some(Accepted::new(AlternativesKind::Literals, values));
        }
        if let Ty::Union { variants } = ty {
            return Some(Accepted::new(
                AlternativesKind::Variants,
                variants.iter().map(|v| v.name.to_string()).collect(),
            ));
        }
        self.required_variants(ty).map(|(_, vs)| {
            Accepted::new(
                AlternativesKind::Variants,
                vs.iter().map(|v| v.to_string()).collect(),
            )
        })
    }

    /// The record a field diagnostic is about, as a declaration to address.
    ///
    /// Read off the owner the field check already resolved, so the record an
    /// `E0210` names and the record the field-use relation keys a legal access
    /// to are one answer. A record declared in this file comes back `Local`,
    /// carrying no module: the module half of a declaration in the file being
    /// checked belongs to the surface, counted from the root that surface
    /// counts `entity` from, and the file's own `module` header is a second
    /// spelling whenever the two disagree (G172).
    ///
    /// `None` for a field set with no declaration behind it, which is absence
    /// of an address rather than a failure to look.
    fn field_owner_decl(&self, owner: &FieldOwner) -> Option<DiagnosticDecl> {
        match owner {
            FieldOwner::Declared { module, name } if *module == self.own_module_key() => {
                Some(DiagnosticDecl::Local { name: name.clone() })
            }
            FieldOwner::Declared { module, name } => Some(DiagnosticDecl::Imported {
                module: module.clone(),
                name: name.clone(),
            }),
            FieldOwner::Undeclared { .. } | FieldOwner::Unresolved { .. } => None,
        }
    }

    /// The declaration a record's fields came from, spelled for this site,
    /// when `written` reached it through a chain of second names (D46), for
    /// E0210's ``type `Page` (a second name for `a.Doc`)`` (G302). `None` when
    /// `written` is that declaration itself, or names nothing declared.
    fn second_name_target(&self, written: &Ty, owner: &FieldOwner) -> Option<String> {
        let FieldOwner::Declared { module, name } = owner else {
            return None;
        };
        let base = match written {
            Ty::App { base, .. } => base.as_ref(),
            other => other,
        };
        let (written_module, written_name) = match base {
            Ty::Imported { module, name } => (module.as_str().to_string(), name.to_string()),
            Ty::Named { symbol, path } => {
                // The same collision guard `direct_type_decl` holds: a prelude
                // id must not index an unrelated declaration of this module.
                let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
                if path.last() != Some(&sym.name) {
                    return None;
                }
                (self.own_module_key(), sym.name.to_string())
            }
            _ => return None,
        };
        if written_module == *module && written_name == *name {
            return None;
        }
        Some(self.scope_spelling(module, name))
    }

    /// How this module writes the declaration `name` of `module`: the bare
    /// name for one of its own or one it imports by name, `ns.Name` through a
    /// namespace or aliased import. A declaration the module has no name for
    /// is spelled through its module's last path segment, the spelling a
    /// namespace import of it would give.
    fn scope_spelling(&self, module: &str, name: &str) -> String {
        if module == self.own_module_key() {
            return name.to_string();
        }
        let table = &self.resolved.symbols.table;
        let symbols = (0..table.len() as u32).filter_map(|i| table.get(SymbolId(i)));
        let mut qualified = None;
        for sym in symbols {
            match &sym.kind {
                SymbolKind::ImportNamed { original, path }
                    if original.as_ref() == name
                        && crate::lower::module_key(path).as_str() == module =>
                {
                    return sym.name.to_string();
                }
                SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. }
                    if qualified.is_none() && crate::lower::module_key(path).as_str() == module =>
                {
                    qualified = Some(format!("{}.{name}", sym.name));
                }
                _ => {}
            }
        }
        qualified.unwrap_or_else(|| {
            let last = module.rsplit('/').next().unwrap_or(module);
            format!("{last}.{name}")
        })
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
            // The kind came from the arms, not from a type the checker knows,
            // and the scrutinee names a type declared somewhere this compile
            // cannot read (a sibling module that is not here, an npm package
            // whose types were never materialized). It may well be a bounded
            // literal union that these arms cover, so E0218's "a string match
            // can never be exhaustive, add an `else`" would be a false claim
            // whose advice switches the check off for good (G291). Coverage is
            // undecidable here, and the error says that instead.
            match self.imported_type_visibility(scrutinee_ty) {
                ImportedTypeVisibility::Unseen { module, name } => {
                    self.cover_unresolved_site(scrutinee_ty, scrutinee_span, match_span);
                    let has_catch_all = arms
                        .iter()
                        .any(|a| is_catch_all_pattern(&a.pattern, Scrutinee::Opaque));
                    if !has_catch_all {
                        self.errors.push(TypeError::UnseenMatchType {
                            type_name: name,
                            module,
                            span: match_span,
                        });
                    }
                    return;
                }
                // The import named something the module does not export,
                // which is E0105 at the import already. One error per mistake.
                ImportedTypeVisibility::NotExported => return,
                ImportedTypeVisibility::Visible => {}
            }
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
    /// Also emits `UnknownVariantPattern` for any head that names no variant of
    /// the union, bare (`Zed`) or constructor (`Zed(x)`). Such an arm used to be
    /// inserted into `covered` unexamined, so a misspelling reached `tsc` and
    /// came back as a raw `TS2678` instead of a Glyph diagnostic pointing at the
    /// arm. The bare spelling stayed silent one release longer (G169), which
    /// left the resolver's `unresolved name` as the only signal and sent the
    /// author looking for a missing import rather than a missing variant.
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
                // G169. A head naming no variant of this union is a typo,
                // whether it was written bare (`Zed`) or as a constructor
                // (`Zed(x)`). Both spellings report E0220 here, which is what
                // the module-local twin has always done: `is_catch_all_pattern`
                // ruled the bare form a variant reference rather than a
                // binding, so the only remaining reading is a variant that does
                // not exist.
                //
                // The bare form used to be inserted into `covered` and
                // otherwise ignored. That insert never changed the
                // exhaustiveness outcome (a name absent from `required` covers
                // nothing that could be missing), so dropping it moves no
                // E0200; what it moves is the silence. An author whose only
                // signal was the resolver's `unresolved name` went looking for
                // an import, and an author who had imported the name from
                // somewhere else got no signal at all. No mention is recorded
                // either way, because the arm named nothing this union
                // declares.
                ArmCoverage::UnknownVariant { name, span } => {
                    unknown.push((name.to_string(), span));
                }
                ArmCoverage::Declined { variant } => {
                    let variant = variant.map(|v| v.to_string());
                    self.cover_decline(cov.as_ref(), arm, variant.as_deref());
                }
            }
        }
        for (name, span) in unknown {
            let suggestion =
                uncovered_nearest(&name, required, |v| covered.contains(v.as_ref()));
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
                union: Some(DiagnosticDecl::Imported {
                    module: module.as_str().to_string(),
                    name: type_name.to_string(),
                }),
                missing_variants: missing.iter().map(|v| (*v).to_string()).collect(),
                variants: required.iter().map(|v| v.to_string()).collect(),
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

    /// Whether the checker can read the declaration behind a scrutinee that
    /// names a type from another module. `Visible` for everything that is not
    /// such a type, and for an imported type whose declaration it reads, so a
    /// visible declaration keeps its own exhaustiveness rules.
    fn imported_type_visibility(&self, ty: &Ty) -> ImportedTypeVisibility {
        let Ty::Imported { module, name } = union_base(ty) else {
            return ImportedTypeVisibility::Visible;
        };
        // The chain of second names is walked link by link rather than through
        // `imported_decl_chain_end`, because the link that breaks is the one
        // to name: `pub type ColType = Kind` in a visible sibling, over a
        // `Kind` imported from a package nobody materialized, is undecided at
        // the package, not at the sibling.
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let (mut module, mut name) = (module.as_str().to_string(), name.to_string());
        loop {
            if !seen.insert((module.clone(), name.clone())) {
                return ImportedTypeVisibility::Visible;
            }
            match declared_type_across_modules(self.decl_ty_resolver, &module, &name) {
                Some(decl) => match &decl.body {
                    Ty::Imported {
                        module: next_module,
                        name: next_name,
                    } => {
                        module = next_module.as_str().to_string();
                        name = next_name.to_string();
                    }
                    _ => return ImportedTypeVisibility::Visible,
                },
                None => {
                    return match self.decl_ty_resolver.module_exports_name(&module, &name) {
                        None => ImportedTypeVisibility::Unseen { module, name },
                        Some(false) => ImportedTypeVisibility::NotExported,
                        // Exported, and not as a `type` this checker reads (an
                        // interface, a function named in type position): the
                        // declaration is in sight, so the value-domain rules
                        // apply as they always did.
                        Some(true) => ImportedTypeVisibility::Visible,
                    };
                }
            }
        }
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
        // G305. A literal the union does not hold can never reach this match,
        // so its arm is dead code, and `tsc` refuses it (TS2678). Checked
        // before the catch-all return: an `else` absorbs the values the arms
        // leave out, and says nothing about a value that is not there.
        for arm in arms {
            if let Pattern::Literal {
                value: LiteralPattern::String(s),
                span,
            } = &arm.pattern
            {
                if !values.iter().any(|v| v == s) {
                    // Only a value no arm already names: writing one that is
                    // named would give the match two arms for it (E0305).
                    let named = |v: &&String| {
                        arms.iter().any(|a| {
                            matches!(&a.pattern, Pattern::Literal {
                                value: LiteralPattern::String(o),
                                ..
                            } if o == *v)
                        })
                    };
                    self.errors.push(TypeError::UnknownLiteralPattern {
                        union: ty_display(scrutinee_ty),
                        value: s.clone(),
                        suggestion: nearest_name(
                            s,
                            values.iter().filter(|v| !named(v)).map(String::as_str),
                        ),
                        span: *span,
                    });
                }
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
                variants: values.to_vec(),
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
    /// its type arguments. The free function `prelude_app` is the one
    /// collision-guarded detector; this is its method form over the
    /// Assigner's prelude, shared by `is_prelude_array` and `prelude_union`.
    fn prelude_app<'a>(&self, ty: &'a Ty, name: &str) -> Option<&'a [Ty]> {
        prelude_app(self.lowerer.prelude, ty, name)
    }

    /// The prelude container `ty` applies, when it is one whose runtime value
    /// is never a `string`, a `number` or a `bool` (G216). The name is returned
    /// beside the arguments because the reverse direction has to tell
    /// `Nullable` from the other four.
    ///
    /// Five names, and the claim is about each one's emitted value.
    /// `Option<T>` and `Result<T, E>` emit as tagged objects, `Array<T>` as a
    /// JavaScript array, `Record<K, V>` as an object, so none of them is a
    /// scalar whatever their arguments are. `Nullable<T>` emits as `T | null`
    /// (D45), and `null` is not a `string`, a `number` or a `bool`, so the
    /// union as a whole is not one either however `T` is filled in. That last
    /// one is the case `tsc` cannot make: the emitted `const v: number | null =
    /// 3` narrows to `number` under TypeScript's control flow, so a later
    /// `takes_int(v)` type-checks there and this relation is the only place it
    /// can be refused.
    ///
    /// What is deliberately left out. `Schema<T>`, `Component<P>` and `Issue`
    /// are interop surface types whose shape comes from a library rather than
    /// from the emitter, and `infer_output<S>` (D28) is a type-level operator
    /// whose result is whatever the schema yields, a `string` included. A
    /// user's own generic (`type Box<T> = T`) is out for the same reason: its
    /// body may be its argument, so deciding it needs the declaration read and
    /// the parameter substituted, which is a different rule.
    fn prelude_container<'a>(&self, ty: &'a Ty) -> Option<(&'static str, &'a [Ty])> {
        prelude_container(self.lowerer.prelude, ty)
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
        let mut unknown: Vec<(String, glyph_ast::Span)> = Vec::new();
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
                // The suggestion is drawn once every arm is read, below.
                ArmCoverage::UnknownVariant { name, span } => {
                    unknown.push((name.to_string(), span));
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
        // Emitted before the catch-all return: an `else` absorbs the variants
        // the arms leave out and says nothing about a head that names none.
        for (name, span) in unknown {
            self.errors.push(TypeError::UnknownVariantPattern {
                union: type_name.clone(),
                suggestion: uncovered_nearest(&name, &variants, |v| covered.contains(v)),
                name,
                span,
            });
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
            union: Some(DiagnosticDecl::from(&union)),
            missing_variants: missing.iter().map(|n| n.to_string()).collect(),
            variants: variants.iter().map(|v| v.to_string()).collect(),
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

    /// If `ty` is a module-local tagged-union `type X = | A | B | ...`, or a
    /// second name for one (D46), return that declaration together with the
    /// type arguments it was applied to. The shared resolution chain behind
    /// `named_union_variants` and `union_variant_payload`.
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
    /// The declaration is read through `local_type_decl`, so it carries the
    /// prelude/module symbol-id collision guard `direct_type_decl` holds once
    /// (a prelude `Result<T, E>` arrives as an application over a `Ty::Named`
    /// whose sentinel symbol id could otherwise index an unrelated
    /// module-local union, and `variant_payload` consults this function
    /// *before* the prelude branch, so a collision would shadow the right
    /// answer) and follows a second name to the union it names: `type Cmd =
    /// Command` reads `Command`'s variants, as `named_record_fields` reads a
    /// record's fields through the same chain. Before the hop, a match over
    /// the alias had no variant set and was silently unchecked (G224).
    fn resolve_named_union<'t>(
        &self,
        ty: &'t Ty,
    ) -> Option<(&glyph_ast::TypeDecl, &'t [Ty])> {
        let (_base, args) = split_type_app(ty);
        let td = self.local_type_decl(ty)?;
        matches!(&td.body, TypeExpr::Union { .. }).then_some((td, args))
    }

    /// The module-local `type` declaration `ty` names, or `None` when this file
    /// cannot see one: a structural type, a generic parameter, a type from
    /// another module, a prelude or stdlib name, or a symbol whose declaration
    /// index does not land on a `type`.
    ///
    /// A generic application resolves through its base, so `Tree<K>` answers
    /// with the same declaration `Tree` does. The resolved symbol's name has to
    /// match the type's lexical path: the prelude and the stdlib mint symbol
    /// ids outside the module table's dense range, and without the check one of
    /// those could index an unrelated module-local declaration and answer for
    /// it.
    ///
    /// A second name is followed to the declaration it names (D46). `type B =
    /// A` declares no type of its own, so asking for `B`'s declaration answers
    /// `A`'s, through any length of chain, and the body that comes back is the
    /// one the chain ends at. What counts as a second name is decided in
    /// `alias_target`; a body of any other form is the declaration's own and
    /// comes back exactly as written, which is what lets a caller distinguish
    /// "declared as a record" from "declared as a generic application that
    /// might be anything".
    fn local_type_decl(&self, ty: &Ty) -> Option<&glyph_ast::TypeDecl> {
        let (base, _args) = split_type_app(ty);
        let canonical = self.resolve_alias_chain(base);
        self.direct_type_decl(&canonical)
    }

    /// `local_type_decl` without the alias hop: the declaration `ty` names as
    /// written, or `None`. The one place the symbol table is read for a
    /// module-local type, so the collision guard lives here once.
    fn direct_type_decl(&self, ty: &Ty) -> Option<&glyph_ast::TypeDecl> {
        direct_type_decl(self.module, self.resolved, ty)
    }

    /// Follow `ty` through second names to the declaration the chain ends at,
    /// answering the `Ty::Named` of that declaration; anything that is not a
    /// module-local alias answers unchanged. The resolver accepts `type A = B`
    /// beside `type B = A`, so a cycle is guarded, and answers the type as
    /// written: a name that names nothing resolves to nothing further.
    fn resolve_alias_chain(&self, ty: &Ty) -> Ty {
        resolve_alias_chain(self.module, self.resolved, &self.lowerer, ty)
    }

    /// `resolve_alias_chain` applied to every name inside `ty`, so two types
    /// that differ only in which name they use for a declaration compare
    /// equal wherever they are compared: at the top, inside a generic
    /// application's arguments, in a record's fields, in a function's
    /// parameters and return, in a union's payloads.
    fn canonicalize_aliases(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Named { .. } => self.resolve_alias_chain(ty),
            Ty::App { base, args } => Ty::App {
                base: Arc::new(self.canonicalize_aliases(base)),
                args: args.iter().map(|a| self.canonicalize_aliases(a)).collect(),
            },
            Ty::Fn {
                params,
                return_ty,
                is_async,
                type_params,
            } => Ty::Fn {
                params: params
                    .iter()
                    .map(|p| FnParam {
                        name: p.name.clone(),
                        owned: p.owned,
                        ty: self.canonicalize_aliases(&p.ty),
                        optional: p.optional,
                    })
                    .collect(),
                return_ty: Arc::new(self.canonicalize_aliases(return_ty)),
                is_async: *is_async,
                type_params: type_params.clone(),
            },
            Ty::Record { fields } => Ty::Record {
                fields: fields
                    .iter()
                    .map(|f| RecordField {
                        name: f.name.clone(),
                        ty: self.canonicalize_aliases(&f.ty),
                        optional: f.optional,
                    })
                    .collect(),
            },
            Ty::Union { variants } => Ty::Union {
                variants: variants
                    .iter()
                    .map(|v| UnionVariant {
                        name: v.name.clone(),
                        payload: v.payload.as_ref().map(|p| self.canonicalize_aliases(p)),
                    })
                    .collect(),
            },
            _ => ty.clone(),
        }
    }

    /// Whether `ty` is a name for a declaration in this file whose body is a
    /// tagged union (D8) or a record. G201: the one thing about a named type
    /// the checker can decide against a primitive.
    ///
    /// Both shapes emit as a JavaScript object (`| Settled` becomes
    /// `{ tag: "Settled" }`), so a value of one is never a `string`, a `number`
    /// or a `bool`, whatever the declaration holds. That is the whole claim,
    /// and it is why the body has to be read rather than assumed. What the
    /// reading keeps out:
    ///
    /// - a body that is a path (`type UserId = string`) is a name for whatever
    ///   it points at, and following it is a separate question;
    /// - a string-literal union (`type Tier = "free" | "pro"`, D30) is a
    ///   `string`, at run time and under Glyph's own assignability rule, so it
    ///   has to stay silent even though it is spelled with `|`;
    /// - an `extern_ts` or `typeof` body is opaque to this checker by design;
    /// - an `interface`, which is not a `Decl::Type` at all, and whose empty
    ///   form TypeScript does let a `string` satisfy.
    ///
    /// The judgement is the declaration's, not the value's, so an application
    /// of a generic union (`Tree<number>`) answers the same as the bare name.
    fn is_declared_union_or_record(&self, ty: &Ty) -> bool {
        self.declared_union_or_record_body(ty).is_some()
    }

    /// The declaration body behind `is_declared_union_or_record`, for a caller
    /// that has to read one more thing off it: the reverse direction of G201
    /// needs the record's field list to apply its empty-record exclusion,
    /// while the forward direction has no exclusion and only needs the bool.
    /// Both directions read the body through this one place, so what counts
    /// as "declared union or record" is decided once.
    fn declared_union_or_record_body(&self, ty: &Ty) -> Option<&TypeExpr> {
        let td = self.local_type_decl(ty)?;
        matches!(&td.body, TypeExpr::Union { .. } | TypeExpr::Record { .. }).then_some(&td.body)
    }

    /// Whether `ty` resolves all the way to a module-local type declaration
    /// that is concretely *not* a tagged union (most often a record). Reads the
    /// declaration through `local_type_decl`, but answers the opposite question
    /// to `resolve_named_union`: not "is this a union", but "does the compiler
    /// know for certain it has no variants at all", as distinct from a type
    /// it simply cannot see into (imported, or unresolved). G146's nested
    /// exhaustiveness recursion needs exactly that distinction: a payload the
    /// compiler cannot resolve must stay silently unchecked (it might be a
    /// real union the resolver just can't reach across a module boundary),
    /// while a payload that resolves right down to a record declaration is
    /// provably not a union and a variant-shaped pattern against it can be
    /// flagged on the spot.
    fn resolves_to_non_union_decl(&self, ty: &Ty) -> bool {
        let Some(td) = self.local_type_decl(ty) else {
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
        // A second name is the declaration it names (D46), however its body
        // is spelled: `type P = Mode`, `type P = a.Mode`, `type P =
        // http.RedirectPolicy`. Reading only the name's own body answered
        // `None` for every one of them, so a match over `P` was held to an
        // unbounded `string` while a match over `Mode` was held to its values.
        let end = self.resolve_alias_chain(ty);
        let ty = &end;
        if let Ty::StringLiteralUnion(values) = ty {
            return Some(values.clone());
        }
        // Every string-literal union declared in another module, whichever
        // spelling brought it here: the annotation `Mode` under a named
        // import, `modes.Mode` under a namespace one, an alias of either, and
        // the type of a field read off an imported record. All four lower to
        // a `Ty::Imported` (G233), so the literal set is read from the
        // declaration here rather than baked into the type, and the type keeps
        // the name its author gave it everywhere a diagnostic prints one.
        if let Ty::Imported { module, name } = ty {
            return imported_string_literal_union_values(
                self.decl_ty_resolver,
                module.as_str(),
                name.as_ref(),
            );
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

    /// The built-in receiver `ty` is, when its member reads can be checked
    /// against a fixed set (G288): an `Array<T>`, a `string`, a string-literal
    /// union or a `number`, written directly or reached through aliases in
    /// this module or a sibling, generic ones included (`type Rows =
    /// Array<Row>`, `type Name = string where value.length > 0`, `type Box<T>
    /// = Array<T>`).
    ///
    /// Anything else answers `None` and the read stays as permissive as it
    /// was: a generic parameter, an interop value, a map, a type whose body
    /// this cannot read. Only the prelude `Array` counts, so a user type that
    /// happens to be called `Array` is not mistaken for it.
    ///
    /// The array's element type comes back beside the receiver: what a
    /// generated method row's `Elem` stands for (G312). `Unknown` for a
    /// string or a number, whose rows never name one, and for an `Array`
    /// written with no argument.
    fn builtin_receiver_elem(&self, ty: &Ty) -> Option<(BuiltinReceiver, Ty)> {
        let mut current = ty.clone();
        // Each step goes one alias further, and a chain can cross modules,
        // where the per-module cycle guard cannot see it; the bound stops a
        // cycle there.
        for _ in 0..16 {
            if let Some(args) = self.prelude_app(&current, "Array") {
                let elem = args.first().cloned().unwrap_or(Ty::Unknown);
                return Some((BuiltinReceiver::Array, elem));
            }
            current = match &current {
                Ty::Prim(Primitive::String) | Ty::StringLiteralUnion(_) => {
                    return Some((BuiltinReceiver::String, Ty::Unknown))
                }
                Ty::Prim(Primitive::Number) => return Some((BuiltinReceiver::Number, Ty::Unknown)),
                Ty::Named { .. } | Ty::Imported { .. } => self.alias_body(&current, &[])?,
                Ty::App { base, args } => self.alias_body(base, args)?,
                _ => return None,
            };
        }
        None
    }

    /// The body of the alias `base` names, with its generic parameters
    /// replaced by `args`: `type Box<T> = Array<T>` applied to `<string>` is
    /// `Array<string>`. Follows this module's chain of second names first, so
    /// `type Local = lib.Rows` answers the sibling's declaration. `None` for a
    /// name with no readable declaration, or an application whose argument
    /// count does not match the declaration's.
    fn alias_body(&self, base: &Ty, args: &[Ty]) -> Option<Ty> {
        let end = self.resolve_alias_chain(base);
        let (generics, body): (Vec<Ident>, Ty) = match &end {
            Ty::Named { .. } => {
                let td = self.direct_type_decl(&end)?;
                (
                    td.generics.iter().map(|g| g.name.clone()).collect(),
                    self.lowerer.lower(&td.body),
                )
            }
            Ty::Imported { module, name } => {
                let decl = self.imported_type_body(module.as_str(), name)?;
                (decl.generics, decl.body)
            }
            _ => return Some(end),
        };
        if generics.len() != args.len() {
            return None;
        }
        let subst: HashMap<Ident, Ty> = generics.into_iter().zip(args.iter().cloned()).collect();
        Some(substitute_type_params(&body, &subst))
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
            //
            // A local second name whose chain leaves the module (`type Page =
            // Doc` or `type Page = a.Doc`, `Doc` a sibling's record) has the
            // sibling's field set, read exactly as the import itself is (D46).
            // Answering only from this module's declarations left `p.titel`
            // on a `p: Page` unchecked while `d.titel` on a `d: Doc` was E0210.
            Ty::Named { .. } => match stdlib_type_fields(self, ty) {
                Some(fields) => Some(undeclared(fields)),
                None => match self.resolve_alias_chain(ty) {
                    Ty::Imported { module, name } => {
                        self.imported_record_shape(module.as_str(), &name, &[])
                    }
                    _ => self
                        .named_record_shape(ty, &[])
                        .or_else(|| self.interface_member_shape(ty)),
                },
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
    ///
    /// The owner is the record the alias chain ends at, not the name the use
    /// site reached it by, which is the rule `imported_record_shape` already
    /// applies across a module boundary: `type B = A` declares no field of its
    /// own, so a site reading `b.x` through it is a site over `A.x`, and
    /// keying it under `B` would put it in the impact set of a rename that
    /// cannot touch it.
    fn named_record_shape(&self, ty: &Ty, args: &[Ty]) -> Option<RecordShape> {
        let fields = self.named_record_fields(ty, args)?;
        let td = self.local_type_decl(ty)?;
        Some(RecordShape {
            owner: FieldOwner::Declared {
                module: self.own_module_key(),
                name: td.name.to_string(),
            },
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
        let (owner_module, owner_name, _) =
            imported_decl_chain_end(self.decl_ty_resolver, module, name)?;
        let fields = self.imported_record_fields(&owner_module, &owner_name, args)?;
        Some(RecordShape {
            owner: FieldOwner::Declared {
                module: owner_module,
                name: owner_name,
            },
            fields,
        })
    }

    /// The declaration an imported name reaches, when that declaration is a
    /// tagged union or a record: where it is declared and its body, or `None`.
    ///
    /// The cross-module counterpart of `declared_union_or_record_body` for
    /// the assignability relation (G215, G226). The chain of second names is
    /// followed to its end, so `pub type Rows = Sheet` in the sibling is a hop
    /// exactly as a local `type Local = Shape` is, and the identity answered
    /// is the pair the chain ends at: two spellings of one declaration answer
    /// the same pair and are one type. What is kept out is what the local
    /// reading keeps out: a body that is a primitive (`pub type Id = string`),
    /// a string-literal union, a generic application, or an `extern_ts` or
    /// `typeof` body (the export view lowers both to `Unknown`), and an
    /// `interface`, which `exported_type` does not answer at all. None of
    /// those is a declaration this checker can decide a pairing from, so each
    /// answers `None`, which every caller reads as undetermined.
    fn imported_union_or_record(&self, ty: &Ty) -> Option<ImportedUnionOrRecord> {
        let Ty::Imported { module, name } = ty else {
            return None;
        };
        let (module, name, decl) =
            imported_decl_chain_end(self.decl_ty_resolver, module.as_str(), name.as_ref())?;
        matches!(decl.body, Ty::Union { .. } | Ty::Record { .. }).then(|| ImportedUnionOrRecord {
            module,
            name,
            body: decl.body,
            open: decl.open,
        })
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
        let decl = self.imported_type_body(module, name)?;
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

    /// The sibling's lowered `type` declaration at the end of the chain
    /// `(module, name)` starts: `imported_decl_chain_end` without the address.
    fn imported_type_body(&self, module: &str, name: &str) -> Option<ImportedTypeDecl> {
        imported_decl_chain_end(self.decl_ty_resolver, module, name).map(|(_, _, decl)| decl)
    }

    /// The field set of a `Ty::Named` record declaration, with any generic
    /// parameters substituted by `args`. Reads the declaration through
    /// `local_type_decl`, so it carries the collision guard and follows a
    /// second name to the record it names (D46): a `b: B` with `type B = A`
    /// has `A`'s fields, as it already did when both were imported.
    fn named_record_fields(&self, ty: &Ty, args: &[Ty]) -> Option<Vec<RecordField>> {
        let td = self.local_type_decl(ty)?;
        self.record_decl_fields(td, args)
    }

    /// The field set of the record declaration `td`, with its generic
    /// parameters substituted by `args`. `None` for a body that is not a
    /// record.
    fn record_decl_fields(&self, td: &glyph_ast::TypeDecl, args: &[Ty]) -> Option<Vec<RecordField>> {
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
                            // An interface method cannot declare `<...>`.
                            type_params: Some(Vec::new()),
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

    /// True when `found` is provably not assignable to `expected`. The whole
    /// assignability relation as the checker actually applies it: the parts
    /// that need to read a declaration are decided here, and everything that
    /// can be judged from the two `Ty` values alone goes to
    /// `definitely_incompatible`.
    ///
    /// Two things need a declaration. An interface used as an ordinary
    /// parameter or return type is satisfied by any value carrying its members,
    /// so it is matched by member shape rather than by the nominal
    /// `Named`-vs-`Named` name check `definitely_incompatible` applies to
    /// `type` aliases (D49); the recursion also reaches an interface nested one
    /// level inside a generic application (`Array<Iface>`). And a declared
    /// union or record is never a `string`, a `number` or a `bool` (G201), nor
    /// is one of those three ever a declared union or record (the reverse
    /// direction, with an empty record excluded), which is the one pairing of
    /// a named type with a primitive the compiler can settle. Everything else
    /// delegates unchanged.
    /// The whole relation as the four sites that check a written value apply
    /// it, and the name the refusal gives the value: `assign_incompatible` on
    /// the two types, plus the two rules that need the expression rather than
    /// its type (G230). `None` is "not refused".
    ///
    /// The `let`, the `const`, the `return` and the call argument all reach
    /// here and nothing else does, so those two rules are stated once.
    /// `assign_incompatible` stays the type-to-type relation: it is what the
    /// recursion through records, applications and containers asks, and what
    /// [`assignability`] answers for a surface holding two `Ty` values and no
    /// program text.
    fn value_refusal(&self, value: &Expr, found: &Ty, expected: &Ty) -> Option<Refusal> {
        // A written array or object literal is checked element by element and
        // field by field first, so the refusal names the one that is wrong and
        // underlines it rather than the whole literal (G237).
        if let Some(inner) = self.written_literal_refusal(value, expected) {
            return Some(inner);
        }
        if self.assign_incompatible(found, expected) {
            let mut r = self.refusal(expected, self.found_display(found, expected), value.span());
            r.missing_fields = self.missing_required_fields(found, expected);
            r.no_common_field = self.shares_no_field_with_weak_record(found, expected);
            return Some(r);
        }
        // The walk records `Ty::Unknown` for an object literal, so the type
        // would name the value nothing. The written form is what the reader
        // sees, and it is what the rule read.
        if self.object_literal_outside_union(value, expected) {
            return Some(self.refusal(expected, "record".to_string(), value.span()));
        }
        None
    }

    fn refusal(&self, expected: &Ty, found: String, span: Span) -> Refusal {
        Refusal {
            expected: expected.clone(),
            found,
            missing_fields: Vec::new(),
            no_common_field: false,
            span,
        }
    }

    /// The required fields of `expected`'s record that the structural record
    /// `found` does not have, in `expected`'s declaration order.
    ///
    /// Read from the same field set the relation decided the refusal from:
    /// `record_fields_of`, which answers for a local record, a second name for
    /// one, an imported record, a generic application of one, a structural
    /// interface and a stdlib type the runtime ships. A `Nullable<T>` is read
    /// through to `T`, the descent `prelude_container_incompatible` makes for
    /// a record. "Required" is `RecordField::optional` being false, which is
    /// exactly the fields written without `?`: the only ones TypeScript
    /// requires (`Option<T>` and `Nullable<T>` fields are required there too,
    /// and `tsc --strict` reports each as missing when it is left out).
    ///
    /// Empty for any other pairing, including a value whose type is a named
    /// record: that pairing is refused by name (D49), and a list of fields
    /// would describe a reason the relation did not use.
    fn missing_required_fields(&self, found: &Ty, expected: &Ty) -> Vec<String> {
        let Ty::Record { fields: have } = found else {
            return Vec::new();
        };
        let canonical = self.canonicalize_aliases(expected);
        if let Some(("Nullable", args)) = self.prelude_container(&canonical) {
            return args
                .first()
                .map(|inner| self.missing_required_fields(found, inner))
                .unwrap_or_default();
        }
        let Some(declared) = self.record_fields_of(&canonical) else {
            return Vec::new();
        };
        declared
            .iter()
            .filter(|d| !d.optional && !have.iter().any(|f| f.name == d.name))
            .map(|d| d.name.to_string())
            .collect()
    }

    /// Whether `found` against `expected` is refused by the weak-type rule
    /// alone (`weak_record_disjoint`), read through the same field set and the
    /// same `Nullable<T>` descent `missing_required_fields` reads, so the
    /// diagnostic names the reason the relation used.
    fn shares_no_field_with_weak_record(&self, found: &Ty, expected: &Ty) -> bool {
        let canonical = self.canonicalize_aliases(expected);
        if let Some(("Nullable", args)) = self.prelude_container(&canonical) {
            return args
                .first()
                .is_some_and(|inner| self.shares_no_field_with_weak_record(found, inner));
        }
        self.record_fields_of(&canonical)
            .is_some_and(|declared| weak_record_disjoint(found, &declared))
    }

    /// The name a refused value goes on the diagnostic under.
    ///
    /// A fresh literal type is printed as the literal where the declared type
    /// is a string-literal union, and as `string` everywhere else. That is
    /// TypeScript's own rule for the same message: `let n: number = "hi"` is
    /// *"Type 'string' is not assignable to type 'number'"* there, while
    /// `let m: Mode = "nope"` is *"Type '\"nope\"' is not assignable to type
    /// 'Mode'"*. The literal is the useful half of the sentence exactly when
    /// the reader has to compare it against a set.
    fn found_display(&self, read: &Ty, expected: &Ty) -> String {
        if self.declares_string_literal_union(expected) {
            ty_display(read)
        } else {
            ty_display(&widen_literals(read))
        }
    }

    /// The refusal a written array or object literal draws from the declared
    /// type's own element or field type, or `None` when nothing inside it is
    /// refused.
    ///
    /// The value rule applied one level in, and then recursively, which is how
    /// a literal keeps its type through a container: `["read", "nope"]`
    /// against an `Array<Mode>` is the element `"nope"` against `Mode`, and
    /// `[{ mode: "nope" }]` against an `Array<Cfg>` is that same pairing two
    /// levels down. The span is the element's or the field value's, so the
    /// diagnostic underlines the literal the reader has to change.
    ///
    /// Only where the declared type says what the element or field should be.
    /// An object literal field the declared record does not declare is left
    /// alone: TypeScript refuses an excess property on a fresh object literal
    /// and Glyph does not, and widening this rule to cover that would be a
    /// separate decision about a shape with no string-literal union in it.
    fn written_literal_refusal(&self, value: &Expr, expected: &Ty) -> Option<Refusal> {
        let canonical = self.canonicalize_aliases(expected);
        match value {
            Expr::Array { elements, .. } => {
                let (_, args) = self.prelude_container(&canonical)?;
                let elem = args.first()?.clone();
                elements.iter().find_map(|el| {
                    let ArrayElem::Expr(e) = el else { return None };
                    self.value_refusal(e, &self.tm.get(e.span()).clone(), &elem)
                })
            }
            Expr::Object { fields, .. } => {
                let declared = self.record_fields_of(&canonical)?;
                fields.iter().find_map(|f| {
                    let ObjectField::KeyValue { key, value, .. } = f else {
                        return None;
                    };
                    let declared = declared.iter().find(|d| d.name == *key)?;
                    self.value_refusal(value, &self.tm.get(value.span()).clone(), &declared.ty)
                })
            }
            _ => None,
        }
    }

    /// The type a binding takes from a value written for it, with a *fresh*
    /// literal type widened to `string` (G237).
    ///
    /// TypeScript's rule, and Glyph's for the same reason: a `let` emits a
    /// TypeScript `let`, which widens, so `let l = "read"` is a `string` in
    /// the program that actually runs and `takes_mode(l)` is `TS2345` there.
    /// A binding that kept the one-literal type would accept a call `tsc`
    /// refuses, which is the one direction this checker may not take.
    ///
    /// Fresh is decided from the written form, not from the type. A literal
    /// type reaching a binding through a name (`fn f(m: "read" | "write") {
    /// let m2 = m }`) is not fresh, and TypeScript does not widen it either:
    /// `m2` is a `"read" | "write"` there and `takes_mode(m2)` compiles. The
    /// test is deliberately narrow, because the two ways of being wrong are
    /// not symmetric. Reading a fresh value as not fresh leaves a pairing
    /// undetermined; reading a name as fresh refuses a program `tsc` accepts.
    ///
    /// Three positions take a type from a value this way: a `let` with no
    /// annotation, and the element binding of a `for` loop over a written
    /// array. A `const` with no annotation lowers to `Unknown` and reaches
    /// nothing here, which is the right answer for it: a `const` emits a
    /// TypeScript `const`, which keeps the literal, so widening it would
    /// refuse `takes_mode(CM)` where `tsc` accepts it.
    /// An object literal is read field by field, because TypeScript widens a
    /// property whose value is a fresh literal and leaves one that arrived
    /// through a name alone: `let c = { mode: "read", from: m }` is
    /// `{ mode: string, from: Mode }` there, and a binding that kept `mode`
    /// narrow would accept a call `tsc` refuses. Only where the written form
    /// and the synthesized record line up, which for a spread-free literal
    /// they always do (G232).
    fn widen_fresh_literals(&self, value: &Expr, ty: Ty) -> Ty {
        if let (Expr::Object { fields, .. }, Ty::Record { fields: recorded }) = (value, &ty) {
            if fields.len() == recorded.len() {
                return Ty::Record {
                    fields: fields
                        .iter()
                        .zip(recorded.iter())
                        .map(|(written, field)| {
                            let ObjectField::KeyValue { value, .. } = written else {
                                return field.clone();
                            };
                            RecordField {
                                name: field.name.clone(),
                                ty: self.widen_fresh_literals(value, field.ty.clone()),
                                optional: field.optional,
                            }
                        })
                        .collect(),
                };
            }
        }
        if produces_fresh_literal(value) {
            widen_literals(&ty)
        } else {
            ty
        }
    }

    /// Whether the relation will reach a string-literal union on its way into
    /// `expected`: the declared type itself, or the `T` of a `Nullable<T>`,
    /// which is the one container `prelude_container_incompatible` descends
    /// with the value's type intact (G216).
    ///
    /// The condition on `written_literal_refusal` reading a literal, so `let m:
    /// Nullable<Mode> = "read"` is read the way `let m: Mode = "read"` is. Any
    /// other container refuses a string outright, and there is nothing to
    /// gain by naming the literal in that refusal.
    fn declares_string_literal_union(&self, expected: &Ty) -> bool {
        let canonical = self.canonicalize_aliases(expected);
        if self.string_literal_union_values(&canonical).is_some() {
            return true;
        }
        match self.prelude_container(&canonical) {
            Some(("Nullable", args)) => args
                .first()
                .is_some_and(|inner| self.declares_string_literal_union(inner)),
            _ => false,
        }
    }

    /// An object literal written where a string-literal union is declared
    /// (G230).
    ///
    /// Nothing synthesizes a record type for `{ x: 1 }` — the walk records
    /// `Ty::Unknown` for it — so the record rule the relation already applies
    /// to a record-typed value never sees an object literal, and `let d: Mode
    /// = { x: 1 }` drew nothing. The written `{ ... }` emits a JavaScript
    /// object whatever it holds, so it is none of the union's literals.
    ///
    /// Stated against a string-literal union and nothing else, deliberately.
    /// An object literal is not a `string`, a `number` or a `bool` either, and
    /// `let g: string = { x: 1 }` is silent for the same missing type, but
    /// that is the object literal's own gap rather than this pairing, and
    /// widening the rule here would start refusing programs with no
    /// string-literal union in them.
    fn object_literal_outside_union(&self, value: &Expr, expected: &Ty) -> bool {
        matches!(value, Expr::Object { .. }) && self.declares_string_literal_union(expected)
    }

    /// The pairings involving a string-literal union the relation decides, on
    /// canonical types (G230). `None` is "no arm here read this pairing", and
    /// the rest of the relation goes on to decide it.
    ///
    /// A string-literal union is a `string` at run time and a narrowing of
    /// `string` in the type, and both halves of that are what the rule says.
    /// Nothing here re-derives which values are records, which are containers
    /// and which live in another module: each direction asks the relation the
    /// corresponding `string` question and inherits every rule and every
    /// exclusion the `string` pairing already carries, `Nullable<T>`'s
    /// recursion and the empty record tsc accepts included.
    ///
    /// - A value of the union where something else is declared is the value
    ///   being a `string` there: `let n: number = mode` is refused because
    ///   `let n: number = s` is, and `let s: string = mode` is accepted
    ///   because every literal in the set is a string.
    /// - A value of some other type where the union is declared is that type
    ///   being a `string` here: a `number`, a `bool`, a record, a tagged
    ///   union, a prelude container and an imported declaration are all
    ///   refused, each by the rule that already refuses it against `string`.
    /// - Two string-literal unions are compared by literal set. A subset is
    ///   accepted; a literal the declared set does not hold is refused.
    ///
    /// A `string` where the union is declared is refused too, and that arm was
    /// established by running both compilers rather than reasoned about. The
    /// declared set is narrower than `string`, so `tsc` refuses the pairing
    /// (`TS2322`); the question was whether Glyph would then refuse something
    /// `tsc` accepts, because TypeScript narrows a `string` and Glyph's
    /// checker does not. Three spellings were tried. A `match` arm does not
    /// narrow the scrutinee: the emitter hoists it to a temporary (`const __m0
    /// = s; switch (__m0)`), so the binding the arm body names is not the one
    /// the `switch` narrowed, and `match s { "read" => { return s } }` under a
    /// `-> Mode` return is `TS2322`. A `let`-bound literal does not keep its
    /// literal type either: `let s = "read"` emits a TypeScript `let`, which
    /// widens, and `takes_mode(s)` is `TS2345`. `match` is Glyph's only
    /// branching construct, so there is no third way to narrow a binding.
    ///
    /// What does reach this arm holding literals is a value-position `match`
    /// whose arms are literals, whose join the walk records as `string`
    /// (`fn agg_of(name: string) -> Agg { return match name { "sum" => "sum",
    /// ... } }`, which `csvql` contains and `tsc` compiles). `written_literal_refusal`
    /// reads that form, and the written literal, before this is asked, and
    /// answers `Unknown` for the arms it cannot read, so the `string` left
    /// here is one no written form pinned to a narrower set.
    ///
    /// That arm holds at every depth since G237, and the generic-argument
    /// fence 0.1.122 shipped is gone. `["read", "write"]` is an
    /// `Array<"read" | "write">` now, so the correct program the fence
    /// existed to protect (`fn arr() -> Array<Mode> { return ["read",
    /// "write"] }`) is accepted by the literal-set rule rather than by a
    /// declined pairing, and the wrong one the fence swallowed is refused:
    /// `let xs = ["read", "write"]` widens to `Array<string>` at the binding
    /// and `return xs` against an `Array<Mode>` is `TS2322` under `tsc` and
    /// `E0204` here.
    ///
    /// The one position that still declines is the key argument of a
    /// `Record`, where TypeScript's index signature accepts a `string` key
    /// against a literal-union key in both directions. See
    /// `Position::RecordKey`.
    fn string_literal_union_verdict(
        &self,
        found: &Ty,
        expected: &Ty,
        position: Position,
    ) -> Option<bool> {
        let f = self.string_literal_union_values(found);
        let e = self.string_literal_union_values(expected);
        match (f, e) {
            // Decisive in both directions, and it has to be: two literal sets
            // reach `definitely_incompatible` as two `Ty::Named`s, and the
            // nominal rule (D49) would refuse `let w: Wide = m` for the names
            // differing when the sets say it stands.
            (Some(f), Some(e)) => Some(f.iter().any(|v| !e.contains(v))),
            // A prelude container is already decided by
            // `prelude_container_incompatible`, which runs first and reads a
            // string-literal union as the `string` it is; asking the widened
            // question again would refuse the `Nullable<T>` that arm accepted.
            (Some(_), None) => (self.prelude_container(expected).is_none()
                && self.assign_incompatible(&Ty::Prim(Primitive::String), expected))
            .then_some(true),
            (None, Some(_)) => {
                if matches!(found, Ty::Prim(Primitive::String)) {
                    // Refused everywhere but a `Record`'s key. See the doc
                    // comment above.
                    return (position != Position::RecordKey).then_some(true);
                }
                self.assign_incompatible(found, &Ty::Prim(Primitive::String))
                    .then_some(true)
            }
            (None, None) => None,
        }
    }

    fn assign_incompatible(&self, found: &Ty, expected: &Ty) -> bool {
        self.assign_incompatible_at(found, expected, Position::Ordinary)
    }

    /// `assign_incompatible`, plus where in a type the pairing sits.
    ///
    /// One rule reads `position` and no other does: a bare `string` where a
    /// string-literal union is declared, which is refused everywhere except
    /// the key argument of a `Record`, where TypeScript's index signature
    /// accepts it (see `Position::RecordKey` and
    /// `string_literal_union_verdict`). Every other refusal the relation makes
    /// is made at every depth, so a `number`, a `bool`, a record, a tagged
    /// union and a literal outside the declared set are refused inside
    /// `Array<Mode>` exactly as they are against `Mode`.
    fn assign_incompatible_at(&self, found: &Ty, expected: &Ty, position: Position) -> bool {
        if let Some(members) = self.interface_member_fields(expected) {
            return match self.record_fields_of(found) {
                // A value carries its members structurally, and they are
                // compared the way a record's fields are: through this whole
                // relation, with the weak-type rule. The free function this
                // went through had no declaration in hand, so a member holding
                // a declared record passed against a record of another shape
                // (G274).
                Some(found_fields) => {
                    // An interface can name itself through a member
                    // (`interface Tree { kids: Array<Tree> }`), and so can the
                    // record compared against it, so the walk can arrive back
                    // at the pairing it started from. A pairing already being
                    // related is assumed to hold, which is TypeScript's answer
                    // for a recursive structural comparison: anything below it
                    // that is refused is refused on its own. The free function
                    // compared `Node` against `Tree` by name one level down and
                    // refused a program `tsc` accepts.
                    let pair = (found.clone(), expected.clone());
                    if self.relating.borrow().contains(&pair) {
                        return false;
                    }
                    self.relating.borrow_mut().push(pair);
                    let value = Ty::Record {
                        fields: found_fields.clone(),
                    };
                    let refused = weak_record_disjoint(&value, &members)
                        || self.record_fields_incompatible(&found_fields, &members);
                    self.relating.borrow_mut().pop();
                    refused
                }
                // `found` is undecidable (an open generic, an unresolved value):
                // stay permissive, exactly as the nominal path does.
                None => false,
            };
        }
        // G293. A generic declaration applied (`Box<string>`) where a
        // `Record<K, V>` map is declared is decided the way the bare name is,
        // by `declared_type_against_map_incompatible`, and not by the base
        // comparison below, which would read `Box` against `Record` as two
        // declarations and refuse a value the map's validator accepts.
        if let (Ty::App { .. }, Some(("Record", args))) = (found, self.prelude_container(expected)) {
            if self.prelude_container(found).is_none() && self.is_declared_type_with_identity(found) {
                return self.declared_type_against_map_incompatible(found, expected, args);
            }
        }
        if let (Ty::App { base: fb, args: fa }, Ty::App { base: eb, args: ea }) = (found, expected) {
            // The base is the constructor, not an argument, so it is asked
            // under `Ordinary`. So is every argument, with one exception: the
            // key of a `Record` on both sides, which TypeScript compares
            // through an index signature rather than by assignability.
            let both_records = self.resolves_to_map(found) && self.resolves_to_map(expected);
            return fa.len() != ea.len()
                || self.assign_incompatible_at(fb, eb, Position::Ordinary)
                || fa.iter().zip(ea.iter()).enumerate().any(|(i, (f, e))| {
                    let at = if both_records && i == 0 {
                        Position::RecordKey
                    } else {
                        Position::Ordinary
                    };
                    self.assign_incompatible_at(f, e, at)
                });
        }
        // G201. A declared union or record where a `string`, a `number` or a
        // `bool` is expected. `definitely_incompatible` cannot judge this on its
        // own: it sees a `Ty::Named` and stays permissive, because a name can
        // stand for a primitive and the free function has no way to look the
        // declaration up. Reading the body is what makes the pair decidable, and
        // reading it is only possible here.
        //
        // It was the largest silent hole in the relation. Passing a declared
        // union where a `string` was declared produced no diagnostic at all
        // while the same call with a `bool` produced E0211, so a signature
        // change was invisible at exactly the call sites most likely to be
        // wrong, and `glyph check --no-tsc` accepted the program.
        //
        // Only these three primitives. `void` is left out for the same reason
        // the scalar-versus-record arm leaves it out. The general question of
        // how much of assignability should become decidable is a separate
        // decision, and answering it wrongly rejects correct programs, which
        // costs more than the silence did.
        if let Ty::Prim(p) = expected {
            if is_concrete_scalar(*p) && self.is_declared_union_or_record(found) {
                return true;
            }
        }
        // The reverse direction. A `string`, a `number` or a `bool` where a
        // declared union or record is expected: `takes_result("Settled")` was
        // caught only by `tsc`, so `glyph check --no-tsc` accepted it. The same
        // reading of the declaration body settles it, with the same exclusions
        // (a primitive alias, a string-literal union, an `extern_ts` or `typeof`
        // body, an `interface`) and one more.
        //
        // A record with zero fields (`type T = { }`) is excluded. TypeScript
        // lets a `string` satisfy the empty object type, so Glyph's emitted
        // TypeScript for such a program compiles under `tsc --strict`, and
        // rejecting it here would make Glyph disagree with tsc on a program tsc
        // accepts. The exclusion is one-directional: `{}` is not a `string`
        // under tsc either, so the arm above keeps firing on the empty record.
        if let Ty::Prim(p) = found {
            if is_concrete_scalar(*p) {
                if let Some(body) = self.declared_union_or_record_body(expected) {
                    let empty_record = matches!(body, TypeExpr::Record { fields, .. } if fields.is_empty());
                    if !empty_record {
                        return true;
                    }
                }
            }
        }
        // D46. Two names for one declaration are one type, so each side is
        // rewritten to the declaration its names resolve to before the nominal
        // comparison reads them. Without this `takes_a(b)` with `type B = A`
        // was E0211 on a program `tsc --strict` accepts (G203), because
        // `definitely_incompatible` compares a `Ty::Named` by its lexical path
        // and has no declaration to look up.
        let found = self.canonicalize_aliases(found);
        let expected = self.canonicalize_aliases(expected);
        // G215, G226. An imported declaration is decided like a local one.
        // Read after canonicalization, because a local second name for an
        // import is the `Ty::Imported` it names by now, and that is the form
        // every rule below has to recognise.
        if self.imported_incompatible(&found, &expected) {
            return true;
        }
        if self.prelude_container_incompatible(&found, &expected) {
            return true;
        }
        // G230. A declared string-literal union refused nothing at all: a
        // `number`, a record and a tagged union all passed where `Mode` was
        // declared, on every surface that runs without tsc.
        if let Some(verdict) = self.string_literal_union_verdict(&found, &expected, position) {
            return verdict;
        }
        // G232. A structural record where a *named* record is declared.
        // `definitely_incompatible` compares two `Ty::Record`s field by field
        // and has no arm at all for a record against a `Ty::Named`, so
        // `let u: User = { name: "a" }` fell through to "not provably
        // incompatible" and the missing `age` reached only `tsc`.
        //
        // Structural, not nominal, and that is TypeScript's rule for the same
        // pairing: a value carrying `User`'s fields is a `User` there, so
        // refusing it by name would refuse a program `tsc --strict` compiles.
        // The nominal rule (D49) is untouched: it decides two *names*, and
        // this side has none.
        //
        // The field set comes from `record_fields_of`, so a local
        // declaration, a second name for one (D46), an imported record and a
        // stdlib type the runtime ships are all read the way every other site
        // reads them. A name with no readable field set — a tagged union, a
        // prelude container, an unresolved `.d.ts` type — answers `None` and
        // the pairing stays where it was.
        //
        // Each field is compared through the whole relation, the way every
        // other position is, rather than through `definitely_incompatible`.
        // The free function reads two `Ty`s with no declaration in hand, so a
        // field holding an imported record, a declared union, a second name
        // or a string-literal union was compared by nothing: `{ o: Order }`
        // passed where `{ o: Other }` was declared and `{ mode: string }`
        // where `{ mode: Mode }` was, both of which `tsc` refuses (TS2345).
        // A written literal was already compared field by field through the
        // relation (`written_literal_refusal`), so the hole was a value of
        // structural type that is not a literal.
        //
        // And TypeScript's weak-type rule (TS2559), mirrored exactly: a
        // record whose fields are all optional refuses a value that shares
        // none of them (`weak_record_disjoint`).
        // G254. An opaque host handle (`dom.Element`) is modeled with an empty
        // field set so a member read is E0210, and an empty field set would
        // let the structural arm below take any record for it: `take({})`
        // passed every Glyph stage and failed in `tsc`, where the type is the
        // host's `HTMLElement`. Nothing a program writes is one, so only a
        // value already of that type is accepted; an undecidable value stays
        // permissive, as everywhere in this relation.
        if self.is_opaque_stdlib_type(&expected)
            && ty_is_decidable(&found)
            && !matches!(found, Ty::Never)
            && found != expected
        {
            return true;
        }
        if matches!(found, Ty::Record { .. } | Ty::Fn { .. }) {
            if let Some(declared) = self.record_fields_of(&expected) {
                if weak_record_disjoint(&found, &declared) {
                    return true;
                }
                if let Ty::Record { fields: have } = &found {
                    return self.record_fields_incompatible(have, &declared);
                }
            }
        }
        // G274. A declared record where a structural one is declared, `m: Mine`
        // passed to `x: { b: string }`, is compared by field, which is
        // TypeScript's rule. The nominal rule (D49) decides two names and this
        // pairing has a name on one side only, the mirror of the arm above.
        if let Ty::Record { fields: declared } = &expected {
            if !matches!(found, Ty::Record { .. }) {
                if let Some(have) = self.record_fields_of(&found) {
                    let value = Ty::Record {
                        fields: have.clone(),
                    };
                    return weak_record_disjoint(&value, declared)
                        || self.record_fields_incompatible(&have, declared);
                }
            }
        }
        // G274. A function's return is compared through this relation, under
        // the guards `definitely_incompatible` documents for its function arm,
        // so `fn() -> { o: { b: string } }` where `fn() -> { o: Other }` is
        // declared is refused as `tsc` refuses it (TS2345, "call signature
        // return types ... are incompatible"). The parameters are still
        // compared by nothing.
        if let (
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
        ) = (&found, &expected)
        {
            if ty_is_decidable(&found) && ty_is_decidable(&expected) {
                return !matches!(**fr, Ty::Prim(Primitive::Void))
                    && !matches!(**er, Ty::Prim(Primitive::Void))
                    && (fa != ea || self.assign_incompatible(fr, er));
            }
        }
        definitely_incompatible(&found, &expected)
    }

    /// Whether `ty` is a stdlib type the checker models as opaque: a name with
    /// an empty field set in `stdlib_type_fields`, which is how a live host
    /// handle is written there (G254).
    fn is_opaque_stdlib_type(&self, ty: &Ty) -> bool {
        stdlib_type_path(ty).is_some()
            && stdlib_type_fields(self, ty).is_some_and(|f| f.is_empty())
    }

    /// A structural record's fields against a declared field set: a required
    /// field the value lacks, a field the value holds as optional where the
    /// declaration requires it (TS2345, `number | undefined` is not a
    /// `number`; G274), or a shared field the relation refuses.
    fn record_fields_incompatible(&self, have: &[RecordField], declared: &[RecordField]) -> bool {
        declared.iter().any(|e| match have.iter().find(|f| f.name == e.name) {
            Some(f) if f.optional && !e.optional => true,
            Some(f) => self.assign_incompatible_at(&f.ty, &e.ty, Position::Ordinary),
            None => !e.optional,
        })
    }

    /// The accepting face of `record_fields_incompatible` and the weak-type
    /// rule, for a structural record against any declared field set.
    fn record_acceptance(&self, found: &Ty, have: &[RecordField], declared: &[RecordField]) -> bool {
        if weak_record_disjoint(found, declared) {
            return false;
        }
        declared.iter().all(|e| match have.iter().find(|f| f.name == e.name) {
            // A field the value may not carry, where the declaration requires
            // one: refused by `record_fields_incompatible` (G274), so never an
            // acceptance.
            Some(f) if f.optional && !e.optional => false,
            Some(f) => self.accepting_rule(&f.ty, &e.ty).is_some(),
            None => e.optional,
        })
    }

    /// The rule that positively accepts `found` where `expected` is declared,
    /// or `None` when no rule covers the pairing.
    ///
    /// The other half of [`assignability`], and the half `assign_incompatible`
    /// cannot express. Read its doc first: a rule is listed here only when it
    /// is **total** over the pairing, meaning it refuses whenever the two
    /// sides differ in the way it reads, so its silence is an acceptance. A
    /// rule that reads one side and stays permissive about the other is not
    /// total and is deliberately absent, whatever the relation returned.
    ///
    /// The nine that are total, each the accepting face of an arm of
    /// `definitely_incompatible`, of `imported_incompatible` or of
    /// `string_literal_union_verdict`:
    ///
    /// - `unknown` as the declared type accepts any value;
    /// - `never` as the value's type fits any declaration (D47);
    /// - two primitives, when they are the same primitive;
    /// - two declared names, when both resolve to the same declaration (the
    ///   nominal rule, D46's second names followed first);
    /// - two imported declarations, when both chains of second names end at
    ///   the same `(module, name)` (G226);
    /// - two generic applications, when the arity matches and the base and
    ///   every argument are themselves accepted here;
    /// - two structural records, when every required field of the declared
    ///   type is present in the value's type, is not optional there, and is
    ///   itself accepted here. Width subtyping is the rule, so an extra field
    ///   in the value's type is fine. `optional` is read on both sides: a
    ///   value whose field is optional where the declaration requires one is
    ///   refused (G274), as TypeScript refuses it (`TS2345`).
    ///
    /// Two more from G230: two string-literal unions, when every literal the
    /// value's type accepts is one the declared type accepts (D30); and a
    /// string-literal union where a `string` is declared, which every literal
    /// in the set satisfies. Not the third direction, some other type where a
    /// string-literal union is declared: every type the relation can read
    /// there it refuses, a bare `string` included, so a silence there is a
    /// type it could not read at all rather than one it accepted.
    ///
    /// What is deliberately not here. Two function types: the relation
    /// compares the returns and the `async` flag and says nothing at all about
    /// the parameters, so a silent pass proves nothing about a parameter
    /// mismatch. An interface as the declared type: the member-shape branch
    /// returns "not refused" for a value whose field set it could not read at
    /// all, which is the exact shape of a rule that did not look. A pairing
    /// with `Ty::Unknown` or an open `Ty::Param` on either side, which
    /// `ty_is_decidable` declines. And a container against a name, or an
    /// application against a bare name, which no arm reads in either
    /// direction.
    fn accepting_rule(&self, found: &Ty, expected: &Ty) -> Option<&'static str> {
        if matches!(expected, Ty::UnknownTop) {
            return Some(
                "`unknown` is the top type, and the relation accepts a value of any type \
                 where it is declared",
            );
        }
        if matches!(found, Ty::Never) {
            return Some(
                "`never` is the bottom type (D47): no value has it, so an expression of it \
                 stands wherever a value is wanted",
            );
        }
        if matches!(expected, Ty::Never) {
            return None;
        }
        // An interface is matched by member shape, and that branch answers
        // "not refused" for a value it could not read a field set from, so no
        // silence under it is an acceptance.
        if self.interface_member_fields(expected).is_some() {
            return None;
        }
        // Mirrors `assign_incompatible`'s own application arm, which runs
        // before canonicalization and recurses through the whole relation.
        if let (Ty::App { base: fb, args: fa }, Ty::App { base: eb, args: ea }) = (found, expected) {
            if fa.len() != ea.len() {
                return None;
            }
            self.accepting_rule(fb, eb)?;
            for (f, e) in fa.iter().zip(ea.iter()) {
                self.accepting_rule(f, e)?;
            }
            return Some(
                "two generic applications are compared by arity, by base and by argument, \
                 and every one of those pairings is itself accepted",
            );
        }
        let found = self.canonicalize_aliases(found);
        let expected = self.canonicalize_aliases(expected);
        // G230, both faces of `string_literal_union_verdict`. Read before
        // the pairings below, because a string-literal union reaches them as a
        // `Ty::Named` and the nominal rule would answer for it without ever
        // reading the literal set.
        if let Some(e) = self.string_literal_union_values(&expected) {
            // A bare `string` included: this rule accepts nothing for it,
            // which is right in both directions. The refusing relation
            // refuses it where the union itself is declared and leaves it
            // undetermined under a generic argument, and neither of those is
            // an acceptance.
            let f = self.string_literal_union_values(&found)?;
            return f.iter().all(|v| e.contains(v)).then_some(
                "two string-literal unions are compared by literal set (D30), and every \
                 literal the value's type accepts is one the declared type accepts",
            );
        }
        if self.string_literal_union_values(&found).is_some() {
            return matches!(expected, Ty::Prim(Primitive::String)).then_some(
                "every literal a string-literal union accepts is a `string` (D30), so a \
                 value of one stands where a `string` is declared",
            );
        }
        match (&found, &expected) {
            (Ty::Prim(a), Ty::Prim(b)) if a == b => Some(
                "two primitives are compared, and these are the same primitive",
            ),
            // G216's backward direction. A `string`, a `number` or a `bool`
            // where a prelude container is declared is refused outright for
            // four of the five, and for `Nullable<T>` the whole pairing is
            // asked again against `T`, so the rule is total exactly as far as
            // that inner pairing is.
            (Ty::Prim(p), _) if is_concrete_scalar(*p) => {
                let ("Nullable", args) = self.prelude_container(&expected)? else {
                    return None;
                };
                self.accepting_rule(&found, args.first()?)?;
                Some(
                    "a `string`, a `number` or a `bool` where a `Nullable<T>` is \
                     declared is compared against `T` (G216), and that pairing is \
                     itself accepted",
                )
            }
            (Ty::Named { path: a, .. }, Ty::Named { path: b, .. })
                if !a.is_empty() && a == b =>
            {
                Some(
                    "two declared types are compared by the name each resolves to (Q15, \
                     nominal, with a chain of second names followed first under D46), and \
                     both reach the same declaration",
                )
            }
            (Ty::Imported { .. }, Ty::Imported { .. }) => {
                let f = self.imported_union_or_record(&found)?;
                let e = self.imported_union_or_record(&expected)?;
                (f.identity() == e.identity()).then_some(
                    "two imported declarations are compared by the module and name each \
                     chain of second names ends at (G226), and both chains end at the same \
                     declaration",
                )
            }
            (Ty::Record { fields: ff }, Ty::Record { fields: ef }) => {
                self.record_acceptance(&found, ff, ef).then_some(
                    "two structural records are compared field by field with width \
                     subtyping, and every required field of the declared type is present, \
                     not optional on the value's side, and of an accepted type",
                )
            }
            // The accepting face of the same rule against a declared record,
            // local, imported, generic or a second name for one: the field set
            // `record_fields_of` reads is the one the refusing arm compared.
            (Ty::Record { fields: ff }, _) => {
                let ef = self.record_fields_of(&expected)?;
                self.record_acceptance(&found, ff, &ef).then_some(
                    "a structural record is compared against the fields of the declared \
                     record it is given for, field by field with width subtyping, and every \
                     required field of the declaration is present, not optional on the \
                     value's side, and of an accepted type",
                )
            }
            _ => None,
        }
    }

    /// The pairings of a prelude container against a primitive the relation
    /// decides, on canonical types (G216).
    ///
    /// `definitely_incompatible` has no arm for `Ty::App` against `Ty::Prim`
    /// and falls through to "not provably incompatible", so `let o: Option<int>
    /// = Some(3)` followed by `let x: string = o` drew nothing, and neither did
    /// `takes_int(v)` with `v: Nullable<int>`. A boundary type that exists to
    /// make `null` visible and then passes silently where an `int` is declared
    /// is the pillar failing at the type it was added for.
    ///
    /// Forward, a container is never a `string`, a `number` or a `bool`: the
    /// argument is in `prelude_container`, and it holds for every argument, so
    /// nothing here reads the type arguments.
    ///
    /// Backward, four of the five are decided the same way, and `Nullable<T>`
    /// is not. A `string` where a `Nullable<string>` is declared is correct
    /// (`let v: Nullable<int> = 3` is the intended spelling of a value that
    /// may also arrive as null), so the pairing is the one against `T`, asked
    /// through this same relation so an alias, a declaration and a nested
    /// container are all read the way they are read anywhere else. That is how
    /// `let v: Nullable<int> = "a"` becomes a mismatch while `let v:
    /// Nullable<int> = 3` stays correct.
    ///
    /// Undetermined, and left that way: a container against `void` (excluded
    /// by `is_concrete_scalar` for the same reason the scalar-versus-record arm
    /// excludes it), a container against a name whose declaration this does not
    /// read, and an alias of a container against anything, because
    /// `alias_target` does not follow a body that is a generic application and
    /// so `type MaybeInt = Option<int>` never reaches here as an `App`.
    fn prelude_container_incompatible(&self, found: &Ty, expected: &Ty) -> bool {
        if let Ty::Prim(p) = expected {
            if is_concrete_scalar(*p) && self.prelude_container(found).is_some() {
                return true;
            }
        }
        // A string-literal union is a `string` at run time (D30), so it is
        // decided here exactly as a `string` is, and the `Nullable<T>`
        // recursion carries the union itself rather than the widened
        // `string`: that is what keeps `let m: Nullable<Mode> = "read"`
        // correct while `let m: Nullable<Mode> = other_string` is refused.
        if matches!(found, Ty::Prim(p) if is_concrete_scalar(*p))
            || self.string_literal_union_values(found).is_some()
        {
            match self.prelude_container(expected) {
                Some(("Nullable", args)) => {
                    return args
                        .first()
                        .is_some_and(|inner| self.assign_incompatible(found, inner));
                }
                Some(_) => return true,
                None => {}
            }
        }
        // G232. A record where a `Nullable<T>` is declared is the pairing
        // against `T`, for the reason the scalar arm above descends: a
        // `Nullable<T>` admits a `T` or the null a boundary produced (D45),
        // and a written record is not null, so the container changes nothing
        // about the question. Without it `let u: Nullable<User> = { name: "a"
        // }` stayed silent while `tsc` reported the missing `age`.
        //
        // Only `Nullable`. The other four containers are left undetermined for
        // a record on purpose, where a scalar is refused outright: a
        // `Record<K, V>` map *is* written as an object literal, so refusing a
        // record against a container as a class would refuse the one spelling
        // a map has, and `Option`/`Result` emit as tagged objects a record
        // literal can legitimately spell.
        if matches!(found, Ty::Record { .. }) {
            if let Some(("Nullable", args)) = self.prelude_container(expected) {
                return args
                    .first()
                    .is_some_and(|inner| self.assign_incompatible(found, inner));
            }
        }
        // G289. A prelude container where a declared tagged union or record is
        // expected. `let user: User = json.parse(body)` hands a `Result` to a
        // `User`, and so does `let u: User = load()` for any function
        // returning one; `definitely_incompatible` has no arm for an
        // application against a bare name, so the pairing was decided by
        // nothing and reached only `tsc` (TS2322).
        //
        // Two declarations, so the nominal rule (D49) is what decides it, the
        // same rule that refuses a `Shape` where a `User` is declared. The
        // container is a declaration of the prelude and the expected type one
        // of this program, local or imported, so they are never the same type
        // whatever the container's arguments are, and nothing here reads them.
        //
        // What stays undetermined: an expected record with no fields, which
        // TypeScript lets any object satisfy and which the primitive arms
        // exclude for the same reason; an expected name whose body is not a
        // union or a record (a primitive alias, a string-literal union, an
        // `extern_ts` or `typeof` body, a generic application such as `type
        // Headers = Record<string, string>`, which D46 keeps its own type but
        // decides nothing from); and an interface, matched by member shape
        // before this runs.
        //
        // A stdlib type is the third kind of declaration, and `let u: url.Url
        // = url.parse(s)` is the same mistake as the `User` one. Only a stdlib
        // type whose shape the checker models counts here: a record with
        // fields or a union. An opaque host handle is refused by its own arm
        // (G254), and a stdlib name with no modeled shape is one this checker
        // cannot see into, so it stays undetermined.
        if self.prelude_container(found).is_some() {
            if stdlib_union_variants(expected).is_some()
                || stdlib_type_fields(self, expected).is_some_and(|f| !f.is_empty())
            {
                return true;
            }
            if let Some(body) = self.declared_union_or_record_body(expected) {
                let empty_record =
                    matches!(body, TypeExpr::Record { fields, .. } if fields.is_empty());
                return !empty_record;
            }
            if let Some(decl) = self.imported_union_or_record(split_type_app(expected).0) {
                return !decl.is_empty_record();
            }
        }
        // G293, the reverse of G289: a declared record or tagged union where
        // a prelude container is declared. `let r: Result<User, string> = u`
        // with `u: User` is two declarations under D49 exactly as the forward
        // pairing is, and nothing decided it, so it reached only `tsc`
        // (TS2322).
        //
        // `Nullable<T>` is the one container that is decided against its
        // argument instead, for the reason the scalar and record arms above
        // descend: it admits a `T` or the null a boundary produced (D45), so
        // `let n: Nullable<User> = u` is the intended spelling and `let n:
        // Nullable<Shape> = u` is `User` against `Shape`.
        //
        // `Record<K, V>` is decided by what the map's validator does, which
        // is D49's rule: an object check and every value passing `V`, keys
        // ignored. `declared_type_against_map_incompatible` states that
        // statically, and says what it leaves to `tsc`.
        //
        // The value side counts what the expected side of G289 counts, with
        // the same exclusion: a record with no fields, an interface, an alias
        // whose body is not a union or a record, and a stdlib name with no
        // modeled shape stay undetermined.
        if let Some((container, args)) = self.prelude_container(expected) {
            if self.is_declared_type_with_identity(found) {
                return match container {
                    "Nullable" => args
                        .first()
                        .is_some_and(|inner| self.assign_incompatible(found, inner)),
                    "Record" => self.declared_type_against_map_incompatible(found, expected, args),
                    _ => true,
                };
            }
        }
        false
    }

    /// G293. A declared record or tagged union (`found`, one
    /// `is_declared_type_with_identity` answers for) where the map `expected`,
    /// a `Record<K, V>` with arguments `args`, is declared.
    ///
    /// The map's validator is an object check, a not-array check, and every
    /// value passing `V`; it ignores the keys. So for a closed record, whose
    /// own validator admits no key beyond its fields, the map's validator
    /// accepts it exactly when every field holds a `V`, and that is the
    /// comparison made here: a required field whose type is not a `V` is
    /// refused, and a record whose fields are all required `V`s is accepted.
    ///
    /// - An `@open` record (D27) is refused unless `V` is `unknown`: the keys
    ///   it admits beyond its fields hold values no validator checked.
    /// - A tagged union is refused wherever any `Record<K, V>` is declared. A
    ///   union has no fields in Glyph, and its runtime shape is the emitter's
    ///   encoding rather than a contract, so comparing it to `V` would let a
    ///   change to the encoding change type checking.
    /// - A required field whose substituted type is still a type parameter
    ///   is refused unless `V` is `unknown` or that parameter
    ///   (`map_field_incompatible`).
    /// - An optional field, and a `K` other than `string`, are not decided:
    ///   the map's validator and `tsc` disagree about them (the validator
    ///   accepts an absent key and ignores the keys `K` names; `tsc` reads the
    ///   `undefined` an optional field may hold and requires `K`'s keys), so
    ///   `tsc` answers. An optional field is skipped and a required field
    ///   beside it still decides the pairing.
    fn declared_type_against_map_incompatible(&self, found: &Ty, expected: &Ty, args: &[Ty]) -> bool {
        let [key, value] = args else {
            return false;
        };
        let (base, type_args) = split_type_app(found);
        // Union or record, and if a record, whether `@open` and its fields.
        let shape: Option<(bool, Vec<RecordField>)> = if stdlib_union_variants(found).is_some() {
            None
        } else if let Some(fields) = stdlib_type_fields(self, found) {
            Some((false, fields))
        } else if let Some(td) = self.local_type_decl(found) {
            match &td.body {
                TypeExpr::Union { .. } => None,
                _ => match self.record_decl_fields(td, type_args) {
                    Some(fields) => Some((glyph_ast::is_open_record(&td.annotations), fields)),
                    None => return false,
                },
            }
        } else if let Some(decl) = self.imported_union_or_record(base) {
            match &decl.body {
                Ty::Union { .. } => None,
                _ => match self.imported_record_fields(&decl.module, &decl.name, type_args) {
                    Some(fields) => Some((decl.open, fields)),
                    None => return false,
                },
            }
        } else {
            return false;
        };
        let Some((open, fields)) = shape else {
            return true;
        };
        if !matches!(key, Ty::Prim(Primitive::String)) {
            return false;
        }
        if open {
            return !matches!(value, Ty::UnknownTop);
        }
        // A record can reach itself through a field held against `V`
        // (`type Node = { kids: Record<string, Node> }`), so a pairing already
        // being related is assumed to hold, as the interface branch assumes.
        let pair = (found.clone(), expected.clone());
        if self.relating.borrow().contains(&pair) {
            return false;
        }
        self.relating.borrow_mut().push(pair);
        let refused = fields
            .iter()
            .any(|f| !f.optional && self.map_field_incompatible(&f.ty, value));
        self.relating.borrow_mut().pop();
        refused
    }

    /// One required field of a closed record against the map's `V`. A field
    /// whose substituted type is still a type parameter (`value: T` in a
    /// `Box<T>` read inside `fn g<T>`) is not a `V`: `T` may be anything, so
    /// it is refused unless `V` is `unknown` or that same parameter. The
    /// relation itself reads an open parameter as undecidable and stays
    /// silent, which would leave the pairing to `tsc` (TS2322) and let it
    /// pass `--no-tsc`, the editor and the playground.
    fn map_field_incompatible(&self, field: &Ty, value: &Ty) -> bool {
        if matches!(field, Ty::Param { .. }) {
            return !matches!(value, Ty::UnknownTop) && field != value;
        }
        self.assign_incompatible(field, value)
    }

    /// Whether `ty` is a declaration D49 gives an identity to and the checker
    /// can read: a record with at least one field or a tagged union, declared
    /// in this module, in a project sibling, or in the stdlib with a modeled
    /// shape. The side of G289 that names the declaration, asked of the value
    /// for G293.
    fn is_declared_type_with_identity(&self, ty: &Ty) -> bool {
        if stdlib_union_variants(ty).is_some()
            || stdlib_type_fields(self, ty).is_some_and(|f| !f.is_empty())
        {
            return true;
        }
        if let Some(body) = self.declared_union_or_record_body(ty) {
            return !matches!(body, TypeExpr::Record { fields, .. } if fields.is_empty());
        }
        self.imported_union_or_record(split_type_app(ty).0)
            .is_some_and(|decl| !decl.is_empty_record())
    }

    /// The pairings of an imported declaration the relation decides, on
    /// canonical types. G215 and G226: `definitely_incompatible` declines
    /// `Ty::Imported` on either side, since the free function has no
    /// declaration to read, so an imported union passed where a `string` was
    /// declared drew nothing under every surface that runs without tsc, and a
    /// local record assigned to a local alias of an imported union lost its
    /// `E0204` the moment the alias canonicalized across the boundary. Each
    /// arm is the imported reading of a rule the local side already applies,
    /// through `imported_union_or_record`, which keeps out exactly what the
    /// local reading keeps out:
    ///
    /// - an imported union or record, or an application of one, is never a
    ///   `string`, a `number` or a `bool` (G201, with no exclusion: `{}` is
    ///   not a `string` under tsc either);
    /// - a `string`, a `number` or a `bool` is never an imported union or a
    ///   record with at least one field (the reverse direction, with the
    ///   zero-field record excluded because tsc lets a `string` satisfy `{}`);
    /// - an imported union or record is a different type from a `type`
    ///   declaration of this module, and from an imported union or record
    ///   whose chain ends at a different `(module, name)`. The identity is
    ///   read after the chain, so `Local`, `Local2` and `Shape` for one
    ///   declaration compare equal, which is what D46 promises.
    ///
    /// A generic application against a bare name stays undetermined in both
    /// directions, as it does for two local names, and an imported declaration
    /// that is neither a union nor a record is compared by nothing here, which
    /// leaves the primitive alias, the string-literal union and the opaque
    /// body exactly as silent as the local rule leaves them.
    fn imported_incompatible(&self, found: &Ty, expected: &Ty) -> bool {
        if let Ty::Prim(p) = expected {
            if is_concrete_scalar(*p)
                && self
                    .imported_union_or_record(split_type_app(found).0)
                    .is_some()
            {
                return true;
            }
        }
        if let Ty::Prim(p) = found {
            if is_concrete_scalar(*p) {
                if let Some(decl) = self.imported_union_or_record(split_type_app(expected).0) {
                    if !decl.is_empty_record() {
                        return true;
                    }
                }
            }
        }
        match (found, expected) {
            (Ty::Imported { .. }, Ty::Imported { .. }) => match (
                self.imported_union_or_record(found),
                self.imported_union_or_record(expected),
            ) {
                (Some(f), Some(e)) => f.identity() != e.identity(),
                _ => false,
            },
            (Ty::Imported { .. }, Ty::Named { .. }) => {
                self.imported_union_or_record(found).is_some()
                    && self.direct_type_decl(expected).is_some()
            }
            (Ty::Named { .. }, Ty::Imported { .. }) => {
                self.direct_type_decl(found).is_some()
                    && self.imported_union_or_record(expected).is_some()
            }
            _ => false,
        }
    }

    /// The declaration and ordered variant list of a union declared in another
    /// module, read off the export view through the same query an imported
    /// union's payload uses. Otherwise None.
    ///
    /// The base is canonicalized first, so a module-local second name for the
    /// imported union (`type Local = Shape`, D46) reads the same declaration
    /// the direct spelling reads. Without the hop the alias lowered to a
    /// `Ty::Named` this arm did not match, and a match over it drew nothing
    /// where the same match over `Shape` drew E0200 (G224).
    fn imported_union_variants(&self, ty: &Ty) -> Option<(UnionRef, Vec<Ident>)> {
        let (base, _args) = split_type_app(ty);
        let canonical = self.resolve_alias_chain(base);
        let Ty::Imported { module, name } = &canonical else { return None };
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
        // A prelude union is declared by the stdlib module the prelude
        // re-exports it from, which is the module the emitter writes the
        // import from and `import std/result { Result }` resolves through
        // (G231). The variant list comes from the same table `glyph_symbol`
        // describes the declaration out of, so what exhaustiveness counts and
        // what an agent is told are one answer.
        let (name, _) = self.prelude_union(ty)?;
        let module = glyph_resolver::prelude_declaring_module(name)?;
        let union = builtin_union(module, name)?;
        let variants = union.variants.iter().map(|v| Ident::from(v.name)).collect();
        Some((UnionRef::builtin(&union), variants))
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
    /// The same walk checks the pattern against the type it destructures, for
    /// a match arm and a JSX `<case>` alike, because both reach it: an object
    /// pattern naming a field the record does not declare is `E0210`, and an
    /// object pattern over a payload that is not a record at all is `E0204`
    /// (G279). Before this, `Loaded({ totl })` over `Loaded({ total: number })`
    /// bound `totl` to `undefined` and only `tsc` objected.
    ///
    /// Deferred: array payloads.
    fn bind_arm_payloads(&mut self, scrutinee_ty: &Ty, pattern: &Pattern) {
        self.record_pattern_tys(Some(scrutinee_ty), pattern);
        let (path, args) = match pattern {
            Pattern::Constructor { path, args, .. } => (path, args),
            // A top-level object pattern destructures the scrutinee itself
            // (`match p { { x: 0, y: y } => .. }`), and is held to the same
            // field check a payload's object pattern is.
            Pattern::Object { fields, span } => {
                self.bind_object_pattern(fields, *span, scrutinee_ty);
                return;
            }
            _ => return,
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
            [Pattern::Object { fields, span }] => {
                self.bind_object_pattern(fields, *span, &payload_ty);
            }
            // `Ok(Some(x))`, `Ok(Some({ total }))` — the payload is itself
            // matched by a constructor, which binds and checks against the
            // payload's own type one level down.
            [sub @ Pattern::Constructor { .. }] => self.bind_arm_payloads(&payload_ty, sub),
            _ => {}
        }
    }

    /// Check an object pattern against the type it destructures and bind each
    /// field to its type from that record. The resolver binds `{ name }` and
    /// `{ name: alias }` at the field's span, so the type is keyed by
    /// `field.span.start`.
    ///
    /// The type is read through `record_shape_of`, the same field set a member
    /// access is checked against, so `{ totl }` and `r.totl` over one record
    /// draw one `E0210` with one field list. A type this checker decides is not
    /// a record (a primitive, an `Array`, a tagged or string-literal union, a
    /// function, `unknown`; the set `props_param` proves) cannot be
    /// destructured by field, and the pattern is `E0204` naming the type
    /// (`PatternNotRecord`). A type
    /// it cannot see into is left alone: a refusal has to be proved.
    ///
    /// A field carrying a structured sub-pattern binds through that pattern
    /// instead, at the sub-pattern's own spans: a nested object destructure
    /// recurses on the field's type, and a nested constructor
    /// (`{ left: Node({ value: v }) }`) goes back through `bind_arm_payloads`,
    /// which resolves the variant's payload before descending again.
    fn bind_object_pattern(&mut self, fields: &[ObjectPatternField], span: Span, ty: &Ty) {
        let shape = self
            .record_shape_of(ty)
            .or_else(|| self.record_shape_of(&self.canonicalize_aliases(ty)));
        let Some(shape) = shape else {
            if let PropsParam::NotRecord(_) = self.props_param(ty, 0) {
                self.errors.push(TypeError::PatternNotRecord {
                    ty: ty_display(ty),
                    span,
                });
            }
            return;
        };
        for pf in fields {
            let Some(rf) = shape.fields.iter().find(|rf| rf.name == pf.key) else {
                // The key is the first thing a field holds, so its span is the
                // field's start plus the key's length, whatever follows it.
                let key_span = Span::new(pf.span.start, pf.span.start + pf.key.len() as u32);
                self.errors.push(TypeError::UnknownField {
                    field: pf.key.to_string(),
                    type_name: field_owner_display(ty),
                    second_name_for: self.second_name_target(ty, &shape.owner),
                    fields: shape.fields.iter().map(|f| f.name.to_string()).collect(),
                    record: self.field_owner_decl(&shape.owner),
                    builtin: None,
                    suggestion: nearest_name(pf.key.as_ref(), shape.fields.iter().map(|f| f.name.as_ref())),
                    span: key_span,
                });
                continue;
            };
            let field_ty = rf.ty.clone();
            if pf.bound_name().is_some() {
                self.local_tys.insert(pf.span.start, field_ty);
                continue;
            }
            match &pf.pattern {
                Some(Pattern::Object { fields: inner, span: inner_span }) => {
                    self.bind_object_pattern(inner, *inner_span, &field_ty)
                }
                Some(sub @ Pattern::Constructor { .. }) => self.bind_arm_payloads(&field_ty, sub),
                _ => {}
            }
        }
    }

    /// Record, for every node of `pattern`, the type of the value that node is
    /// matched against, keyed by the node's own span, and for every bare
    /// identifier node what it is: a variant reference or a binding.
    ///
    /// The emitter reads both back. A variant declared with a record payload
    /// has that payload spread flat into the tag object while every other
    /// payload sits under `value`, so a pattern reached *through* a payload has
    /// to know which, and the type is the only thing that answers it. The
    /// resolution lives here rather than in the emitter because this is the
    /// side that holds the lowerer and the cross-module declaration resolver;
    /// an emitter-local answer would be a second, weaker copy of a question
    /// already answered once.
    ///
    /// A node whose type cannot be resolved is not recorded, and the emitter
    /// falls back to what it can decide from the variant name alone. The walk
    /// does not stop there, because the classification is recorded for every
    /// identifier node whether or not its type resolved: with no variant set
    /// to consult, `is_variant_reference` answers from the name's shape, which
    /// is the answer every coverage pass in this file gives for the same node
    /// (`Scrutinee::Union` over an empty set). The emitter then lowers an arm
    /// exactly as the checker counted it, and holds no rule of its own (G214).
    ///
    /// Three positions, three rules, each the one the checks above apply at
    /// that position:
    /// - an arm head, and a constructor's payload sub-pattern, are classified
    ///   against the variant set of the value there (`required_variants`), the
    ///   list `check_arm_reachability` and `check_patterns_exhaustive` read;
    /// - an object field's sub-pattern is a binding when `bound_name` says the
    ///   field binds it, which is how the resolver bound it and how
    ///   `is_refutable` counts it; a field it does not bind is walked as a
    ///   pattern in its own right;
    /// - an array element or rest is classified by shape alone, because that is
    ///   what `is_refutable` reads for array exhaustiveness and what the
    ///   emitter's chain choice reads for the same element.
    ///
    /// One pass reads a list this walk does not: `check_imported_union_coverage`
    /// over a scrutinee whose type never resolved classifies the heads against
    /// the imported union it recovered from the arms, while the reachability
    /// pass and this walk see no variants there and read shape. Only a
    /// lowercase imported variant over an untyped scrutinee can tell, and it
    /// is recorded here the way the reachability pass counted it.
    fn record_pattern_tys(&mut self, ty: Option<&Ty>, pattern: &Pattern) {
        if let Some(ty) = ty {
            self.tm.insert(pattern.span(), ty.clone());
        }
        match pattern {
            Pattern::Ident { name, span } => {
                let variants: Vec<Ident> = ty
                    .and_then(|t| self.required_variants(t))
                    .map(|(_, vs)| vs)
                    .unwrap_or_default();
                let kind = if is_variant_reference(name, Scrutinee::Union(&variants)) {
                    IdentPattern::VariantReference
                } else {
                    IdentPattern::Binding
                };
                self.tm.record_ident_pattern(*span, kind);
            }
            Pattern::Constructor { path, args, .. } => {
                let payload_ty = path
                    .last()
                    .and_then(|variant| ty.and_then(|t| self.variant_payload(t, variant)));
                for sub in args {
                    self.record_pattern_tys(payload_ty.as_ref(), sub);
                }
            }
            Pattern::Object { fields, .. } => {
                let rec_fields = match ty {
                    Some(Ty::Record { fields }) => fields.clone(),
                    _ => Vec::new(),
                };
                for pf in fields {
                    let Some(sub) = &pf.pattern else { continue };
                    let field_ty = rec_fields.iter().find(|rf| rf.name == pf.key).map(|rf| &rf.ty);
                    if pf.bound_name().is_some() {
                        // `{ key: alias }`: the field binds `alias`, whatever
                        // the field's type declares.
                        if let Some(field_ty) = field_ty {
                            self.tm.insert(sub.span(), field_ty.clone());
                        }
                        self.tm.record_ident_pattern(sub.span(), IdentPattern::Binding);
                        continue;
                    }
                    self.record_pattern_tys(field_ty, sub);
                }
            }
            Pattern::Array { elements, rest, .. } => {
                let elem = ty.and_then(|t| self.prelude_app(t, "Array").and_then(|a| a.first()).cloned());
                for el in elements {
                    self.record_array_slot(elem.as_ref(), el);
                }
                // A rest binding holds the tail, which is another array of the
                // same element type.
                if let Some(r) = rest.as_deref() {
                    self.record_array_slot(ty, r);
                }
            }
            Pattern::Wildcard { .. }
            | Pattern::Else { .. }
            | Pattern::Literal { .. }
            | Pattern::IsType { .. } => {}
        }
    }

    /// One array element or rest slot of `record_pattern_tys`: a bare
    /// identifier there is classified by shape, the reading `is_refutable`
    /// gives it for array exhaustiveness; anything else is walked as a pattern.
    fn record_array_slot(&mut self, ty: Option<&Ty>, p: &Pattern) {
        let Pattern::Ident { name, span } = p else {
            self.record_pattern_tys(ty, p);
            return;
        };
        if let Some(ty) = ty {
            self.tm.insert(*span, ty.clone());
        }
        let kind = if is_constructor_shaped(name) {
            IdentPattern::VariantReference
        } else {
            IdentPattern::Binding
        };
        self.tm.record_ident_pattern(*span, kind);
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
        // The same hop `imported_union_variants` takes: a local second name
        // for an imported union reaches the payload the way the direct
        // spelling does, so a nested pattern under it is typed and checked.
        let canonical = self.resolve_alias_chain(base);
        if let Ty::Imported { module, name } = &canonical {
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

/// Where in a type a pairing the assignability relation is deciding sits: the
/// declared type itself, or an argument of a generic application the relation
/// recursed into.
///
/// A written value the relation refuses: what was declared where it was
/// written, what the value reads as, and where to underline.
///
/// The type rather than its rendering, because the call site builds the
/// diagnostic and needs the declared type to list the values it accepts. The
/// span is the value's own at the top and the offending element's or field's
/// when the rule descended into a written literal.
///
/// `missing_fields` is the declared record's required fields the value's
/// record lacks, when the value is a structural record and the declared type
/// has a field set to compare it against. Empty otherwise.
struct Refusal {
    expected: Ty,
    found: String,
    missing_fields: Vec<String>,
    /// The refusal is the weak-type rule's (see `weak_record_disjoint`).
    no_common_field: bool,
    span: Span,
}

/// One rule turns on it (the `string` half of `string_literal_union_verdict`),
/// and it exists rather than a `bool` so the two ends of the distinction are
/// named at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    /// Every position the relation compares two types in: the pairing the
    /// caller asked about, a generic argument, a record field, a payload.
    Ordinary,
    /// The key argument of a `Record<K, V>` against the key argument of
    /// another `Record<K, V>`.
    ///
    /// TypeScript writes `Record<string, V>` as an index signature, which
    /// covers every key a `Record<Mode, V>` declares, so it accepts the
    /// pairing in both directions; refusing it here would refuse a program
    /// `tsc --strict` compiles. Established by running both compilers:
    /// `Record<string, number>` into a `Record<Mode, number>` and back are
    /// each clean under `tsc --strict`, while `Array<string>` into an
    /// `Array<Mode>` is `TS2322`.
    RecordKey,
}

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
/// - a `Named` type against a differently-shaped type stays permissive (a
///   newtype alias may resolve to that shape), as does every other pair. The
///   one exception is decided by the caller: `assign_incompatible` reads the
///   declaration, and a body that is a tagged union or a record is not a
///   `string`, a `number` or a `bool` (G201), nor the other way round, where
///   an empty record is excluded because tsc lets a `string` satisfy `{}`.
///   The same caller reads a declaration in another module through
///   `imported_union_or_record` and decides the same pairings for it, plus
///   the identity of two declarations (G215, G226); `Ty::Imported` reaching
///   this function is declined on either side.
fn definitely_incompatible(found: &Ty, expected: &Ty) -> bool {
    if matches!(expected, Ty::UnknownTop) {
        return false;
    }
    // `never` is the bottom type (D47): no value has it, so an expression of it
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

/// A declaration in another module that the assignability relation can decide
/// a pairing from: the tagged union or record an imported name reaches,
/// addressed by the module and name its chain of second names ends at. Built
/// only by `imported_union_or_record`, which is where the body is read.
struct ImportedUnionOrRecord {
    module: String,
    name: String,
    body: Ty,
    /// The declaration carries `@open` (D27).
    open: bool,
}

impl ImportedUnionOrRecord {
    /// The declaration's address, which is what two imported declarations are
    /// compared by: one declaration under two names has one address.
    fn identity(&self) -> (&str, &str) {
        (&self.module, &self.name)
    }

    /// `pub type T = { }`, the one record a primitive is not decided against,
    /// because tsc lets a `string` satisfy the empty object type.
    fn is_empty_record(&self) -> bool {
        matches!(&self.body, Ty::Record { fields } if fields.is_empty())
    }
}

/// True for a concrete scalar primitive (`string`/`number`/`bool`) — the ones
/// that are never a record or function. `void` is deliberately excluded; its
/// assignability in return and callback positions is subtler.
fn is_concrete_scalar(p: Primitive) -> bool {
    matches!(p, Primitive::String | Primitive::Number | Primitive::Bool)
}

/// E0228's clause for a type whose shape the checker cannot see into: an
/// `extern_ts` or `typeof` body, an unresolved name, a prelude type with no
/// literal form of its own. It says the one thing that is true of every type
/// name without claiming a construction that may not exist.
const GENERIC_CONSTRUCTION: &str = "a value is written in its own form, with the type on the annotation";

/// How a value of a prelude type is written, or `None` for a prelude *value*
/// (`Ok`, `Err`, `Some`, `None`, `par`, `print`, `assert`), which is a legal
/// expression and not G217's error at all.
///
/// Matched without a wildcard: a new prelude entry has to be classified as a
/// type or a value here, which is the question E0228 turns on.
fn prelude_type_construction(kind: PreludeKind) -> Option<&'static str> {
    Some(match kind {
        PreludeKind::String => "a value is a string literal, with the type on the annotation",
        PreludeKind::Number | PreludeKind::Int => {
            "a value is a number literal, with the type on the annotation"
        }
        PreludeKind::BigInt => "a value is a `bigint`, with the type on the annotation",
        PreludeKind::Bool => "a value is `true` or `false`",
        PreludeKind::Void => "`void` is the absence of a value; there is nothing to write",
        PreludeKind::Never => "`never` has no values (D47); nothing can be written here",
        PreludeKind::UnknownTop => {
            "a value of any type is an `unknown`, with the type on the annotation"
        }
        PreludeKind::Result => "a value is `Ok(v)` or `Err(e)`",
        PreludeKind::Option => "a value is `Some(v)` or `None`",
        PreludeKind::Nullable => {
            "a value is a plain `T`, or the null a boundary produced (D45); `nullable.from_option` makes one"
        }
        PreludeKind::Array => "a value is an array literal such as `[a, b]`",
        PreludeKind::Record => "a value is an object literal such as `{ key: value }`",
        PreludeKind::Schema
        | PreludeKind::Component
        | PreludeKind::Issue
        | PreludeKind::InferOutput => GENERIC_CONSTRUCTION,
        PreludeKind::Ok
        | PreludeKind::Err
        | PreludeKind::Some
        | PreludeKind::None
        | PreludeKind::Par
        | PreludeKind::Print
        | PreludeKind::Assert => return None,
    })
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
pub(crate) fn stdlib_named(module: &str, name: &str) -> Ty {
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
            | ("dom", "Element")
            | ("storage", "StorageError")
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
        // The field's type is `ServerErrorKind` by name, so a mismatch on it
        // says `ServerErrorKind` and the literals come from the one table the
        // runtime is checked against (G294).
        ("net", "ServerError") => vec![
            (
                "kind",
                Ty::Imported {
                    module: "std/net".into(),
                    name: Ident::from("ServerErrorKind"),
                },
            ),
            ("message", Ty::Prim(Primitive::String)),
            ("code", Ty::Prim(Primitive::String)),
        ],
        // A live page element, the host's `HTMLElement` in the TypeScript.
        // Opaque on purpose: an empty field set makes every member read E0210,
        // so the only things a program does with one are hand it to a library
        // (`createRoot`) or back to `std/dom`. React owns the DOM it renders,
        // and a write underneath it is a bug.
        ("dom", "Element") => Vec::new(),
        // `kind` is a string-literal union for the reason `ServerError`'s is:
        // a full store and a refused one lead to different decisions, and a
        // `match` over the two is exhaustive with no catch-all.
        ("storage", "StorageError") => vec![
            (
                "kind",
                Ty::StringLiteralUnion(vec!["quota".to_string(), "unavailable".to_string()]),
            ),
            ("message", Ty::Prim(Primitive::String)),
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
        // needs no variant table: D30 exhaustiveness reads the members off
        // `HttpErrorKind`'s entry in `BUILTIN_STRING_LITERAL_UNIONS`. Without this a caller matching `e.kind` gets E0218
        // ("a string match can never be exhaustive") and is told to add a
        // catch-all, which is advice to switch the check off.
        ("http", "HttpError") => vec![
            ("status", Ty::Prim(Primitive::Number)),
            ("message", Ty::Prim(Primitive::String)),
            (
                "kind",
                Ty::Imported {
                    module: "std/http".into(),
                    name: Ident::from("HttpErrorKind"),
                },
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
    let (ns, name) = stdlib_type_path(ty)?;
    // `fs.ErrorKind` is written through a namespace import, so the namespace
    // segment is the last segment of the module key (G231).
    let union = builtin_union(&format!("std/{ns}"), name)?;
    let variants = union.variants.iter().map(|v| Ident::from(v.name)).collect();
    Some((UnionRef::builtin(&union), variants))
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

/// The type of a built-in method read as a callee, from its generated row
/// (G312): a function of the row's parameters and return type, with `elem`
/// for the array's `T` and `array` building `Array<X>`. A row the mapping
/// rules left untyped is `Unknown`, which the call arm leaves unchecked.
///
/// The type parameters are `None`, "not known", rather than an empty list:
/// `map` and `flatMap` have TypeScript type parameters the row does not
/// carry, so claiming none would refuse `xs.map<string>(f)`, which `tsc`
/// accepts.
fn builtin_method_ty(sig: &MethodSig, elem: &Ty, array: impl Fn(Ty) -> Ty) -> Ty {
    fn lower(t: MethodTy, elem: &Ty, array: &dyn Fn(Ty) -> Ty) -> Ty {
        match t {
            MethodTy::Unknown => Ty::Unknown,
            MethodTy::Number => Ty::Prim(Primitive::Number),
            MethodTy::String => Ty::Prim(Primitive::String),
            MethodTy::Bool => Ty::Prim(Primitive::Bool),
            MethodTy::Void => Ty::Prim(Primitive::Void),
            MethodTy::Elem => elem.clone(),
            MethodTy::ArrayOf(inner) => array(lower(*inner, elem, array)),
        }
    }
    let MethodSig::Typed { params, ret } = sig else {
        return Ty::Unknown;
    };
    Ty::Fn {
        params: params
            .iter()
            .map(|p| FnParam {
                name: None,
                owned: false,
                ty: lower(p.ty, elem, &array),
                optional: p.optional,
            })
            .collect(),
        return_ty: Arc::new(lower(*ret, elem, &array)),
        is_async: false,
        type_params: None,
    }
}

/// If `ty` is an application of the prelude container type called `name`
/// (`Array`, `Option`, `Nullable`, `Record`, `Result`), return its type
/// arguments. The single collision-guarded prelude-app detector: prelude and
/// module symbol tables both number ids from 0, so an id match alone could
/// collide with an unrelated module symbol, and a name match alone would let
/// a user's own `type Option<T>` answer for the prelude's (G213). Both are
/// required: the lexical name on the base path AND the prelude id. Public so
/// the emitter decides a runtime check from the same guard the checker
/// decides a type from.
pub fn prelude_app<'a>(prelude: &Prelude, ty: &'a Ty, name: &str) -> Option<&'a [Ty]> {
    let Ty::App { base, args } = ty else { return None };
    let Ty::Named { symbol, path } = base.as_ref() else { return None };
    if path.last().map(|n| n.as_ref()) != Some(name) {
        return None;
    }
    (prelude.lookup(name) == Some(SymbolId(symbol.0))).then_some(args.as_slice())
}

/// The prelude container `ty` applies, with its type arguments, or `None`.
///
/// The five names are the closed list `prelude_container_incompatible` is
/// stated against, and the list lives here so a surface that has to name the
/// same set (the impact tool's `change_signature_type` cells) reads it rather
/// than writing a sixth copy that drifts.
pub fn prelude_container<'a>(prelude: &Prelude, ty: &'a Ty) -> Option<(&'static str, &'a [Ty])> {
    for name in ["Option", "Result", "Array", "Record", "Nullable"] {
        if let Some(args) = prelude_app(prelude, ty, name) {
            return Some((name, args));
        }
    }
    None
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
pub fn split_type_app(ty: &Ty) -> (&Ty, &[Ty]) {
    match ty {
        Ty::App { base, args } => (base.as_ref(), args.as_slice()),
        other => (other, &[]),
    }
}

/// Follow `(module, name)` across the sibling's own chain of second names to
/// the declaration it ends at, and answer where that is beside what it is.
///
/// `pub type Rows = Sheet` in a sibling lowers to a `Ty::Imported` body on the
/// export view, which is another hop, so a caller reading the declaration
/// behind an imported name reads the one at the end. The address answered is
/// the pair the chain ends at, which is what two imported declarations are
/// compared by: one declaration under two names has one address (D46). A
/// cycle (`pub type A = B` beside `pub type B = A`) answers `None` rather than
/// looping.
///
/// The one place a cross-module chain is walked, for the checker's own field,
/// literal-set and assignability reads alike. Public so a surface that decides
/// a pairing the way `imported_incompatible` decides it (the impact tool's
/// `change_signature_type` cells) reads the declaration through this walk and
/// the resolver the checker ran with, rather than through a walk of its own.
/// The declaration of `name` in `module`, wherever it is declared: a project
/// module answers through the resolver, and the standard library's own
/// string-literal unions (`http.RedirectPolicy`, `log.Level`) answer from the
/// table in `ty.rs`, since no Glyph source declares them (G294).
///
/// The two walks over a chain of second names go through here, so a stdlib
/// union is in sight for exhaustiveness, assignability and the emitter alike.
fn declared_type_across_modules(
    resolver: &dyn DeclTyResolver,
    module: &str,
    name: &str,
) -> Option<ImportedTypeDecl> {
    resolver
        .imported_type_decl(module, name)
        .or_else(|| builtin_string_literal_union_decl(module, name))
}

pub fn imported_decl_chain_end(
    resolver: &dyn DeclTyResolver,
    module: &str,
    name: &str,
) -> Option<(String, String, ImportedTypeDecl)> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let (mut module, mut name) = (module.to_string(), name.to_string());
    loop {
        if !seen.insert((module.clone(), name.clone())) {
            return None;
        }
        let decl = declared_type_across_modules(resolver, &module, &name)?;
        match &decl.body {
            Ty::Imported {
                module: next_module,
                name: next_name,
            } => {
                let (next_module, next_name) =
                    (next_module.as_str().to_string(), next_name.to_string());
                module = next_module;
                name = next_name;
            }
            _ => return Some((module, name, decl)),
        }
    }
}

/// Whether a written expression produces a *fresh* literal type: one this
/// program's text created here, rather than one that arrived through a name.
///
/// True only for the forms whose literal type is written out: a string
/// literal, an array literal whose every element is one of these, an object
/// literal whose every field value is one of these, and a value-position
/// `match` whose every arm is. Everything else answers false, which leaves its
/// type alone.
fn produces_fresh_literal(e: &Expr) -> bool {
    match e {
        Expr::String { .. } => true,
        // A spread excluded for the same reason it stops the record being
        // synthesized at all: its fields are not written here, so nothing can
        // be said about them (G232).
        Expr::Object { fields, .. } => fields.iter().all(|f| match f {
            ObjectField::KeyValue { value, .. } => produces_fresh_literal(value),
            ObjectField::Spread { .. } => false,
        }),
        Expr::Array { elements, .. } => {
            !elements.is_empty()
                && elements.iter().all(|el| match el {
                    ArrayElem::Expr(e) => produces_fresh_literal(e),
                    ArrayElem::Spread(_) => false,
                })
        }
        Expr::Match { arms, .. } => arms.iter().all(|a| match &a.body {
            MatchArmBody::Expr(e) => produces_fresh_literal(e),
            MatchArmBody::Block(_) => false,
        }),
        _ => false,
    }
}

/// `ty` with every string-literal type in it replaced by `string`: at the top,
/// inside a generic application's arguments, and inside a record's fields.
///
/// Deep, because TypeScript's widening is: `let xs = ["read", "write"]` is a
/// `string[]` there, not a `("read" | "write")[]`, and `return xs` against an
/// `Array<Mode>` is `TS2322`.
fn widen_literals(ty: &Ty) -> Ty {
    match ty {
        Ty::StringLiteralUnion(_) => Ty::Prim(Primitive::String),
        Ty::App { base, args } => Ty::App {
            base: Arc::new(widen_literals(base)),
            args: args.iter().map(widen_literals).collect(),
        },
        Ty::Record { fields } => Ty::Record {
            fields: fields
                .iter()
                .map(|f| RecordField {
                    name: f.name.clone(),
                    ty: widen_literals(&f.ty),
                    optional: f.optional,
                })
                .collect(),
        },
        _ => ty.clone(),
    }
}

/// The literal set of a string-literal union declared in another module, or
/// `None` for anything else.
///
/// The one accessor every consumer of an imported D30 union goes through
/// (G233): assignability, match exhaustiveness, the emitter's narrowing
/// assertion and comparison lowering, `glyph_symbol`'s `literals`, and
/// `glyph_assignable`. The type itself carries the declaration's address and
/// its name, never the set, so `expected `Mode`` is what a diagnostic prints
/// through every import spelling, exactly as it prints for the same
/// declaration read locally.
///
/// The chain of second names is followed to its end, so `pub type Alias =
/// Mode` in the sibling answers `Mode`'s literals the way a local
/// `type Alias = Mode` does.
pub fn imported_string_literal_union_values(
    resolver: &dyn DeclTyResolver,
    module: &str,
    name: &str,
) -> Option<Vec<String>> {
    let (_, _, decl) = imported_decl_chain_end(resolver, module, name)?;
    match decl.body {
        Ty::StringLiteralUnion(values) => Some(values),
        _ => None,
    }
}

/// What the checker can read of a scrutinee type named from another module
/// (`imported_type_visibility`).
enum ImportedTypeVisibility {
    /// Not an imported type, or one whose declaration is in sight.
    Visible,
    /// Declared in a module this compile does not contain.
    Unseen { module: String, name: String },
    /// The module is in sight and exports no such name (E0105 at the import).
    NotExported,
}

/// The base a union type applies, seeing through one `Ty::App`. `Tree<string>`
/// and `Tree` both answer with the union's own type.
fn union_base(ty: &Ty) -> &Ty {
    split_type_app(ty).0
}

/// TypeScript's weak-type rule (TS2559), exactly as `tsc --strict` applies
/// it, established row by row against it: a declared record with at least
/// one field, every one of them optional, refuses a value that has at least
/// one property (or is a function, which has a call signature) and shares no
/// field name with it.
///
/// What it does not cover, as TypeScript does not: an empty record (`{}` has
/// no field, so it is not weak), a record with a required field (refused for
/// the missing field instead), a value with no field at all, and a value that
/// shares one name, whatever that field's type (the shared field is compared
/// on its own). An index signature would make a type not weak, and a Glyph
/// record has none.
fn weak_record_disjoint(found: &Ty, declared: &[RecordField]) -> bool {
    if declared.is_empty() || !declared.iter().all(|f| f.optional) {
        return false;
    }
    match found {
        Ty::Fn { .. } => true,
        Ty::Record { fields } => {
            !fields.is_empty() && !fields.iter().any(|f| declared.iter().any(|d| d.name == f.name))
        }
        _ => false,
    }
}

fn ty_is_decidable(ty: &Ty) -> bool {
    match ty {
        Ty::Unknown => false,
        Ty::Param { .. } => false,
        // An imported type is undecidable from the `Ty` alone: the identity
        // is here, the declaration is not, and without this arm the `_ =>
        // true` fallback would draw a false `QuestionOnNonResult` on `g()?`
        // where `g` returns an imported `type Res = Result<A, B>`. The
        // pairings that need the declaration (an imported union or record
        // against a primitive, or against another declaration) are decided
        // where it can be read, in `Assigner::imported_incompatible` (G215,
        // G226), before `definitely_incompatible` is asked.
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
    // Two literal sets join to their union, which is what makes an array
    // literal of mixed literals an `Array<"read" | "write">` and a
    // value-position `match` of literal arms the set of its arms (G237).
    // TypeScript reads both the same way.
    if let (Ty::StringLiteralUnion(x), Ty::StringLiteralUnion(y)) = (a, b) {
        let mut values = x.clone();
        for v in y {
            if !values.contains(v) {
                values.push(v.clone());
            }
        }
        return Ty::StringLiteralUnion(values);
    }
    // A literal joined with a `string` is a `string`: the set stops being
    // closed the moment one arm can produce anything.
    if matches!(
        (a, b),
        (Ty::StringLiteralUnion(_), Ty::Prim(Primitive::String))
            | (Ty::Prim(Primitive::String), Ty::StringLiteralUnion(_))
    ) {
        return Ty::Prim(Primitive::String);
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
///
/// One exception to "first wins": a `string` candidate supersedes a
/// string-literal one bound earlier. `array.fold(xs, "", fn(out: string, ..)
/// -> string { .. })` binds `A` from the `""` first, and TypeScript, which
/// takes every candidate, infers `string` from the two. Keeping `""` would
/// make the callback's `string` return a mismatch against a literal set, on a
/// program `tsc --strict` accepts.
fn collect_type_param_bindings(param: &Ty, arg: &Ty, out: &mut HashMap<Ident, Ty>) {
    match (param, arg) {
        (Ty::Param { name, .. }, concrete) if !concrete.is_unknown() => {
            let bound = out.entry(name.clone()).or_insert_with(|| concrete.clone());
            if matches!(bound, Ty::StringLiteralUnion(_))
                && matches!(concrete, Ty::Prim(Primitive::String))
            {
                *bound = concrete.clone();
            }
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

/// A written type as the source spells it, for a diagnostic that quotes the
/// program back (`Nullable<Option<int>>` says `Option<int>`, not
/// `Option<number>`). Every form is exact except a tagged union, which lists
/// its variant names without payloads; E0229 quotes a component parameter of
/// any form, a function type or `Nullable<{ name: string }>` included.
/// The callee of a call as it is written, for a diagnostic that names it:
/// `id`, `lib.id`, `json.parse`. Any other callee shape is shown as `callee`.
/// A receiver as the help can write it back: a name, a member path, or a
/// call (`load()`, with its arguments elided when it has any). Anything else
/// is written as a placeholder.
fn receiver_source(e: &Expr) -> String {
    match e {
        Expr::Ident { name, .. } => name.to_string(),
        Expr::Member { object, field, optional, .. } => {
            let dot = if *optional { "?." } else { "." };
            format!("{}{dot}{field}", receiver_source(object))
        }
        Expr::Call { callee, args, .. } => {
            let inner = if args.is_empty() { "" } else { "..." };
            format!("{}({inner})", receiver_source(callee))
        }
        _ => "<value>".to_string(),
    }
}

fn callee_source(callee: &Expr) -> String {
    match callee {
        Expr::Ident { name, .. } => name.to_string(),
        Expr::Member { object, field, .. } => match object.as_ref() {
            Expr::Ident { name, .. } => format!("{name}.{field}"),
            _ => field.to_string(),
        },
        _ => "callee".to_string(),
    }
}

/// The number of type parameters a prelude type has. The containers take
/// TypeScript's counts (`Result<T, E>`, `Record<K, V>`, `Array<T>`); every
/// other prelude type, the primitives and `Component` and `Issue` among them,
/// takes none. A prelude value or namespace is not a type and answers `None`.
/// The match lists every kind, so a container added to the prelude has to be
/// given its count here rather than inheriting a 0 that would refuse it.
fn prelude_type_param_count(kind: PreludeKind) -> Option<usize> {
    Some(match kind {
        PreludeKind::Result | PreludeKind::Record => 2,
        PreludeKind::Option
        | PreludeKind::Nullable
        | PreludeKind::Array
        | PreludeKind::Schema
        | PreludeKind::InferOutput => 1,
        PreludeKind::String
        | PreludeKind::Number
        | PreludeKind::Int
        | PreludeKind::BigInt
        | PreludeKind::Bool
        | PreludeKind::Void
        | PreludeKind::UnknownTop
        | PreludeKind::Never
        | PreludeKind::Component
        | PreludeKind::Issue => 0,
        PreludeKind::Ok
        | PreludeKind::Err
        | PreludeKind::Some
        | PreludeKind::None
        | PreludeKind::Par
        | PreludeKind::Print
        | PreludeKind::Assert => return None,
    })
}

fn type_expr_source(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Path { segments, .. } => segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("."),
        TypeExpr::Generic { base, args, .. } => format!(
            "{}<{}>",
            type_expr_source(base),
            args.iter().map(type_expr_source).collect::<Vec<_>>().join(", ")
        ),
        TypeExpr::Fn {
            params,
            return_ty,
            is_async,
            ..
        } => {
            let params = params
                .iter()
                .map(|p| match &p.name {
                    Some(name) => format!("{name}: {}", type_expr_source(&p.ty)),
                    None => type_expr_source(&p.ty),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let ret = return_ty
                .as_ref()
                .map(|r| format!(" -> {}", type_expr_source(r)))
                .unwrap_or_default();
            let prefix = if *is_async { "async " } else { "" };
            format!("{prefix}fn({params}){ret}")
        }
        TypeExpr::Record { fields, .. } if fields.is_empty() => "{}".to_string(),
        TypeExpr::Record { fields, .. } => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|f| {
                    let mark = if f.optional { "?" } else { "" };
                    format!("{}{mark}: {}", f.name, type_expr_source(&f.ty))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeExpr::Union { variants, .. } => variants
            .iter()
            .map(|v| v.name.to_string())
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::StringLiteralUnion { values, .. } => values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Extern { raw, .. } => format!("extern_ts(\"{raw}\")"),
        TypeExpr::TypeOf { path, .. } => format!(
            "typeof {}",
            path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".")
        ),
    }
}

/// Replace every `Ty::Param` named in `subst` with its bound type, walking
/// the type structurally. An empty substitution (the non-generic call case)
/// returns a clone unchanged, so this is a no-op for ordinary calls.
/// A `type` declaration's body read at an application of it: the body itself
/// for a non-generic declaration, the body with `args` substituted for
/// `generics` for a generic one applied with one argument per parameter, and
/// `None` for a generic one written bare or with the wrong count (E0233
/// refuses those), whose body says nothing about the parameters.
fn alias_body_applied(body: &Ty, generics: &[Ident], args: &[Ty]) -> Option<Ty> {
    if generics.is_empty() {
        return Some(body.clone());
    }
    if generics.len() != args.len() {
        return None;
    }
    let subst: HashMap<Ident, Ty> = generics.iter().cloned().zip(args.iter().cloned()).collect();
    Some(substitute_type_params(body, &subst))
}

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
        Ty::Fn {
            params,
            return_ty,
            is_async,
            type_params,
        } => Ty::Fn {
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
            type_params: type_params.clone(),
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
/// the points they already pushed them: the module-local checker escalates an
/// unknown head to E0220 inside its loop, the imported one collects them and
/// reports after. Which head shapes count as unknown is decided here, once, so
/// the two cannot drift apart on it again (G169).
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
    /// variant of a different union. Both spellings, `Loadign` and
    /// `Loadign(x)`, land here and both callers report E0220 for them.
    UnknownVariant { name: &'p Ident, span: Span },
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
                ArmCoverage::UnknownVariant { name, span: *span }
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

/// The E0220 suggestion for a head that names no variant (G305): the nearest
/// of the variants no other arm already covers. A covered one is left out
/// because writing it would give the match a second arm for that variant, an
/// arm that can never run (E0305). With every variant covered there is no
/// suggestion. A variant reached only through a nested sub-pattern (`A(X)`) is
/// not covered and stays a candidate: `A(y)` beside it can still run.
fn uncovered_nearest(
    name: &str,
    variants: &[Ident],
    covered: impl Fn(&Ident) -> bool,
) -> Option<String> {
    nearest_name(
        name,
        variants.iter().filter(|v| !covered(v)).map(|v| v.as_ref()),
    )
}

/// The nearest of `candidates` to `name` by Levenshtein distance, when one is
/// close enough to be a plausible typo; the first of several equally near. The
/// one threshold behind every `did you mean` hint, a variant head's (E0220)
/// and a record field's or a built-in member's (E0210), so they guess equally
/// far.
fn nearest_name<'c>(name: &str, candidates: impl Iterator<Item = &'c str>) -> Option<String> {
    let name_len = name.chars().count();
    // A short name tolerates fewer edits than a long one; at least 2 so a
    // single transposition (`Loadign` vs `Loading`, distance 2) still matches.
    let threshold = (name_len / 3).max(2);
    candidates
        .map(|v| (levenshtein(name, v), v))
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


/// True when an element passes React at least one child: an element, an
/// `{expr}`, or text that survives the emitter's whitespace rule. Text that
/// spans lines and holds only spaces and tabs is formatting and is dropped;
/// any other text, a lone space on one line included, is passed.
fn jsx_writes_children(children: &[JsxChild]) -> bool {
    children.iter().any(|c| match c {
        JsxChild::Element(_) | JsxChild::Expr(_) => true,
        JsxChild::Text { content, .. } => {
            let spans_lines = content.contains('\n') || content.contains('\r');
            content.chars().any(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\r'))
                || (!spans_lines && !content.is_empty())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::ParamOwner;
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

    /// Ask [`assignability`] about two type spellings read in the scope of
    /// `src`, by declaring each as a second name and lowering its body.
    fn assignability_of(src: &str, from: &str, to: &str) -> Assignability {
        let full = format!("{src}\ntype ProbeFrom = {from}\ntype ProbeTo = {to}\n");
        let m = glyph_parser::parse(&full).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let lowerer = Lowerer::new(&resolved, &prelude);
        let resolver = LocalDeclTy::new(&m, &lowerer);
        let body = |name: &str| {
            m.items
                .iter()
                .find_map(|d| match d {
                    Decl::Type(t) if t.name.as_ref() == name => Some(&t.body),
                    _ => None,
                })
                .expect("probe declaration")
        };
        let from_ty = lowerer.lower(body("ProbeFrom"));
        let to_ty = lowerer.lower(body("ProbeTo"));
        assignability(&m, &resolved, &prelude, &resolver, &from_ty, &to_ty)
    }

    const UNION: &str = "module m\ntype Order = Paid | Shipped\n";

    /// G216's pairing, asked as a question about two types rather than about a
    /// program: the container is refused where the scalar is declared, and the
    /// scalar is accepted where the container's argument accepts it.
    #[test]
    fn assignability_decides_the_nullable_pairing_both_ways() {
        assert_eq!(
            assignability_of("module m\n", "Nullable<int>", "int"),
            Assignability::Incompatible
        );
        assert_eq!(
            assignability_of("module m\n", "string", "Nullable<int>"),
            Assignability::Incompatible
        );
        // `Nullable<int>` accepts an `int`, and the acceptance is the inner
        // pairing's, not the absence of a refusal.
        assert!(matches!(
            assignability_of("module m\n", "int", "Nullable<int>"),
            Assignability::Compatible { .. }
        ));
        // Nothing is claimed one level in where the inner pairing is not
        // decided either. `type T = { }` is the body the relation compares a
        // primitive against by nothing, because tsc lets a `string` satisfy
        // the empty object type.
        assert!(matches!(
            assignability_of("module m\ntype Empty = { }\n", "string", "Nullable<Empty>"),
            Assignability::NoRule
        ));
    }

    /// The rule that outranks the rest: a pairing no rule reads comes back as
    /// no rule, never as an acceptance. Two function types are the shape of
    /// it, because the relation compares the returns and skips the parameters.
    #[test]
    fn assignability_reports_no_rule_where_the_relation_only_stayed_silent() {
        assert!(matches!(
            assignability_of("module m\n", "fn(string) -> bool", "fn(number) -> bool"),
            Assignability::NoRule
        ));
        // A declared union where a prelude container is declared was the
        // second shape here until G293 applied D49 to it.
        assert_eq!(
            assignability_of(UNION, "Order", "Array<string>"),
            Assignability::Incompatible
        );
    }

    /// The `Compatible` half is claimed only from a total rule, and each of
    /// the ones that are total is exercised here.
    #[test]
    fn assignability_accepts_only_from_a_total_rule() {
        for (from, to) in [
            ("string", "string"),
            ("int", "number"),
            ("Order", "Order"),
            ("Array<string>", "Array<string>"),
            ("{ id: string, extra: bool }", "{ id: string }"),
            ("never", "string"),
            ("string", "unknown"),
        ] {
            assert!(
                matches!(
                    assignability_of(UNION, from, to),
                    Assignability::Compatible { .. }
                ),
                "`{from}` into `{to}` was not accepted"
            );
        }
        // A record missing a required field is refused, not accepted.
        assert_eq!(
            assignability_of(UNION, "{ id: string }", "{ id: string, n: number }"),
            Assignability::Incompatible
        );
    }

    /// `optional` is read on both sides of the record rule.
    ///
    /// A value whose field is optional where the declaration requires one is
    /// `TS2345` under `tsc`, and the relation refuses it too since G274; it
    /// drew nothing before, which is why this answered `NoRule` then. The
    /// tell that the oldest rule was wrong is that it was symmetric in a flag
    /// TypeScript is not: both directions came back accepted.
    #[test]
    fn an_optional_value_field_is_not_accepted_where_a_required_one_is_declared() {
        assert_eq!(
            assignability_of(UNION, "{ a?: string }", "{ a: string }"),
            Assignability::Incompatible
        );
        // The other direction is TypeScript's own: a value that always
        // carries the field satisfies a declaration that may go without it.
        assert!(matches!(
            assignability_of(UNION, "{ a: string }", "{ a?: string }"),
            Assignability::Compatible { .. }
        ));
        // Optional on both sides is accepted, and so is required on both.
        assert!(matches!(
            assignability_of(UNION, "{ a?: string }", "{ a?: string }"),
            Assignability::Compatible { .. }
        ));
        // The rule reads nested records the same way, because the field's own
        // pairing is asked through this relation.
        assert_eq!(
            assignability_of(UNION, "{ r: { a?: string } }", "{ r: { a: string } }"),
            Assignability::Incompatible
        );
    }

    fn errors_of(src: &str) -> Vec<TypeError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        ty_errs
    }

    /// G39. A module-level `const` carrying a type annotation is that type, so
    /// what reads a receiver's type is checked against it. It used to be
    /// `Ty::Unknown`, so a field typo and a wrong argument were both silent at
    /// a `const`-rooted expression while the same program written with an
    /// annotated `let` inside the function reported both. Which binding form
    /// the author reached for is not allowed to decide whether a guarantee
    /// holds.
    ///
    /// Exhaustiveness was already fine: the coverage check resolves a union
    /// from the arm heads, not from the scrutinee's type, so `match M` over a
    /// `const` of a union reported E0200 before this too.
    #[test]
    fn an_annotated_module_const_is_checked_like_an_annotated_let() {
        let errs = errors_of(
            "module x\n\
             type Sheet = { rows: number, cols: number }\n\
             const ORIGIN: Sheet = { rows: 0, cols: 0 }\n\
             fn f() -> number {\n\
             \x20 return ORIGIN.rowz\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::UnknownField { field, type_name, .. }
                    if field == "rowz" && type_name == "Sheet"
            )),
            "a field typo on an annotated const is E0210: {errs:?}"
        );
    }

    /// The same rule reaching the argument position: a `const` typed `number`
    /// passed where a `string` is declared is E0211, which is what the
    /// annotated-`let` spelling has always reported.
    #[test]
    fn an_annotated_module_const_is_checked_at_an_argument_position() {
        let errs = errors_of(
            "module x\n\
             const N: number = 1\n\
             fn g(s: string) -> string { return s }\n\
             fn f() -> string {\n\
             \x20 return g(N)\n\
             }\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "a wrong-typed const argument is E0211: {errs:?}"
        );
    }

    /// A `const` with no annotation stays `Ty::Unknown`. Inferring its type
    /// from the initializer is a separate question, and guessing one here
    /// would start reporting against a type the author never wrote.
    #[test]
    fn an_unannotated_module_const_stays_unchecked() {
        let errs = errors_of(
            "module x\n\
             const ORIGIN = { rows: 0, cols: 0 }\n\
             fn f() -> number {\n\
             \x20 return ORIGIN.rowz\n\
             }\n",
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, TypeError::UnknownField { .. })),
            "no annotation, no claim: {errs:?}"
        );
    }

    /// An annotated module `const` gets the check an annotated `let` gets: the
    /// initializer is judged against the annotation, under the same code. It
    /// used to be silent, so `const X: number = "hi"` was caught only by `tsc`
    /// (TS2322) while `let x: number = "hi"` inside a function was E0204.
    #[test]
    fn an_annotated_module_const_initializer_is_checked_against_its_annotation() {
        let errs = errors_of(
            "module x\n\
             const X: number = \"hi\"\n\
             fn f() -> number {\n\
             \x20 return X\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::TypeMismatch { expected, found, .. }
                    if expected == "number" && found == "string"
            )),
            "a const initializer that disagrees with its annotation is E0204: {errs:?}"
        );
    }

    /// The check judges only a provable mismatch. A fitting primitive, a record
    /// literal against a declared record (structural, undecided by name) and a
    /// literal against a string-literal union (D30) all stay silent.
    #[test]
    fn an_annotated_module_const_with_a_fitting_initializer_stays_silent() {
        let errs = errors_of(
            "module x\n\
             type Sheet = { rows: number, cols: number }\n\
             type Tier = \"free\" | \"pro\"\n\
             const N: number = 1\n\
             const ORIGIN: Sheet = { rows: 0, cols: 0 }\n\
             const T: Tier = \"free\"\n\
             fn f() -> number {\n\
             \x20 return N\n\
             }\n",
        );
        assert!(errs.is_empty(), "a fitting initializer draws nothing: {errs:?}");
    }

    /// The two rules meet at a const: a primitive initializer for a const
    /// annotated with a declared union is the reverse direction of G201 at the
    /// const position.
    #[test]
    fn a_primitive_initializer_for_a_const_of_a_declared_union_is_flagged() {
        let errs = errors_of(
            "module x\n\
             type PaymentResult =\n\
             \x20 | Settled\n\
             \x20 | Declined\n\
             const R: PaymentResult = \"Settled\"\n\
             fn f() -> number {\n\
             \x20 return 1\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::TypeMismatch { expected, found, .. }
                    if expected == "PaymentResult" && found == "string"
            )),
            "errs: {errs:?}"
        );
    }

    /// No annotation, nothing to disagree with.
    #[test]
    fn an_unannotated_module_const_initializer_is_not_judged() {
        let errs = errors_of(
            "module x\n\
             const X = \"hi\"\n\
             fn f() -> string {\n\
             \x20 return X\n\
             }\n",
        );
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "no annotation, no claim: {errs:?}"
        );
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
    fn a_match_over_next_line_without_an_err_arm_is_non_exhaustive() {
        // G105. `fs.next_line` answers `Result<Option<string>, FsError>` rather
        // than `Option<string>` so that a read error at line 400,000 is a value
        // the caller must handle and not an end of input. That only holds if the
        // checker knows the shape: an unmodeled stdlib function types `Unknown`,
        // its `match` is unchecked, and the two `Ok` arms alone would build.
        let two_ok_arms = "module x\n\
             import std/fs\n\
             fn head(r: fs.LineReader) -> string {\n\
             \x20 return match fs.next_line(r) {\n\
             \x20\x20\x20 Ok(Some(line)) => line,\n\
             \x20\x20\x20 Ok(None) => \"\",\n\
             \x20 }\n\
             }\n";
        let errs = errors_of(two_ok_arms);
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })),
            "a `match` over `fs.next_line` with no `Err` arm must be E0200: {errs:?}"
        );

        // With the `Err` arm the match is complete, and the reader is closed
        // on the way out of the `Ok(None)` arm by the runtime, not the checker.
        let with_err = two_ok_arms.replace(
            "    Ok(None) => \"\",\n",
            "    Ok(None) => \"\",\n    Err(e) => e.message,\n",
        );
        assert_ne!(two_ok_arms, with_err, "the replacement must have applied");
        let errs = errors_of(&with_err);
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })),
            "three arms cover Result<Option<string>, FsError>: {errs:?}"
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

    /// A written string literal carries the one-literal type it spells
    /// (G237), which is what `tsc` reads it as; the `let` that binds it widens
    /// that back to `string`, which is also what `tsc` does, since the binding
    /// emits a TypeScript `let`.
    #[test]
    fn string_literal_typed() {
        let (m, _, tm) = type_map_of(r#"module x
fn main() { let x = "hi" }
"#);
        assert_eq!(
            tm.get(first_let_value_span(&m)),
            &Ty::StringLiteralUnion(vec!["hi".to_string()])
        );
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
        // When the initializer types as Unknown (a method read here; `length`
        // is a `number` since 0.1.132, but methods are not typed), the
        // binding records nothing and refs stay Unknown — no false pinning.
        let src = r#"module x
fn main(s: string) -> number {
  let n = s.trim
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
component View(props: { s: Status }) -> Component {
  return <match value={props.s}>
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
        // The complement: every variant has a `<case>` (`Done(n)` binds the
        // whole payload and covers the variant), so no exhaustiveness error.
        let src = r#"module x
type Status = | Idle | Loading | Done(number)
component View(props: { s: Status }) -> Component {
  return <match value={props.s}>
    <case Idle><span>idle</span></case>
    <case Loading><span>loading</span></case>
    <case Done(n)><span>{n}</span></case>
  </match>
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    /// D44 through a `<case>`: a case whose pattern tests a field can fail,
    /// so on its own it does not cover the variant, exactly as the arm
    /// `Done({ status: 404 })` does not. A sibling case taking the rest of
    /// `Done` closes it.
    #[test]
    fn a_refutable_jsx_case_alone_does_not_cover_its_variant() {
        let open = r#"module x
type Resp = | Pending | Done({ status: number, body: string })
component View(props: { r: Resp }) -> Component {
  return <match value={props.r}>
    <case Pending><p>wait</p></case>
    <case Done({ status: 404 })><p>missing</p></case>
  </match>
}
"#;
        let errs = ty_errors_of(open);
        assert!(
            matches!(&errs[..], [TypeError::NonExhaustiveMatch { missing, .. }] if missing.contains("Done")),
            "{errs:?}"
        );
        let closed = open.replace(
            "    <case Done({ status: 404 })><p>missing</p></case>\n",
            "    <case Done({ status: 404 })><p>missing</p></case>\n    <case Done({ body })><p>{body}</p></case>\n",
        );
        assert!(ty_errors_of(&closed).is_empty(), "{:?}", ty_errors_of(&closed));
    }

    /// A case's bindings are typed by the arm's payload binding, so a use of
    /// one is checked: the positional payload is an `Array<string>`, and
    /// passing it where a `number` is wanted is E0211 naming that type. Under
    /// `bind` the name had no type and nothing was checked.
    #[test]
    fn a_jsx_case_binding_has_the_payload_type() {
        let src = r#"module x
type Load = | Loading | Loaded(Array<string>)
fn takes_number(n: number) -> string {
  return "x"
}
component View(props: { s: Load }) -> Component {
  return <match value={props.s}>
    <case Loading><p>wait</p></case>
    <case Loaded(items)><p>{takes_number(items)}</p></case>
  </match>
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(&errs[..], [TypeError::ArgumentTypeMismatch { found, .. }] if found == "Array<string>"),
            "{errs:?}"
        );
    }

    /// G279, for an arm and a case alike: a record pattern naming a field the
    /// payload record does not declare is E0210 at the field, carrying the
    /// record's fields; the whole-payload binding's member access draws the
    /// same E0210.
    #[test]
    fn a_record_pattern_naming_a_missing_field_is_e0210_in_an_arm_and_a_case() {
        let decl = "module x\ntype Load = | Loading | Loaded({ users: Array<string>, total: number })\n";
        let arm = format!(
            "{decl}fn f(l: Load) -> number {{\n  return match l {{\n    Loading => 0,\n    Loaded({{ totl }}) => 1,\n  }}\n}}\n"
        );
        let case = format!(
            "{decl}component V(props: {{ s: Load }}) -> Component {{\n  return <match value={{props.s}}>\n    <case Loading><p>a</p></case>\n    <case Loaded({{ usres }})><p>b</p></case>\n  </match>\n}}\n"
        );
        let whole = format!(
            "{decl}component V(props: {{ s: Load }}) -> Component {{\n  return <match value={{props.s}}>\n    <case Loading><p>a</p></case>\n    <case Loaded(r)><p>{{r.totl}}</p></case>\n  </match>\n}}\n"
        );
        for (src, field) in [(&arm, "totl"), (&case, "usres"), (&whole, "totl")] {
            let errs = ty_errors_of(src);
            match &errs[..] {
                [TypeError::UnknownField { field: f, type_name, fields, .. }] => {
                    assert_eq!(f, field);
                    assert_eq!(type_name, "{ users: Array<string>, total: number }");
                    assert_eq!(fields, &["users".to_string(), "total".to_string()]);
                }
                other => panic!("expected one E0210 for `{field}`, got {other:?}"),
            }
        }
        let good = arm.replace("Loaded({ totl }) => 1", "Loaded({ total }) => total");
        assert!(ty_errors_of(&good).is_empty(), "{:?}", ty_errors_of(&good));
    }

    /// G279's other half: an object pattern over a payload that is not a
    /// record is refused in an arm and in a case, naming the payload's type.
    #[test]
    fn an_object_pattern_over_a_positional_payload_is_refused() {
        let decl = "module x\ntype Load = | Loading | Loaded(Array<string>)\n";
        let arm = format!(
            "{decl}fn f(l: Load) -> number {{\n  return match l {{\n    Loading => 0,\n    Loaded({{ users }}) => 1,\n  }}\n}}\n"
        );
        let case = format!(
            "{decl}component V(props: {{ s: Load }}) -> Component {{\n  return <match value={{props.s}}>\n    <case Loading><p>a</p></case>\n    <case Loaded({{ users }})><p>b</p></case>\n  </match>\n}}\n"
        );
        for src in [&arm, &case] {
            let errs = ty_errors_of(src);
            assert!(
                matches!(&errs[..], [e @ TypeError::PatternNotRecord { ty, .. }] if ty == "Array<string>" && e.code() == "E0204"),
                "{errs:?}"
            );
        }
    }

    // ----- G300: a field named with a keyword is read like any other -----

    /// `type Event = { type: string }` declares a field the parser now lets
    /// the program read (`e.type`). The read is typed by the declaration, a
    /// misuse of it is the ordinary mismatch, and a misspelling, keyword or
    /// not, is the ordinary E0210 listing the keyword field.
    #[test]
    fn a_keyword_named_field_is_read_and_checked() {
        let decl = "module m\ntype Event = { type: string, id: number }\n";
        let ok = format!("{decl}pub fn f(e: Event) -> string {{\n  return e.type\n}}\n");
        assert!(ty_errors_of(&ok).is_empty(), "{:?}", ty_errors_of(&ok));

        let mismatch = format!("{decl}pub fn f(e: Event) -> number {{\n  let n: number = e.type\n  return n\n}}\n");
        let errs = ty_errors_of(&mismatch);
        assert!(
            matches!(&errs[..], [e @ TypeError::TypeMismatch { .. }] if e.code() == "E0204"),
            "{errs:?}"
        );

        for wrong in ["typ", "match"] {
            let src = format!("{decl}pub fn f(e: Event) -> string {{\n  return e.{wrong}\n}}\n");
            let errs = ty_errors_of(&src);
            match &errs[..] {
                [e @ TypeError::UnknownField { field, type_name, fields, .. }] => {
                    assert_eq!(e.code(), "E0210");
                    assert_eq!((field.as_str(), type_name.as_str()), (wrong, "Event"));
                    assert!(fields.iter().any(|f| f == "type"), "{fields:?}");
                }
                other => panic!("e.{wrong}: expected one E0210, got {other:?}"),
            }
        }
    }

    // ----- G288: a member read on a built-in receiver is checked -----

    /// The one E0210 a test expects, with the fields a built-in receiver
    /// fills in.
    fn the_member_error(src: &str) -> (String, String, Vec<String>, Option<BuiltinReceiver>, Option<String>, TypeError) {
        let errs = ty_errors_of(src);
        match &errs[..] {
            [e @ TypeError::UnknownField { field, type_name, fields, builtin, suggestion, record, .. }] => {
                assert!(record.is_none(), "a built-in type has no declaration: {e:?}");
                (
                    field.clone(),
                    type_name.clone(),
                    fields.clone(),
                    *builtin,
                    suggestion.clone(),
                    e.clone(),
                )
            }
            other => panic!("expected one E0210, got {other:?}"),
        }
    }

    /// The reproduction: `xs.lenght` on an `Array<string>` passed every Glyph
    /// stage and was left to `tsc`. It is E0210 naming the array type, listing
    /// the members, and pointing at the one that was meant.
    #[test]
    fn a_member_typo_on_an_array_is_e0210_naming_the_array_type() {
        let src = "module m\npub fn f(xs: Array<string>) -> number {\n  return xs.lenght\n}\n";
        let (field, type_name, fields, builtin, suggestion, e) = the_member_error(src);
        assert_eq!(field, "lenght");
        assert_eq!(type_name, "Array<string>");
        assert_eq!(builtin, Some(BuiltinReceiver::Array));
        assert_eq!(suggestion.as_deref(), Some("length"));
        assert!(fields.iter().any(|f| f == "length") && fields.iter().any(|f| f == "push"), "{fields:?}");
        assert_eq!(e.code(), "E0210");
        assert_eq!(
            e.to_string(),
            "type `Array<string>` has no field `lenght`; did you mean `length`?"
        );
        assert_eq!(e.alternatives_kind(), Some(AlternativesKind::Members));
        let help = e.help().expect("help").into_owned();
        assert!(help.contains("JavaScript array") && help.contains("std/array"), "{help}");
        assert!(!help.contains("add the field"), "there is no declaration to add to: {help}");
    }

    /// A string is the same gap through the same arm: `s.lenght` on a
    /// `string` is E0210 naming `string`.
    #[test]
    fn a_member_typo_on_a_string_is_e0210_naming_string() {
        let src = "module m\npub fn f(s: string) -> number {\n  return s.lenght\n}\n";
        let (field, type_name, _, builtin, suggestion, e) = the_member_error(src);
        assert_eq!((field.as_str(), type_name.as_str()), ("lenght", "string"));
        assert_eq!(builtin, Some(BuiltinReceiver::String));
        assert_eq!(suggestion.as_deref(), Some("length"));
        assert!(e.help().expect("help").contains("std/string"));
    }

    /// The callee of a method call and of a `mut` method statement goes
    /// through the same member read, so a misspelled method is caught too.
    #[test]
    fn a_misspelled_array_method_is_e0210() {
        let src = "module m\npub fn f(xs: Array<number>) -> number {\n  mut xs.pushh(1)\n  return 0\n}\n";
        let (field, type_name, _, _, suggestion, _) = the_member_error(src);
        assert_eq!((field.as_str(), type_name.as_str()), ("pushh", "Array<number>"));
        assert_eq!(suggestion.as_deref(), Some("push"));
        let src = "module m\npub fn f(s: string) -> string {\n  return s.toUppercase()\n}\n";
        let (field, _, _, _, suggestion, _) = the_member_error(src);
        assert_eq!(field, "toUppercase");
        assert_eq!(suggestion.as_deref(), Some("toUpperCase"));
    }

    /// What Glyph accepted before still compiles: every member a JavaScript
    /// array or string has, as a read, a method call or a `mut` statement,
    /// on a string-literal union and in a refinement's predicate as well.
    #[test]
    fn real_members_of_arrays_and_strings_stay_legal() {
        let src = r#"module m
type Mode = "read" | "write"
type Name = string where value.length > 0
pub fn f(xs: Array<string>, s: string, m: Mode, n: Name) -> number {
  mut xs.push("a")
  let joined = xs.map(fn(x: string) -> string { return x.trim() }).join(",")
  let up = s.toUpperCase()
  let sorted = xs.toSorted()
  let own = xs.hasOwnProperty("0")
  return xs.length + s.length + m.length + n.length + joined.length
}
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    // ----- G312: built-in methods are typed, and an uncalled one is refused -----

    /// A method of a JavaScript array, string or number read without being
    /// called is E0234. `let up = s.toUpperCase` followed by `up()` passes
    /// `tsc --strict` and throws a TypeError at run time, because an ES
    /// module is strict and the detached method runs with `this` undefined.
    /// If this regresses, that crash compiles clean under both checkers
    /// again, and the only report is the user's stack trace.
    #[test]
    fn a_builtin_method_read_without_a_call_is_e0234() {
        let cases = [
            (
                "module m\npub fn f(s: string) -> string {\n  let up = s.toUpperCase\n  return up()\n}\n",
                "toUpperCase",
                "string",
                "std/string",
            ),
            (
                "module m\npub fn f(xs: Array<number>) -> number {\n  let find = xs.indexOf\n  return find(2)\n}\n",
                "indexOf",
                "Array<number>",
                "std/array",
            ),
            (
                "module m\npub fn f(n: number) -> string {\n  let fmt = n.toFixed\n  return fmt(2)\n}\n",
                "toFixed",
                "number",
                "std/math",
            ),
            // Handed on as a value is the same detached read.
            (
                "module m\npub fn f(xs: Array<string>, s: string) -> Array<string> {\n  return xs.map(s.concat)\n}\n",
                "concat",
                "string",
                "std/string",
            ),
        ];
        for (src, method, type_name, module) in cases {
            let errs = ty_errors_of(src);
            let hits: Vec<&TypeError> = errs.iter().filter(|e| e.code() == "E0234").collect();
            assert_eq!(hits.len(), 1, "{method}: expected one E0234, got {errs:?}");
            let e = hits[0];
            assert_eq!(
                e.to_string(),
                format!("method `{method}` of `{type_name}` is read without being called"),
            );
            let help = e.help().expect("help").into_owned();
            assert!(help.contains(&format!("{method}(")), "{help}");
            assert!(help.contains(module), "{help}");
        }
    }

    /// Record 88, 4c: `.bind`, `.call` and `.apply` stay refused, since what
    /// they return is untyped in Glyph and a lambda already says the same
    /// thing checked. The help names `.bind` and `.call` and writes the
    /// lambda out when the method takes no argument; otherwise it names the
    /// call the lambda wraps.
    #[test]
    fn the_e0234_help_names_bind_and_call_and_writes_the_lambda() {
        let help_of = |src: &str| -> String {
            let errs = ty_errors_of(src);
            let e = errs.iter().find(|e| e.code() == "E0234").unwrap_or_else(|| panic!("{errs:?}"));
            e.help().expect("help").into_owned()
        };
        assert_eq!(
            help_of("module m\npub fn f(s: string) -> string {\n  let up = s.toUpperCase.bind(s)\n  return up()\n}\n"),
            "Call the method where it is read, `s.toUpperCase()`. To hand it on as a value, wrap the call in a lambda, `fn() -> string { return s.toUpperCase() }`; `.bind` and `.call` are refused like the read itself, because what they return is untyped in Glyph. `std/string` holds Glyph's own functions over a JavaScript string, which can be passed as values."
        );
        assert_eq!(
            help_of("module m\npub fn f(xs: Array<number>) -> number {\n  let find = xs.indexOf\n  return find(2)\n}\n"),
            "Call the method where it is read, `xs.indexOf(...)`. To hand it on as a value, wrap the call in a lambda that calls `xs.indexOf(...)`; `.bind` and `.call` are refused like the read itself, because what they return is untyped in Glyph. `std/array` holds Glyph's own functions over a JavaScript array, which can be passed as values."
        );
    }

    /// The callee is tracked by span, not by a flag: in `s.trim.bind(s)()`
    /// the callee is `s.trim.bind(s)`, and the `s.trim` inside it is a read
    /// that is not called, so it is E0234. A flag set for the whole callee
    /// walk would take it for a call.
    #[test]
    fn an_uncalled_method_inside_a_callee_is_e0234() {
        let errs = ty_errors_of(
            "module m\npub fn f(s: string) -> string {\n  return s.trim.bind(s)()\n}\n",
        );
        let hits: Vec<String> =
            errs.iter().filter(|e| e.code() == "E0234").map(|e| e.to_string()).collect();
        assert_eq!(
            hits,
            vec!["method `trim` of `string` is read without being called".to_string()],
            "{errs:?}"
        );
    }

    /// The other side of E0234: a method that is called is never refused for
    /// the form, as a call, a `mut` statement, a callee in a chain, an
    /// argument or a JSX child, and a data property read alone stays legal.
    /// AGENTS.md documents the method form, so refusing a call would break
    /// documented programs.
    #[test]
    fn a_called_builtin_method_is_never_refused() {
        let src = r#"module m
pub fn f(xs: Array<number>, s: string, n: number) -> number {
  mut xs.push(1)
  let up: string = s.toUpperCase()
  let at: number = xs.indexOf(2)
  let has: bool = xs.includes(3)
  let joined: string = xs.join(",")
  let tail: Array<number> = xs.slice(1)
  let fixed: string = n.toFixed(2)
  let trimmed: number = s.trim().length
  let doubled = xs.map(fn(x: number) -> number { return x * 2 })
  let count = s.split(",").length
  print(s.padStart(4, " "))
  return xs.length + at + trimmed + count + up.length + joined.length + tail.length + fixed.length
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// A called method has the type TypeScript's library declares for it,
    /// read through the receiver's element type, and its arguments are
    /// checked against its parameters. Before this every built-in method
    /// call was `Unknown`, so `let n: string = xs.indexOf(2)` passed
    /// `glyph check --no-tsc`, the editor and the playground, and only `tsc`
    /// refused it (TS2322). If this regresses, the mistake is visible again
    /// only to a build that runs `tsc`.
    #[test]
    fn a_builtin_method_call_has_the_type_typescript_declares() {
        let src = r#"module m
pub fn f(xs: Array<number>, s: string, n: number) -> number {
  let a: string = xs.indexOf(2)
  let b: number = s.trim()
  let c: string = xs.slice(1)
  let d: number = n.toFixed(2)
  let e: string = xs.includes(1)
  return 0
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(
            g289_mismatches(&errs),
            vec![
                ("string".to_string(), "number".to_string()),
                ("number".to_string(), "string".to_string()),
                ("string".to_string(), "Array<number>".to_string()),
                ("number".to_string(), "string".to_string()),
                ("string".to_string(), "bool".to_string()),
            ],
            "{errs:?}"
        );

        // A parameter is the receiver's element type, and the count is
        // TypeScript's: `indexOf` needs its search element, and its
        // `fromIndex` may be left off.
        let src = r#"module m
pub fn f(xs: Array<number>) -> number {
  let a = xs.indexOf("two")
  let b = xs.indexOf()
  let c = xs.indexOf(2, 1)
  return 0
}
"#;
        let errs = ty_errors_of(src);
        let wrong: Vec<String> = argument_mismatches(&errs).into_iter().map(|(e, _)| e).collect();
        assert_eq!(wrong, vec!["number".to_string()], "{errs:?}");
        let counts: Vec<&str> = errs.iter().map(|e| e.code()).filter(|c| *c == "E0213").collect();
        assert_eq!(counts, vec!["E0213"], "{errs:?}");
    }

    /// `length` is the one data property an array and a string have (every
    /// other member is a method), and it is a `number`. It used to read as
    /// `Unknown`, so `let a: string = xs.length` passed `--no-tsc` and was
    /// left to `tsc` (TS2322). The receiver is reached the way the member
    /// check reaches it: an array, a string, a string-literal union, and an
    /// alias of each.
    #[test]
    fn length_on_an_array_or_a_string_is_a_number() {
        let src = r#"module m
type Mode = "read" | "write"
type Rows = Array<string>
pub fn f(xs: Array<string>, s: string, m: Mode, r: Rows) -> number {
  let a: string = xs.length
  let b: string = s.length
  let c: bool = m.length
  let d: string = r.length
  return 0
}
"#;
        let errs = ty_errors_of(src);
        assert_eq!(
            g289_mismatches(&errs),
            vec![
                ("string".to_string(), "number".to_string()),
                ("string".to_string(), "number".to_string()),
                ("bool".to_string(), "number".to_string()),
                ("string".to_string(), "number".to_string()),
            ],
            "{errs:?}"
        );
    }

    /// A `number` is what `length` is, so it goes wherever a `number` or an
    /// `int` (D31) is wanted. A method read without a call, which this test
    /// once held `Unknown`, is E0234 since G312; called, it has its type.
    #[test]
    fn length_goes_where_a_number_or_an_int_goes() {
        let src = r#"module m
fn want(n: int) -> int {
  return n
}
pub fn f(xs: Array<string>, s: string) -> int {
  let a: number = xs.length
  let b: int = s.length
  let c = want(xs.length)
  let up: string = s.toUpperCase()
  return s.length
}
"#;
        let errs = ty_errors_of(src);
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// A `match` over `xs.length` is a match over a `number`: the arms are
    /// literals and the rest of the numbers is uncovered, so a missing `else`
    /// is E0218, as it is over any other `number`.
    #[test]
    fn a_match_over_length_without_else_is_e0218() {
        let src = r#"module m
pub fn f(xs: Array<string>) -> string {
  return match xs.length {
    0 => "none",
    1 => "one",
  }
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            matches!(errs.as_slice(), [TypeError::NonExhaustiveValueMatch { .. }]),
            "{errs:?}"
        );
    }

    /// An alias reaches the array or string it names, generic or not, and so
    /// does a refinement of `string`; the error names the type as written.
    #[test]
    fn a_member_typo_through_an_alias_is_e0210() {
        for (decl, param, shown) in [
            ("type Rows = Array<string>", "Rows", "Rows"),
            ("type Rows = Array<string>\ntype Again = Rows", "Again", "Again"),
            ("type Box<T> = Array<T>", "Box<number>", "Box<number>"),
            ("type Name = string where value.length > 0", "Name", "Name"),
            ("type Mode = \"read\" | \"write\"", "Mode", "Mode"),
        ] {
            let src = format!("module m\n{decl}\npub fn f(r: {param}) -> number {{\n  return r.lenght\n}}\n");
            let (field, type_name, _, _, suggestion, _) = the_member_error(&src);
            assert_eq!((field.as_str(), type_name.as_str()), ("lenght", shown), "{decl}");
            assert_eq!(suggestion.as_deref(), Some("length"), "{decl}");
        }
    }

    /// Where the receiver is not decidably an array or a string the read
    /// stays as permissive as before: a generic parameter, a generic alias
    /// whose body is its argument, a record that declares the name.
    #[test]
    fn an_undecidable_receiver_is_still_left_alone() {
        let src = r#"module m
type Id<T> = T
type Shaped = { lenght: number }
fn a<T>(x: T) -> number { return x.lenght }
fn b(x: Id<Unknownish>) -> number { return x.lenght }
fn c(x: Shaped) -> number { return x.lenght }
type Unknownish = extern_ts("any")
"#;
        assert!(ty_errors_of(src).is_empty(), "{:?}", ty_errors_of(src));
    }

    /// A record's field typo gets the same "did you mean" an array member
    /// does, on each of the three sites that raise E0210 for a record: a
    /// field read, a JSX attribute against a props record, a record pattern.
    /// A name nothing is near gets none, as on an array.
    #[test]
    fn a_record_field_typo_suggests_the_nearest_field() {
        let decl = "module x\ntype Order = { total: number, items: Array<string> }\n";
        let read = format!("{decl}pub fn f(o: Order) -> number {{\n  return o.totl\n}}\n");
        let attr = format!(
            "{decl}component Row(props: Order) -> Component {{\n  return <p>a</p>\n}}\ncomponent V() -> Component {{\n  return <Row totl={{1}} total={{1}} items={{[]}} />\n}}\n"
        );
        let pat = "module x\ntype Load = | Loading | Loaded({ users: Array<string>, total: number })\nfn f(l: Load) -> number {\n  return match l {\n    Loading => 0,\n    Loaded({ totl }) => 1,\n  }\n}\n".to_string();
        for src in [&read, &attr, &pat] {
            let errs = ty_errors_of(src);
            let e = errs
                .iter()
                .find(|e| matches!(e, TypeError::UnknownField { .. }))
                .unwrap_or_else(|| panic!("no E0210 in {errs:?}"));
            let TypeError::UnknownField { suggestion, builtin, .. } = e else { unreachable!() };
            assert_eq!(suggestion.as_deref(), Some("total"), "{src}");
            assert_eq!(*builtin, None);
            assert!(e.to_string().ends_with("; did you mean `total`?"), "{e}");
        }
        let far = format!("{decl}pub fn f(o: Order) -> number {{\n  return o.customer\n}}\n");
        let errs = ty_errors_of(&far);
        assert!(
            matches!(&errs[..], [TypeError::UnknownField { suggestion: None, .. }]),
            "{errs:?}"
        );
    }

    /// A `number` is a JavaScript number, whose members TypeScript's library
    /// fixes the same way, so `n.toFixd(2)` is E0210 naming `number` and
    /// pointing at `toFixed`, where it was left to `tsc`.
    #[test]
    fn a_member_typo_on_a_number_is_e0210_naming_number() {
        let src = "module m\npub fn f(n: number) -> string {\n  return n.toFixd(2)\n}\n";
        let (field, type_name, fields, builtin, suggestion, e) = the_member_error(src);
        assert_eq!((field.as_str(), type_name.as_str()), ("toFixd", "number"));
        assert_eq!(builtin, Some(BuiltinReceiver::Number));
        assert_eq!(suggestion.as_deref(), Some("toFixed"));
        assert!(fields.iter().any(|f| f == "toPrecision"), "{fields:?}");
        let help = e.help().expect("help").into_owned();
        assert!(help.contains("JavaScript number") && help.contains("std/math"), "{help}");
    }

    /// Every member a JavaScript number has stays legal, on `number` and on
    /// `int`, which the checker holds as a `number` and which emits as one.
    #[test]
    fn real_members_of_numbers_stay_legal() {
        let src = r#"module m
pub fn f(n: number, i: int) -> string {
  let a = n.toFixed(2)
  let b = n.toPrecision(3)
  let c = n.toExponential(1)
  let d = i.toString()
  let e = n.toLocaleString()
  let own = n.hasOwnProperty("x")
  return a
}
"#;
        let errs: Vec<_> = ty_errors_of(src);
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// A `<case Loaded(items)>` binding is the payload's `Array<string>`
    /// (G275), so `{items.foo}` inside it is E0210 rather than a `tsc` error.
    #[test]
    fn a_member_typo_on_a_jsx_case_binding_is_e0210() {
        let src = r#"module x
type Load = | Loading | Loaded(Array<string>)
component View(props: { s: Load }) -> Component {
  return <match value={props.s}>
    <case Loading><p>wait</p></case>
    <case Loaded(items)><p>{items.foo}</p></case>
  </match>
}
"#;
        let (field, type_name, fields, builtin, _, _) = the_member_error(src);
        assert_eq!((field.as_str(), type_name.as_str()), ("foo", "Array<string>"));
        assert_eq!(builtin, Some(BuiltinReceiver::Array));
        assert!(fields.iter().any(|f| f == "length"), "{fields:?}");
        let fine = src.replace("{items.foo}", "{items.length}");
        assert!(ty_errors_of(&fine).is_empty(), "{:?}", ty_errors_of(&fine));
    }

    /// `bind={x}` on a `<case>` is E0231, and the help writes the pattern for
    /// the payload's shape: an object pattern reading the field `bind` read
    /// for a record, a name in parentheses for one value, the bare variant
    /// for none.
    #[test]
    fn bind_on_a_case_is_e0231_with_the_rewrite_for_the_payload_shape() {
        let src = |payload: &str, case: &str| {
            format!(
                "module x\ntype Load = | Loading | Loaded{payload}\ncomponent V(props: {{ s: Load }}) -> Component {{\n  return <match value={{props.s}}>\n    <case Loading><p>a</p></case>\n    {case}\n  </match>\n}}\n"
            )
        };
        let cases = [
            (
                src("({ users: Array<string> })", "<case Loaded bind={users}><p>{users}</p></case>"),
                "write `<case Loaded({ users })>`",
            ),
            (
                src("(Array<string>)", "<case Loaded bind={items}><p>{items}</p></case>"),
                "write `<case Loaded(items)>`",
            ),
            (
                src("(Array<string>)", "<case Loaded(items)><p>a</p></case>").replace(
                    "<case Loading>",
                    "<case Loading bind={x}>",
                ),
                "write `<case Loading>`",
            ),
        ];
        for (program, rewrite) in cases {
            let errs = ty_errors_of(&program);
            match &errs[..] {
                [e @ TypeError::CaseBind { .. }] => {
                    assert_eq!(e.code(), "E0231");
                    let help = e.help().expect("E0231 has help");
                    assert!(help.contains(rewrite), "{help}");
                }
                other => panic!("expected one E0231, got {other:?}\n{program}"),
            }
        }
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
    Loaded => 2,
    Failed => 3,
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
        // longer read as a catch-all — the genuinely missing `Loading` variant
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
        // `Loading` still surfaces as E0200 because the dropped arm covers nothing.
        let src = r#"module x
type Feed = | Loading | Loaded | Failed
fn show(f: Feed) -> number {
  return match f {
    Loaded => 2,
    Failed => 3,
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
            "the missing `Loading` variant must still surface: {ty_errs:?}"
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
    Feed.Loaded => 2,
    Feed.Failed => 3,
    Feed.Loadign => 1,
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
            "the missing `Loading` variant must still surface: {ty_errs:?}"
        );
    }

    /// G305. The suggestion is drawn from the variants no other arm covers.
    /// `Loadin` is one edit from `Loading`, but a `Loading` arm is already
    /// there and writing it again is an arm that can never run (E0305), so the
    /// suggestion is the uncovered `Loaded`, two edits away. With every variant
    /// covered there is nothing left to suggest. A variant reached only through
    /// a nested sub-pattern (`Wrap(X)`) is not covered: `Wrap(y)` beside it can
    /// still run, so it stays a candidate.
    #[test]
    fn e0220_suggests_only_a_variant_no_other_arm_covers() {
        let src = r#"module x
type Inner = | X | Y
type Feed = | Loading | Loaded | Failed
type Boxed = | Wrap(Inner) | Done
fn show(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loadin => 2,
    Failed => 3,
  }
}
fn all(f: Feed) -> number {
  return match f {
    Loading => 1,
    Loaded => 2,
    Failed => 3,
    Loadign => 4,
  }
}
fn nest(b: Boxed) -> number {
  return match b {
    Wrap(X) => 1,
    Wrapp(y) => 2,
    Done => 3,
  }
}
"#;
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, ty_errs) = assign_types(&m, &resolved, &prelude);
        let got: Vec<(String, Option<String>)> = ty_errs
            .iter()
            .filter_map(|e| match e {
                TypeError::UnknownVariantPattern { name, suggestion, .. } => {
                    Some((name.clone(), suggestion.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("Loadin".to_string(), Some("Loaded".to_string())),
                ("Loadign".to_string(), None),
                ("Wrapp".to_string(), Some("Wrap".to_string())),
            ],
            "{ty_errs:?}"
        );
        assert!(
            !format!("{}", ty_errs.iter().find(|e| e.code() == "E0220").unwrap())
                .contains("`Loading`"),
            "{ty_errs:?}"
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
        // `r.parse()?` types as Unknown (a member-access call on an opaque
        // receiver), so the operand rule cannot prove it isn't a Result and
        // stays silent. The receiver used to be a `string`, which has no
        // `parse` at all and is E0210 since G288.
        let src = r#"module x
type Raw = extern_ts("any")
fn f(r: Raw) -> Result<number, string> {
  let v = r.parse()?
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
    fn return_primitive_against_declared_record_is_flagged() {
        // This test used to pin the opposite boundary: a primitive value
        // against a named return type was not judged, and the test existed so
        // that a future change would be a deliberate one. This is that change.
        // The declaration body is a record, so a `number` cannot be one, and
        // the return position reports it under the code it has always used.
        let src = "module x\ntype U = { x: number }\nfn f() -> U { return 5 }\n";
        let errs = ty_errors_of(src);
        assert!(
            matches!(
                errs.as_slice(),
                [TypeError::TypeMismatch { expected, found, .. }]
                    if expected == "U" && found == "number"
            ),
            "errs: {errs:?}"
        );
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

    /// G291. A literal match over a type declared in a module this compile
    /// cannot see. It could be a literal union the arms cover, so E0218 ("a
    /// string match can never be exhaustive, add an `else`") was a claim about
    /// a type nobody had read, and its advice switched the check off. It is
    /// E0232 now, through the named and the namespace spelling alike, and the
    /// help sends the reader to the declaration before it mentions `else`.
    #[test]
    fn a_match_over_an_unseen_imported_type_is_undecided_not_open() {
        for src in [
            "module app\nimport catalog { ColType }\n\
             pub fn width(t: ColType) -> int {\n\
             \x20 return match t {\n    \"text\" => 10,\n    \"int\" => 4,\n  }\n}\n",
            "module app\nimport catalog\n\
             pub fn width(t: catalog.ColType) -> int {\n\
             \x20 return match t {\n    \"text\" => 10,\n    \"int\" => 4,\n  }\n}\n",
            "module app\nimport catalog as c\n\
             pub fn width(t: c.ColType) -> int {\n\
             \x20 return match t {\n    1 => 10,\n    2 => 4,\n  }\n}\n",
        ] {
            let errs = errors_of(src);
            match &errs[..] {
                [e @ TypeError::UnseenMatchType {
                    type_name, module, ..
                }] => {
                    assert_eq!((type_name.as_str(), module.as_str()), ("ColType", "catalog"));
                    assert_eq!(e.code(), "E0232");
                    let help = e.help().expect("E0232 has help");
                    assert!(help.starts_with("Make `catalog` visible"), "{help}");
                    assert!(help.contains("glyph gen dts catalog"), "{help}");
                    let first_else = help.find("`else`").expect("names the opt-out");
                    assert!(
                        first_else > help.find("gen dts").unwrap(),
                        "`else` is the last suggestion, not the first: {help}"
                    );
                }
                other => panic!("expected one E0232, got {other:?}\n{src}"),
            }
        }
    }

    /// An `else` arm is the explicit opt-out, and still compiles.
    #[test]
    fn an_unseen_imported_type_match_with_a_catch_all_is_accepted() {
        let errs = errors_of(
            "module app\nimport catalog { ColType }\n\
             pub fn width(t: ColType) -> int {\n\
             \x20 return match t {\n    \"text\" => 10,\n    else => 0,\n  }\n}\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// A local `string` is not unseen: E0218 is a true statement about it.
    #[test]
    fn a_local_string_match_keeps_e0218() {
        let errs = errors_of(
            "module app\ntype Name = string\n\
             pub fn width(t: Name, s: string) -> int {\n\
             \x20 let a = match t {\n    \"text\" => 10,\n  }\n\
             \x20 return match s {\n    \"text\" => 10,\n  }\n}\n",
        );
        let codes: Vec<&str> = errs.iter().map(|e| e.code()).collect();
        assert_eq!(codes, ["E0218", "E0218"], "{errs:?}");
    }

    /// The three cases a project with cross-module context separates: the
    /// declaration is visible (checked against the real union), the module is
    /// visible and has no such export (E0105 at the import, nothing more on
    /// the match), and the module is not in the compile (E0232).
    #[test]
    fn visibility_of_the_declaration_decides_which_error_a_match_gets() {
        struct Project;
        impl DeclTyResolver for Project {
            fn decl_ty(&self, _decl_idx: u32) -> Ty {
                Ty::Unknown
            }
            fn imported_type_decl(
                &self,
                module_path: &str,
                type_name: &str,
            ) -> Option<ImportedTypeDecl> {
                (module_path == "catalog" && type_name == "ColType").then(|| ImportedTypeDecl {
                    name: Ident::from("ColType"),
                    generics: Vec::new(),
                    body: Ty::StringLiteralUnion(vec![
                        "text".to_string(),
                        "int".to_string(),
                        "real".to_string(),
                    ]),
                    open: false,
                })
            }
            fn module_exports_name(&self, module_path: &str, name: &str) -> Option<bool> {
                (module_path == "catalog").then_some(name == "ColType")
            }
        }
        let check = |import: &str, arms: &str| {
            let src = format!(
                "module app\nimport {import}\n\
                 pub fn width(t: ColType) -> int {{\n  return match t {{\n{arms}  }}\n}}\n"
            );
            let m = glyph_parser::parse(&src).expect("parse failed");
            let syms = collect_module_symbols(&m).unwrap();
            let prelude = build_prelude();
            let (resolved, _errs) = resolve_module(&m, syms, &prelude);
            let (_tm, errs) = assign_types_with_resolver(&m, &resolved, &prelude, &Project);
            errs.iter().map(|e| e.code()).collect::<Vec<_>>()
        };
        let all = "    \"text\" => 10,\n    \"int\" => 4,\n    \"real\" => 8,\n";
        let short = "    \"text\" => 10,\n";
        assert_eq!(check("catalog { ColType }", all), Vec::<&str>::new());
        assert_eq!(check("catalog { ColType }", short), ["E0200"]);
        // A name the visible module does not export is E0105 in the resolver.
        let src_missing = "module app\nimport catalog { ColTyp }\n\
             pub fn width(t: ColTyp) -> int {\n  return match t {\n    \"text\" => 10,\n  }\n}\n";
        let m = glyph_parser::parse(src_missing).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types_with_resolver(&m, &resolved, &prelude, &Project);
        assert!(errs.is_empty(), "the import already failed; one error per mistake: {errs:?}");
        assert_eq!(check("elsewhere { ColType }", short), ["E0232"]);
    }

    /// A stdlib type the checker does not model has no file to bring into the
    /// build, so the help does not send the reader looking for one. (The
    /// stdlib's string-literal unions are modeled since G294; `Response` is a
    /// record nobody models, which keeps this path reachable.)
    #[test]
    fn an_unseen_stdlib_type_says_there_is_nothing_to_make_visible() {
        let errs = errors_of(
            "module app\nimport std/http { Response }\n\
             pub fn rank(l: Response) -> int {\n\
             \x20 return match l {\n    \"debug\" => 1,\n  }\n}\n",
        );
        match &errs[..] {
            [e @ TypeError::UnseenMatchType { module, .. }] => {
                assert_eq!(module, "std/http");
                let help = e.help().unwrap();
                assert!(!help.contains("gen dts"), "{help}");
                assert!(help.contains("nothing to make visible"), "{help}");
            }
            other => panic!("expected one E0232, got {other:?}"),
        }
    }

    /// The codes a program draws, for comparing a stdlib union against the
    /// same union declared in the program.
    fn codes_of(src: &str) -> Vec<&'static str> {
        errors_of(src).iter().map(|e| e.code()).collect()
    }

    /// G294. Every string-literal union the standard library exports, matched
    /// through each import spelling with every literal covered and no `else`.
    /// Through 0.1.130 each of these was E0232: the declaration lives in the
    /// hand-written runtime, so the checker could not read it, and an `else`
    /// was the only way to compile a match over three values.
    #[test]
    fn a_full_match_over_a_stdlib_literal_union_needs_no_else() {
        for (module, name, values) in crate::ty::BUILTIN_STRING_LITERAL_UNIONS {
            let ns = module.strip_prefix("std/").unwrap();
            let arms: String = values
                .iter()
                .enumerate()
                .map(|(i, v)| format!("    \"{v}\" => {i},\n"))
                .collect();
            for (import, ty) in [
                (module.to_string(), format!("{ns}.{name}")),
                (format!("{module} as m"), format!("m.{name}")),
                (format!("{module} {{ {name} }}"), name.to_string()),
            ] {
                let src = format!(
                    "module app\nimport {import}\n\
                     pub fn rank(v: {ty}) -> int {{\n  return match v {{\n{arms}  }}\n}}\n"
                );
                assert_eq!(codes_of(&src), Vec::<&str>::new(), "{src}");
            }
        }
    }

    /// A stdlib literal union left partly unmatched is E0200 naming what is
    /// missing, exactly as the same union declared in the program is.
    #[test]
    fn a_partial_match_over_a_stdlib_literal_union_names_the_missing_value() {
        let src = "module app\nimport std/http\n\
                   pub fn f(p: http.RedirectPolicy) -> int {\n\
                   \x20 return match p {\n    \"follow\" => 1,\n    \"error\" => 3,\n  }\n}\n";
        match &errors_of(src)[..] {
            [e @ TypeError::NonExhaustiveMatch {
                missing_variants, ..
            }] => {
                assert_eq!(e.code(), "E0200");
                assert_eq!(missing_variants, &["manual".to_string()]);
            }
            other => panic!("expected one E0200 naming \"manual\", got {other:?}"),
        }
        let level = "module app\nimport std/log { Level }\n\
                     pub fn rank(l: Level) -> int {\n\
                     \x20 return match l {\n    \"debug\" => 1,\n  }\n}\n";
        match &errors_of(level)[..] {
            [TypeError::NonExhaustiveMatch {
                missing_variants, ..
            }] => assert_eq!(missing_variants, &["info", "warn", "error"]),
            other => panic!("expected one E0200, got {other:?}"),
        }
    }

    /// A literal the stdlib union does not have is refused with the codes the
    /// program-declared union draws, in an arm and in a returned value alike.
    #[test]
    fn a_misspelled_stdlib_literal_is_refused_as_a_user_union_is() {
        let arm = |head: &str, ty: &str| {
            format!(
                "module app\n{head}\
                 pub fn f(p: {ty}) -> int {{\n\
                 \x20 return match p {{\n    \"follow\" => 1,\n    \"manuel\" => 2,\n    \"error\" => 3,\n  }}\n}}\n"
            )
        };
        let local_head = "type RedirectPolicy = \"follow\" | \"manual\" | \"error\"\n";
        let local = codes_of(&arm(local_head, "RedirectPolicy"));
        assert!(!local.is_empty(), "the program-declared union refuses the typo");
        assert_eq!(codes_of(&arm("import std/http\n", "http.RedirectPolicy")), local);

        let ret = |head: &str, ty: &str| {
            format!("module app\n{head}pub fn pick() -> {ty} {{\n  return \"folow\"\n}}\n")
        };
        let local = codes_of(&ret(local_head, "RedirectPolicy"));
        assert!(!local.is_empty(), "the program-declared union refuses the typo");
        assert_eq!(codes_of(&ret("import std/http\n", "http.RedirectPolicy")), local);
    }

    /// A `kind` field read off a stdlib error record has its union's name, so a
    /// mismatch names both declarations, and its literals come from the same
    /// table the annotation reads.
    /// G302 through the standard library. `type P = http.RedirectPolicy` is a
    /// second name for the stdlib union (D46), by the namespace spelling and
    /// the aliased one alike: a match over a `P` is held to the union's three
    /// values, a misspelled literal is refused, and a `P` and an
    /// `http.RedirectPolicy` pass for each other. The chain stopped at the
    /// qualified body, so the match drew E0232 and `else` was the only way
    /// to compile it, which switches the check off.
    #[test]
    fn a_qualified_alias_of_a_stdlib_union_is_a_second_name() {
        for (import, q) in [("std/http", "http"), ("std/http as h", "h")] {
            let head = format!("module app\nimport {import}\ntype P = {q}.RedirectPolicy\n");
            let full = format!(
                "{head}pub fn rank(p: P) -> int {{\n  return match p {{\n\
                 \x20   \"follow\" => 1,\n    \"manual\" => 2,\n    \"error\" => 3,\n  }}\n}}\n"
            );
            assert_eq!(codes_of(&full), Vec::<&str>::new(), "{full}");
            let partial = format!(
                "{head}pub fn rank(p: P) -> int {{\n  return match p {{\n\
                 \x20   \"follow\" => 1,\n    \"error\" => 3,\n  }}\n}}\n"
            );
            match &errors_of(&partial)[..] {
                [e @ TypeError::NonExhaustiveMatch {
                    missing_variants, ..
                }] => {
                    assert_eq!(e.code(), "E0200");
                    assert_eq!(missing_variants, &["manual".to_string()]);
                }
                other => panic!("expected one E0200 naming \"manual\" for {q}: {other:?}"),
            }
            let typo = format!("{head}pub fn pick() -> P {{\n  return \"folow\"\n}}\n");
            let direct = format!(
                "module app\nimport {import}\npub fn pick() -> {q}.RedirectPolicy {{\n  return \"folow\"\n}}\n"
            );
            assert!(!codes_of(&direct).is_empty(), "the direct spelling refuses the typo");
            assert_eq!(codes_of(&typo), codes_of(&direct), "{typo}");
            let both_ways = format!(
                "{head}fn take(p: P) -> P {{\n  return p\n}}\n\
                 pub fn there(r: {q}.RedirectPolicy) -> {q}.RedirectPolicy {{\n  return take(r)\n}}\n"
            );
            assert_eq!(codes_of(&both_ways), Vec::<&str>::new(), "{both_ways}");
        }
    }

    /// The same hop for a union declared in the program and reached by a bare
    /// second name, through the named stdlib import too: a match over `P` is
    /// held to the values of the union `P` names. It read `P`'s own body,
    /// found a name rather than a literal set, and drew E0218 on `string`.
    #[test]
    fn a_match_over_a_second_name_of_a_literal_union_is_held_to_its_values() {
        for head in [
            "type M = \"follow\" | \"manual\" | \"error\"\ntype P = M\n",
            "import std/http { RedirectPolicy }\ntype P = RedirectPolicy\n",
        ] {
            let partial = format!(
                "module app\n{head}pub fn rank(p: P) -> int {{\n  return match p {{\n\
                 \x20   \"follow\" => 1,\n    \"error\" => 3,\n  }}\n}}\n"
            );
            match &errors_of(&partial)[..] {
                [TypeError::NonExhaustiveMatch {
                    missing_variants, ..
                }] => assert_eq!(missing_variants, &["manual".to_string()]),
                other => panic!("expected one E0200 naming \"manual\": {other:?}\n{partial}"),
            }
        }
    }

    #[test]
    fn a_stdlib_error_kind_field_is_named_by_its_union() {
        let src = "module app\nimport std/http\nimport std/net\n\
                   pub fn f(e: http.HttpError) -> net.ServerErrorKind {\n  return e.kind\n}\n";
        let errs = errors_of(src);
        let messages: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
        assert_eq!(errs.iter().map(|e| e.code()).collect::<Vec<_>>(), ["E0204"], "{messages:?}");
        assert!(
            messages[0].contains("expected `ServerErrorKind`, found `HttpErrorKind`"),
            "{messages:?}"
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
        // `.length` on an `Array` must not false-positive: an array's member
        // reads are checked since G288, against the members a JavaScript
        // array has, and `length` is one of them.
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
        // Distinct named types are nominally incompatible (D49).
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

    // ----- G201: a declared union or record is never a primitive -----

    #[test]
    fn declared_union_argument_into_a_string_parameter_is_flagged() {
        // G201. The defect: a value of a declared tagged union passed where a
        // `string` is declared drew no diagnostic at all, while the `bool`
        // control below drew E0211. Only `tsc` caught it, so
        // `glyph check --no-tsc` accepted the program.
        let src = r#"module x
type PaymentResult =
  | Settled
  | Declined

fn takes_string(s: string) -> string {
  return s
}

fn main(r: PaymentResult) -> string {
  return takes_string(r)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "string" && found == "PaymentResult"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn declared_record_argument_into_a_number_parameter_is_flagged() {
        // The record half of the same rule. A `Ty::Record` already mismatched a
        // primitive; a `Ty::Named` standing for a declared record did not.
        let src = r#"module x
type Point = { x: number, y: number }

fn takes_number(n: number) -> number {
  return n
}

fn main(p: Point) -> number {
  return takes_number(p)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "number" && found == "Point"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn declared_union_returned_where_a_string_is_declared_is_flagged() {
        // The same relation drives `return`, so the return position gains the
        // diagnostic with the argument position.
        let src = r#"module x
type PaymentResult =
  | Settled
  | Declined

fn main(r: PaymentResult) -> string {
  return r
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::TypeMismatch { expected, found, .. }
                    if expected == "string" && found == "PaymentResult"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_primitive_alias_argument_stays_silent() {
        // The false positive the rule must not produce. `type UserId = string`
        // is a name for `string`, so passing one where a `string` is declared is
        // correct code and has to keep compiling.
        let src = r#"module x
type UserId = string

fn takes_string(s: string) -> string {
  return s
}

fn main(u: UserId) -> string {
  return takes_string(u)
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_string_literal_union_alias_argument_stays_silent() {
        // D30: a string-literal union behaves like `string` for assignability.
        // It is a declared type whose body is a union of literals, which is
        // exactly the shape a rule reading "declared union" too loosely would
        // reject.
        let src = r#"module x
type Tier = "free" | "pro"

fn takes_string(s: string) -> string {
  return s
}

fn main(t: Tier) -> string {
  return takes_string(t)
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_declared_union_into_a_void_parameter_stays_silent() {
        // `void` is excluded from the rule for the same reason it is excluded
        // from the scalar-versus-record arm: its assignability is subtler.
        let src = r#"module x
type PaymentResult =
  | Settled
  | Declined

fn takes_void(v: void) -> number {
  return 1
}

fn main(r: PaymentResult) -> number {
  return takes_void(r)
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_declared_union_into_a_matching_named_parameter_stays_silent() {
        // The rule fires on the primitive expectation only. The same value
        // passed where its own type is declared is correct and silent.
        let src = r#"module x
type PaymentResult =
  | Settled
  | Declined

fn takes_result(r: PaymentResult) -> number {
  return 1
}

fn main(r: PaymentResult) -> number {
  return takes_result(r)
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_generic_union_application_argument_is_flagged() {
        // A generic declared union arrives as `Ty::App` over the `Ty::Named`,
        // and `Tree<number>` is no more a `string` than `Tree` is.
        let src = r#"module x
type Tree<T> =
  | Leaf
  | Node({ value: T })

fn takes_string(s: string) -> string {
  return s
}

fn main(t: Tree<number>) -> string {
  return takes_string(t)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "string" && found == "Tree<number>"
            )),
            "errs: {errs:?}"
        );
    }

    // ----- the reverse direction: a primitive is never a declared union or
    // record -----

    #[test]
    fn a_primitive_into_a_declared_union_parameter_is_flagged() {
        // G201 left this direction deliberately unchanged and pinned it as
        // silent; this is that boundary moving. A `string` passed where a
        // declared tagged union is expected was caught only by `tsc`, so `glyph
        // check --no-tsc` accepted `takes_result("Settled")`.
        let src = r#"module x
type PaymentResult =
  | Settled
  | Declined

fn takes_result(r: PaymentResult) -> number {
  return 1
}

fn main() -> number {
  return takes_result("Settled")
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "PaymentResult" && found == "string"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_number_into_a_declared_record_parameter_is_flagged() {
        // The record half of the reverse rule.
        let src = r#"module x
type Point = { x: number, y: number }

fn takes_point(p: Point) -> number {
  return p.x
}

fn main() -> number {
  return takes_point(1)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "Point" && found == "number"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_bool_returned_where_a_declared_union_is_declared_is_flagged() {
        // The return position shares the relation, so it gains the reverse
        // direction with the argument position, under the code `return` uses.
        let src = r#"module x
type PaymentResult =
  | Settled
  | Declined

fn main() -> PaymentResult {
  return true
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::TypeMismatch { expected, found, .. }
                    if expected == "PaymentResult" && found == "bool"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_string_bound_to_a_let_annotated_with_a_declared_record_is_flagged() {
        // The annotated `let` position, same relation, same code as G149.
        let src = r#"module x
type Point = { x: number, y: number }

fn main() -> number {
  let p: Point = "origin"
  return 1
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::TypeMismatch { expected, found, .. }
                    if expected == "Point" && found == "string"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_primitive_into_a_generic_union_application_parameter_is_flagged() {
        // `Tree<number>` is no more satisfied by a `string` than `Tree` is.
        let src = r#"module x
type Tree<T> =
  | Leaf
  | Node({ value: T })

fn takes_tree(t: Tree<number>) -> number {
  return 1
}

fn main() -> number {
  return takes_tree("leaf")
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "Tree<number>" && found == "string"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_primitive_into_an_empty_record_parameter_stays_silent() {
        // The one exclusion. TypeScript lets a `string` satisfy the empty
        // object type, so the emitted TypeScript of this program compiles under
        // `tsc --strict`; rejecting it would make Glyph disagree with tsc on a
        // program tsc accepts.
        let src = r#"module x
type Anything = { }

fn takes_anything(a: Anything) -> number {
  return 1
}

fn main() -> number {
  return takes_anything("x")
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_declared_empty_record_into_a_string_parameter_is_still_flagged() {
        // The exclusion is one-directional: `{}` is not a `string` under tsc
        // either, so the G201 direction keeps firing on the empty record.
        let src = r#"module x
type Anything = { }

fn takes_string(s: string) -> string {
  return s
}

fn main(a: Anything) -> string {
  return takes_string(a)
}
"#;
        let errs = ty_errors_of(src);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "string" && found == "Anything"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_string_into_a_primitive_alias_parameter_stays_silent() {
        // `type UserId = string` names `string`; a literal fits it.
        let src = r#"module x
type UserId = string

fn takes_id(u: UserId) -> number {
  return 1
}

fn main() -> number {
  return takes_id("u-1")
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_string_into_a_string_literal_union_parameter_stays_silent() {
        // D30: a string-literal union is spelled with `|` but is a `string`.
        let src = r#"module x
type Tier = "free" | "pro"

fn takes_tier(t: Tier) -> number {
  return 1
}

fn main() -> number {
  return takes_tier("free")
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_string_into_an_interface_parameter_stays_silent() {
        // An interface is matched by member shape, not by this rule, and its
        // empty form is one TypeScript lets a `string` satisfy.
        let src = r#"module x
interface Marker { }

fn takes_marker(m: Marker) -> number {
  return 1
}

fn main() -> number {
  return takes_marker("x")
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn a_string_into_an_extern_ts_typed_parameter_stays_silent() {
        // An `extern_ts` body is opaque to this checker by design.
        let src = r#"module x
type Raw = extern_ts("string | number")

fn takes_raw(r: Raw) -> number {
  return 1
}

fn main() -> number {
  return takes_raw("x")
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    #[test]
    fn an_imported_union_argument_without_a_project_context_stays_silent() {
        // A db-less caller holds the identity of an imported type and not its
        // declaration, so the pairing is undetermined for it, exactly as a
        // field typo on an imported record is. The project-aware reading is
        // the group below.
        let src = r#"module x
import payments { PaymentResult }

fn takes_string(s: string) -> string {
  return s
}

fn main(r: PaymentResult) -> string {
  return takes_string(r)
}
"#;
        assert!(ty_errors_of(src).is_empty(), "errs: {:?}", ty_errors_of(src));
    }

    // ----- G215, G226: an imported declaration is decided like a local one -----

    /// A project of one sibling, `lib`, as `glyph_db::exported_type` would
    /// answer it, plus a second module `other` declaring a union under the
    /// same display name. One entry per shape the rule has to read past. The
    /// module's own declarations are answered by `LocalDeclTy`, so a call's
    /// arguments are checked against a real signature.
    struct LibDecls<'a> {
        local: LocalDeclTy<'a>,
    }

    impl DeclTyResolver for LibDecls<'_> {
        fn decl_ty(&self, decl_idx: u32) -> Ty {
            self.local.decl_ty(decl_idx)
        }

        fn imported_type_decl(
            &self,
            module_path: &str,
            type_name: &str,
        ) -> Option<ImportedTypeDecl> {
            let decl = |name: &str, generics: Vec<&str>, body: Ty| ImportedTypeDecl {
                name: Ident::from(name),
                generics: generics.into_iter().map(Ident::from).collect(),
                body,
                open: false,
            };
            let union = |names: &[&str]| Ty::Union {
                variants: names
                    .iter()
                    .map(|n| UnionVariant {
                        name: Ident::from(*n),
                        payload: None,
                    })
                    .collect(),
            };
            let field = |name: &str, ty: Ty| RecordField {
                name: Ident::from(name),
                ty,
                optional: false,
            };
            match (module_path, type_name) {
                ("lib", "Status") => Some(decl("Status", vec![], union(&["Pending", "Done"]))),
                ("lib", "Point") => Some(decl(
                    "Point",
                    vec![],
                    Ty::Record {
                        fields: vec![field("x", Ty::Prim(Primitive::Number))],
                    },
                )),
                ("lib", "Box") => Some(decl(
                    "Box",
                    vec!["T"],
                    Ty::Record {
                        fields: vec![field(
                            "value",
                            Ty::Param {
                                name: Ident::from("T"),
                                owner: ParamOwner::Unresolved,
                            },
                        )],
                    },
                )),
                ("lib", "Empty") => Some(decl("Empty", vec![], Ty::Record { fields: vec![] })),
                // `@open pub type Meta = { id: string }` in the sibling.
                ("lib", "Meta") => Some(ImportedTypeDecl {
                    open: true,
                    ..decl(
                        "Meta",
                        vec![],
                        Ty::Record {
                            fields: vec![field("id", Ty::Prim(Primitive::String))],
                        },
                    )
                }),
                ("lib", "Id") => Some(decl("Id", vec![], Ty::Prim(Primitive::String))),
                ("lib", "Tier") => Some(decl(
                    "Tier",
                    vec![],
                    Ty::StringLiteralUnion(vec!["free".into(), "pro".into()]),
                )),
                // `pub type Rows = Point` in the sibling: a hop, not a type.
                ("lib", "Rows") => Some(decl(
                    "Rows",
                    vec![],
                    Ty::Imported {
                        module: "lib".into(),
                        name: Ident::from("Point"),
                    },
                )),
                ("other", "Status") => Some(decl("Status", vec![], union(&["Up", "Down"]))),
                _ => None,
            }
        }
    }

    fn lib_errors_of(src: &str) -> Vec<TypeError> {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let lowerer = Lowerer::new(&resolved, &prelude);
        let decls = LibDecls {
            local: LocalDeclTy::new(&m, &lowerer),
        };
        let (_tm, ty_errs) = assign_types_with_resolver(&m, &resolved, &prelude, &decls);
        ty_errs
    }

    fn has_argument_mismatch(errs: &[TypeError], expected: &str, found: &str) -> bool {
        errs.iter().any(|e| matches!(
            e,
            TypeError::ArgumentTypeMismatch { expected: x, found: f, .. }
                if x == expected && f == found
        ))
    }

    fn has_type_mismatch(errs: &[TypeError], expected: &str, found: &str) -> bool {
        errs.iter().any(|e| matches!(
            e,
            TypeError::TypeMismatch { expected: x, found: f, .. }
                if x == expected && f == found
        ))
    }

    #[test]
    fn an_imported_union_argument_into_a_string_parameter_is_flagged() {
        // G215, the program the ledger records: silent under every surface
        // that runs without tsc, where the same union declared locally is
        // E0211 by G201.
        let src = r#"module x
import lib { Status, Pending }

fn shout(s: string) -> string {
  return s
}

fn main() -> string {
  let st: Status = Pending
  return shout(st)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "string", "Status"), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_union_is_decided_under_the_namespace_spelling_too() {
        // The identity is the declaration's, so the spelling that brought it
        // into scope cannot change the verdict.
        let src = r#"module x
import lib

fn shout(s: string) -> string {
  return s
}

fn main(st: lib.Status) -> string {
  return shout(st)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "string", "Status"), "errs: {errs:?}");
    }

    #[test]
    fn an_application_of_an_imported_record_into_a_number_parameter_is_flagged() {
        // The judgement is the declaration's, so `Box<number>` answers as the
        // bare name does, exactly as a local `Tree<number>` does under G201.
        let src = r#"module x
import lib { Box }

fn takes_number(n: number) -> number {
  return n
}

fn main(b: Box<number>) -> number {
  return takes_number(b)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "number", "Box<number>"), "errs: {errs:?}");
    }

    #[test]
    fn a_string_into_an_imported_union_parameter_is_flagged() {
        // The reverse direction across the boundary.
        let src = r#"module x
import lib { Status }

fn takes_status(s: Status) -> Status {
  return s
}

fn main() -> Status {
  return takes_status("Pending")
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "Status", "string"), "errs: {errs:?}");
    }

    #[test]
    fn a_number_into_an_imported_fielded_record_parameter_is_flagged() {
        let src = r#"module x
import lib { Point }

fn takes_point(p: Point) -> Point {
  return p
}

fn main() -> Point {
  return takes_point(3)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "Point", "number"), "errs: {errs:?}");
    }

    #[test]
    fn a_string_into_an_imported_empty_record_parameter_stays_silent() {
        // The one exclusion the reverse direction keeps locally, kept here:
        // tsc lets a `string` satisfy `{}`.
        let src = r#"module x
import lib { Empty }

fn takes_empty(e: Empty) -> Empty {
  return e
}

fn main() -> Empty {
  return takes_empty("x")
}
"#;
        let errs = lib_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_empty_record_into_a_string_parameter_is_still_flagged() {
        // And the forward direction has no exclusion, as locally: `{}` is not
        // a `string` under tsc either.
        let src = r#"module x
import lib { Empty }

fn takes_string(s: string) -> string {
  return s
}

fn main(e: Empty) -> string {
  return takes_string(e)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "string", "Empty"), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_primitive_alias_stays_silent_against_any_primitive() {
        // `pub type Id = string` is a name for `string`. Locally the alias is
        // undetermined against `string` and against `number` alike, and the
        // imported reading keeps both silences.
        let src = r#"module x
import lib { Id }

fn takes_string(s: string) -> string {
  return s
}

fn takes_number(n: number) -> number {
  return n
}

fn main(i: Id) -> number {
  takes_string(i)
  return takes_number(i)
}
"#;
        let errs = lib_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_string_literal_union_reached_as_a_body_stays_silent() {
        // A `Ty::Imported` whose declaration is a string-literal union is a
        // `string` at run time and under Glyph's own rule, so it is compared
        // by nothing, as the local alias is.
        let src = r#"module x
import lib { Tier }

fn takes_string(s: string) -> string {
  return s
}

fn takes_tier(t: Tier) -> Tier {
  return t
}

fn main(t: Tier) -> Tier {
  takes_string(t)
  return takes_tier("free")
}
"#;
        let errs = lib_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_local_record_bound_to_a_local_alias_of_an_imported_union_is_flagged() {
        // G226, the program the ledger records. `Local` canonicalizes to the
        // `Ty::Imported` it names (D46), and the pairing of that declaration
        // against a declaration of this module is decided by identity, which
        // is the `E0204` 0.1.118 reported by the nominal comparison and
        // 0.1.120 change 1 lost.
        let src = r#"module x
import lib { Status }

type Local = Status
type Other = { x: int }

fn main() -> Local {
  let o: Other = { x: 1, }
  let l: Local = o
  return l
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_type_mismatch(&errs, "Local", "Other"), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_union_into_a_local_record_parameter_is_flagged() {
        // The same pairing the other way round: an imported declaration where
        // a declaration of this module is expected.
        let src = r#"module x
import lib { Status }

type Other = { x: int }

fn takes_other(o: Other) -> Other {
  return o
}

fn main(s: Status) -> Other {
  return takes_other(s)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "Other", "Status"), "errs: {errs:?}");
    }

    #[test]
    fn two_local_aliases_of_one_imported_declaration_are_one_type() {
        // The identity is read after the chain, so the two second names and
        // the direct import compare equal. Under 0.1.118 `let b: Local2 = a`
        // was a false E0204 by spelling.
        let src = r#"module x
import lib { Status, Pending }

type Local = Status
type Local2 = Status

fn main() -> Local2 {
  let s: Status = Pending
  let a: Local = s
  let b: Local2 = a
  return b
}
"#;
        let errs = lib_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_siblings_own_alias_chain_ends_at_the_declaration_it_names() {
        // `pub type Rows = Point` in the sibling is a hop the same way a local
        // alias is, so `Rows` and `Point` are one declaration and a `Rows`
        // where a `Point` is expected is correct code.
        let src = r#"module x
import lib { Rows, Point }

fn takes_point(p: Point) -> Point {
  return p
}

fn main(r: Rows) -> Point {
  return takes_point(r)
}
"#;
        let errs = lib_errors_of(src);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn two_imported_declarations_with_one_display_name_are_different_types() {
        // The address is `(module, name)`, not the name diagnostics print: the
        // corpus holds eleven unrelated declarations named `Command`.
        let src = r#"module x
import lib { Status }
import other as o

fn takes_status(s: Status) -> Status {
  return s
}

fn main(s: o.Status) -> Status {
  return takes_status(s)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "Status", "Status"), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_union_against_a_prelude_application_is_refused() {
        // G293. An imported union is a declaration of the program and
        // `Option` one of the prelude, so D49 refuses the pairing the way it
        // refuses a local union against `Option<number>`. This pinned the
        // silence until the reverse of G289 was applied.
        let src = r#"module x
import lib { Status }

fn takes_option(o: Option<number>) -> Option<number> {
  return o
}

fn main(s: Status) -> Option<number> {
  return takes_option(s)
}
"#;
        let errs = lib_errors_of(src);
        assert!(has_argument_mismatch(&errs, "Option<number>", "Status"), "errs: {errs:?}");
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
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
            type_params: Some(vec![]),
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
            type_params: Some(vec![]),
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

    /// A `fold` seeded with `""` and stepped by a `string`-returning callback
    /// accumulates a `string`, as `tsc` infers from the two candidates. The
    /// accumulator used to stay bound to the literal `""`, and once a
    /// function's return was compared through the relation (G274) the
    /// callback was refused on `shortlink` and `zipper`, which `tsc` accepts.
    /// A literal that reaches a literal set through `id` keeps its type.
    #[test]
    fn a_string_candidate_widens_a_literal_bound_earlier() {
        let errs = errors_of(
            "module x\nimport std/array\n\
             type Mode = \"read\" | \"write\"\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn takes(m: Mode) -> number { return 1 }\n\
             fn f(xs: Array<number>) -> string {\n\
             \x20 let n = takes(id(\"read\"))\n\
             \x20 return array.fold(xs, \"\", fn(out: string, _i: number) -> string { return out })\n\
             }\n",
        );
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
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
    fn array_fold_while_returns_the_accumulator_type() {
        // G101. `fold_while` is `fold` that stops before consuming the next
        // element once `done(acc)` holds. Its result is the accumulator, bound
        // off `init` the way `fold` binds it, so a `let` over it carries a type
        // and a field typo on that type is a Glyph error.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/array\n\
             fn step(acc: number, x: number) -> number { return acc + x }\n\
             fn stop(acc: number) -> bool { return acc > 10 }\n\
             fn f(xs: Array<number>) -> void {\n  let total = array.fold_while(xs, 0, step, stop)\n  return void\n}\n",
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
    fn a_fold_while_stop_test_that_returns_a_number_is_an_argument_mismatch() {
        // The stop test is `fn(A) -> bool`. A callback returning a number
        // would be truthy on every non-zero accumulator, so the fold would stop
        // after the first element on a build `tsc --strict` passed if the
        // signature were left unmodeled. Same shape as the G99 async checks.
        let errs = errors_of(
            "module x\nimport std/array\n\
             fn step(acc: number, x: number) -> number { return acc + x }\n\
             fn score(acc: number) -> number { return acc }\n\
             fn f(xs: Array<number>) -> void {\n  let total = array.fold_while(xs, 0, step, score)\n  return void\n}\n",
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "a `done` returning number should be an argument-type mismatch: {errs:?}"
        );
    }

    #[test]
    fn array_try_fold_returns_a_prelude_result_bound_from_the_callback() {
        // `try_fold`'s error type has no argument of its own: it is bound from
        // the callback's declared `Result<A, E>` return, the same route
        // `array.map` uses for `U`. The result is the prelude `Result`, so the
        // `?` operator and the exhaustiveness checker both recognise it.
        let (m, _, tm) = type_map_of(
            "module x\nimport std/array\n\
             fn step(acc: number, x: number) -> Result<number, string> { return Ok(acc + x) }\n\
             fn f(xs: Array<number>) -> void {\n  let total = array.try_fold(xs, 0, step)\n  return void\n}\n",
        );
        let ty = tm.get(first_let_value_span_anywhere(&m));
        assert!(
            matches!(ty, Ty::App { base, args }
                if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Result"))
                    && args.first() == Some(&Ty::Prim(Primitive::Number))
                    && args.get(1) == Some(&Ty::Prim(Primitive::String))),
            "got {ty:?}"
        );
    }

    #[test]
    fn a_try_fold_matched_without_an_err_arm_is_non_exhaustive() {
        // The reason to type it at all: left `Unknown`, a `match` with only an
        // `Ok` arm builds clean and throws at run time on the first `Err`.
        let errs = errors_of(
            "module x\nimport std/array\n\
             fn step(acc: number, x: number) -> Result<number, string> { return Ok(acc + x) }\n\
             fn f(xs: Array<number>) -> number {\n  return match array.try_fold(xs, 0, step) {\n    Ok(total) => total,\n  }\n}\n",
        );
        assert!(
            errs.iter().any(|e| e.code() == "E0200"),
            "a try_fold result matched without an Err arm must be E0200: {errs:?}"
        );
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
                module: "std/result".to_string(),
                declared: "Result".to_string(),
                name: "Result".to_string(),
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
                module: "std/option".to_string(),
                declared: "Option".to_string(),
                name: "Option".to_string(),
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

    /// G169: a bare PascalCase arm head naming no variant of an *imported*
    /// union is the same mistake as the module-local one, and it gets the same
    /// diagnostic. The imported side used to credit the head as covered and say
    /// nothing, so a typo left only the resolver's `unresolved name` behind and
    /// sent the author looking for an import they never needed.
    #[test]
    fn an_unknown_bare_head_over_an_imported_union_is_e0220() {
        let src = r#"module app
import model { Answer, Yes, No, Zed }
fn f(a: Answer) -> number {
  return match a {
    Yes => 1,
    No => 0,
    Zed => 2,
  }
}
"#;
        let (_cov, errs) = imported_coverage_and_errors(src);
        let reported = errs.iter().find_map(|e| match e {
            TypeError::UnknownVariantPattern {
                union,
                name,
                suggestion,
                ..
            } => Some((union.clone(), name.clone(), suggestion.clone())),
            _ => None,
        });
        assert_eq!(
            reported,
            Some(("Answer".to_string(), "Zed".to_string(), None)),
            "both variants are covered, so there is nothing to suggest: {errs:?}"
        );
        // The unknown head must not be credited as coverage on its way past:
        // an arm naming nothing this union declares cannot make it exhaustive.
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })),
            "every declared variant is named, so there is no gap: {errs:?}"
        );
    }

    /// G305 on the imported side: the suggestion skips a variant another arm
    /// covers. `Nos` is one edit from the covered `No` and two from `Yes`.
    #[test]
    fn an_imported_union_head_suggests_only_an_uncovered_variant() {
        let src = r#"module app
import model { Answer, Yes, No, Nos }
fn f(a: Answer) -> number {
  return match a {
    No => 0,
    Nos => 1,
  }
}
"#;
        let (_cov, errs) = imported_coverage_and_errors(src);
        let reported = errs.iter().find_map(|e| match e {
            TypeError::UnknownVariantPattern { name, suggestion, .. } => {
                Some((name.clone(), suggestion.clone()))
            }
            _ => None,
        });
        assert_eq!(
            reported,
            Some(("Nos".to_string(), Some("Yes".to_string()))),
            "errs: {errs:?}"
        );
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
component View(props: { s: Status }) -> Component {
  return <match value={props.s}>
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
                open: false,
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
            lowerer: Lowerer::new(&resolved, &prelude),
            resolved: &resolved,
            tm: &mut tm,
            errors: &mut errors,
            coverage: &mut coverage,
            field_uses: &mut field_uses,
            assign_target: None,
            call_callee: None,
            member_object: None,
            relating: RefCell::new(Vec::new()),
            decl_ty_resolver: resolver,
            return_stack: Vec::new(),
            local_tys: HashMap::new(),
            stdlib_values: HashMap::new(),
            local_stdlib_values: HashMap::new(),
            null_chains: NullChains::default(),
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
    fn a_local_alias_of_a_local_union_requires_that_unions_variants() {
        // D46: `Cmd` is a second name for `Command`, so a match over a `Cmd`
        // is held to `Command`'s variant set, keyed under `Command`'s own
        // declaration. Before the hop `resolve_named_union` read `Cmd`'s body
        // as written, found a bare name rather than a union, and the match
        // went unchecked (G224).
        let src = r#"module app
type Command =
  | Up
  | Down
type Cmd = Command
fn run(c: Cmd) -> string {
  return "x"
}
"#;
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            a.required_variants(&ty)
        });
        let (union, variants) = got.expect("an alias of a local union resolves");
        assert_eq!(
            union,
            UnionRef::Local {
                module: "app".to_string(),
                name: "Command".to_string(),
            }
        );
        assert_eq!(variants, vec![Ident::from("Up"), Ident::from("Down")]);
    }

    #[test]
    fn a_local_alias_of_an_imported_union_requires_the_imported_variants() {
        // The cross-module spelling of the same rule: `Local` lowers to a
        // `Ty::Named` of this module, the chain follows its body to the
        // `Ty::Imported` the import symbol lowers to, and the variant set is
        // read through `imported_type_decl` exactly as it is for `Answer`
        // spelled directly. The type end is the declaring module's, not this
        // file's: the alias declares no union of its own.
        let src = r#"module app
import model { Answer }
type Local = Answer
fn run(a: Local) -> string {
  return "x"
}
"#;
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            assert!(matches!(ty, Ty::Named { .. }), "the alias lowers to its own name: {ty:?}");
            assert_eq!(
                a.resolve_alias_chain(&ty),
                Ty::Imported {
                    module: ModuleKey::from("model"),
                    name: Ident::from("Answer"),
                },
                "the chain ends at the sibling's declaration"
            );
            a.required_variants(&ty)
        });
        let (union, variants) = got.expect("an alias of an imported union resolves");
        assert_eq!(
            union,
            UnionRef::Imported {
                module: "model".to_string(),
                name: "Answer".to_string(),
            }
        );
        assert_eq!(variants, vec![Ident::from("Yes"), Ident::from("No")]);
    }

    #[test]
    fn a_qualified_alias_of_an_imported_union_is_a_second_name() {
        // G302. D46 follows a name however it is spelled. `model.Answer` under
        // `import model`, and `m.Answer` under `import model as m`, lower to
        // the `Ty::Imported` the named spelling reaches, so the chain has to
        // end there too. It stopped at the alias's own name, and a match over
        // `Local` was held to no variant set at all.
        for (import, body) in [
            ("import model", "model.Answer"),
            ("import model as m", "m.Answer"),
        ] {
            let src = format!(
                "module app\n{import}\ntype Local = {body}\n\
                 fn run(a: Local) -> string {{\n  return \"x\"\n}}\n"
            );
            let got = with_assigner(&src, &ImportedAnswerDecl, |a| {
                let ty = first_param_ty(a);
                assert_eq!(
                    a.resolve_alias_chain(&ty),
                    Ty::Imported {
                        module: ModuleKey::from("model"),
                        name: Ident::from("Answer"),
                    },
                    "`{body}` ends at the sibling's declaration"
                );
                a.required_variants(&ty)
            });
            let (union, variants) = got.unwrap_or_else(|| panic!("`{body}` resolves a union"));
            assert_eq!(
                union,
                UnionRef::Imported {
                    module: "model".to_string(),
                    name: "Answer".to_string(),
                }
            );
            assert_eq!(variants, vec![Ident::from("Yes"), Ident::from("No")]);
        }
    }

    #[test]
    fn a_variants_payload_is_read_through_a_local_alias() {
        // The payload side of the same hop: a nested pattern under
        // `Circle(..)` over an `S` is typed and recursed into from `Shape`'s
        // declaration, so the alias cannot hide a payload the direct spelling
        // exposes.
        let src = r#"module app
type Shape =
  | Circle(number)
  | Square
type S = Shape
fn run(s: S) -> string {
  return "x"
}
"#;
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            a.variant_payload(&ty, &Ident::from("Circle"))
        });
        assert_eq!(got, Some(Ty::Prim(Primitive::Number)));
    }

    #[test]
    fn a_prelude_unions_type_end_is_a_builtin_with_no_declaration() {
        // `Result` has a fixed variant table and no declaration in any project
        // module, and it keys under the stdlib module that declares it
        // (G231): `std/result::Result`, which is the module the emitter
        // already writes the import from.
        let src = "module app\nfn f(r: Result<number, string>) -> number {\n  return 0\n}\n";
        let got = with_assigner(src, &ImportedAnswerDecl, |a| {
            let ty = first_param_ty(a);
            a.required_variants(&ty)
        });
        let (union, variants) = got.expect("a prelude Result resolves");
        assert_eq!(
            union,
            UnionRef::Builtin {
                module: "std/result".to_string(),
                declared: "Result".to_string(),
                name: "Result".to_string(),
            }
        );
        assert_eq!(variants, vec![Ident::from("Ok"), Ident::from("Err")]);
        assert_eq!(union.display(), "Result");
    }

    #[test]
    fn a_stdlib_unions_type_end_keys_under_its_module_and_prints_its_display_name() {
        // `fs.ErrorKind` is published by the stdlib stubs and declared nowhere
        // in the project. It keys as `std/fs::ErrorKind` (G231), and the name
        // it prints is still the one Glyph source writes, with the dot.
        let src = "module app\nfn f(n: number) -> number {\n  return n\n}\n";
        let ty = stdlib_named("fs", "ErrorKind");
        let got = with_assigner(src, &ImportedAnswerDecl, |a| a.required_variants(&ty));
        let (union, variants) = got.expect("a stdlib union resolves");
        assert_eq!(
            union,
            UnionRef::Builtin {
                module: "std/fs".to_string(),
                declared: "ErrorKind".to_string(),
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
            module: "std/result".to_string(),
            declared: "Result".to_string(),
            name: "Result".to_string(),
        };
        assert_eq!(
            CoverageTypeName::from(&builtin),
            CoverageTypeName::Builtin {
                module: "std/result".to_string(),
                declared: "Result".to_string(),
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
            Some(&DiagnosticDecl::Local {
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
            Some(&DiagnosticDecl::Imported {
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
            Some(&DiagnosticDecl::Builtin {
                module: "std/result".to_string(),
                declared: "Result".to_string(),
                name: "Result".to_string(),
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
            Some(&DiagnosticDecl::Local {
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

    // ----- D45: `Nullable<T>` is its own type, bridged explicitly -----

    #[test]
    fn a_nullable_is_not_an_option_at_an_argument() {
        let errs = errors_of(
            "module x\nfn take(o: Option<int>) -> int { return 1 }\n\
             fn f(n: Nullable<int>) -> int { return take(n) }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "a Nullable passed where an Option is declared must be E0211: {errs:?}"
        );
    }

    #[test]
    fn a_nullable_is_not_an_option_at_a_let_in_either_direction() {
        let errs = errors_of(
            "module x\nfn f(n: Nullable<int>) -> int {\n  let o: Option<int> = n\n  return 1\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "Nullable into Option must be E0204: {errs:?}"
        );
        let errs = errors_of(
            "module x\nfn f(o: Option<int>) -> int {\n  let n: Nullable<int> = o\n  return 1\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "Option into Nullable must be E0204: {errs:?}"
        );
    }

    #[test]
    fn nullable_of_option_is_rejected_in_a_record_field() {
        let errs = errors_of("module x\ntype Frame = { s: Nullable<Option<int>> }\n");
        let hit = errs
            .iter()
            .find(|e| matches!(e, TypeError::NullableNested { .. }))
            .unwrap_or_else(|| panic!("expected E0227 for Nullable<Option<int>>: {errs:?}"));
        assert_eq!(hit.code(), "E0227");
        assert!(hit.to_string().contains("Option<int>"), "{hit}");
    }

    #[test]
    fn nullable_of_nullable_is_rejected_in_a_parameter() {
        let errs = errors_of("module x\nfn f(n: Nullable<Nullable<int>>) -> int { return 1 }\n");
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NullableNested { .. })),
            "{errs:?}"
        );
    }

    #[test]
    fn nullable_over_an_alias_for_option_is_rejected() {
        // The rule is about what `T` is, not how it is spelled: an alias whose
        // body is `Option<int>` is still an `Option`.
        let errs = errors_of(
            "module x\ntype O = Option<int>\nfn f(n: Nullable<O>) -> int { return 1 }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::NullableNested { .. })),
            "{errs:?}"
        );
    }

    #[test]
    fn nullable_of_a_plain_type_is_accepted() {
        let errs = errors_of(
            "module x\ntype User = { name: string }\n\
             type Frame = { a: Nullable<int>, b: Nullable<Array<int>>, c: Nullable<User>, d?: Nullable<string> }\n\
             fn f(n: Nullable<int>) -> Nullable<int> {\n  let m: Nullable<int> = n\n  return m\n}\n",
        );
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::NullableNested { .. })),
            "{errs:?}"
        );
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "a Nullable<int> is assignable to a Nullable<int>: {errs:?}"
        );
    }

    #[test]
    fn to_option_returns_an_option_the_exhaustiveness_check_sees() {
        let errs = errors_of(
            "module x\nimport std/nullable\n\
             fn f(n: Nullable<int>) -> int {\n  return match nullable.to_option(n) {\n    Some(v) => v,\n  }\n}\n",
        );
        let hit = errs
            .iter()
            .find_map(|e| match e {
                TypeError::NonExhaustiveMatch { type_name, missing, .. } => {
                    Some((type_name.clone(), missing.clone()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected E0200 on the Option to_option returns: {errs:?}"));
        assert_eq!(hit.0, "Option");
        assert!(hit.1.contains("None"), "{hit:?}");
    }

    #[test]
    fn from_option_returns_a_nullable_not_an_option() {
        let errs = errors_of(
            "module x\nimport std/nullable\n\
             fn f(o: Option<int>) -> int {\n  let x: Option<int> = nullable.from_option(o)\n  return 1\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { expected, found, .. }
                if expected.starts_with("Option") && found.starts_with("Nullable"))),
            "{errs:?}"
        );
    }

    #[test]
    fn is_null_returns_a_bool() {
        let errs = errors_of(
            "module x\nimport std/nullable\n\
             fn f(n: Nullable<int>) -> int {\n  let s: string = nullable.is_null(n)\n  return 1\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { found, .. } if found == "bool")),
            "{errs:?}"
        );
    }

    // ----- G304 / D45: a member read through a `Nullable` -----

    /// The records every G304 test reads through. `Note` has a plain field, a
    /// string-literal union, a nested record, a `Nullable` record and a
    /// `Nullable` string, so each shape of chain has something to reach.
    const G304_TYPES: &str = "module x\nimport std/array\n\
         type Inner = { name: string }\n\
         type Note = { name: string, kind: \"a\" | \"b\", inner: Inner, maybe: Nullable<Inner>, label: Nullable<string> }\n";

    fn g304_errors(body: &str) -> Vec<TypeError> {
        errors_of(&format!("{G304_TYPES}{body}"))
    }

    fn g304_codes(body: &str) -> Vec<&'static str> {
        g304_errors(body).iter().map(|e| e.code()).collect()
    }

    #[test]
    fn a_plain_dot_read_through_a_nullable_is_e0235() {
        // TypeScript's TS18047: `n` may be null.
        let errs = g304_errors("fn f(n: Nullable<Note>) -> string {\n  return n.name\n}\n");
        let hits: Vec<_> = errs.iter().filter(|e| e.code() == "E0235").collect();
        assert_eq!(hits.len(), 1, "expected one E0235 for `n.name`: {errs:?}");
        let e = hits[0];
        assert!(
            matches!(e, TypeError::NullableRead { kind: crate::NullableReadKind::Dot, .. }),
            "the plain dot is its own form, not an escaping chain: {e:?}"
        );
        assert!(e.to_string().contains("may be null"), "{e}");
        assert!(e.help().is_some_and(|h| h.starts_with("Read the field with `?.`")), "{e:?}");
    }

    #[test]
    fn a_local_alias_of_a_nullable_is_read_as_the_nullable() {
        // The guarantee may not depend on which name the parameter wrote.
        let alias = "type MaybeNote = Nullable<Note>\n";
        let codes = g304_codes(&format!("{alias}fn f(n: MaybeNote) -> string {{\n  return n.name\n}}\n"));
        assert_eq!(codes, vec!["E0235"], "the plain dot");
        let codes = g304_codes(&format!("{alias}fn f(n: MaybeNote) -> Nullable<string> {{\n  return n?.name\n}}\n"));
        assert_eq!(codes, vec!["E0235"], "the escaping chain");
        let codes = g304_codes(&format!("{alias}fn f(n: MaybeNote) -> string {{\n  return n?.nmae ?? \"\"\n}}\n"));
        assert_eq!(codes, vec!["E0210"], "the field check");
        let errs = g304_errors(&format!("{alias}fn f(n: MaybeNote) -> string {{\n  return n?.name ?? \"\"\n}}\n"));
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn a_generic_alias_of_a_nullable_is_read_as_the_substituted_nullable() {
        // `type Maybe<T> = Nullable<T>` applied to `Note` is `Nullable<Note>`,
        // so the guarantee does not depend on whether the alias is generic.
        let alias = "type Maybe<T> = Nullable<T>\n";
        let codes = g304_codes(&format!("{alias}fn f(m: Maybe<Note>) -> string {{\n  return m.name\n}}\n"));
        assert_eq!(codes, vec!["E0235"], "the plain dot");
        let codes = g304_codes(&format!("{alias}fn f(m: Maybe<Note>) -> string {{\n  return \"hi ${{m?.name}}\"\n}}\n"));
        assert_eq!(codes, vec!["E0235"], "the interpolated chain");
        let codes = g304_codes(&format!("{alias}fn f(m: Maybe<Note>) -> string {{\n  return m?.nmae ?? \"\"\n}}\n"));
        assert_eq!(codes, vec!["E0210"], "the field check");
        let errs = g304_errors(&format!("{alias}fn f(m: Maybe<Note>) -> string {{\n  return m?.name ?? \"\"\n}}\n"));
        assert!(errs.is_empty(), "{errs:?}");
        // A generic alias of a generic alias substitutes through both hops.
        let two = "type Maybe<T> = Nullable<T>\ntype Opt<U> = Maybe<U>\n";
        let codes = g304_codes(&format!("{two}fn f(m: Opt<Note>) -> string {{\n  return m.name\n}}\n"));
        assert_eq!(codes, vec!["E0235"], "two hops");
    }

    #[test]
    fn an_escaping_chain_in_jsx_is_e0235_and_a_defaulted_one_is_clean() {
        // A JSX child holds the chain's value as an attribute does. `tsc`
        // accepts the child (a `ReactNode` may be `undefined`) and React
        // renders it as nothing, but Glyph has no type for `undefined`.
        let child = "component Card(props: { n: Nullable<Note> }) -> Component {\n  return <p>{props.n?.name}</p>\n}\n";
        assert_eq!(g304_codes(child), vec!["E0235"], "the JSX child");
        let attr = "component Label(props: { text: string }) -> Component {\n  return <p>{props.text}</p>\n}\n\
                    component Card(props: { n: Nullable<Note> }) -> Component {\n  return <Label text={props.n?.name} />\n}\n";
        assert_eq!(g304_codes(attr), vec!["E0235"], "the JSX attribute");
        let fine = "component Card(props: { n: Nullable<Note> }) -> Component {\n  return <p>{props.n?.name ?? \"\"}</p>\n}\n";
        let errs = g304_errors(fine);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn a_plain_dot_after_a_chain_reaches_a_nullable_field_is_e0235() {
        // `n?.maybe` is a `Nullable<Inner>`, so `.name` on it may read null.
        let codes = g304_codes("fn f(n: Nullable<Note>) -> string {\n  return n?.maybe.name ?? \"\"\n}\n");
        assert!(codes.contains(&"E0235"), "{codes:?}");
    }

    #[test]
    fn an_optional_chain_over_a_nullable_checks_the_field_against_the_record() {
        let errs = g304_errors("fn f(n: Nullable<Note>) -> string {\n  return n?.nmae ?? \"\"\n}\n");
        let e = errs
            .iter()
            .find_map(|e| match e {
                TypeError::UnknownField { field, type_name, suggestion, .. } => {
                    Some((field.clone(), type_name.clone(), suggestion.clone()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected E0210 for `n?.nmae`: {errs:?}"));
        assert_eq!(e.0, "nmae");
        assert_eq!(e.1, "Note");
        assert_eq!(e.2.as_deref(), Some("name"));
        assert!(!errs.iter().any(|e| e.code() == "E0235"), "the `??` handles it: {errs:?}");
    }

    #[test]
    fn a_chain_past_the_first_field_is_checked_against_the_inner_record() {
        let errs = g304_errors("fn f(n: Nullable<Note>) -> string {\n  return n?.inner.nmae ?? \"\"\n}\n");
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::UnknownField { type_name, .. } if type_name == "Inner")),
            "{errs:?}"
        );
    }

    #[test]
    fn an_optional_chain_over_a_nullable_that_escapes_is_e0235() {
        // Each position holds the `undefined` the chain produces on a null
        // `n`, and Glyph has no type for it.
        let cases = [
            ("a return", "fn f(n: Nullable<Note>) -> Nullable<string> {\n  return n?.name\n}\n"),
            (
                "a let",
                "fn f(n: Nullable<Note>) -> string {\n  let x = n?.name\n  return \"a\"\n}\n",
            ),
            (
                "an interpolation",
                "fn f(n: Nullable<Note>) -> void {\n  print(\"${n?.name}\")\n  return void\n}\n",
            ),
            (
                "an argument",
                "fn keep(v: unknown) -> void { return void }\n\
                 fn f(n: Nullable<Note>) -> void {\n  keep(n?.name)\n  return void\n}\n",
            ),
            (
                "a match scrutinee",
                "fn f(n: Nullable<Note>) -> string {\n  return match n?.kind {\n    \"a\" => \"A\",\n    else => \"other\",\n  }\n}\n",
            ),
            (
                "a field value",
                "type Out = { v: unknown }\n\
                 fn f(n: Nullable<Note>) -> Out {\n  return { v: n?.name }\n}\n",
            ),
            (
                "a deeper chain",
                "fn f(n: Nullable<Note>) -> Nullable<string> {\n  return n?.inner.name\n}\n",
            ),
            (
                "the right of `??`",
                "fn f(n: Nullable<Note>, m: Nullable<Note>) -> string {\n  let x = n?.name ?? m?.name\n  return \"a\"\n}\n",
            ),
        ];
        for (what, body) in cases {
            let errs = g304_errors(body);
            let hits: Vec<_> = errs.iter().filter(|e| e.code() == "E0235").collect();
            assert_eq!(hits.len(), 1, "{what}: expected one E0235: {errs:?}");
            assert!(
                hits[0].help().is_some_and(|h| h.contains("??") && h.contains("nullable.to_option")),
                "{what}: {:?}",
                hits[0]
            );
        }
    }

    #[test]
    fn an_optional_chain_over_a_nullable_is_legal_where_the_absence_is_handled() {
        let cases = [
            "fn f(n: Nullable<Note>) -> string {\n  return n?.name ?? \"none\"\n}\n",
            "fn f(n: Nullable<Note>) -> bool {\n  return n?.name == \"x\"\n}\n",
            "fn f(n: Nullable<Note>) -> bool {\n  return \"x\" != n?.inner.name\n}\n",
            "fn f(n: Nullable<Note>) -> string {\n  return n?.inner.name ?? \"none\"\n}\n",
            "fn f(n: Nullable<Note>) -> string {\n  return n?.maybe?.name ?? \"none\"\n}\n",
            "fn f(n: Nullable<Note>) -> string {\n  let s: string = n?.name ?? \"none\"\n  return s\n}\n",
            "fn f(n: Nullable<Note>, xs: Array<string>) -> Array<string> {\n  return array.map(xs, fn(x: string) -> string { return n?.name ?? x })\n}\n",
            "fn f(n: Nullable<Note>) -> string {\n  return match n?.name == \"x\" {\n    true => n?.name ?? \"\",\n    false => \"no\",\n  }\n}\n",
        ];
        for body in cases {
            let errs = g304_errors(body);
            assert!(errs.is_empty(), "expected no diagnostics for\n{body}\n{errs:?}");
        }
    }

    // ----- G311 / E0236: `?.` on an `Option` or a `Result` (record 87) -----

    /// The E0236 diagnostics in `errs`, as (message, help).
    fn e0236s(errs: &[TypeError]) -> Vec<(String, String)> {
        errs.iter()
            .filter(|e| e.code() == "E0236")
            .map(|e| (e.to_string(), e.help().map(|h| h.into_owned()).unwrap_or_default()))
            .collect()
    }

    /// An `Option` is never null, so `?.` never short-circuits on one: the
    /// read lands on the tag object. One E0236, and the field is not also
    /// checked against the payload (an E0210 would imply the read could be
    /// legal). The help opens the value with `match`, and points a wire null
    /// at `Nullable`, never at `nullable.from_option`.
    #[test]
    fn an_optional_chain_on_an_option_is_e0236() {
        let errs = g304_errors(
            "fn f(o: Option<Note>) -> string {\n  let x = o?.name\n  let y = o?.nmae\n  return \"a\"\n}\n",
        );
        assert_eq!(
            e0236s(&errs),
            vec![
                (
                    "`?.` on an `Option<Note>`: an `Option` is never null, so `?.` reads `name` off the `Option` itself, not off its `Some` value".to_string(),
                    "Open it with `match o { Some(n) => n.name, None => <default> }`. If the absence is a JSON `null` from the wire, declare the type `Nullable<Note>` (D45).".to_string(),
                ),
                (
                    "`?.` on an `Option<Note>`: an `Option` is never null, so `?.` reads `nmae` off the `Option` itself, not off its `Some` value".to_string(),
                    "Open it with `match o { Some(n) => n.nmae, None => <default> }`. If the absence is a JSON `null` from the wire, declare the type `Nullable<Note>` (D45).".to_string(),
                ),
            ],
            "{errs:?}"
        );
        assert_eq!(errs.len(), 2, "nothing but the two E0236: {errs:?}");
        assert!(errs.iter().all(|e| !e.help().unwrap_or_default().contains("from_option")));
    }

    /// `?.` is optional chaining, not `?` followed by `.`: on a `Result` it
    /// propagates nothing. A field read is told to propagate first and read
    /// after (the csvql form), a method call to drop the `?`. A method call
    /// through `?.` is refused too: `r?.map(f)` runs as `r.map(f)` but reads
    /// as "skip on `Err`".
    #[test]
    fn an_optional_chain_on_a_result_is_e0236_for_a_field_and_a_method() {
        let errs = g304_errors(
            "fn load() -> Result<Note, string> { return Ok({ name: \"a\", kind: \"a\", inner: { name: \"b\" }, maybe: null, label: null }) }\n\
             fn keep(n: Note) -> Note { return n }\n\
             fn f(r: Result<Note, string>) -> string {\n  let a = load()?.name\n  let b = r?.map(keep)\n  let c = r?.label\n  return \"a\"\n}\n",
        );
        let message = "`?.` on a `Result<Note, string>`: `?.` is optional chaining, not `?` followed by `.`; it does not propagate an `Err`, and a `Result` is never null";
        assert_eq!(
            e0236s(&errs),
            vec![
                (
                    message.to_string(),
                    "Propagate first, then read: `let n = load()?` then `n.name`.".to_string(),
                ),
                (message.to_string(), "Drop the `?`: `r.map(...)`.".to_string()),
                (
                    message.to_string(),
                    "Propagate first, then read: `let n = r?` then `n.label`.".to_string(),
                ),
            ],
            "{errs:?}"
        );
        assert!(!errs.iter().any(|e| e.code() == "E0210"), "{errs:?}");
    }

    /// Reported once, at the link where the chain starts: `o?.a.b` and
    /// `o?.a?.b` are one E0236 each, and nothing cascades from the node.
    #[test]
    fn an_optional_chain_on_an_option_is_reported_once_per_chain() {
        let errs = g304_errors(
            "fn f(o: Option<Note>, r: Result<Note, string>) -> string {\n  let a = o?.inner.name\n  let b = o?.inner?.name\n  let c = r?.inner.nmae.x\n  return \"a\"\n}\n",
        );
        let codes: Vec<&str> = errs.iter().map(|e| e.code()).collect();
        assert_eq!(codes, vec!["E0236", "E0236", "E0236"], "{errs:?}");
    }

    /// The receiver counts as an `Option` or a `Result` through any alias,
    /// generic ones included (D46). A user type that shadows the prelude
    /// name is not one: the name is resolved, not matched by spelling.
    #[test]
    fn an_optional_chain_on_an_aliased_option_or_result_is_e0236() {
        let errs = g304_errors(
            "type MaybeNote = Option<Note>\n\
             type Res<T> = Result<T, string>\n\
             type Deeper = Res<Note>\n\
             fn f(o: MaybeNote, r: Res<Note>, d: Deeper) -> string {\n  let a = o?.name\n  let b = r?.name\n  let c = d?.name\n  return \"a\"\n}\n",
        );
        let found: Vec<String> = e0236s(&errs).into_iter().map(|(m, _)| m).collect();
        assert_eq!(found.len(), 3, "{errs:?}");
        assert!(found[0].starts_with("`?.` on an `Option<Note>`"), "{found:?}");
        assert!(found[1].starts_with("`?.` on a `Result<Note, string>`"), "{found:?}");
        assert!(found[2].starts_with("`?.` on a `Result<Note, string>`"), "{found:?}");

        let errs = errors_of(
            "module x\n\
             type Option<T> = { name: T, }\n\
             fn f(o: Option<string>) -> string {\n  return o?.name\n}\n",
        );
        assert!(e0236s(&errs).is_empty(), "a user `Option` is not the prelude's: {errs:?}");
    }

    /// The `?.` over a `Nullable` is untouched (E0235's rules), and a plain
    /// dot on an `Option` or a `Result` is unchanged in this release: the
    /// member set on it is a later release (record 87, Q3).
    #[test]
    fn e0236_leaves_nullable_chains_and_the_plain_dot_alone() {
        let errs = g304_errors(
            "fn keep(n: Note) -> Note { return n }\n\
             fn f(n: Nullable<Note>, o: Option<Note>, r: Result<Note, string>) -> string {\n  let a = n?.name ?? \"none\"\n  let b = o.tag\n  let c = r.map(keep)\n  return a\n}\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
        let errs = g304_errors("fn f(n: Nullable<Note>) -> string {\n  let x = n?.name\n  return \"a\"\n}\n");
        let codes: Vec<&str> = errs.iter().map(|e| e.code()).collect();
        assert_eq!(codes, vec!["E0235"], "{errs:?}");
    }

    #[test]
    fn a_defaulted_chain_has_the_field_type() {
        // `n?.name ?? "none"` is a `string`, so it is refused where a `number`
        // is declared (TS2322 under `tsc`).
        let errs = g304_errors(
            "fn f(n: Nullable<Note>) -> number {\n  let k: number = n?.name ?? \"none\"\n  return k\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { expected, found, .. }
                if expected == "number" && found == "string")),
            "{errs:?}"
        );
        assert!(!errs.iter().any(|e| e.code() == "E0235"), "{errs:?}");
    }

    #[test]
    fn a_defaulted_chain_to_a_nullable_field_has_the_inner_type() {
        // `label` is a `Nullable<string>`; `??` replaces its null as well.
        let errs = g304_errors(
            "fn f(n: Nullable<Note>) -> number {\n  let k: number = n?.label ?? \"none\"\n  return k\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { found, .. } if found == "string")),
            "{errs:?}"
        );
        let errs = g304_errors(
            "fn f(n: Nullable<Note>) -> string {\n  let s: string = n?.label ?? \"none\"\n  return s\n}\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn a_default_that_does_not_fit_leaves_the_coalesce_to_tsc() {
        // `string | number` has no Glyph spelling; the checker claims nothing.
        let errs = g304_errors(
            "fn f(n: Nullable<Note>) -> number {\n  let k: number = n?.name ?? 3\n  return k\n}\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn a_coalesce_on_the_nullable_itself_has_the_inner_type() {
        let errs = g304_errors(
            "fn f(n: Nullable<string>) -> string {\n  let s: string = n ?? \"none\"\n  return s\n}\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
        let errs = g304_errors(
            "fn f(n: Nullable<string>) -> number {\n  let k: number = n ?? \"none\"\n  return k\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::TypeMismatch { expected, found, .. }
                if expected == "number" && found == "string")),
            "{errs:?}"
        );
    }

    /// D46. `type B = A` is a second name for `A`'s declaration, so a `B`
    /// passes where an `A` is declared and the reverse. G203 recorded the
    /// E0211 this used to draw, on a program `tsc --strict` accepts.
    #[test]
    fn a_bare_path_alias_is_a_second_name_for_the_declaration_it_resolves_to() {
        let errs = errors_of(
            "module x\n\
             type A = { x: number, }\n\
             type B = A\n\
             fn takes_a(a: A) -> number {\n  return a.x\n}\n\
             fn takes_b(b: B) -> number {\n  return b.x\n}\n\
             fn go(a: A, b: B) -> number {\n  return takes_a(b) + takes_b(a)\n}\n",
        );
        assert!(errs.is_empty(), "an alias passes both ways: {errs:?}");
    }

    /// The hop reads a chain and stops at the declaration, so two aliases of
    /// one record are one type between themselves as well, and an alias one
    /// level inside a generic application is resolved where the application's
    /// arguments are compared.
    #[test]
    fn an_alias_chain_is_followed_to_its_end() {
        let errs = errors_of(
            "module x\n\
             type A = { x: number, }\n\
             type B = A\n\
             type C = B\n\
             fn takes_a(a: A) -> number {\n  return a.x\n}\n\
             fn takes_bs(bs: Array<B>) -> number {\n  return 1\n}\n\
             fn go(c: C, cs: Array<C>) -> number {\n  return takes_a(c) + takes_bs(cs)\n}\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// D49. Nominal typing is untouched where no alias is involved: a second record
    /// of the same shape is another type (D49), and the E0211 stays.
    #[test]
    fn a_separately_declared_record_of_the_same_shape_is_still_another_type() {
        let errs = errors_of(
            "module x\n\
             type A = { x: number, }\n\
             type C = { x: number, }\n\
             fn takes_a(a: A) -> number {\n  return a.x\n}\n\
             fn go(c: C) -> number {\n  return takes_a(c)\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "{errs:?}"
        );
    }

    /// A generic application is not a bare path and is not followed, and the
    /// ruling decides no new incompatibility from that, so the answer for
    /// `type Boxed<T> = W<T>` against `W<number>` is what it was before: the
    /// nominal E0211. Pinned so a widening of the hop is a deliberate change.
    #[test]
    fn a_generic_application_alias_is_not_followed() {
        let errs = errors_of(
            "module x\n\
             type W<T> = { v: T, }\n\
             type Boxed<T> = W<T>\n\
             fn takes_w(w: W<number>) -> number {\n  return 1\n}\n\
             fn go(b: Boxed<number>) -> number {\n  return takes_w(b)\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::ArgumentTypeMismatch { .. })),
            "{errs:?}"
        );
    }

    /// G206. A field typo through a module-local alias is E0210, as it already
    /// was through the same alias imported from a sibling module.
    #[test]
    fn a_field_typo_through_a_local_alias_is_e0210() {
        let errs = errors_of(
            "module x\n\
             type A = { x: number, }\n\
             type B = A\n\
             fn go(b: B) -> number {\n  return b.naem\n}\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::UnknownField { field, .. } if field == "naem"
            )),
            "{errs:?}"
        );
    }

    /// G302's message. E0210 through a second name names the name the site
    /// wrote and the declaration the fields come from, so the reader does not
    /// have to go through `type Page = Doc` to find them. The parenthetical
    /// appears only when a chain was followed, however many hops it took, and
    /// on each site that raises E0210 for a record.
    #[test]
    fn a_field_typo_through_a_second_name_names_both_names() {
        let errs = errors_of(
            "module x\n\
             type Doc = { title: string, }\n\
             type Page = Doc\n\
             type Again = Page\n\
             fn a(p: Page) -> string {\n  return p.titel\n}\n\
             fn b(q: Again) -> string {\n  return q.titel\n}\n\
             fn c(d: Doc) -> string {\n  return d.titel\n}\n\
             component Row(props: Page) -> Component {\n  return <p>a</p>\n}\n\
             component V() -> Component {\n  return <Row titel={\"a\"} title={\"b\"} />\n}\n",
        );
        let got: Vec<String> = errs
            .iter()
            .filter(|e| matches!(e, TypeError::UnknownField { .. }))
            .map(|e| e.to_string())
            .collect();
        assert_eq!(
            got,
            vec![
                "type `Page` (a second name for `Doc`) has no field `titel`; did you mean `title`?",
                "type `Again` (a second name for `Doc`) has no field `titel`; did you mean `title`?",
                "type `Doc` has no field `titel`; did you mean `title`?",
                "type `Page` (a second name for `Doc`) has no field `titel`; did you mean `title`?",
            ],
            "{errs:?}"
        );
    }

    /// `B.parse` is `A.parse`: the descriptor member resolves through the
    /// alias, so the parsed value carries the record's field set.
    #[test]
    fn parse_on_a_local_alias_resolves_to_the_records_descriptor() {
        let errs = errors_of(
            "module x\n\
             type A = { x: number, }\n\
             type B = A\n\
             fn go(v: unknown) -> number {\n\
             \x20 return match B.parse(v) {\n\
             \x20\x20\x20 Ok(p) => p.naem,\n\
             \x20\x20\x20 Err(_) => 0,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::UnknownField { field, .. } if field == "naem"
            )),
            "{errs:?}"
        );
    }

    /// A field reached through an alias is a site over the record the chain
    /// ends at, which is the rule the cross-module path already applies: an
    /// alias declares no field of its own, so keying the site under it would
    /// put it in the impact set of a rename that cannot touch it.
    #[test]
    fn a_field_access_through_a_local_alias_is_keyed_under_the_record() {
        struct NoCrossModule;
        impl DeclTyResolver for NoCrossModule {
            fn decl_ty(&self, _decl_idx: u32) -> Ty {
                Ty::Unknown
            }
        }
        let src = "module x\n\
                   type A = { x: number, }\n\
                   type B = A\n\
                   fn go(b: B) -> number {\n  return b.x\n}\n";
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let (_tm, ty_errs, _cov, uses) =
            assign_types_with_relations(&m, &resolved, &prelude, &NoCrossModule);
        assert!(ty_errs.is_empty(), "{ty_errs:?}");
        let reads: Vec<_> = uses
            .sites()
            .iter()
            .filter(|s| matches!(s.access(), FieldAccess::Read))
            .collect();
        assert_eq!(reads.len(), 1, "one read of `x`: {:?}", uses.sites());
        assert!(
            matches!(reads[0].owner(), FieldOwner::Declared { name, .. } if name == "A"),
            "keyed under the record, not the alias: {:?}",
            reads[0].owner()
        );
    }

    /// `type A = B` and `type B = A` name nothing. The resolver accepts the
    /// pair today, so the hop has to terminate on it rather than loop.
    #[test]
    fn a_cyclic_alias_pair_terminates() {
        let _ = errors_of(
            "module x\n\
             type A = B\n\
             type B = A\n\
             fn f(a: A) -> number {\n  return 1\n}\n\
             fn go(b: B) -> number {\n  return f(b)\n}\n",
        );
    }

    /// G205. An imported const's type comes from `imported_const_decl`, under
    /// both import spellings, so a field typo on it is E0210 naming the record
    /// exactly as it is in the declaring module. The resolver here stands in
    /// for `glyph_db::exported_const` and answers the one const it knows; the
    /// unannotated const it does not know stays `Unknown` and silent.
    #[test]
    fn an_imported_const_is_checked_against_its_annotation_under_both_spellings() {
        struct OneImportedConst;
        impl DeclTyResolver for OneImportedConst {
            fn decl_ty(&self, _decl_idx: u32) -> Ty {
                Ty::Unknown
            }
            fn imported_const_decl(&self, module_path: &str, const_name: &str) -> Option<Ty> {
                (module_path == "lib" && const_name == "ORIGIN").then(|| Ty::Imported {
                    module: "lib".into(),
                    name: Ident::from("Sheet"),
                })
            }
            fn imported_type_decl(
                &self,
                module_path: &str,
                type_name: &str,
            ) -> Option<ImportedTypeDecl> {
                (module_path == "lib" && type_name == "Sheet").then(|| ImportedTypeDecl {
                    name: Ident::from("Sheet"),
                    generics: Vec::new(),
                    body: Ty::Record {
                        fields: vec![RecordField {
                            name: Ident::from("rows"),
                            ty: Ty::Prim(Primitive::Number),
                            optional: false,
                        }],
                    },
                    open: false,
                })
            }
        }
        let src = "module app\n\
                   import lib { ORIGIN, LOOSE }\n\
                   import lib as l\n\
                   fn named() -> number {\n  return ORIGIN.rowz\n}\n\
                   fn spaced() -> number {\n  return l.ORIGIN.rowz\n}\n\
                   fn loose() -> number {\n  return LOOSE.rowz\n}\n";
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types_with_resolver(&m, &resolved, &prelude, &OneImportedConst);
        let typos: Vec<_> = errs
            .iter()
            .filter(|e| matches!(e, TypeError::UnknownField { field, type_name, .. } if field == "rowz" && type_name == "Sheet"))
            .collect();
        assert_eq!(typos.len(), 2, "one per annotated spelling, none for the unannotated const: {errs:?}");
    }

    // ----- G216: a prelude container against a primitive -----

    fn has_type_name_as_value(errs: &[TypeError], name: &str) -> Option<String> {
        errs.iter().find_map(|e| match e {
            TypeError::TypeNameAsValue {
                name: n,
                construction,
                ..
            } if n == name => Some(construction.clone()),
            _ => None,
        })
    }

    #[test]
    fn an_option_into_a_string_annotation_is_flagged() {
        // The ledger program. `Option<int>` emits as a tagged object, so the
        // `let` annotated `string` is holding something that is not one, and
        // `definitely_incompatible` had no arm to say so.
        let errs = ty_errors_of(
            "module x\nfn main() -> void {\n  let o: Option<int> = Some(3)\n  let x: string = o\n  print(x)\n}\n",
        );
        assert!(
            has_type_mismatch(&errs, "string", "Option<number>"),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_nullable_argument_into_an_int_parameter_is_flagged() {
        // The case tsc cannot make: the emitted `const v: number | null = 3`
        // narrows back to `number` before the call is checked there.
        let errs = ty_errors_of(
            "module x\nfn takes_int(n: int) -> int {\n  return n\n}\nfn main() -> int {\n  let v: Nullable<int> = 3\n  return takes_int(v)\n}\n",
        );
        assert!(
            has_argument_mismatch(&errs, "number", "Nullable<number>"),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn an_array_into_a_string_return_is_flagged() {
        let errs = ty_errors_of(
            "module x\nfn names(xs: Array<string>) -> string {\n  return xs\n}\n",
        );
        assert!(
            has_type_mismatch(&errs, "string", "Array<string>"),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_string_into_an_option_annotation_is_flagged() {
        // The reverse direction for the four containers that are decided by
        // the container alone.
        let errs = ty_errors_of(
            "module x\nfn main() -> void {\n  let o: Option<int> = \"three\"\n  print(o)\n}\n",
        );
        assert!(
            has_type_mismatch(&errs, "Option<number>", "string"),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_string_into_a_nullable_int_annotation_is_flagged() {
        // `Nullable<T>` is decided against `T`, not against the container.
        let errs = ty_errors_of(
            "module x\nfn main() -> void {\n  let v: Nullable<int> = \"three\"\n  print(v)\n}\n",
        );
        assert!(
            has_type_mismatch(&errs, "Nullable<number>", "string"),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_number_into_a_nullable_int_annotation_is_accepted() {
        // The whole point of D45: a `Nullable<int>` holds an `int` or null, so
        // writing the `int` is the intended spelling and must stay silent.
        // Returned rather than printed: `print` takes a `string` (G303).
        let errs = ty_errors_of(
            "module x\nfn main() -> Nullable<int> {\n  let v: Nullable<int> = 3\n  return v\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_schema_application_against_a_string_stays_undetermined() {
        // `Schema<T>` is an interop surface type whose shape comes from a
        // library rather than from the emitter, so `prelude_container` leaves
        // it out and the pairing is not decided here.
        let errs = ty_errors_of(
            "module x\nfn f(s: Schema<int>) -> string {\n  return s\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_container_against_a_named_type_stays_undetermined() {
        // A name whose declaration this rule does not read. `type Id = string`
        // is a primitive alias, which the local G201 reading excludes too.
        let errs = ty_errors_of(
            "module x\ntype Id = string\nfn f(o: Option<int>) -> Id {\n  return o\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn an_alias_of_a_container_stays_undetermined() {
        // `alias_target` does not follow a body that is a generic application,
        // so `MaybeInt` never reaches the relation as an `App`. Recorded as a
        // test because it is the reading the rule declines, not an oversight
        // waiting to be found again.
        let errs = ty_errors_of(
            "module x\ntype MaybeInt = Option<int>\nfn f(o: MaybeInt) -> string {\n  return o\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    // ----- G217: a type name where a value is wanted -----

    #[test]
    fn a_record_type_name_in_expression_position_names_the_literal() {
        let errs = ty_errors_of(
            "module x\ntype Order = {\n  id: string,\n  total: int,\n}\nfn make() -> Order {\n  return Order { id: \"a\", total: 1 }\n}\n",
        );
        let construction =
            has_type_name_as_value(&errs, "Order").unwrap_or_else(|| panic!("errs: {errs:?}"));
        assert!(
            construction.contains("{ id: ..., total: ... }"),
            "construction: {construction}"
        );
    }

    #[test]
    fn a_union_type_name_in_expression_position_names_its_variants() {
        let errs = ty_errors_of(
            "module x\ntype Status =\n  | Pending\n  | Paid({ transaction_id: string })\nfn make() -> Status {\n  return Status\n}\n",
        );
        let construction =
            has_type_name_as_value(&errs, "Status").unwrap_or_else(|| panic!("errs: {errs:?}"));
        assert!(
            construction.contains("`Pending`") && construction.contains("`Paid(record)`"),
            "construction: {construction}"
        );
    }

    #[test]
    fn a_string_literal_union_type_name_names_its_literals() {
        let errs = ty_errors_of(
            "module x\ntype Tier = \"free\" | \"pro\"\nfn make() -> Tier {\n  return Tier\n}\n",
        );
        let construction =
            has_type_name_as_value(&errs, "Tier").unwrap_or_else(|| panic!("errs: {errs:?}"));
        assert!(
            construction.contains("\"free\"") && construction.contains("\"pro\""),
            "construction: {construction}"
        );
    }

    #[test]
    fn an_interface_name_in_expression_position_says_it_is_a_shape() {
        let errs = ty_errors_of(
            "module x\ninterface Describable {\n  fn describe() -> string\n}\nfn make() -> unknown {\n  return Describable\n}\n",
        );
        let construction =
            has_type_name_as_value(&errs, "Describable").unwrap_or_else(|| panic!("errs: {errs:?}"));
        assert!(construction.contains("interface"), "construction: {construction}");
    }

    #[test]
    fn a_prelude_type_name_in_expression_position_is_flagged() {
        let errs = ty_errors_of("module x\nfn make() -> unknown {\n  return string\n}\n");
        assert!(
            has_type_name_as_value(&errs, "string").is_some(),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_descriptor_receiver_is_not_a_type_name_used_as_a_value() {
        // `Order.parse(json)` and `Order.is(v)` are the two forms where naming
        // the type in an expression is the point.
        let errs = ty_errors_of(
            "module x\ntype Order = {\n  id: string,\n}\nfn read(raw: unknown) -> Result<Order, Array<Issue>> {\n  return Order.parse(raw)\n}\n",
        );
        assert!(
            has_type_name_as_value(&errs, "Order").is_none(),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn a_variant_name_used_as_a_value_is_not_flagged() {
        let errs = ty_errors_of(
            "module x\ntype Status =\n  | Pending\n  | Done\nfn make() -> Status {\n  let s: Status = Pending\n  return s\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_function_name_used_as_a_value_is_not_flagged() {
        let errs = ty_errors_of(
            "module x\nfn helper(n: int) -> int {\n  return n\n}\nfn make() -> fn(n: int) -> int {\n  return helper\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn a_prelude_value_name_used_as_a_value_is_not_flagged() {
        // `None` is a prelude *value*: the classification table has to keep
        // the two halves of the prelude apart.
        let errs = ty_errors_of(
            "module x\nfn make() -> Option<int> {\n  return None\n}\n",
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn an_imported_record_type_name_in_expression_position_is_flagged() {
        // The same mistake with the declaration in a sibling: the answer comes
        // from `imported_type_decl`, so it does not depend on which file the
        // record was declared in.
        let errs = lib_errors_of(
            "module x\nimport lib { Point }\nfn make() -> Point {\n  return Point { x: 1 }\n}\n",
        );
        let construction =
            has_type_name_as_value(&errs, "Point").unwrap_or_else(|| panic!("errs: {errs:?}"));
        assert!(
            construction.contains("{ x: ... }"),
            "construction: {construction}"
        );
    }

    #[test]
    fn an_imported_type_name_as_a_descriptor_receiver_is_not_flagged() {
        let errs = lib_errors_of(
            "module x\nimport lib { Point }\nfn read(raw: unknown) -> unknown {\n  return Point.parse(raw)\n}\n",
        );
        assert!(
            has_type_name_as_value(&errs, "Point").is_none(),
            "errs: {errs:?}"
        );
    }

    // ----- G230: a declared string-literal union -----

    /// Every literal a `TypeMismatch` says the declared type accepts.
    fn mismatch_accepted(errs: &[TypeError]) -> Vec<Vec<String>> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::TypeMismatch { accepted, .. }
                | TypeError::ArgumentTypeMismatch { accepted, .. } => Some(accepted.clone()),
                _ => None,
            })
            .map(|a| a.map(|a| a.values).unwrap_or_default())
            .collect()
    }

    /// The kind each of those lists carries, so the values and the kind are
    /// pinned together rather than one at a time.
    fn mismatch_accepted_kinds(errs: &[TypeError]) -> Vec<Option<AlternativesKind>> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::TypeMismatch { accepted, .. }
                | TypeError::ArgumentTypeMismatch { accepted, .. } => {
                    Some(accepted.as_ref().map(|a| a.kind))
                }
                _ => None,
            })
            .collect()
    }

    fn mismatch_found(errs: &[TypeError]) -> Vec<String> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::TypeMismatch { found, .. }
                | TypeError::ArgumentTypeMismatch { found, .. } => Some(found.clone()),
                _ => None,
            })
            .collect()
    }

    const MODE: &str = "module m\ntype Mode = \"read\" | \"write\"\n";

    /// The finding. `type Mode = \"read\" | \"write\"` refused nothing at all:
    /// a `number`, a `bool`, a record, a tagged union and a prelude container
    /// all passed where `Mode` was declared, and only `tsc` said otherwise.
    #[test]
    fn a_value_that_is_not_a_string_where_a_string_literal_union_is_declared_is_refused() {
        let errs = errors_of(&format!(
            "{MODE}\
             type Point = {{ x: number }}\n\
             type Shape = | Circle(number) | Square(number)\n\
             fn f(n: number, b: bool, p: Point, sh: Shape, o: Option<number>) -> void {{\n\
             \x20 let a: Mode = n\n\
             \x20 let b2: Mode = b\n\
             \x20 let c: Mode = p\n\
             \x20 let d: Mode = sh\n\
             \x20 let e: Mode = o\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["number", "bool", "Point", "Shape", "Option<number>"], "errs: {errs:?}");
        for accepted in mismatch_accepted(&errs) {
            assert_eq!(accepted, vec!["read".to_string(), "write".to_string()]);
        }
    }

    /// The literal the author wrote decides the pairing, because the type does
    /// not: Glyph types every string literal `string`.
    #[test]
    fn a_string_literal_is_decided_against_the_declared_set() {
        let errs = errors_of(&format!(
            "{MODE}fn f() -> void {{\n  let a: Mode = \"read\"\n  let b: Mode = \"rw\"\n}}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["\"rw\""], "only the literal outside the set: {errs:?}");
        assert_eq!(mismatch_accepted(&errs)[0], vec!["read".to_string(), "write".to_string()]);
        // And the set is a set of literals, which is what a consumer has to
        // know to put the quotes back: `read` is not a value, `"read"` is.
        assert_eq!(
            mismatch_accepted_kinds(&errs),
            vec![Some(AlternativesKind::Literals)],
            "errs: {errs:?}"
        );
    }

    /// An object literal is typed `Ty::Unknown`, so the record rule never sees
    /// one. The written form is read instead, and it names the value.
    #[test]
    fn an_object_literal_where_a_string_literal_union_is_declared_is_refused() {
        let errs = errors_of(&format!(
            "{MODE}fn f() -> void {{\n  let a: Mode = {{ x: 1 }}\n}}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["record"], "errs: {errs:?}");
    }

    /// A `string` that no written form pins to a narrower set. `tsc` refuses
    /// it (`TS2322`), and so does this.
    #[test]
    fn a_bare_string_where_a_string_literal_union_is_declared_is_refused() {
        let errs = errors_of(&format!(
            "{MODE}fn f(s: string) -> void {{\n  let a: Mode = s\n}}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["string"], "errs: {errs:?}");
    }

    /// The exception to the arm above, and the one `csvql` contains. Every arm
    /// of a value-position `match` is a literal, the walk joins them to
    /// `string`, and the program is one `tsc` compiles.
    #[test]
    fn a_value_position_match_of_literals_is_read_as_its_literal_set() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn ok(name: string) -> Mode {{\n\
             \x20 return match name {{\n\
             \x20   \"read\" => \"read\",\n\
             \x20   else => \"write\",\n\
             \x20 }}\n\
             }}\n"
        ));
        assert!(errs.is_empty(), "errs: {errs:?}");

        let bad = errors_of(&format!(
            "{MODE}\
             fn no(name: string) -> Mode {{\n\
             \x20 return match name {{\n\
             \x20   \"read\" => \"read\",\n\
             \x20   else => \"exec\",\n\
             \x20 }}\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&bad), vec!["\"read\" | \"exec\""], "errs: {bad:?}");
    }

    /// An arm whose value went through a binding is a `string`, and the join
    /// of a `string` with a literal is a `string`, so the `match` is refused
    /// where a `Mode` is declared.
    ///
    /// It used to be undetermined, deliberately, because the checker could not
    /// tell a bare `string` arm from a literal one. It can now, and the answer
    /// it reaches is `tsc`'s: run on this exact program, 0.1.122 is silent
    /// under `--no-tsc` and `TS2322: Type 'string' is not assignable to type
    /// 'Mode'` with `tsc` in the loop, where this build is `E0204` without
    /// `tsc`.
    #[test]
    fn a_match_arm_whose_value_went_through_a_binding_is_a_string() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn f(name: string) -> Mode {{\n\
             \x20 return match name {{\n\
             \x20   \"read\" => {{ let x = \"read\"\n    x }},\n\
             \x20   else => \"write\",\n\
             \x20 }}\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["string"], "errs: {errs:?}");
    }

    /// The reverse direction. Every literal in the set is a `string`, and
    /// nothing else the relation reads accepts one.
    #[test]
    fn a_string_literal_union_value_is_a_string_and_nothing_else() {
        let errs = errors_of(&format!(
            "{MODE}\
             type Point = {{ x: number }}\n\
             fn f(m: Mode) -> void {{\n\
             \x20 let a: string = m\n\
             \x20 let b: number = m\n\
             \x20 let c: Point = m\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["Mode", "Mode"], "the `string` is accepted: {errs:?}");
    }

    /// Two literal sets, compared as sets. A subset stands; a literal outside
    /// the declared set does not.
    #[test]
    fn two_string_literal_unions_are_compared_by_literal_set() {
        let errs = errors_of(
            "module m\n\
             type Mode = \"read\" | \"write\"\n\
             type Wide = \"read\" | \"write\" | \"exec\"\n\
             type Narrow = \"read\"\n\
             fn f(m: Mode, w: Wide, n: Narrow) -> void {\n\
             \x20 let a: Wide = m\n\
             \x20 let b: Mode = n\n\
             \x20 let c: Mode = w\n\
             \x20 let d: Narrow = m\n\
             }\n",
        );
        assert_eq!(mismatch_found(&errs), vec!["Wide", "Mode"], "only the supersets: {errs:?}");
    }

    /// A second name for the declaration is the declaration (D46), on both
    /// sides of the pairing.
    #[test]
    fn an_alias_of_a_string_literal_union_follows_the_chain() {
        let errs = errors_of(&format!(
            "{MODE}\
             type Alias = Mode\n\
             type Second = Alias\n\
             fn f() -> void {{\n\
             \x20 let a: Second = \"rw\"\n\
             \x20 let b: Second = \"read\"\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["\"rw\""], "errs: {errs:?}");
    }

    /// `Nullable<T>` is descended with the union intact, so the literal is
    /// read one level in exactly as it is at the top.
    #[test]
    fn a_nullable_string_literal_union_is_decided_by_its_argument() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn f(n: number) -> void {{\n\
             \x20 let a: Nullable<Mode> = \"read\"\n\
             \x20 let b: Nullable<Mode> = \"rw\"\n\
             \x20 let c: Nullable<Mode> = n\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["\"rw\"", "number"], "errs: {errs:?}");
    }

    /// A call argument is the same relation at a different site, and the code
    /// is E0211.
    #[test]
    fn a_call_argument_against_a_string_literal_union_parameter_is_e0211() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn takes(m: Mode) -> Mode {{\n  return m\n}}\n\
             fn f(n: number) -> void {{\n\
             \x20 print(takes(\"read\"))\n\
             \x20 print(takes(\"rw\"))\n\
             \x20 print(takes(n))\n\
             }}\n"
        ));
        let codes: Vec<&str> = errs.iter().map(|e| e.code()).collect();
        assert_eq!(codes, vec!["E0211", "E0211"], "errs: {errs:?}");
        assert_eq!(mismatch_found(&errs), vec!["\"rw\"", "number"]);
    }

    /// G215's rule for this shape: a union declared in a sibling module is
    /// decided the way a local one is, under the named-import spelling and the
    /// namespace spelling alike.
    #[test]
    fn an_imported_string_literal_union_is_decided_like_a_local_one() {
        struct ModesModule;
        impl DeclTyResolver for ModesModule {
            fn decl_ty(&self, _decl_idx: u32) -> Ty {
                Ty::Unknown
            }
            fn imported_type_decl(
                &self,
                module_path: &str,
                type_name: &str,
            ) -> Option<ImportedTypeDecl> {
                (module_path == "modes" && type_name == "Mode").then(|| ImportedTypeDecl {
                    name: Ident::from("Mode"),
                    generics: Vec::new(),
                    body: Ty::StringLiteralUnion(vec!["read".to_string(), "write".to_string()]),
                    open: false,
                })
            }
        }
        let src = "module app\n\
                   import modes { Mode }\n\
                   import modes as md\n\
                   fn named(n: number) -> void {\n\
                   \x20 let a: Mode = \"read\"\n\
                   \x20 let b: Mode = \"rw\"\n\
                   \x20 let c: Mode = n\n\
                   }\n\
                   fn spaced(n: number) -> void {\n\
                   \x20 let a: md.Mode = \"read\"\n\
                   \x20 let b: md.Mode = \"rw\"\n\
                   \x20 let c: md.Mode = n\n\
                   }\n";
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types_with_resolver(&m, &resolved, &prelude, &ModesModule);
        assert_eq!(
            mismatch_found(&errs),
            vec!["\"rw\"", "number", "\"rw\"", "number"],
            "the boundary does not change the answer: {errs:?}"
        );
        for accepted in mismatch_accepted(&errs) {
            assert_eq!(accepted, vec!["read".to_string(), "write".to_string()]);
        }
    }

    /// What [`assignability`] answers for the three directions, which is what
    /// `glyph_assignable` reports.
    #[test]
    fn assignability_answers_the_string_literal_union_pairings() {
        let src = "module m\ntype Mode = \"read\" | \"write\"\ntype Wide = \"read\" | \"write\" | \"exec\"\n";
        assert!(matches!(
            assignability_of(src, "Mode", "Wide"),
            Assignability::Compatible { .. }
        ));
        assert!(matches!(
            assignability_of(src, "Mode", "string"),
            Assignability::Compatible { .. }
        ));
        assert_eq!(
            assignability_of(src, "Wide", "Mode"),
            Assignability::Incompatible
        );
        assert_eq!(
            assignability_of(src, "number", "Mode"),
            Assignability::Incompatible
        );
        assert_eq!(
            assignability_of(src, "string", "Mode"),
            Assignability::Incompatible
        );
        assert_eq!(
            assignability_of(src, "Mode", "number"),
            Assignability::Incompatible
        );
        // An undecidable value where the union is declared is not read at all,
        // and the relation says so rather than accepting it.
        assert_eq!(
            assignability_of(src, "unknown", "Mode"),
            Assignability::NoRule
        );
    }

    // ----- the generic-argument boundary (G230, corrected) -----

    /// The review's `fp3` and `fp4`, which 0.1.122 fenced and this decides.
    /// `["read", "write"]` is an `Array<"read" | "write">` (G237), so the
    /// `Ty::App` recursion asks two literal sets and the subset rule accepts
    /// them. `tsc --strict` reads the same expressions the same way and
    /// compiles all four.
    #[test]
    fn a_written_literal_under_a_generic_argument_is_accepted() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn takes(xs: Array<Mode>) -> number {{\n  return xs.length\n}}\n\
             fn arr() -> Array<Mode> {{\n  return [\"read\", \"write\"]\n}}\n\
             fn call() -> number {{\n  return takes([\"read\"])\n}}\n\
             fn deep() -> Array<Array<Mode>> {{\n  return [[\"read\"]]\n}}\n"
        ));
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
    }

    /// A `number`, a `bool` and a literal set that is not a subset are
    /// refused inside `Array<Mode>` the same way they are refused against
    /// `Mode`.
    #[test]
    fn a_wrong_value_under_a_generic_argument_is_still_refused() {
        let errs = errors_of(&format!(
            "{MODE}\
             type Wide = \"read\" | \"write\" | \"exec\"\n\
             fn f(ns: Array<number>, bs: Array<bool>, w: Array<Wide>) -> void {{\n\
             \x20 let a: Array<Mode> = ns\n\
             \x20 let b: Array<Mode> = bs\n\
             \x20 let c: Array<Mode> = w\n\
             }}\n"
        ));
        assert_eq!(
            mismatch_found(&errs),
            vec!["Array<number>", "Array<bool>", "Array<Wide>"],
            "errs: {errs:?}"
        );
    }

    /// The review's `fp2`, decided. `fn id<T>(x: T) -> T` called with a
    /// string literal binds `T` to the literal type the walk now records, so
    /// `id("read")` is a `Mode` here exactly as it is under `tsc`, and
    /// `id(s)` for a `string`-typed `s` is refused there and here.
    #[test]
    fn a_generic_call_returning_its_type_parameter_is_instantiated() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn id<T>(x: T) -> T {{\n  return x\n}}\n\
             fn plain() -> string {{\n  return \"read\"\n}}\n\
             fn viaGeneric() -> Mode {{\n  return id(\"read\")\n}}\n\
             fn viaBad() -> Mode {{\n  return id(\"nope\")\n}}\n\
             fn viaString(s: string) -> Mode {{\n  return id(s)\n}}\n\
             fn viaPlain() -> Mode {{\n  return plain()\n}}\n"
        ));
        assert_eq!(
            mismatch_found(&errs),
            vec!["\"nope\"", "string", "string"],
            "the instantiated literal is accepted, the wrong one and the two \
             `string`s are refused: {errs:?}"
        );
    }

    // ----- literal-typed expressions (G237) -----

    /// Every pairing the literal type decides, in both directions, on one
    /// program. Each verdict below was established by running `tsc --strict`
    /// on the equivalent TypeScript: the accepted ones compile there and the
    /// refused ones are `TS2322`/`TS2345`.
    ///
    /// `opt_bad` is the one wrong program here that draws nothing, and it is
    /// asserted as a silence rather than left out. A prelude constructor
    /// (`Ok`, `Err`, `Some`) has no type at all in this checker: the walk
    /// records `Ty::Unknown` for it, pending use-site generic instantiation,
    /// so `Some("nope")` reaches the relation as an undecidable value rather
    /// than as an `Option<"nope">`. `tsc` reports it (`Type 'Option<"nope">'
    /// is not assignable to type 'Option<Mode>'`), and it is the constructor's
    /// missing type that keeps Glyph silent, not the literal's.
    #[test]
    fn a_written_literal_is_decided_against_every_container_it_is_written_in() {
        let errs = errors_of(&format!(
            "{MODE}\
             type Cfg = {{ mode: Mode }}\n\
             fn takes(m: Mode) -> string {{\n  return m\n}}\n\
             fn arr_ok() -> Array<Mode> {{\n  return [\"read\", \"write\"]\n}}\n\
             fn arr_bad() -> Array<Mode> {{\n  return [\"read\", \"nope\"]\n}}\n\
             fn rec_ok() -> Cfg {{\n  return {{ mode: \"read\" }}\n}}\n\
             fn rec_bad() -> Cfg {{\n  return {{ mode: \"nope\" }}\n}}\n\
             fn opt_ok() -> Option<Mode> {{\n  return Some(\"read\")\n}}\n\
             fn opt_bad() -> Option<Mode> {{\n  return Some(\"nope\")\n}}\n\
             fn null_ok() -> Nullable<Mode> {{\n  return \"read\"\n}}\n\
             fn null_bad() -> Nullable<Mode> {{\n  return \"nope\"\n}}\n\
             fn deep_ok() -> Array<Cfg> {{\n  return [{{ mode: \"read\" }}]\n}}\n\
             fn deep_bad() -> Array<Cfg> {{\n  return [{{ mode: \"nope\" }}]\n}}\n\
             fn arg_ok() -> string {{\n  return takes(\"read\")\n}}\n\
             fn arg_bad() -> string {{\n  return takes(\"nope\")\n}}\n"
        ));
        assert_eq!(
            mismatch_found(&errs),
            vec!["\"nope\""; 5],
            "each refusal names the literal that is not in the set: {errs:?}"
        );
    }

    /// The widening rule, stated as a program. A `let` with no annotation
    /// emits a TypeScript `let`, which widens, so the binding is a `string`
    /// and every use of it where a `Mode` is declared is refused: `tsc`
    /// refuses the same three.
    #[test]
    fn a_fresh_literal_widens_at_a_binding_that_infers_its_type() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn takes(m: Mode) -> string {{\n  return m\n}}\n\
             fn one() -> string {{\n  let l = \"read\"\n  return takes(l)\n}}\n\
             fn many() -> Array<Mode> {{\n  let xs = [\"read\", \"write\"]\n  return xs\n}}\n\
             fn each() -> string {{\n\
             \x20 let out = \"\"\n\
             \x20 for m in [\"read\", \"write\"] {{ mut out = out + takes(m) }}\n\
             \x20 return out\n\
             }}\n"
        ));
        assert_eq!(
            mismatch_found(&errs),
            vec!["string", "Array<string>", "string"],
            "errs: {errs:?}"
        );
    }

    /// Freshness is read off the written form, not off the type. A literal
    /// type that arrived through a name is not widened, and TypeScript does
    /// not widen it either, so `takes(m2)` compiles under both.
    #[test]
    fn a_literal_type_reached_through_a_name_does_not_widen() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn takes(m: Mode) -> string {{\n  return m\n}}\n\
             fn named(m: Mode) -> string {{\n  let m2 = m\n  return takes(m2)\n}}\n\
             fn inline(m: \"read\" | \"write\") -> string {{\n  let m2 = m\n  return takes(m2)\n}}\n"
        ));
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
    }

    /// The `Record` key. TypeScript writes `Record<string, V>` as an index
    /// signature, which covers every key a `Record<Mode, V>` declares, so it
    /// accepts the pairing in both directions; `Array<string>` against
    /// `Array<Mode>` is `TS2322` in the same file. Established by running
    /// both, which is why the carve-out is this narrow.
    #[test]
    fn a_record_key_accepts_a_string_where_a_literal_union_is_declared() {
        let errs = errors_of(&format!(
            "{MODE}\
             fn keys(r: Record<string, number>) -> Record<Mode, number> {{\n  return r\n}}\n\
             fn back(r: Record<Mode, number>) -> Record<string, number> {{\n  return r\n}}\n\
             fn arr(xs: Array<string>) -> Array<Mode> {{\n  return xs\n}}\n"
        ));
        assert_eq!(
            mismatch_found(&errs),
            vec!["Array<string>"],
            "only the array is refused: {errs:?}"
        );
    }

    /// The review's `asym`, both halves in one program. Both are decided now
    /// (G237), and each the way `tsc --strict` decides it: the wrong value in
    /// the record field is refused and named, and the right value in the
    /// generic argument is accepted. 0.1.122 was silent on both, which is what
    /// the review caught: it bought the second by giving up the first.
    #[test]
    fn the_record_field_and_the_generic_argument_are_both_decided() {
        let errs = errors_of(&format!(
            "{MODE}\
             type Cfg = {{ mode: Mode }}\n\
             fn wrong() -> Cfg {{\n  return {{ mode: \"nope\" }}\n}}\n\
             fn right() -> Array<Mode> {{\n  return [\"read\"]\n}}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["\"nope\""], "errs: {errs:?}");
        assert_eq!(
            mismatch_accepted(&errs),
            vec![vec!["read".to_string(), "write".to_string()]],
            "the field's own declared type is what the refusal names: {errs:?}"
        );
    }
    // ----- G274: the relation's other nested positions -----

    /// The four positions the record arm did not reach, each a program that
    /// passed `glyph check --no-tsc` and that `tsc --strict` refuses with
    /// TS2345: an optional field where a required one is declared, a declared
    /// record where a structural one is declared, a function whose return
    /// does not fit, and an interface member holding the wrong record. Each
    /// refused row is followed by the correct program beside it, which stays
    /// accepted.
    #[test]
    fn the_relations_other_nested_positions_are_compared_by_field() {
        let rows: [(&str, bool); 8] = [
            (
                "module m\n\
                 fn req(x: { a: number }) -> number { return 1 }\n\
                 fn opt(y: { a?: number }) -> number { return req(y) }\n",
                true,
            ),
            (
                "module m\n\
                 fn req(x: { a?: number }) -> number { return 1 }\n\
                 fn opt(y: { a: number }) -> number { return req(y) }\n",
                false,
            ),
            (
                "module m\n\
                 type Mine = { b: number }\n\
                 fn st(x: { b: string }) -> number { return 1 }\n\
                 fn f(m: Mine) -> number { return st(m) }\n",
                true,
            ),
            (
                "module m\n\
                 type Mine = { b: string, c: number }\n\
                 fn st(x: { b: string }) -> number { return 1 }\n\
                 fn f(m: Mine) -> number { return st(m) }\n",
                false,
            ),
            (
                "module m\n\
                 type Other = { a: number }\n\
                 fn tf(f: fn() -> { o: Other }) -> number { return 1 }\n\
                 fn mk() -> { o: { b: string } } { return { o: { b: \"x\" } } }\n\
                 fn g() -> number { return tf(mk) }\n",
                true,
            ),
            (
                "module m\n\
                 type Other = { a: number }\n\
                 fn tf(f: fn() -> { o: Other }) -> number { return 1 }\n\
                 fn mk() -> { o: { a: number } } { return { o: { a: 1 } } }\n\
                 fn g() -> number { return tf(mk) }\n",
                false,
            ),
            (
                "module m\n\
                 type Other = { a: number }\n\
                 interface Has { o: Other }\n\
                 fn take(h: Has) -> number { return 1 }\n\
                 fn g() -> number {\n\
                 \x20 let v = { o: { b: \"x\" } }\n\
                 \x20 return take(v)\n\
                 }\n",
                true,
            ),
            (
                "module m\n\
                 type Other = { a: number }\n\
                 interface Has { o: Other }\n\
                 fn take(h: Has) -> number { return 1 }\n\
                 fn g() -> number {\n\
                 \x20 let v = { o: { a: 1 } }\n\
                 \x20 return take(v)\n\
                 }\n",
                false,
            ),
        ];
        for (src, refused) in rows {
            let errs = errors_of(src);
            assert_eq!(!mismatch_found(&errs).is_empty(), refused, "{src}\nerrs: {errs:?}");
        }
    }

    /// A record that names itself through a member, against an interface
    /// that does the same, is compared structurally and terminates. It used
    /// to be refused, `Node` against `Tree` by name one level down, on a
    /// program `tsc --strict` accepts.
    #[test]
    fn a_self_referencing_interface_is_compared_structurally_and_terminates() {
        let errs = errors_of(
            "module m\n\
             interface Tree { kids: Array<Tree> }\n\
             type Node = { kids: Array<Node> }\n\
             fn take(t: Tree) -> number { return 1 }\n\
             fn g(n: Node) -> number { return take(n) }\n",
        );
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
        let errs = errors_of(
            "module m\n\
             interface Tree { kids: Array<Tree>, label: string }\n\
             type Node = { kids: Array<Node>, label: number }\n\
             fn take(t: Tree) -> number { return 1 }\n\
             fn g(n: Node) -> number { return take(n) }\n",
        );
        assert_eq!(mismatch_found(&errs).len(), 1, "errs: {errs:?}");
    }

    // ----- The record arm through the whole relation, and the weak-type rule -----

    /// The refusals, as `(line, no_common_field)`, one per mismatch, with the
    /// line 1-based so a row reads against the program it came from.
    fn record_refusals(src: &str) -> Vec<(usize, bool)> {
        errors_of(src)
            .iter()
            .filter_map(|e| match e {
                TypeError::TypeMismatch { no_common_field, span, .. }
                | TypeError::ArgumentTypeMismatch { no_common_field, span, .. } => {
                    Some((src[..span.start as usize].matches('\n').count() + 1, *no_common_field))
                }
                _ => None,
            })
            .collect()
    }

    const RECORD_ROWS: &str = "module m\n\
        type Mode = \"read\" | \"write\"\n\
        type Status = | Active | Done\n\
        type A = { k: string }\n\
        type B = A\n\
        fn mode(x: { mode: Mode }) -> number { return 1 }\n\
        fn text(x: { u: string }) -> number { return 1 }\n\
        fn status(x: { s: Status }) -> number { return 1 }\n\
        fn deep(x: { w: { mode: Mode } }) -> number { return 1 }\n\
        fn named(x: { a: A }) -> number { return 1 }\n";

    /// A field of a structural record value is compared through the whole
    /// relation, as every other position is. The free function it used to go
    /// through had no declaration in hand, so a field holding a declared
    /// union, a string-literal union or a second name was compared by nothing,
    /// and each refusal below passed `glyph check --no-tsc` on a program
    /// `tsc --strict` refuses with TS2345. The value is a `let` binding, not a
    /// literal: a written literal was already compared field by field.
    #[test]
    fn a_record_value_is_compared_field_by_field_through_the_whole_relation() {
        let src = format!(
            "{RECORD_ROWS}fn f(st: Status, b: B) -> number {{\n\
             \x20 let vm = {{ mode: \"read\" }}\n\
             \x20 let vu = {{ u: st }}\n\
             \x20 let vs = {{ s: \"x\" }}\n\
             \x20 let vd = {{ w: {{ mode: \"read\" }} }}\n\
             \x20 let vb = {{ a: b }}\n\
             \x20 return mode(vm) + text(vu) + status(vs) + deep(vd) + named(vb)\n\
             }}\n"
        );
        // `mode(vm)`: the `let` widens `"read"` to `string`, which `Mode`
        // refuses. `text(vu)`: a tagged union is never a `string`.
        // `status(vs)`: a `string` is never a tagged union. `deep(vd)`: the
        // same refusal one record further in. `named(vb)` is accepted: `B` is
        // a second name for `A` (D46), and `tsc` accepts it too.
        let rows = record_refusals(&src);
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert!(rows.iter().all(|(line, weak)| *line == 17 && !weak), "{rows:?}");
    }

    /// TypeScript's weak-type rule (TS2559), mirrored row by row against
    /// `tsc --strict`: a declared record with at least one field, all of them
    /// optional, refuses a value that has a property or is a function and
    /// shares no field name with it. The refusal says so in its own words.
    #[test]
    fn a_value_sharing_no_field_with_an_all_optional_record_is_refused() {
        let src = "module m\n\
            type W = { a?: string }\n\
            type W2 = { a?: string, b?: number }\n\
            type E = { }\n\
            type NotWeak = { a?: string, b: number }\n\
            fn w(x: W) -> number { return 1 }\n\
            fn w2(x: W2) -> number { return 1 }\n\
            fn e(x: E) -> number { return 1 }\n\
            fn nw(x: NotWeak) -> number { return 1 }\n\
            fn inline(x: { a?: string }) -> number { return 1 }\n\
            fn nullable(x: Nullable<W>) -> number { return 1 }\n\
            fn deep(x: { o: W }) -> number { return 1 }\n\
            fn f(g: fn() -> number, opt: { a?: string }) -> number {\n\
            \x20 let vx = { x: 1 }\n\
            \x20 let vax = { a: \"s\", x: 1 }\n\
            \x20 let vn = { a: 1 }\n\
            \x20 let vo = { o: { x: 1 } }\n\
            \x20 let l: W = vx\n\
            \x20 return w(vx) + w2(vx) + inline(vx) + nullable(vx) + w(g) + w({ x: 1 })\n\
            \x20   + e(vx) + w(vax) + w(opt)\n\
            \x20   + nw(vx) + w(vn) + deep(vo)\n\
            }\n";
        let rows = record_refusals(src);
        // Line 18 is the annotated `let`: TS2559.
        // Line 19: `w(vx)`, `w2(vx)`, `inline(vx)`, `nullable(vx)` and the
        // function `w(g)` are TS2559, and the fresh `w({ x: 1 })` is TS2353
        // (an excess property, refused by `tsc` first): six refusals, each the
        // weak rule's.
        // Line 20: an empty record is not weak, `vax` shares `a`, and `opt`
        // shares `a` (optional on both sides): `tsc` accepts all three.
        // Line 21: `NotWeak` requires `b` (TS2741, a missing field), `vn`
        // shares `a` with the wrong type (TS2322), and `deep(vo)` is the weak
        // rule one record in, refused as the outer record (TS2345): refused,
        // and none of them by the weak rule at the top.
        assert_eq!(
            rows,
            vec![
                (18, true),
                (19, true),
                (19, true),
                (19, true),
                (19, true),
                (19, true),
                (19, true),
                (21, false),
                (21, false),
                (21, false),
            ],
            "{rows:?}"
        );
        let message = errors_of(src)[0].to_string();
        assert!(
            message.contains("no field in common with `W`, whose fields are all optional"),
            "{message}"
        );
    }

    // ----- G232: the type of an object literal -----

    /// Every type an expression reports, keyed by the source text it spans.
    /// Enough to state what a literal's type *is*, rather than only what the
    /// relation does with it.
    fn object_ty(src: &str, literal: &str) -> Ty {
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (tm, _ty_errs) = assign_types(&m, &resolved, &prelude);
        let start = src.find(literal).expect("the literal is in the source");
        let span = Span::new(start as u32, (start + literal.len()) as u32);
        tm.get(span).clone()
    }

    /// The finding. `{ x: 1 }` was `Ty::Unknown`, so nothing compared it to
    /// anything and `let g: string = { x: 1 }` passed `glyph check --no-tsc`
    /// on a program `tsc` refuses with TS2322.
    #[test]
    fn an_object_literal_has_the_record_type_it_writes() {
        let ty = object_ty(
            "module m\nfn f() -> void {\n  let g = { x: 1, s: \"a\" }\n}\n",
            "{ x: 1, s: \"a\" }",
        );
        let Ty::Record { fields } = ty else {
            panic!("an object literal is a record: {ty:?}");
        };
        assert_eq!(fields.len(), 2, "{fields:?}");
        assert_eq!(fields[0].name.as_ref(), "x");
        assert_eq!(fields[0].ty, Ty::Prim(Primitive::Number));
        assert!(!fields[0].optional, "a written key is not optional");
        assert_eq!(fields[1].name.as_ref(), "s");
    }

    /// A spread contributes fields this walk cannot enumerate. A record built
    /// without them would be missing a field the value carries, and "missing
    /// required field" would then fire on a literal that has it, so the
    /// literal keeps no type at all.
    #[test]
    fn an_object_literal_with_a_spread_has_no_type() {
        let ty = object_ty(
            "module m\nfn f(base: { x: number }) -> void {\n  let g = { ...base, y: 1 }\n}\n",
            "{ ...base, y: 1 }",
        );
        assert_eq!(ty, Ty::Unknown, "a spread's fields are not enumerable here");
    }

    /// A record where a primitive is declared, in each of the positions the
    /// four value sites cover.
    #[test]
    fn a_record_where_a_primitive_is_declared_is_refused() {
        let errs = errors_of(
            "module m\n\
             fn takes(s: string) -> void { }\n\
             fn r() -> number {\n  return { x: 1 }\n}\n\
             fn f() -> void {\n\
             \x20 let g: string = { x: 1 }\n\
             \x20 takes({ x: 1 })\n\
             }\n",
        );
        assert_eq!(
            mismatch_found(&errs),
            vec!["record", "record", "record"],
            "errs: {errs:?}"
        );
    }

    /// A required field the literal does not write. The refusal names the
    /// declared type by its own name, which is the half a reader repairs the
    /// program from.
    #[test]
    fn a_record_missing_a_required_field_is_refused_and_names_the_type() {
        let errs = errors_of(
            "module m\n\
             type User = { name: string, age: number }\n\
             fn f() -> void {\n  let u: User = { name: \"a\" }\n}\n",
        );
        let found = errs
            .iter()
            .find_map(|e| match e {
                TypeError::TypeMismatch { expected, found, .. } => Some((expected, found)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("errs: {errs:?}"));
        assert_eq!(found.0, "User", "errs: {errs:?}");
        assert_eq!(found.1, "record", "errs: {errs:?}");
    }

    /// An optional field on the declared side may be absent, and an extra
    /// field on the value's side is fine: width subtyping is the rule, and
    /// this change does not touch it. TypeScript refuses the excess property
    /// on a fresh literal (TS2353) and Glyph does not, which predates G232.
    #[test]
    fn an_optional_field_may_be_absent_and_an_extra_field_is_accepted() {
        let errs = errors_of(
            "module m\n\
             type Opt = { name: string, nick?: string }\n\
             fn f() -> void {\n\
             \x20 let a: Opt = { name: \"a\" }\n\
             \x20 let b: Opt = { name: \"b\", nick: \"bb\", extra: 1 }\n\
             }\n",
        );
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
    }

    /// A field whose value the checker cannot type keeps `Unknown` and sinks
    /// nothing around it: the missing-field question does not depend on any
    /// field's type, so it is still answered.
    #[test]
    fn an_undecidable_field_value_does_not_stop_the_missing_field_rule() {
        let errs = errors_of(
            "module m\n\
             type User = { name: string, age: number }\n\
             fn f(raw: unknown) -> void {\n  let u: User = { name: raw }\n}\n",
        );
        assert_eq!(mismatch_found(&errs), vec!["record"], "errs: {errs:?}");
    }

    /// A binding takes the literal's type, so the use site is decided the way
    /// the written literal is, and a fresh literal *field* widens on the way
    /// in exactly as TypeScript widens it. Without the widening the binding
    /// would carry a narrower type than the program that runs has, and the
    /// call would be accepted where `tsc` reports TS2345.
    #[test]
    fn a_binding_carries_the_literal_type_with_fresh_fields_widened() {
        let errs = errors_of(
            "module m\n\
             type Mode = \"read\" | \"write\"\n\
             type User = { name: string, age: number }\n\
             fn takes_mode(m: Mode) -> void { }\n\
             fn takes_user(u: User) -> void { }\n\
             fn f() -> void {\n\
             \x20 let c = { mode: \"read\" }\n\
             \x20 let u = { name: \"a\" }\n\
             \x20 takes_mode(c.mode)\n\
             \x20 takes_user(u)\n\
             }\n",
        );
        assert_eq!(
            mismatch_found(&errs),
            vec!["string", "record"],
            "errs: {errs:?}"
        );
    }

    /// A field written through a name keeps its type: TypeScript does not
    /// widen one that arrived that way either, so widening it here would
    /// refuse a program `tsc` accepts.
    #[test]
    fn a_field_written_through_a_name_is_not_widened() {
        let errs = errors_of(
            "module m\n\
             type Mode = \"read\" | \"write\"\n\
             fn takes_mode(m: Mode) -> void { }\n\
             fn f(m: Mode) -> void {\n\
             \x20 let c = { mode: m }\n\
             \x20 takes_mode(c.mode)\n\
             }\n",
        );
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
    }

    /// `Nullable<T>` admits a `T` or the null a boundary produced (D45), and a
    /// written record is not null, so the pairing is the one against `T`. The
    /// other containers stay undetermined for a record on purpose: a
    /// `Record<K, V>` map *is* written as an object literal.
    #[test]
    fn a_record_against_a_nullable_is_the_pairing_against_its_argument() {
        let errs = errors_of(
            "module m\n\
             type User = { name: string, age: number }\n\
             fn f() -> void {\n\
             \x20 let bad: Nullable<User> = { name: \"a\" }\n\
             \x20 let good: Nullable<User> = { name: \"b\", age: 1 }\n\
             \x20 let map: Record<string, number> = { one: 1 }\n\
             }\n",
        );
        assert_eq!(mismatch_found(&errs), vec!["record"], "errs: {errs:?}");
    }

    /// Two names for one declaration are one type (D46), and the field set
    /// behind the second name is the one the comparison reads.
    #[test]
    fn a_record_is_compared_through_a_second_name_for_the_declaration() {
        let errs = errors_of(
            "module m\n\
             type User = { name: string, age: number }\n\
             type Person = User\n\
             fn f() -> void {\n  let p: Person = { name: \"a\" }\n}\n",
        );
        assert_eq!(mismatch_found(&errs), vec!["record"], "errs: {errs:?}");
    }

    /// The nominal rule (D49) decides two *names* and a written literal has
    /// none, so the comparison against a declared record is structural, which
    /// is what `tsc` does with the same pairing. A complete literal is
    /// accepted whatever the declaration is called.
    #[test]
    fn a_complete_literal_satisfies_a_declared_record_structurally() {
        let errs = errors_of(
            "module m\n\
             type User = { name: string, age: number }\n\
             fn takes(u: User) -> void { }\n\
             fn make() -> User {\n  return { name: \"m\", age: 0 }\n}\n\
             fn f() -> void {\n\
             \x20 let u: User = { name: \"a\", age: 1 }\n\
             \x20 takes({ name: \"b\", age: 2 })\n\
             }\n",
        );
        assert!(mismatch_found(&errs).is_empty(), "errs: {errs:?}");
    }

    // -----------------------------------------------------------------------
    // G240: a component element's attributes, against its props record
    // -----------------------------------------------------------------------

    /// The program the gap was written from. All three spellings passed
    /// `glyph check --no-tsc` with no diagnostic: nothing in the front end
    /// read a JSX attribute, so neither D30's literal set nor any other rule
    /// reached one, and `tsc` on the emitted TypeScript was the only thing
    /// that saw it.
    const BUTTON: &str = "module m\n\
         type Variant = \"primary\" | \"danger\"\n\
         type Props = { variant: Variant, label: string, hint?: string }\n\
         component Button(props: Props) -> Component {\n\
         \x20 return <button>{props.label}</button>\n\
         }\n";

    #[test]
    fn a_jsx_attribute_outside_the_declared_literal_set_is_refused() {
        let errs = errors_of(&format!(
            "{BUTTON}\
             component App() -> Component {{\n\
             \x20 return <Button variant=\"nope\" label=\"go\" />\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["\"nope\""], "errs: {errs:?}");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, .. } if expected == "Variant"
            )),
            "the attribute is named by the field's own declared type: {errs:?}"
        );
        assert_eq!(
            mismatch_accepted(&errs),
            vec![vec!["primary".to_string(), "danger".to_string()]],
            "the D30 literal set rides along as the alternatives: {errs:?}"
        );
    }

    /// The expression form of the same attribute, so the refusal comes from
    /// the type the walk recorded rather than from a written literal.
    #[test]
    fn a_jsx_attribute_expression_of_the_wrong_type_is_refused() {
        let errs = errors_of(&format!(
            "{BUTTON}\
             component App() -> Component {{\n\
             \x20 return <Button variant={{42}} label=\"go\" />\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["number"], "errs: {errs:?}");
    }

    /// An attribute the props record does not declare is the field diagnostic,
    /// which carries the record's own field list, so `glyph fix` can take the
    /// one field a character away from the typo.
    #[test]
    fn a_jsx_attribute_the_props_record_does_not_declare_is_refused() {
        let errs = errors_of(&format!(
            "{BUTTON}\
             component App() -> Component {{\n\
             \x20 return <Button variant=\"danger\" label=\"go\" bogus=\"x\" />\n\
             }}\n"
        ));
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::UnknownField { field, type_name, fields, .. }
                    if field == "bogus"
                        && type_name == "Props"
                        && fields == &["variant", "label", "hint"]
            )),
            "an undeclared attribute names the props record and its fields: {errs:?}"
        );
    }

    /// Every correct shape, in one program, all of which `tsc --strict`
    /// compiles: the right attributes, an optional field written and omitted,
    /// a function-typed prop, children on a component that declares no
    /// `children` field, `key`, and the compiler's own directives.
    #[test]
    fn the_correct_jsx_attribute_shapes_draw_nothing() {
        let errs = errors_of(
            "module m\n\
             type Variant = \"primary\" | \"danger\"\n\
             type Props = { variant: Variant, label: string, hint?: string }\n\
             component Button(props: Props) -> Component {\n\
             \x20 return <button>{props.label}</button>\n\
             }\n\
             component Card(props: { title: string }) -> Component {\n\
             \x20 return <div>{props.title}</div>\n\
             }\n\
             component App(a: { xs: Array<string> }) -> Component {\n\
             \x20 return <div class=\"x\" bogus=\"ok\">\n\
             \x20   <Button variant=\"danger\" label=\"go\" />\n\
             \x20   <Button variant=\"primary\" label=\"b\" hint=\"h\" key={1} />\n\
             \x20   <Card title=\"t\"><span>child</span></Card>\n\
             \x20   <for x in={a.xs}><Card title={x} /></for>\n\
             \x20   <if cond={a.xs.length == 0}><Card title=\"none\" /></if>\n\
             \x20   <else><Card title=\"some\" /></else>\n\
             \x20 </div>\n\
             }\n",
        );
        assert!(errs.is_empty(), "no correct shape is refused: {errs:?}");
    }

    /// A spread may carry any prop, so an element that has one is not asked
    /// for a missing attribute. What it writes explicitly is still checked:
    /// `tsc` refuses the wrong literal and the undeclared name with the
    /// spread present, and so does this.
    #[test]
    fn a_spread_leaves_the_written_attributes_checked() {
        let errs = errors_of(&format!(
            "{BUTTON}\
             component App(p: Props) -> Component {{\n\
             \x20 return <div>\n\
             \x20   <Button {{...p}} />\n\
             \x20   <Button {{...p}} variant=\"nope\" />\n\
             \x20   <Button {{...p}} bogus=\"x\" />\n\
             \x20 </div>\n\
             }}\n"
        ));
        assert_eq!(mismatch_found(&errs), vec!["\"nope\""], "errs: {errs:?}");
        assert!(
            errs.iter()
                .any(|e| matches!(e, TypeError::UnknownField { field, .. } if field == "bogus")),
            "an undeclared attribute is decidable next to a spread: {errs:?}"
        );
        assert_eq!(errs.len(), 2, "the spread alone draws nothing: {errs:?}");
    }

    /// A component declaring no parameter takes no props, and `tsc` accepts
    /// any attribute on one, so nothing here does either.
    ///
    /// A component whose single parameter is not a record is refused at its
    /// declaration (E0229, G243), and its element draws nothing more: the
    /// attribute rule reads the props record, and a second diagnostic at
    /// every element would say the same thing once per use. This test used to
    /// assert that `Greeting(name: string)` drew nothing at all.
    #[test]
    fn a_component_without_a_props_record_is_left_alone() {
        let errs = errors_of(
            "module m\n\
             component Bare() -> Component {\n  return <div>hi</div>\n}\n\
             component Greeting(name: string) -> Component {\n  return <div>{name}</div>\n}\n\
             component App() -> Component {\n\
             \x20 return <div>\n\
             \x20   <Bare nope=\"x\" />\n\
             \x20   <Greeting name=\"x\" />\n\
             \x20 </div>\n\
             }\n",
        );
        assert!(
            matches!(
                errs.as_slice(),
                [TypeError::ComponentParamNotRecord { component, param, .. }]
                    if component == "Greeting" && param == "name"
            ),
            "errs: {errs:?}"
        );
    }

    /// The E0229 the one parameter of `component C(p: <ty>)` draws, if any,
    /// with `decls` declared above it.
    fn props_param_errors(decls: &str, ty: &str) -> Vec<TypeError> {
        errors_of(&format!(
            "module m\n{decls}\ncomponent C(p: {ty}) -> Component {{\n  return <div>hi</div>\n}}\n"
        ))
        .into_iter()
        .filter(|e| e.code() == "E0229")
        .collect()
    }

    /// G243. Every type the checker can decide is not a record is refused at
    /// the parameter, named as the source writes it. Each of these, rendered
    /// with an attribute, is TS2769 under `tsc --strict`, and rendered with
    /// none hands the parameter `{}`.
    #[test]
    fn a_component_parameter_that_is_not_a_record_is_e0229() {
        let tier = "type Tier = \"free\" | \"pro\"\n";
        let state = "type S =\n  | Idle\n  | Busy(number)\n";
        let cases: &[(&str, &str)] = &[
            ("", "string"),
            ("", "number"),
            ("", "int"),
            ("", "bool"),
            (tier, "Tier"),
            ("", "\"free\" | \"pro\""),
            (state, "S"),
            ("", "Option<string>"),
            ("", "Result<string, string>"),
            ("", "Array<string>"),
            ("", "Nullable<{ name: string }>"),
            ("", "fn(n: number) -> string"),
            ("type Name = string\n", "Name"),
            ("type Name = string\ntype Label = Name\n", "Label"),
            ("type Tree<T> =\n  | Leaf\n  | Node(T)\n", "Tree<string>"),
        ];
        for (decls, ty) in cases {
            let errs = props_param_errors(decls, ty);
            assert!(
                matches!(
                    errs.as_slice(),
                    [TypeError::ComponentParamNotRecord { component, param, ty: shown, why: PropsRefusal::NotAnObject, .. }]
                        if component == "C" && param == "p" && shown == ty
                ),
                "`p: {ty}` should be one E0229 naming it as written: {errs:?}"
            );
            assert_eq!(errs[0].actual(), Some(*ty), "{errs:?}");
            assert_eq!(errs[0].expected(), None, "a props record is a rule, not a type");
        }
    }

    /// The other side of G243: a record in any spelling the checker reads as
    /// one, and every type it cannot decide or that describes any object, draw
    /// nothing. A refusal has to be proved (exact-or-absent).
    #[test]
    fn a_component_parameter_that_may_be_a_record_is_left_alone() {
        let props = "type P = { name: string }\n";
        let cases: &[(&str, &str)] = &[
            ("", "{ name: string }"),
            (props, "P"),
            ("type P = { name: string }\ntype Q = P\n", "Q"),
            ("type Box<T> = { value: T }\n", "Box<string>"),
            ("interface Named {\n  name: string,\n}\n", "Named"),
            ("interface Empty {\n}\n", "Empty"),
            ("", "Record<string, string>"),
            ("type Same<T> = T\n", "Same<string>"),
            ("type Raw = extern_ts(\"{ name: string }\")\n", "Raw"),
        ];
        for (decls, ty) in cases {
            let errs = props_param_errors(decls, ty);
            assert!(errs.is_empty(), "`p: {ty}` should draw no E0229: {errs:?}");
        }
    }

    /// A component with no parameter has nothing to refuse.
    /// Two parameters stay E0214 alone: the second is the mistake that
    /// diagnostic names, and the first is not judged as a props record.
    #[test]
    fn e0229_reads_only_a_single_decidable_parameter() {
        let bare = errors_of("module m\ncomponent Bare() -> Component {\n  return <div>hi</div>\n}\n");
        assert!(bare.is_empty(), "{bare:?}");
        let two = errors_of(
            "module m\ncomponent Row(label: string, count: number) -> Component {\n  return <p>{label}</p>\n}\n",
        );
        assert!(
            matches!(two.as_slice(), [TypeError::ComponentMultipleParams { count: 2, .. }]),
            "{two:?}"
        );
    }

    /// D19: a component written without a return type returns the prelude
    /// `Component`, so its body is held to what `-> Component` holds it to.
    /// A `?` in it is E0201 under both spellings: it would hand React the
    /// propagated `Err` object to render. A plain `fn` with no annotation
    /// keeps its permissive return context.
    #[test]
    fn an_unannotated_component_returns_the_prelude_component() {
        let body = "{\n  let v = get()?\n  return <p>{v}</p>\n}\n";
        let head = "module m\nfn get() -> Result<string, string> {\n  return Ok(\"x\")\n}\n";
        for decl in ["component A()", "component A() -> Component"] {
            let errs = errors_of(&format!("{head}{decl} {body}"));
            assert!(
                matches!(errs.as_slice(), [TypeError::QuestionOutsideResultFn { .. }]),
                "`{decl}`: {errs:?}"
            );
        }
        let plain = errors_of(&format!("{head}fn f() {body}"));
        assert!(plain.is_empty(), "{plain:?}");
    }

    /// G246. `unknown` holds the props object and gives the component nothing
    /// it can read, so it is E0229 under any second name. `tsc --strict` with
    /// React 18's types refuses `<Greeting name="x" />` against it (TS2769)
    /// and accepts `<Greeting />`; the declaration is refused either way.
    #[test]
    fn an_unknown_component_parameter_is_e0229() {
        for (decls, ty) in [("", "unknown"), ("type Loose = unknown\n", "Loose")] {
            let errs = props_param_errors(decls, ty);
            assert!(
                matches!(
                    errs.as_slice(),
                    [TypeError::ComponentParamNotRecord { ty: shown, why: PropsRefusal::Unknown, .. }]
                        if shown == ty
                ),
                "`p: {ty}` should be one E0229 for `unknown`: {errs:?}"
            );
            let help = errs[0].help().unwrap_or_default();
            assert!(help.contains("nothing it can read"), "{help}");
        }
    }

    /// G246. A component's own type parameter with no bound cannot be filled
    /// by an element (`<Show name="x" />` is TS2769) and may be filled with a
    /// primitive by a caller, so it is E0229. A bounded one is left alone:
    /// `tsc --strict` with React 18's types accepts `<Show label="x" />`
    /// against `<T: HasLabel>` and against `<T: L>` for a record `L`, and the
    /// checker does not read a bound, so it cannot refuse any other.
    #[test]
    fn an_unbounded_component_type_parameter_is_e0229() {
        let iface = "interface HasLabel {\n  label: string,\n}\ntype L = { label: string }\n";
        let generic = |generics: &str| {
            errors_of(&format!(
                "module m\n{iface}component Show<{generics}>(p: T) -> Component {{\n  return <div>hi</div>\n}}\n"
            ))
            .into_iter()
            .filter(|e| e.code() == "E0229")
            .collect::<Vec<_>>()
        };
        let errs = generic("T");
        assert!(
            matches!(
                errs.as_slice(),
                [TypeError::ComponentParamNotRecord { ty, why: PropsRefusal::TypeParam, .. }]
                    if ty == "T"
            ),
            "`<T>` should be one E0229 for a type parameter: {errs:?}"
        );
        let help = errs[0].help().unwrap_or_default();
        assert!(help.contains("record or interface bound"), "{help}");
        for allowed in ["T: HasLabel", "T: L", "T: Record<string, string>"] {
            let errs = generic(allowed);
            assert!(errs.is_empty(), "`<{allowed}>` should draw no E0229: {errs:?}");
        }
        // A type parameter of a generic `type` read through its body is the
        // argument's to decide, not the component's.
        let same = props_param_errors("type Same<X> = X\n", "Same<{ name: string }>");
        assert!(same.is_empty(), "{same:?}");
    }

    // ----- D48: destructuring `let` -----

    fn destructure_errors(body: &str) -> Vec<TypeError> {
        ty_errors_of(&format!(
            "module x\nimport react {{ useState }}\nfn take_num(n: number) -> number {{ return n }}\nfn nums() -> Array<number> {{ return [1] }}\ntype R = {{ a: number }}\nfn rec() -> R {{ return {{ a: 1 }} }}\npub fn run() -> number {{\n{body}\n  return 0\n}}\n"
        ))
    }

    fn e0230(errs: &[TypeError]) -> Vec<&DestructureRefusal> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::DestructureUnproven { why, .. } => Some(why),
                _ => None,
            })
            .collect()
    }

    /// Each name takes its own element's type: `b` is a `string` here, so
    /// passing it where a number is wanted is E0211, and `a` passes.
    #[test]
    fn a_let_pattern_over_a_literal_types_each_element() {
        let errs = destructure_errors("  let [a, b] = [1, \"x\"]\n  take_num(a)\n  take_num(b)");
        let mismatches: Vec<&TypeError> =
            errs.iter().filter(|e| e.code() == "E0211").collect();
        assert_eq!(mismatches.len(), 1, "{errs:?}");
        assert!(e0230(&errs).is_empty(), "{errs:?}");
        let TypeError::ArgumentTypeMismatch { found, .. } = mismatches[0] else {
            panic!("{mismatches:?}");
        };
        assert_eq!(found, "string");
    }

    /// The element types widen the way a `let` widens (D30): `["read"]`
    /// gives a `string`, not the literal `"read"`.
    #[test]
    fn a_let_pattern_widens_a_fresh_literal_element() {
        let errs = ty_errors_of(
            "module x\ntype Mode = \"read\" | \"write\"\nfn take(m: Mode) -> Mode { return m }\npub fn run() -> Mode {\n  let [m] = [\"read\"]\n  return take(m)\n}\n",
        );
        assert!(errs.iter().any(|e| e.code() == "E0211"), "{errs:?}");
    }

    /// A nested pattern is typed against its element, and a rest is an array
    /// of the elements left over.
    #[test]
    fn nested_patterns_and_a_rest_are_typed() {
        let errs = destructure_errors(
            "  let [[a, s], ...more] = [[1, \"x\"], 2, 3]\n  take_num(a)\n  take_num(s)\n  take_num(more)",
        );
        let found: Vec<String> = errs
            .iter()
            .filter_map(|e| match e {
                TypeError::ArgumentTypeMismatch { found, .. } => Some(found.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(found, ["string", "Array<number>"], "{errs:?}");
    }

    #[test]
    fn a_literal_of_another_length_is_e0230() {
        let errs = destructure_errors("  let [a, b] = [1, 2, 3]\n  take_num(a + b)");
        assert_eq!(
            e0230(&errs),
            [&DestructureRefusal::Count { pattern: 2, at_least: false, literal: 3 }],
            "{errs:?}"
        );
        let short = destructure_errors("  let [a, b, ...r] = [1]\n  take_num(a + b)\n  take_num(r)");
        assert_eq!(
            e0230(&short),
            [&DestructureRefusal::Count { pattern: 2, at_least: true, literal: 1 }],
            "{short:?}"
        );
        // A rest takes any number of further elements, none included.
        let exact = destructure_errors("  let [a, ...r] = [1]\n  take_num(a)\n  take_num(r)");
        assert!(e0230(&exact).is_empty(), "{exact:?}");
    }

    /// A nested literal is counted too.
    #[test]
    fn a_nested_literal_of_another_length_is_e0230() {
        let errs = destructure_errors("  let [[a, b], c] = [[1], 2]\n  take_num(a + b + c)");
        assert_eq!(
            e0230(&errs),
            [&DestructureRefusal::Count { pattern: 2, at_least: false, literal: 1 }],
            "{errs:?}"
        );
    }

    #[test]
    fn an_array_of_unknown_length_is_e0230_naming_the_match() {
        let errs = destructure_errors("  let [a, b] = nums()\n  take_num(a + b)");
        assert_eq!(
            e0230(&errs),
            [&DestructureRefusal::UnknownLength { ty: "Array<number>".to_string(), spread: false }],
            "{errs:?}"
        );
        let e = errs.iter().find(|e| e.code() == "E0230").unwrap();
        assert!(e.help().unwrap().contains("match xs { [a, b] => ..., else => ... }"));
        // A literal with a spread has no written length either.
        let spread = destructure_errors("  let [a, b] = [1, ...nums()]\n  take_num(a + b)");
        assert!(
            matches!(e0230(&spread)[..], [DestructureRefusal::UnknownLength { spread: true, .. }]),
            "{spread:?}"
        );
    }

    #[test]
    fn a_value_that_is_not_an_array_is_e0230() {
        for (init, ty) in [("rec()", "R"), ("\"ab\"", "string"), ("1", "number")] {
            let errs = destructure_errors(&format!("  let [a] = {init}\n  take_num(a)"));
            assert_eq!(
                e0230(&errs),
                [&DestructureRefusal::NotAnArray { ty: ty.to_string() }],
                "{init}: {errs:?}"
            );
        }
        let unknown = ty_errors_of(
            "module x\npub fn run(v: unknown) -> number {\n  let [a] = v\n  return 0\n}\n",
        );
        assert!(
            matches!(e0230(&unknown)[..], [DestructureRefusal::NotAnArray { .. }]),
            "{unknown:?}"
        );
    }

    /// A value the checker cannot see into is left to `tsc`, and its names
    /// stay open rather than pinned to anything.
    #[test]
    fn an_npm_value_is_accepted_for_tsc_to_prove() {
        let errs = destructure_errors(
            "  let [count, set_count] = useState(0)\n  let [_, [x, y], ...rest] = useState(1)\n  take_num(count)\n  set_count(1)\n  take_num(x + y)\n  take_num(rest)",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// A `match` initializer is judged by the type the arms join to.
    #[test]
    fn a_match_initializer_is_judged_by_its_type() {
        let errs = destructure_errors(
            "  let [a, b] = match true {\n    true => nums(),\n    false => nums(),\n  }\n  take_num(a + b)",
        );
        assert!(
            matches!(e0230(&errs)[..], [DestructureRefusal::UnknownLength { .. }]),
            "{errs:?}"
        );
    }

    /// The `(expected, found)` pair of every E0204 in `errs`, in order.
    fn g289_mismatches(errs: &[TypeError]) -> Vec<(String, String)> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::TypeMismatch { expected, found, .. } => {
                    Some((expected.clone(), found.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// G289. `json.parse` returns a `Result`, and a `let` annotated with a
    /// record is refused at the `let`, naming both types the way `tsc` names
    /// them in its TS2322. Before this the call had no signature, so the
    /// pairing was decided by nothing and `glyph check --no-tsc` passed it.
    #[test]
    fn a_let_annotation_that_disagrees_with_json_parse_is_refused() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             type User = { id: string, }\n\
             pub fn f(body: string) -> string {\n\
             \x20 let user: User = json.parse(body)\n\
             \x20 return user.id\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("User".to_string(), "Result<unknown, Array<Issue>>".to_string())],
            "{errs:?}"
        );
    }

    /// The named-import spelling reaches the same signature, and the written
    /// type argument is the `Ok` type the diagnostic names.
    #[test]
    fn json_parse_is_typed_under_both_import_spellings_and_with_a_type_argument() {
        let errs = errors_of(
            "module m\n\
             import std/json { parse }\n\
             type User = { id: string, }\n\
             pub fn f(body: string) -> string {\n\
             \x20 let user: User = parse<User>(body)\n\
             \x20 return user.id\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("User".to_string(), "Result<User, Array<Issue>>".to_string())],
            "{errs:?}"
        );
    }

    /// What Glyph's checker does not refuse yet: the bare call where a
    /// `Result<User, …>` is declared (line `a`), a `match` over the call, and
    /// an empty record, which any object satisfies. Line `a` is not accepted
    /// by the build: the emitter writes `json.parse<unknown>(body)`, so `tsc`
    /// refuses it (TS2322) where it used to infer `T = User` from the
    /// annotation and cast (record 89). Glyph refuses it itself once `unknown`
    /// is assignable only to `unknown` (0.1.144), and this line flips to
    /// E0204 then. With a type argument the `Ok` binding is the written type,
    /// so a misspelled field on it is a Glyph error.
    #[test]
    fn json_parse_against_a_result_or_an_empty_record_stays_silent() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             import std/result { Ok, Err, Result }\n\
             type User = { id: string, }\n\
             type Empty = {}\n\
             pub fn f(body: string) -> string {\n\
             \x20 let a: Result<User, Array<Issue>> = json.parse(body)\n\
             \x20 let b: Result<User, Array<Issue>> = json.parse<User>(body)\n\
             \x20 let c: Empty = json.parse(body)\n\
             \x20 return match json.parse<User>(body) {\n\
             \x20\x20\x20 Ok(u) => u.idd,\n\
             \x20\x20\x20 Err(_e) => \"\",\n\
             \x20 }\n\
             }\n",
        );
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::UnknownField { field, .. } if field == "idd"
            )),
            "the `Ok` binding of `json.parse<User>` is a `User`: {errs:?}"
        );
    }

    /// G298. `json.parse_with(text, schema)` is `json.parse`'s sibling and had
    /// no row, so its result was `Unknown` and a record annotation over it
    /// passed `glyph check --no-tsc` while `tsc` refused it. `T` rides on the
    /// `Schema<T>` argument, and a written type argument names it.
    #[test]
    fn a_let_annotation_that_disagrees_with_json_parse_with_is_refused() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             type User = { id: string, }\n\
             pub fn f(body: string, s: Schema<User>) -> string {\n\
             \x20 let user: User = json.parse_with(body, s)\n\
             \x20 let other: User = json.parse_with<User>(body, User.schema)\n\
             \x20 return user.id\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![
                ("User".to_string(), "Result<User, Array<Issue>>".to_string()),
                ("User".to_string(), "Result<User, Array<Issue>>".to_string()),
            ],
            "{errs:?}"
        );
    }

    /// With no type argument and a schema whose type the checker does not
    /// hold (`User.schema` is not typed here), `T` is undecided rather than
    /// `unknown`: a `match` over the result reads `Ok(u).id` without a
    /// refusal, and the container is still a container.
    #[test]
    fn json_parse_with_an_untyped_schema_leaves_the_ok_type_undecided() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             import std/result { Ok, Err }\n\
             type User = { id: string, }\n\
             pub fn f(body: string) -> string {\n\
             \x20 let r: Result<User, Array<Issue>> = json.parse_with(body, User.schema)\n\
             \x20 let bad: User = json.parse_with(body, User.schema)\n\
             \x20 return match json.parse_with(body, User.schema) {\n\
             \x20\x20\x20 Ok(u) => u.id,\n\
             \x20\x20\x20 Err(_e) => \"\",\n\
             \x20 }\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("User".to_string(), "Result<?, Array<Issue>>".to_string())],
            "{errs:?}"
        );
        assert!(
            !errs.iter().any(|e| matches!(e, TypeError::UnknownField { .. })),
            "{errs:?}"
        );
    }

    /// With a typed schema the `Ok` binding is the schema's type, so a
    /// misspelled field on it is a Glyph error; the two-argument arity is held.
    #[test]
    fn json_parse_with_types_its_ok_binding_and_its_arity() {
        let errs = errors_of(
            "module m\n\
             import std/json { parse_with }\n\
             import std/result { Ok, Err }\n\
             type User = { id: string, }\n\
             pub fn f(body: string, s: Schema<User>) -> string {\n\
             \x20 let _x = parse_with(body)\n\
             \x20 return match parse_with(body, s) {\n\
             \x20\x20\x20 Ok(u) => u.idd,\n\
             \x20\x20\x20 Err(_e) => \"\",\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::UnknownField { field, .. } if field == "idd")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| matches!(e, TypeError::ArgumentCountMismatch { expected: 2, found: 1, .. })),
            "{errs:?}"
        );
    }

    /// The relation half of G289, which is not about `json.parse`: any prelude
    /// container is a different declaration from a record or tagged union the
    /// program declares, or a stdlib type the checker models, so the nominal
    /// rule (D49) refuses it at a `let`, a `return` and a call argument. A
    /// user function returning `Result<User, string>` was as silent as the
    /// stdlib call.
    #[test]
    fn a_prelude_container_where_a_declared_type_is_expected_is_refused() {
        let errs = errors_of(
            "module m\n\
             import std/result { Err, Result }\n\
             import std/option { Some, Option }\n\
             import std/url\n\
             type User = { id: string, }\n\
             type Shape = | Circle(number) | Square(number)\n\
             fn load() -> Result<User, string> { return Err(\"x\") }\n\
             fn take(u: User) -> string { return u.id }\n\
             fn back(r: Result<User, string>) -> User { return r }\n\
             pub fn f(s: string, xs: Array<User>) -> string {\n\
             \x20 let u: User = load()\n\
             \x20 let o: Option<number> = Some(1)\n\
             \x20 let sh: Shape = o\n\
             \x20 let one: User = xs\n\
             \x20 let link: url.Url = url.parse(s)\n\
             \x20 return take(load())\n\
             }\n",
        );
        let got = g289_mismatches(&errs);
        for want in [
            ("User", "Result<User, string>"),
            ("Shape", "Option<number>"),
            ("User", "Array<User>"),
            ("url.Url", "Result<url.Url, string>"),
        ] {
            assert!(
                got.iter().filter(|(e, f)| e == want.0 && f == want.1).count() >= 1,
                "missing E0204 expected `{}` found `{}`: {errs:?}",
                want.0,
                want.1
            );
        }
        // `back`'s `return r` and the `let u` are both `Result<User, string>`
        // where `User` is declared.
        assert_eq!(
            got.iter()
                .filter(|(e, f)| e == "User" && f == "Result<User, string>")
                .count(),
            2,
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                TypeError::ArgumentTypeMismatch { expected, found, .. }
                    if expected == "User" && found == "Result<User, string>"
            )),
            "the call argument is E0211: {errs:?}"
        );
    }

    /// An alias whose body is a container is its own type under D46 but
    /// decides nothing, so the container it names still fits it.
    #[test]
    fn a_container_against_an_alias_of_a_container_stays_silent() {
        let errs = errors_of(
            "module m\n\
             import std/result { Ok, Result }\n\
             type User = { id: string, }\n\
             type Loaded = Result<User, string>\n\
             type Headers = Record<string, string>\n\
             fn load() -> Result<User, string> { return Ok({ id: \"a\" }) }\n\
             pub fn f(h: Record<string, string>) -> string {\n\
             \x20 let l: Loaded = load()\n\
             \x20 let k: Headers = h\n\
             \x20 return \"\"\n\
             }\n",
        );
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// D49. A prelude container is another declaration, so a record whose
    /// fields the container happens to carry still refuses it. `tsc` accepts
    /// the `Sized` line, since an array has a `length`; `Sized.parse` refuses
    /// an array, and the checker answers as the validator does.
    #[test]
    fn a_container_against_a_record_it_could_satisfy_by_shape_is_refused() {
        let errs = errors_of(
            "module m\n\
             import std/option { Option }\n\
             type Sized = { length: number, }\n\
             type Flags = { verbose?: bool, }\n\
             type Tagged = { tag: string, }\n\
             pub fn f(xs: Array<string>, m: Record<string, bool>, o: Option<number>) -> number {\n\
             \x20 let s: Sized = xs\n\
             \x20 let g: Flags = m\n\
             \x20 let t: Tagged = o\n\
             \x20 return s.length\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![
                ("Sized".to_string(), "Array<string>".to_string()),
                ("Flags".to_string(), "Record<string, bool>".to_string()),
                ("Tagged".to_string(), "Option<number>".to_string()),
            ],
            "{errs:?}"
        );
    }

    /// D34 against D49: an interface has no descriptor and no identity, so
    /// the array a `Sized` record refuses is left to member shape when
    /// `Sized` is an interface.
    #[test]
    fn a_container_against_an_interface_of_its_members_stays_silent() {
        let errs = errors_of(
            "module m\n\
             interface Sized {\n\
             \x20 length: number\n\
             }\n\
             pub fn f(xs: Array<string>) -> number {\n\
             \x20 let s: Sized = xs\n\
             \x20 return s.length\n\
             }\n",
        );
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// The `(expected, found)` pair of every E0211 in `errs`, in order.
    fn argument_mismatches(errs: &[TypeError]) -> Vec<(String, String)> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::ArgumentTypeMismatch { expected, found, .. } => {
                    Some((expected.clone(), found.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// G292. A written type argument is the binding of the parameter it is
    /// written for, so the argument is checked against it and the call
    /// returns it. Before this the checker inferred `T` from `"x"` and never
    /// read the `<number>`, and `glyph check --no-tsc` passed a program `tsc`
    /// refuses twice (TS2345, TS2322).
    #[test]
    fn an_explicit_type_argument_binds_the_parameter_it_is_written_for() {
        let errs = errors_of(
            "module m\n\
             fn id<T>(x: T) -> T { return x }\n\
             pub fn f() -> string {\n\
             \x20 return id<number>(\"x\")\n\
             }\n",
        );
        assert_eq!(
            argument_mismatches(&errs),
            vec![("number".to_string(), "string".to_string())],
            "{errs:?}"
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("string".to_string(), "number".to_string())],
            "{errs:?}"
        );
    }

    /// The return type is the written argument even where no value argument
    /// could have bound it, which inference alone left open and silent.
    #[test]
    fn an_explicit_type_argument_types_a_result_no_argument_binds() {
        let errs = errors_of(
            "module m\n\
             fn empty<T>() -> Array<T> { return [] }\n\
             fn pair<A, B>(a: A, b: B) -> B { return b }\n\
             pub fn f() -> number {\n\
             \x20 let xs: Array<string> = empty<number>()\n\
             \x20 let b: string = pair<string, number>(\"a\", 1)\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![
                ("Array<string>".to_string(), "Array<number>".to_string()),
                ("string".to_string(), "number".to_string()),
            ],
            "{errs:?}"
        );
        assert!(argument_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// What `tsc` accepts stays accepted: an argument the written type
    /// admits, a wider written type than the argument's, and a written
    /// string-literal union the argument belongs to.
    #[test]
    fn a_correct_explicit_type_argument_stays_silent() {
        let errs = errors_of(
            "module m\n\
             type Mode = \"read\" | \"write\"\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn empty<T>() -> Array<T> { return [] }\n\
             pub fn f() -> string {\n\
             \x20 let n: number = id<number>(3)\n\
             \x20 let m: Mode = id<Mode>(\"read\")\n\
             \x20 let xs: Array<string> = empty<string>()\n\
             \x20 return id<string>(\"x\")\n\
             }\n",
        );
        assert!(argument_mismatches(&errs).is_empty(), "{errs:?}");
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// A count of written type arguments that is not the declaration's
    /// binds nothing positionally: `tsc` refuses the call (TS2558), and
    /// pairing the first argument with the first parameter would report a
    /// mismatch about a binding the program never made.
    #[test]
    fn a_wrong_number_of_explicit_type_arguments_binds_nothing() {
        let errs = errors_of(
            "module m\n\
             fn id<T>(x: T) -> T { return x }\n\
             pub fn f() -> string {\n\
             \x20 return id<number, bool>(\"x\")\n\
             }\n",
        );
        assert!(argument_mismatches(&errs).is_empty(), "{errs:?}");
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// The `(name, expected, found)` of every E0233 in `errs`, in order.
    fn type_arg_counts(errs: &[TypeError]) -> Vec<(String, usize, usize)> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::TypeArgumentCountMismatch {
                    name,
                    expected,
                    found,
                    ..
                } => Some((name.clone(), *expected, *found)),
                _ => None,
            })
            .collect()
    }

    /// E0233. A call writing more or fewer type arguments than the
    /// declaration has parameters is refused, where it used to bind nothing
    /// and pass `--no-tsc` (TS2558 under `tsc`). A non-generic `fn` takes 0,
    /// so any written type argument on it is the same mistake.
    #[test]
    fn a_wrong_number_of_type_arguments_on_a_call_is_e0233() {
        let errs = errors_of(
            "module m\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn pair<A, B>(a: A, b: B) -> B { return b }\n\
             fn one(x: number) -> number { return x }\n\
             pub fn f() -> number {\n\
             \x20 let a = id<number, bool>(1)\n\
             \x20 let b = pair<number>(1, 2)\n\
             \x20 let c = one<number>(1)\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            type_arg_counts(&errs),
            vec![
                ("id".to_string(), 1, 2),
                ("pair".to_string(), 2, 1),
                ("one".to_string(), 0, 1),
            ],
            "{errs:?}"
        );
        let e = errs
            .iter()
            .find(|e| matches!(e, TypeError::TypeArgumentCountMismatch { .. }))
            .expect("an E0233");
        assert_eq!(e.code(), "E0233");
        assert_eq!(
            e.to_string(),
            "wrong number of type arguments: `id` takes 1, found 2"
        );
    }

    /// A modeled stdlib function whose type parameters a written argument
    /// fills has a parameter list the checker reads, so a wrong count on it
    /// is E0233 too. A stdlib function the checker models without one stays
    /// unchecked here, because its count is not known.
    #[test]
    fn a_wrong_number_of_type_arguments_on_json_parse_is_e0233() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             import std/json { parse }\n\
             pub fn f() -> number {\n\
             \x20 let a = json.parse<number, string>(\"1\")\n\
             \x20 let b = parse<number, string>(\"1\")\n\
             \x20 let c = json.parse<number>(\"1\")\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            type_arg_counts(&errs),
            vec![("json.parse".to_string(), 1, 2), ("parse".to_string(), 1, 2)],
            "{errs:?}"
        );
    }

    /// The right count, and no type arguments at all on a call, stay silent.
    #[test]
    fn the_right_number_of_type_arguments_is_not_e0233() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn pair<A, B>(a: A, b: B) -> B { return b }\n\
             fn one(x: number) -> number { return x }\n\
             pub fn f() -> number {\n\
             \x20 let a = id<number>(1)\n\
             \x20 let b = pair<number, number>(1, 2)\n\
             \x20 let c = one(1)\n\
             \x20 let d = id(1)\n\
             \x20 let e = json.parse<number>(\"1\")\n\
             \x20 return 0\n\
             }\n",
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
    }

    /// A function bound to a value keeps its type parameters, so type
    /// arguments written on a call through the value are read the way they
    /// are on the declaration: the right count binds (E0211, E0204), a wrong
    /// one is E0233, and any on a non-generic function is E0233 too. If this
    /// regresses, `let f = id` followed by `f<number>("x")` passes
    /// `glyph check --no-tsc` with the written type silently dropped, and
    /// only `tsc` refuses it (TS2345, TS2558): how a function was reached
    /// would decide whether the guarantee holds.
    #[test]
    fn type_arguments_on_a_function_value_are_read_like_the_declarations() {
        let errs = errors_of(
            "module m\n\
             fn id<T>(x: T) -> T { return x }\n\
             pub fn f() -> string {\n\
             \x20 let f = id\n\
             \x20 let s: string = f<number>(\"x\")\n\
             \x20 return s\n\
             }\n",
        );
        assert_eq!(
            argument_mismatches(&errs),
            vec![("number".to_string(), "string".to_string())],
            "{errs:?}"
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("string".to_string(), "number".to_string())],
            "{errs:?}"
        );

        let errs = errors_of(
            "module m\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn inc(x: number) -> number { return x + 1 }\n\
             pub fn g() -> number {\n\
             \x20 let f = inc\n\
             \x20 let h = id\n\
             \x20 let n: number = f<string>(1)\n\
             \x20 let m: number = h<number, bool>(1)\n\
             \x20 return n + m\n\
             }\n",
        );
        assert_eq!(
            type_arg_counts(&errs),
            vec![("f".to_string(), 0, 1), ("h".to_string(), 1, 2)],
            "{errs:?}"
        );
        // A wrong count binds nothing, so no mismatch is invented from it.
        assert!(argument_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// The right count through a function value, and none at all, stay
    /// silent.
    #[test]
    fn the_right_type_arguments_on_a_function_value_are_not_refused() {
        let errs = errors_of(
            "module m\n\
             fn id<T>(x: T) -> T { return x }\n\
             fn inc(x: number) -> number { return x + 1 }\n\
             pub fn g() -> number {\n\
             \x20 let f = id\n\
             \x20 let k = inc\n\
             \x20 let a: number = f<number>(1)\n\
             \x20 let b: number = f(1)\n\
             \x20 let c: number = k(1)\n\
             \x20 return a + b + c\n\
             }\n",
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
        assert!(argument_mismatches(&errs).is_empty(), "{errs:?}");
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// A lambda has no `<...>` of its own, so a value holding one takes no
    /// type arguments: `f<number>(1)` is E0233, as TS2558 refuses it. If a
    /// lambda's list went from empty to unknown, the call would bind nothing
    /// and pass `--no-tsc` silently.
    #[test]
    fn type_arguments_on_a_lambda_value_are_e0233() {
        let errs = errors_of(
            "module m\n\
             pub fn g() -> number {\n\
             \x20 let f = fn(x: number) -> number { return x }\n\
             \x20 return f<number>(1)\n\
             }\n",
        );
        assert_eq!(type_arg_counts(&errs), vec![("f".to_string(), 0, 1)], "{errs:?}");
    }

    /// A function held in a record field, or declared as an interface method,
    /// has a written function type, and a written function type takes no type
    /// arguments either. Both are E0233.
    #[test]
    fn type_arguments_on_a_record_field_or_interface_method_are_e0233() {
        let errs = errors_of(
            "module m\n\
             type Ops = { run: fn(number) -> number, }\n\
             interface Svc {\n\
             \x20 fn step(x: number) -> number\n\
             }\n\
             pub fn g(o: Ops, s: Svc) -> number {\n\
             \x20 let a = o.run<number>(1)\n\
             \x20 let b = s.step<number>(1)\n\
             \x20 return a + b\n\
             }\n",
        );
        assert_eq!(
            type_arg_counts(&errs),
            vec![("o.run".to_string(), 0, 1), ("s.step".to_string(), 0, 1)],
            "{errs:?}"
        );
    }

    /// A built-in method's type parameters are unknown, not absent: the
    /// generated row does not carry `map`'s `U`, so `xs.map<string>(f)`,
    /// which `tsc` accepts, is accepted. An empty list here would make it a
    /// false E0233.
    #[test]
    fn type_arguments_on_a_builtin_method_are_not_counted() {
        let errs = errors_of(
            "module m\n\
             pub fn g(xs: Array<number>) -> Array<string> {\n\
             \x20 return xs.map<string>(fn(x: number) -> string { return \"a\" })\n\
             }\n",
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
    }

    /// The help on every E0233 in `errs`, in order.
    fn type_arg_count_helps(errs: &[TypeError]) -> Vec<String> {
        errs.iter()
            .filter(|e| matches!(e, TypeError::TypeArgumentCountMismatch { .. }))
            .map(|e| e.help().expect("E0233 has help").into_owned())
            .collect()
    }

    /// Record 88, 4a. `json.parse`'s `T` is filled only by a type argument
    /// written on a direct call, which the emitter turns into a validating
    /// call. A `json.parse` value is the function itself, so a type argument
    /// on a call through it validates nothing: the emitted call is the
    /// casting `parse<T>`. The value therefore has no type parameters, and a
    /// type argument on it is E0233 under either import spelling, a type
    /// parameter of the enclosing function included. If this regresses,
    /// `let p = json.parse` then `p<User>(s)` passes every Glyph stage and
    /// `tsc` with an unvalidated `User`: how the function was reached would
    /// decide whether the input is checked.
    #[test]
    fn type_arguments_on_a_json_parse_value_are_e0233() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             import std/json { parse }\n\
             type User = { name: string, }\n\
             pub fn f(s: string) -> number {\n\
             \x20 let p = json.parse\n\
             \x20 let q = parse\n\
             \x20 let a = p<User>(s)\n\
             \x20 let b = q<User>(s)\n\
             \x20 return 0\n\
             }\n\
             pub fn read<T>(b: string) -> Result<T, Array<Issue>> {\n\
             \x20 let p = json.parse\n\
             \x20 return p<T>(b)\n\
             }\n",
        );
        assert_eq!(
            type_arg_counts(&errs),
            vec![
                ("p".to_string(), 0, 1),
                ("q".to_string(), 0, 1),
                ("p".to_string(), 0, 1),
            ],
            "{errs:?}"
        );
        assert_eq!(
            type_arg_count_helps(&errs),
            vec![
                "Call `json.parse<User>(...)` directly; a `json.parse` value returns `unknown`, because only a type argument written on the call itself makes the call validate its input.".to_string(),
                "Call `parse<User>(...)` directly; a `parse` value returns `unknown`, because only a type argument written on the call itself makes the call validate its input.".to_string(),
                "Call `json.parse<T>(...)` directly; a `json.parse` value returns `unknown`, because only a type argument written on the call itself makes the call validate its input.".to_string(),
            ],
            "{errs:?}"
        );
    }

    /// A `json.parse` value returns `Result<unknown, ...>`, the top type and
    /// not the undecided one (`Ty::Unknown`, displayed `?`), under either
    /// import spelling. A refusal that names the value's type shows which.
    #[test]
    fn a_json_parse_value_returns_unknown_not_an_undecided_type() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             import std/json { parse }\n\
             pub fn f(s: string) -> number {\n\
             \x20 let p = json.parse\n\
             \x20 let q = parse\n\
             \x20 let a: number = p(s)\n\
             \x20 let b: number = q(s)\n\
             \x20 let c: Result<unknown, Array<Issue>> = p(s)\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![
                ("number".to_string(), "Result<unknown, Array<Issue>>".to_string()),
                ("number".to_string(), "Result<unknown, Array<Issue>>".to_string()),
            ],
            "{errs:?}"
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
    }

    /// The direct call keeps its own rule: `json.parse<User>(s)` binds `T`
    /// to `User` and is the validating call, under either import spelling.
    #[test]
    fn a_direct_json_parse_call_is_unchanged_by_the_value_rule() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             import std/json { parse }\n\
             type User = { name: string, }\n\
             pub fn f(s: string) -> number {\n\
             \x20 let a: Result<User, Array<Issue>> = json.parse<User>(s)\n\
             \x20 let b: Result<User, Array<Issue>> = parse<User>(s)\n\
             \x20 return 0\n\
             }\n",
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// E0233 on a type application: a written type with more or fewer
    /// arguments than the declaration it names. Before this it was left to
    /// `tsc` (TS2314, TS2315) and `--no-tsc` passed it. A local `type`, a
    /// local `interface`, a prelude container and a non-generic declaration
    /// (which takes 0) are all counted, wherever the type is written.
    #[test]
    fn a_wrong_number_of_type_arguments_on_a_type_is_e0233() {
        let errs = errors_of(
            "module m\n\
             type Box<T> = { value: T, }\n\
             type User = { name: string, }\n\
             interface Named<T> {\n\
             \x20 name: T\n\
             }\n\
             type Wrapped = { b: Box<number, string>, }\n\
             fn take(r: Result<number>) -> number { return 0 }\n\
             pub fn f(n: Named<string, number>) -> number {\n\
             \x20 let o: Option<number, string> = Some(1)\n\
             \x20 let u: User<number> = { name: \"a\", }\n\
             \x20 let xs: Array<Box<number, bool>> = []\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            type_arg_counts(&errs),
            vec![
                ("Box".to_string(), 1, 2),
                ("Result".to_string(), 2, 1),
                ("Named".to_string(), 1, 2),
                ("Option".to_string(), 1, 2),
                ("User".to_string(), 0, 1),
                ("Box".to_string(), 1, 2),
            ],
            "{errs:?}"
        );
    }

    /// The right count on a type stays silent, a type parameter applied to
    /// arguments is not a declaration the checker can count, and the prelude
    /// containers take the counts TypeScript gives them.
    #[test]
    fn the_right_number_of_type_arguments_on_a_type_is_not_e0233() {
        let errs = errors_of(
            "module m\n\
             import std/result { Result }\n\
             type Box<T> = { value: T, }\n\
             type Pair<A, B> = { a: A, b: B, }\n\
             fn take(r: Result<number, string>, m: Record<string, number>) -> number { return 0 }\n\
             pub fn f(b: Box<number>, p: Pair<string, Box<bool>>) -> number {\n\
             \x20 let o: Option<number> = Some(1)\n\
             \x20 let n: Nullable<string> = null\n\
             \x20 let xs: Array<Box<number>> = []\n\
             \x20 return 0\n\
             }\n",
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
    }

    /// G292, the bare count. A generic declaration written with no type
    /// arguments takes fewer than it has parameters, so it is E0233 wherever
    /// a type is written: a parameter, `let`, field or return annotation, a
    /// type argument, an interface name, a prelude container and a D46 body. Left alone, every parameter read through it stayed an
    /// unsubstituted `Param`, so nothing behind it was checked even by shape.
    #[test]
    fn a_generic_type_written_with_no_type_arguments_is_e0233() {
        let errs = errors_of(
            "module m\n\
             import std/json\n\
             type Box<T> = { value: T, }\n\
             interface Named<T> {\n\
             \x20 name: T\n\
             }\n\
             type B = Box\n\
             type Holder = { b: Box, }\n\
             fn arr(a: Array) -> number { return 0 }\n\
             fn back(x: Box<number>) -> Box { return x }\n\
             pub fn f(n: Named, x: Box<number>, m: Record) -> number {\n\
             \x20 let b: Box = x\n\
             \x20 let r = json.parse<Box>(\"1\")\n\
             \x20 return 0\n\
             }\n",
        );
        let mut got = type_arg_counts(&errs);
        got.sort();
        let mut want = vec![
            ("Box".to_string(), 1, 0),
            ("Box".to_string(), 1, 0),
            ("Array".to_string(), 1, 0),
            ("Box".to_string(), 1, 0),
            ("Named".to_string(), 1, 0),
            ("Record".to_string(), 2, 0),
            ("Box".to_string(), 1, 0),
            ("Box".to_string(), 1, 0),
        ];
        want.sort();
        assert_eq!(got, want, "{errs:?}");
        let e = errs
            .iter()
            .find(|e| matches!(e, TypeError::TypeArgumentCountMismatch { name, .. } if name == "Array"))
            .expect("an E0233 on `Array`");
        assert_eq!(
            e.to_string(),
            "wrong number of type arguments: `Array` takes 1, found 0"
        );
    }

    /// What the bare count leaves alone. A type name in an expression
    /// (`Box.is(v)`) is the descriptor, not a type. A non-generic
    /// declaration, a primitive and a type parameter are written bare as
    /// always, and the base of an applied type is not itself a bare use. A
    /// name is counted by what it resolves to; the shadowing cases are in
    /// `tests/negative/type_shadows_prelude_array.glyph` and the CLI test of
    /// a sibling's non-generic `Result`.
    #[test]
    fn a_bare_name_that_takes_no_arguments_is_not_e0233() {
        let errs = errors_of(
            "module m\n\
             type Box<T> = { value: T, }\n\
             type User = { name: string, }\n\
             pub fn f<T>(t: T, u: User, b: Box<User>, v: unknown) -> bool {\n\
             \x20 let s: string = u.name\n\
             \x20 return Box.is(v)\n\
             }\n",
        );
        assert!(type_arg_counts(&errs).is_empty(), "{errs:?}");
    }

    /// G293, the reverse of G289. A declared record or tagged union where a
    /// prelude container is declared is another declaration (D49), refused at
    /// a `let`, a `return` and a call argument. `Nullable` is decided against
    /// its argument instead, and `Record` field by field (see the tests after
    /// the next one).
    #[test]
    fn a_declared_type_where_a_prelude_container_is_expected_is_refused() {
        let errs = errors_of(
            "module m\n\
             import std/result { Result }\n\
             import std/option { Option }\n\
             import std/url\n\
             type User = { name: string, }\n\
             type Shape = | Circle(number) | Square(number)\n\
             fn take(r: Result<User, string>) -> number { return 0 }\n\
             fn back(u: User) -> Option<User> { return u }\n\
             pub fn f(u: User, s: Shape, link: url.Url) -> number {\n\
             \x20 let r: Result<User, string> = u\n\
             \x20 let xs: Array<Shape> = s\n\
             \x20 let o: Option<url.Url> = link\n\
             \x20 let n: Nullable<Shape> = u\n\
             \x20 return take(u)\n\
             }\n",
        );
        let got = g289_mismatches(&errs);
        for want in [
            ("Result<User, string>", "User"),
            ("Option<User>", "User"),
            ("Array<Shape>", "Shape"),
            ("Option<url.Url>", "url.Url"),
            ("Nullable<Shape>", "User"),
        ] {
            assert!(
                got.iter().any(|(e, f)| e == want.0 && f == want.1),
                "missing E0204 expected `{}` found `{}`: {errs:?}",
                want.0,
                want.1
            );
        }
        assert_eq!(got.len(), 5, "{errs:?}");
        assert_eq!(
            argument_mismatches(&errs),
            vec![("Result<User, string>".to_string(), "User".to_string())],
            "{errs:?}"
        );
    }

    /// What stays accepted: a `Nullable` of the same declaration (D45), the
    /// value wrapped in its container, a record with no fields, an interface,
    /// an object literal, which has no declaration and is compared field by
    /// field, and a closed record whose fields are all required `V`s where a
    /// `Record<string, V>` map is declared (the tests below decide the rest of
    /// that pairing).
    #[test]
    fn a_declared_type_against_a_container_it_belongs_in_stays_silent() {
        let errs = errors_of(
            "module m\n\
             import std/result { Ok, Result }\n\
             import std/option { Some, Option }\n\
             type User = { name: string, }\n\
             type Empty = {}\n\
             interface Named {\n\
             \x20 name: string\n\
             }\n\
             pub fn f(u: User, e: Empty, i: Named) -> number {\n\
             \x20 let n: Nullable<User> = u\n\
             \x20 let r: Result<User, string> = Ok(u)\n\
             \x20 let o: Option<User> = Some(u)\n\
             \x20 let m: Record<string, string> = e\n\
             \x20 let mu: Record<string, string> = u\n\
             \x20 let k: Record<string, string> = { name: \"a\" }\n\
             \x20 let j: Nullable<Named> = i\n\
             \x20 return 0\n\
             }\n",
        );
        assert!(g289_mismatches(&errs).is_empty(), "{errs:?}");
    }

    /// G293, `Record<string, V>`. A closed declared record is compared field
    /// by field against `V`, which is what the map's validator does with it:
    /// a required field whose type is not a `V` is refused, and a record whose
    /// fields are all required `V`s is accepted. The same for a record
    /// declared here, a generic one applied, and a stdlib-modeled one.
    #[test]
    fn a_declared_record_where_a_string_keyed_map_is_declared_is_compared_by_field() {
        let errs = errors_of(
            "module m\n\
             import std/url\n\
             import std/fs\n\
             type User = { name: string, age: number, }\n\
             type Label = { name: string, title: string, }\n\
             type Box<T> = { value: T, }\n\
             fn take(m: Record<string, string>) -> number { return 0 }\n\
             fn back(u: User) -> Record<string, string> { return u }\n\
             pub fn f(u: User, l: Label, bs: Box<string>, bn: Box<number>, p: url.Param, info: fs.FileInfo) -> number {\n\
             \x20 let a: Record<string, string> = u\n\
             \x20 let b: Record<string, string> = l\n\
             \x20 let c: Record<string, string> = bs\n\
             \x20 let d: Record<string, string> = bn\n\
             \x20 let e: Record<string, string> = p\n\
             \x20 let g: Record<string, string> = info\n\
             \x20 let h: Record<string, unknown> = u\n\
             \x20 return take(u)\n\
             }\n",
        );
        let got = g289_mismatches(&errs);
        for want in [
            ("Record<string, string>", "User"),
            ("Record<string, string>", "Box<number>"),
            ("Record<string, string>", "fs.FileInfo"),
        ] {
            assert!(
                got.iter().any(|(e, f)| e == want.0 && f == want.1),
                "missing E0204 expected `{}` found `{}`: {errs:?}",
                want.0,
                want.1
            );
        }
        // The three above plus the `return u`; `Label`, `Box<string>`,
        // `url.Param` and `Record<string, unknown>` are accepted.
        assert_eq!(got.len(), 4, "{errs:?}");
        assert_eq!(
            argument_mismatches(&errs),
            vec![("Record<string, string>".to_string(), "User".to_string())],
            "{errs:?}"
        );
    }

    /// G303. `print` and `assert` are prelude functions with a declared
    /// parameter (`runtime/glyph-prelude.d.ts`), and so are `number.to_string`
    /// and `number.parse`. They typed `Unknown`, so a wrong argument passed
    /// Glyph's checker and only `tsc` refused it.
    #[test]
    fn a_prelude_function_argument_is_checked_against_its_parameter() {
        let errs = errors_of(
            "module m\n\
             pub fn f(b: bool, n: number, s: string) -> void {\n\
             \x20 print(b)\n\
             \x20 print(n)\n\
             \x20 assert(s)\n\
             \x20 let t = number.to_string(b)\n\
             \x20 let r = number.parse(n)\n\
             \x20 print(t)\n\
             }\n",
        );
        let found: Vec<(&str, String, String)> = errs
            .iter()
            .filter_map(|e| match e {
                TypeError::ArgumentTypeMismatch { expected, found, .. } => {
                    Some((e.code(), expected.clone(), found.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            found,
            vec![
                ("E0211", "string".to_string(), "bool".to_string()),
                ("E0211", "string".to_string(), "number".to_string()),
                ("E0211", "bool".to_string(), "string".to_string()),
                ("E0211", "number".to_string(), "bool".to_string()),
                ("E0211", "string".to_string(), "number".to_string()),
            ],
            "{errs:?}"
        );
    }

    /// G293. An `@open` record's validator admits keys beyond its fields,
    /// holding values no validator checked, so it is refused where any
    /// `Record<string, V>` is declared unless `V` is `unknown`, even when
    /// every declared field is a `V`.
    #[test]
    fn an_open_record_where_a_string_keyed_map_is_declared_is_refused_unless_unknown() {
        let errs = errors_of(
            "module m\n\
             @open\n\
             type Meta = { id: string, }\n\
             type Alias = Meta\n\
             pub fn f(meta: Meta, al: Alias) -> number {\n\
             \x20 let a: Record<string, string> = meta\n\
             \x20 let b: Record<string, string> = al\n\
             \x20 let c: Record<string, unknown> = meta\n\
             \x20 return 0\n\
             }\n",
        );
        let got = g289_mismatches(&errs);
        assert!(
            got.iter().any(|(e, f)| e == "Record<string, string>" && f == "Meta"),
            "{errs:?}"
        );
        assert!(
            got.iter().any(|(e, f)| e == "Record<string, string>" && f == "Alias"),
            "{errs:?}"
        );
        assert_eq!(got.len(), 2, "{errs:?}");
    }

    /// G293 with a generic `@open` record. Substituting the arguments does
    /// not close it: `Bag<string>` has only `string` fields and its validator
    /// still admits keys beyond them, so it is refused where a
    /// `Record<string, string>` is declared and accepted only for `unknown`.
    #[test]
    fn a_generic_open_record_where_a_string_keyed_map_is_declared_is_refused_unless_unknown() {
        let errs = errors_of(
            "module m\n\
             @open\n\
             type Bag<T> = { id: T, }\n\
             pub fn f(b: Bag<string>) -> number {\n\
             \x20 let a: Record<string, string> = b\n\
             \x20 let c: Record<string, unknown> = b\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("Record<string, string>".to_string(), "Bag<string>".to_string())],
            "{errs:?}"
        );
    }

    /// G293 with a generic record whose substituted field is still a type
    /// parameter. `T` may be anything, so `Box<T>` is not a
    /// `Record<string, string>` and Glyph refuses it, as 0.1.131 did; `tsc`
    /// refuses it too (TS2322). It is accepted where `V` is `unknown`, and
    /// where `V` is that same parameter.
    #[test]
    fn a_generic_record_with_an_open_parameter_field_is_not_a_string_map() {
        let errs = errors_of(
            "module m\n\
             type Box<T> = { value: T, }\n\
             fn g<T>(x: Box<T>) -> Record<string, string> { return x }\n\
             fn h<T>(x: Box<T>) -> Record<string, unknown> { return x }\n\
             fn k<T>(x: Box<T>) -> Record<string, T> { return x }\n\
             pub fn f<T>(b: Box<T>) -> number {\n\
             \x20 let a: Record<string, string> = b\n\
             \x20 let c: Record<string, unknown> = b\n\
             \x20 return 0\n\
             }\n",
        );
        let got = g289_mismatches(&errs);
        assert_eq!(got.len(), 2, "{errs:?}");
        assert!(
            got.iter().all(|(e, f)| e == "Record<string, string>" && f.starts_with("Box<")),
            "{errs:?}"
        );
    }

    /// G293. A tagged union has no fields in Glyph, and its runtime encoding
    /// is not a contract, so it is refused wherever any `Record<K, V>` is
    /// declared: a `V` of `unknown` and a `K` other than `string` included.
    #[test]
    fn a_tagged_union_where_a_map_is_declared_is_refused() {
        let errs = errors_of(
            "module m\n\
             import std/fs\n\
             type Mode = | A | B\n\
             type Key = \"a\" | \"b\"\n\
             pub fn f(mode: Mode, k: fs.ErrorKind) -> number {\n\
             \x20 let a: Record<string, string> = mode\n\
             \x20 let b: Record<string, unknown> = mode\n\
             \x20 let c: Record<Key, string> = mode\n\
             \x20 let d: Record<string, string> = k\n\
             \x20 return 0\n\
             }\n",
        );
        let got = g289_mismatches(&errs);
        for want in [
            ("Record<string, string>", "Mode"),
            ("Record<string, unknown>", "Mode"),
            ("Record<Key, string>", "Mode"),
            ("Record<string, string>", "fs.ErrorKind"),
        ] {
            assert!(
                got.iter().any(|(e, f)| e == want.0 && f == want.1),
                "missing E0204 expected `{}` found `{}`: {errs:?}",
                want.0,
                want.1
            );
        }
        assert_eq!(got.len(), 4, "{errs:?}");
    }

    /// G293. What the map's validator and `tsc` disagree about is not decided
    /// here, and `tsc` answers it. An optional field: the validator accepts
    /// the absent key, while `tsc` accepts `nick?: string` and refuses `nick?:
    /// number` (TS2322), both checked against `glyph check`. A `K` other than
    /// `string`: `tsc` requires the keys and the validator ignores them. A
    /// required field that is not a `V` still decides the pairing beside an
    /// optional one.
    #[test]
    fn an_optional_field_and_a_non_string_key_are_left_to_tsc() {
        let errs = errors_of(
            "module m\n\
             type Nick = { name: string, nick?: string, }\n\
             type Loose = { name: string, nick?: number, }\n\
             type Both = { age: number, nick?: number, }\n\
             type User = { name: string, age: number, }\n\
             type Key = \"name\" | \"age\"\n\
             pub fn f(n: Nick, l: Loose, b: Both, u: User) -> number {\n\
             \x20 let a: Record<string, string> = n\n\
             \x20 let c: Record<string, string> = l\n\
             \x20 let d: Record<Key, string> = u\n\
             \x20 let e: Record<number, string> = u\n\
             \x20 let g: Record<string, string> = b\n\
             \x20 return 0\n\
             }\n",
        );
        assert_eq!(
            g289_mismatches(&errs),
            vec![("Record<string, string>".to_string(), "Both".to_string())],
            "{errs:?}"
        );
    }

    /// G293 across a module boundary. An imported record is compared field by
    /// field where a `Record<string, V>` is declared, an imported `@open`
    /// record is refused unless `V` is `unknown`, and an imported union is
    /// refused wherever a map is declared.
    #[test]
    fn an_imported_declaration_where_a_string_keyed_map_is_declared() {
        let src = r#"module x
import lib { Point, Meta, Status }

fn main(p: Point, m: Meta, s: Status) -> number {
  let a: Record<string, number> = p
  let b: Record<string, string> = p
  let c: Record<string, string> = m
  let d: Record<string, unknown> = m
  let e: Record<string, string> = s
  return 0
}
"#;
        let errs = lib_errors_of(src);
        let got = g289_mismatches(&errs);
        for want in [
            ("Record<string, string>", "Point"),
            ("Record<string, string>", "Meta"),
            ("Record<string, string>", "Status"),
        ] {
            assert!(
                got.iter().any(|(e, f)| e == want.0 && f == want.1),
                "missing E0204 expected `{}` found `{}`: {errs:?}",
                want.0,
                want.1
            );
        }
        assert_eq!(got.len(), 3, "{errs:?}");
    }

    /// The other side of G303: the arguments the declarations accept stay
    /// silent, the results carry their types, and a call with the wrong
    /// number of arguments is E0213 as it is for any `fn`.
    #[test]
    fn a_prelude_function_accepts_what_it_declares() {
        let errs = errors_of(
            "module m\n\
             pub fn f(b: bool, n: number, s: string) -> void {\n\
             \x20 print(s)\n\
             \x20 print(\"lit\")\n\
             \x20 print(number.to_string(n))\n\
             \x20 assert(b)\n\
             \x20 assert(n > 0)\n\
             \x20 let r = number.parse(s)\n\
             \x20 let x: number = number.to_string(n)\n\
             \x20 print()\n\
             }\n",
        );
        let codes: Vec<&str> = errs
            .iter()
            .map(|e| e.code())
            .filter(|c| *c != "E0217")
            .collect();
        assert_eq!(codes, vec!["E0204", "E0213"], "{errs:?}");
    }

    /// The `E0220`s a program draws for literal arms, as `(value, union,
    /// suggestion)`.
    fn unknown_literal_arms(errs: &[TypeError]) -> Vec<(String, String, Option<String>)> {
        errs.iter()
            .filter_map(|e| match e {
                TypeError::UnknownLiteralPattern { value, union, suggestion, .. } => {
                    assert_eq!(e.code(), "E0220");
                    Some((value.clone(), union.clone(), suggestion.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// G305. A literal arm outside the scrutinee's string-literal union can
    /// never run, and `tsc` refuses it (TS2678). It passed Glyph's checker,
    /// whether the literal was a new value (`"c"`) or a misspelling next to
    /// the right one (`"manuel"` beside `"manual"`), and an `else` did not
    /// change that.
    #[test]
    fn a_literal_arm_outside_the_union_is_refused() {
        let errs = errors_of(
            "module m\n\
             type Mode = \"a\" | \"b\"\n\
             type Entry = \"manual\" | \"auto\"\n\
             pub fn f(m: Mode) -> string {\n\
             \x20 return match m {\n\
             \x20\x20\x20 \"a\" => \"A\",\n\
             \x20\x20\x20 \"b\" => \"B\",\n\
             \x20\x20\x20 \"c\" => \"C\",\n\
             \x20 }\n\
             }\n\
             pub fn g(e: Entry) -> string {\n\
             \x20 return match e {\n\
             \x20\x20\x20 \"manuel\" => \"X\",\n\
             \x20\x20\x20 else => \"A\",\n\
             \x20 }\n\
             }\n\
             pub fn h(i: \"x\" | \"y\") -> string {\n\
             \x20 return match i {\n\
             \x20\x20\x20 \"x\" => \"X\",\n\
             \x20\x20\x20 \"y\" => \"Y\",\n\
             \x20\x20\x20 \"zebra\" => \"Z\",\n\
             \x20 }\n\
             }\n",
        );
        assert_eq!(
            unknown_literal_arms(&errs),
            vec![
                ("c".to_string(), "Mode".to_string(), None),
                ("manuel".to_string(), "Entry".to_string(), Some("manual".to_string())),
                ("zebra".to_string(), "\"x\" | \"y\"".to_string(), None),
            ],
            "{errs:?}"
        );
        let codes: Vec<&str> = errs.iter().map(|e| e.code()).collect();
        assert_eq!(codes, vec!["E0220", "E0220", "E0220"], "{errs:?}");
    }

    /// G305's suggestion rule. The did-you-mean comes only from the values no
    /// other arm names: `"ad"` is one edit from both `"ab"` and `"ac"`, and
    /// `"ab"` has an arm, so the suggestion is `"ac"`. With every value named
    /// (`"c"` over `"a" | "b"` with both arms written) there is none, since
    /// either would give the match a second arm that can never run (E0305).
    /// An `else` names nothing, so it leaves `"ac"` a candidate.
    #[test]
    fn a_literal_arm_suggests_only_a_value_no_other_arm_names() {
        let errs = errors_of(
            "module m\n\
             type Two = \"ab\" | \"ac\"\n\
             type Mode = \"a\" | \"b\"\n\
             pub fn f(t: Two) -> string {\n\
             \x20 return match t {\n\
             \x20\x20\x20 \"ab\" => \"B\",\n\
             \x20\x20\x20 \"ad\" => \"D\",\n\
             \x20\x20\x20 else => \"E\",\n\
             \x20 }\n\
             }\n\
             pub fn g(m: Mode) -> string {\n\
             \x20 return match m {\n\
             \x20\x20\x20 \"a\" => \"A\",\n\
             \x20\x20\x20 \"c\" => \"C\",\n\
             \x20\x20\x20 \"b\" => \"B\",\n\
             \x20 }\n\
             }\n",
        );
        assert_eq!(
            unknown_literal_arms(&errs),
            vec![
                ("ad".to_string(), "Two".to_string(), Some("ac".to_string())),
                ("c".to_string(), "Mode".to_string(), None),
            ],
            "{errs:?}"
        );
    }

    /// The arms G305 must not touch: every value of the union, with or
    /// without a catch-all, and a literal arm over a plain `string`, whose
    /// domain has no outside.
    #[test]
    fn literal_arms_inside_the_union_stay_silent() {
        let errs = errors_of(
            "module m\n\
             type Mode = \"a\" | \"b\"\n\
             pub fn f(m: Mode, s: string) -> string {\n\
             \x20 let x = match m {\n\
             \x20\x20\x20 \"a\" => \"A\",\n\
             \x20\x20\x20 else => \"B\",\n\
             \x20 }\n\
             \x20 let y = match s {\n\
             \x20\x20\x20 \"c\" => \"C\",\n\
             \x20\x20\x20 else => \"D\",\n\
             \x20 }\n\
             \x20 return match m {\n\
             \x20\x20\x20 \"a\" => x,\n\
             \x20\x20\x20 \"b\" => y,\n\
             \x20 }\n\
             }\n",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// G305 under both import spellings of a sibling's union.
    #[test]
    fn a_literal_arm_outside_an_imported_union_is_refused() {
        struct ModesModule;
        impl DeclTyResolver for ModesModule {
            fn decl_ty(&self, _decl_idx: u32) -> Ty {
                Ty::Unknown
            }
            fn imported_type_decl(
                &self,
                module_path: &str,
                type_name: &str,
            ) -> Option<ImportedTypeDecl> {
                (module_path == "modes" && type_name == "Mode").then(|| ImportedTypeDecl {
                    name: Ident::from("Mode"),
                    generics: Vec::new(),
                    body: Ty::StringLiteralUnion(vec!["manual".to_string(), "auto".to_string()]),
                    open: false,
                })
            }
        }
        let src = "module app\n\
                   import modes { Mode }\n\
                   import modes as md\n\
                   pub fn named(m: Mode) -> number {\n\
                   \x20 return match m {\n\
                   \x20\x20\x20 \"auto\" => 2,\n\
                   \x20\x20\x20 \"manuel\" => 3,\n\
                   \x20 }\n\
                   }\n\
                   pub fn spaced(m: md.Mode) -> number {\n\
                   \x20 return match m {\n\
                   \x20\x20\x20 \"auto\" => 2,\n\
                   \x20\x20\x20 \"manuel\" => 3,\n\
                   \x20 }\n\
                   }\n";
        let m = glyph_parser::parse(src).expect("parse failed");
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _errs) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = assign_types_with_resolver(&m, &resolved, &prelude, &ModesModule);
        let expected = ("manuel".to_string(), "Mode".to_string(), Some("manual".to_string()));
        assert_eq!(
            unknown_literal_arms(&errs),
            vec![expected.clone(), expected],
            "the import spelling does not change the answer: {errs:?}"
        );
    }
}
