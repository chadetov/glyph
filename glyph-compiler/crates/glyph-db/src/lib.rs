//! Glyph incremental query database (I4).
//!
//! Wraps the per-file pipeline as salsa-tracked queries. The dependency DAG
//! is a fan-out from `parse_module`; downstream queries call it directly and
//! rely on salsa's memoization to share one parsed AST per `SourceFile`.
//!
//! ```text
//!                    SourceFile  [salsa::input]
//!                         │
//!                         ▼
//!                   parse_module
//!              ┌──────┬──────┬──────┐
//!              ▼      ▼      ▼      │
//!      module_symbols │  import_diagnostics
//!              │      │
//!              ▼      │
//!           resolve ◄─┘
//!              │
//!              ▼
//!          type_map
//! ```
//!
//! Per I4: per-file inputs, per-declaration intermediates. This slice ships
//! the per-file half; per-declaration tracked queries (lowered decl types,
//! resolved-ref subsets) land in week 2 day 6+. The cross-stage data flow
//! mirrors what the example tests already do by hand — but now it's
//! memoized: changing one file's text invalidates only that file's queries.
//!
//! ## Why manual `Update` impls and not `#[derive(salsa::Update)]`?
//!
//! salsa stores tracked-fn return values internally and uses
//! `Update::maybe_update` to decide if downstream queries need
//! recomputation. The blanket impls cover primitives, `Vec`, `Arc`, etc., but
//! a user struct needs either `#[derive(salsa::Update)]` (which requires the
//! transitive type closure to implement Update — invasive across
//! `glyph-ast`, `glyph-resolver`, and `glyph-typechecker`) or a hand-written
//! impl. We pick the wrapper-with-manual-Update approach: `glyph-ast` and the
//! other upstream crates stay unaware of salsa. The wrapper newtypes here
//! (`ParsedModule`, `Symbols`, `Diagnostics`, `Resolved`, `Types`) hold
//! `Arc<…>` of the real payload, and the local `impl_wrapper_update!` macro
//! produces a `maybe_update` that compares for equality (delegating to
//! `Arc`'s `PartialEq`, which fast-paths on pointer-equality) and overwrites
//! on change. Single macro definition = single source of truth for the
//! invalidation protocol. Same semantics salsa's blanket `Update` for
//! `Vec<T>` ultimately provides.
//!
//! The `unsafe` for `Update::maybe_update` is satisfied here because:
//! - the payload types all derive `Eq`, so the `Eq` invariant the trait doc
//!   asks for holds; and
//! - we never hand out `'db`-bound references to the inner payload that
//!   outlive the wrapper, so the "no `&'db T`" caution doesn't bind.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::sync::Arc;

pub use salsa::Setter;

use std::collections::{HashMap, HashSet};

use glyph_ast::{Decl, Ident, Module, ModulePath, TypeExpr};
use glyph_parser::ParseError;
use glyph_resolver::{
    build_prelude, collect_module_symbols, path_key, resolve_module, verify_imports,
    ModuleExports, ModuleGraph, ModuleSymbols, Prelude, ResolveError, ResolvedModule,
    StdlibStubs, SymbolKind,
};
use glyph_typechecker::{
    assign_types_with_resolver, DeclTyResolver, Lowerer, Ty, TypeError, TypeMap,
};

// ============================================================================
// Update macro for wrapper newtypes
// ============================================================================

/// Implements `salsa::Update` for an `Arc`-wrapped newtype whose `PartialEq`
/// is derived. Centralizes the unsafe-impl protocol so the five wrapper
/// types can't drift apart by copy-paste error. See the module docstring for
/// the safety rationale.
macro_rules! impl_wrapper_update {
    ($t:ty) => {
        // SAFETY: the wrapper derives `Eq`; the body follows the contract at
        // salsa::update::Update::maybe_update — compare via PartialEq, write
        // only on change, return whether a change occurred. No `'db`-bound
        // references escape the wrapper.
        unsafe impl salsa::Update for $t {
            unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
                // SAFETY: caller guarantees `old_pointer` is a valid
                // initialized `Self` per Update::maybe_update's contract.
                let old = unsafe { &mut *old_pointer };
                if *old == new_value {
                    false
                } else {
                    *old = new_value;
                    true
                }
            }
        }
    };
}

// ============================================================================
// Database trait + concrete db
// ============================================================================

/// The Glyph compiler's salsa database trait. Adds two pieces of "ambient"
/// state alongside the salsa runtime: the global prelude and the module
/// graph. Neither changes during the lifetime of a `CompilerDb`, so they're
/// not modeled as salsa inputs — tracked queries read them via these
/// accessors and the invalidation story is "rebuild the db if the prelude
/// or graph changes," which matches how `glyph build` will use it.
#[salsa::db]
pub trait Db: salsa::Database {
    fn prelude(&self) -> &Prelude;
    fn module_graph(&self) -> &dyn ModuleGraph;
    /// Lazy-init salsa input describing the project's files. Tracked
    /// queries (notably `import_diagnostics`) read this to compose
    /// cross-module verification atop the static `module_graph`.
    fn project_files_input(&self) -> ProjectFiles;
}

/// Concrete database. Holds the salsa storage, a prelude, the stdlib
/// module graph, and a lazily-created `ProjectFiles` salsa input. Multiple
/// `CompilerDb`s can coexist; they don't share state.
#[salsa::db]
#[derive(Clone)]
pub struct CompilerDb {
    storage: salsa::Storage<Self>,
    prelude: Arc<Prelude>,
    /// Static module graph for stdlib/third-party paths (Phase 5 package
    /// manifest territory). Project-local files resolve through the
    /// separate `project_files` salsa input.
    module_graph: Arc<dyn ModuleGraph + Send + Sync>,
    /// Salsa input listing the project's `.glyph` files and their module
    /// paths. **Invariant: always `Some` after `new()`.** The `Option`
    /// is a structural concession (Rust can't have a struct field whose
    /// constructor requires the partially-built struct), not a
    /// nullability claim. Eager init in `new()` means every clone of
    /// this db shares the same `ProjectFiles` ID — a lazy-init path
    /// would let two clones independently create distinct inputs and
    /// silently disagree on which `set_project` mutations to observe.
    project_files: Option<ProjectFiles>,
    /// Test-only event log shared by every clone of this db. The salsa
    /// callback installed in `new()` pushes every `EventKind` into the
    /// underlying `Arc<Mutex<Vec<EventKind>>>`, so tests can `drain_events()`
    /// to assert on cache hits, re-execution, and so on.
    #[cfg(test)]
    events: tests::EventLog,
}

impl CompilerDb {
    /// Build a database with the default prelude and the stdlib stubs as the
    /// module graph. Useful for tests; production callers should construct
    /// their own graph (Phase 5 package manifest).
    pub fn with_default_stdlib() -> Self {
        Self::new(build_prelude(), Arc::new(StdlibStubs::new()))
    }

    /// Build a database with the given prelude and graph.
    pub fn new(prelude: Prelude, module_graph: Arc<dyn ModuleGraph + Send + Sync>) -> Self {
        #[cfg(test)]
        let events = tests::EventLog::default();
        #[cfg(test)]
        let storage = {
            let events = events.clone();
            salsa::Storage::new(Some(Box::new(move |ev: salsa::Event| {
                events.record(ev.kind);
            })))
        };
        #[cfg(not(test))]
        let storage = salsa::Storage::default();
        // Two-step initialization: construct the db with `project_files:
        // None`, then create the `ProjectFiles` salsa input against the
        // live db, then assign. `ProjectFiles::new` requires `&dyn Db`,
        // so the field can't be populated in the struct literal.
        let mut db = Self {
            storage,
            prelude: Arc::new(prelude),
            module_graph,
            project_files: None,
            #[cfg(test)]
            events,
        };
        db.project_files = Some(ProjectFiles::new(&db, Vec::new()));
        db
    }

    /// Get the `ProjectFiles` salsa input. Cheap: returns a `Copy` salsa
    /// ID. The same ID for every clone of this db (see the `project_files`
    /// field doc).
    pub fn project_files_input(&self) -> ProjectFiles {
        self.project_files
            .expect("project_files is set by `CompilerDb::new`; this should never fire")
    }

    /// Replace the project's file set. Salsa-invalidates any tracked query
    /// that transitively depends on the file list — including
    /// `project_exports` and any `import_diagnostics` that consults it.
    /// One salsa revision per call.
    pub fn set_project(&mut self, entries: Vec<(String, SourceFile)>) {
        let pf = self.project_files_input();
        pf.set_entries(self).to(entries);
    }
}

#[salsa::db]
impl salsa::Database for CompilerDb {}

#[salsa::db]
impl Db for CompilerDb {
    fn prelude(&self) -> &Prelude {
        &self.prelude
    }

    fn module_graph(&self) -> &dyn ModuleGraph {
        &*self.module_graph
    }

    fn project_files_input(&self) -> ProjectFiles {
        CompilerDb::project_files_input(self)
    }
}

// ============================================================================
// Salsa input: SourceFile
// ============================================================================

/// One file's source text. Carrying a virtual path (instead of a real
/// filesystem path) keeps the test surface and the future-`glyph build`
/// surface the same: both pass strings.
#[salsa::input]
pub struct SourceFile {
    #[returns(ref)]
    pub virtual_path: String,
    #[returns(ref)]
    pub text: String,
}

// ============================================================================
// Salsa input: ProjectFiles
// ============================================================================

/// Salsa input representing the project's set of `.glyph` files. Each
/// entry pairs a slash-joined module path (`"app/users"`) with the
/// corresponding `SourceFile`. Mutated via `CompilerDb::set_project`;
/// changes invalidate `project_exports` and any tracked query that
/// transitively reads it.
#[salsa::input]
pub struct ProjectFiles {
    #[returns(ref)]
    pub entries: Vec<(String, SourceFile)>,
}

// ============================================================================
// Stage 1 wrapper: ParsedModule
// ============================================================================

/// Outcome of parsing one file. Either an `Arc<Module>` (success) or a
/// stringified error message. `Arc` lets downstream stages share the AST
/// cheaply without cloning the whole tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedModule {
    inner: Arc<ParsedModuleInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedModuleInner {
    module: Result<Module, ParseError>,
}

impl ParsedModule {
    pub fn ok(module: Module) -> Self {
        Self {
            inner: Arc::new(ParsedModuleInner {
                module: Ok(module),
            }),
        }
    }

    pub fn err(error: ParseError) -> Self {
        Self {
            inner: Arc::new(ParsedModuleInner {
                module: Err(error),
            }),
        }
    }

    /// Returns the parsed module, or `None` if parsing failed.
    pub fn module(&self) -> Option<&Module> {
        self.inner.module.as_ref().ok()
    }

    /// Returns the structured parse error, if any. Carries the span and the
    /// thiserror-derived Display message; downstream consumers should render
    /// via `format!("{e}")` rather than `{e:?}`.
    pub fn error(&self) -> Option<&ParseError> {
        self.inner.module.as_ref().err()
    }
}

impl_wrapper_update!(ParsedModule);

// ============================================================================
// Stage 2 wrapper: Symbols
// ============================================================================

/// Outcome of `collect_module_symbols`: the top-level symbol table for the
/// file. Collection can fail (duplicate-name, relative-import); failure
/// reports the errors and a zero-symbol table for downstream stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbols {
    inner: Arc<SymbolsInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolsInner {
    symbols: Option<ModuleSymbols>,
    errors: Vec<ResolveError>,
}

impl Symbols {
    pub fn ok(symbols: ModuleSymbols) -> Self {
        Self {
            inner: Arc::new(SymbolsInner {
                symbols: Some(symbols),
                errors: Vec::new(),
            }),
        }
    }

    pub fn err(errors: Vec<ResolveError>) -> Self {
        Self {
            inner: Arc::new(SymbolsInner {
                symbols: None,
                errors,
            }),
        }
    }

    pub fn symbols(&self) -> Option<&ModuleSymbols> {
        self.inner.symbols.as_ref()
    }

    pub fn errors(&self) -> &[ResolveError] {
        &self.inner.errors
    }
}

impl_wrapper_update!(Symbols);

// ============================================================================
// Stage 3 wrapper: Diagnostics (cross-module verification)
// ============================================================================

/// Outcome of `verify_imports`: the set of `UnknownExportedName` errors for
/// the file's imports. Always succeeds (returns an empty list on no errors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostics {
    inner: Arc<DiagnosticsInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticsInner {
    errors: Vec<ResolveError>,
}

impl Diagnostics {
    pub fn new(errors: Vec<ResolveError>) -> Self {
        Self {
            inner: Arc::new(DiagnosticsInner { errors }),
        }
    }

    pub fn errors(&self) -> &[ResolveError] {
        &self.inner.errors
    }
}

impl_wrapper_update!(Diagnostics);

// ============================================================================
// Stage 4 wrapper: Resolved
// ============================================================================

/// Outcome of `resolve_module`: the resolved-name map plus any per-reference
/// resolution errors. The `ResolvedModule` and errors come together because
/// `resolve_module` returns them as a tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    inner: Arc<ResolvedInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedInner {
    resolved: Option<ResolvedModule>,
    errors: Vec<ResolveError>,
}

impl Resolved {
    pub fn new(resolved: ResolvedModule, errors: Vec<ResolveError>) -> Self {
        Self {
            inner: Arc::new(ResolvedInner {
                resolved: Some(resolved),
                errors,
            }),
        }
    }

    pub fn skipped(reason_errors: Vec<ResolveError>) -> Self {
        Self {
            inner: Arc::new(ResolvedInner {
                resolved: None,
                errors: reason_errors,
            }),
        }
    }

    pub fn resolved(&self) -> Option<&ResolvedModule> {
        self.inner.resolved.as_ref()
    }

    pub fn errors(&self) -> &[ResolveError] {
        &self.inner.errors
    }
}

impl_wrapper_update!(Resolved);

// ============================================================================
// Stage 5 wrapper: Types
// ============================================================================

/// Outcome of `assign_types`: the span-keyed `TypeMap`. Empty when the
/// upstream stages failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Types {
    inner: Arc<TypesInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypesInner {
    type_map: TypeMap,
    errors: Vec<TypeError>,
}

impl Types {
    pub fn new(type_map: TypeMap, errors: Vec<TypeError>) -> Self {
        Self {
            inner: Arc::new(TypesInner { type_map, errors }),
        }
    }

    pub fn empty() -> Self {
        Self::new(TypeMap::new(), Vec::new())
    }

    pub fn type_map(&self) -> &TypeMap {
        &self.inner.type_map
    }

    /// Typechecker diagnostics. Day 14 surfaces the first real entries
    /// (non-exhaustive match on tagged unions). Future weeks add
    /// bidirectional-check errors, `?` mismatches, `owned`
    /// single-consumption violations, and so on.
    pub fn errors(&self) -> &[TypeError] {
        &self.inner.errors
    }
}

impl_wrapper_update!(Types);

// ============================================================================
// Stage 6 wrapper: DeclTy (per-declaration intermediate)
// ============================================================================

/// Lowered `Ty` for a single top-level declaration, keyed by `decl_idx` into
/// the parsed module's `items` Vec. Per-declaration granularity is what makes
/// "edit one fn body, don't recompute the others' types" cheap — when
/// `decl_ty(file, k)`'s body re-runs but produces a structurally-equal Ty,
/// salsa's Update returns false and downstream queries that depend on
/// `decl_ty(file, k)` stay cached.
///
/// `Fn` and `Component` lower to a `Ty::Fn`. Other declarations are
/// `Ty::Unknown` here — `type` and `const` decls have their type information
/// fed in through different channels (the resolver carries the `type` body;
/// `const`'s annotation is read directly in week 3's bidirectional checker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclTy {
    inner: Arc<DeclTyInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclTyInner {
    ty: Ty,
}

impl DeclTy {
    pub fn new(ty: Ty) -> Self {
        Self {
            inner: Arc::new(DeclTyInner { ty }),
        }
    }

    /// Returns the lowered type. Cheap (one Arc deref).
    pub fn ty(&self) -> &Ty {
        &self.inner.ty
    }
}

impl_wrapper_update!(DeclTy);

// ============================================================================
// Stage 7 wrappers: DeclAst and ResolvedDecl (per-declaration input slices)
// ============================================================================

/// One top-level declaration extracted from a `ParsedModule`.
///
/// The wrapper carries the original `Decl` (for downstream consumers
/// like the Lowerer) plus a **source-byte canonical**: the bytes the
/// decl covers in the source file. `PartialEq` compares only the
/// canonical, not the Decl values, so two `DeclAst`s are equal when
/// their source text is identical — even if absolute byte positions
/// shifted because of edits to *other* decls. This lifts day-8's
/// equal-length restriction.
///
/// The trade-off: comment / whitespace edits *within* this decl change
/// the source bytes and invalidate the wrapper, even when the AST is
/// semantically identical. Acceptable in practice since comments rarely
/// change without surrounding code changing too; a future "strip spans
/// and compare structural AST" implementation could go finer.
#[derive(Debug, Clone)]
pub struct DeclAst {
    inner: Arc<DeclAstInner>,
}

#[derive(Debug, Clone)]
struct DeclAstInner {
    decl: Option<Decl>,
    /// Source bytes covered by `Decl`'s outer span. The fingerprint used
    /// by salsa's Update; comparing two `DeclAst`s is just an Arc<str>
    /// content compare.
    canonical: Arc<str>,
}

impl PartialEq for DeclAstInner {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for DeclAstInner {}

impl PartialEq for DeclAst {
    fn eq(&self, other: &Self) -> bool {
        // Inner is Arc<...>; fast-path on Arc::ptr_eq, then fall through
        // to inner PartialEq (which compares only the canonical bytes).
        Arc::ptr_eq(&self.inner, &other.inner) || *self.inner == *other.inner
    }
}

impl Eq for DeclAst {}

impl DeclAst {
    /// Construct an empty `DeclAst` (no decl, no canonical). Used on
    /// upstream-failure paths (parse failed, decl_idx out of range).
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(DeclAstInner {
                decl: None,
                canonical: Arc::from(""),
            }),
        }
    }

    /// Construct with the decl + the canonical source bytes.
    pub fn new(decl: Decl, source: &str, span: glyph_ast::Span) -> Self {
        let bytes = canonical_bytes(source, span);
        Self {
            inner: Arc::new(DeclAstInner {
                decl: Some(decl),
                canonical: bytes,
            }),
        }
    }

    /// `Some` if the file parsed and `decl_idx` was in range, else `None`.
    ///
    /// **Staleness contract**: under a backdated revision (when the
    /// decl's source bytes match cached but other parts of the file
    /// changed length), the returned `Decl` carries the spans of the
    /// REVISION WHERE IT WAS LAST RE-EXECUTED, not the current
    /// revision. Today's only consumer is `decl_ty` via the Lowerer,
    /// which also reads `resolved_decl` — both wrappers backdate
    /// together, so OLD `Decl` spans index correctly into OLD
    /// resolution-map keys. A future caller that pairs `decl_ast` with
    /// freshly-built current-revision data (e.g., a per-revision
    /// `TypeMap`) must NOT trust the carried spans to align with
    /// current source positions.
    pub fn decl(&self) -> Option<&Decl> {
        self.inner.decl.as_ref()
    }
}

impl_wrapper_update!(DeclAst);

/// The resolver output sliced to spans inside one declaration's signature.
/// Carries a `ResolvedModule` whose `resolutions` map contains only the
/// entries the `Lowerer` will query when typing this decl's params and
/// return type — i.e. every `TypeExpr` path inside the signature.
///
/// Uses the same source-byte canonical as `DeclAst` for `PartialEq`. Two
/// `ResolvedDecl`s are equal when their decl's source bytes match —
/// independent of any absolute-span shifts in the carried
/// `ResolvedModule`. The (Symbol.span values inside the cloned symbol
/// table) and (absolute spans as keys in the resolution map) become
/// invisible to salsa's change-detection.
///
/// Correctness rationale: when the source bytes of the decl are
/// unchanged, the symbolic resolutions for the decl's signature are
/// unchanged too (module structure is stable, `SymbolId` allocation is
/// source-order, prelude is fixed). The Lowerer still queries the
/// resolution map by absolute span, and those absolute spans match what
/// the current revision's AST produces — so lowering still works.
#[derive(Debug, Clone)]
pub struct ResolvedDecl {
    inner: Arc<ResolvedDeclInner>,
}

#[derive(Debug, Clone)]
struct ResolvedDeclInner {
    resolved: Option<ResolvedModule>,
    canonical: Arc<str>,
}

impl PartialEq for ResolvedDeclInner {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for ResolvedDeclInner {}

impl PartialEq for ResolvedDecl {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || *self.inner == *other.inner
    }
}

impl Eq for ResolvedDecl {}

impl ResolvedDecl {
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(ResolvedDeclInner {
                resolved: None,
                canonical: Arc::from(""),
            }),
        }
    }

    pub fn new(resolved: ResolvedModule, source: &str, span: glyph_ast::Span) -> Self {
        let bytes = canonical_bytes(source, span);
        Self {
            inner: Arc::new(ResolvedDeclInner {
                resolved: Some(resolved),
                canonical: bytes,
            }),
        }
    }

    /// `Some` if the file parsed and `decl_idx` was in range, else `None`.
    ///
    /// **Staleness contract** (same as `DeclAst::decl`): under a
    /// backdated revision, the carried `ResolvedModule`'s symbols and
    /// resolutions are keyed by the spans of the REVISION WHERE THIS
    /// WAS LAST RE-EXECUTED. Consumers pairing this with a freshly-
    /// built per-revision `Decl` (with current-revision spans) will
    /// see lookups miss. Today's consumer (`decl_ty`'s Lowerer) reads
    /// only this resolved-module and a `Decl` from the
    /// lockstep-backdated `DeclAst`, so the keys match.
    pub fn resolved(&self) -> Option<&ResolvedModule> {
        self.inner.resolved.as_ref()
    }
}

impl_wrapper_update!(ResolvedDecl);

/// Extract the source-byte slice for a span. Used as the canonical
/// fingerprint for `DeclAst` and `ResolvedDecl`. Falls back to an empty
/// string on malformed spans so PartialEq can't panic in release, but
/// `debug_assert!`s in test/debug builds — a malformed span here is a
/// parser bug, and silently producing an empty canonical would make
/// two mismatched malformed decls compare equal (Update returns false,
/// salsa serves a stale `Ty`).
fn canonical_bytes(source: &str, span: glyph_ast::Span) -> Arc<str> {
    let start = span.start as usize;
    let end = span.end as usize;
    let valid = end >= start
        && end <= source.len()
        && source.is_char_boundary(start)
        && source.is_char_boundary(end);
    debug_assert!(
        valid,
        "canonical_bytes: malformed span {span:?} for source of length {} — parser bug",
        source.len()
    );
    if !valid {
        return Arc::from("");
    }
    Arc::from(&source[start..end])
}

/// The outermost source span of a top-level declaration.
fn decl_outer_span(d: &Decl) -> glyph_ast::Span {
    match d {
        Decl::Import(i) => i.span,
        Decl::Fn(f) => f.span,
        Decl::Type(t) => t.span,
        Decl::Const(c) => c.span,
        Decl::Component(c) => c.span,
    }
}

// ============================================================================
// Stage 8 wrapper: Exports (per-file module exports)
// ============================================================================

/// The set of names a file exports — every top-level decl plus
/// tagged-union variants hoisted into module scope. Imports do NOT
/// re-export (D15: barrel files forbidden); names brought in via
/// `import M { N }` stay local to the importing module.
///
/// Salsa-backed via `module_exports(file)`. When fn 5's name doesn't
/// change across an edit (only its body shifts), the wrapper's
/// PartialEq returns true → Update returns false → consumers of this
/// file's exports (e.g., the ProjectGraph in another file's
/// `import_diagnostics`) skip re-execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exports {
    inner: Arc<ExportsInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportsInner {
    exports: ModuleExports,
}

impl Exports {
    pub fn new(exports: ModuleExports) -> Self {
        Self {
            inner: Arc::new(ExportsInner { exports }),
        }
    }

    pub fn empty() -> Self {
        Self::new(ModuleExports::empty())
    }

    pub fn exports(&self) -> &ModuleExports {
        &self.inner.exports
    }
}

impl_wrapper_update!(Exports);

// ============================================================================
// Stage 9 wrapper: ProjectExports (salsa-tracked aggregate)
// ============================================================================

/// Salsa-tracked aggregation of every project file's `module_exports`,
/// keyed by module path. Returned by the `project_exports(db, project)`
/// query. Implements `ModuleGraph`, so `verify_imports` can consume it
/// directly inside another tracked query like `import_diagnostics`.
///
/// Day-10 wiring: `import_diagnostics(db, file)` reads
/// `project_exports(db, db.project_files_input())` and composes it with
/// the static stdlib graph. When fn `helper` is removed from `lib.glyph`,
/// salsa re-runs `module_exports(lib)`, which produces a different export
/// set, which invalidates `project_exports`, which invalidates
/// `import_diagnostics(app.glyph)` — `app.glyph`'s diagnostics
/// auto-fire `UnknownExportedName` without the caller doing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectExports {
    inner: Arc<ProjectExportsInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectExportsInner {
    by_path: std::collections::BTreeMap<String, ModuleExports>,
}

impl ProjectExports {
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(ProjectExportsInner {
                by_path: std::collections::BTreeMap::new(),
            }),
        }
    }

    pub fn new(by_path: std::collections::BTreeMap<String, ModuleExports>) -> Self {
        Self {
            inner: Arc::new(ProjectExportsInner { by_path }),
        }
    }

    /// Number of registered modules.
    pub fn len(&self) -> usize {
        self.inner.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.by_path.is_empty()
    }
}

impl ModuleGraph for ProjectExports {
    fn exports_of(&self, path: &ModulePath) -> Option<&ModuleExports> {
        self.inner.by_path.get(&path_key(path))
    }
}

impl_wrapper_update!(ProjectExports);

// ============================================================================
// ProjectGraph — a ModuleGraph backed by salsa-cached file exports
// ============================================================================

/// In-memory aggregation of multiple files' `module_exports` results.
/// Construct via `ProjectGraph::build(db, [(module_path_str, SourceFile), ...])`
/// — the build iterates and salsa-fetches each file's exports.
///
/// **Day-10 note**: this is the *non-tracked* aggregator, kept for
/// callers that need a `ModuleGraph` to hand to `verify_imports`
/// directly (e.g. unit tests). For automatic cross-file invalidation,
/// register the project on the db via `CompilerDb::set_project(...)`
/// and call `import_diagnostics(db, file)` — that path uses the
/// salsa-tracked `ProjectExports` and `project_exports` query, which
/// invalidate dependent files' diagnostics automatically when any
/// file's export set changes.
///
/// `ProjectGraph` itself is not salsa-tracked; rebuilding it costs
/// O(N) HashMap inserts but the per-file `module_exports` work is
/// cached. Use when you need the `ModuleGraph` shape outside the salsa
/// query context.
#[derive(Debug, Default, Clone)]
pub struct ProjectGraph {
    by_path: HashMap<String, ModuleExports>,
}

impl ProjectGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a `ProjectGraph` from a set of `(module_path, SourceFile)`
    /// pairs. The `module_path` is the slash-joined path (`"std/array"`,
    /// `"app/users"`) that matches what `path_key` produces for an
    /// `import` statement's `ModulePath`.
    pub fn build<I, S>(db: &dyn Db, files: I) -> Self
    where
        I: IntoIterator<Item = (S, SourceFile)>,
        S: Into<String>,
    {
        let mut by_path: HashMap<String, ModuleExports> = HashMap::new();
        for (path, file) in files {
            let exports = module_exports(db, file);
            let key = path.into();
            let prev = by_path.insert(key.clone(), exports.exports().clone());
            debug_assert!(
                prev.is_none(),
                "ProjectGraph::build: duplicate module path `{key}` — earlier file's exports overwritten"
            );
        }
        Self { by_path }
    }

    /// Number of registered modules.
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

impl ModuleGraph for ProjectGraph {
    fn exports_of(&self, path: &ModulePath) -> Option<&ModuleExports> {
        self.by_path.get(&path_key(path))
    }
}

// ============================================================================
// Tracked queries
// ============================================================================

/// Parse the source file's text. Returns `ParsedModule::err(error)` on parse
/// failure; downstream queries gracefully degrade. The structured error
/// preserves the failing span — see `ParsedModule::error()`.
#[salsa::tracked]
pub fn parse_module(db: &dyn Db, file: SourceFile) -> ParsedModule {
    match glyph_parser::parse(file.text(db)) {
        Ok(m) => ParsedModule::ok(m),
        Err(e) => ParsedModule::err(e),
    }
}

/// Top-level symbol table for a file. Empty (`Symbols::err`) if parsing
/// failed or collection itself reported errors.
#[salsa::tracked]
pub fn module_symbols(db: &dyn Db, file: SourceFile) -> Symbols {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return Symbols::err(Vec::new());
    };
    match collect_module_symbols(module) {
        Ok(s) => Symbols::ok(s),
        Err(errs) => Symbols::err(errs),
    }
}

/// Per-file export surface. Returns every name a file makes visible to
/// importers — top-level decls (fn, type, const, component) plus
/// tagged-union variants hoisted into module scope. Imports are NOT
/// re-exported per D15.
///
/// Backs the `ProjectGraph` aggregation: editing a file's body re-runs
/// this query, but when the export set hasn't changed (most edits don't
/// add or remove top-level names) the wrapper's Update returns false and
/// downstream consumers of this file's exports skip re-validation.
#[salsa::tracked]
pub fn module_exports(db: &dyn Db, file: SourceFile) -> Exports {
    let syms = module_symbols(db, file);
    let Some(table) = syms.symbols() else {
        return Exports::empty();
    };
    // **Include-list, not exclude-list**: when a new `SymbolKind` variant
    // lands (e.g. `Macro`, `TypeAlias`), this `matches!` will reject it by
    // default and the compiler exhaustiveness check will force a
    // decision rather than silently leaking the new kind into the
    // export surface. Imports stay locally-bound per D15 (no barrel files).
    let names: std::collections::BTreeSet<Ident> = table
        .by_name
        .iter()
        .filter(|(_, id)| {
            let sym = table
                .table
                .get(**id)
                .expect("by_name points at table entry");
            matches!(
                sym.kind,
                SymbolKind::Function { .. }
                    | SymbolKind::Type { .. }
                    | SymbolKind::Const { .. }
                    | SymbolKind::Component { .. }
                    | SymbolKind::Variant { .. }
            )
        })
        .map(|(name, _)| name.clone())
        .collect();
    Exports::new(ModuleExports::from_names(names))
}

/// Aggregate every project file's `module_exports` into a single
/// salsa-tracked `ProjectExports` value. Re-runs whenever any registered
/// file's exports change OR when the project file list itself changes.
/// When the new aggregate is content-equal to the cached one (the common
/// case: a body edit that doesn't add or remove a top-level name), the
/// wrapper's Update returns false and downstream consumers
/// (`import_diagnostics`) backdate.
#[salsa::tracked]
pub fn project_exports(db: &dyn Db, project: ProjectFiles) -> ProjectExports {
    let mut by_path = std::collections::BTreeMap::<String, ModuleExports>::new();
    for (path, file) in project.entries(db).iter() {
        let exports = module_exports(db, *file);
        let prev = by_path.insert(path.clone(), exports.exports().clone());
        debug_assert!(
            prev.is_none(),
            "project_exports: duplicate module path `{path}` in ProjectFiles \
             entries — earlier file's exports silently overwritten. Match \
             the safety contract of `ProjectGraph::build`."
        );
    }
    ProjectExports::new(by_path)
}

/// Cross-module verification (`import M { N }` checks). Composes the
/// static stdlib `module_graph` with the salsa-tracked
/// `project_exports`, so editing a project file's exports auto-invalidates
/// the dependent file's diagnostics — no manual graph rebuild needed.
#[salsa::tracked]
pub fn import_diagnostics(db: &dyn Db, file: SourceFile) -> Diagnostics {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return Diagnostics::new(Vec::new());
    };
    let project = project_exports(db, db.project_files_input());
    let composite = glyph_resolver::CompositeGraph {
        first: db.module_graph(),
        second: &project,
    };
    let errs = verify_imports(module, &composite);
    Diagnostics::new(errs)
}

/// Intra-module name resolution. Returns a `Resolved::skipped(...)` placeholder
/// if upstream stages failed; otherwise the `ResolvedModule` plus any
/// resolution errors.
#[salsa::tracked]
pub fn resolve(db: &dyn Db, file: SourceFile) -> Resolved {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return Resolved::skipped(Vec::new());
    };
    let symbols = module_symbols(db, file);
    let Some(syms) = symbols.symbols() else {
        return Resolved::skipped(symbols.errors().to_vec());
    };
    let (resolved, errors) = resolve_module(module, syms.clone(), db.prelude());
    Resolved::new(resolved, errors)
}

/// Assign a `Ty` to every expression node in the file. Empty `TypeMap` if
/// upstream stages failed.
///
/// Routes per-decl signature lowering through the salsa-tracked `decl_ty`
/// query via `SalsaDeclTy`. The Assigner's old in-call HashMap cache is
/// gone; calls to `decl_ty_resolver.decl_ty(idx)` hit the cross-revision
/// memo on `crate::decl_ty(db, file, idx)`. Editing any line in the file
/// still invalidates `parse_module` and `resolve`, so `decl_ty(file, k)`
/// re-executes for every k — but for untouched fns each re-execution
/// returns a structurally-equal `Ty`, the wrapper's Update returns false,
/// and salsa backdates the revision so downstream consumers of those
/// `decl_ty` entries skip. The day-7 win is the cache-sharing across the
/// `type_map` ↔ `decl_ty` boundary, not sub-decl input granularity (that's
/// week 2 day 8+).
#[salsa::tracked]
pub fn type_map(db: &dyn Db, file: SourceFile) -> Types {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return Types::empty();
    };
    let resolved = resolve(db, file);
    let Some(resolved_module) = resolved.resolved() else {
        return Types::empty();
    };
    let decl_ty_resolver = SalsaDeclTy { db, file };
    let (tm, ty_errs) = assign_types_with_resolver(
        module,
        resolved_module,
        db.prelude(),
        &decl_ty_resolver,
    );
    Types::new(tm, ty_errs)
}

/// `DeclTyResolver` impl that fetches per-decl types from the salsa-tracked
/// `decl_ty(db, file, idx)` query. Lives at the `glyph-db` ↔ `glyph-typechecker`
/// boundary so the typechecker stays unaware of salsa.
struct SalsaDeclTy<'a> {
    db: &'a dyn Db,
    file: SourceFile,
}

impl DeclTyResolver for SalsaDeclTy<'_> {
    fn decl_ty(&self, decl_idx: u32) -> Ty {
        decl_ty(self.db, self.file, decl_idx).ty().clone()
    }
}

/// Extract the `decl_idx`-th top-level declaration from the parsed
/// module. The salsa memo gets backdated whenever the decl's source
/// bytes are unchanged across a file edit — even when absolute byte
/// positions shifted because of edits to other decls.
#[salsa::tracked]
pub fn decl_ast(db: &dyn Db, file: SourceFile, decl_idx: u32) -> DeclAst {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return DeclAst::empty();
    };
    let Some(decl) = module.items.get(decl_idx as usize) else {
        return DeclAst::empty();
    };
    let span = decl_outer_span(decl);
    let source = file.text(db);
    DeclAst::new(decl.clone(), source, span)
}

/// Per-declaration slice of the resolver output. Contains a
/// `ResolvedModule` whose `resolutions` map is restricted to spans
/// inside `module.items[decl_idx]`'s signature (param types + return
/// type). The wrapper's PartialEq compares the decl's source bytes only
/// (same canonical as `DeclAst`); so when fn 0's body changes length,
/// fn 1's `ResolvedDecl` is still considered equal — the carried
/// `ResolvedModule` has different absolute spans but the bytes that
/// produced it are unchanged.
#[salsa::tracked]
pub fn resolved_decl(db: &dyn Db, file: SourceFile, decl_idx: u32) -> ResolvedDecl {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return ResolvedDecl::empty();
    };
    let Some(decl) = module.items.get(decl_idx as usize) else {
        return ResolvedDecl::empty();
    };
    let resolved = resolve(db, file);
    let Some(full) = resolved.resolved() else {
        return ResolvedDecl::empty();
    };
    let mut sig_spans: HashSet<(u32, u32)> = HashSet::new();
    collect_signature_spans(decl, &mut sig_spans);
    let sliced = full.sliced(|s| sig_spans.contains(&(s.start, s.end)));
    let span = decl_outer_span(decl);
    let source = file.text(db);
    ResolvedDecl::new(sliced, source, span)
}

/// Lower the type of the `decl_idx`-th top-level declaration.
///
/// True per-decl input granularity (day 8): depends only on
/// `decl_ast(file, decl_idx)` and `resolved_decl(file, decl_idx)`, not on
/// the whole-file `parse_module`/`resolve` outputs. Editing fn 5's body
/// makes `decl_ast(file, 5)` and `resolved_decl(file, 5)` change, but for
/// k≠5 (with stable byte positions) both per-decl slices stay
/// content-equal — Update returns false, and `decl_ty(file, k)` skips
/// re-execution entirely (zero `WillExecute` in salsa's event log).
///
/// Out-of-range or non-callable decls return `DeclTy::new(Ty::Unknown)`.
#[salsa::tracked]
pub fn decl_ty(db: &dyn Db, file: SourceFile, decl_idx: u32) -> DeclTy {
    let ast = decl_ast(db, file, decl_idx);
    let Some(decl) = ast.decl() else {
        return DeclTy::new(Ty::Unknown);
    };
    let rd = resolved_decl(db, file, decl_idx);
    let Some(resolved_module) = rd.resolved() else {
        return DeclTy::new(Ty::Unknown);
    };
    let lowerer = Lowerer::new(resolved_module, db.prelude());
    DeclTy::new(lowerer.lower_decl_signature(decl))
}

/// Collect every span the `Lowerer` will query from the resolution map
/// while lowering `decl`'s signature: every `TypeExpr::Path` span inside
/// param types and the return type. Used by `resolved_decl` to slice the
/// full per-file resolution map down to per-decl scope.
fn collect_signature_spans(decl: &Decl, out: &mut HashSet<(u32, u32)>) {
    let (params, return_ty) = match decl {
        Decl::Fn(f) => (f.params.as_slice(), f.return_ty.as_ref()),
        Decl::Component(c) => (c.params.as_slice(), c.return_ty.as_ref()),
        // Non-callable decls have no signature spans to collect; the
        // matching `decl_ty` arm returns `Ty::Unknown` and the Lowerer is
        // never invoked, so the slice doesn't matter.
        Decl::Import(_) | Decl::Type(_) | Decl::Const(_) => return,
    };
    for p in params {
        collect_type_expr_spans(&p.ty, out);
    }
    if let Some(rt) = return_ty {
        collect_type_expr_spans(rt, out);
    }
}

fn collect_type_expr_spans(te: &TypeExpr, out: &mut HashSet<(u32, u32)>) {
    match te {
        TypeExpr::Path { span, .. } => {
            out.insert((span.start, span.end));
        }
        TypeExpr::Generic { base, args, .. } => {
            collect_type_expr_spans(base, out);
            for a in args {
                collect_type_expr_spans(a, out);
            }
        }
        TypeExpr::Fn {
            params, return_ty, ..
        } => {
            for p in params {
                collect_type_expr_spans(&p.ty, out);
            }
            if let Some(rt) = return_ty.as_deref() {
                collect_type_expr_spans(rt, out);
            }
        }
        TypeExpr::Record { fields, .. } => {
            for f in fields {
                collect_type_expr_spans(&f.ty, out);
            }
        }
        TypeExpr::Union { variants, .. } => {
            for v in variants {
                if let Some(p) = &v.payload {
                    collect_type_expr_spans(p, out);
                }
            }
        }
        // Raw TypeScript carries no Glyph name spans to collect.
        TypeExpr::Extern { .. } => {}
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use glyph_ast::{Decl, Expr, Span, Stmt};
    use glyph_typechecker::{Primitive, Ty};
    use std::sync::Mutex;

    /// Test-only log of salsa event kinds. Cloning the wrapper shares the
    /// underlying Vec so the callback inside `CompilerDb::new` can write
    /// while the test reads.
    #[derive(Default, Clone)]
    pub struct EventLog {
        inner: Arc<Mutex<Vec<salsa::EventKind>>>,
    }

    impl EventLog {
        pub fn record(&self, kind: salsa::EventKind) {
            self.inner.lock().unwrap().push(kind);
        }

        pub fn drain(&self) -> Vec<salsa::EventKind> {
            std::mem::take(&mut *self.inner.lock().unwrap())
        }
    }

    impl CompilerDb {
        /// Drain and return all salsa events recorded since the last drain.
        /// Test-only.
        pub fn drain_events(&self) -> Vec<salsa::EventKind> {
            self.events.drain()
        }
    }

    fn new_file(db: &CompilerDb, name: &str, text: &str) -> SourceFile {
        SourceFile::new(db, name.to_string(), text.to_string())
    }

    /// Count all `WillExecute` events in a salsa event log. salsa's
    /// `DatabaseKeyIndex` Debug renders as numeric ingredient/value ids
    /// (`IngredientIndex(N), Id(M)`), so we don't try to filter by query
    /// name — the test instead drains events between phases and asserts a
    /// zero-execution second phase.
    fn count_will_execute(events: &[salsa::EventKind]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, salsa::EventKind::WillExecute { .. }))
            .count()
    }

    /// Count `DidValidateMemoizedValue` events. Salsa fires this when a
    /// query's memo is checked across a revision and found to be still
    /// valid — i.e., the body did NOT re-execute. Used by day-8 tests to
    /// confirm decl_ty was a memo hit even when its (cheaper) sibling
    /// slicing queries fired WillExecute to re-validate themselves.
    fn count_validated_memos(events: &[salsa::EventKind]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, salsa::EventKind::DidValidateMemoizedValue { .. }))
            .count()
    }

    #[test]
    fn parse_query_returns_module_for_valid_source() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "ok.glyph", "module x\nfn main() {}\n");
        let parsed = parse_module(&db, file);
        assert!(parsed.module().is_some());
        assert!(parsed.error().is_none());
    }

    #[test]
    fn parse_query_returns_error_for_invalid_source() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "bad.glyph", "module x\nfn main(\n");
        let parsed = parse_module(&db, file);
        assert!(parsed.module().is_none());
        assert!(parsed.error().is_some());
    }

    #[test]
    fn module_symbols_query_finds_top_level_fn() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "ok.glyph", "module x\nfn helper() {}\n");
        let syms = module_symbols(&db, file);
        let table = syms.symbols().expect("collect should succeed");
        assert!(table.lookup("helper").is_some());
    }

    #[test]
    fn import_diagnostics_flag_unknown_export() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "bad.glyph",
            "module x\nimport std/result { Result, Bogus }\n",
        );
        let diags = import_diagnostics(&db, file);
        assert_eq!(diags.errors().len(), 1, "errors: {:?}", diags.errors());
        assert!(matches!(
            &diags.errors()[0],
            ResolveError::UnknownExportedName { name, module, .. }
                if name == "Bogus" && module == "std/result"
        ));
    }

    #[test]
    fn import_diagnostics_empty_when_all_imports_resolve() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "ok.glyph",
            "module x\nimport std/result { Result, Ok, Err }\nimport std/array\n",
        );
        let diags = import_diagnostics(&db, file);
        assert!(diags.errors().is_empty(), "got: {:?}", diags.errors());
    }

    #[test]
    fn resolve_query_handles_basic_module() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "ok.glyph",
            "module x\nfn id(a: number) -> number { return a }\n",
        );
        let r = resolve(&db, file);
        assert!(r.resolved().is_some());
        assert!(r.errors().is_empty(), "errors: {:?}", r.errors());
    }

    #[test]
    fn type_map_query_assigns_concrete_type_to_literal() {
        let db = CompilerDb::with_default_stdlib();
        let src = "module x\nfn main() { let x = 42 }\n";
        let file = new_file(&db, "ok.glyph", src);
        let types = type_map(&db, file);
        assert!(!types.type_map().is_empty());
        // Find the `42` literal's span by walking the parsed AST and assert
        // its type entry is Ty::Prim(Number) — not just "has an entry".
        let parsed = parse_module(&db, file);
        let module = parsed.module().expect("parse should succeed");
        let span = literal_42_span(module).expect("AST should contain `42` literal");
        assert!(
            matches!(types.type_map().get(span), Ty::Prim(Primitive::Number)),
            "got {:?}",
            types.type_map().get(span)
        );
    }

    /// Locate the span of the `42` literal in `fn main() { let x = 42 }`.
    fn literal_42_span(m: &glyph_ast::Module) -> Option<Span> {
        let Decl::Fn(f) = &m.items[0] else { return None };
        let Stmt::Let(l) = &f.body.stmts[0] else { return None };
        match &l.value {
            Expr::Number { span, .. } => Some(*span),
            _ => None,
        }
    }

    #[test]
    fn downstream_queries_short_circuit_on_parse_error() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "bad.glyph", "module x\nfn main(\n");
        let parsed = parse_module(&db, file);
        let syms = module_symbols(&db, file);
        let r = resolve(&db, file);
        let tm = type_map(&db, file);
        // The structured error survives — consumers can render via Display.
        assert!(parsed.error().is_some());
        assert!(syms.symbols().is_none());
        assert!(r.resolved().is_none());
        assert!(tm.type_map().is_empty());
    }

    #[test]
    fn changing_text_invalidates_downstream() {
        let mut db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "a.glyph", "module x\nfn main() {}\n");
        // Snapshot pre-change values from every stage of the pipeline.
        let items_before = parse_module(&db, file).module().unwrap().items.len();
        let symbols_before = module_symbols(&db, file)
            .symbols()
            .map(|s| s.table.len())
            .unwrap();
        let typed_entries_before = type_map(&db, file).type_map().len();
        // Mutate the input.
        file.set_text(&mut db).to(
            "module x\nfn main() {}\nfn helper(x: number) -> number { return x }\n".to_string(),
        );
        // Each downstream stage observes the new AST — not just parse.
        let items_after = parse_module(&db, file).module().unwrap().items.len();
        let symbols_after = module_symbols(&db, file)
            .symbols()
            .map(|s| s.table.len())
            .unwrap();
        let typed_entries_after = type_map(&db, file).type_map().len();
        assert_eq!(items_before, 1);
        assert_eq!(items_after, 2);
        assert!(
            symbols_after > symbols_before,
            "{symbols_before} → {symbols_after}"
        );
        assert!(
            typed_entries_after > typed_entries_before,
            "{typed_entries_before} → {typed_entries_after}"
        );
    }

    #[test]
    fn unchanged_text_returns_same_result() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "a.glyph", "module x\nfn main() {}\n");
        let first = parse_module(&db, file);
        let second = parse_module(&db, file);
        // Salsa memoizes on unchanged input: the second call returns a clone
        // of the cached `ParsedModule`, whose `inner: Arc<_>` is pointer-equal
        // to the first. If salsa had re-executed `parse_module`, the body
        // would have called `Arc::new(...)` and produced a fresh allocation
        // with a different address — `ptr_eq` would fail.
        assert!(Arc::ptr_eq(&first.inner, &second.inner));
    }

    #[test]
    fn other_file_results_survive_unrelated_text_change() {
        // I4's central promise: mutating file A's text does not invalidate
        // file B's queries.
        let mut db = CompilerDb::with_default_stdlib();
        let a = new_file(&db, "a.glyph", "module a\nfn main() {}\n");
        let b = new_file(&db, "b.glyph", "module b\nfn helper() {}\n");
        let b_first = parse_module(&db, b);
        // Touch file A.
        a.set_text(&mut db).to(
            "module a\nfn main() {}\nfn extra() {}\n".to_string(),
        );
        let b_second = parse_module(&db, b);
        // File B's parsed AST should still be the same Arc — salsa didn't
        // recompute it just because file A changed.
        assert!(Arc::ptr_eq(&b_first.inner, &b_second.inner));
    }

    #[test]
    fn decl_ty_returns_lowered_fn_signature() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "ok.glyph",
            "module x\nfn add(a: number, b: number) -> number { return a + b }\n",
        );
        let d = decl_ty(&db, file, 0);
        match d.ty() {
            Ty::Fn {
                params, return_ty, ..
            } => {
                assert_eq!(params.len(), 2);
                assert!(matches!(params[0].ty, Ty::Prim(Primitive::Number)));
                assert!(matches!(params[1].ty, Ty::Prim(Primitive::Number)));
                assert!(matches!(&**return_ty, Ty::Prim(Primitive::Number)));
            }
            other => panic!("expected Ty::Fn, got {other:?}"),
        }
    }

    #[test]
    fn decl_ty_unknown_for_non_callable_decl() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "ok.glyph",
            "module x\ntype User = { name: string }\n",
        );
        // Type decls don't produce a Fn-shaped DeclTy in this query.
        let d = decl_ty(&db, file, 0);
        assert!(matches!(d.ty(), Ty::Unknown));
    }

    #[test]
    fn decl_ty_unknown_for_out_of_range_idx() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "ok.glyph", "module x\nfn one() {}\n");
        let d = decl_ty(&db, file, 99);
        assert!(matches!(d.ty(), Ty::Unknown));
    }

    #[test]
    fn decl_ty_memoizes_per_decl_index_within_a_revision() {
        // First-phase calls fill the cache; the second-phase repeat calls
        // must execute zero queries — salsa returns the memoized DeclTy for
        // each (file, decl_idx) key without re-running any body.
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "two.glyph",
            "module x\nfn add(a: number, b: number) -> number { return a + b }\nfn ident(x: string) -> string { return x }\n",
        );
        // Discard any events produced by `SourceFile::new` itself so the
        // phase-1 count reflects only the decl_ty fetches.
        db.drain_events();
        // Phase 1: prime the cache.
        let _ = decl_ty(&db, file, 0);
        let _ = decl_ty(&db, file, 1);
        let primed = db.drain_events();
        // The two decl_ty calls drag in parse_module, module_symbols, resolve,
        // plus decl_ty(0) and decl_ty(1) themselves — at least 4 WillExecute
        // events. `>= 4` lets the assertion absorb harmless additions (like
        // a future intermediate query) without going so loose that a
        // regression that silently skipped decl_ty would still pass.
        assert!(
            count_will_execute(&primed) >= 4,
            "phase-1 should run parse_module + module_symbols + resolve + decl_ty(0) + decl_ty(1); events: {primed:?}"
        );
        // Phase 2: repeat calls in the same revision. Should be cache hits.
        let _ = decl_ty(&db, file, 0);
        let _ = decl_ty(&db, file, 1);
        let _ = decl_ty(&db, file, 0);
        let repeats = db.drain_events();
        assert_eq!(
            count_will_execute(&repeats),
            0,
            "repeat calls should hit the per-decl memo; events: {repeats:?}"
        );
    }

    #[test]
    fn editing_one_fn_body_keeps_other_fn_decl_ty_content_equal() {
        // Day-6 acceptance: changing the body of fn #0 must produce a
        // *content-equal* DeclTy for fn #1 across the edit, so downstream
        // consumers that depend on decl_ty(file, 1) can be backdated by
        // salsa and skip re-execution. Salsa's backdating compares values
        // via Update::maybe_update — when our wrapper's PartialEq returns
        // true, downstream queries observe "no change."
        //
        // Note: salsa does NOT preserve memo Arc identity across re-execution
        // (see `function/backdate.rs`); it preserves the `changed_at`
        // revision instead. So Arc::ptr_eq does NOT hold here. The honest
        // verification is content equality plus the per-decl memo test
        // above, which exercises the same plumbing.
        let mut db = CompilerDb::with_default_stdlib();
        let src_before = r#"module x
fn add(a: number, b: number) -> number { return a + b }
fn ident(x: string) -> string { return x }
"#;
        let file = new_file(&db, "two.glyph", src_before);
        let add_before = decl_ty(&db, file, 0);
        let ident_before = decl_ty(&db, file, 1);
        let src_after = r#"module x
fn add(a: number, b: number) -> number { return b + a }
fn ident(x: string) -> string { return x }
"#;
        file.set_text(&mut db).to(src_after.to_string());
        let add_after = decl_ty(&db, file, 0);
        let ident_after = decl_ty(&db, file, 1);
        assert_eq!(add_before.ty(), add_after.ty());
        assert_eq!(ident_before.ty(), ident_after.ty());
    }

    #[test]
    fn changing_fn_signature_changes_its_decl_ty_content() {
        let mut db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "one.glyph",
            "module x\nfn id(a: number) -> number { return a }\n",
        );
        let before = decl_ty(&db, file, 0);
        // Change `a: number` to `a: string`.
        file.set_text(&mut db).to(
            "module x\nfn id(a: string) -> number { return a }\n".to_string(),
        );
        let after = decl_ty(&db, file, 0);
        assert_ne!(before.ty(), after.ty(), "signature change should change DeclTy");
        match after.ty() {
            Ty::Fn {
                params, return_ty, ..
            } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0].ty, Ty::Prim(Primitive::String)));
                assert!(matches!(&**return_ty, Ty::Prim(Primitive::Number)));
            }
            other => panic!("expected Ty::Fn, got {other:?}"),
        }
    }

    #[test]
    fn decl_ty_for_component_returns_fn_shape() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "comp.glyph",
            "module x\ncomponent Btn(label: string) -> Component { return <button></button> }\n",
        );
        let d = decl_ty(&db, file, 0);
        match d.ty() {
            Ty::Fn { params, .. } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0].ty, Ty::Prim(Primitive::String)));
            }
            other => panic!("expected Ty::Fn, got {other:?}"),
        }
    }

    #[test]
    fn decl_ty_unknown_for_const_decl() {
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(&db, "ok.glyph", "module x\nconst PI = 3.14\n");
        let d = decl_ty(&db, file, 0);
        assert!(matches!(d.ty(), Ty::Unknown));
    }

    #[test]
    fn type_map_consumes_decl_ty_so_body_edit_does_not_relower_other_fns() {
        // Day-7 acceptance: `type_map` now routes per-decl Ty lookups
        // through the salsa-tracked `decl_ty` query. After editing one fn's
        // body, re-running `type_map` causes `decl_ty(file, k)` to re-execute
        // for every k (since parse_module/resolve are per-file inputs) — but
        // for k whose signature is unchanged, the new Ty is content-equal,
        // the wrapper's Update returns false, and salsa backdates the
        // revision counter. A later direct call to `decl_ty(file, k)` then
        // returns from the memo without re-running.
        //
        // Fixture: `use_helper` references `helper`, so type_map's walk
        // invokes `SalsaDeclTy::decl_ty(helper_idx)`. `helper` is not
        // referenced from anywhere, so `SalsaDeclTy::decl_ty(use_helper_idx)`
        // is NOT invoked by type_map — `decl_ty(file, 1)` enters the memo
        // only when called directly.
        let mut db = CompilerDb::with_default_stdlib();
        // Edit `helper`'s body `a + 1` → `1 + a`. Same length, same set of
        // expression spans — every TypeMap key is preserved across the edit
        // and every Ty value (`Number` for both `a` and `1`, `Unknown` for
        // the binary op) is unchanged. `use_helper`'s body is untouched.
        let src_before = r#"module x
fn helper(a: number) -> number { return a + 1 }
fn use_helper(x: number) -> number { return helper(x) }
"#;
        let file = new_file(&db, "calls.glyph", src_before);
        let before = type_map(&db, file);
        let src_after = r#"module x
fn helper(a: number) -> number { return 1 + a }
fn use_helper(x: number) -> number { return helper(x) }
"#;
        file.set_text(&mut db).to(src_after.to_string());
        // Drain events so the next assertion measures only post-edit work.
        db.drain_events();
        let after = type_map(&db, file);
        let post_edit_events = db.drain_events();
        // Full content equality — not just `len()`. `TypeMap: PartialEq + Eq`
        // (added in day 5) makes this a strict check. A bug that returned
        // wrong `Ty` values for any span would be caught here.
        assert_eq!(
            before.type_map(),
            after.type_map(),
            "type_map should be content-equal across a body-only edit",
        );
        // The post-edit type_map run executes parse_module, module_symbols,
        // resolve, decl_ty(0) [helper], decl_ty(1) is NOT visited by
        // type_map's walk because helper is unreferenced... wait — in this
        // fixture helper IS referenced by use_helper, so decl_ty(helper)
        // does fire during type_map's walk. So phase-1 count is roughly
        // parse + symbols + resolve + decl_ty(helper) + type_map = 5.
        // What we care about: it shouldn't be much more. A loose `<= 8`
        // upper bound lets future cheap intermediate queries land without
        // breaking the test, while still flagging a regression that re-ran
        // every decl in a large file.
        let post_edit_count = count_will_execute(&post_edit_events);
        assert!(
            post_edit_count <= 8,
            "post-edit type_map should re-execute a bounded set of queries; got {post_edit_count}, events: {post_edit_events:?}"
        );
        // After the post-edit run, calling decl_ty for the referenced helper
        // hits the memo (the resolver inside type_map already warmed it
        // AND salsa's backdating preserved its revision). Zero WillExecute.
        let _ = decl_ty(&db, file, 0);
        let helper_events = db.drain_events();
        assert_eq!(
            count_will_execute(&helper_events),
            0,
            "decl_ty(helper) should be a memo hit after the post-edit type_map; events: {helper_events:?}"
        );
        // No-op repeat type_map call also fires zero WillExecute — the full
        // chain (parse_module → resolve → decl_ty → type_map) is cached.
        let _ = type_map(&db, file);
        let repeat_events = db.drain_events();
        assert_eq!(
            count_will_execute(&repeat_events),
            0,
            "repeat type_map call should hit the memo end-to-end; events: {repeat_events:?}"
        );
    }

    #[test]
    fn type_map_warms_salsa_decl_ty_for_referenced_decls_and_only_those() {
        // Structural check that type_map's resolver routes through the
        // salsa-tracked `decl_ty` query — and only invokes it for decls that
        // are actually referenced from an expression. The asymmetric
        // invariant matters: it pins type_map's behavior at the per-ref
        // granularity. If a future refactor made type_map eager-warm every
        // decl, the unreferenced `main` would also be cached — a different
        // (potentially less correct) regime.
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "calls.glyph",
            "module x\nfn helper() -> number { return 1 }\nfn main() -> number { return helper() }\n",
        );
        // Pin the (idx, name) mapping the rest of the test relies on. If a
        // future contributor reorders the fixture, this assertion fails
        // loudly instead of silently inverting the test's meaning.
        let parsed = parse_module(&db, file);
        let module = parsed.module().expect("parse should succeed");
        assert!(
            matches!(&module.items[0], glyph_ast::Decl::Fn(f) if f.name.as_ref() == "helper"),
            "fixture invariant: decl_idx 0 must be `helper`"
        );
        assert!(
            matches!(&module.items[1], glyph_ast::Decl::Fn(f) if f.name.as_ref() == "main"),
            "fixture invariant: decl_idx 1 must be `main`"
        );

        let _ = type_map(&db, file);
        db.drain_events();
        // `helper` is referenced by `main` → its decl_ty memo should be warm.
        let _ = decl_ty(&db, file, 0);
        let helper_events = db.drain_events();
        assert_eq!(
            count_will_execute(&helper_events),
            0,
            "decl_ty(helper) should be a memo hit (referenced by main); events: {helper_events:?}"
        );
        // `main` is the entry point itself; nothing else references it →
        // its decl_ty memo should NOT have been warmed by type_map.
        let _ = decl_ty(&db, file, 1);
        let main_events = db.drain_events();
        assert!(
            count_will_execute(&main_events) >= 1,
            "decl_ty(main) should fire WillExecute (unreferenced — not warmed); events: {main_events:?}"
        );
    }

    use glyph_resolver::{verify_imports as verify_imports_fn, CompositeGraph};

    #[test]
    fn module_exports_lists_top_level_decls_only() {
        // Top-level fn, type, const, and component all become exports.
        // Imports do NOT (D15: no re-export).
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "lib.glyph",
            "module lib\nimport std/array\nfn helper() -> number { return 1 }\ntype User = { name: string }\nconst PI = 3.14\n",
        );
        let exports = module_exports(&db, file);
        let names = &exports.exports().names;
        assert!(names.contains("helper" as &str), "expected `helper`");
        assert!(names.contains("User" as &str), "expected `User`");
        assert!(names.contains("PI" as &str), "expected `PI`");
        // `array` was introduced by `import std/array` — it's a local
        // binding in `lib`, not an export FROM `lib`.
        assert!(!names.contains("array" as &str), "imports should not re-export");
    }

    #[test]
    fn module_exports_includes_union_variants() {
        // Tagged-union variants are hoisted into module scope as Variant
        // symbols (see glyph-resolver/collect.rs); they're part of the
        // module's exported surface.
        let db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "lib.glyph",
            "module lib\ntype FeedError = | NotFound | Timeout\n",
        );
        let exports = module_exports(&db, file);
        let names = &exports.exports().names;
        assert!(names.contains("FeedError" as &str));
        assert!(names.contains("NotFound" as &str));
        assert!(names.contains("Timeout" as &str));
    }

    #[test]
    fn project_graph_serves_cross_module_named_imports() {
        let db = CompilerDb::with_default_stdlib();
        let lib = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper() -> number { return 1 }\nfn other() -> number { return 2 }\n",
        );
        let app = new_file(&db, "app.glyph", "module app\nimport lib { helper }\n");
        let project = ProjectGraph::build(&db, [("lib", lib)]);
        let stdlib = StdlibStubs::new();
        let composite = CompositeGraph {
            first: &stdlib,
            second: &project,
        };
        let parsed = parse_module(&db, app);
        let errs = verify_imports_fn(parsed.module().expect("parse"), &composite);
        assert!(
            errs.is_empty(),
            "import lib {{ helper }} should resolve via the project graph; got: {errs:?}"
        );
    }

    #[test]
    fn project_graph_flags_unknown_export() {
        let db = CompilerDb::with_default_stdlib();
        let lib = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper() -> number { return 1 }\n",
        );
        let app = new_file(&db, "app.glyph", "module app\nimport lib { bogus }\n");
        let project = ProjectGraph::build(&db, [("lib", lib)]);
        let stdlib = StdlibStubs::new();
        let composite = CompositeGraph {
            first: &stdlib,
            second: &project,
        };
        let parsed = parse_module(&db, app);
        let errs = verify_imports_fn(parsed.module().expect("parse"), &composite);
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
        assert!(matches!(
            &errs[0],
            ResolveError::UnknownExportedName { name, module, .. }
                if name == "bogus" && module == "lib"
        ));
    }

    #[test]
    fn module_exports_memoizes_across_body_edits_that_dont_change_decl_names() {
        // The export *set* doesn't change when a fn body is edited (only
        // the names matter). After set_text, salsa walks the dep chain:
        //   - parse_module re-executes (text changed, output differs)
        //   - module_symbols re-executes (parse_module changed) but its
        //     SymbolTable is content-equal across the edit → backdated
        //   - module_exports sees its sole dep is backdated, validates
        //     WITHOUT re-executing its own body → DidValidateMemoizedValue
        //
        // The two assertions below pin this fire pattern: at most 2
        // WillExecute events (parse_module + module_symbols, NOT
        // module_exports), and at least 1 DidValidateMemoizedValue
        // (module_exports served from cache).
        let mut db = CompilerDb::with_default_stdlib();
        let file = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper(a: number) -> number { return a + 1 }\n",
        );
        let _ = module_exports(&db, file);
        // Body-only edit, same length so spans stay stable.
        file.set_text(&mut db).to(
            "module lib\nfn helper(a: number) -> number { return 1 + a }\n".to_string(),
        );
        db.drain_events();
        let _ = module_exports(&db, file);
        let events = db.drain_events();
        let we = count_will_execute(&events);
        let valid = count_validated_memos(&events);
        // `we <= 2` excludes module_exports from the re-executed set; a
        // regression where module_exports re-runs its body would yield
        // we=3 (parse_module + module_symbols + module_exports).
        assert!(
            we <= 2,
            "module_exports should not re-execute (only parse_module + module_symbols may); got we={we}, events: {events:?}"
        );
        // `valid >= 1` confirms a memo hit fired. Jointly with `we <= 2`,
        // this proves module_exports is the validated query (not just
        // module_symbols transiently). Do not drop either assertion.
        assert!(
            valid >= 1,
            "module_exports should be a memo hit; got valid={valid}, events: {events:?}"
        );
    }

    #[test]
    fn import_diagnostics_resolves_against_project_via_db_input() {
        // The salsa-wired path: register `lib` on the db, then call
        // import_diagnostics on `app` — no manual CompositeGraph/verify
        // call needed. `helper` is exported by lib; the import resolves.
        let mut db = CompilerDb::with_default_stdlib();
        let lib = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper() -> number { return 1 }\n",
        );
        let app = new_file(&db, "app.glyph", "module app\nimport lib { helper }\n");
        db.set_project(vec![("lib".to_string(), lib)]);
        let diags = import_diagnostics(&db, app);
        assert!(diags.errors().is_empty(), "errors: {:?}", diags.errors());
    }

    #[test]
    fn import_diagnostics_flags_unknown_project_export_via_db_input() {
        let mut db = CompilerDb::with_default_stdlib();
        let lib = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper() -> number { return 1 }\n",
        );
        let app = new_file(
            &db,
            "app.glyph",
            "module app\nimport lib { helper, bogus }\n",
        );
        db.set_project(vec![("lib".to_string(), lib)]);
        let diags = import_diagnostics(&db, app);
        assert!(matches!(
            diags.errors().iter().find(|e| matches!(
                e,
                ResolveError::UnknownExportedName { name, .. } if name == "bogus"
            )),
            Some(_)
        ), "expected UnknownExportedName(bogus); got: {:?}", diags.errors());
    }

    #[test]
    fn removing_a_lib_export_auto_invalidates_dependent_app_diagnostics() {
        // Day-10 acceptance: edit lib.glyph to remove `helper`, then
        // app.glyph's import_diagnostics auto-fires UnknownExportedName
        // without the caller doing anything (no graph rebuild, no
        // explicit invalidation). Salsa walks the chain
        // app.import_diagnostics → project_exports → module_exports(lib);
        // when module_exports(lib) changes, project_exports updates, and
        // app's diagnostics re-execute against the new export set.
        let mut db = CompilerDb::with_default_stdlib();
        let lib = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper() -> number { return 1 }\n",
        );
        let app = new_file(&db, "app.glyph", "module app\nimport lib { helper }\n");
        db.set_project(vec![("lib".to_string(), lib)]);
        let before = import_diagnostics(&db, app);
        assert!(before.errors().is_empty(), "before: {:?}", before.errors());

        // Remove `helper` from lib (replace with a differently-named fn).
        lib.set_text(&mut db).to(
            "module lib\nfn other() -> number { return 1 }\n".to_string(),
        );
        let after = import_diagnostics(&db, app);
        assert!(
            after.errors().iter().any(|e| matches!(
                e,
                ResolveError::UnknownExportedName { name, .. } if name == "helper"
            )),
            "expected UnknownExportedName(helper) after removal; got: {:?}",
            after.errors()
        );
    }

    #[test]
    fn body_only_edit_to_lib_does_not_invalidate_app_diagnostics() {
        // When only a body changes in lib, lib's exports don't change,
        // so module_exports(lib) backdates, so project_exports backdates,
        // so app's import_diagnostics is a memo hit. Verifies the
        // per-stage backdating actually fires across the file boundary.
        //
        // The chain salsa re-validates on this edit: parse_module(lib),
        // module_symbols(lib), module_exports(lib), project_exports —
        // four re-executions where the upstream value differs but the
        // downstream output is content-equal, so each backdates. Then
        // import_diagnostics(app)'s dep is fresh, so import_diagnostics
        // itself gets DidValidateMemoizedValue without re-executing.
        let mut db = CompilerDb::with_default_stdlib();
        let lib = new_file(
            &db,
            "lib.glyph",
            "module lib\nfn helper(a: number) -> number { return a + 1 }\n",
        );
        let app = new_file(&db, "app.glyph", "module app\nimport lib { helper }\n");
        db.set_project(vec![("lib".to_string(), lib)]);
        let _ = import_diagnostics(&db, app);
        // Same-length body swap, exports unchanged.
        lib.set_text(&mut db).to(
            "module lib\nfn helper(a: number) -> number { return 1 + a }\n".to_string(),
        );
        db.drain_events();
        let _ = import_diagnostics(&db, app);
        let events = db.drain_events();
        let we = count_will_execute(&events);
        let valid = count_validated_memos(&events);
        // **Jointly load-bearing**: `we <= 4` excludes
        // `import_diagnostics(app)` from the re-executed set (a
        // regression where it re-ran would push the count to ≥5).
        // `valid >= 1` confirms a memo hit fired — combined with the
        // bound, it must be import_diagnostics(app)'s memo hit. Without
        // the `we` bound, `valid >= 1` alone is satisfied by
        // module_exports/project_exports' backdating without import_
        // diagnostics actually being a hit.
        assert!(
            we <= 4,
            "post-edit chain should fire at most parse_module(lib) + \
             module_symbols(lib) + module_exports(lib) + project_exports \
             — NOT import_diagnostics(app); got we={we}, events: {events:?}"
        );
        assert!(
            valid >= 1,
            "expected ≥1 DidValidateMemoizedValue (import_diagnostics(app) \
             served from cache when lib's exports stable); events: {events:?}"
        );
    }

    #[test]
    fn length_changing_body_edit_skips_decl_ty_for_other_fns() {
        // Day-12 acceptance: the source-byte canonical for `DeclAst` and
        // `ResolvedDecl` makes their PartialEq immune to absolute-span
        // shifts. So a length-changing edit to fn 0's body (which shifts
        // every later decl's spans) no longer invalidates decl_ty for
        // those later decls — their canonical bytes are unchanged.
        //
        // Day 8's `editing_one_fn_body_skips_decl_ty_for_other_fns` used
        // an equal-length swap; this test exercises the genuinely
        // length-changing case that day-8's wrapper could not handle.
        let mut db = CompilerDb::with_default_stdlib();
        let src_before = r#"module x
fn helper(a: number) -> number { return a }
fn other(x: number) -> number { return x }
"#;
        let file = new_file(&db, "two.glyph", src_before);
        // Prime both decl_ty memos.
        let _ = decl_ty(&db, file, 0);
        let _ = decl_ty(&db, file, 1);
        db.drain_events();
        // Length-changing edit: `return a` → `return a + 1 + 2 + 3`.
        // The original was 8 chars in the body content; the new one is
        // 20 chars. `other`'s bytes shift later in the file accordingly.
        let src_after = r#"module x
fn helper(a: number) -> number { return a + 1 + 2 + 3 }
fn other(x: number) -> number { return x }
"#;
        file.set_text(&mut db).to(src_after.to_string());
        db.drain_events();
        // decl_ty for `other` (decl_idx 1) should still be served from
        // memo despite the absolute-span shift. The source bytes
        // covered by `other`'s outer span are byte-identical to before;
        // the canonical comparison detects that.
        let _ = decl_ty(&db, file, 1);
        let events = db.drain_events();
        let we = count_will_execute(&events);
        let valid = count_validated_memos(&events);
        // **Exact count rather than upper bound.** The chain
        // re-validates exactly five queries: parse_module, module_symbols,
        // resolve, decl_ast(other_idx), resolved_decl(other_idx). Each
        // runs to compute a new value; their Update impls check whether
        // the new value matches the cached canonical. The day-12 win:
        // decl_ast and resolved_decl produce content-equal canonicals
        // (source bytes unchanged for the untouched decl), so backdating
        // fires and decl_ty's deps are considered fresh — decl_ty's body
        // does NOT re-execute.
        //
        // Why exact rather than `we <= 5`: a future tracked query inserted
        // into the chain would push the count to 6 and a one-sided bound
        // would still pass (or, in a regression where decl_ty also runs,
        // count to 6 and would pass for the wrong reason). Pinning the
        // count to 5 forces a deliberate retabulation when the chain
        // changes — and confirms decl_ty (the 6th potential query in this
        // walk) stayed out of the re-executed set.
        assert_eq!(
            we, 5,
            "expected exactly 5 WillExecute (parse + symbols + resolve + decl_ast + resolved_decl, NOT decl_ty); events: {events:?}"
        );
        assert!(
            valid >= 1,
            "expected ≥1 DidValidateMemoizedValue (decl_ty served from memo across length-changing edit); got valid={valid}, events: {events:?}"
        );
    }

    #[test]
    fn editing_one_fn_body_skips_decl_ty_for_other_fns() {
        // Day-8 acceptance: with per-decl input slicing in place
        // (decl_ast + resolved_decl), editing fn 0's body must NOT cause
        // decl_ty(file, 1) to re-execute. The previous regime had decl_ty
        // re-running for every k on every edit and relying on output-level
        // backdating to spare downstream consumers. Day 8 moves the win
        // upstream: decl_ty(file, k≠edited) doesn't even run.
        //
        // Fixture is an equal-length body edit so neither decl's spans
        // shift across the change. Without that the DeclAst Eq (which
        // includes span values) would invalidate the unedited decl too.
        let mut db = CompilerDb::with_default_stdlib();
        let src_before = r#"module x
fn helper(a: number) -> number { return a + 1 }
fn use_helper(x: number) -> number { return helper(x) }
"#;
        let file = new_file(&db, "two.glyph", src_before);
        // Prime decl_ty for both decls.
        let _ = decl_ty(&db, file, 0);
        let _ = decl_ty(&db, file, 1);
        db.drain_events();
        // Same-length body edit to `helper` only — `use_helper`'s body and
        // signature are untouched and at the same byte positions.
        let src_after = r#"module x
fn helper(a: number) -> number { return 1 + a }
fn use_helper(x: number) -> number { return helper(x) }
"#;
        file.set_text(&mut db).to(src_after.to_string());
        // decl_ty for the EDITED fn should fire (its decl_ast changed).
        let _ = decl_ty(&db, file, 0);
        let edited_events = db.drain_events();
        assert!(
            count_will_execute(&edited_events) >= 1,
            "decl_ty(helper) should re-execute after its body changed; events: {edited_events:?}"
        );
        // decl_ty for the UNEDITED fn — the day-8 win. Salsa walks the
        // dep graph to verify freshness: decl_ast(file, 1) and
        // resolved_decl(file, 1) re-execute as part of validation (they
        // cheaply return content-equal values), but `decl_ty(file, 1)`'s
        // own body does NOT re-run — salsa serves it as a memo hit. The
        // tell is a `DidValidateMemoizedValue` event, not a `WillExecute`,
        // for decl_ty's ingredient.
        //
        // What this catches: a regression where decl_ty was wired back to
        // depend on parse_module/resolve directly would see decl_ty itself
        // re-execute (WillExecute on ingredient 5), and no
        // DidValidateMemoizedValue would fire for it.
        let _ = decl_ty(&db, file, 1);
        let unedited_events = db.drain_events();
        let we = count_will_execute(&unedited_events);
        let valid = count_validated_memos(&unedited_events);
        // The two assertions below are **jointly load-bearing**. The
        // documented regression — re-coupling decl_ty to parse_module/
        // resolve directly — yields we=1 (decl_ty alone re-executing)
        // and valid=0. The `we <= 2` bound admits we=1, so it does NOT
        // catch the regression on its own; `valid >= 1` is what rejects
        // it. Do not drop either assertion thinking the other suffices.
        //
        // The two re-validations counted under `we` are decl_ast(file, 1)
        // and resolved_decl(file, 1) — both cheap (clone-a-Decl and
        // filter-spans, respectively). Day-8's promise is that decl_ty's
        // body is NOT among the re-executed queries.
        assert!(
            we <= 2,
            "expected ≤2 WillExecute (decl_ast + resolved_decl validation); got {we} events: {unedited_events:?}"
        );
        assert!(
            valid >= 1,
            "expected ≥1 DidValidateMemoizedValue (decl_ty served from memo); got {valid} events: {unedited_events:?}"
        );
    }
}
