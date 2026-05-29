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
use glyph_typechecker::{assign_types, TypeMap};

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
        Self {
            storage: salsa::Storage::default(),
            prelude: Arc::new(prelude),
            module_graph,
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

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_ast::{Decl, Expr, Span, Stmt};
    use glyph_typechecker::{Primitive, Ty};

    fn new_file(db: &CompilerDb, name: &str, text: &str) -> SourceFile {
        SourceFile::new(db, name.to_string(), text.to_string())
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
}
