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

use crate::module_graph::ModuleId;

/// Stable handle to a `Symbol` inside a `SymbolTable`. Cheap to copy. The
/// `u32` is an index into the table's `Vec<Symbol>` storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

/// The identity of one top-level declaration: the module it is declared in,
/// and the name it is declared under.
///
/// The crate now has two identity schemes, and this one exists because
/// [`SymbolId`] cannot do this job. A `SymbolId` is a dense index into one
/// module's [`SymbolTable`], so it is a *position*, and positions move:
/// inserting `fn audit` above `fn charge` moves `charge` from `SymbolId(1)` to
/// `SymbolId(2)`, and the renumbering reaches the lowered types: with `charge`
/// byte-identical, adding a `type Tax` above `type Money` changes its return
/// type from `SymbolRef(0)` to `SymbolRef(1)`. Anything that has to keep
/// pointing at a declaration across an edit (a semantic-graph edge, a
/// cross-file reference, a stored answer) needs the name, not the position.
/// Carrying the position instead is the same class of mistake as carrying a
/// foreign module's symbol id, which `Ty::Imported`'s doc comment describes.
///
/// **What a key survives.** Any edit that neither renames the declaration nor
/// moves it to another module: a declaration inserted above, below or between;
/// a rewritten body; a changed signature. Also the declaration's *kind*, which
/// is deliberately not part of the key. Glyph's module namespace is flat and
/// single (`fn Foo` beside `type Foo`, a hoisted variant beside a function of
/// that name, and `const NAME` beside `fn NAME` are all already rejected as
/// duplicate declarations), so a kind buys no uniqueness, and including one
/// would make rewriting `const Limit` into `fn Limit()` a different entity when
/// it is the same one.
///
/// **What a key does not survive.** A rename, or a move to another module.
/// Both are the correct answer: both are what every other consumer of the name
/// sees change too. It is also no more portable than its [`ModuleId`], which is
/// an interner index rather than a durable name; see that type's docs.
///
/// Locals are out of scope. A `let`, a parameter and a match binding are not
/// declarations, live in transient scopes rather than the table, and have no
/// key here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclKey {
    module: ModuleId,
    name: Ident,
}

impl DeclKey {
    pub fn new(module: ModuleId, name: Ident) -> Self {
        Self { module, name }
    }

    /// The module the declaration is declared in.
    pub fn module(&self) -> ModuleId {
        self.module
    }

    /// The name the declaration is declared under.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: Ident,
    pub kind: SymbolKind,
    /// Source span of the declaration site, or a zero-span for prelude
    /// built-ins (which have no source).
    pub span: Span,
    /// Whether the declaration is exported from its module (`pub`, 0.1.16).
    /// Drives the module's export surface: a non-public name is invisible to
    /// `import M { N }` in another module. Prelude/built-in symbols are public.
    pub is_public: bool,
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

    /// `import express { default as app }` — the module's *default* export
    /// bound to a local name.
    ///
    /// Distinct from `ImportNamed` on purpose. A default export has no name in
    /// the source module, so there is nothing to check it against: the export
    /// list that catches `string.repeeat` (G27) would reject every default
    /// import if this reused `ImportNamed` with `original: "default"`. It also
    /// emits differently (`import app from "express"`, not
    /// `import { app } from "express"`), and conflating the two is how the
    /// emitter would silently write the form `tsc` rejects with TS2595.
    ImportDefault {
        path: ModulePath,
        local: Ident,
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
    /// `int` — a whole `number`. TypeScript has no integer type, so it emits as
    /// `number` and is a `number` in Glyph's own checker; its only enforcement is
    /// a runtime `Number.isInteger` check in a descriptor (so a wire `3.5` fails
    /// an `int` field's `.parse`). Lets `gen` materialize a JSON-Schema `integer`
    /// with real boundary validation instead of collapsing it to `number`.
    Int,
    /// `bigint` — an exact arbitrary-precision integer. Emits as TypeScript
    /// `bigint`, distinct from `number` (`tsc` keeps them apart: no mixed
    /// arithmetic, `123n` literals only). Its descriptor checks
    /// `typeof x === "bigint"`. For exact large whole numbers (account IDs,
    /// counters) that a float `number` would silently round past 2^53.
    BigInt,
    Bool,
    Void,
    /// TypeScript's `unknown` keyword. A top type.
    UnknownTop,
    /// `never` — the bottom type (D43). Nothing is a value of it, so a
    /// function returning it does not return.
    Never,

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
    /// `infer_output<S>` (D28) — a type-level operator, not a container type.
    /// For a record `S` of parser-shaped fields (`{ parse(input): Result<V, _> }`)
    /// it yields `{ field: V, ... }`, the record of parsed output types.
    /// Resolved by name so it doesn't trip E0103; the emitter lowers it to a
    /// per-module TS mapped type and `tsc` reduces it.
    InferOutput,

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

    /// Whether the table holds nothing. Paired with `len` so a caller can ask
    /// the question directly rather than comparing a count to zero.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Convenience helper: build a prelude symbol with a zero span and the given
/// kind. Used by `prelude.rs`.
pub fn prelude_symbol(name: &str, kind: PreludeKind) -> Symbol {
    Symbol {
        name: Arc::from(name),
        kind: SymbolKind::Prelude { kind },
        span: Span::new(0, 0),
        is_public: true,
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

#[cfg(test)]
mod decl_key_tests {
    use super::*;
    use crate::module_graph::ModuleInterner;

    #[test]
    fn same_module_and_name_is_the_same_key() {
        let mut ids = ModuleInterner::new();
        let m = ids.intern_key("pay/charges");
        assert_eq!(
            DeclKey::new(m, Ident::from("charge")),
            DeclKey::new(m, Ident::from("charge"))
        );
    }

    #[test]
    fn a_different_name_in_the_same_module_is_a_different_key() {
        let mut ids = ModuleInterner::new();
        let m = ids.intern_key("pay/charges");
        assert_ne!(
            DeclKey::new(m, Ident::from("charge")),
            DeclKey::new(m, Ident::from("audit"))
        );
    }

    #[test]
    fn the_same_name_in_a_different_module_is_a_different_key() {
        let mut ids = ModuleInterner::new();
        let a = ids.intern_key("pay/charges");
        let b = ids.intern_key("pay/refunds");
        assert_ne!(
            DeclKey::new(a, Ident::from("charge")),
            DeclKey::new(b, Ident::from("charge"))
        );
    }

    #[test]
    fn keys_order_by_module_then_name() {
        // The ordering is what lets a caller hold keys in a `BTreeSet` and get
        // one module's declarations as a contiguous range.
        let mut ids = ModuleInterner::new();
        let a = ids.intern_key("a");
        let b = ids.intern_key("b");
        let mut keys = vec![
            DeclKey::new(b, Ident::from("alpha")),
            DeclKey::new(a, Ident::from("zeta")),
            DeclKey::new(a, Ident::from("alpha")),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                DeclKey::new(a, Ident::from("alpha")),
                DeclKey::new(a, Ident::from("zeta")),
                DeclKey::new(b, Ident::from("alpha")),
            ]
        );
    }

    #[test]
    fn a_key_hashes_by_module_and_name() {
        use std::collections::HashSet;
        let mut ids = ModuleInterner::new();
        let m = ids.intern_key("pay/charges");
        let mut set = HashSet::new();
        set.insert(DeclKey::new(m, Ident::from("charge")));
        assert!(set.contains(&DeclKey::new(m, Ident::from("charge"))));
        assert!(!set.contains(&DeclKey::new(m, Ident::from("refund"))));
    }
}
