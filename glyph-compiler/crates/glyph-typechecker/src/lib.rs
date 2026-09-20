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
//!   verify method-call mutation per Q7 resolution)
//! - D7  type expressions; nominal newtypes (no general refinement types in
//!   v1 per Q15 resolution; mapped types deferred to v1.1 per Q1)
//! - D8  runtime descriptor emission for every type declaration (Q8 core)
//! - D9  exhaustive match: per-scrutinee checkers (tagged-union variant
//!   set with arbitrary-depth single-payload recursion, prelude
//!   `Result`/`Option`, array length coverage, and `bool`). Not the
//!   general Maranget matrix — products of independent refutable
//!   columns are conservatively treated as covered (deferred to v1.1).
//! - D16 `void` type and value
//! - D24 `@redact` metadata propagates with the type's runtime descriptor
//! - D25 `owned` single-consumption analysis across paths (manifesto carve-out)
//! - D27 annotation dispatch table (recognizes `@example`, `@pure`, `@redact`,
//!   `@doc`, etc.; unknown annotations are a hard error)
//!
//! Phase 1 week 7: error-message audit. Elm-quality bar per Q6 resolution.

#![forbid(unsafe_code)]

pub mod assign;
pub mod concurrency;
pub mod lower;
pub mod owned;
pub mod stdlib;
pub mod ty;
pub mod type_map;

pub use assign::{
    alias_target, assign_types, assign_types_with_coverage, assign_types_with_relations,
    assign_types_with_resolver, assignability, direct_type_decl, imported_decl_chain_end,
    imported_string_literal_union_values,
    prelude_app, prelude_container, resolve_alias_chain, split_type_app,
    Assignability,
    CoverageCatchAll, CoverageDecline, CoverageGap, CoverageMention, CoverageSite,
    CoverageSiteRef, CoverageState, CoverageTypeName, DeclTyResolver, FieldAccess, FieldOwner,
    FieldSite, FileFieldUses, FileMatchCoverage, LocalDeclTy,
};
pub use lower::{lower_type_expr, ExportLowerer, Lowerer};
pub use stdlib::stdlib_signature;
pub use ty::{
    builtin_union, builtin_union_of_variant, builtin_unions, BuiltinUnion, BuiltinVariant, FnParam,
    ImportedTypeDecl, ModuleKey, ParamOwner, Primitive, RecordField, SymbolRef, Ty, UnionVariant,
};
pub use type_map::{IdentPattern, TypeMap};

pub use glyph_resolver::AlternativesKind;

/// A finite set of things a position accepts, with the kind of thing it is.
///
/// The values alone are ambiguous: `["read", "write"]` is a pair of literals a
/// string-literal union accepts and `["Pending", "Paid"]` is a pair of variant
/// names, and a consumer writing one of them has to put quotes round the first
/// and not round the second. The checker knows which at the moment it builds
/// the list, so the kind rides along instead of being guessed from the shape of
/// a second tool's answer about the declared type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub kind: AlternativesKind,
    pub values: Vec<String>,
}

impl Accepted {
    pub fn new(kind: AlternativesKind, values: Vec<String>) -> Self {
        Accepted { kind, values }
    }
}

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
        Ty::Never => "never".to_string(),
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
        Ty::StringLiteralUnion(values) => values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(" | "),
        // The bare name: the same declaration hovers identically whether it is
        // read locally or through any of the three import spellings.
        Ty::Imported { name, .. } => name.to_string(),
    }
}

/// The declaration a diagnostic is about, as an identity rather than as the
/// name its message happens to print.
///
/// Named for its first use, the union an exhaustiveness error is over. It also
/// carries the record an `UnknownField` is about: a field typo and a missing
/// arm both name a declaration the reader has to go to, and the three cases
/// below are the same three either way. Renaming the type to `DiagnosticDecl`
/// is a follow-up, not a behaviour change.
///
/// E0200 names the union and the variants it is missing inside one English
/// sentence, in backticks. An agent repairing the match needs both to make the
/// next call, and a regex over a message is a contract on prose the compiler
/// is free to rewrite: every improvement to a sentence then breaks a consumer
/// silently. This is `entity` (0.1.107) one field along. The message is
/// unchanged; the fields sit beside it.
///
/// Three cases, because a consumer acts on the difference: only the first two
/// have a declaration to go to at all, and only the second already knows which
/// module that is.
///
/// `Local` carries no module on purpose. The module half of a declaration in
/// the file being checked is counted from a root only the surface knows: the
/// project src root for `glyph check --json`, the file's own project for the
/// MCP tools. That is the same reason `decl_name` hands back a bare name and
/// lets its caller qualify it, and it is what keeps the union's identity and
/// the enclosing declaration's identity in one diagnostic spelled the same
/// way. The alternative, the file's own `module` header, is a second spelling
/// of the same address whenever the header and the path disagree (G172).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticDecl {
    /// Declared in the file the diagnostic is on. The module half is the
    /// caller's; see the type's own note.
    Local { name: String },
    /// Declared in another project module, under the module key imports name
    /// it by, which is the key the project already resolves it through.
    Imported { module: String, name: String },
    /// A prelude or stdlib union (`Result`, `Option`, `fs.ErrorKind`), keyed by
    /// the stdlib module that declares it (G231). `declared` is the name inside
    /// that module and `name` is the spelling a program writes, which differ
    /// for a type reached through a namespace: `std/fs::ErrorKind` is written
    /// `fs.ErrorKind`.
    Builtin {
        module: String,
        declared: String,
        name: String,
    },
}

impl DiagnosticDecl {
    /// The union's own name, which is what `glyph_variants` is called with.
    pub fn name(&self) -> &str {
        match self {
            DiagnosticDecl::Local { name }
            | DiagnosticDecl::Imported { name, .. }
            | DiagnosticDecl::Builtin { name, .. } => name,
        }
    }

    /// The module the union is declared in, given `this_module`: the module
    /// half of the file the diagnostic is on, as the calling surface counts
    /// it.
    ///
    /// A builtin answers with the stdlib module that declares it. That module
    /// is not a file of the project, and it is still the module the resolver
    /// registers the export under, the emitter writes the import from, and
    /// `import std/result { Result }` resolves through.
    pub fn module<'a>(&'a self, this_module: &'a str) -> Option<&'a str> {
        match self {
            DiagnosticDecl::Local { .. } => Some(this_module),
            DiagnosticDecl::Imported { module, .. } => Some(module),
            DiagnosticDecl::Builtin { module, .. } => Some(module),
        }
    }

    /// `module::name`, the identity `glyph_variants` reports for the same
    /// declaration.
    ///
    /// A builtin keys under the name it has inside its stdlib module rather
    /// than under the spelling written here, so `fs.ErrorKind` is
    /// `std/fs::ErrorKind` and not `std/fs::fs.ErrorKind`.
    pub fn declaration(&self, this_module: &str) -> Option<String> {
        if let DiagnosticDecl::Builtin {
            module, declared, ..
        } = self
        {
            return Some(format!("{module}::{declared}"));
        }
        self.module(this_module)
            .map(|module| format!("{module}::{}", self.name()))
    }

    /// `"declaration"` or `"builtin"`, matching the `kind` the MCP tools
    /// report for the same distinction on a match-coverage type end.
    pub fn kind(&self) -> &'static str {
        match self {
            DiagnosticDecl::Local { .. } | DiagnosticDecl::Imported { .. } => "declaration",
            DiagnosticDecl::Builtin { .. } => "builtin",
        }
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
    ///
    /// `union` and `missing_variants` are the same two facts as structure, for
    /// a consumer that has to act on them rather than read them (G195). They
    /// are carried beside the message, not lifted out of it: `type_name` and
    /// `missing` still render exactly the sentence they always did, including
    /// the quoting, which differs between a tagged union's backticked variants
    /// and a string-literal union's double-quoted values.
    ///
    /// `union` is `None` for a literal set written inline into a signature,
    /// which is declared nowhere and has nothing to address. The missing
    /// members are still known and still listed.
    #[error("non-exhaustive match on `{type_name}`: missing variants {missing}")]
    NonExhaustiveMatch {
        type_name: String,
        missing: String,
        /// The union the match is over, when it has a declaration or a builtin
        /// name to give.
        union: Option<DiagnosticDecl>,
        /// The unmentioned variants in declaration order, unquoted. For a
        /// string-literal union, whose members are values rather than tags,
        /// these are the values, the same way a coverage gap records them.
        missing_variants: Vec<String>,
        /// Every variant the union declares, in declaration order, unquoted.
        ///
        /// `missing_variants` says what is absent; this says what the set is.
        /// An agent writing the missing arms needs both, because the arms it
        /// adds have to sit in a `match` whose other arms it did not write,
        /// and reading the declaration back is a second call for a list the
        /// checker held while it computed the difference (G220).
        variants: Vec<String>,
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
        /// The values the declared type accepts, when it accepts a finite set
        /// the checker holds: a string-literal union's members. `None` for
        /// every other type, where the accepted set is not enumerable and an
        /// invented list would be a claim (D30).
        accepted: Option<Accepted>,
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

    /// A `mut` whose read and write straddle an `await`: the value written was
    /// read before a suspension point, so another task may have written in
    /// between and this write silently discards it.
    ///
    /// `read_at` points at the read, `span` at the write, because the fix is
    /// almost always to move the read after the `await` rather than to remove
    /// the write.
    #[error("`{place}` is read before an `await` and written after it, so a concurrent write in between is lost")]
    MutAcrossAwait {
        place: String,
        read_at: Span,
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
        /// Every field the record does declare, in declaration order.
        ///
        /// The checker resolved the record to decide the access was illegal,
        /// so the set was in hand at the moment this fired. It used to reach
        /// nobody: the JSON named the record only inside the sentence and
        /// carried no list, so an agent repairing a typo had to go read the
        /// declaration (G220).
        fields: Vec<String>,
        /// The record itself, as a declaration to address. `None` for a field
        /// set with no declaration behind it: an inline `{ a: string }`
        /// annotation, a variant's record payload, a stdlib type whose table
        /// the runtime ships.
        record: Option<DiagnosticDecl>,
        span: Span,
    },

    /// `r.name` where `r` is a `Record<K, V>` map.
    ///
    /// A map's keys are arbitrary, so the compiler cannot know the key is
    /// there, and typing the access as `V` states something it has not checked.
    /// The value is `undefined` when the key is absent, under a type saying it
    /// is a `V`, and nothing downstream reports it: a mistyped column name read
    /// off a database row compiled clean and rendered as the text
    /// `"undefined"`.
    ///
    /// `record.get(r, "name")` returns `Option<V>`, which is the same lookup
    /// with the absent case in the type where a `match` can reach it.
    #[error("`{type_name}` is a map, so `{field}` may not be there")]
    MapFieldAccess {
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
        /// The values the parameter accepts, when it accepts a finite set the
        /// checker holds. Same rule as `TypeMismatch::accepted`.
        accepted: Option<Accepted>,
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
    ///
    /// The stdlib's TypeScript wrappers are the one place a trailing argument is
    /// optional (`array.slice`, `string.slice`, `pad_start`, `pad_end`,
    /// `string.index_of`, `json.stringify`); the spec documents that boundary
    /// convention. Those calls escape this check only because the callee types
    /// as `Unknown`, so adding one of them to `stdlib_fn_ty` means teaching this
    /// check a minimum and maximum arity first.
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

    /// A `match` whose scrutinee carries no variant set to reason about — a
    /// record, an imported type, anything the tag-, array-, bool- and
    /// value-domain checks all declined — and whose arms can every one of them
    /// fail, with no catch-all behind them. There is no tag to count, but
    /// nothing makes the match produce a value either: the emitted chain falls
    /// off its end and throws. A field pattern that tests a value (D44,
    /// `{ x: 0, y: y }`) is the shape that makes this reachable, so the check
    /// is scoped to a match that contains one.
    #[error("non-exhaustive match: every arm can fail and no arm is a catch-all")]
    NonExhaustiveFieldMatch { span: Span },

    /// D45: `Nullable<T>` where `T` is itself `Nullable` or `Option`. The first
    /// would give two states one runtime spelling (`null`); the second would
    /// put a tagged object under a null-tolerant field, which is the ambiguity
    /// the type exists to avoid. `inner` is the argument as it was written.
    #[error("`Nullable<{inner}>`: the argument of `Nullable` may not itself be `Nullable` or `Option`")]
    NullableNested { inner: String, span: Span },

    /// G217. A type's name standing where a value is wanted. `return Order {
    /// id: "a", total: 1 }` is the TypeScript-adjacent guess for constructing a
    /// record, and Glyph has no such form: the value is the record literal on
    /// its own and the name belongs on the `let`, the parameter, or the return
    /// type. Before this the name typed as `Unknown`, the braces after it
    /// parsed as a second statement, and the only thing the compiler said about
    /// the whole program was an `E0108 unreachable code` warning on an exit-0
    /// build.
    ///
    /// `construction` is how a value of this type is actually written, read off
    /// the declaration the name reaches, so the message carries the repair
    /// rather than only the refusal. A type name that is the object of a member
    /// access is not this error: `Order.parse(json)` and `Order.is(v)` are the
    /// descriptor forms, where naming the type is the point.
    #[error("`{name}` is a type, not a value; {construction}")]
    TypeNameAsValue {
        name: String,
        construction: String,
        span: Span,
    },

    /// A `@redact fields: [...]` annotation (D24) names a field the type does
    /// not have — a typo or a renamed field. Redaction is type-level
    /// enforcement, so an unknown field name is a hard error: it would silently
    /// mask nothing. Only record types have redactable fields.
    #[error("`@redact` names field `{field}`, which `{type_name}` does not have")]
    RedactUnknownField {
        field: String,
        type_name: String,
        /// The declaration this error is about (see `decl_name`). Carried
        /// separately from `type_name`, which is part of the message and
        /// answers a different question: which type lacks the field.
        decl: String,
        span: Span,
    },

    /// An `@<name>` annotation the compiler does not recognize (D27). Unknown
    /// annotations are a hard error, not a silent no-op: a typo like `@puer`
    /// would otherwise carry no meaning while looking like it did. The recognized
    /// v1 set is `@example`, `@doc`, `@redact`, `@open`, `@pure`, `@public`.
    #[error("unknown annotation `@{name}`")]
    UnknownAnnotation {
        name: String,
        /// The declaration the annotation decorates (see `decl_name`).
        decl: String,
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

    /// A `match` arm whose bare head is a PascalCase name that is not a variant
    /// of the scrutinee's tagged union. Glyph classifies a bare arm head by
    /// shape (the same rule the resolver uses): a lowercase/underscore-led name
    /// (`x`, `_rest`) is a fresh binding, while a PascalCase name (`Loading`)
    /// is a variant *reference*. A PascalCase head that names no variant of
    /// this union is a typo (`Loadign` for `Loading`) or a wrong-union variant,
    /// not a binding. Escalating it is a verifiability win: left as a binding
    /// it would silently act as an irrefutable catch-all, masking a genuine
    /// missing variant and misrouting values at runtime. `suggestion` is the
    /// nearest real variant by edit distance, when one is close enough.
    #[error("`{name}` is not a variant of `{union}`{}", .suggestion.as_deref().map(|s| format!("; did you mean `{s}`?")).unwrap_or_default())]
    UnknownVariantPattern {
        union: String,
        name: String,
        suggestion: Option<String>,
        span: Span,
    },

    /// `await` written outside an `async fn`. Glyph has no user-visible
    /// `Promise`, so `await` is only meaningful inside a callable declared
    /// `async`; anywhere else the emitted TypeScript is rejected by `tsc`
    /// (TS1308) with no Glyph-level explanation. The innermost enclosing
    /// callable decides: a synchronous lambda nested inside an `async fn` is
    /// its own context, matching TypeScript.
    #[error("`await` is only valid inside an `async fn`")]
    AwaitOutsideAsyncFn { span: Span },

    /// A `match` arm that yields no value while the `match` itself is used as
    /// a value (bound by a `let`, assigned with `mut`, returned, or the value
    /// of a typed callable's body). An arm body that is an empty block, or a
    /// block whose last statement is not an expression and does not diverge,
    /// lowers to `case X: { break; }` — the binding is then never assigned and
    /// the value is `undefined` at run time with no TypeScript error.
    #[error("this `match` arm produces no value, but the `match` is used as a value")]
    MatchArmProducesNoValue { span: Span },
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
            TypeError::MutAcrossAwait { span, .. } => *span,
            TypeError::OwnedNotConsumed { span, .. } => *span,
            TypeError::OwnedUsedAfterMove { span, .. } => *span,
            TypeError::NonExhaustiveArrayMatch { span, .. } => *span,
            TypeError::NonExhaustiveBoolMatch { span, .. } => *span,
            TypeError::NonExhaustiveValueMatch { span, .. } => *span,
            TypeError::NonExhaustiveFieldMatch { span } => *span,
            TypeError::NullableNested { span, .. } => *span,
            TypeError::TypeNameAsValue { span, .. } => *span,
            TypeError::RedactUnknownField { span, .. } => *span,
            TypeError::UnknownAnnotation { span, .. } => *span,
            TypeError::UnknownField { span, .. } => *span,
            TypeError::MapFieldAccess { span, .. } => *span,
            TypeError::ArgumentTypeMismatch { span, .. } => *span,
            TypeError::ArgumentCountMismatch { span, .. } => *span,
            TypeError::MutateConst { span, .. } => *span,
            TypeError::ComponentMultipleParams { span, .. } => *span,
            TypeError::OwnedAliased { span, .. } => *span,
            TypeError::UnreachableMatchArm { span } => *span,
            TypeError::UnusedResult { span } => *span,
            TypeError::UnknownVariantPattern { span, .. } => *span,
            TypeError::AwaitOutsideAsyncFn { span } => *span,
            TypeError::MatchArmProducesNoValue { span } => *span,
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
            TypeError::MutAcrossAwait { .. } => "E0225",
            TypeError::OwnedNotConsumed { .. } => "E0206",
            TypeError::OwnedUsedAfterMove { .. } => "E0207",
            TypeError::NonExhaustiveArrayMatch { .. } => "E0208",
            TypeError::NonExhaustiveBoolMatch { .. } => "E0209",
            TypeError::NonExhaustiveValueMatch { .. } => "E0218",
            TypeError::NonExhaustiveFieldMatch { .. } => "E0226",
            TypeError::NullableNested { .. } => "E0227",
            TypeError::TypeNameAsValue { .. } => "E0228",
            TypeError::RedactUnknownField { .. } => "E0219",
            TypeError::UnknownAnnotation { .. } => "E0221",
            TypeError::UnknownField { .. } => "E0210",
            TypeError::MapFieldAccess { .. } => "E0224",
            TypeError::ArgumentTypeMismatch { .. } => "E0211",
            TypeError::MutateConst { .. } => "E0212",
            TypeError::ArgumentCountMismatch { .. } => "E0213",
            TypeError::ComponentMultipleParams { .. } => "E0214",
            TypeError::OwnedAliased { .. } => "E0215",
            TypeError::UnreachableMatchArm { .. } => "E0216",
            TypeError::UnusedResult { .. } => "E0217",
            TypeError::UnknownVariantPattern { .. } => "E0220",
            TypeError::AwaitOutsideAsyncFn { .. } => "E0222",
            TypeError::MatchArmProducesNoValue { .. } => "E0223",
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
            TypeError::MapFieldAccess { .. } => {
                "Use `record.get(map, \"key\")`, which returns `Option<V>` so the absent case has somewhere to go. `record.has` tests for the key alone."
            }
            TypeError::NullableNested { .. } => {
                "Write `Nullable` over the plain type (`Nullable<int>`) for a field the wire sends as null, or `Option<T>` alone inside the program; `nullable.to_option` and `nullable.from_option` convert between them."
            }
            TypeError::TypeNameAsValue { .. } => {
                "Write the value in its own form and move the type's name to the annotation on the `let`, the parameter, or the return type."
            }
            TypeError::OwnedRequiresResourceType { .. } => {
                "`owned` is only for `resource`-marked types. Drop `owned`, or mark the type `resource`."
            }
            TypeError::MutAcrossAwait { .. } => {
                "Move the read after the `await`, so the value written is the one that is current when it is written."
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
            TypeError::NonExhaustiveFieldMatch { .. } => {
                "Add an `else` arm, or an arm whose pattern always matches. A field pattern that tests a value can fail, so an arm carrying one covers nothing by itself."
            }
            TypeError::RedactUnknownField { .. } => {
                "Check the field name for a typo. `@redact` lists fields of the record type it decorates, and only record types have redactable fields."
            }
            TypeError::UnknownAnnotation { .. } => {
                "Check the annotation name for a typo. The recognized annotations are `@example`, `@doc`, `@redact`, `@open`, `@pure`, and `@public`."
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
            TypeError::UnknownVariantPattern { .. } => {
                "Check the name for a typo, or add the variant to the union. A lowercase name would be a binding; a PascalCase name is read as a variant."
            }
            TypeError::AwaitOutsideAsyncFn { .. } => {
                "Mark the enclosing function `async fn`, or call a non-async function here."
            }
            TypeError::MatchArmProducesNoValue { .. } => {
                "End the arm with an expression, or `return` from it. `X => {}` is a no-op only where the `match` is a statement."
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
            TypeError::NullableNested { .. } => Some(
                "`Nullable<T>` is `T | null` at run time (D45): null is the whole of its absent state, so a nested `Nullable` or `Option` has no second thing to say.",
            ),
            TypeError::TypeNameAsValue { .. } => Some(
                "Glyph has no `TypeName { ... }` construction form. A type name appears in an annotation; the only place it stands in an expression is as the receiver of its own descriptor, `T.parse` and `T.is`.",
            ),
            TypeError::NonExhaustiveFieldMatch { .. } => Some(
                "Coverage is proved over a set of tags, not over a product of fields (D44), so two field tests are never read as leaving nothing between them.",
            ),
            TypeError::UnknownVariantPattern { .. } => Some(
                "A bare PascalCase arm head is a variant reference, not a binding (D9). Treating a non-variant as a binding would hide it as a silent catch-all.",
            ),
            TypeError::AwaitOutsideAsyncFn { .. } => Some(
                "Glyph has no user-visible `Promise`: an `async fn -> T` is awaited to a `T`, and `await` only appears inside one.",
            ),
            TypeError::MatchArmProducesNoValue { .. } => Some(
                "A value-position arm lowers to `case X: { break; }` when it yields nothing, so the value would be `undefined` at run time.",
            ),
            _ => None,
        }
    }

    /// The top-level declaration this error is *about*, when the checker knew
    /// it at the point it raised the error and the error's own span cannot be
    /// used to find it again.
    ///
    /// Both annotation errors are that shape. An annotation's text sits in the
    /// gap between two declaration spans: annotations are parsed before the
    /// keyword, and a `Decl` span starts at the keyword, so a containment walk
    /// over top-level items matches nothing for an offset inside `@puer`.
    /// The checker had the declaration by reference either way, so the name is
    /// carried on the error rather than re-derived from a position that cannot
    /// yield it. Consumers prefer this over their own span walk; `None` means
    /// the walk is the answer, not that there is no declaration.
    pub fn decl_name(&self) -> Option<&str> {
        match self {
            TypeError::UnknownAnnotation { decl, .. } => Some(decl),
            TypeError::RedactUnknownField { decl, .. } => Some(decl),
            _ => None,
        }
    }

    /// The union this error is about, when it is about one.
    ///
    /// `None` is absence of a relation, not "the checker did not look": an
    /// error that concerns no union answers nothing here, and so does a match
    /// over a literal set with no declaration behind it. An identity guessed
    /// from a message would be worse than either.
    pub fn union(&self) -> Option<&DiagnosticDecl> {
        match self {
            TypeError::NonExhaustiveMatch { union, .. } => union.as_ref(),
            _ => None,
        }
    }

    /// The variants this error reports unmentioned, in declaration order and
    /// unquoted, when it reports any.
    ///
    /// `None` rather than an empty list for every other error: an empty list
    /// would read as "nothing is missing", which is a claim, and this is the
    /// absence of one.
    pub fn missing_variants(&self) -> Option<&[String]> {
        match self {
            TypeError::NonExhaustiveMatch {
                missing_variants, ..
            } => Some(missing_variants),
            _ => None,
        }
    }

    /// The type the checker required here, as it displays it.
    ///
    /// Only the errors that compare two types answer. A wrong argument *count*
    /// is not a type comparison, so E0213 answers `None` here and keeps its
    /// two numbers in its sentence: a consumer reading `expected` as a type
    /// must never be handed a count under the same key (G220).
    pub fn expected(&self) -> Option<&str> {
        match self {
            TypeError::TypeMismatch { expected, .. }
            | TypeError::ArgumentTypeMismatch { expected, .. } => Some(expected),
            // `?` propagates an `E` to the enclosing function's `Result<_, E>`;
            // the expected side is that `E`.
            TypeError::QuestionErrorTypeMismatch { expected, .. } => Some(expected),
            _ => None,
        }
    }

    /// The type the checker found here, as it displays it.
    ///
    /// Answers for one more class than `expected` does: an error that names a
    /// single offending type and states its requirement in prose has an actual
    /// and no expected, and dropping the actual because there is no pair would
    /// lose a fact the checker held.
    pub fn actual(&self) -> Option<&str> {
        match self {
            TypeError::TypeMismatch { found, .. }
            | TypeError::ArgumentTypeMismatch { found, .. }
            | TypeError::QuestionErrorTypeMismatch { found, .. }
            | TypeError::QuestionOnNonResult { found, .. } => Some(found),
            // "a resource type" is a rule, not a type, so there is no
            // `expected` to pair with; the type in hand is still reported.
            TypeError::OwnedRequiresResourceType { ty, .. } => Some(ty),
            _ => None,
        }
    }

    /// The declaration this error is *about*, when it is about one other than
    /// the declaration it sits in.
    ///
    /// The enclosing declaration is `entity`; this is the symbol at fault. For
    /// a non-exhaustive match it is the union, for a field typo the record. An
    /// error whose symbol at fault *is* the enclosing declaration answers
    /// `None` rather than repeating `entity` under a second key.
    pub fn cause(&self) -> Option<&DiagnosticDecl> {
        match self {
            TypeError::NonExhaustiveMatch { union, .. } => union.as_ref(),
            TypeError::UnknownField { record, .. } => record.as_ref(),
            _ => None,
        }
    }

    /// What may legally stand where the offending thing stands, when the
    /// checker holds a finite list of it.
    ///
    /// Three shapes, one meaning: the record's own fields against a field
    /// typo, the values a string-literal union accepts against a mismatch, the
    /// one variant a mistyped pattern head most likely meant. `None` is not
    /// "anything goes": it is the checker having no enumerable set, which is
    /// the ordinary case for a type.
    pub fn alternatives(&self) -> Option<Vec<String>> {
        match self {
            TypeError::UnknownField { fields, .. } if !fields.is_empty() => {
                Some(fields.clone())
            }
            TypeError::TypeMismatch { accepted, .. }
            | TypeError::ArgumentTypeMismatch { accepted, .. } => {
                accepted.as_ref().map(|a| a.values.clone())
            }
            TypeError::UnknownVariantPattern { suggestion, .. } => {
                suggestion.as_ref().map(|s| vec![s.clone()])
            }
            _ => None,
        }
    }

    /// What kind of thing `alternatives` holds here: variant names, the
    /// contents of string literals, a record's fields.
    ///
    /// Answers wherever `alternatives` answers and nowhere else, so a consumer
    /// reads the two keys as one fact. Read off the error variant rather than
    /// inferred from the names, which cannot be done: `Paid` and `read` are
    /// both bare identifiers on the wire.
    pub fn alternatives_kind(&self) -> Option<AlternativesKind> {
        match self {
            TypeError::UnknownField { fields, .. } if !fields.is_empty() => {
                Some(AlternativesKind::Fields)
            }
            TypeError::TypeMismatch { accepted, .. }
            | TypeError::ArgumentTypeMismatch { accepted, .. } => {
                accepted.as_ref().map(|a| a.kind)
            }
            TypeError::UnknownVariantPattern { suggestion, .. } => {
                suggestion.as_ref().map(|_| AlternativesKind::Variants)
            }
            _ => None,
        }
    }

    /// The other names a reader of this diagnostic has to know about, when the
    /// error carries a set of them.
    ///
    /// Today that is the union's whole variant list on a non-exhaustive match:
    /// `missing_variants` is the gap and this is the set it was taken from, so
    /// an agent writing the arms sees the shape the `match` has to end up in
    /// without a second call.
    pub fn related(&self) -> Option<Vec<String>> {
        match self {
            TypeError::NonExhaustiveMatch { variants, .. } if !variants.is_empty() => {
                Some(variants.clone())
            }
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
