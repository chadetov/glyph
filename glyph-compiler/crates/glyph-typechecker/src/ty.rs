//! The Glyph type representation, `Ty`.
//!
//! Distinct from `glyph_ast::TypeExpr`: a `TypeExpr` is the syntactic shape the
//! user wrote (e.g. `Array<User>`); a `Ty` is the resolved, normalized type
//! the rest of the compiler reasons about (`App(Array, [User])` where `User`
//! itself is a `Named` pointing at a resolver-assigned symbol).
//!
//! Week 2 scope (this slice): the enum exists and every `Expr` node gets a
//! `Ty` (mostly `Unknown`). Week 3 will populate it from function signatures,
//! match arms, and tagged-union dispatch.
//!
//! Design notes:
//! - `Ty` is interned by reference identity via `Arc`. Cheap to clone, share
//!   across the salsa cache once that wraps in week 2 day-3+ (I4).
//! - No mapped types (Q1 → v1.1). No refinement types (Q15 nominal newtypes
//!   only). No conditional types. The v1 floor.
//! - The prelude types (`Result`, `Option`, `Array`, primitives) are
//!   `Ty::Named` with `SymbolId`s pre-assigned by `glyph-resolver::prelude`.

use std::sync::Arc;

use glyph_ast::Ident;

/// A resolved Glyph type. Built by the typechecker from `glyph_ast::TypeExpr`
/// plus resolution information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// Placeholder type — emitted for any expression we don't yet know how to
    /// check. Acceptance for week 2 is "every Expr has a Ty"; `Unknown` is
    /// the legal v0 default everywhere we haven't propagated a real type.
    Unknown,

    /// A built-in primitive: `string`, `number`, `bool`, `void`.
    Prim(Primitive),

    /// `unknown` — TypeScript's `unknown`. A top type; only assignable via an
    /// `is` check or explicit cast. Distinct from `Unknown` (that's the
    /// compiler's "haven't figured it out yet" placeholder).
    UnknownTop,

    /// A named type referenced by symbol id. The actual definition lives in
    /// the resolver's symbol table; the typechecker fetches it on demand.
    /// `path` is the original lexical path for diagnostics (e.g. `["http", "Response"]`
    /// or `["Result"]`).
    Named {
        symbol: SymbolRef,
        path: Vec<Ident>,
    },

    /// A type-parameter binding inside a generic declaration. `fn f<T>(...)`
    /// emits a `Param("T", DeclSlot::Fn(idx))` so monomorphization can
    /// substitute.
    Param { name: Ident, owner: ParamOwner },

    /// Generic application: `Result<User, FeedError>` is `App(Result, [User, FeedError])`.
    App { base: Arc<Ty>, args: Vec<Ty> },

    /// Structural record type: `{ name: string, age: number }`. Optional
    /// fields recorded.
    Record { fields: Vec<RecordField> },

    /// Function type: `fn(a: string, b: number) -> bool`.
    Fn {
        params: Vec<FnParam>,
        return_ty: Arc<Ty>,
        is_async: bool,
    },

    /// Tagged union (D8): `Ok(T) | Err(E)`. Variants carry an optional payload.
    Union { variants: Vec<UnionVariant> },
}

/// Stable handle for a named type or value. Mirrors `glyph_resolver::SymbolId`
/// but kept here as an opaque newtype so the typechecker doesn't depend on the
/// resolver's storage choices. The two are converted at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolRef(pub u32);

impl From<glyph_resolver::SymbolId> for SymbolRef {
    fn from(id: glyph_resolver::SymbolId) -> Self {
        SymbolRef(id.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    String,
    Number,
    Bool,
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordField {
    pub name: Ident,
    pub ty: Ty,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnParam {
    pub name: Option<Ident>,
    /// D25: this parameter takes ownership of its argument (a move). Drives
    /// the single-consumption analysis: passing an `owned`-bound handle into
    /// an `owned` parameter is the consume.
    pub owned: bool,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionVariant {
    pub name: Ident,
    pub payload: Option<Ty>,
}

/// Which generic-parameter scope a `Ty::Param` belongs to. The same parameter
/// name can appear in multiple declarations; `ParamOwner` distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamOwner {
    /// Generic parameter on a `fn` or `component` declaration.
    Callable(SymbolRef),
    /// Generic parameter on a `type` declaration.
    TypeDecl(SymbolRef),
    /// Owner not yet resolved. The day-2 lowering doesn't track which
    /// declaration introduced a `Ty::Param` — week 3's bidirectional checker
    /// fills the real owner on first lookup.
    Unresolved,
}

impl Ty {
    /// Return a fresh `Ty::Unknown`. The compiler-wide default whenever no
    /// type information is available.
    pub fn unknown() -> Ty {
        Ty::Unknown
    }

    /// Returns true if this type is `Unknown` — the compiler placeholder, not
    /// the user-visible `unknown` keyword.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }
}

impl Primitive {
    pub fn as_str(self) -> &'static str {
        match self {
            Primitive::String => "string",
            Primitive::Number => "number",
            Primitive::Bool => "bool",
            Primitive::Void => "void",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_constructor() {
        let t = Ty::unknown();
        assert!(t.is_unknown());
    }

    #[test]
    fn primitive_names_match_spec() {
        assert_eq!(Primitive::String.as_str(), "string");
        assert_eq!(Primitive::Number.as_str(), "number");
        assert_eq!(Primitive::Bool.as_str(), "bool");
        assert_eq!(Primitive::Void.as_str(), "void");
    }

    #[test]
    fn app_holds_args() {
        let result_app = Ty::App {
            base: Arc::new(Ty::Named {
                symbol: SymbolRef(0),
                path: vec!["Result".into()],
            }),
            args: vec![Ty::Prim(Primitive::String), Ty::Unknown],
        };
        match result_app {
            Ty::App { args, .. } => assert_eq!(args.len(), 2),
            _ => panic!(),
        }
    }
}
