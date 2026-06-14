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
//! - D9  exhaustive match: per-scrutinee checkers (tagged-union variant
//!       set with arbitrary-depth single-payload recursion, prelude
//!       `Result`/`Option`, array length coverage, and `bool`). Not the
//!       general Maranget matrix — products of independent refutable
//!       columns are conservatively treated as covered (deferred to v1.1).
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
pub mod owned;
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
    /// `Result<_, _>`. This is the enclosing-function side of the week-3
    /// task-2 rule; the operand side ("`expr` must be a `Result` and its
    /// `E` must match the function's `E`") is carried by
    /// `QuestionOnNonResult` and `QuestionErrorTypeMismatch`.
    #[error("the `?` operator is only valid inside a function that returns `Result`")]
    QuestionOutsideResultFn { span: Span },

    /// `expr?` whose operand is decidably not a `Result`. The `?` operator
    /// unwraps a `Result`, propagating its `Err` to the caller, so its
    /// operand must be a `Result<T, E>` (week-3 task 2, operand side).
    /// Fires only when the operand's type is fully resolved and provably
    /// non-`Result`; an operand whose type can't be judged (`Unknown`, a
    /// generic parameter, an application over an unresolved base) stays
    /// permissive, so the check never produces a false positive.
    #[error("the `?` operator requires a `Result` operand, but found `{found}`")]
    QuestionOnNonResult { found: String, span: Span },

    /// `expr?` whose operand error type `E` differs from the enclosing
    /// function's declared `Result<_, E>` error type. v1 has no `From`
    /// conversion (the brainstorm's Q5 plan), so the two `E`s must match
    /// exactly. Fires only when both error types are fully resolved and
    /// provably distinct — when either side is undecidable the check stays
    /// silent.
    #[error("the `?` operator propagates error type `{found}`, but the enclosing function returns `Result<_, {expected}>`")]
    QuestionErrorTypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },

    /// A value's type is incompatible with the type required at its
    /// position. Day-21 emits this for `return` statements whose value is a
    /// concrete primitive (`string`/`number`/`bool`/`void`) that differs
    /// from the function's declared primitive return type. The check is
    /// deliberately narrow — assignability over named, generic, record, and
    /// function types is a later day — so it never fires on a type it can't
    /// judge with certainty.
    #[error("type mismatch: expected `{expected}`, found `{found}`")]
    TypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },

    /// `let owned x: T = ...` where `T` is not a type declared with the
    /// `resource` marker (D25). The `owned` modifier is the narrow carve-out
    /// for resource handles only; binding a non-resource value with `owned`
    /// has no meaning. Fires only when `T` is decidably non-resource — a
    /// binding whose type can't be judged is left untracked, never flagged.
    #[error("`owned` requires a resource type, but `{name}` has non-resource type `{ty}`")]
    OwnedRequiresResourceType {
        name: String,
        ty: String,
        span: Span,
    },

    /// An `owned` resource handle (D25) is still live on a path that exits
    /// the function — either a `return` reached while the handle is
    /// unconsumed, or fall-through to the end of the body. The handle must
    /// be consumed (moved into an `owned` parameter) exactly once on every
    /// path. `span` points at the `let owned` binding.
    #[error("`owned` resource `{name}` is not consumed on every path before the function returns")]
    OwnedNotConsumed { name: String, span: Span },

    /// An `owned` resource handle (D25) is used after it was moved (consumed).
    /// Covers both double-consume (moved into a second `owned` parameter) and
    /// any read of the handle after the move. `span` points at the offending
    /// use; the move site is named in the message.
    #[error("`owned` resource `{name}` is used after it was consumed")]
    OwnedUsedAfterMove { name: String, span: Span },

    /// A `match` over an array scrutinee does not cover every length. Array
    /// patterns cover lengths: `[]` covers the empty array, `[a, b]` covers
    /// exactly length 2, and `[a, ...rest]` covers every length ≥ 1 — but only
    /// when the fixed elements are irrefutable (bindings/wildcards), since a
    /// literal element like `["help"]` matches only some arrays of that length.
    /// `missing` names the smallest uncovered case.
    #[error("non-exhaustive array match: {missing} not covered")]
    NonExhaustiveArrayMatch { missing: String, span: Span },

    /// A `match` over a `bool` scrutinee covers neither both `true` and
    /// `false` nor a catch-all (`_`, `else`, or a binding). D3 makes `match`
    /// the only conditional, so an open boolean match is a real gap rather
    /// than a stylistic choice. `missing` names the uncovered case(s).
    /// Boolean *expressions* such as comparisons type as `Unknown` and are
    /// not checked; only a value of statically-known `bool` type triggers
    /// this.
    #[error("non-exhaustive match on `bool`: {missing} not covered")]
    NonExhaustiveBoolMatch { missing: String, span: Span },
}

impl TypeError {
    pub fn span(&self) -> Span {
        match self {
            TypeError::NonExhaustiveMatch { span, .. } => *span,
            TypeError::QuestionOutsideResultFn { span } => *span,
            TypeError::QuestionOnNonResult { span, .. } => *span,
            TypeError::QuestionErrorTypeMismatch { span, .. } => *span,
            TypeError::TypeMismatch { span, .. } => *span,
            TypeError::OwnedRequiresResourceType { span, .. } => *span,
            TypeError::OwnedNotConsumed { span, .. } => *span,
            TypeError::OwnedUsedAfterMove { span, .. } => *span,
            TypeError::NonExhaustiveArrayMatch { span, .. } => *span,
            TypeError::NonExhaustiveBoolMatch { span, .. } => *span,
        }
    }

    /// Stable diagnostic code. Typechecker codes live in the `E02xx` range
    /// (see `docs/error-codes.md`). `--explain <code>` documents each one.
    pub fn code(&self) -> &'static str {
        match self {
            TypeError::NonExhaustiveMatch { .. } => "E0200",
            TypeError::QuestionOutsideResultFn { .. } => "E0201",
            TypeError::QuestionOnNonResult { .. } => "E0202",
            TypeError::QuestionErrorTypeMismatch { .. } => "E0203",
            TypeError::TypeMismatch { .. } => "E0204",
            TypeError::OwnedRequiresResourceType { .. } => "E0205",
            TypeError::OwnedNotConsumed { .. } => "E0206",
            TypeError::OwnedUsedAfterMove { .. } => "E0207",
            TypeError::NonExhaustiveArrayMatch { .. } => "E0208",
            TypeError::NonExhaustiveBoolMatch { .. } => "E0209",
        }
    }

    /// A one-line, actionable fix (the Elm-quality bar): what to change.
    pub fn help(&self) -> Option<&'static str> {
        Some(match self {
            TypeError::NonExhaustiveMatch { .. } => {
                "Add an arm for each missing variant, or an `else` arm to catch the rest."
            }
            TypeError::QuestionOutsideResultFn { .. } => {
                "Use `?` only inside a function that returns `Result<_, _>`, or handle the error with `match`."
            }
            TypeError::QuestionOnNonResult { .. } => {
                "`?` unwraps a `Result`; this operand is not one. Drop the `?`, or make the expression return a `Result`."
            }
            TypeError::QuestionErrorTypeMismatch { .. } => {
                "v1 has no automatic error conversion. Map the error first (e.g. `.map_err(...)`) so its `E` matches the function's."
            }
            TypeError::TypeMismatch { .. } => {
                "Change the value, or the declared type, so the two agree."
            }
            TypeError::OwnedRequiresResourceType { .. } => {
                "`owned` is only for `resource`-marked types. Drop `owned`, or mark the type `resource`."
            }
            TypeError::OwnedNotConsumed { .. } => {
                "Consume the handle on every path (move it into an `owned` parameter) before the function returns."
            }
            TypeError::OwnedUsedAfterMove { .. } => {
                "A consumed handle cannot be used again. Reorder so every use comes before the consume."
            }
            TypeError::NonExhaustiveArrayMatch { .. } => {
                "Add an arm for the missing length, a `[first, ...rest]` arm, or a catch-all binding."
            }
            TypeError::NonExhaustiveBoolMatch { .. } => {
                "Cover both `true` and `false`, or add an `else` arm."
            }
        })
    }

    /// An optional background note (the "why").
    pub fn note(&self) -> Option<&'static str> {
        match self {
            TypeError::NonExhaustiveMatch { .. } => Some(
                "Tagged unions are sealed (D9): adding a variant forces every match to be updated. \
                 A `_`/`else` catch-all is allowed but forfeits that guarantee.",
            ),
            TypeError::OwnedNotConsumed { .. } | TypeError::OwnedUsedAfterMove { .. } => Some(
                "`owned` is the D25 resource-handle carve-out: a handle is consumed exactly once on every path.",
            ),
            _ => None,
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
