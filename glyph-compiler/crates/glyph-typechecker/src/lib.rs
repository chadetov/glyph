//! Glyph typechecker — Phase 1 week 2 (slice 1: type representation).
//!
//! Bidirectional checker. Salsa-backed per Q5 hybrid (week 2 day-3+).
//!
//! Implements (Phase 1 week 2 slice 1):
//! - `Ty` — the resolved, normalized type representation (see `ty.rs`)
//! - `TypeMap` — span-indexed map from `Expr` nodes to `Ty`
//!
//! Implements (Phase 1 week 3, planned):
//! - D5  `mut` is syntactic only (grammar restricts; typechecker does NOT
//!       verify method-call mutation per Q7 resolution)
//! - D7  type expressions; nominal newtypes (no general refinement types in
//!       v1 per Q15 resolution; mapped types deferred to v1.1 per Q1)
//! - D8  runtime descriptor emission for every type declaration (Q8 core)
//! - D9  exhaustive match via Maranget 2007 (~400 LoC)
//! - D16 `void` type and value
//! - D24 `@redact` metadata propagates with the type's runtime descriptor
//! - D25 `owned` single-consumption analysis across paths (manifesto carve-out)
//! - D27 annotation dispatch table (recognizes `@example`, `@pure`, `@redact`,
//!       `@doc`, etc.; unknown annotations are a hard error)
//!
//! Phase 1 week 7: error-message audit. Elm-quality bar per Q6 resolution.

#![forbid(unsafe_code)]

pub mod assign;
pub mod lower;
pub mod ty;
pub mod type_map;

pub use assign::assign_types;
pub use lower::{lower_type_expr, Lowerer};
pub use ty::{FnParam, ParamOwner, Primitive, RecordField, SymbolRef, Ty, UnionVariant};
pub use type_map::TypeMap;

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("typechecker week 3 stub: full check not yet implemented")]
    NotImplemented,
}

/// Entry-point stub. Real implementation lands Phase 1 week 3.
pub fn typecheck_module() -> Result<(), TypeError> {
    Err(TypeError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_module_compiles() {
        assert!(typecheck_module().is_err());
        let _t: Ty = Ty::unknown();
    }
}
