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

use glyph_ast::Module;
use glyph_parser::ParseError;
use glyph_resolver::{
    build_prelude, collect_module_symbols, resolve_module, verify_imports, ModuleGraph,
    ModuleSymbols, Prelude, ResolveError, ResolvedModule, StdlibStubs,
};
use glyph_typechecker::{assign_types, Lowerer, Ty, TypeMap};

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
}

/// Concrete database. Holds the salsa storage, a prelude, and a module
/// graph. Multiple `CompilerDb`s can coexist; they don't share state.
#[salsa::db]
#[derive(Clone)]
pub struct CompilerDb {
    storage: salsa::Storage<Self>,
    prelude: Arc<Prelude>,
    module_graph: Arc<dyn ModuleGraph + Send + Sync>,
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
        Self {
            storage,
            prelude: Arc::new(prelude),
            module_graph,
            #[cfg(test)]
            events,
        }
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
}

impl Types {
    pub fn new(type_map: TypeMap) -> Self {
        Self {
            inner: Arc::new(TypesInner { type_map }),
        }
    }

    pub fn type_map(&self) -> &TypeMap {
        &self.inner.type_map
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

/// Cross-module verification (`import M { N }` checks). Reads the module
/// graph off the database. Returns the list of `UnknownExportedName`
/// errors; empty on success.
#[salsa::tracked]
pub fn import_diagnostics(db: &dyn Db, file: SourceFile) -> Diagnostics {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return Diagnostics::new(Vec::new());
    };
    let errs = verify_imports(module, db.module_graph());
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
#[salsa::tracked]
pub fn type_map(db: &dyn Db, file: SourceFile) -> Types {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return Types::new(TypeMap::new());
    };
    let resolved = resolve(db, file);
    let Some(resolved_module) = resolved.resolved() else {
        return Types::new(TypeMap::new());
    };
    let tm = assign_types(module, resolved_module, db.prelude());
    Types::new(tm)
}

/// Lower the type of the `decl_idx`-th top-level declaration.
///
/// Per-decl granularity in salsa terms: any file edit invalidates
/// `parse_module(file)` and `resolve(file)`, both of which this query
/// depends on, so salsa re-executes `decl_ty(file, k)` for *every* `k`. The
/// win is at the *output* level — when only fn bodies changed, each
/// re-execution produces a structurally-equal `Ty`, the wrapper's
/// `Update::maybe_update` returns false, and salsa backdates the
/// `changed_at` revision so downstream consumers of `decl_ty(file, k)` for
/// untouched decls observe "no change" and skip. True per-decl input
/// granularity (re-executing only the touched `k`) would require slicing
/// `parse_module`'s output into per-decl sub-queries; see week 2 day 7+.
///
/// `decl_idx` is the index into `module.items`. Out-of-range or
/// non-callable decls return `DeclTy::new(Ty::Unknown)`.
#[salsa::tracked]
pub fn decl_ty(db: &dyn Db, file: SourceFile, decl_idx: u32) -> DeclTy {
    let parsed = parse_module(db, file);
    let Some(module) = parsed.module() else {
        return DeclTy::new(Ty::Unknown);
    };
    let resolved = resolve(db, file);
    let Some(resolved_module) = resolved.resolved() else {
        return DeclTy::new(Ty::Unknown);
    };
    let lowerer = Lowerer::new(resolved_module, db.prelude());
    let ty = module
        .items
        .get(decl_idx as usize)
        .map(|d| lowerer.lower_decl_signature(d))
        .unwrap_or(Ty::Unknown);
    DeclTy::new(ty)
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
}
