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

pub use assign::{
    assign_types, assign_types_with_resolver, DeclTyResolver, LocalDeclTy,
};
pub use lower::{lower_type_expr, Lowerer};
pub use ty::{FnParam, ParamOwner, Primitive, RecordField, SymbolRef, Ty, UnionVariant};
pub use type_map::TypeMap;

use glyph_ast::Span;

/// Errors emitted by the typechecker. Day-14 surfaces the first real
/// variant: `NonExhaustiveMatch`, emitted when a `match` over a
/// tagged-union scrutinee fails to cover every variant (D9). Further
/// variants land in later week-3 days as the bidirectional checker,
/// `?` typing, and `owned` analysis ship.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypeError {
    /// `match X { ... }` where some variants of X's tagged-union type
    /// have no covering arm and no wildcard / `else` catches the rest.
    /// `missing` is a comma-separated list of variant names in
    /// declaration order (so the diagnostic is reproducible).
    #[error("non-exhaustive match on `{type_name}`: missing variants {missing}")]
    NonExhaustiveMatch {
        type_name: String,
        missing: String,
        span: Span,
    },

    /// `expr?` used where the enclosing function does not return `Result`.
    /// The `?` operator propagates the `Err` arm to the caller, so it is
    /// only legal inside a function whose declared return type is
    /// `Result<_, _>`. Day-15 scope is the enclosing-function side of the
    /// rule (D + week-3 task 2); the operand-side check ("`expr` must be a
    /// `Result` and its `E` must match the function's `E`") needs the
    /// bidirectional checker and lands in a later day.
    #[error("the `?` operator is only valid inside a function that returns `Result`")]
    QuestionOutsideResultFn { span: Span },
}

impl TypeError {
    pub fn span(&self) -> Span {
        match self {
            TypeError::NonExhaustiveMatch { span, .. } => *span,
            TypeError::QuestionOutsideResultFn { span } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_module_compiles() {
        let _t: Ty = Ty::unknown();
    }
}
