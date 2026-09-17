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
        ("int", PreludeKind::Int),
        ("bigint", PreludeKind::BigInt),
        ("bool", PreludeKind::Bool),
        ("void", PreludeKind::Void),
        ("unknown", PreludeKind::UnknownTop),
        ("never", PreludeKind::Never),
        // Generic container types
        ("Result", PreludeKind::Result),
        ("Option", PreludeKind::Option),
        ("Nullable", PreludeKind::Nullable),
        ("Array", PreludeKind::Array),
        ("Record", PreludeKind::Record),
        ("Schema", PreludeKind::Schema),
        ("Component", PreludeKind::Component),
        ("Issue", PreludeKind::Issue),
        // Type-level operators (D28)
        ("infer_output", PreludeKind::InferOutput),
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

/// The stdlib module a prelude name is re-exported from, or `None` for a
/// prelude name no stdlib module declares.
///
/// The prelude is a curated re-export, not a second declaration: `Result` here
/// and `Result` in `std/result` are one type. The emitter proves it, writing
/// `import { Ok, type Result } from "./.glyph-runtime/std/result"` for a
/// program that never imported anything, and `import std/result { Result }`
/// resolves to the same declaration today. So a prelude name that a stdlib
/// module declares has an identity already, `std/result::Result`, and this is
/// the table that says which one (G231).
///
/// The names that answer `None` are the residue: `Array`, `Record`, `Schema`,
/// `Component`, `Issue`, `par`, `print`, `assert`, `infer_output` and the
/// primitives. No stdlib module declares any of them, so there is no module to
/// key them under and inventing a `std/prelude` for them would be inventing an
/// address rather than reporting one.
pub fn prelude_declaring_module(name: &str) -> Option<&'static str> {
    Some(match name {
        "Result" | "Ok" | "Err" => "std/result",
        "Option" | "Some" | "None" => "std/option",
        "Nullable" => "std/nullable",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolKind;

    #[test]
    fn primitive_types_present() {
        let p = build_prelude();
        for name in ["string", "number", "bool", "void", "unknown", "never"] {
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

    #[test]
    fn nullable_is_a_prelude_type_constructor_distinct_from_option() {
        // D45: `Nullable<T>` is its own prelude name with its own symbol, so a
        // `Ty::Named` built from it can never compare equal to `Option`'s.
        let p = build_prelude();
        let id = p.lookup("Nullable").expect("missing prelude type: Nullable");
        match p.table.get(id).unwrap().kind {
            SymbolKind::Prelude { kind } => assert_eq!(kind, PreludeKind::Nullable),
            _ => panic!("Nullable should be a Prelude symbol"),
        }
        assert_ne!(id, p.lookup("Option").unwrap());
    }
}

#[cfg(test)]
mod reexport_tests {
    use super::*;
    use crate::module_graph::{ModuleGraph, StdlibStubs};
    use glyph_ast::{ModulePath, Span};

    /// Every module this table names must actually export the name, or the
    /// identity it hands out addresses nothing.
    #[test]
    fn every_keyed_prelude_name_is_exported_by_the_module_it_names() {
        let stubs = StdlibStubs::new();
        for name in ["Result", "Ok", "Err", "Option", "Some", "None", "Nullable"] {
            let module = prelude_declaring_module(name)
                .unwrap_or_else(|| panic!("`{name}` has no declaring module"));
            let path = ModulePath {
                segments: module
                    .split('/')
                    .map(|s| std::sync::Arc::from(s) as glyph_ast::Ident)
                    .collect(),
                span: Span::new(0, 0),
            };
            let exports = stubs
                .exports_of(&path)
                .unwrap_or_else(|| panic!("`{module}` is not a stdlib module"));
            assert!(
                exports.contains(name),
                "`{module}` does not export `{name}`"
            );
        }
    }

    /// The residue stays unkeyed. A name no stdlib module declares gets no
    /// invented module, which is the half of G231 that is a decision rather
    /// than a lookup.
    #[test]
    fn the_residue_has_no_declaring_module() {
        for name in [
            "Array",
            "Record",
            "Schema",
            "Component",
            "Issue",
            "par",
            "print",
            "assert",
            "infer_output",
            "string",
            "number",
            "int",
            "bool",
        ] {
            assert_eq!(
                prelude_declaring_module(name),
                None,
                "`{name}` was given an invented module"
            );
        }
    }

    /// Every name the table keys is a name the prelude actually binds.
    #[test]
    fn every_keyed_name_is_in_the_prelude() {
        let p = build_prelude();
        for name in ["Result", "Ok", "Err", "Option", "Some", "None", "Nullable"] {
            assert!(p.lookup(name).is_some(), "`{name}` is not a prelude name");
        }
    }
}
