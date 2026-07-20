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

/// Render a `Ty` for human display (LSP hover, diagnostics). Structural where
/// useful — `Array<number>`, `Result<User, string>`, `{ name: string }`,
/// `fn(number) -> bool`, `A | B(T)` — and `?` for the not-yet-inferred
/// placeholder. Distinct from the terse internal `ty_display` used in error
/// strings, which collapses composites to a category word.
pub fn display_ty(ty: &Ty) -> String {
    match ty {
        Ty::Unknown => "?".to_string(),
        Ty::UnknownTop => "unknown".to_string(),
        Ty::Prim(p) => p.as_str().to_string(),
        Ty::Named { path, .. } if !path.is_empty() => {
            path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".")
        }
        Ty::Named { .. } => "?".to_string(),
        Ty::Param { name, .. } => name.to_string(),
        Ty::App { base, args } => {
            let args = args.iter().map(display_ty).collect::<Vec<_>>().join(", ");
            format!("{}<{}>", display_ty(base), args)
        }
        Ty::Record { fields } if fields.is_empty() => "{}".to_string(),
        Ty::Record { fields } => {
            let fields = fields
                .iter()
                .map(|f| {
                    let opt = if f.optional { "?" } else { "" };
                    format!("{}{}: {}", f.name, opt, display_ty(&f.ty))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {fields} }}")
        }
        Ty::Fn {
            params,
            return_ty,
            is_async,
        } => {
            let params = params
                .iter()
                .map(|p| display_ty(&p.ty))
                .collect::<Vec<_>>()
                .join(", ");
            let prefix = if *is_async { "async " } else { "" };
            format!("{prefix}fn({params}) -> {}", display_ty(return_ty))
        }
        Ty::Union { variants } => variants
            .iter()
            .map(|v| match &v.payload {
                Some(p) => format!("{}({})", v.name, display_ty(p)),
                None => v.name.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

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

    /// `x.field` where `x`'s type is decidably a record (or a named record
    /// type) that has no field named `field`. Fires only when the object's type
    /// resolves to a concrete record whose field set is known — a typo'd or
    /// renamed field. An object whose type can't be judged (`Unknown`, a generic
    /// parameter, a non-record like `Array`/a namespace) is left unchecked, so
    /// the check never produces a false positive.
    #[error("type `{type_name}` has no field `{field}`")]
    UnknownField {
        field: String,
        type_name: String,
        span: Span,
    },

    /// A call argument's type is decidably incompatible with the parameter type
    /// it is passed to. Fires only when both types are fully resolved and
    /// provably distinct (primitive mismatches, different named types, a generic
    /// application over a different base) — an argument or parameter whose type
    /// can't be judged stays permissive, so the check never produces a false
    /// positive.
    #[error("argument type mismatch: expected `{expected}`, found `{found}`")]
    ArgumentTypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },

    /// A call supplies the wrong number of arguments for the callee's declared
    /// arity. Glyph `fn`/`component` parameters are all required — there are no
    /// optional or variadic parameters in v1, and call arguments carry no spread
    /// — so a call whose argument count differs from the parameter count is
    /// always wrong. Fires only when the callee resolves to a concrete `Ty::Fn`
    /// (a module-level fn/component or a typed lambda binding); a callee whose
    /// signature can't be judged (a member-access method, an unresolved name)
    /// stays `Unknown` and is left unchecked.
    #[error("wrong number of arguments: expected {expected}, found {found}")]
    ArgumentCountMismatch {
        expected: usize,
        found: usize,
        span: Span,
    },

    /// `mut N = ...` reassigning a module-level `const` binding. D20 makes
    /// `const` immutable; only a function-level `let` may be reassigned with
    /// `mut`. Fires when the assignment target resolves to a `const` declaration.
    #[error("cannot reassign `{name}`: it is a `const`")]
    MutateConst { name: String, span: Span },

    /// A `match` over a `bool` scrutinee covers neither both `true` and
    /// `false` nor a catch-all (`_`, `else`, or a binding). D3 makes `match`
    /// the only conditional, so an open boolean match is a real gap rather
    /// than a stylistic choice. `missing` names the uncovered case(s).
    /// Boolean *expressions* such as comparisons type as `Unknown` and are
    /// not checked; only a value of statically-known `bool` type triggers
    /// this.
    #[error("non-exhaustive match on `bool`: {missing} not covered")]
    NonExhaustiveBoolMatch { missing: String, span: Span },

    /// A `match` over a `number` or `string` scrutinee with literal-value arms
    /// but no catch-all. Those domains are unbounded, so a set of literal arms
    /// can never be exhaustive; the emitter lowers the match to a `switch` whose
    /// `default` throws, turning an uncovered value into a runtime crash. D3
    /// makes `match` the only conditional, so an open value match is a real gap.
    /// `type_name` is `number` or `string`.
    #[error("non-exhaustive match on `{type_name}`: no catch-all for the other values")]
    NonExhaustiveValueMatch { type_name: String, span: Span },

    /// A `@redact fields: [...]` annotation (D24) names a field the type does
    /// not have — a typo or a renamed field. Redaction is type-level
    /// enforcement, so an unknown field name is a hard error: it would silently
    /// mask nothing. Only record types have redactable fields.
    #[error("`@redact` names field `{field}`, which `{type_name}` does not have")]
    RedactUnknownField {
        field: String,
        type_name: String,
        span: Span,
    },

    /// A `component` declared with more than one parameter. A component lowers to
    /// a React function component, which is called with a single props object, so
    /// multiple positional parameters would silently bind the first to the whole
    /// props object and leave the rest undefined (D19). A component takes a
    /// single props record (`component C(props: P)`), or no parameters.
    #[error("a component takes a single props record, not {count} parameters")]
    ComponentMultipleParams { count: usize, span: Span },

    /// `let g = h` whose initializer is a bare reference to a live `owned`
    /// handle. Aliasing a handle creates a second binding to the same resource,
    /// so both could be consumed — defeating single-consumption (D25). Consume
    /// the handle (move it into an `owned` parameter) instead of rebinding it.
    #[error("cannot alias the `owned` handle `{name}`")]
    OwnedAliased { name: String, span: Span },

    /// A `match` arm that can never be reached because an earlier arm is
    /// irrefutable — a catch-all (`_`, `else`) or a binding (a bare
    /// identifier that is not a variant of the scrutinee's type) matches
    /// every value, so no later arm ever runs. Glyph's `match` is
    /// first-match-wins, so an arm after a total pattern is dead code (D9).
    /// This is also a soundness guard: the emitter lowers a leading binding
    /// catch-all to a `switch` `default`, and a JS `switch` gives `case`
    /// priority over `default` regardless of source order, so a shadowed
    /// later arm would silently win at runtime. Rejecting the dead arm
    /// removes that hazard. `span` points at the unreachable arm.
    #[error("unreachable match arm: an earlier arm already matches every value")]
    UnreachableMatchArm { span: Span },

    /// A `Result`-typed expression used as a statement and discarded. Because a
    /// `Result` carries a possible `Err`, dropping it silently swallows a
    /// failure. This is a **warning** (severity `Warning`), not an error:
    /// discarding a `Result` is legal but almost always a mistake. `span` points
    /// at the dropped expression.
    #[error("this `Result` is discarded; its `Err` case is silently ignored")]
    UnusedResult { span: Span },
}

/// Diagnostic severity. Most diagnostics are hard `Error`s that fail the build;
/// a `Warning` is surfaced but does not (verifiability stays the lead pillar, so
/// warnings are reserved for "legal but almost certainly a mistake").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl TypeError {
    /// Whether this diagnostic fails the build (`Error`) or is only surfaced
    /// (`Warning`). Everything is an `Error` except the few advisory lints.
    pub fn severity(&self) -> Severity {
        match self {
            TypeError::UnusedResult { .. } => Severity::Warning,
            _ => Severity::Error,
        }
    }
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
            TypeError::NonExhaustiveValueMatch { span, .. } => *span,
            TypeError::RedactUnknownField { span, .. } => *span,
            TypeError::UnknownField { span, .. } => *span,
            TypeError::ArgumentTypeMismatch { span, .. } => *span,
            TypeError::ArgumentCountMismatch { span, .. } => *span,
            TypeError::MutateConst { span, .. } => *span,
            TypeError::ComponentMultipleParams { span, .. } => *span,
            TypeError::OwnedAliased { span, .. } => *span,
            TypeError::UnreachableMatchArm { span } => *span,
            TypeError::UnusedResult { span } => *span,
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
            TypeError::NonExhaustiveValueMatch { .. } => "E0218",
            TypeError::RedactUnknownField { .. } => "E0219",
            TypeError::UnknownField { .. } => "E0210",
            TypeError::ArgumentTypeMismatch { .. } => "E0211",
            TypeError::MutateConst { .. } => "E0212",
            TypeError::ArgumentCountMismatch { .. } => "E0213",
            TypeError::ComponentMultipleParams { .. } => "E0214",
            TypeError::OwnedAliased { .. } => "E0215",
            TypeError::UnreachableMatchArm { .. } => "E0216",
            TypeError::UnusedResult { .. } => "E0217",
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
            TypeError::NonExhaustiveValueMatch { .. } => {
                "Add an `else` arm. A `number`/`string` match with only literal arms can never be exhaustive."
            }
            TypeError::RedactUnknownField { .. } => {
                "Check the field name for a typo. `@redact` lists fields of the record type it decorates, and only record types have redactable fields."
            }
            TypeError::UnknownField { .. } => {
                "Check the field name for a typo, or add the field to the type."
            }
            TypeError::ArgumentTypeMismatch { .. } => {
                "Pass a value of the expected type, or change the parameter's type."
            }
            TypeError::ArgumentCountMismatch { .. } => {
                "Supply exactly one argument per parameter. Glyph has no optional or variadic parameters."
            }
            TypeError::MutateConst { .. } => {
                "`const` is immutable (D20). Use a function-level `let` if the binding must change."
            }
            TypeError::ComponentMultipleParams { .. } => {
                "Take a single props record: `component C(props: P)` with `type P = { ... }`, then read `props.field`."
            }
            TypeError::OwnedAliased { .. } => {
                "An `owned` handle cannot be rebound. Consume it directly (pass it to an `owned` parameter) instead of aliasing it."
            }
            TypeError::UnreachableMatchArm { .. } => {
                "Remove this arm, or move the catch-all/binding arm below it so the specific arms come first."
            }
            TypeError::UnusedResult { .. } => {
                "Handle it with `match`, propagate it with `?`, or bind it (`let _ = ...`) to say the discard is intentional."
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
            TypeError::UnreachableMatchArm { .. } => Some(
                "`match` is first-match-wins (D9): a catch-all or binding arm matches every value, so any arm after it is dead code.",
            ),
            TypeError::UnusedResult { .. } => Some(
                "Errors in Glyph are values, not exceptions: a dropped `Result` is a dropped error path. Making the discard explicit keeps failures visible.",
            ),
            TypeError::NonExhaustiveValueMatch { .. } => Some(
                "`number` and `string` are unbounded, so literal arms can never cover every value; the emitted `switch` `default` throws at runtime.",
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
