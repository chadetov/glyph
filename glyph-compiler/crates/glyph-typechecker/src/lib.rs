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

pub mod absent;
pub mod assign;
pub mod async_slot;
pub mod builtin_members;
mod builtin_methods;
pub mod concurrency;
pub mod deprecated;
pub mod lower;
pub mod mutation;
pub mod owned;
pub mod pending;
pub mod reassign;
pub mod stdlib;
mod stdlib_member_classes;
mod tail;
pub mod ty;
pub mod type_map;

pub use assign::{
    alias_target, assign_types, assign_types_with_coverage, assign_types_with_relations,
    assign_types_with_resolver, assignability, const_binding_ty, decl_signature_ty,
    direct_type_decl, imported_decl_chain_end, unannotated_const_ty,
    imported_string_literal_union_values,
    prelude_app, prelude_container, declared_record_fields, expand_transparent, resolve_alias_chain,
    split_type_app,
    unowned_params_at_relation, Assignability,
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

use std::borrow::Cow;
pub use builtin_members::BuiltinReceiver;
pub use absent::AbsentRepair;
pub use mutation::MutationSite;

/// What a `Record` key is, for the note and help of a refusal made by the key
/// invariance rule (G373). Mirrors the checker's key domains, without the
/// identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// `string`, or a second name for it.
    String,
    /// `number`, `int` or `bigint`, which the checker does not tell apart yet.
    Number,
    /// A string-literal union, or a single literal.
    Literals,
    /// A refined type (D39).
    Refined,
    /// A type parameter of the enclosing declaration.
    Param,
}

/// The place a `mut` writes, on a value refused there (G501): the place as
/// written, the type it holds, and where that type came from, so the note can
/// say why the place holds that type rather than only that it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutPlace {
    /// The target as the program writes it: `x`, `r.n`, `v[0]`.
    pub place: String,
    /// The type the place holds, as the checker displays it.
    pub holds: String,
    pub origin: MutPlaceOrigin,
}

/// Where a `mut` target's type comes from (D66).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutPlaceOrigin {
    /// A name with a written annotation (a `let` or a parameter), declared at
    /// this byte offset.
    Declared { at: u32 },
    /// An unannotated `let`: the type of its initializer, which starts at
    /// this byte offset.
    Initializer { at: u32 },
    /// A record or interface field, declared with that type.
    Field { field: String },
    /// An element of an array (`map` false) or a value of a `Record` map
    /// (`map` true), whose type is this container.
    Element { container: String, map: bool },
}

impl MutPlace {
    /// The note, with the line a declaration or initializer is on read off
    /// `source`, which is the file the error is in.
    pub fn note(&self, source: &str) -> String {
        let line = |at: u32| {
            let at = (at as usize).min(source.len());
            source.as_bytes()[..at].iter().filter(|b| **b == b'\n').count() + 1
        };
        let article = match self.holds.chars().next() {
            Some('a' | 'e' | 'i' | 'o' | 'u' | 'A' | 'E' | 'I' | 'O' | 'U') => "an",
            _ => "a",
        };
        let head = format!("`{}` holds {article} `{}`", self.place, self.holds);
        match &self.origin {
            MutPlaceOrigin::Declared { at } => format!("{head}: declared at line {}.", line(*at)),
            MutPlaceOrigin::Initializer { at } => format!(
                "{head}: the type of its initializer at line {}, and a `mut` does not change a binding's type.",
                line(*at)
            ),
            MutPlaceOrigin::Field { field } => {
                format!("{head}: the type field `{field}` is declared with.")
            }
            MutPlaceOrigin::Element { container, map: false } => {
                format!("{head}: the element type of `{container}`.")
            }
            MutPlaceOrigin::Element { container, map: true } => {
                format!("{head}: the value type of `{container}`.")
            }
        }
    }
}

/// The help on a value a `mut` writes that its place does not hold (G501).
pub const MUT_PLACE_HELP: &str =
    "Write a value of that type, or bind the new value to a new name.";

/// The facts a `Record`-against-`Record` refusal carries when the two keys are
/// different key domains (G373), so its note and help are computed from the
/// program rather than from example names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordKeyMismatch {
    /// The value's key, as the program spells it where the checker can read
    /// the annotation, else as the checker displays it.
    pub found_key: String,
    /// The declared key, spelled the same way.
    pub expected_key: String,
    pub found_kind: KeyKind,
    pub expected_kind: KeyKind,
    /// For two literal sets: the members the value's key has that the
    /// declared key does not, in the value's order.
    pub extra: Vec<String>,
    /// For two literal sets: the members the declared key has that the
    /// value's key does not, in the declared order.
    pub missing: Vec<String>,
    /// The declared type, written so an `is` arm or `json.parse` can check it,
    /// when it can be checked and the checker knows how the program spells it.
    pub declared: Option<String>,
    /// Whether the declared type can be checked by an `is` arm or
    /// `json.parse` at all: its key is closed and no type parameter is in it.
    pub checkable: bool,
    /// The declared `Record` with the value's key in place of its own, when the
    /// declared type is written out as `Record<K, V>`: what the position
    /// would say to keep the value's key.
    pub declared_with_found_key: Option<String>,
}

impl RecordKeyMismatch {
    /// One side is `string` (or a second name for it) and the other is not.
    /// Answers the string side's key and the other side's, in that order.
    fn open_and_closed(&self) -> Option<(&str, &str, KeyKind)> {
        match (self.found_kind, self.expected_kind) {
            (KeyKind::String, k @ (KeyKind::Literals | KeyKind::Refined | KeyKind::Param)) => {
                Some((&self.found_key, &self.expected_key, k))
            }
            (k @ (KeyKind::Literals | KeyKind::Refined | KeyKind::Param), KeyKind::String) => {
                Some((&self.expected_key, &self.found_key, k))
            }
            _ => None,
        }
    }

    /// The note: why the two keys are refused, in the program's names.
    pub fn note(&self) -> String {
        let head = "a `Record`'s key type must match exactly.";
        if let Some((open, closed, kind)) = self.open_and_closed() {
            let lack = if kind == KeyKind::Literals {
                " or lack one of its members"
            } else {
                ""
            };
            return format!(
                "{head} A `Record` keyed by `{open}` may hold keys outside `{closed}`{lack}, and a `Record` keyed by `{closed}` passed on as one keyed by `{open}` can be written a key outside `{closed}`."
            );
        }
        if self.found_kind == KeyKind::Literals && self.expected_kind == KeyKind::Literals {
            let list = |xs: &[String]| {
                xs.iter()
                    .map(|v| format!("`{v:?}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let mut parts = Vec::new();
            if !self.extra.is_empty() {
                parts.push(format!(
                    "`{}` has {}, which `{}` does not",
                    self.found_key,
                    list(&self.extra),
                    self.expected_key
                ));
            }
            if !self.missing.is_empty() {
                parts.push(format!(
                    "`{}` has {}, which `{}` does not",
                    self.expected_key,
                    list(&self.missing),
                    self.found_key
                ));
            }
            return format!(
                "{head} A `Record` keyed by a string-literal union holds a value for every member and no other key, and {}.",
                parts.join(", and ")
            );
        }
        format!(
            "{head} `{}` and `{}` are different key types, so a `Record` keyed by one is not a `Record` keyed by the other.",
            self.found_key, self.expected_key
        )
    }

    /// The help, when the checker can say what to write. `None` falls back to
    /// the mismatch's ordinary help.
    pub fn help(&self) -> Option<String> {
        let check = self
            .declared
            .as_ref()
            .map(|d| format!("Check it with `is {d}` or `json.parse<{d}>`."));
        match (self.found_kind, self.expected_kind) {
            // Narrowing to a closed key: the value has to be checked.
            (KeyKind::String, KeyKind::Literals | KeyKind::Refined) => check,
            (KeyKind::String, KeyKind::Param) => Some(format!(
                "A `Record` keyed by `{}` cannot be checked against the type parameter `{}`. Take the value as a `Record` keyed by `{}` where it comes in.",
                self.found_key, self.expected_key, self.expected_key
            )),
            // Widening a closed key: keep it.
            (k @ (KeyKind::Literals | KeyKind::Refined | KeyKind::Param), KeyKind::String) => {
                let keep = match &self.declared_with_found_key {
                    Some(d) => format!("Keep the key `{}`: declare this position `{d}`", self.found_key),
                    None => format!(
                        "Keep the key `{}`: declare this position as a `Record` keyed by `{}`",
                        self.found_key, self.found_key
                    ),
                };
                Some(if k == KeyKind::Param {
                    format!("{keep}.")
                } else {
                    format!(
                        "{keep}, or make the function generic over its key (`<K: string>`) and key the `Record` by `K`."
                    )
                })
            }
            (KeyKind::Literals, KeyKind::Literals) if self.missing.is_empty() => check,
            (KeyKind::Literals, KeyKind::Literals) => Some(match &self.declared_with_found_key {
                Some(d) => format!(
                    "Give the value a key for every member of `{}`, or declare this position `{d}`.",
                    self.expected_key
                ),
                None => format!("Give the value a key for every member of `{}`.", self.expected_key),
            }),
            _ => None,
        }
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
            ..
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
        // D51. The words, not an invented `Promise<T>` or `Pending<T>`: no
        // such type can be written in Glyph, and a hover that showed one
        // would teach a spelling that does not compile.
        Ty::Pending(inner) => format!("a pending `{}`", display_ty(inner)),
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
    #[error("type mismatch: expected `{expected}`, found `{found}`{}{}", missing_fields_suffix(.missing_fields), no_common_field_suffix(*.no_common_field, .expected))]
    TypeMismatch {
        expected: String,
        found: String,
        /// The values the declared type accepts, when it accepts a finite set
        /// the checker holds: a string-literal union's members. `None` for
        /// every other type, where the accepted set is not enumerable and an
        /// invented list would be a claim (D30).
        accepted: Option<Accepted>,
        /// The required fields of the declared record that the value's record
        /// does not have, in the declared record's order. Empty unless the
        /// value is a structural record and at least one is absent.
        ///
        /// "found `record`" said a record was refused and not which field it
        /// lacked, where `tsc` names the property. The checker read the
        /// declared field set to decide the refusal, so the names were in hand.
        missing_fields: Vec<String>,
        /// The declared record's fields are all optional and the value shares
        /// none of them, which is TypeScript's weak-type rule (TS2559) and the
        /// whole of the reason for the refusal. False everywhere else.
        no_common_field: bool,
        /// Both types are `Record` maps whose keys are different key domains
        /// (G373). `None` everywhere else.
        record_key: Option<Box<RecordKeyMismatch>>,
        /// The value is refused only for holding `unknown` where another type
        /// is declared (G325), so the help names the ways to check one.
        unknown_value: bool,
        /// The place a `mut` writes, when the value is refused there (G501).
        /// `None` at every other site.
        place: Option<Box<MutPlace>>,
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

    /// An `is` pattern whose type is, or holds, a type parameter of the
    /// enclosing `fn` or `component` (G362). A type parameter is erased at
    /// run time, so there is nothing to check. Reported here rather than
    /// only by the emitter, which runs once the checker is clean, so every
    /// such pattern in a file is reported in one run (record 194), and the
    /// arm's name is left untyped so nothing else is reported about it.
    /// `container` is the innermost generic application holding the
    /// parameter, `None` when the pattern is the parameter itself.
    #[error("{}", type_parameter_pattern_message(container.as_deref(), param, owner))]
    TypeParameterPattern {
        container: Option<String>,
        param: String,
        owner: String,
        span: Span,
    },

    /// A `mut` whose read and write straddle an `await`: the value written was
    /// read before a suspension point, so another task may have written in
    /// between and this write silently discards it.
    ///
    /// `read_at` points at the read, `span` at the write, because the fix is
    /// almost always to move the read after the `await` rather than to remove
    /// the write. `via` is empty, or ` (read through `array.get(c, 0)`)` when
    /// the read was spelled as a stdlib lookup rather than as the place.
    #[error("`{place}` is read before an `await` and written after it, so a concurrent write in between is lost{via}")]
    MutAcrossAwait {
        place: String,
        via: String,
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
    /// parameter, a namespace, an interop value) is left unchecked, so the
    /// check never produces a false positive.
    ///
    /// A built-in receiver is checked the same way (G288): an `Array<T>`, a
    /// `string` or a `number` has the fixed member set TypeScript's library gives it, and
    /// `xs.lenght` is this error naming `Array<string>`, with every member in
    /// `fields` and the nearest one in `suggestion`.
    ///
    /// A record's field typo carries the nearest declared field in
    /// `suggestion` the same way: `o.totl` on `{ total: number }` says "did
    /// you mean `total`?".
    ///
    /// A prelude `Option` or `Result` is checked against its member set
    /// (G311, record 87): a `Result`'s members are `map` and `map_err`, an
    /// `Option` has none, and `tag` and `value` are the runtime encoding
    /// (D49), not members. `tagged` is set, the message names the
    /// member set, and `fields` holds it.
    ///
    /// A field *written* in an object literal is the same fact (D57, G273):
    /// `let b: Opt = { name: "b", extra: 1 }` is this error at the key
    /// `extra`, wherever the checker relates a written literal to a declared
    /// record. `literal` is set then, holding every key the literal writes
    /// itself, so the help and `glyph fix` can tell a rename that would write
    /// a field twice from one that would not.
    #[error("type `{type_name}`{} has no field `{field}`{}{}", .second_name_for.as_deref().map(|t| format!(" (a second name for `{t}`)")).unwrap_or_default(), .tagged.as_ref().map(|t| tagged_member_set_tail(t.container)).unwrap_or_default(), .suggestion.as_deref().map(|s| format!("; did you mean `{s}`?")).unwrap_or_default())]
    UnknownField {
        field: String,
        type_name: String,
        /// The declaration the fields come from, when `type_name` reached it
        /// through a chain of second names (D46): `Doc` for a `p: Page` with
        /// `type Page = Doc`. Spelled as the site's scope spells it (`Doc`
        /// under `import a { Doc }`, `a.Doc` under `import a`). `None` when
        /// no chain was followed.
        second_name_for: Option<String>,
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
        /// Set when the receiver is a built-in JavaScript value rather than a
        /// record: there is no declaration to add a field to, and the help
        /// says what the value is instead.
        builtin: Option<BuiltinReceiver>,
        /// Set when the receiver is the prelude `Option` or `Result`, whose
        /// member set is fixed by the language rather than declared.
        tagged: Option<TaggedMember>,
        /// The nearest declared name by edit distance, when one is close
        /// enough to be a plausible typo: a record's field or a built-in
        /// receiver's member, by the same threshold either way.
        suggestion: Option<String>,
        /// Set when the field is a key written in an object literal rather
        /// than a read (D57): the keys that literal writes explicitly, in the
        /// order written. Fields that arrive through a spread are not in it.
        /// `None` for a member read, a destructuring pattern and a JSX
        /// attribute.
        literal: Option<Vec<String>>,
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
    #[error("argument type mismatch: expected `{expected}`, found `{found}`{}{}", missing_fields_suffix(.missing_fields), no_common_field_suffix(*.no_common_field, .expected))]
    ArgumentTypeMismatch {
        expected: String,
        found: String,
        /// The values the parameter accepts, when it accepts a finite set the
        /// checker holds. Same rule as `TypeMismatch::accepted`.
        accepted: Option<Accepted>,
        /// The parameter record's required fields the argument lacks. Same
        /// rule as `TypeMismatch::missing_fields`; for a JSX component element these
        /// are the props it does not write.
        missing_fields: Vec<String>,
        /// Same rule as `TypeMismatch::no_common_field`.
        no_common_field: bool,
        /// True for a component element that leaves out a required `children`
        /// while writing children between its tags. Those children are
        /// `createElement` arguments after the props object, so they never
        /// fill the field; the help says so and names the two spellings that
        /// compile. False everywhere else.
        children_between_tags: bool,
        /// True when the parameter is `sqlite.from_trusted`'s (G387, D52), so
        /// the help can say that a constant query needs no escape at all. Any
        /// other `taint.Trusted` slot gets the help without the SQL clause.
        sql_escape: bool,
        /// Same rule as `TypeMismatch::record_key`.
        record_key: Option<Box<RecordKeyMismatch>>,
        /// Same rule as `TypeMismatch::unknown_value`.
        unknown_value: bool,
        /// The help for a call argument refused because it is the wrong kind
        /// of function (D40 as amended for 0.1.159): a sync function where an
        /// async one is declared, or an async callback where a sync one that
        /// keeps a value is. Computed from the program by `async_slot`, so it
        /// names the callee and the value. `None` everywhere else.
        kind_help: Option<String>,
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

    /// A call or a type application writes more or fewer type arguments than
    /// the declaration it names has type parameters: `id<number, bool>(x)` on
    /// `fn id<T>`, `one<number>(x)` on a non-generic `fn one`, or
    /// `Box<number, string>` on `type Box<T>`. Raised only where the checker
    /// reads the declaration's parameter list: a declaration of this module,
    /// a project sibling's, a prelude type, and a stdlib function whose type
    /// parameters a written argument fills. TypeScript refuses the same
    /// programs (TS2558 for a call, TS2314 and TS2315 for a type).
    ///
    /// Its own code rather than E0213's: E0213 counts the values a call
    /// passes, and one code meaning two counts is one an agent dispatching on
    /// codes cannot tell apart.
    ///
    /// `value_of` is set when the callee is a value read off a stdlib
    /// function whose type parameter only a type argument written on a direct
    /// call fills (`let p = json.parse` then `p<User>(s)`, record 88): the
    /// function as the program spelled it, and the written type arguments.
    /// It changes the help, never the message.
    #[error("wrong number of type arguments: `{name}` takes {expected}, found {found}")]
    TypeArgumentCountMismatch {
        name: String,
        expected: usize,
        found: usize,
        value_of: Option<(String, String)>,
        span: Span,
    },

    /// A method of a JavaScript array, string or number read without being
    /// called: `let up = s.toUpperCase`, or `xs.map(s.trim)` (G312). The
    /// emitted read hands on the bare function, and an ES module is strict,
    /// so when it is called later it runs with `this` undefined and throws a
    /// TypeError. `tsc` accepts the read, so nothing else reports it before
    /// it runs.
    ///
    /// A call is never refused (`s.toUpperCase()`, `mut xs.push(x)`), and
    /// neither is a data property (`xs.length`): only a member the generated
    /// method table holds, outside a callee's position.
    #[error("method `{method}` of `{type_name}` is read without being called")]
    ///
    /// `.bind`, `.call` and `.apply` on the read are refused with it (record
    /// 88): what they return is untyped in Glyph, and a lambda says the same
    /// thing checked. `call` is the call as the help writes it back
    /// (`s.toUpperCase()`), and `lambda` the lambda that wraps it, when the
    /// method takes no argument and its result has a type.
    UncalledBuiltinMethod {
        method: String,
        type_name: String,
        builtin: BuiltinReceiver,
        call: String,
        lambda: Option<String>,
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

    /// A `match` with literal-value arms and no catch-all over a named type
    /// declared in a module this compile cannot read: a sibling that is not
    /// in the project being checked, an npm package whose types were never
    /// materialized, or any import in the one-file playground. The type may be
    /// a bounded literal union the arms cover or a plain `string` they cannot,
    /// and the checker has no way to tell, so it neither claims the match is
    /// open (E0218, whose advice is an `else`) nor accepts it unchecked.
    #[error(
        "cannot check this match for exhaustiveness: `{type_name}` is declared in `{module}`, and that declaration is not visible to the checker"
    )]
    UnseenMatchType {
        type_name: String,
        module: String,
        span: Span,
    },

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

    /// G304, D45: a member read through a `Nullable<R>` that does not handle
    /// the null. Two forms, one code (E0235):
    ///
    /// - `n.f` with the plain dot, where `n` may be null. TypeScript refuses it
    ///   too (TS18047).
    /// - the value of an optional chain over one (`n?.f`, `n?.a.b`) standing
    ///   anywhere but the left of `??` or an operand of `==`/`!=`. That value
    ///   is an `F` or JavaScript's `undefined`, and Glyph has no type for
    ///   `undefined`. `tsc` accepts most of these (`let x = n?.f`, an
    ///   interpolation, an `unknown` argument), which is why the checker is
    ///   the one that has to say so.
    ///
    /// `field` is the field the read names (for a chain, the first one after
    /// the `?.` over the `Nullable`), and `type_name` the `Nullable` it reads
    /// through.
    #[error("{}", nullable_read_message(*.kind, .field, .type_name))]
    NullableRead {
        kind: NullableReadKind,
        field: String,
        type_name: String,
        span: Span,
    },

    /// G387, D52: a `sqlite.Sql` slot (the `sql` argument of a `Db` method,
    /// or any parameter, binding or return declared `sqlite.Sql`) holding text
    /// that is not fixed when the program is written. The message names where
    /// the text is run and the name or expression it is built from, and the
    /// help is computed: a rewrite to `?` placeholders with the parameter
    /// array where the query is a literal at a `Db` call, prose otherwise.
    #[error("{message}")]
    SqlFromRuntimeValue {
        message: String,
        help: String,
        span: Span,
    },

    /// G380, D5: a call that changes its receiver in place, standing anywhere
    /// but as the whole call of a `mut` statement, or under `mut` on a
    /// receiver that is not a place. Judged for the methods the generated
    /// rows mark `mutates` (`push`, `sort` and seven others on an `Array<T>`);
    /// a receiver the checker types `Unknown` is never judged. `kind` is
    /// where the call stands, which `glyph fix` reads: only a `Statement`
    /// takes the `mut ` it inserts. Message and help are computed from the
    /// call as written.
    ///
    /// `comparator_missing` is set on a `sort()` with no comparator over
    /// values that are not text, which E0243 refuses beside it (G388):
    /// `mut ` alone would leave that sort, so `glyph fix` writes nothing.
    #[error("{message}")]
    MutationOutsideMut {
        kind: MutationSite,
        message: String,
        help: String,
        comparator_missing: bool,
        span: Span,
    },

    /// G388, decision record 182: `sort()` or `toSorted()` with no
    /// comparator on an `Array<E>` whose elements are not text. JavaScript's
    /// default order compares the text of each element, so `[100, 9, 25]`
    /// sorts as `100, 25, 9`. Text is a `string`, a string-literal union, a
    /// refined `string`, a second name for one of those, and a type parameter
    /// bounded by one. A receiver the checker cannot type, and an element
    /// type it cannot decide, are not judged. The
    /// help writes the comparator in the form the call was written in.
    #[error("{message}")]
    TextOrderSort {
        message: String,
        help: String,
        span: Span,
    },

    /// G380, D5: `mut` before a call Glyph knows changes nothing: a method
    /// the generated rows mark non-mutating on an `Array<T>`, a `string` or a
    /// `number` (`mut xs.concat(ys)`, whose result is discarded), or a
    /// function read through a namespace import of a Glyph module
    /// (`mut array.push(xs, 1)`, `mut io.println(s)`), which D5's "a free
    /// function call is not a `mut` statement" already meant.
    #[error("{message}")]
    MutOnNonMutatingCall {
        message: String,
        help: String,
        span: Span,
    },

    /// G392, D56: a call to a built-in method whose TypeScript return can
    /// be `undefined` or `null` (`users.find(f)`, `xs.at(-1)`,
    /// `s.codePointAt(0)`, `s.match(p)`), on a receiver Glyph types as an
    /// `Array<T>` or a `string`. Glyph writes a value that may be missing as
    /// an `Option`, and the help names the `std/*` function that returns one.
    /// `repair` is what `glyph fix` writes, or why it writes nothing.
    #[error("{message}")]
    AbsentBuiltinResult {
        message: String,
        help: String,
        repair: AbsentRepair,
        span: Span,
    },

    /// G389: a read or call of a member of an `Array<T>`, a `string` or a
    /// `number` that TypeScript's library marks `@deprecated` (`s.substr`,
    /// `s.trimLeft`, `s.bold`). A **warning**: each one still runs in every
    /// engine and `tsc --strict` accepts it. The message quotes the tag; the
    /// help names what to write instead, computed from the call.
    #[error("{message}")]
    DeprecatedBuiltinMember {
        message: String,
        help: String,
        span: Span,
    },

    /// G311, record 87 (D18): `?.` on a receiver that resolves to the
    /// prelude `Option<T>` or `Result<T, E>`, through any alias. Neither is
    /// ever null at run time, so the chain never short-circuits: it reads
    /// the member off the tag object. On a `Result` the form is also the
    /// misreading of `?.` as `?` followed by `.` (csvql's
    /// `bind_agg(...)?.label`), which propagates nothing. A method called
    /// through `?.` (`r?.map(f)`) is the same refusal.
    ///
    /// Reported once, at the link where the chain starts, and the member is
    /// not also checked against the payload type: an E0210 there would imply
    /// the read could be legal. The node is `Unknown`, so nothing cascades.
    ///
    /// `type_name` is the container as resolved (`Option<Note>` for an alias
    /// of it), `receiver` the receiver as written, `binding` the name the
    /// help binds the payload to, and `payload` the payload type.
    #[error("{}", optional_chain_on_tagged_message(*.container, .type_name, .field))]
    OptionalChainOnTagged {
        container: TaggedContainer,
        type_name: String,
        field: String,
        method: bool,
        receiver: String,
        binding: String,
        payload: String,
        span: Span,
    },

    /// G429, decision record 157 (D18): `??` whose left operand resolves to
    /// the prelude `Option<T>` or `Result<T, E>`, through any alias. Neither
    /// is ever null or undefined, so the default replaces only the absence
    /// of an operand that can itself be absent (a `?.` chain, a field
    /// declared `f?:`, an index read), and a present one hands back the tag
    /// object. Same fact as `?.` on one, so the same code, E0236.
    ///
    /// An operand that cannot be absent is refused whatever the default is.
    /// One that can is refused unless the default is accepted where the
    /// container is declared (`b.o ?? None` is accepted and typed as the
    /// container). The node is `Unknown`, so nothing cascades. `message`
    /// and `help` are computed where the `??` is, in the program's own
    /// names, and `repair` is the `match` `glyph fix` writes and re-checks.
    #[error("{message}")]
    CoalesceOnTagged {
        message: String,
        help: String,
        repair: AbsentRepair,
        span: Span,
    },

    /// G385, record 129 (D54): a value with no text form converted to text.
    /// One rule covers the three implicit conversions: a `${}` in a `"..."`
    /// or `"""..."""` literal, an operand of `+` whose other operand is a
    /// string, and the argument of `string.from`. Text-like is an allowlist
    /// (`string`, a string-literal union, `number`, `int`, `bigint`, `bool`,
    /// an alias or `where` refinement of one, and `never`), so a type the
    /// checker learns later is refused here by default. A value the checker
    /// types as its internal `Unknown` is not judged.
    ///
    /// `message` and `help` are computed where the value is, in the program's
    /// own names, so neither is rebuilt here.
    #[error("{message}")]
    NoTextForm {
        message: String,
        help: String,
        span: Span,
    },

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

    /// A `component` whose one parameter is not a record (G243). React calls a
    /// component with the single props object the element builds, and the
    /// emitter has no other way to pass anything, so the parameter receives
    /// that object whatever type it declares. `<Greeting name="x" />` over
    /// `component Greeting(name: string)` is refused by `tsc` (TS2769), and
    /// `<Greeting />` is accepted there while `name` holds `{}` at run time.
    ///
    /// Raised for a type the checker can decide is not a record: a
    /// primitive, a string-literal union, a tagged union, an `Array`, `Option`,
    /// `Result` or `Nullable`, or a function type, seen through any second
    /// name. Raised too for `unknown`, which holds the props object but gives
    /// the component nothing it can read, and for a type parameter with no
    /// record or interface bound, which may be filled with a primitive (G246).
    /// A type it cannot see into (an `extern_ts` or `typeof` body, an opaque
    /// library type) and `Record<K, V>` draw nothing. `span` is the parameter,
    /// `ty` is its type as the source writes it, and `why` says which of the
    /// three it is.
    #[error("component `{component}` takes its props as one record, but its parameter `{param}` is `{ty}`{}", props_refusal_suffix(*.why))]
    ComponentParamNotRecord {
        component: String,
        param: String,
        ty: String,
        why: PropsRefusal,
        span: Span,
    },

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

    /// A literal `match` arm over a string-literal union (D30) whose value the
    /// union does not hold (G305): `"c"` over `"a" | "b"`, or a misspelling
    /// beside the right literal (`"manuel"` beside `"manual"`). The arm can
    /// never run, and `tsc` refuses it (TS2678). The literal-union twin of
    /// `UnknownVariantPattern`, under the same code: in both an arm names a
    /// case the scrutinee's union does not have. `suggestion` is the nearest
    /// literal by the same edit-distance rule.
    #[error("`\"{value}\"` is not a value of `{union}`{}", .suggestion.as_deref().map(|s| format!("; did you mean `\"{s}\"`?")).unwrap_or_default())]
    UnknownLiteralPattern {
        union: String,
        value: String,
        suggestion: Option<String>,
        span: Span,
    },

    /// `await` written outside an `async fn`. A call to an async function is
    /// a pending value (D51) that only an async callable can wait for, so
    /// `await` is only meaningful inside one; anywhere else the emitted
    /// TypeScript is rejected by `tsc`
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

    /// A destructuring `let` whose initializer cannot be proved to have the
    /// pattern's length (D48, G255). A written array literal is counted, so a
    /// count that disagrees is refused; an `Array<T>` has no length the checker
    /// knows; anything else is not an array. A value whose type the checker
    /// cannot see (an npm import) is not refused here: the emitted
    /// `satisfies readonly [unknown, ...]` has `tsc` prove it instead. The
    /// parser raises the same code for a pattern shape that can fail, the
    /// E0010 and E0101 precedent of one rule caught at two stages. `span` is
    /// the initializer, or the element a nested pattern destructures.
    #[error("{}", why.message())]
    DestructureUnproven { why: DestructureRefusal, span: Span },

    /// An object pattern over a value whose type is decidably not a record:
    /// `Loaded({ users })` where `Loaded` carries an `Array<string>` (G279).
    /// A non-record payload sits under `value` in the tag object rather than
    /// spread into it, so the pattern would read fields of the tag object and
    /// bind `undefined` under a type that says otherwise. The same code as a
    /// value of the wrong type, because it is one: the pattern's shape is the
    /// type it expects, and the payload is not that type. `ty` is the type the
    /// pattern was matched against.
    #[error("`{ty}` is not a record, so an object pattern cannot destructure it")]
    PatternNotRecord { ty: String, span: Span },

    /// `record.remove` on a `Record` whose key is a string-literal union, by
    /// any spelling (G373). A `Record<Tier, V>` holds a value for every `Tier`
    /// (the validator's totality rule), so removing one leaves a value that is
    /// no `Record<Tier, V>`, and no other declared type either. E0211, at the
    /// record argument.
    #[error("record.remove takes a Record whose key type has no fixed member set; {} holds every `{key}`, so removing one leaves a value of no declared type", remove_subject(.record, .key))]
    RecordRemoveFiniteKey {
        /// The key, as the program spells it where the checker can read it.
        key: String,
        /// The record argument's type as the program wrote it, when the
        /// argument is a name whose annotation the checker holds.
        record: Option<String>,
        /// The record with an optional field per member that the help offers,
        /// when every member is a field name and the value type is known as
        /// written.
        partial: Option<String>,
        span: Span,
    },

    /// `bind={x}` on a JSX `<case>` (D6). A `<case>` takes the pattern a
    /// `match` arm takes, so `bind` is not an attribute of it: the payload is
    /// bound by the pattern, `<case Loaded(items)>` for one value and
    /// `<case Loaded({ users })>` for a record's fields. `bind` read a field
    /// with its own name off the tag object whatever the payload was, which
    /// bound `undefined` for a positional payload under no type at all (G275).
    /// `shape` is what the variant carries, which decides the rewrite the help
    /// spells out.
    #[error("`bind` is not an attribute of `<case>`; a `<case>` takes a pattern, as a `match` arm does")]
    CaseBind {
        variant: String,
        name: String,
        shape: CaseBindShape,
        span: Span,
    },

    /// G384, D51: a pending value nothing waits for. A call to a callee the
    /// checker knows is async types as a pending `T`, and `await` unwraps it;
    /// anywhere else but the positions `pending::check_pending` lists, the
    /// call starts, the program moves on, and a failure is lost.
    ///
    /// One code for every position, so it always means a forgotten `await`.
    /// It is not E0217: that is a warning about a dropped `Result`, fixed with
    /// a `match`, and this is an error about ordering, fixed with `await`. A
    /// pending `Result` is this error and not that warning.
    ///
    /// `subject` is the expression as written, `callee` the function called
    /// when it is a call, `resolves_to` the `T` (`None` for `void`), and
    /// `task` the name this module imports `std/task` under, for the help.
    #[error("{}", pending::pending_message(.position, .subject, .callee.as_deref(), .resolves_to.as_deref(), .context))]
    PendingNotAwaited {
        position: pending::PendingPosition,
        subject: String,
        callee: Option<String>,
        resolves_to: Option<String>,
        settles_to_result: bool,
        context: pending::WaitContext,
        task: Option<String>,
        span: Span,
    },

    /// An async function passed where a sync `fn(...) -> void` is declared:
    /// `ids.forEach(save)` with `async fn save`, an async lambda given to
    /// `timers.every` (D40, as amended for 0.1.159). The slot calls it and
    /// drops what it returns, so each call runs on its own and its failure is
    /// lost. E0211, at the argument, with its own message: the general
    /// argument mismatch would only print two function types.
    ///
    /// `callee` is the function called, as the program spells it; `loop_over`
    /// the array a `forEach` walks, for the loop the help offers; `can_await`
    /// whether the calling code is async; `task` the module's name for
    /// `std/task`.
    #[error("{}", async_slot::async_slot_message(.callee, .callback))]
    AsyncCallbackIntoSyncVoid {
        callee: String,
        callback: async_slot::AsyncCallback,
        loop_over: Option<String>,
        can_await: bool,
        task: Option<String>,
        span: Span,
    },
}

/// What the variant a `<case ... bind={x}>` names carries, as far as the
/// checker could tell (E0231). It decides which pattern the help writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseBindShape {
    /// A record payload: the fields are bound by an object pattern.
    Record,
    /// One value of any other type: bound by a name in parentheses.
    Positional,
    /// No payload at all: there is nothing to bind.
    Nullary,
    /// The checker could not resolve the variant's payload.
    Unknown,
}

/// Why a destructuring `let` was refused at check (D48, E0230).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestructureRefusal {
    /// A written array literal of another length. `at_least` is set when the
    /// pattern ends in a rest, which takes any number of further elements.
    Count {
        pattern: usize,
        at_least: bool,
        literal: usize,
    },
    /// An `Array<T>`, whose length is known only at run time. `ty` is how the
    /// checker displays it; `spread` is set for a written literal with a
    /// spread in it, whose element type the checker does not read and whose
    /// length the spread decides.
    UnknownLength { ty: String, spread: bool },
    /// A value that is not an array: a primitive, a record, a union,
    /// `unknown`, a type parameter.
    NotAnArray { ty: String },
}

impl DestructureRefusal {
    fn message(&self) -> String {
        match self {
            DestructureRefusal::Count {
                pattern,
                at_least,
                literal,
            } => {
                let takes = match at_least {
                    true => "at least ",
                    false => "",
                };
                let noun = |n: usize| match n {
                    1 => "element",
                    _ => "elements",
                };
                format!(
                    "the pattern takes {takes}{pattern} {}, and the array has {literal}",
                    noun(*pattern)
                )
            }
            DestructureRefusal::UnknownLength { spread: true, .. } => {
                "the length of an array literal with a spread in it is not known, so a name in the pattern could hold `undefined`".to_string()
            }
            DestructureRefusal::UnknownLength { ty, spread: false } => format!(
                "the length of `{ty}` is not known, so a name in the pattern could hold `undefined`"
            ),
            DestructureRefusal::NotAnArray { ty } => {
                format!("a `let` pattern destructures an array, and this value is `{ty}`")
            }
        }
    }

    fn type_shown(&self) -> Option<&str> {
        match self {
            DestructureRefusal::Count { .. } | DestructureRefusal::UnknownLength { spread: true, .. } => {
                None
            }
            DestructureRefusal::UnknownLength { ty, .. } | DestructureRefusal::NotAnArray { ty } => {
                Some(ty)
            }
        }
    }
}

/// Diagnostic severity. Most diagnostics are hard `Error`s that fail the build;
/// a `Warning` is surfaced but does not (verifiability stays the lead pillar, so
/// warnings are reserved for "legal but almost certainly a mistake").
/// Why a component's one parameter cannot be its props record (E0229).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropsRefusal {
    /// A type whose value is never an object: a primitive, a string-literal
    /// union, a tagged union, an `Array`, a `Nullable` or a function type.
    NotAnObject,
    /// `unknown`. It holds the props object, and gives the component nothing
    /// it can read without a check.
    Unknown,
    /// A type parameter with no record or interface bound, which an element
    /// cannot fill with props and a caller may fill with a primitive.
    TypeParam,
}

/// Which of E0235's two forms a member read through a `Nullable` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullableReadKind {
    /// `n.f` with the plain dot, where `n` may be null.
    Dot,
    /// The value of `n?.f` (or a chain it starts) held somewhere other than
    /// the left of `??` or an operand of `==`/`!=`.
    ChainEscapes,
}

/// Which prelude tagged type a `?.` was written on (E0236).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaggedContainer {
    Option,
    Result,
}

/// A plain-dot member read on a prelude `Option` or `Result` that is not one
/// of its members (E0210, G311). What the help needs to write the `match` or
/// the `?` back in the program's own names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedMember {
    pub container: TaggedContainer,
    /// The receiver as written (`r`, `load()`).
    pub receiver: String,
    /// The name the help binds the payload to.
    pub binding: String,
    /// The payload type, shown (`Note`).
    pub payload: String,
    /// The member is the callee of a call (`r.unwrap()`).
    pub method: bool,
}

impl TaggedContainer {
    /// The members a plain dot may name on this prelude type: `map` and
    /// `map_err` on a `Result`, nothing on an `Option` (record 87, Q3).
    pub fn members(self) -> &'static [&'static str] {
        match self {
            TaggedContainer::Option => &[],
            TaggedContainer::Result => &["map", "map_err"],
        }
    }
}

/// The names the runtime encoding of an `Option` or a `Result` puts on the
/// object (D49). Not members: the help says so rather than suggesting one.
pub(crate) fn is_tagged_encoding_name(field: &str) -> bool {
    matches!(field, "tag" | "value")
}

fn tagged_member_set_tail(container: TaggedContainer) -> &'static str {
    match container {
        TaggedContainer::Option => ": an `Option` has no members",
        TaggedContainer::Result => ": a `Result`'s only members are `map` and `map_err`",
    }
}

/// The help for a key an object literal writes that its declared record does
/// not declare (D57). Three repairs, in the order a reader weighs them: the
/// declared field it was meant to be, deleting it, or declaring it. The
/// declaration is named only when there is one to edit; an inline record type
/// or a stdlib record has none. `suggestion` is never a field the literal
/// already writes (the checker leaves it out then), so the rename it names
/// never writes a field twice.
fn literal_field_help(field: &str, type_name: &str, declared: bool, suggestion: Option<&str>) -> String {
    let declare = if declared {
        format!("declare `{field}` in `{type_name}`")
    } else {
        format!("add `{field}` to the type")
    };
    match suggestion {
        Some(s) => format!(
            "Rename it to `{s}`, delete it, or {declare}. A field the type does not declare is never read through it."
        ),
        None => format!(
            "Delete it, rename it to a field `{type_name}` declares that the literal does not already write, or {declare}. A field the type does not declare is never read through it."
        ),
    }
}

fn tagged_member_help(field: &str, t: &TaggedMember) -> String {
    let TaggedMember { container, receiver, binding, payload, method } = t;
    let open = match container {
        TaggedContainer::Option => format!("match {receiver} {{ Some({binding}) => ..., None => ... }}"),
        TaggedContainer::Result => format!("match {receiver} {{ Ok({binding}) => ..., Err(e) => ... }}"),
    };
    if is_tagged_encoding_name(field) {
        let (article, name) = match container {
            TaggedContainer::Option => ("an", "Option"),
            TaggedContainer::Result => ("a", "Result"),
        };
        let propagate = match container {
            TaggedContainer::Option => String::new(),
            TaggedContainer::Result => format!(", or propagate the `Err` with `?`: `let {binding} = {receiver}?`"),
        };
        return format!(
            "`{field}` is part of how the emitted TypeScript encodes {article} `{name}`, not a member of it (D49). Open it with `{open}`{propagate}."
        );
    }
    match (container, method) {
        (TaggedContainer::Option, _) => {
            optional_chain_on_tagged_help(*container, field, *method, receiver, binding, payload)
        }
        (TaggedContainer::Result, true) => format!(
            "Open it with `{open}`, propagate the `Err` with `?`, or transform it with `map` or `map_err`."
        ),
        (TaggedContainer::Result, false) => format!(
            "Propagate first, then read: `let {binding} = {receiver}?` then `{binding}.{field}`. Or open it with `match {receiver} {{ Ok({binding}) => {binding}.{field}, Err(e) => ... }}`."
        ),
    }
}

fn optional_chain_on_tagged_message(
    container: TaggedContainer,
    type_name: &str,
    field: &str,
) -> String {
    match container {
        TaggedContainer::Option => format!(
            "`?.` on an `{type_name}`: an `Option` is never null, so `?.` reads `{field}` off the `Option` itself, not off its `Some` value"
        ),
        TaggedContainer::Result => format!(
            "`?.` on a `{type_name}`: `?.` is optional chaining, not `?` followed by `.`; it does not propagate an `Err`, and a `Result` is never null"
        ),
    }
}

fn optional_chain_on_tagged_help(
    container: TaggedContainer,
    field: &str,
    method: bool,
    receiver: &str,
    binding: &str,
    payload: &str,
) -> String {
    let call = if method { "(...)" } else { "" };
    match (container, method) {
        (TaggedContainer::Option, _) => format!(
            "Open it with `match {receiver} {{ Some({binding}) => {binding}.{field}{call}, None => <default> }}`. If the absence is a JSON `null` from the wire, declare the type `Nullable<{payload}>` (D45)."
        ),
        (TaggedContainer::Result, false) => format!(
            "Propagate first, then read: `let {binding} = {receiver}?` then `{binding}.{field}`."
        ),
        (TaggedContainer::Result, true) => format!("Drop the `?`: `{receiver}.{field}(...)`."),
    }
}

fn nullable_read_message(kind: NullableReadKind, field: &str, type_name: &str) -> String {
    match kind {
        NullableReadKind::Dot => format!(
            "`.{field}` reads through a `{type_name}`, which may be null"
        ),
        NullableReadKind::ChainEscapes => format!(
            "`?.{field}` over a `{type_name}` is `undefined` when the value is null, and it is not given a default with `??` or compared with `==`/`!=`"
        ),
    }
}

/// The tail E0229's sentence gets for the two refusals the type alone does
/// not explain.
fn props_refusal_suffix(why: PropsRefusal) -> &'static str {
    match why {
        PropsRefusal::NotAnObject => "",
        PropsRefusal::Unknown => ", which gives it nothing it can read",
        PropsRefusal::TypeParam => ", a type parameter with no record or interface bound",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// The tail a type mismatch's sentence gets when the refused record lacks
/// required fields: `: missing field `age`` for one, `: missing fields `age`,
/// `email`` for several, nothing when none is missing. The names are quoted
/// the way the exhaustiveness message quotes the variants it lists.
/// The tail a refusal under the weak-type rule gets: the value is refused
/// because it shares no field with a record whose fields are all optional,
/// and nothing else about it is wrong, so the sentence has to say so.
fn no_common_field_suffix(no_common_field: bool, expected: &str) -> String {
    if no_common_field {
        format!(": it has no field in common with `{expected}`, whose fields are all optional")
    } else {
        String::new()
    }
}

/// The subject of E0211's `record.remove` sentence: the record's own type when
/// the program wrote one the checker read, else the `Record` described by its
/// key.
fn remove_subject(record: &Option<String>, key: &str) -> String {
    match record {
        Some(r) => format!("`{r}`"),
        None => format!("a `Record` keyed by `{key}`"),
    }
}

fn missing_fields_suffix(missing: &[String]) -> String {
    match missing {
        [] => String::new(),
        [one] => format!(": missing field `{one}`"),
        many => format!(
            ": missing fields {}",
            many.iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The E0304 message for a check naming a type parameter, bare or as a
/// part of the checked type (G362). One text for the checker's report on an
/// `is` pattern and the emitter's on every other spelling.
pub fn type_parameter_pattern_message(container: Option<&str>, param: &str, owner: &str) -> String {
    match container {
        Some(c) => format!(
            "cannot validate `{c}`: its type argument `{param}` is a type parameter of `{owner}`, which has no runtime check"
        ),
        None => format!(
            "cannot validate `{param}`: `{param}` is a type parameter of `{owner}`, which has no runtime check"
        ),
    }
}

/// The help on E0304 for a type parameter, shared with the emitter.
pub const TYPE_PARAMETER_PATTERN_HELP: &str = "Check a type that has a runtime check, or take a `Schema<T>` argument and call `schema.parse(v)`, or `json.parse_with(text, schema)` for text. If the code meant a type declared with the same name, rename the type parameter: inside its function the parameter hides that type.";

/// The note on E0304 for a type parameter, shared with the emitter.
pub const TYPE_PARAMETER_PATTERN_NOTE: &str = "Nothing can check a type parameter, because a type parameter is erased at run time. That holds for an `is` pattern, `json.parse<T>`, a generic descriptor's type argument (`Box.parse<T>(v)`), and `T.parse`, `T.is` and `T.schema`. The same holds for a part: `is Array<T>`, `json.parse<Array<T>>` and `is Box<T>` check each element or field with `T`'s check. A name is read by scope, so inside `fn f<Doc>` the type and the value `Doc` name the parameter even when the module declares a `type Doc`.";

/// The help on a value refused for holding `unknown` where another type is
/// declared (G325, record 89).
pub const UNKNOWN_VALUE_HELP: &str = "An `unknown` stands only where `unknown` is declared. Check it first: `json.parse<T>(text)` or `T.parse(value)` gives a `Result<T, Array<Issue>>`, and a `match` arm with an `is` pattern (`is string => …`) narrows it.";

impl TypeError {
    /// Whether this diagnostic fails the build (`Error`) or is only surfaced
    /// (`Warning`). Everything is an `Error` except the few advisory lints.
    pub fn severity(&self) -> Severity {
        match self {
            TypeError::UnusedResult { .. } | TypeError::DeprecatedBuiltinMember { .. } => Severity::Warning,
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
            TypeError::TypeParameterPattern { span, .. } => *span,
            TypeError::MutAcrossAwait { span, .. } => *span,
            TypeError::OwnedNotConsumed { span, .. } => *span,
            TypeError::OwnedUsedAfterMove { span, .. } => *span,
            TypeError::NonExhaustiveArrayMatch { span, .. } => *span,
            TypeError::NonExhaustiveBoolMatch { span, .. } => *span,
            TypeError::NonExhaustiveValueMatch { span, .. } => *span,
            TypeError::UnseenMatchType { span, .. } => *span,
            TypeError::NonExhaustiveFieldMatch { span } => *span,
            TypeError::NullableNested { span, .. } => *span,
            TypeError::NullableRead { span, .. } => *span,
            TypeError::OptionalChainOnTagged { span, .. } => *span,
            TypeError::CoalesceOnTagged { span, .. } => *span,
            TypeError::SqlFromRuntimeValue { span, .. } => *span,
            TypeError::MutationOutsideMut { span, .. } => *span,
            TypeError::MutOnNonMutatingCall { span, .. } => *span,
            TypeError::TextOrderSort { span, .. } => *span,
            TypeError::AbsentBuiltinResult { span, .. } => *span,
            TypeError::DeprecatedBuiltinMember { span, .. } => *span,
            TypeError::NoTextForm { span, .. } => *span,
            TypeError::TypeNameAsValue { span, .. } => *span,
            TypeError::RedactUnknownField { span, .. } => *span,
            TypeError::UnknownAnnotation { span, .. } => *span,
            TypeError::UnknownField { span, .. } => *span,
            TypeError::MapFieldAccess { span, .. } => *span,
            TypeError::ArgumentTypeMismatch { span, .. } => *span,
            TypeError::ArgumentCountMismatch { span, .. } => *span,
            TypeError::TypeArgumentCountMismatch { span, .. } => *span,
            TypeError::UncalledBuiltinMethod { span, .. } => *span,
            TypeError::MutateConst { span, .. } => *span,
            TypeError::ComponentMultipleParams { span, .. } => *span,
            TypeError::ComponentParamNotRecord { span, .. } => *span,
            TypeError::OwnedAliased { span, .. } => *span,
            TypeError::UnreachableMatchArm { span } => *span,
            TypeError::UnusedResult { span } => *span,
            TypeError::UnknownVariantPattern { span, .. } => *span,
            TypeError::UnknownLiteralPattern { span, .. } => *span,
            TypeError::AwaitOutsideAsyncFn { span } => *span,
            TypeError::MatchArmProducesNoValue { span } => *span,
            TypeError::DestructureUnproven { span, .. } => *span,
            TypeError::PatternNotRecord { span, .. } => *span,
            TypeError::RecordRemoveFiniteKey { span, .. } => *span,
            TypeError::CaseBind { span, .. } => *span,
            TypeError::PendingNotAwaited { span, .. } => *span,
            TypeError::AsyncCallbackIntoSyncVoid { span, .. } => *span,
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
            TypeError::TypeParameterPattern { .. } => "E0304",
            TypeError::MutAcrossAwait { .. } => "E0225",
            TypeError::OwnedNotConsumed { .. } => "E0206",
            TypeError::OwnedUsedAfterMove { .. } => "E0207",
            TypeError::NonExhaustiveArrayMatch { .. } => "E0208",
            TypeError::NonExhaustiveBoolMatch { .. } => "E0209",
            TypeError::NonExhaustiveValueMatch { .. } => "E0218",
            TypeError::UnseenMatchType { .. } => "E0232",
            TypeError::NonExhaustiveFieldMatch { .. } => "E0226",
            TypeError::NullableNested { .. } => "E0227",
            TypeError::NullableRead { .. } => "E0235",
            TypeError::OptionalChainOnTagged { .. } | TypeError::CoalesceOnTagged { .. } => "E0236",
            TypeError::SqlFromRuntimeValue { .. } => "E0239",
            TypeError::MutationOutsideMut { .. } => "E0240",
            TypeError::MutOnNonMutatingCall { .. } => "E0241",
            TypeError::AbsentBuiltinResult { .. } => "E0242",
            TypeError::TextOrderSort { .. } => "E0243",
            TypeError::DeprecatedBuiltinMember { .. } => "E0244",
            TypeError::NoTextForm { .. } => "E0237",
            TypeError::TypeNameAsValue { .. } => "E0228",
            TypeError::RedactUnknownField { .. } => "E0219",
            TypeError::UnknownAnnotation { .. } => "E0221",
            TypeError::UnknownField { .. } => "E0210",
            TypeError::MapFieldAccess { .. } => "E0224",
            TypeError::ArgumentTypeMismatch { .. } => "E0211",
            TypeError::MutateConst { .. } => "E0212",
            TypeError::ArgumentCountMismatch { .. } => "E0213",
            TypeError::TypeArgumentCountMismatch { .. } => "E0233",
            TypeError::UncalledBuiltinMethod { .. } => "E0234",
            TypeError::ComponentMultipleParams { .. } => "E0214",
            TypeError::ComponentParamNotRecord { .. } => "E0229",
            TypeError::OwnedAliased { .. } => "E0215",
            TypeError::UnreachableMatchArm { .. } => "E0216",
            TypeError::UnusedResult { .. } => "E0217",
            TypeError::UnknownVariantPattern { .. } => "E0220",
            TypeError::UnknownLiteralPattern { .. } => "E0220",
            TypeError::AwaitOutsideAsyncFn { .. } => "E0222",
            TypeError::MatchArmProducesNoValue { .. } => "E0223",
            TypeError::DestructureUnproven { .. } => "E0230",
            TypeError::PatternNotRecord { .. } => "E0204",
            TypeError::RecordRemoveFiniteKey { .. } => "E0211",
            TypeError::CaseBind { .. } => "E0231",
            TypeError::PendingNotAwaited { .. } => "E0238",
            TypeError::AsyncCallbackIntoSyncVoid { .. } => "E0211",
        }
    }

    /// A one-line, actionable fix (the Elm-quality bar): what to change.
    pub fn help(&self) -> Option<Cow<'static, str>> {
        if let TypeError::TypeMismatch { record_key: Some(k), .. }
        | TypeError::ArgumentTypeMismatch { record_key: Some(k), .. } = self
        {
            if let Some(h) = k.help() {
                return Some(Cow::Owned(h));
            }
        }
        // G325. Said before the taint helps: an `unknown` where a
        // `taint.Trusted` is declared is unchecked first and untrusted second.
        if let TypeError::TypeMismatch { unknown_value: true, .. }
        | TypeError::ArgumentTypeMismatch { unknown_value: true, .. } = self
        {
            return Some(Cow::Borrowed(UNKNOWN_VALUE_HELP));
        }
        if let TypeError::SqlFromRuntimeValue { help, .. }
        | TypeError::MutationOutsideMut { help, .. }
        | TypeError::MutOnNonMutatingCall { help, .. }
        | TypeError::AbsentBuiltinResult { help, .. }
        | TypeError::TextOrderSort { help, .. }
        | TypeError::DeprecatedBuiltinMember { help, .. } = self
        {
            return Some(Cow::Owned(help.clone()));
        }
        // A plain value where `std/taint`'s branded wrapper is declared (G387,
        // record 132). "Change the parameter's type" is wrong advice for a
        // stdlib parameter, and the wrapper is only ever made by `std/taint`'s
        // own functions, so the help names them.
        if let TypeError::TypeMismatch { expected, found, .. }
        | TypeError::ArgumentTypeMismatch { expected, found, .. } = self
        {
            if expected.starts_with("taint.Trusted<") && found.starts_with("taint.Tainted<") {
                return Some(Cow::Borrowed(
                    "Pass it through `taint.sanitize(…)` first; a `Tainted` value reaches a `Trusted` slot only through it.",
                ));
            }
            if expected.starts_with("taint.Trusted<") {
                // The SQL clause fits only `sqlite.from_trusted`'s parameter;
                // a field, a `let`, a `return` or another sink is not a query
                // (record 137).
                let sql = matches!(self, TypeError::ArgumentTypeMismatch { sql_escape: true, .. });
                return Some(Cow::Borrowed(match sql {
                    true => "Pass a constant query directly; text built at run time goes through `taint.sanitize(…)`, or `taint.trust_unchecked(…)` for text you wrote.",
                    false => "Text built at run time goes through `taint.sanitize(…)`, or `taint.trust_unchecked(…)` for text you wrote.",
                }));
            }
            if expected.starts_with("taint.Tainted<") {
                return Some(Cow::Borrowed(
                    "Wrap a value that comes from outside the program with `taint.taint(…)`; a `Tainted` value reaches a `Trusted` slot only through `taint.sanitize(…)`.",
                ));
            }
        }
        if let TypeError::TypeMismatch { place: Some(_), .. } = self {
            return Some(Cow::Borrowed(MUT_PLACE_HELP));
        }
        if let TypeError::RecordRemoveFiniteKey { key, partial, .. } = self {
            let partial = match partial {
                Some(p) => format!("declare a record with optional fields (`{p}`)"),
                None => "declare a record with an optional field for each member".to_string(),
            };
            return Some(Cow::Owned(format!(
                "For a partial map over `{key}`, {partial}; to replace a value, use `record.set`."
            )));
        }
        if let TypeError::CaseBind {
            variant,
            name,
            shape,
            ..
        } = self
        {
            return Some(Cow::Owned(case_bind_help(variant, name, *shape)));
        }
        if let TypeError::PendingNotAwaited {
            position,
            subject,
            settles_to_result,
            context,
            task,
            ..
        } = self
        {
            return Some(Cow::Owned(pending::pending_help(
                position,
                subject,
                *settles_to_result,
                context,
                task.as_deref(),
            )));
        }
        if let TypeError::ArgumentTypeMismatch { kind_help: Some(h), .. } = self {
            return Some(Cow::Owned(h.clone()));
        }
        if let TypeError::AsyncCallbackIntoSyncVoid {
            callback,
            loop_over,
            can_await,
            task,
            ..
        } = self
        {
            return Some(Cow::Owned(async_slot::async_slot_help(
                callback,
                loop_over.as_deref(),
                *can_await,
                task.as_deref(),
            )));
        }
        if let TypeError::UnknownField { tagged: Some(t), field, .. } = self {
            return Some(Cow::Owned(tagged_member_help(field, t)));
        }
        if let TypeError::UnknownField {
            literal: Some(_),
            field,
            type_name,
            record,
            suggestion,
            ..
        } = self
        {
            return Some(Cow::Owned(literal_field_help(
                field,
                type_name,
                record.is_some(),
                suggestion.as_deref(),
            )));
        }
        if let TypeError::UnknownField {
            builtin: Some(receiver),
            type_name,
            ..
        } = self
        {
            return Some(Cow::Owned(format!(
                "Check the member name for a typo. `{type_name}` is {}, so its members are the ones TypeScript's library declares for it and no others; `{}` holds Glyph's own functions over it.",
                receiver.runtime_name(),
                receiver.std_module(),
            )));
        }
        if let TypeError::UncalledBuiltinMethod {
            builtin,
            call,
            lambda,
            ..
        } = self
        {
            let wrap = match lambda {
                Some(l) => format!("wrap the call in a lambda, `{l}`"),
                None => format!("wrap the call in a lambda that calls `{call}`"),
            };
            return Some(Cow::Owned(format!(
                "Call the method where it is read, `{call}`. To hand it on as a value, {wrap}; `.bind` and `.call` are refused like the read itself, because what they return is untyped in Glyph. `{}` holds Glyph's own functions over {}, which can be passed as values.",
                builtin.std_module(),
                builtin.runtime_name(),
            )));
        }
        if let TypeError::NoTextForm { help, .. } | TypeError::CoalesceOnTagged { help, .. } = self {
            return Some(Cow::Owned(help.clone()));
        }
        if let TypeError::OptionalChainOnTagged {
            container,
            field,
            method,
            receiver,
            binding,
            payload,
            ..
        } = self
        {
            return Some(Cow::Owned(optional_chain_on_tagged_help(
                *container, field, *method, receiver, binding, payload,
            )));
        }
        if let TypeError::TypeArgumentCountMismatch {
            value_of: Some((function, written)),
            ..
        } = self
        {
            return Some(Cow::Owned(format!(
                "Call `{function}<{written}>(...)` directly; a `{function}` value returns `unknown`, because only a type argument written on the call itself makes the call validate its input."
            )));
        }
        if let TypeError::UnseenMatchType {
            type_name, module, ..
        } = self
        {
            // A stdlib type the checker does not model yet: there is no file
            // the reader can bring into the build, so the help says so rather
            // than sending them to look for one.
            if module.starts_with("std/") {
                return Some(Cow::Owned(format!(
                    "The checker does not read `{type_name}` from `{module}` yet, so there is nothing to make visible from this project. \
                     Until it does, an `else` arm is the only way to compile this match, and it opts the match out of exhaustiveness checking."
                )));
            }
            return Some(Cow::Owned(format!(
                "Make `{module}` visible to this compile, so the arms are checked against `{type_name}` itself. \
                 If `{module}` is a module of this project, build from the project root so its file is read. \
                 If it is an npm package, materialize its types with `glyph gen dts {module}` and import `{type_name}` from the generated module. \
                 An `else` arm also compiles, but it opts this match out of exhaustiveness checking for good."
            )));
        }
        Some(Cow::Borrowed(match self {
            TypeError::NonExhaustiveMatch { .. } => {
                "Add an arm for each missing variant."
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
            TypeError::TypeMismatch { no_common_field: true, .. } => {
                "Write at least one of the declared record's fields, or change the declared type. A value with none of them is refused by `tsc` too (TS2559)."
            }
            TypeError::TypeMismatch { .. } => {
                "Change the value, or the declared type, so the two agree."
            }
            TypeError::MapFieldAccess { .. } => {
                "Use `record.get(map, \"key\")`, which returns `Option<V>` so the absent case has somewhere to go. `record.has` tests for the key alone."
            }
            TypeError::NullableRead { kind: NullableReadKind::Dot, .. } => {
                "Read the field with `?.` and give the absent case a value with `??` (`n?.name ?? \"none\"`), or convert the whole value with `nullable.to_option(n)` and `match` on it."
            }
            TypeError::NullableRead { kind: NullableReadKind::ChainEscapes, .. } => {
                "Give the absent case a value with `??` (`n?.name ?? \"none\"`), or convert the whole value with `nullable.to_option(n)` and `match` on it."
            }
            TypeError::NullableNested { .. } => {
                "Write `Nullable` over the plain type (`Nullable<int>`) for a field the wire sends as null, or `Option<T>` alone inside the program; `nullable.to_option` and `nullable.from_option` convert between them."
            }
            TypeError::TypeNameAsValue { .. } => {
                "Write the value in its own form and move the type's name to the annotation on the `let`, the parameter, or the return type."
            }
            TypeError::TypeParameterPattern { .. } => TYPE_PARAMETER_PATTERN_HELP,
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
            // `dom.Element` has no fields by design (G254): the help that says
            // "add the field to the type" would be advice to edit the stdlib.
            TypeError::UnknownField { type_name, fields, .. }
                if fields.is_empty() && type_name == "dom.Element" =>
            {
                "`dom.Element` is opaque: Glyph reads no member of a live page element. Hand it to a library (React's `createRoot(el)`) or back to `std/dom`; a page write underneath React is a bug."
            }
            TypeError::UnknownField { .. } => {
                "Check the field name for a typo, or add the field to the type."
            }
            TypeError::ArgumentTypeMismatch { children_between_tags: true, .. } => {
                "Children written between the tags are not part of the props object: the element passes them to React as separate arguments, so they never fill a required `children` field. Declare it optional (`children?: T`) and read it as optional, or pass it as an attribute: `children={...}`."
            }
            TypeError::ArgumentTypeMismatch { no_common_field: true, .. } => {
                "Pass a value that writes at least one of the parameter record's fields, or change the parameter's type. A value with none of them is refused by `tsc` too (TS2559)."
            }
            TypeError::ArgumentTypeMismatch { .. } => {
                "Pass a value of the expected type, or change the parameter's type."
            }
            TypeError::ArgumentCountMismatch { .. } => {
                "Supply exactly one argument per parameter. Glyph has no optional or variadic parameters."
            }
            TypeError::TypeArgumentCountMismatch { .. } => {
                "Write one type argument per type parameter the declaration lists, or none on a call and let the arguments infer them."
            }
            TypeError::MutateConst { .. } => {
                "`const` is immutable (D20). Use a function-level `let` if the binding must change."
            }
            TypeError::ComponentMultipleParams { .. } => {
                "Take a single props record: `component C(props: P)` with `type P = { ... }`, then read `props.field`."
            }
            TypeError::ComponentParamNotRecord { why: PropsRefusal::Unknown, .. } => {
                "An `unknown` props object gives the component nothing it can read. Declare the props it takes as a record, e.g. `props: { name: string }`, and read `props.name`."
            }
            TypeError::ComponentParamNotRecord { why: PropsRefusal::TypeParam, .. } => {
                "A type parameter may be filled with anything, a primitive included, so the props need a record or interface bound: `component Show<T: HasLabel>(props: T)`. Or declare the props as a record."
            }
            TypeError::ComponentParamNotRecord { why: PropsRefusal::NotAnObject, .. } => {
                "A component takes one props record. Declare the parameter as one, e.g. `props: { name: string }`, and read `props.name`. If you call it as a function rather than render it as an element, declare it with `fn`."
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
                "Check the arm for a typo, or add the variant to the union. An arm names a variant the union has, or for a string-literal union a value the union has. A lowercase name would be a binding; a PascalCase name is read as a variant."
            }
            TypeError::UnknownLiteralPattern { .. } => {
                "Check the literal for a typo, or add the value to the union. If the arm is for a value the union no longer has, delete it."
            }
            TypeError::AwaitOutsideAsyncFn { .. } => {
                "Mark the enclosing function `async fn`, or call a non-async function here."
            }
            TypeError::MatchArmProducesNoValue { .. } => {
                "End the arm with an expression, or `return` from it. `X => {}` is a no-op only where the `match` is a statement."
            }
            TypeError::DestructureUnproven { why: DestructureRefusal::Count { .. }, .. } => {
                "Write one name or `_` per element, or end the pattern with `...rest` to take the elements left over."
            }
            TypeError::DestructureUnproven { why: DestructureRefusal::UnknownLength { .. }, .. } => {
                "Its length is known only at run time, so match on it with a fallback arm: `match xs { [a, b] => ..., else => ... }`."
            }
            TypeError::DestructureUnproven { why: DestructureRefusal::NotAnArray { .. }, .. } => {
                "Bind the value to one name and read from that. A `let` pattern takes an array whose length is known: a written array literal, or a tuple an npm library returns."
            }
            TypeError::PatternNotRecord { .. } => {
                "Bind the value to one name and read from it, `Loaded(items)`. An object pattern names the fields of a record."
            }
            TypeError::CaseBind { .. } => unreachable!("answered above with its rewrite"),
            TypeError::PendingNotAwaited { .. } => unreachable!("answered above with the call it names"),
            TypeError::AsyncCallbackIntoSyncVoid { .. } => unreachable!("answered above with the callback it names"),
            TypeError::UnseenMatchType { .. } => unreachable!("answered above with the module it names"),
            TypeError::UncalledBuiltinMethod { .. } => unreachable!("answered above with the method it names"),
            TypeError::OptionalChainOnTagged { .. } => unreachable!("answered above with the receiver it names"),
            TypeError::NoTextForm { .. } => unreachable!("answered above with the value it names"),
            TypeError::CoalesceOnTagged { .. } => unreachable!("answered above with its computed help"),
            TypeError::RecordRemoveFiniteKey { .. } => unreachable!("answered above with the key it names"),
            TypeError::SqlFromRuntimeValue { .. } => unreachable!("answered above with its computed help"),
            TypeError::MutationOutsideMut { .. } => unreachable!("answered above with its computed help"),
            TypeError::MutOnNonMutatingCall { .. } => unreachable!("answered above with its computed help"),
            TypeError::AbsentBuiltinResult { .. } => unreachable!("answered above with its computed help"),
            TypeError::TextOrderSort { .. } => unreachable!("answered above with its computed help"),
            TypeError::DeprecatedBuiltinMember { .. } => unreachable!("answered above with its computed help"),
        }))
    }

    /// An optional background note (the "why").
    ///
    /// Computed where the reason is in the program's own names: a `Record`
    /// key refusal (G373) names both keys. Every other note is fixed text.
    /// The note as a renderer holding the file shows it: [`Self::note`],
    /// except where the note names a line, which only the file can give.
    pub fn note_in(&self, source: &str) -> Option<Cow<'static, str>> {
        if let TypeError::TypeMismatch { place: Some(p), record_key: None, .. } = self {
            return Some(Cow::Owned(p.note(source)));
        }
        self.note()
    }

    pub fn note(&self) -> Option<Cow<'static, str>> {
        if let TypeError::TypeMismatch { record_key: Some(k), .. }
        | TypeError::ArgumentTypeMismatch { record_key: Some(k), .. } = self
        {
            return Some(Cow::Owned(k.note()));
        }
        self.fixed_note().map(Cow::Borrowed)
    }

    fn fixed_note(&self) -> Option<&'static str> {
        match self {
            TypeError::TypeParameterPattern { .. } => Some(TYPE_PARAMETER_PATTERN_NOTE),
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
            TypeError::UnseenMatchType { .. } => Some(
                "The type could be a literal union these arms cover, or a plain `string` they never can. Without its declaration the checker cannot tell which, and a match it cannot prove covers every value is not accepted.",
            ),
            TypeError::UncalledBuiltinMethod { .. } => Some(
                "A method reads its receiver from `this`, which a call through the member supplies. Read alone, the method is a bare function, and an ES module is strict, so calling it later runs it with `this` undefined and throws a TypeError. `tsc` accepts the read.",
            ),
            TypeError::NoTextForm { .. } => Some(
                "A value becomes text in three places: a `${}`, a `+` with a string, and `string.from`. Only a string, a number or a bool has a text form; anything else prints JavaScript's default, such as `[object Object]`, and `tsc` accepts all three.",
            ),
            TypeError::OptionalChainOnTagged { .. } => Some(
                "`Option` and `Result` are tagged values at run time and never null, so `?.` never short-circuits on one: it reads the member off the tag object. `match` is the one place either is opened, and `?` is the one way a `Result` propagates.",
            ),
            TypeError::CoalesceOnTagged { .. } => Some(
                "`Option` and `Result` are tagged values at run time and never null or undefined, so `??` never replaces one: its default stands in only for an operand that is missing, and a present `Option` comes back as the tag object, which prints as `[object Object]`. `tsc` accepts it. `match` is the one place either is opened.",
            ),
            TypeError::MutationOutsideMut { .. } => Some(
                "Every change to a value whose type Glyph knows is a `mut` statement (D5), so `grep -n \"^\\s*mut \"` lists all of them. A call that changes its receiver counts, and it writes to a place, the same places `mut x = e` can: a name, a field, an index.",
            ),
            TypeError::MutOnNonMutatingCall { .. } => Some(
                "`mut` marks a change (D5), so `grep -n \"^\\s*mut \"` lists the program's writes. A call that changes nothing would put a line in that list that writes nothing, and the value it returns is thrown away.",
            ),
            TypeError::AbsentBuiltinResult { .. } => Some(
                "A missing value in Glyph is an `Option`, which `match` opens and the checker holds to a `None` arm (D56). These methods hand back JavaScript's `undefined` instead, a value Glyph's checker cannot see, so a read off it passes `glyph check` and fails at run time; the `std/*` functions are the same lookups with the absence in the type.",
            ),
            TypeError::TextOrderSort { .. } => Some(
                "With no comparator, JavaScript's `sort` and `toSorted` convert each element to a string and compare the strings, so numbers come out in the order of their digits and a record sorts as `[object Object]`. That order is right for text alone. `tsc` accepts the call on any array, and `array.sort` always takes a comparator.",
            ),
            TypeError::DeprecatedBuiltinMember { .. } => Some(
                "TypeScript's library marks this member `@deprecated`. It still runs in every engine, since Annex B of the language keeps it, and `tsc --strict` accepts it, so this is a warning and the build goes on. An editor on TypeScript's language service strikes the member through, and `tsc` on the command line says nothing.",
            ),
            TypeError::SqlFromRuntimeValue { .. } => Some(
                "A query's text is fixed when the program is written (D52). Values go in the parameter array, where SQLite binds them as data and never reads them as SQL; `sqlite.from_trusted` is the one way to run text built at run time, and it takes a `taint.Trusted<string>`.",
            ),
            TypeError::NullableRead { .. } => Some(
                "`Nullable<T>` is `T | null` (D45), and `?.` over a null value gives JavaScript's `undefined`, which Glyph has no type for. The chain may stand only where the absence is handled on the spot: the left of `??`, or an operand of `==` or `!=`.",
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
            TypeError::UnknownLiteralPattern { .. } => Some(
                "A string-literal union is a closed set of values (D30), so a value outside it never reaches the match and the arm is dead code.",
            ),
            TypeError::AwaitOutsideAsyncFn { .. } => Some(
                "A call to an `async fn -> T` is a pending `T` until it is awaited (D51), and `await` only appears inside an async callable.",
            ),
            TypeError::PendingNotAwaited { .. } => Some(
                "Calling an async function starts it and hands back a pending value (D51). Only `await` waits for it and sees its failure; a pending value may also be returned from an async callable or bound to a name that is awaited later.",
            ),
            TypeError::AsyncCallbackIntoSyncVoid { .. } => Some(
                "A `fn(...) -> void` parameter is called and its result dropped, so an `async fn` passed there is started and never awaited (D40). TypeScript accepts it; Glyph holds `fn` and `async fn` apart, the `void` slot included.",
            ),
            TypeError::MatchArmProducesNoValue { .. } => Some(
                "A value-position arm lowers to `case X: { break; }` when it yields nothing, so the value would be `undefined` at run time.",
            ),
            TypeError::DestructureUnproven { .. } => Some(
                "A destructuring `let` always matches (D48): every name it binds holds an element, never the `undefined` a short array leaves behind. The length is proved, never assumed.",
            ),
            TypeError::CaseBind { .. } => Some(
                "A `<case>` is a `match` arm written in JSX (D6): its positional attribute is the arm's pattern, checked, bound and counted for exhaustiveness the way the arm's is.",
            ),
            TypeError::ComponentParamNotRecord { .. } => Some(
                "React calls a component with the one props object its element builds, so the parameter holds that object whatever it declares: `<Greeting />` hands it `{}`, and an element with attributes does not type-check (TS2769).",
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

    /// The required record fields this error reports absent, in the declared
    /// record's order, when it reports any.
    ///
    /// `None` on every other error, and on a type mismatch that is not about a
    /// missing field, for the reason `missing_variants` answers `None`: an
    /// empty list would claim nothing is missing.
    pub fn missing_fields(&self) -> Option<&[String]> {
        match self {
            TypeError::TypeMismatch { missing_fields, .. }
            | TypeError::ArgumentTypeMismatch { missing_fields, .. }
                if !missing_fields.is_empty() =>
            {
                Some(missing_fields)
            }
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
            // "a type with a runtime check" is a rule; the parameter is the
            // type in hand, as the emitter's E0304 reports it.
            TypeError::TypeParameterPattern { param, .. } => Some(param),
            // "a props record" is a rule, as "a resource type" is above.
            TypeError::ComponentParamNotRecord { ty, .. } => Some(ty),
            // "an array of known length" is a rule too.
            TypeError::DestructureUnproven { why, .. } => why.type_shown(),
            // "a record" is a rule too.
            TypeError::PatternNotRecord { ty, .. } => Some(ty),
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
            TypeError::UnknownVariantPattern { suggestion, .. }
            | TypeError::UnknownLiteralPattern { suggestion, .. } => {
                suggestion.as_ref().map(|s| vec![s.clone()])
            }
            _ => None,
        }
    }

    /// What kind of thing `alternatives` holds here: variant names, the
    /// contents of string literals, a record's fields, a built-in value's
    /// members.
    ///
    /// Answers wherever `alternatives` answers and nowhere else, so a consumer
    /// reads the two keys as one fact. Read off the error variant rather than
    /// inferred from the names, which cannot be done: `Paid` and `read` are
    /// both bare identifiers on the wire.
    pub fn alternatives_kind(&self) -> Option<AlternativesKind> {
        match self {
            // A built-in receiver's list is its members, and so is a prelude
            // `Option`'s or `Result`'s; no declaration holds either. A
            // record's is its fields, which one does.
            TypeError::UnknownField { fields, builtin: Some(_), .. }
            | TypeError::UnknownField { fields, tagged: Some(_), .. }
                if !fields.is_empty() =>
            {
                Some(AlternativesKind::Members)
            }
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
            TypeError::UnknownLiteralPattern { suggestion, .. } => {
                suggestion.as_ref().map(|_| AlternativesKind::Literals)
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

/// The rewrite E0231's help spells out, by what the variant carries. `name` is
/// the one `bind={name}` gave, kept so the rewrite binds the name the body
/// already reads. For a record payload that is also the field `bind` read, so
/// `<case Loaded({ users })>` reads exactly what `bind={users}` did.
fn case_bind_help(variant: &str, name: &str, shape: CaseBindShape) -> String {
    match shape {
        CaseBindShape::Record => format!(
            "The payload of `{variant}` is a record, so name the fields you read: write `<case {variant}({{ {name} }})>`. `{{ {name}: other }}` binds the field under another name."
        ),
        CaseBindShape::Positional => format!(
            "The payload of `{variant}` is one value, so bind it to a name: write `<case {variant}({name})>`."
        ),
        CaseBindShape::Nullary => format!(
            "`{variant}` carries no payload, so there is nothing to bind: write `<case {variant}>`."
        ),
        CaseBindShape::Unknown => format!(
            "Bind the payload with a pattern: `<case {variant}({name})>` for one value, `<case {variant}({{ {name} }})>` for a field of a record payload."
        ),
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
