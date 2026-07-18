//! Symbols — what a name in scope refers to.
//!
//! The resolver builds a `SymbolTable` per module. Each top-level declaration
//! and each import introduces one or more `Symbol`s. Local bindings (function
//! parameters, `let`s inside a block, match arm bindings) are not stored in
//! the table; they live in transient scopes during the resolution walk.
//!
//! `SymbolId` is the stable identifier handed out to the typechecker and
//! downstream consumers. Two symbols in the same module never share an id.
//! Cross-module symbols are produced when the import graph stitches modules
//! together (week 2 day 3+); the resolver hands out a fresh `SymbolId` for
//! each imported alias and records the upstream module + name.

use std::sync::Arc;

use glyph_ast::{Ident, ModulePath, Span};

/// Stable handle to a `Symbol` inside a `SymbolTable`. Cheap to copy. The
/// `u32` is an index into the table's `Vec<Symbol>` storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: Ident,
    pub kind: SymbolKind,
    /// Source span of the declaration site, or a zero-span for prelude
    /// built-ins (which have no source).
    pub span: Span,
}

/// What kind of thing this name refers to. Drives type lookup at the
/// typechecker boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    /// `fn name(...) { ... }` — index into the module's `items` Vec.
    Function { decl_idx: u32 },
    /// `type Name = ...` — index into the module's `items` Vec.
    Type { decl_idx: u32 },
    /// `const NAME = ...` — index into the module's `items` Vec.
    Const { decl_idx: u32 },
    /// `component Name(props) -> Component { ... }` — index into the module's
    /// `items` Vec.
    Component { decl_idx: u32 },

    /// A variant of a tagged union: `type FeedError = | NetworkError(...) | ...`
    /// hoists `NetworkError` into module scope as a `Variant` symbol pointing
    /// back at the `Type` declaration that owns it. The variant's own name
    /// lives on the enclosing `Symbol.name`; the typechecker uses `decl_idx`
    /// to recover the parent union's payload type.
    Variant { decl_idx: u32 },

    /// `import std/io` — `io` (the last path segment) is the introduced name.
    /// `member` access through it goes to the imported module's exports.
    ImportNamespace {
        path: ModulePath,
    },

    /// `import std/http as h` — `h` is the introduced name; otherwise identical
    /// to `ImportNamespace`.
    ImportAlias {
        path: ModulePath,
        alias: Ident,
    },

    /// `import std/result { Ok, Err }` — each named import becomes one
    /// `ImportNamed`. Resolved against the target module's exports during
    /// cross-module pass (week 2 day 3+).
    ImportNamed {
        path: ModulePath,
        /// The name as written in the import list. May be aliased (`{ Ok as O }`)
        /// once D15 grows that form; in v1 it's identical to the introduced name.
        original: Ident,
    },

    /// Prelude built-in: a primitive (`string`, `number`, ...) or a generic
    /// container (`Result`, `Option`, `Array`, ...). The string discriminator
    /// is enough for now; the typechecker maps these to concrete `Ty` variants.
    Prelude { kind: PreludeKind },
}

/// Discriminates prelude built-ins so the typechecker can map symbol → `Ty`
/// without a big string match. Keep small; this enum is the contract between
/// the resolver and the typechecker for built-ins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreludeKind {
    // Primitive types
    String,
    Number,
    Bool,
    Void,
    /// TypeScript's `unknown` keyword. A top type.
    UnknownTop,

    // Generic container types (resolved by name; arity in the typechecker)
    Result,
    Option,
    Array,
    /// `Record<K, V>` from the validator example.
    Record,
    /// `Schema<T>` from the validator example.
    Schema,
    /// `Component` from the React example.
    Component,
    /// `Issue` — the non-generic record type (`{ path, message }`) that
    /// `json.parse`/schema decoders report in their `Err` arm. Ambient and
    /// unwritable-by-import, so it lives in the prelude table.
    Issue,

    // Prelude values
    Ok,
    Err,
    Some,
    None,
    /// `par` namespace (`par.all`, `par.all_ok`).
    Par,
    /// `print` (used in examples; will become `io.println` once stdlib lands).
    Print,
    /// `assert(condition)` — used inside `@doc @run` blocks (D26); a failed
    /// assertion throws and fails the build.
    Assert,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SymbolTable {
    /// Dense Vec of `Symbol`. Index = `SymbolId.0`.
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, sym: Symbol) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(sym);
        id
    }

    pub fn get(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.0 as usize)
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }
}

/// Convenience helper: build a prelude symbol with a zero span and the given
/// kind. Used by `prelude.rs`.
pub fn prelude_symbol(name: &str, kind: PreludeKind) -> Symbol {
    Symbol {
        name: Arc::from(name),
        kind: SymbolKind::Prelude { kind },
        span: Span::new(0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_returns_dense_ids() {
        let mut t = SymbolTable::new();
        let a = t.intern(prelude_symbol("string", PreludeKind::String));
        let b = t.intern(prelude_symbol("number", PreludeKind::Number));
        assert_eq!(a.0, 0);
        assert_eq!(b.0, 1);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn get_round_trips() {
        let mut t = SymbolTable::new();
        let id = t.intern(prelude_symbol("Ok", PreludeKind::Ok));
        let sym = t.get(id).unwrap();
        assert_eq!(sym.name.as_ref(), "Ok");
    }
}
