//! The Glyph prelude — names visible in every module without an import.
//!
//! The prelude is a fixed table this slice; once stdlib lands (Phase 1 week 5
//! per the implementation plan), the prelude shrinks to a curated re-export
//! of `std/result`, `std/option`, primitives, and `par`. Until then this
//! module owns the canonical list.
//!
//! Contents (Phase 1 week 2 slice 1):
//! - Primitive types: `string`, `number`, `bool`, `void`, `unknown`
//! - Generic container types: `Result`, `Option`, `Array`, `Record`, `Schema`,
//!   `Component`
//! - Value constructors: `Ok`, `Err`, `Some`, `None`
//! - Namespace: `par` (used as `par.all`, `par.all_ok`)
//! - Built-in: `print`
//!
//! The list is **not** the v1 final stdlib; it's the minimum set the four
//! example files reference. Anything missing here surfaces as an unresolved
//! name during week-2 acceptance — useful signal for what stdlib needs.

use std::collections::HashMap;

use glyph_ast::Ident;

use crate::symbol::{prelude_symbol, PreludeKind, SymbolId, SymbolTable};

/// All prelude names → ids. Returned by `build_prelude()`; embedded as a
/// fallback scope during resolution (see `resolve.rs`).
#[derive(Debug, Clone)]
pub struct Prelude {
    pub table: SymbolTable,
    pub by_name: HashMap<Ident, SymbolId>,
}

impl Prelude {
    pub fn lookup(&self, name: &str) -> Option<SymbolId> {
        self.by_name.get(name).copied()
    }
}

/// Construct the prelude.
///
/// The order matters only for stable `SymbolId` allocation across builds; we
/// keep types first, then values, then namespaces. Tests in `tests/` may rely
/// on this ordering for fixture stability.
pub fn build_prelude() -> Prelude {
    let entries: &[(&str, PreludeKind)] = &[
        // Primitive types
        ("string", PreludeKind::String),
        ("number", PreludeKind::Number),
        ("bool", PreludeKind::Bool),
        ("void", PreludeKind::Void),
        ("unknown", PreludeKind::UnknownTop),
        // Generic container types
        ("Result", PreludeKind::Result),
        ("Option", PreludeKind::Option),
        ("Array", PreludeKind::Array),
        ("Record", PreludeKind::Record),
        ("Schema", PreludeKind::Schema),
        ("Component", PreludeKind::Component),
        ("Issue", PreludeKind::Issue),
        // Type-level operators (D28)
        ("infer_shape", PreludeKind::InferShape),
        // Value constructors
        ("Ok", PreludeKind::Ok),
        ("Err", PreludeKind::Err),
        ("Some", PreludeKind::Some),
        ("None", PreludeKind::None),
        // Namespace
        ("par", PreludeKind::Par),
        // Built-in
        ("print", PreludeKind::Print),
        ("assert", PreludeKind::Assert),
    ];

    let mut table = SymbolTable::new();
    let mut by_name = HashMap::new();
    for (name, kind) in entries {
        let id = table.intern(prelude_symbol(name, *kind));
        by_name.insert(table.get(id).unwrap().name.clone(), id);
    }

    Prelude { table, by_name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolKind;

    #[test]
    fn primitive_types_present() {
        let p = build_prelude();
        for name in ["string", "number", "bool", "void", "unknown"] {
            assert!(p.lookup(name).is_some(), "missing prelude type: {name}");
        }
    }

    #[test]
    fn result_and_option_present() {
        let p = build_prelude();
        for name in ["Result", "Option", "Ok", "Err", "Some", "None"] {
            assert!(p.lookup(name).is_some(), "missing prelude name: {name}");
        }
    }

    #[test]
    fn ambient_container_types_present() {
        // Regression for BUG-3: `Issue` is documented as an ambient prelude
        // type (json.parse's Err arm is `Array<Issue>`) and must resolve with
        // no import, alongside the other ambient container types.
        let p = build_prelude();
        for name in ["Array", "Record", "Schema", "Component", "Issue"] {
            assert!(p.lookup(name).is_some(), "missing ambient prelude type: {name}");
        }
    }

    #[test]
    fn prelude_symbols_have_correct_kind() {
        let p = build_prelude();
        let ok_id = p.lookup("Ok").unwrap();
        match p.table.get(ok_id).unwrap().kind {
            SymbolKind::Prelude { kind } => assert_eq!(kind, PreludeKind::Ok),
            _ => panic!("Ok should be a Prelude symbol"),
        }
    }
}
