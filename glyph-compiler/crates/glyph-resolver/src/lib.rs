//! Glyph resolver — Phase 1 week 2.
//!
//! Two responsibilities:
//! - Build a `SymbolTable` of every top-level declaration and import in a
//!   `Module`.
//! - Walk every expression and pattern, mapping each identifier reference to
//!   either a local binding, a module-level symbol, an imported symbol, or a
//!   prelude built-in. Unresolved names produce structured `ResolveError`s.
//!
//! Salsa-backed per Q5 hybrid: the day-3+ slice wraps `collect_module_symbols`
//! and `resolve_module` as tracked queries (I4 — per-file inputs,
//! per-declaration intermediates). Until then both are pure functions of the
//! AST; the API is salsa-shaped already.
//!
//! Implements (Phase 1 week 2):
//! - D15 three import forms; no relative imports (barrel-file and re-export
//!   detection arrive in week 2 day 3+ once the module graph spans files)
//! - D19 `component` declarations resolved like `fn` declarations
//! - D20 `const` module-level, `let` function-level (parser enforces;
//!   resolver trusts the grammar)
//! - D4  `fn` declaration vs anonymous expression

#![forbid(unsafe_code)]

mod collect;
mod error;
mod module_graph;
mod prelude;
mod resolve;
mod symbol;

pub use collect::{collect_module_symbols, ModuleSymbols};
pub use error::ResolveError;
pub use module_graph::{
    verify_imports, CompositeGraph, ModuleExports, ModuleGraph, StdlibStubs,
};
pub use prelude::{build_prelude, Prelude};
pub use resolve::{resolve_module, ResolvedModule, ResolvedRef};
pub use symbol::{PreludeKind, Symbol, SymbolId, SymbolKind, SymbolTable};

#[cfg(test)]
mod smoke {
    #[test]
    fn library_exports_compile() {
        let _ = super::build_prelude();
    }
}
