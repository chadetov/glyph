//! Glyph emit — AST-to-TypeScript visitor (Phase 1 week 4).
//!
//! A dumb visitor, no IR, per the Q5 hybrid resolution. Emitted TS may be
//! ugly; humans read Glyph, agents read Glyph, and `tsc --strict` reads the
//! output. Every top-level declaration is `export`ed so the three D15 import
//! forms round-trip.
//!
//! ## This slice (first emission day)
//!
//! Implemented: modules + imports (D15), `fn` declarations (generics, params,
//! return types, async), `const`, simple `type` aliases, blocks and the
//! statement forms (`let`, `mut`, `return`, `for`, `loop`, `break`,
//! `continue`), the expression forms (literals, D22 template literals, ident,
//! binary/unary, call with type args, member/index, `await`, array/object
//! literals with spread, lambdas), and type annotations (primitives, generic
//! applications, function and record types).
//!
//! Tagged unions lower to a TS discriminated union on a `tag` field plus a
//! constructor per variant (a `const` for a no-payload variant, a function for
//! a payload variant; record payloads spread their fields). A generic union
//! carries its type parameters on the alias; each constructor is generic over
//! only the parameters its own payload mentions, and the rest are widened to
//! `never` in its return type (so `Left({ a: A })` of `Either<A, B>` emits
//! `Left<A>(...): Either<A, never>`, and a no-payload variant becomes a `const`
//! of `Either<never, never>`) — every constructor then fits every
//! instantiation. A
//! non-generic record type additionally emits a Q8 runtime descriptor — an
//! `export const X = { is(v): v is X { ... }, parse(v) { ... } }` whose `is`
//! predicate shallowly validates each field (primitives by `typeof`, others by
//! presence) so `is TypeName` checks hold at runtime, and whose `parse` reuses
//! that guard to validate an `unknown` into a `Result` (the inline `Ok`/`Err`
//! shape, so the descriptor needs no `std/result` import).
//!
//! A `match` over a tagged union lowers to a `switch` on the `tag`
//! discriminant, with constructor-pattern arms (`Ok(x)`, `NetworkError({ url })`)
//! binding the payload and `_`/`else` becoming `default`. A `match` over a
//! primitive with literal arms (`match n { 0 => .., else => .. }`) switches on
//! the scrutinee value directly. In statement position (`return match`, or a
//! bare `match` statement) the switch is emitted directly so `return` keeps its
//! function semantics; in value position (`let x = match`, nested) it is
//! wrapped in an immediately-invoked arrow.
//!
//! The `?` operator unwraps a `Result`: it binds the operand to a temporary,
//! returns it on `Err`, and reads the `Ok` payload. A `?` nested inside a
//! larger expression — mid-chain (`await x?.foo()`), an argument (`f(x?)`), a
//! template — is hoisted out to a preceding statement first (`hoist_tries`),
//! and the `?` node is replaced by a read of the temporary's `Ok` payload; a
//! whole-value `?` goes through the same path. Glyph async is colorless, so
//! `await` on a method chain is placed on the head async call of the receiver
//! spine (`(await load(p)).map_err(f)`), not the whole chain.
//!
//! A block-body match arm (`Variant => { stmts }`) emits its statements into
//! the case; it is supported in statement position (where a block `return`
//! returns from the function) but rejected in value position (an IIFE arrow
//! would capture the return).
//!
//! A type-guard `match` (`is TypeName` arms) lowers to an `if`/`else if` chain:
//! `is string` → `typeof __m === "string"`, `is User` → `User.is(__m)` (the Q8
//! record descriptor), `is Record<...>`/`is Array<...>` → an object /
//! `Array.isArray` check; a missing `else` throws.
//!
//! An array `match` (`[]`, `["add", ...rest]`, `[a, b]`) also lowers to an
//! `if`/`else if` chain: each arm is a length check (`=== n`, or `>= n` with a
//! `...rest`) joined with an equality check per literal element; identifier
//! elements bind by index and a `...rest` binds `slice(n)`. Source order is
//! match order; a missing `_`/`else` throws (the typechecker proves array
//! exhaustiveness, so the throw is unreachable for a well-typed match).
//!
//! A non-`void` function, lambda, or block implicitly returns its tail
//! expression (Glyph block value, like Rust): a bare tail expression becomes
//! `return expr`, a tail `match` returns each arm's value, a tail `E?` returns
//! its `Ok` payload. A `void`/unannotated function runs its tail for effect.
//!
//! A nested constructor pattern (`Err(NetworkError({ s }))`) is rewritten so
//! each outer variant with nested arms dispatches its payload through an inner
//! `match` (the `Err(..)` arms collapse to one `case "Err"` with an inner
//! switch); deeper nesting recurses through the same rewrite.
//!
//! A `component` (D19) emits as a React function component; JSX (D6) lowers to
//! `React.createElement(tag, props, ...children)`. The directives lower
//! structurally: `<if>`/`<else>` → a ternary, `<for x in={xs}>` → `xs.map`,
//! `<match value={v}>` with `<case V bind={x}>` arms → a switch-returning IIFE
//! binding `x` to the same-named payload field.
//!
//! Deferred, surfaced as `EmitError::Unsupported` rather than emitting invalid
//! TS: value-position block arms, object match patterns and nested
//! non-constructor patterns inside a constructor or array arm, and `is` checks
//! on union/generic/imported types.
//!
//! ## Reserved-word identifiers (handled upstream)
//!
//! Glyph's lexer permits TS reserved words (`class`, `default`, `new`, `eval`,
//! ...) as soft-keyword identifiers, and this emitter copies a binding /
//! parameter / import name (a tagged-union variant's constructor name, and a
//! record type's descriptor `const` name) verbatim, so such a name would reach
//! `tsc` in a binding position where it is illegal. (Object keys, record
//! fields, and member access are safe — only binding positions break.)
//! The resolver now rejects these at the source with `E0109`
//! (`glyph-resolver`'s `reserved` module, checked in `collect`'s `intern_vis`
//! and `resolve`'s `bind_local`), so no reserved word reaches emit — the
//! "stricter-than-TS" fix, not emit-time mangling (which would break
//! import-name matching across modules).
//!
//! One more gap in the same family, fixed once type context is threaded into
//! the emitter (or by a resolver rule):
//! - The lowering synthesizes `__`-prefixed temporaries (`__mN` for match
//!   scrutinees, `__rN` for `?` operands); a user identifier with one of those
//!   exact names would collide. A resolver rule reserving the `__` prefix is
//!   the proper fix.

#![forbid(unsafe_code)]

use glyph_ast::{
    ArrayElem, BinOp, Block, ComponentDecl, Decl, Expr, FnTypeParam, GenericParam, Ident,
    ImportDecl, ImportKind, JsxAttr, JsxChild, JsxElement, LiteralPattern, MatchArm, MatchArmBody,
    Module, MutKind, ObjectField, Param, Pattern, PostfixOp, RecordTypeField, Span, Stmt,
    TemplatePart, TypeExpr, UnaryOp, UnionVariant,
};
use glyph_resolver::{Prelude, ResolvedModule, ResolvedRef, SymbolId, SymbolKind};
use glyph_typechecker::{Primitive, Ty, TypeMap};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// A construct whose emission lands in a later week-4 day. Carries the
    /// construct name (for the diagnostic) and the offending span.
    #[error("TS emission for {construct} is not implemented yet")]
    Unsupported { construct: &'static str, span: Span },
    /// An `<else>` (D6) that is not the immediate sibling of its `<if>`. This
    /// is an intentional adjacency rule, not a missing feature: the pairing is
    /// only recognized when the `<else>` directly follows its `<if>` (a sibling
    /// element between them breaks it).
    #[error("an `<else>` must immediately follow its `<if>`")]
    MisplacedElse { span: Span },
    /// A `?` inside an arm of a `match` that is nested in a larger expression.
    /// That match lowers to an immediately-invoked arrow, and `?` returns from
    /// the enclosing *function*, which an arrow cannot do. This is a positional
    /// rule, not a missing feature: the same arm compiles when the match is the
    /// whole value of a `let`/`mut`/`return` (which is a value-position match,
    /// and is exactly why this is not named for that position).
    #[error("`?` cannot propagate out of a `match` used inside a larger expression")]
    TryInNestedExpressionMatch { span: Span },
    /// A `?` in a position the emitter never hoists out of: the operand of a
    /// construct that is rendered through `expr` rather than through a
    /// statement's value (a `match` scrutinee, for instance). `?` expands to a
    /// preceding `const` plus an early `return`, so it can only appear where a
    /// statement can be inserted ahead of it. A positional rule, not a missing
    /// feature.
    #[error("`?` cannot be used in this position")]
    TryInUnhoistablePosition { span: Span },
    /// `T.parse`/`T.is` on a record holding a field whose type has no runtime
    /// check. Declaring the record is fine; trusting one at a boundary is not.
    /// The descriptor used to emit a branch that could never fire, under a
    /// message naming the type it never checked, so `parse` reported success
    /// for a value it had not validated.
    #[error("cannot validate `{type_name}`: field `{field}` has type `{field_ty}`, which has no runtime check")]
    UnverifiableDescriptorUse {
        type_name: String,
        field: String,
        field_ty: String,
        span: Span,
    },
}

impl EmitError {
    pub fn span(&self) -> Span {
        match self {
            EmitError::Unsupported { span, .. } => *span,
            EmitError::MisplacedElse { span } => *span,
            EmitError::TryInNestedExpressionMatch { span } => *span,
            EmitError::TryInUnhoistablePosition { span } => *span,
            EmitError::UnverifiableDescriptorUse { span, .. } => *span,
        }
    }

    /// Stable diagnostic code (emit range `E03xx`; see `docs/error-codes.md`).
    pub fn code(&self) -> &'static str {
        match self {
            EmitError::Unsupported { .. } => "E0300",
            EmitError::MisplacedElse { .. } => "E0301",
            EmitError::TryInNestedExpressionMatch { .. } => "E0302",
            EmitError::TryInUnhoistablePosition { .. } => "E0303",
            EmitError::UnverifiableDescriptorUse { .. } => "E0304",
        }
    }

    /// A one-line, actionable fix.
    pub fn help(&self) -> Option<&'static str> {
        match self {
            EmitError::Unsupported { .. } => {
                Some("Rewrite using a construct the v1 emitter supports; see the spec for the supported forms.")
            }
            EmitError::MisplacedElse { .. } => Some(
                "Move the `<else>` so it is the next sibling after its `<if>`; remove or relocate any element that sits between them.",
            ),
            EmitError::TryInNestedExpressionMatch { .. } => Some(
                "Bind the match first (`let x = match ... { ... }`) and use `?` there, or move the `?` out of the arm.",
            ),
            EmitError::TryInUnhoistablePosition { .. } => Some(
                "Bind the operand first (`let r = f(x)?`) and use `r` here.",
            ),
            EmitError::UnverifiableDescriptorUse { .. } => Some(
                "Split the wire type from the domain type: parse a record whose fields are all checkable, then build this one from it.",
            ),
        }
    }

    /// An optional background note explaining the rule behind the error.
    pub fn note(&self) -> Option<&'static str> {
        match self {
            EmitError::Unsupported { .. } => None,
            EmitError::MisplacedElse { .. } => Some(
                "`<else>` is paired with its `<if>` only when it is the immediately following sibling; a sibling element (such as a `<p>`) between them breaks the pairing. This is an intentional D6 restriction.",
            ),
            EmitError::TryInNestedExpressionMatch { .. } => Some(
                "A match nested in a larger expression lowers to an arrow, and `?` returns from the enclosing function, which an arrow cannot do. A match that is the whole value of a `let`/`mut`/`return` lowers to a statement `switch`, where `?` works.",
            ),
            EmitError::TryInUnhoistablePosition { .. } => Some(
                "`?` expands to a `const` binding plus an early `return` placed before the statement it appears in, so it is only legal where such a statement can be inserted. A `match` scrutinee is one of the positions that is emitted as a plain expression.",
            ),
            EmitError::UnverifiableDescriptorUse { .. } => Some(
                "A record may hold a value the compiler cannot check (a socket, an `extern_ts` type, an `unknown`); holding one is ordinary. What is refused is `parse`/`is` on it, because a boundary that reports success has to have checked what it claims. The check propagates: a field whose type is itself such a record, or an array or `Option` of one, is unverifiable for the same reason.",
            ),
        }
    }
}


/// What a record descriptor can say about one field at runtime.
///
/// The distinction E0304 rests on. A type that *names* something the emitter
/// cannot see into (a host handle, an `extern_ts` type, a generic tagged union)
/// makes the descriptor claim a shape it never checked. `unknown` is not that
/// case: it claims nothing, every value satisfies it, so for a required field
/// presence is the entire check and there is nothing to lie about.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FieldCheck {
    /// A real predicate over the value.
    Deep(String),
    /// The field is `unknown`: being there is all there is to check.
    PresenceOnly,
    /// Nothing can check it, and the declared type names something specific.
    Unverifiable,
}

/// The discriminant field of an emitted tagged-union value. Single-sourced
/// here because the forthcoming `match` → `switch` and `?` lowering must read
/// the same field these constructors write.
///
/// ## ADT representation contract (read before writing match/`?` lowering)
///
/// A variant value is a flat object `{ tag: "Variant", ...payload }`:
/// - **No payload** → `{ tag: "Variant" }` (emitted as an exported `const`).
/// - **Record payload** `Variant({ a, b })` → `{ tag: "Variant", a, b }` — the
///   record fields are spread flat. A `Variant({ a })` object-pattern reads
///   `scrutinee.a`; a whole-payload bind `Variant(p)` must reconstruct the
///   record from those flat fields.
/// - **Non-record payload** `Variant(T)` → `{ tag: "Variant", value: <T> }`;
///   a `Variant(x)` bind reads `scrutinee.value`.
///
/// A payload field named `tag` would collide with the discriminant and is
/// rejected at emit (see `emit_union`).
const TAG: &str = "tag";

/// The field a non-record (single-value) payload is stored under, e.g. `Ok(x)`
/// → `{ tag: "Ok", value: x }`. The sibling of `TAG`; single-sourced because
/// the union constructors write it and `match` lowering reads it.
const PAYLOAD: &str = "value";

/// D24: the placeholder a `@redact`ed field is replaced with in the descriptor's
/// `redact(value)` masking copy.
const REDACTION_SENTINEL: &str = "[REDACTED]";

/// The error variant tag of the prelude `Result`. The `?` lowering tests it to
/// propagate failures; single-sourced alongside `TAG`/`PAYLOAD` since it is
/// part of the same `Result` wire-format contract.
const RESULT_ERR: &str = "Err";

/// The success variant tag of the prelude `Result`. A record descriptor's
/// `parse` builds an `Ok` of the validated value; single-sourced with
/// `RESULT_ERR` since both are the same `Result` wire-format contract.
const RESULT_OK: &str = "Ok";

/// The local name the `?` lowering binds the prelude `Err` constructor to, used
/// to re-wrap a propagated error (`return __glyph_err(__r.value)`). Re-wrapping
/// yields a `Result<never, E>`, which is assignable to any `Result<Y, E>` the
/// enclosing function returns — required because `Result` now carries
/// `T`-dependent combinator methods (`map`/`map_err`). Aliased so it never
/// collides with a user import of `Err`. A module that uses `?` gets a
/// generated `import { Err as __glyph_err } from "std/result"`.
const ERR_CTOR: &str = "__glyph_err";

/// The local name a descriptor's `parse` binds the prelude `Ok` constructor to.
/// A descriptor's `parse` returns a real `Result` (not a bare `{tag,value}`
/// object), so its success value has to be built by the prelude constructor that
/// attaches the `map`/`map_err` combinators. Aliased for the same reason
/// [`ERR_CTOR`] is: it must never collide with a user import of `Ok`.
const OK_CTOR: &str = "__glyph_ok";

/// The local name the injected `std/result` import binds the prelude `Result`
/// *type* to, used to annotate a descriptor's `parse` return
/// (`__GlyphResult<User, Issue[]>`). Aliased like the constructors so a user
/// type named `Result` is never shadowed.
const RESULT_TY: &str = "__GlyphResult";

/// The local name a record descriptor binds the prelude `schema` factory to,
/// for its auto-generated `T.schema` member (`T.schema = __glyph_schema<T>(...)`).
/// Aliased so it never collides with a user binding; a module that emits any
/// record descriptor gets `import { schema as __glyph_schema } from "std/schema"`.
const SCHEMA_FACTORY: &str = "__glyph_schema";

/// The TS mapped-type alias `infer_output<S>` (D28) lowers to. A module that
/// renders any `infer_output` gets one injected alias that maps each field of a
/// record of parsers to that parser's output type. It matches a field
/// *structurally* — any `{ parse(input: unknown): Result<V, _> }` shape — so it
/// is independent of what the validator type is named (`Schema`, `Codec`, a
/// user's own), reading the `Ok` payload out of the parse result's wire form
/// (`{ tag: "Ok"; value: V }`). Self-contained: it references no in-scope type.
/// Aliased to never collide with a user type.
const INFER_OUTPUT_ALIAS: &str = "__GlyphInferOutput";

/// The JavaScript globals the emitter writes into its own output.
///
/// One list, defined in the resolver because the drift test that keeps it in
/// step with this file lives beside it. A module that declares one of these as
/// a top-level name used to be rejected (E0110), because `export function
/// Error(...)` shadows the `Error` the emitted `?` lowering and descriptor
/// throws depend on. A domain type is allowed to be called `Error`: a
/// spreadsheet cell really is `Number | Text | Empty | Error`. So instead of
/// taking the name away from the author, the module captures the global under a
/// private alias and the emitter's own references go through that. The author's
/// name emits verbatim, which is the half that matters for grep.
use glyph_resolver::JS_GLOBALS as EMITTER_GLOBALS;

/// `Error` -> `__glyph_Error`. Only ever used in a module that shadows it.
fn global_alias(name: &str) -> String {
    format!("__glyph_{name}")
}

/// The prelude tagged-union constructors (`std/result`, `std/option`). Their
/// discriminant tags are fixed by the runtime regardless of whether the
/// scrutinee's type resolved, so the `match` lowering treats them as variant
/// tags even when `union_variant_names` returns `None` (a prelude scrutinee
/// has no user `Decl::Type` to read variants from). This is what lets a bare
/// `None` arm lower to `case "None":` instead of a binding `default:`, and
/// what lets `degroup_nested_arms` group `Ok(None)` with `Ok(Some(x))`.
const PRELUDE_VARIANTS: [&str; 4] = ["Ok", "Err", "Some", "None"];

/// Whether `name` is a prelude tagged-union constructor (see `PRELUDE_VARIANTS`).
fn is_prelude_variant(name: &str) -> bool {
    PRELUDE_VARIANTS.contains(&name)
}

/// The relative module specifier from importer module `from` to imported module
/// `to`, both `/`-joined module paths (e.g. `sub/a`). The emitted `.ts` tree
/// mirrors the module paths, so the specifier is the path from the importer's
/// directory to the target file, extensionless (`bundler` resolution adds it).
/// `sub/a` importing `sub/b` -> `./b`; `sub/a` importing `top` -> `../top`;
/// `a` importing `sub/b` -> `./sub/b`.
fn relative_specifier(from: &str, to: &str) -> String {
    let from_segs: Vec<&str> = from.split('/').collect();
    // The importer's directory is its path minus the file component.
    let from_dir = &from_segs[..from_segs.len().saturating_sub(1)];
    let to_segs: Vec<&str> = to.split('/').collect();
    let to_dir_len = to_segs.len().saturating_sub(1);

    // Drop the shared leading directories.
    let mut i = 0;
    while i < from_dir.len() && i < to_dir_len && from_dir[i] == to_segs[i] {
        i += 1;
    }
    let ups = from_dir.len() - i;
    let mut out = String::new();
    if ups == 0 {
        out.push_str("./");
    } else {
        for _ in 0..ups {
            out.push_str("../");
        }
    }
    out.push_str(&to_segs[i..].join("/"));
    out
}

/// The relative specifier from a module at `module_path` to a file bundled
/// under `.glyph-runtime/` at the output root (`glyph-bootstrap`,
/// `std/result`, ...). A root module reaches it with `./`; a nested one
/// (`sub/a`) needs one `../` per enclosing directory. Extensionless, matching
/// `relative_specifier` (both `tsc`'s `bundler` resolution and external
/// bundlers add the extension).
///
/// Every runtime reference is relative for the same reason the bootstrap
/// import became relative: a host toolchain compiling the emitted files under
/// its own configuration (a Vite app importing a generated module, a host
/// project's `tsc`) resolves a relative path natively, where a bare `std/*`
/// specifier resolves only under the generated `tsconfig.json`'s `paths` map,
/// which the host never reads (G122).
fn runtime_specifier(module_path: &str, tail: &str) -> String {
    let dir_depth = module_path.split('/').count().saturating_sub(1);
    let mut spec = String::new();
    if dir_depth == 0 {
        spec.push_str("./");
    } else {
        for _ in 0..dir_depth {
            spec.push_str("../");
        }
    }
    spec.push_str(".glyph-runtime/");
    spec.push_str(tail);
    spec
}

/// The relative specifier from a module at `module_path` to the bundled runtime
/// bootstrap, which sits at the output root.
fn bootstrap_specifier(module_path: &str) -> String {
    runtime_specifier(module_path, "glyph-bootstrap")
}

/// Whether a constructor pattern's single argument is itself a variant pattern
/// (so the outer arm needs an inner dispatch on the payload's tag): a nested
/// constructor (`Ok(Some(x))`) or a bare no-payload prelude variant
/// (`Ok(None)`, which parses as a `Pattern::Ident`, not a `Pattern::Constructor`).
fn is_nested_variant_arg(p: &Pattern) -> bool {
    match p {
        Pattern::Constructor { .. } => true,
        // A literal payload (`Ok(true)`, `Some(0)`) is a nested pattern too: it
        // is degrouped into an inner value-match on the payload.
        Pattern::Literal { .. } => true,
        Pattern::Ident { name, .. } => is_prelude_variant(name),
        _ => false,
    }
}

/// How a lowered `match` arm yields control: `return` its value (the match is
/// in return position) or run it for effect and `break` (statement position).
#[derive(Clone, Copy)]
enum ArmTerm {
    Return,
    Break,
    /// A value-position `match` lowered as a statement `switch`: a value arm
    /// assigns the binding in `self.assign_target` (then breaks), while a block
    /// arm's `return` still returns from the function. Lets `let x = match { ...
    /// None => return Err(e) }` compile without an IIFE capturing the `return`.
    Assign,
}

/// Project context the emitter needs to resolve cross-module import specifiers.
/// A project (sibling `.glyph`) import must emit a relative specifier (`./x`)
/// rather than the bare module path, which neither `tsc` nor `tsx` resolves;
/// `std/*` is left bare (tsconfig-mapped) and an external npm package (e.g.
/// `react`) is left bare too. The default (`EmitContext::single`) treats every
/// import as non-project, which is correct for a one-module program.
#[derive(Clone, Copy)]
pub struct EmitContext<'a> {
    /// The importing module's own path (e.g. `sub/a`), used to compute the
    /// relative path to a sibling module.
    pub module_path: &'a str,
    /// Every project module path, so a project import is told apart from a
    /// `std/*` or external one.
    pub project_modules: &'a std::collections::BTreeSet<String>,
    /// `(module path, variant name)` for every record-payload variant across the
    /// project, so a `Variant(v)` bind on an *imported* union knows whether to
    /// bind the whole object (record payload) or `.value` (single-value payload).
    /// Empty for a single-module build (no imported project unions).
    pub record_payload_variants: &'a std::collections::BTreeSet<(String, String)>,
    /// `(module path, type name) -> arity` for every generic record descriptor
    /// across the project, so an *imported* generic descriptor's
    /// `Imported.parse<T>(v)` call threads its runtime checker argument (a
    /// module-local scan resolves the import to arity 0 and would drop it).
    /// Empty for a single-module build (no imported generic descriptors).
    pub generic_descriptor_arities: &'a std::collections::BTreeMap<(String, String), usize>,
    /// `(module path, type name)` for every *non-generic* exported type across
    /// the project that emits a runtime descriptor (a record, a tagged union
    /// whose descriptor name is free, or a D39 refined primitive). Without it a
    /// field typed by an imported record or refined alias resolves to no
    /// descriptor and falls to the `!== undefined` presence floor, so the
    /// emitted boundary check is weaker than the type declares.
    /// Empty for a single-module build (no imported descriptors).
    pub plain_descriptors: &'a std::collections::BTreeSet<(String, String)>,
    /// `(module path, type name) -> body` for every *non-generic* exported type
    /// across the project that emits **no** runtime descriptor: a string-literal
    /// union (D30), an alias to a primitive (`type Count = int`), an alias to
    /// another such alias. A module-local scan resolves these through
    /// `resolve_alias_leaf`, so a field typed by a local `"text" | "int"` gets a
    /// membership check; the same type imported from a sibling resolved to
    /// nothing and fell to the `!== undefined` floor, which accepts any string.
    /// That is the D30 guarantee evaporating at a module boundary, the same hole
    /// G76 closed for `match` exhaustiveness.
    /// Empty for a single-module build (no imported aliases).
    pub descriptorless_aliases: &'a std::collections::BTreeMap<(String, String), TypeExpr>,
}

impl<'a> EmitContext<'a> {
    /// Context for a standalone single-module program: no project siblings, so
    /// every import stays bare. `EMPTY` backs the borrow.
    pub fn single() -> Self {
        EmitContext {
            module_path: "",
            project_modules: &EMPTY_MODULES,
            record_payload_variants: &EMPTY_VARIANTS,
            generic_descriptor_arities: &EMPTY_ARITIES,
            plain_descriptors: &EMPTY_DESCRIPTORS,
            descriptorless_aliases: &EMPTY_ALIASES,
        }
    }
}

static EMPTY_VARIANTS: std::sync::LazyLock<std::collections::BTreeSet<(String, String)>> =
    std::sync::LazyLock::new(std::collections::BTreeSet::new);

static EMPTY_DESCRIPTORS: std::sync::LazyLock<std::collections::BTreeSet<(String, String)>> =
    std::sync::LazyLock::new(std::collections::BTreeSet::new);

static EMPTY_ALIASES: std::sync::LazyLock<
    std::collections::BTreeMap<(String, String), TypeExpr>,
> = std::sync::LazyLock::new(Default::default);

static EMPTY_ARITIES: std::sync::LazyLock<std::collections::BTreeMap<(String, String), usize>> =
    std::sync::LazyLock::new(std::collections::BTreeMap::new);

static EMPTY_MODULES: std::sync::LazyLock<std::collections::BTreeSet<String>> =
    std::sync::LazyLock::new(std::collections::BTreeSet::new);

/// Emit a whole module to a TypeScript source string. `resolved` and `types`
/// are the resolution and type-inference results for `module`; the emitter
/// consults them where lowering needs the scrutinee's type (e.g. to tell a
/// bare-identifier variant arm from a binding). `ctx` carries the project
/// module set so cross-module imports emit resolvable specifiers.
pub fn emit_module(
    module: &Module,
    resolved: &ResolvedModule,
    types: &TypeMap,
    prelude: &Prelude,
    ctx: EmitContext,
) -> Result<String, EmitError> {
    emit_module_mapped(module, resolved, types, prelude, ctx).map(|o| o.ts)
}

/// Which `EMITTER_GLOBALS` this module shadows with a top-level declaration.
///
/// Covers every name that reaches the emitted module's top level: a `fn`,
/// `type`, `const` or `component`, and a tagged union's variant constructors,
/// which are emitted as top-level `const`/`function` declarations of their own.
fn shadowed_globals_of(module: &Module) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut note = |name: &str| {
        if EMITTER_GLOBALS.contains(&name) {
            out.insert(name.to_string());
        }
    };
    for item in &module.items {
        match item {
            Decl::Fn(f) => note(f.name.as_ref()),
            Decl::Const(c) => note(c.name.as_ref()),
            Decl::Component(c) => note(c.name.as_ref()),
            Decl::Type(td) => {
                note(td.name.as_ref());
                if let TypeExpr::Union { variants, .. } = &td.body {
                    for v in variants {
                        note(v.name.as_ref());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The emitted TypeScript plus a coarse source map back to Glyph spans, for
/// remapping `tsc` diagnostics onto the original `.glyph` source.
pub struct EmitOutput {
    pub ts: String,
    /// `(byte offset into `ts`, originating Glyph span)`, strictly increasing.
    pub source_map: Vec<(usize, Span)>,
}

/// Like [`emit_module`], but also returns the source map (see [`EmitOutput`]).
pub fn emit_module_mapped(
    module: &Module,
    resolved: &ResolvedModule,
    types: &TypeMap,
    prelude: &Prelude,
    ctx: EmitContext,
) -> Result<EmitOutput, EmitError> {
    let shadowed = Rc::new(shadowed_globals_of(module));
    let mut e = Emitter {
        out: String::new(),
        indent: 0,
        tmp_counter: 0,
        used_try: Rc::new(Cell::new(false)),
        used_schema: Rc::new(Cell::new(false)),
        used_result: Rc::new(Cell::new(false)),
        used_infer_output: Rc::new(Cell::new(false)),
        shadowed_globals: shadowed,
        desc_param_guards: RefCell::new(Vec::new()),
            assign_target: RefCell::new(None),
        return_cast: None,
        emit_export: "export ",
        loop_labels: Vec::new(),
        module,
        resolved,
        prelude,
        types,
        synth_types: Rc::new(RefCell::new(HashMap::new())),
        source_map: Vec::new(),
        ctx,
    };
    e.emit_module()?;
    Ok(EmitOutput {
        ts: e.out,
        source_map: e.source_map,
    })
}

struct Emitter<'a> {
    out: String,
    indent: usize,
    /// Counter for synthesized scrutinee temporaries (`__m0`, `__m1`, ...), so
    /// two `match` statements in one function body don't redeclare the name.
    tmp_counter: usize,
    /// Set once any `?` is lowered (including inside a lambda or value-position
    /// match rendered by a sub-emitter), so the module gets the generated `Err`
    /// import the re-wrap needs. Shared across the main emitter and every
    /// sub-emitter via the `Rc<Cell>`.
    used_try: Rc<Cell<bool>>,
    /// Set once any record descriptor is emitted, so the module gets the
    /// generated `schema` factory import its `T.schema` member needs.
    used_schema: Rc<Cell<bool>>,
    /// Set once any descriptor's `parse` is emitted, so the module gets the
    /// `Ok`/`Err` constructors and the `Result` type its return needs. Merged
    /// with `used_try` into one `std/result` import line, since both bind
    /// `__glyph_err` and two declarations of it would collide.
    used_result: Rc<Cell<bool>>,
    /// Set once `infer_output<S>` (D28) is rendered anywhere, so the module gets
    /// the one injected mapped-type alias (`__GlyphInferOutput`) it lowers to.
    /// Shared across sub-emitters via the `Rc<Cell>` like the flags above.
    used_infer_output: Rc<Cell<bool>>,
    /// The `EMITTER_GLOBALS` this module shadows with a top-level declaration.
    /// Empty for almost every module, which is why the capture is conditional:
    /// a module that shadows nothing emits exactly what it emitted before.
    shadowed_globals: Rc<BTreeSet<String>>,
    /// Type-parameter guard bindings in scope while a *generic* record
    /// descriptor's field checks are generated: `(param name, guard var)` pairs
    /// such as `("T", "__is_T")`. A field typed `T` is validated by calling its
    /// threaded checker instead of a presence check. Empty for non-generic
    /// descriptors (the common case), so their emission is unchanged.
    desc_param_guards: RefCell<Vec<(String, String)>>,
    /// The binding a value-position `match` lowered as a statement `switch`
    /// assigns to (`ArmTerm::Assign`). Set only while such a `switch` is emitted
    /// (save/restore around it), so its value arms assign the binding and its
    /// `return` arms still return from the function.
    assign_target: RefCell<Option<String>>,
    /// The declared return type (rendered) the current function's `return`
    /// values must be cast to, set only when that type mentions `infer_output<S>`
    /// (D28) — a combinator asserting a dynamically-built value matches the
    /// shape-derived type. `None` otherwise (every honest generic return is
    /// cast-free), and reset for lambdas and value-position match IIFEs (their
    /// returns are not the enclosing function's return).
    return_cast: Option<String>,
    /// The export prefix for the declaration currently being emitted: `"export "`
    /// when the decl is `pub` (or is `fn main`, always exported), `""` otherwise
    /// (0.1.16 module-private-by-default). Set per top-level decl and read by
    /// every sub-emit (the type alias, its runtime descriptor, union variant
    /// constructors) so a private type's descriptor and constructors are private
    /// too.
    emit_export: &'static str,
    /// Stack of enclosing loops, innermost last. Each entry is the loop's TS
    /// label when it needs one (a `break`/`continue` buried in a `match` arm,
    /// which lowers to a `switch` that would otherwise capture the jump), or
    /// `None` when a plain unlabeled jump suffices. `break`/`continue`
    /// statements read the top entry. A sub-emitter starts empty: a `break`
    /// inside a lambda or value-position match cannot target an outer loop.
    loop_labels: Vec<Option<String>>,
    module: &'a Module,
    resolved: &'a ResolvedModule,
    /// The prelude symbol table, used to resolve a `ResolvedRef::Prelude(id)`
    /// back to its name so the emitter can inject `import`s for the prelude
    /// tagged-union values/types a module references without an explicit import.
    prelude: &'a Prelude,
    types: &'a TypeMap,
    /// Types of synthesized scrutinee temporaries the `TypeMap` doesn't know
    /// about — keyed by temp name (`__p1`). `degroup_nested_arms` records the
    /// payload type of each grouping temp here so the inner `match` that
    /// dispatches it knows whether a variant's payload is a record (bind the
    /// whole object) or a single value (bind `.value`). Shared with
    /// sub-emitters so a value-position inner match still sees it.
    synth_types: Rc<RefCell<HashMap<String, Ty>>>,
    /// A coarse source map for remapping `tsc` errors back to Glyph source:
    /// `(byte offset into `out`, originating Glyph span)` checkpoints, recorded
    /// as each declaration and top-level statement begins. Offsets are strictly
    /// increasing (emission is append-only), so a `.ts` position maps to the
    /// last checkpoint at or before it. Only the top-level emitter records here;
    /// a sub-emitter (lambda/IIFE body) renders into a spliced string whose
    /// internal offsets don't correspond to the final file, so its statements
    /// map to the enclosing top-level statement — coarser but still correct.
    source_map: Vec<(usize, Span)>,
    /// Project context for resolving cross-module import specifiers.
    ctx: EmitContext<'a>,
}

/// What a two-binding `for` is iterating, as far as the type checker settled it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IterShape {
    /// The prelude `Array<T>`: pairs are `it.entries()`, index is a number.
    Array,
    /// A record or map-like object: pairs are `Object.entries(it)`, key is a string.
    Record,
    /// Not settled statically. Decided at run time rather than guessed, because
    /// guessing wrong changes what the program computes.
    Unknown,
}

/// The two bounds of a `for` that walks `array.range(..)` directly.
struct CountingRange {
    start: String,
    end: String,
}

impl<'a> Emitter<'a> {
    /// How this module must spell a JavaScript global the emitter writes.
    ///
    /// `Error` normally, `__glyph_Error` in a module that declares its own
    /// `Error`. Every emitter-internal reference goes through here, so adding a
    /// new global reference without adding it to `EMITTER_GLOBALS` is caught by
    /// the drift test rather than by a user whose type is named `Number`.
    fn g(&self, name: &str) -> String {
        if self.shadowed_globals.contains(name) {
            global_alias(name)
        } else {
            name.to_string()
        }
    }

    /// A fresh sub-emitter at the given indent, inheriting the temporary
    /// counter so synthesized names don't repeat. Used to render a lambda body
    /// or a value-position `match` into its own string before splicing it in.
    fn sub(&self, indent: usize) -> Emitter<'a> {
        Emitter {
            out: String::new(),
            indent,
            tmp_counter: self.tmp_counter,
            used_try: Rc::clone(&self.used_try),
            used_schema: Rc::clone(&self.used_schema),
            used_result: Rc::clone(&self.used_result),
            used_infer_output: Rc::clone(&self.used_infer_output),
            shadowed_globals: Rc::clone(&self.shadowed_globals),
            // Descriptor field checks are generated on the main emitter, never a
            // sub-emitter (lambda/IIFE) body, so a sub starts with no guards.
            desc_param_guards: RefCell::new(Vec::new()),
            assign_target: RefCell::new(None),
            // A sub-emitter (lambda body, value-position match IIFE) does not
            // inherit the function's return cast; its returns are not the
            // function's. `emit_fn_block` sets it from the lambda's own type.
            return_cast: None,
            // Inherit the enclosing decl's export prefix (a lambda body rendered
            // by a sub-emitter belongs to the same decl).
            emit_export: self.emit_export,
            // A sub-emitter is a fresh function scope: an inner `break`/
            // `continue` cannot reach a loop in the enclosing emitter.
            loop_labels: Vec::new(),
            module: self.module,
            resolved: self.resolved,
            prelude: self.prelude,
            types: self.types,
            synth_types: Rc::clone(&self.synth_types),
            // A sub-emitter's output is spliced into the parent as a string, so
            // its byte offsets don't map to the final file; give it a throwaway
            // map. Its statements map to the enclosing top-level statement.
            source_map: Vec::new(),
            ctx: self.ctx,
        }
    }

    fn pad(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    /// Write an indented line plus a trailing newline.
    fn line(&mut self, s: &str) {
        self.pad();
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// Emit `return <value>;`, appending the function's generic return cast
    /// (`as RetType`) when one is in effect (see `return_cast`).
    fn emit_return(&mut self, value: &str) {
        match self.return_cast.clone() {
            Some(c) => self.line(&format!("return {value} as {c};")),
            None => self.line(&format!("return {value};")),
        }
    }

    // ----- declarations -----

    fn emit_module(&mut self) -> Result<(), EmitError> {
        // Copy the `&Module` reference (references are `Copy`) so iterating it
        // doesn't borrow `self` across the `&mut self` emit calls.
        let module = self.module;
        // A `component` lowers to React `createElement` calls, which need the
        // React namespace in scope. The Glyph source imports named hooks from
        // `react` but not React itself, so add the namespace import here.
        if module
            .items
            .iter()
            .any(|d| matches!(d, Decl::Component(_)))
        {
            self.line("import * as React from \"react\";");
            self.out.push('\n');
        }
        for (i, decl) in module.items.iter().enumerate() {
            if i > 0 {
                self.out.push('\n');
            }
            self.emit_decl(decl)?;
        }
        // Source-map offsets were recorded during body emission. The imports
        // below are prepended with `insert_str(0, ...)`, shifting the body; note
        // the body length now so the offsets can be corrected afterward.
        let body_len = self.out.len();
        // A module that referenced a prelude tagged-union value/type
        // (`Ok`/`Err`/`Result`, `Some`/`None`/`Option`) without an explicit
        // import still needs the runtime `import` in the emitted TS — the
        // prelude makes the name resolve, but `tsc`/`tsx` need the binding.
        // Inserted before the schema/try imports below so those end up first.
        let prelude_header = self.prelude_import_header();
        if !prelude_header.is_empty() {
            self.out.insert_str(0, &prelude_header);
        }
        // A module that emitted any record descriptor needs the `schema`
        // factory for its `T.schema` member; and a module that lowered any `?`
        // re-wraps the propagated error with the prelude `Err`. Prepend the
        // (aliased) imports now that emission has set the flags. `?`'s import is
        // inserted last so it ends up first.
        // The one mapped-type alias `infer_output<S>` (D28) lowers to. It matches
        // each field structurally by its `parse` signature and reads the `Ok`
        // payload out of the parse result's wire form, so it depends on no
        // in-scope type name. `tsc` reduces `__GlyphInferOutput<Shape>` at each
        // call site; unused when no `infer_output` was rendered.
        // A module that shadows a global the emitter depends on captures the
        // real one first, so the author keeps the name and the compiler keeps
        // the global. `globalThis` is what makes this work: by the time these
        // run, the module's own `Error` is hoisted and a bare `Error` would
        // already mean the user's.
        if !self.shadowed_globals.is_empty() {
            let mut captures = String::new();
            for name in self.shadowed_globals.iter() {
                let alias = global_alias(name);
                // `Array` and `Promise` are written in type position too, and a
                // `const` is not a type. Each gets a type alias as well.
                if name == "Array" || name == "Promise" {
                    captures.push_str(&format!(
                        "type {alias}<T> = globalThis.{name}<T>;\n"
                    ));
                }
                captures.push_str(&format!("const {alias} = globalThis.{name};\n"));
            }
            captures.push('\n');
            self.out.insert_str(0, &captures);
        }
        if self.used_infer_output.get() {
            self.out.insert_str(
                0,
                &format!(
                    "type {INFER_OUTPUT_ALIAS}<S> = {{ [K in keyof S]: S[K] extends {{ parse(input: unknown): infer R }} ? (Extract<R, {{ tag: \"Ok\" }}> extends {{ value: infer V }} ? V : never) : never }};\n\n"
                ),
            );
        }
        if self.used_schema.get() {
            let spec = runtime_specifier(self.ctx.module_path, "std/schema");
            self.out.insert_str(
                0,
                &format!("import {{ schema as {SCHEMA_FACTORY} }} from \"{spec}\";\n\n"),
            );
        }
        // One `std/result` import covers both consumers: `?` needs `Err` to
        // re-wrap a propagated failure, and a descriptor's `parse` needs
        // `Ok`/`Err` plus the `Result` type to return. They share `__glyph_err`,
        // so emitting two lines would redeclare it; build the alias list from
        // both flags and emit a single line. The aliases are what let this
        // coexist with `prelude_import_header`'s unaliased
        // `import { Ok, Err, Result } from "std/result"` — two imports of the
        // same specifier with distinct locals are legal TypeScript.
        if self.used_try.get() || self.used_result.get() {
            let mut names: Vec<String> = Vec::new();
            if self.used_result.get() {
                names.push(format!("{RESULT_OK} as {OK_CTOR}"));
            }
            names.push(format!("{RESULT_ERR} as {ERR_CTOR}"));
            if self.used_result.get() {
                names.push(format!("type Result as {RESULT_TY}"));
            }
            let spec = runtime_specifier(self.ctx.module_path, "std/result");
            self.out.insert_str(
                0,
                &format!("import {{ {} }} from \"{spec}\";\n\n", names.join(", ")),
            );
        }
        // Every emitted module pulls in the runtime bootstrap for its side
        // effects, so the ambient `number`/`par`/`print` globals exist at run
        // time no matter which module an external bundler (Vite, esbuild) treats
        // as the entry. `glyph run` also installs them via its generated
        // entrypoint; the ESM loader dedups by resolved URL and the installs are
        // idempotent, so the redundancy is harmless. Inserted last so it lands
        // first, ahead of every other import.
        let bootstrap = bootstrap_specifier(self.ctx.module_path);
        self.out.insert_str(0, &format!("import \"{bootstrap}\";\n\n"));
        // Shift every recorded checkpoint past the prepended import header so
        // offsets are correct against the final output.
        let prepended = self.out.len() - body_len;
        for (offset, _) in self.source_map.iter_mut() {
            *offset += prepended;
        }
        Ok(())
    }

    /// Build the `import` lines for prelude tagged-union names this module
    /// references without an explicit import. Explicitly imported names resolve
    /// to a module symbol (not `ResolvedRef::Prelude`), so they are naturally
    /// excluded and never double-imported. Names are grouped per runtime module
    /// and sorted for deterministic output; empty when nothing is needed.
    fn prelude_import_header(&self) -> String {
        let mut result: BTreeSet<&'static str> = BTreeSet::new();
        let mut option: BTreeSet<&'static str> = BTreeSet::new();
        for (_, r) in self.resolved.resolutions.iter() {
            let ResolvedRef::Prelude(id) = r else { continue };
            let Some(sym) = self.prelude.table.get(id) else {
                continue;
            };
            match sym.name.as_ref() {
                "Result" => {
                    result.insert("Result");
                }
                "Ok" => {
                    result.insert("Ok");
                }
                "Err" => {
                    result.insert("Err");
                }
                "Option" => {
                    option.insert("Option");
                }
                "Some" => {
                    option.insert("Some");
                }
                "None" => {
                    option.insert("None");
                }
                _ => {}
            }
        }
        let mut out = String::new();
        for (names, module) in [(&result, "std/result"), (&option, "std/option")] {
            if !names.is_empty() {
                // `Result` and `Option` are `export type` in the runtime, so
                // they carry the inline `type` modifier here for the same
                // reason a written-out named import does: without it the name
                // survives type stripping and fails to link (G114). The
                // constructors are real values and stay bare.
                let list = names
                    .iter()
                    .map(|n| {
                        if glyph_resolver::is_stdlib_type_only(module, n) {
                            format!("type {n}")
                        } else {
                            (*n).to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let spec = runtime_specifier(self.ctx.module_path, module);
                out.push_str(&format!("import {{ {list} }} from \"{spec}\";\n"));
            }
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }

    fn emit_decl(&mut self, decl: &Decl) -> Result<(), EmitError> {
        self.source_map.push((self.out.len(), decl.span()));
        // 0.1.16: module-private by default. A decl is `export`ed only when `pub`
        // (or it is `fn main`, the entrypoint the runner imports). The prefix is
        // read by every sub-emit for this decl (descriptor, constructors).
        self.emit_export = if decl.is_public() { "export " } else { "" };
        let ex = self.emit_export;
        match decl {
            Decl::Import(im) => self.emit_import(im),
            Decl::Fn(f) => {
                let generics = self.generics(&f.generics)?;
                let params = self.params(&f.params)?;
                // Glyph's `async fn -> T` awaits to `T`; TS annotates the
                // wrapper, so the emitted return type is `Promise<T>`.
                let ret = match &f.return_ty {
                    Some(te) if f.is_async => format!(": {}<{}>", self.g("Promise"), self.ty(te)?),
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                let prefix = if f.is_async {
                    format!("{ex}async function")
                } else {
                    format!("{ex}function")
                };
                self.pad();
                self.out
                    .push_str(&format!("{prefix} {}{generics}({params}){ret} ", f.name));
                let cast = self.fn_return_cast(&f.return_ty)?;
                self.emit_fn_block(&f.body, returns_value(&f.return_ty), cast)?;
                self.out.push('\n');
                Ok(())
            }
            Decl::Const(c) => {
                let ty = match &c.ty {
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                let value = self.expr(&c.value)?;
                self.line(&format!("{ex}const {}{ty} = {value};", c.name));
                Ok(())
            }
            Decl::Interface(i) => self.emit_interface(i),
            Decl::Type(t) => {
                if let TypeExpr::Union { variants, .. } = &t.body {
                    if t.refinement.is_some() {
                        return Err(EmitError::Unsupported {
                            construct: "a `where` refinement on a union type (v1 supports refinements on primitive base types)",
                            span: t.span,
                        });
                    }
                    return self.emit_union(&t.name, &t.generics, variants);
                }
                let generics = self.generics(&t.generics)?;
                let body = self.ty(&t.body)?;
                self.line(&format!("{ex}type {}{generics} = {body};", t.name));
                // Q8: a record type also emits a runtime descriptor whose `is`
                // predicate makes `is TypeName` checks work at runtime (no type
                // erasure). A generic record emits a checker-threaded descriptor
                // (its `is`/`parse` take one checker per type parameter).
                if let TypeExpr::Record { fields, .. } = &t.body {
                    if t.refinement.is_some() {
                        return Err(EmitError::Unsupported {
                            construct: "a `where` refinement on a record type (v1 supports refinements on primitive base types)",
                            span: t.span,
                        });
                    }
                    let redact = glyph_ast::redact_fields(&t.annotations).unwrap_or_default();
                    let open = glyph_ast::is_open_record(&t.annotations);
                    self.emit_record_descriptor(&t.name, &t.generics, fields, &redact, open)?;
                } else if let Some(pred) = &t.refinement {
                    // D39: a refined primitive type gets a runtime descriptor whose
                    // `is`/`parse` run the base leaf-check AND the predicate, so a
                    // value that fails the predicate is rejected at the boundary.
                    self.emit_refinement_descriptor(&t.name, &t.body, pred)?;
                }
                Ok(())
            }
            Decl::Component(c) => self.emit_component(c),
        }
    }

    /// Emit a structural interface (0.1.16) as a TypeScript `interface`. A method
    /// member emits as a method signature (`name(p: T): R`), a field member as a
    /// property signature (`name: T` / `name?: T`). Purely type-level: no runtime
    /// descriptor (like a `.d.ts` type, it is checked by `tsc`, not validated).
    fn emit_interface(&mut self, i: &glyph_ast::InterfaceDecl) -> Result<(), EmitError> {
        let ex = self.emit_export;
        let generics = self.generics(&i.generics)?;
        if i.members.is_empty() {
            self.line(&format!("{ex}interface {}{generics} {{}}", i.name));
            return Ok(());
        }
        self.line(&format!("{ex}interface {}{generics} {{", i.name));
        self.indent += 1;
        for m in &i.members {
            match m {
                glyph_ast::InterfaceMember::Method {
                    name,
                    params,
                    return_ty,
                    ..
                } => {
                    let ps = self.params(params)?;
                    let ret = match return_ty {
                        Some(te) => format!(": {}", self.ty(te)?),
                        None => ": void".to_string(),
                    };
                    self.line(&format!("{name}({ps}){ret};"));
                }
                glyph_ast::InterfaceMember::Field(f) => {
                    let opt = if f.optional { "?" } else { "" };
                    let ty = self.ty(&f.ty)?;
                    self.line(&format!("{}{opt}: {ty};", f.name));
                }
            }
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// Emit the Q8 runtime descriptor for a record type: an `is` type guard
    /// (each field checked by `field_value_check`, which recurses through
    /// primitives, named-type descriptors, `Array<E>`, `Option<E>`, `Record<K,V>`,
    /// and inline records), plus a `parse` entry point that validates an
    /// `unknown` and returns a `Result` (`Ok` of the value, or an `Err`
    /// describing the failure).
    ///
    /// `parse` returns the real prelude `Result`: it builds its value with the
    /// aliased `Ok`/`Err` constructors and annotates the return
    /// `__GlyphResult<T, Issue[]>`. An earlier version inlined the `tag`/`value`
    /// wire format instead, to avoid a `std/result` dependency, but a bare
    /// `{tag,value}` union is not assignable to `Result<T, E>` (which intersects
    /// the `map`/`map_err` combinators), so `return User.parse(v)` from a
    /// `Result`-returning function was a `tsc` error even though Glyph's own
    /// typechecker reports `parse` as a `Result`. The dependency is paid for the
    /// same way `?` and `T.schema` already pay for theirs: `emit_module` injects
    /// one aliased import when the flag is set. Cost: a `parse` now allocates
    /// the two combinator closures the constructors build, the same cost every
    /// other `Ok(...)` in Glyph already pays. `parse` reaches the sibling `is`
    /// guard through `this` rather than by the descriptor's name, so it stays
    /// correct even for a record whose name shadows the `parse` parameter (a
    /// type literally named `value`).
    ///
    /// A **generic** record type (`Paginated<T>`) emits the same descriptor with
    /// `is`/`parse` as generic methods that take one runtime checker per type
    /// parameter (`__is_T: (v) => boolean`). A field typed `T` is validated by
    /// calling that checker; the call site supplies it (`Paginated.parse<User>(v)`
    /// passes a checker built from `User`). A generic descriptor omits the
    /// `schema` member (a `Schema<Paginated<T>>` factory would need the checker
    /// threaded too — later work) and `redact`.
    ///
    /// **Soundness limitation**: the remaining shallow cases are the
    /// `field_value_check` fallbacks — a bare *unconstrained* type argument or an
    /// imported (`.d.ts`) type are only checked for presence (`!== undefined`),
    /// so their `value is X` narrowing is stronger than the runtime proof. An
    /// imported type gets a real descriptor once materialized with `glyph gen dts`.
    /// D39: emit the runtime descriptor for a refined primitive type
    /// (`type Amount = int where value >= 0`). The `is` guard runs the base
    /// leaf-check and the predicate; the base check narrows `value` first, so
    /// the predicate (which refers to the bound `value`) sees the base type. A
    /// value that fails the predicate is rejected by `.parse` at the boundary.
    ///
    /// The rejection names the constraint: `expected Password (string where
    /// value.length >= 8)`, with the base type and the predicate spelled the way
    /// the declaration spells them. That is the greppability half of D39 — the
    /// string that reaches an HTTP 422 body greps straight back to the
    /// `type Password = ...` line. The issue carries `code: "refinement"`.
    fn emit_refinement_descriptor(
        &mut self,
        name: &Ident,
        base: &TypeExpr,
        predicate: &Expr,
    ) -> Result<(), EmitError> {
        let base_check = self.field_value_check(base, "value");
        let pred = self.expr(predicate)?;
        let constraint = format!("{} where {}", type_label(base), strip_outer_parens(&pred));
        self.line(&format!("{}const {name} = {{", self.emit_export));
        self.indent += 1;
        self.line(&format!("is(value: unknown): value is {name} {{"));
        self.indent += 1;
        self.line(&format!("return {base_check} && {pred};"));
        self.indent -= 1;
        self.line("},");
        self.used_result.set(true);
        self.line(&format!(
            "parse(value: unknown): {RESULT_TY}<{name}, Issue[]> {{"
        ));
        self.indent += 1;
        // A value of the wrong base type and a base-typed value that fails the
        // predicate are different failures, so they get different messages. The
        // base check runs first and reports the base type alone; only a value
        // that *is* a `string` can meaningfully be told it failed a `where`.
        self.line(&format!("if (!({base_check})) {{"));
        self.indent += 1;
        self.line(&format!(
            "return {ERR_CTOR}([{{ path: [], message: {}, code: \"type\" }}]);",
            escape_double_quoted(&format!("expected {name} ({})", type_label(base)))
        ));
        self.indent -= 1;
        self.line("}");
        self.line(&format!("return {pred}"));
        self.indent += 1;
        self.line(&format!("? {OK_CTOR}(value as {name})"));
        self.line(&format!(
            ": {ERR_CTOR}([{{ path: [], message: {}, code: \"refinement\" }}]);",
            escape_double_quoted(&format!("expected {name} ({constraint})"))
        ));
        self.indent -= 1;
        self.indent -= 1;
        self.line("},");
        self.used_schema.set(true);
        self.line(&format!(
            "schema: {SCHEMA_FACTORY}<{name}>(\"{name}\", (v): v is {name} => {name}.is(v), (v: unknown): {RESULT_TY}<{name}, Issue[]> => {name}.parse(v)),"
        ));
        self.indent -= 1;
        self.line("};");
        Ok(())
    }

    fn emit_record_descriptor(
        &mut self,
        name: &Ident,
        generics: &[GenericParam],
        fields: &[RecordTypeField],
        redact: &[String],
        open: bool,
    ) -> Result<(), EmitError> {
        let is_generic = !generics.is_empty();
        // `<T extends Bound>` for the method signatures; `<T>` for the predicate
        // type (`value is Name<T>`) and the internal `this.is<T>(...)` call.
        let decl_generics = self.generics(generics)?;
        let type_args = if is_generic {
            format!(
                "<{}>",
                generics
                    .iter()
                    .map(|g| g.name.as_ref())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            String::new()
        };
        let self_ty = format!("{name}{type_args}");
        // One `__is_T: (v) => boolean` parameter per type parameter, after `value`.
        let checker_params: Vec<String> = generics
            .iter()
            .map(|g| format!("{}: (v: unknown) => boolean", Self::guard_param_name(g.name.as_ref())))
            .collect();
        let entry_params = std::iter::once("value: unknown".to_string())
            .chain(checker_params.iter().cloned())
            .collect::<Vec<_>>()
            .join(", ");
        // Bind the guards so `field_value_check` validates a `T`-typed field with
        // its threaded checker while the field checks are generated, then clear.
        *self.desc_param_guards.borrow_mut() = generics
            .iter()
            .map(|g| (g.name.to_string(), Self::guard_param_name(g.name.as_ref())))
            .collect();
        let field_checks: Vec<String> = fields.iter().map(|f| self.record_field_check(f)).collect();
        self.desc_param_guards.borrow_mut().clear();
        // Strict-by-default: reject a value carrying keys the type doesn't
        // declare (mass-assignment / leaked-field protection). `@open` opts out.
        // `allowed_keys` is the disjunction that a known key satisfies (empty for
        // an empty record, where no key is allowed); `None` when `@open`.
        let allowed_keys: Option<String> = if open {
            None
        } else if fields.is_empty() {
            Some(String::new())
        } else {
            Some(
                fields
                    .iter()
                    .map(|f| format!("__k === \"{}\"", f.name))
                    .collect::<Vec<_>>()
                    .join(" || "),
            )
        };
        // `is` combines every field check and the strict-keys check with `&&`.
        let mut checks: Vec<String> = field_checks.clone();
        if let Some(allowed) = &allowed_keys {
            if allowed.is_empty() {
                checks.push(format!("{}.keys(value as object).length === 0", self.g("Object")));
            } else {
                checks.push(format!(
                    "{}.keys(value as object).every((__k: string) => {allowed})",
                    self.g("Object")
                ));
            }
        }
        self.line(&format!("{}const {name} = {{", self.emit_export));
        self.indent += 1;
        self.line(&format!("is{decl_generics}({entry_params}): value is {self_ty} {{"));
        self.indent += 1;
        // An array is an object to `typeof`, so it has to be excluded explicitly:
        // without this an `Array` satisfies every check of a record with no
        // required fields, and `Empty.is([])` (and `Empty.parse([])`) answered
        // true for a value that is not a record at all.
        if checks.is_empty() {
            self.line(
                &format!(
                    "return typeof value === \"object\" && value !== null && !{}.isArray(value);",
                    self.g("Array")
                ),
            );
        } else {
            self.line(
                &format!(
                    "return typeof value === \"object\" && value !== null && !{}.isArray(value)",
                    self.g("Array")
                ),
            );
            self.indent += 1;
            for (i, c) in checks.iter().enumerate() {
                let term = if i + 1 == checks.len() { ";" } else { "" };
                self.line(&format!("&& {c}{term}"));
            }
            self.indent -= 1;
        }
        self.indent -= 1;
        self.line("},");
        // `parse` validates field by field and returns `Result<T, Issue[]>`: each
        // failing field contributes an `Issue` naming it (`path: [field]`), so a
        // boundary rejection reports which field is wrong rather than a single
        // "expected T" string. A non-object value fails outright, and an array is
        // called out by name (it reaches here as a `typeof "object"`). When no
        // issue is found the value is exactly a `T`, so the `Ok` payload is a
        // checked cast.
        //
        // The per-field test is ordered so the answer says *which rule* failed
        // rather than collapsing three of them into one string: an absent
        // required field reports `code: "missing"`, a present-but-wrong one
        // reports `code: "type"` and names the declared type, and a field whose
        // type has its own descriptor delegates to `T.parse` so the nested
        // issues arrive with the field name prepended to their `path` (and a
        // refinement's constraint message survives the trip). Leaf types,
        // unconstrained type parameters, and imported `.d.ts` types have no
        // descriptor to delegate to and keep the flat `field_value_check`.
        self.used_result.set(true);
        self.line(&format!(
            "parse{decl_generics}({entry_params}): {RESULT_TY}<{self_ty}, Issue[]> {{"
        ));
        self.indent += 1;
        self.line(&format!("if ({}.isArray(value)) {{", self.g("Array")));
        self.indent += 1;
        self.line(&format!(
            "return {ERR_CTOR}([{{ path: [], message: \"expected {name} (an object), got an array\", code: \"type\" }}]);"
        ));
        self.indent -= 1;
        self.line("}");
        self.line("if (typeof value !== \"object\" || value === null) {");
        self.indent += 1;
        self.line(&format!(
            "return {ERR_CTOR}([{{ path: [], message: \"expected {name} (an object)\", code: \"type\" }}]);"
        ));
        self.indent -= 1;
        self.line("}");
        self.line("const __issues: Issue[] = [];");
        // The type-parameter guards are needed again here: a field typed `T`
        // still validates through its threaded checker inside `parse`.
        *self.desc_param_guards.borrow_mut() = generics
            .iter()
            .map(|g| (g.name.to_string(), Self::guard_param_name(g.name.as_ref())))
            .collect();
        for (i, f) in fields.iter().enumerate() {
            self.emit_field_parse_check(i, f);
        }
        self.desc_param_guards.borrow_mut().clear();
        if let Some(allowed) = &allowed_keys {
            let cond = if allowed.is_empty() { "false" } else { allowed.as_str() };
            self.line(&format!("for (const __k of {}.keys(value as object)) {{", self.g("Object")));
            self.indent += 1;
            self.line(&format!(
                "if (!({cond})) __issues.push({{ path: [__k], message: \"unexpected field `\" + __k + \"`\", code: \"unexpected\" }});"
            ));
            self.indent -= 1;
            self.line("}");
        }
        self.line(&format!(
            "return __issues.length === 0 ? {OK_CTOR}(value as {self_ty}) : {ERR_CTOR}(__issues);"
        ));
        self.indent -= 1;
        self.line("},");
        // Q8/Q40 `T.schema`: a `Schema<T>` built from the `is` guard by the
        // prelude factory (the factory carries the recursive `array()`). The
        // guard references the descriptor by name in a lazy closure — `this` is
        // not the descriptor object inside this object literal, but the closure
        // only runs once the `const` is initialized. A generic descriptor omits
        // `schema` (the factory would need the per-parameter checker too).
        if !is_generic {
            self.used_schema.set(true);
            self.line(&format!(
                "schema: {SCHEMA_FACTORY}<{name}>(\"{name}\", (v): v is {name} => {name}.is(v), (v: unknown): {RESULT_TY}<{name}, Issue[]> => {name}.parse(v)),"
            ));
        }
        // D24 `@redact`: a `redact(value)` that returns a serialization-safe copy
        // with the named PII fields replaced by a sentinel. Emitted only when the
        // type carries `@redact fields: [...]`. The return type is a plain object
        // (not `name`), since the masked fields no longer hold their declared
        // types — the copy is for logging/`json.stringify`, not continued typed
        // use. Additive: it never touches `is`/`parse`/`schema`.
        if !redact.is_empty() && !is_generic {
            let masks: String = redact
                .iter()
                .map(|f| format!("\"{f}\": \"{REDACTION_SENTINEL}\""))
                .collect::<Vec<_>>()
                .join(", ");
            self.line(&format!(
                "redact(value: {name}): Record<string, unknown> {{ return {{ ...value, {masks} }}; }},"
            ));
        }
        self.indent -= 1;
        self.line("};");
        Ok(())
    }

    /// One field's contribution to a record descriptor's `parse`, as statements
    /// pushing zero or one `Issue` (or several, when the field delegates).
    ///
    /// A required field is tested in order: absent first (`code: "missing"`),
    /// then wrong (`code: "type"`, naming the declared type). An optional field
    /// (`f?: T`) skips the absence test entirely, so absence stays legal exactly
    /// where the type says it is. When the field's type has its own emitted
    /// descriptor the check becomes `T.parse(...)` and its issues are spliced in
    /// with this field's name prepended to each `path`, which is what makes a
    /// nested rejection reportable as `["body", "password"]` and what carries a
    /// refinement's constraint message out to the caller.
    fn emit_field_parse_check(&mut self, index: usize, field: &RecordTypeField) {
        let fname = field.name.to_string();
        let access = format!("(value as Record<string, unknown>).{fname}");
        // An optional field is absent in either of JSON's two spellings for it:
        // the key is omitted, or the key is there holding `null`. Every real
        // payload uses the second (a Discord gateway frame carries `"s": null`),
        // and `glyph gen openapi` already maps a `nullable` schema field to an
        // optional one and documents that a literal null is treated as absent —
        // the generator said so and the descriptor did not implement it, so a
        // generated type rejected the payload it was generated from.
        //
        // Treating null as absent is the only coherent reading: the field's type
        // is `T`, and `null` is not a value of `T`.
        let present = format!("{access} !== undefined && {access} !== null");
        let missing_issue = format!(
            "__issues.push({{ path: [\"{fname}\"], message: {}, code: \"missing\" }});",
            escape_double_quoted(&format!("field `{fname}` is required"))
        );
        match self.field_descriptor_name(&field.ty) {
            Some(desc) => {
                let res = format!("__r{index}");
                let issue = format!("__i{index}");
                let splice = |this: &mut Self| {
                    this.line(&format!("const {res} = {desc}.parse({access});"));
                    this.line(&format!("if ({res}.tag === \"Err\") {{"));
                    this.indent += 1;
                    this.line(&format!("for (const {issue} of {res}.value) {{"));
                    this.indent += 1;
                    this.line(&format!(
                        "__issues.push({{ path: [\"{fname}\", ...{issue}.path], message: {issue}.message, code: {issue}.code }});"
                    ));
                    this.indent -= 1;
                    this.line("}");
                    this.indent -= 1;
                    this.line("}");
                };
                if field.optional {
                    self.line(&format!("if ({present}) {{"));
                    self.indent += 1;
                    splice(self);
                    self.indent -= 1;
                    self.line("}");
                } else {
                    self.line(&format!("if ({access} === undefined) {{"));
                    self.indent += 1;
                    self.line(&missing_issue);
                    self.indent -= 1;
                    self.line("} else {");
                    self.indent += 1;
                    splice(self);
                    self.indent -= 1;
                    self.line("}");
                }
            }
            None => {
                // `unknown` (and anything the emitter cannot see into) has no
                // predicate worth emitting: the old code wrote
                // `else if (!(x !== undefined))`, a branch that can never fire,
                // under a message naming a type it never checked. The
                // missing-key check is the whole check; E0304 refuses the cases
                // where that is not honest enough to call validation.
                let presence_only = matches!(
                    self.field_check(&field.ty, &access),
                    FieldCheck::PresenceOnly | FieldCheck::Unverifiable
                );
                let check = self.field_value_check(&field.ty, &access);
                let type_issue = format!(
                    "__issues.push({{ path: [\"{fname}\"], message: {}, code: \"type\" }});",
                    escape_double_quoted(&format!(
                        "field `{fname}` must be {}",
                        type_label(&field.ty)
                    ))
                );
                if field.optional {
                    // An optional field with nothing to check needs no branch
                    // at all: absent is fine and present is unconstrained.
                    if !presence_only {
                        self.line(&format!("if ({present} && !({check})) {{"));
                        self.indent += 1;
                        self.line(&type_issue);
                        self.indent -= 1;
                        self.line("}");
                    }
                } else if presence_only {
                    self.line(&format!("if ({access} === undefined) {{"));
                    self.indent += 1;
                    self.line(&missing_issue);
                    self.indent -= 1;
                    self.line("}");
                } else {
                    self.line(&format!("if ({access} === undefined) {{"));
                    self.indent += 1;
                    self.line(&missing_issue);
                    self.indent -= 1;
                    self.line(&format!("}} else if (!({check})) {{"));
                    self.indent += 1;
                    self.line(&type_issue);
                    self.indent -= 1;
                    self.line("}");
                }
            }
        }
    }

    /// The descriptor a field of type `ty` can delegate its `parse` to, or `None`
    /// when there is none to delegate to. Mirrors the descriptor branches of
    /// [`Self::field_value_check`] exactly, including their order: a leaf type
    /// and a type parameter bound to a threaded checker never delegate, a local
    /// alias resolves through to whatever it names, and a two-segment path
    /// reaches another module's descriptor by the same binding the emitted type
    /// annotation uses.
    fn field_descriptor_name(&self, ty: &TypeExpr) -> Option<String> {
        if is_named_type(ty, "int") || js_typeof(ty).is_some() {
            return None;
        }
        match ty {
            TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                let name = segments[0].as_ref();
                if self.param_guard(name).is_some() {
                    return None;
                }
                if self.has_descriptor(name) {
                    return Some(name.to_string());
                }
                let leaf = self.resolve_alias_leaf(name)?;
                self.field_descriptor_name(&leaf)
            }
            TypeExpr::Path { segments, .. } if segments.len() == 2 => {
                let ns = segments[0].as_ref();
                let name = segments[1].as_ref();
                self.has_namespaced_descriptor(ns, name)
                    .then(|| format!("{ns}.{name}"))
            }
            _ => None,
        }
    }

/// An assignment target, rendered without the bounds check a read gets.
    ///
    /// `mut xs[i] = v` has to emit `xs[i] = v`; wrapping the target would
    /// produce `__glyph_index(xs, i) = v`, which is not assignable. Writing
    /// past the end of an array is also not the bug G30 is about: it grows the
    /// array rather than handing back a value that was never there.
    fn lvalue(&mut self, e: &Expr) -> Result<String, EmitError> {
        match e {
            Expr::Index { object, index, .. } => Ok(format!(
                "{}[{}]",
                self.expr(object)?,
                self.expr(index)?
            )),
            other => self.expr(other),
        }
    }

    fn emit_import(&mut self, im: &ImportDecl) -> Result<(), EmitError> {
        let path = im
            .path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        // A project (sibling) module must be imported by a relative specifier so
        // `tsc`/`tsx` resolve it against the emitted file tree. A `std/*` import
        // is relative too, pointing into the bundled `.glyph-runtime/std/` (see
        // `runtime_specifier`); only an external npm package stays bare, because
        // node_modules resolution is the one thing every host resolves the same
        // way. An `extern/*` import names a hand-written `.ts` file the build
        // stages into `<out>/extern/`; it emits a relative specifier like a
        // sibling so the same resolution finds it (this is the sanctioned way to
        // reach hand-written TypeScript without a relative import in Glyph
        // source).
        let module_path = path.clone();
        let spec = if self.ctx.project_modules.contains(&path) || path.starts_with("extern/") {
            relative_specifier(self.ctx.module_path, &path)
        } else if path.starts_with("std/") {
            runtime_specifier(self.ctx.module_path, &path)
        } else {
            path
        };
        let line = match &im.kind {
            ImportKind::Named(names) => {
                // A standard-library name the runtime declares with `export
                // type` has no runtime binding, so it must carry the inline
                // `type` modifier. `tsc` elides such a name from a value import
                // list, which is why the un-marked form type-checks; a type
                // *stripper* leaves it, and the import then fails to link
                // against a module that really has no such export (G114). The
                // inline modifier is the spelling a tool with no type
                // information can act on.
                //
                // A Glyph-declared type needs no marker: it emits a runtime
                // descriptor `const` under its own name, so the binding exists.
                let names = names
                    .iter()
                    .map(|n| {
                        if self.import_name_is_type_only(&module_path, n.as_ref()) {
                            format!("type {}", n.as_ref())
                        } else {
                            n.as_ref().to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("import {{ {names} }} from \"{spec}\";")
            }
            ImportKind::Namespace => {
                let alias = im.path.segments.last().map(|s| s.as_ref()).unwrap_or("ns");
                format!("import * as {alias} from \"{spec}\";")
            }
            ImportKind::Aliased(alias) => {
                format!("import * as {alias} from \"{spec}\";")
            }
            // The form `tsc` demands for a CommonJS `export =` callable. Every
            // other kind emits a named or namespace import, which is TS2595
            // ("can only be imported by using a default import") or leaves the
            // binding uncallable.
            ImportKind::Default(local) => {
                format!("import {local} from \"{spec}\";")
            }
        };
        self.line(&line);
        Ok(())
    }

    /// Emit a tagged union as a TS discriminated union plus a constructor per
    /// variant. The discriminant is a `tag` string literal. A record payload's
    /// fields are spread alongside the tag; a no-payload variant becomes a
    /// `const`, a payload variant a constructor function. A generic union's
    /// alias and constructor functions carry its type parameters; a no-payload
    /// variant `const` is typed at `<never>` so it is assignable to every
    /// instantiation.
    fn emit_union(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        variants: &[UnionVariant],
    ) -> Result<(), EmitError> {
        // A record-payload field named `tag` collides with the discriminant —
        // it would both duplicate the `tag` type member and let the spread
        // overwrite the tag at runtime. Reject it rather than emit broken TS.
        for v in variants {
            if let Some(TypeExpr::Record { fields, span }) = &v.payload {
                if fields.iter().any(|f| f.name.as_ref() == TAG) {
                    return Err(EmitError::Unsupported {
                        construct: "a union payload field named `tag` (reserved as the discriminant)",
                        span: *span,
                    });
                }
            }
        }
        let generics_str = self.generics(generics)?;
        self.line(&format!("{}type {name}{generics_str} =", self.emit_export));
        self.indent += 1;
        for (i, v) in variants.iter().enumerate() {
            let term = if i + 1 == variants.len() { ";" } else { "" };
            let members = self.variant_members(v)?;
            self.line(&format!("| {{ {members} }}{term}"));
        }
        self.indent -= 1;
        self.out.push('\n');
        for v in variants {
            self.emit_variant_constructor(name, generics, v)?;
        }
        // Q8: a non-generic tagged union also emits a runtime descriptor so
        // `is TypeName` and `TypeName.parse` work at runtime (no type erasure).
        // Skipped for a generic union (its type arguments live at the call
        // site) and when a variant shares the union's name (the descriptor
        // `const` would collide with that variant's constructor `const`).
        if generics.is_empty() && union_descriptor_name_free(name, variants) {
            self.emit_union_descriptor(name, variants);
        }
        Ok(())
    }

    /// Emit the Q8 runtime descriptor for a non-generic tagged union: an `is`
    /// type guard that checks `value` is an object whose `tag` names a variant
    /// AND whose payload matches that variant, a self-contained `parse` returning
    /// a `Result`, and a `T.schema` member. Mirrors `emit_record_descriptor`.
    ///
    /// The guard switches on the discriminant tag and validates the matched
    /// variant's payload: a record payload's fields are checked like a record's
    /// (recursively, via `record_field_check`), a single-value payload's `value`
    /// is checked against its type, and a no-payload variant passes on the tag
    /// alone.
    fn emit_union_descriptor(&mut self, name: &str, variants: &[UnionVariant]) {
        self.line(&format!("{}const {name} = {{", self.emit_export));
        self.indent += 1;
        self.line(&format!("is(value: unknown): value is {name} {{"));
        self.indent += 1;
        self.line("if (typeof value !== \"object\" || value === null) {");
        self.indent += 1;
        self.line("return false;");
        self.indent -= 1;
        self.line("}");
        self.line(&format!(
            "switch ((value as {{ {TAG}?: unknown }}).{TAG}) {{"
        ));
        self.indent += 1;
        for v in variants {
            let check = self.union_variant_check(v);
            self.line(&format!("case \"{}\": return {check};", v.name));
        }
        self.line("default: return false;");
        self.indent -= 1;
        self.line("}");
        self.indent -= 1;
        self.line("},");
        // parse(): reuse the guard, wrap the narrowed value with the prelude
        // constructors so the return is a real `Result`, exactly like the record
        // one.
        self.used_result.set(true);
        self.line(&format!(
            "parse(value: unknown): {RESULT_TY}<{name}, Issue[]> {{"
        ));
        self.indent += 1;
        self.line("return this.is(value)");
        self.indent += 1;
        self.line(&format!("? {OK_CTOR}(value)"));
        self.line(&format!(
            ": {ERR_CTOR}([{{ path: [], message: \"expected {name}\" }}]);"
        ));
        self.indent -= 1;
        self.indent -= 1;
        self.line("},");
        // Q8/Q40 `T.schema`: a `Schema<T>` built from the `is` guard.
        self.used_schema.set(true);
        self.line(&format!(
            "schema: {SCHEMA_FACTORY}<{name}>(\"{name}\", (v): v is {name} => {name}.is(v), (v: unknown): {RESULT_TY}<{name}, Issue[]> => {name}.parse(v)),"
        ));
        self.indent -= 1;
        self.line("};");
    }

    /// The object-type members of a variant: the `tag` literal, plus a record
    /// payload's fields spread inline, or a non-record payload under `value`.
    fn variant_members(&self, v: &UnionVariant) -> Result<String, EmitError> {
        let mut s = format!("{TAG}: \"{}\"", v.name);
        match &v.payload {
            None => {}
            Some(TypeExpr::Record { fields, .. }) => {
                for f in fields {
                    let opt = if f.optional { "?" } else { "" };
                    s.push_str(&format!("; {}{opt}: {}", f.name, self.ty(&f.ty)?));
                }
            }
            Some(other) => s.push_str(&format!("; {PAYLOAD}: {}", self.ty(other)?)),
        }
        Ok(s)
    }

    fn emit_variant_constructor(
        &mut self,
        union: &str,
        generics: &[GenericParam],
        v: &UnionVariant,
    ) -> Result<(), EmitError> {
        let name = &v.name;
        // A constructor is generic only over the union parameters its payload
        // actually mentions; the rest are widened to `never` in the return
        // type. This keeps `Left({ a })` in `Either<A, B>` inferring
        // `Either<A, never>` (assignable to any `Either<A, B>`) instead of
        // leaving the unused `B` as `unknown`.
        let used: Vec<bool> = generics
            .iter()
            .map(|g| {
                v.payload
                    .as_ref()
                    .is_some_and(|p| type_mentions(p, g.name.as_ref()))
            })
            .collect();
        let ret = apply_generics(union, generics, &used);
        let ctor_generics = {
            let names: Vec<&str> = generics
                .iter()
                .zip(&used)
                .filter(|(_, &u)| u)
                .map(|(g, _)| g.name.as_ref())
                .collect();
            if names.is_empty() {
                String::new()
            } else {
                format!("<{}>", names.join(", "))
            }
        };
        let ex = self.emit_export;
        match &v.payload {
            None => self.line(&format!("{ex}const {name}: {ret} = {{ {TAG}: \"{name}\" }};")),
            // Spread the fields FIRST so the discriminant always wins, even if
            // the record (somehow) carried a colliding key.
            Some(payload @ TypeExpr::Record { .. }) => self.line(&format!(
                "{ex}function {name}{ctor_generics}(fields: {}): {ret} {{ return {{ ...fields, {TAG}: \"{name}\" }}; }}",
                self.ty(payload)?
            )),
            Some(other) => self.line(&format!(
                "{ex}function {name}{ctor_generics}({PAYLOAD}: {}): {ret} {{ return {{ {TAG}: \"{name}\", {PAYLOAD} }}; }}",
                self.ty(other)?
            )),
        }
        Ok(())
    }

    fn generics(&self, generics: &[GenericParam]) -> Result<String, EmitError> {
        if generics.is_empty() {
            return Ok(String::new());
        }
        let mut parts = Vec::with_capacity(generics.len());
        for g in generics {
            // A bound lowers to a TS `extends` clause (`<T: Bound>` -> `<T extends
            // Bound>`). v1 carries a single bound.
            match g.bounds.first() {
                Some(bound) => parts.push(format!("{} extends {}", g.name, self.ty(bound)?)),
                None => parts.push(g.name.to_string()),
            }
        }
        Ok(format!("<{}>", parts.join(", ")))
    }

    fn params(&self, params: &[Param]) -> Result<String, EmitError> {
        let mut out = Vec::with_capacity(params.len());
        for p in params {
            out.push(format!("{}: {}", p.name, self.ty(&p.ty)?));
        }
        Ok(out.join(", "))
    }

    /// Emit lambda parameters. An un-annotated lambda parameter (which the
    /// parser records as type `unknown`) is emitted without a type so
    /// TypeScript infers it from the lambda's call-site context — the
    /// higher-order function's signature. Annotating it `unknown` would instead
    /// force every use of the parameter to fail. An explicitly typed parameter
    /// keeps its annotation.
    fn lambda_params(&self, params: &[Param]) -> Result<String, EmitError> {
        let mut out = Vec::with_capacity(params.len());
        for p in params {
            if is_unknown_type(&p.ty) {
                out.push(p.name.to_string());
            } else {
                out.push(format!("{}: {}", p.name, self.ty(&p.ty)?));
            }
        }
        Ok(out.join(", "))
    }

    // ----- statements -----

    fn emit_block(&mut self, block: &Block) -> Result<(), EmitError> {
        self.out.push_str("{\n");
        self.indent += 1;
        self.emit_void_stmts(&block.stmts)?;
        self.indent -= 1;
        self.pad();
        self.out.push('}');
        Ok(())
    }

    /// Emit a statement sequence for effect (void context), lowering `defer`.
    /// The first `defer` splits the sequence: statements before it emit plainly,
    /// then the rest are wrapped in `try { ... } finally { <deferred>; }` so the
    /// deferred expression runs on every exit path. Nested defers recurse, giving
    /// last-in-first-out cleanup order.
    fn emit_void_stmts(&mut self, stmts: &[Stmt]) -> Result<(), EmitError> {
        match stmts.iter().position(|s| matches!(s, Stmt::Defer(_))) {
            Some(i) => {
                for s in &stmts[..i] {
                    self.emit_stmt(s)?;
                }
                let Stmt::Defer(d) = &stmts[i] else { unreachable!() };
                self.line("try {");
                self.indent += 1;
                self.emit_void_stmts(&stmts[i + 1..])?;
                self.indent -= 1;
                self.line("} finally {");
                self.indent += 1;
                let v = self.emit_value(&d.expr)?;
                self.line(&format!("{v};"));
                self.indent -= 1;
                self.line("}");
                Ok(())
            }
            None => {
                for s in stmts {
                    self.emit_stmt(s)?;
                }
                Ok(())
            }
        }
    }

    /// Emit a function body, applying implicit tail returns when the function
    /// yields a value (a non-`void` return type). The body's final expression
    /// is the returned value (`return expr`); a `void` or unannotated function
    /// runs its tail for effect. A function body is never inside a `switch`, so
    /// no fall-through break is emitted.
    /// The rendered return type a function's `return` values are cast to, or
    /// `None`. A cast is emitted only when the function yields a value AND its
    /// declared return type mentions `infer_output<S>` (D28) — the one case a
    /// combinator dynamically assembles a value of a shape-derived type the
    /// body cannot otherwise prove. Every honest generic return (`T`,
    /// `Result<T, E>`, `Array<T>`, …) emits with NO cast and stays precisely
    /// checked by `tsc`; the pre-0.1.10 blanket "any generic return" cast is
    /// gone.
    fn fn_return_cast(
        &self,
        return_ty: &Option<TypeExpr>,
    ) -> Result<Option<String>, EmitError> {
        match return_ty {
            Some(te) if returns_value(return_ty) && type_mentions_infer_output(te) => {
                Ok(Some(self.ty(te)?))
            }
            _ => Ok(None),
        }
    }

    fn emit_fn_block(
        &mut self,
        block: &Block,
        returns_value: bool,
        return_cast: Option<String>,
    ) -> Result<(), EmitError> {
        let saved = std::mem::replace(&mut self.return_cast, return_cast);
        self.out.push_str("{\n");
        self.indent += 1;
        let term = if returns_value {
            ArmTerm::Return
        } else {
            ArmTerm::Break
        };
        self.emit_value_block_stmts(&block.stmts, term, false)?;
        self.indent -= 1;
        self.pad();
        self.out.push('}');
        self.return_cast = saved;
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), EmitError> {
        self.source_map.push((self.out.len(), stmt.span()));
        match stmt {
            Stmt::Let(l) => {
                // `let` (not `const`): a `mut` statement may reassign it later.
                let ty = match &l.ty {
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                match &l.value {
                    // A `match` that is the WHOLE initializer never needs the value
                    // IIFE: declare the binding, then lower the match as a statement
                    // `switch` whose value arms assign it and whose block arms keep
                    // function-level `return`. Doing this unconditionally (rather
                    // than only when an arm is a block) is what makes an `await` arm
                    // legal (no synchronous arrow wrapping it, TS1308), keeps a
                    // self-referential accumulator out of circular inference
                    // (TS7024), and lets a block arm appear at all. The
                    // `default: throw` on an exhaustive switch keeps `tsc`'s
                    // definite-assignment happy.
                    Expr::Match { scrutinee, arms, .. } => {
                        // When an arm also binds `l.name` (a destructured field
                        // of the same name, or a `let` in a block arm), the
                        // arm's `const` would shadow the declaration this
                        // statement is assigning, emitting `const x = __m0.x;
                        // x = x;`. Route the assignment through a synthesized
                        // temporary in that case and copy it out afterward. The
                        // non-colliding case — every program that has ever
                        // compiled — emits byte-identical TS.
                        let target = if match_binds_name(arms, l.name.as_ref()) {
                            let t = self.fresh_temp("__a");
                            self.line(&format!("let {t}{ty};"));
                            Some(t)
                        } else {
                            self.line(&format!("let {}{ty};", l.name));
                            None
                        };
                        let assign = target.clone().unwrap_or_else(|| l.name.to_string());
                        let prev = self.assign_target.borrow_mut().replace(assign);
                        let res = self.emit_scoped_match(
                            scrutinee,
                            arms,
                            ArmTerm::Assign,
                            target.is_some(),
                        );
                        *self.assign_target.borrow_mut() = prev;
                        res?;
                        if let Some(t) = target {
                            self.line(&format!("let {}{ty} = {t};", l.name));
                        }
                    }
                    _ => {
                        // `emit_value` hoists any `?` in the initializer first, so
                        // both a whole-value `?` (`let x = E?`) and a mid-chain `?`
                        // (`let x = await f()?.g()`) propagate the `Err` and bind
                        // the `Ok` payload.
                        let value = self.emit_value(&l.value)?;
                        if l.name.as_ref() == "_" {
                            // `let _ = expr` discards. It is the spelling the
                            // unused-binding lint tells you to use, so a function
                            // that ignores two results writes it twice, and two
                            // `const _` in one scope is a `tsc` redeclaration
                            // error about a variable the author never meant to
                            // declare. Emitting the initializer as a statement
                            // keeps the effect, drops the binding, and cannot
                            // collide. A named `_foo` still binds.
                            self.line(&format!("{value};"));
                        } else {
                            self.line(&format!("let {}{ty} = {value};", l.name));
                        }
                    }
                }
            }
            Stmt::Mut(m) => match &m.kind {
                // `mut <lvalue> = match { ... }`: the mirror of the `let` case
                // above. The lvalue is rendered first (so `a.b` and `a[i]` work)
                // and becomes the assign target; no declaration is emitted, since
                // the binding already exists.
                MutKind::Assign {
                    target,
                    value: Expr::Match { scrutinee, arms, .. },
                } => {
                    let t = self.lvalue(target)?;
                    // Same shadowing guard as the `let` case: an arm binding
                    // that shares a name with the lvalue would make the arm's
                    // `const` the thing assigned.
                    let tmp = if lvalue_mentions_match_binding(arms, &t) {
                        let tmp = self.fresh_temp("__a");
                        self.line(&format!("let {tmp};"));
                        Some(tmp)
                    } else {
                        None
                    };
                    let assign = tmp.clone().unwrap_or_else(|| t.clone());
                    let prev = self.assign_target.borrow_mut().replace(assign);
                    let res =
                        self.emit_scoped_match(scrutinee, arms, ArmTerm::Assign, tmp.is_some());
                    *self.assign_target.borrow_mut() = prev;
                    res?;
                    if let Some(tmp) = tmp {
                        self.line(&format!("{t} = {tmp};"));
                    }
                }
                MutKind::Assign { target, value } => {
                    // `emit_value` hoists any `?` in the RHS (like `let`),
                    // emitting the unwrap before this assignment; the target
                    // is a plain lvalue and needs no hoisting.
                    let t = self.lvalue(target)?;
                    let v = self.emit_value(value)?;
                    self.line(&format!("{t} = {v};"));
                }
                MutKind::MethodCall { call } => {
                    let v = self.emit_value(call)?;
                    self.line(&format!("{v};"));
                }
            },
            Stmt::Return(r) => match &r.value {
                // A `return match { ... }` lowers to a `switch` statement so
                // that `return` keeps its function-return semantics (an IIFE
                // would capture the return). Each arm returns its value.
                Some(Expr::Match { scrutinee, arms, .. }) => {
                    self.emit_match_dispatch(scrutinee, arms, ArmTerm::Return)?;
                }
                Some(v) => {
                    let v = self.emit_value(v)?;
                    self.emit_return(&v);
                }
                None => self.line("return;"),
            },
            Stmt::For(f) => {
                let iter = self.expr(&f.iter)?;
                let header = match f.bindings.as_slice() {
                    // `for x in xs` over an array/iterable: a `for...of`.
                    //
                    // Except over `array.range(n)`, which is the shape everyone
                    // reaches for to count. `range` *builds* an n-element array
                    // and the loop then walks it, so the idiom that reads like a
                    // counting loop allocates one array per execution. Measured
                    // over an 81-element scan, 200k rounds: `for c in cells` 40
                    // ms, `array.filter` with a closure 72 ms, and this form 168
                    // ms — the slowest of the three, and the one an outside
                    // team's benchmark recommended for a hot path (G117). It
                    // lowers to a counting `for` instead, which allocates
                    // nothing and is what a reader already assumes it is.
                    [v] => match self.counting_range(&f.iter)? {
                        Some(bounds) => {
                            let end = self.fresh_temp("end");
                            format!(
                                "for (let {v} = {}, {end} = {}; {v} < {end}; {v}++) ",
                                bounds.start, bounds.end
                            )
                        }
                        None => format!("for (const {v} of {iter}) "),
                    },
                    // `for k, v in it` over key/value pairs. An array's pairs are
                    // `it.entries()` — the index is a NUMBER. A record is a plain
                    // object, so its pairs are `Object.entries(it)` — the key is a
                    // STRING.
                    //
                    // The two differ, so a wrong guess is a wrong program, not a
                    // style choice. This used to default to the record form when
                    // the iterand's type was unknown, which made the index a
                    // string at run time in a build reporting no diagnostics and
                    // a clean `tsc --strict`: `index + 1` computed `"01"`. A
                    // value Glyph cannot see the type of takes that path, and
                    // the `Ok` payload of a generic record's `parse` is one.
                    //
                    // A known type still emits the direct form. An unknown one
                    // defers to the run time, which knows exactly what the value
                    // is when the compiler does not.
                    [k, v] => {
                        let pairs = match self.iter_shape(&f.iter) {
                            IterShape::Array => format!("{iter}.entries()"),
                            IterShape::Record => format!("{}.entries({iter})", self.g("Object")),
                            // Written bare, like `__glyph_index` and
                            // `__glyph_eq`: a Glyph runtime global the bootstrap
                            // installs, not a JS one a module could shadow.
                            IterShape::Unknown => format!("__glyph_pairs({iter})"),
                        };
                        format!("for (const [{k}, {v}] of {pairs}) ")
                    }
                    _ => {
                        return Err(EmitError::Unsupported {
                            construct: "a `for` loop with more than two bindings",
                            span: f.span,
                        })
                    }
                };
                self.emit_loop(&header, &f.body)?;
            }
            Stmt::Loop(l) => {
                self.emit_loop("while (true) ", &l.body)?;
            }
            // A user `break`/`continue` always targets the enclosing loop. When
            // that loop is labeled (the jump is buried in a `match`, i.e. a
            // `switch`, that would otherwise capture it), emit the labeled form.
            Stmt::Break(_) => self.line(&self.loop_jump("break")),
            Stmt::Continue(_) => self.line(&self.loop_jump("continue")),
            // A `defer` reached here has no following statements to guard (the
            // sequence emitters wrap the rest of a block in `try`/`finally` when
            // a defer precedes it); running it in place is the equivalent effect.
            Stmt::Defer(d) => {
                let v = self.emit_value(&d.expr)?;
                self.line(&format!("{v};"));
            }
            Stmt::Expr(Expr::Match { scrutinee, arms, .. }) => {
                // A statement-position `match` runs each arm for its effects
                // and `break`s out of the switch.
                self.emit_match_dispatch(scrutinee, arms, ArmTerm::Break)?;
            }
            Stmt::Expr(Expr::Postfix {
                op: PostfixOp::Try,
                operand,
                ..
            }) => {
                // A bare `E?` statement: propagate `Err`, discard the `Ok` value.
                self.emit_try_unwrap(operand)?;
            }
            Stmt::Expr(e) => {
                let s = self.emit_value(e)?;
                self.line(&format!("{s};"));
            }
        }
        Ok(())
    }

    /// Emit a loop (`while`/`for`) given its already-rendered header (ending in a
    /// space) and body. The loop is labeled iff its body contains a
    /// `break`/`continue` that a lowered `match` (a `switch`) would capture, so
    /// the labeled jump reaches the loop instead of breaking only the switch.
    /// The label (or `None`) is pushed for the body so `break`/`continue` read
    /// it, then popped.
    fn emit_loop(&mut self, header: &str, body: &Block) -> Result<(), EmitError> {
        let label = if loop_body_needs_label(body) {
            Some(self.fresh_temp("__loop"))
        } else {
            None
        };
        self.pad();
        if let Some(l) = &label {
            self.out.push_str(&format!("{l}: "));
        }
        self.out.push_str(header);
        self.loop_labels.push(label);
        let r = self.emit_block(body);
        self.loop_labels.pop();
        r?;
        self.out.push('\n');
        Ok(())
    }

    /// Render a `break`/`continue` statement for the innermost enclosing loop,
    /// labeled when that loop carries a label (see `emit_loop`).
    fn loop_jump(&self, kw: &str) -> String {
        match self.loop_labels.last() {
            Some(Some(label)) => format!("{kw} {label};"),
            _ => format!("{kw};"),
        }
    }

    /// Emit the inlined unwrap of a `?` operand: bind the operand `Result` to a
    /// fresh temporary, propagate an `Err` by returning it from the enclosing
    /// function, and return the temporary's name so the caller can read its
    /// `Ok` payload (`<tmp>.value`). The typechecker has already proven the
    /// operand is a `Result` and the function returns a compatible `Result`.
    fn emit_try_unwrap(&mut self, operand: &Expr) -> Result<String, EmitError> {
        let op = self.expr(operand)?;
        let r = self.fresh_temp("__r");
        self.line(&format!("const {r} = {op};"));
        // Propagate by re-wrapping the error (`Err(__r.value)`, a
        // `Result<never, E>`) rather than returning `__r` itself. The re-wrap is
        // assignable to any `Result<Y, E>` the enclosing function returns, which
        // `return __r` is not once `Result` carries `T`-dependent combinator
        // methods. `used_try` triggers the generated `Err` import.
        self.used_try.set(true);
        self.line(&format!(
            "if ({r}.{TAG} === \"{RESULT_ERR}\") {{ return {ERR_CTOR}({r}.{PAYLOAD}); }}"
        ));
        Ok(r)
    }

    /// Emit an expression that is a statement's value (a `let`/`return`/tail
    /// value, or a bare expression statement). Any `?` nested inside it (a
    /// mid-chain `?`, a `?` in an argument, etc.) is first hoisted to preceding
    /// statements; the returned string is the value with each `?` replaced by
    /// its unwrapped `Ok` payload. A `?` that is the whole statement value is
    /// also handled here, so the statement emitter need not special-case it.
    fn emit_value(&mut self, e: &Expr) -> Result<String, EmitError> {
        if contains_hoistable_try(e) {
            // Place each `await` on the head async call of its spine BEFORE
            // hoisting, so a mid-chain `?` whose operand is that call hoists the
            // AWAITED result (`const __r = await load(p)`), not the pending
            // Promise. Without this the `Err` guard tests `Promise.tag` (always
            // false) and the chain reads `Promise.value` (a runtime crash).
            let placed = place_awaits(e);
            let lifted = self.hoist_tries(&placed)?;
            self.expr(&lifted)
        } else {
            self.expr(e)
        }
    }

    /// Hoist every `?` nested in `e` out to a preceding statement: for each, in
    /// evaluation order, emit its inlined unwrap (`emit_try_unwrap`) and replace
    /// the `?` with a read of the temporary's `Ok` payload (`__rN.value`).
    /// Returns the rewritten expression, which is free of `?` and so emits
    /// through `expr` directly.
    ///
    /// Does not descend into a lambda body or a nested `match`/JSX: a `?` there
    /// belongs to that construct's own statement context and is hoisted when it
    /// is emitted.
    ///
    /// `emit_value` runs `place_awaits` first, so when a `?` operand is an
    /// awaited call the `await` already sits on that call and the hoisted temp
    /// holds the awaited `Result` rather than a pending Promise.
    fn hoist_tries(&mut self, e: &Expr) -> Result<Expr, EmitError> {
        Ok(match e {
            Expr::Postfix {
                op: PostfixOp::Try,
                operand,
                span,
            } => {
                let operand = self.hoist_tries(operand)?;
                let r = self.emit_try_unwrap(&operand)?;
                Expr::Member {
                    object: Box::new(Expr::Ident {
                        name: Arc::from(r.as_str()),
                        span: *span,
                    }),
                    field: Arc::from(PAYLOAD),
                    optional: false,
                    span: *span,
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => Expr::Binary {
                op: *op,
                left: Box::new(self.hoist_tries(left)?),
                right: Box::new(self.hoist_tries(right)?),
                span: *span,
            },
            Expr::Unary { op, operand, span } => Expr::Unary {
                op: *op,
                operand: Box::new(self.hoist_tries(operand)?),
                span: *span,
            },
            Expr::Call {
                callee,
                type_args,
                args,
                span,
            } => {
                let callee = Box::new(self.hoist_tries(callee)?);
                let mut new_args = Vec::with_capacity(args.len());
                for a in args {
                    new_args.push(self.hoist_tries(a)?);
                }
                Expr::Call {
                    callee,
                    type_args: type_args.clone(),
                    args: new_args,
                    span: *span,
                }
            }
            Expr::Member {
                object,
                field,
                optional,
                span,
            } => Expr::Member {
                object: Box::new(self.hoist_tries(object)?),
                field: field.clone(),
                optional: *optional,
                span: *span,
            },
            Expr::Index {
                object,
                index,
                span,
            } => Expr::Index {
                object: Box::new(self.hoist_tries(object)?),
                index: Box::new(self.hoist_tries(index)?),
                span: *span,
            },
            Expr::Await { expr, span } => Expr::Await {
                expr: Box::new(self.hoist_tries(expr)?),
                span: *span,
            },
            Expr::Array { elements, span } => {
                let mut els = Vec::with_capacity(elements.len());
                for el in elements {
                    els.push(match el {
                        ArrayElem::Expr(e) => ArrayElem::Expr(self.hoist_tries(e)?),
                        ArrayElem::Spread(e) => ArrayElem::Spread(self.hoist_tries(e)?),
                    });
                }
                Expr::Array {
                    elements: els,
                    span: *span,
                }
            }
            Expr::Object { fields, span } => {
                let mut fs = Vec::with_capacity(fields.len());
                for f in fields {
                    fs.push(match f {
                        ObjectField::KeyValue { key, value, span } => ObjectField::KeyValue {
                            key: key.clone(),
                            value: self.hoist_tries(value)?,
                            span: *span,
                        },
                        ObjectField::Spread { value, span } => ObjectField::Spread {
                            value: self.hoist_tries(value)?,
                            span: *span,
                        },
                    });
                }
                Expr::Object {
                    fields: fs,
                    span: *span,
                }
            }
            Expr::TemplateString { parts, span } => {
                let mut ps = Vec::with_capacity(parts.len());
                for p in parts {
                    ps.push(match p {
                        TemplatePart::Text { content, span } => TemplatePart::Text {
                            content: content.clone(),
                            span: *span,
                        },
                        TemplatePart::Expr { value, span } => TemplatePart::Expr {
                            value: self.hoist_tries(value)?,
                            span: *span,
                        },
                    });
                }
                Expr::TemplateString {
                    parts: ps,
                    span: *span,
                }
            }
            // Leaves, and the opaque lambda/match/JSX constructs, carry no
            // hoistable `?` of their own: clone unchanged.
            other => other.clone(),
        })
    }

    /// A fresh synthesized temporary name (`__r0`, `__m1`, ...). Bumping the
    /// counter here keeps every call site from forgetting it.
    fn fresh_temp(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.tmp_counter);
        self.tmp_counter += 1;
        name
    }

    /// The variant names of the tagged union `ty` refers to, used to tell a
    /// bare-identifier arm (a no-payload variant) from a binding. Resolves a
    /// module-local `Ty::Named` to its `type X = | A | B` declaration; prelude
    /// unions and non-union (or unknown) types return None.
    ///
    /// This `Ty::Named` → `TypeDecl` → union chain is the third copy (after
    /// `assign.rs::resolve_named_union` and `owned.rs`); a public helper in
    /// `glyph-typechecker` that all three consume is a worthwhile cleanup.
    /// Whether `iter`'s inferred type is the prelude `Array` (`Array<T>` lowers
    /// to `App(Array, [T])`). Used to choose `it.entries()` (numeric index) over
    /// `Object.entries(it)` (string key) for a two-binding `for`. An unknown
    /// type (e.g. a value narrowed by an `is Array<..>` arm, before flow
    /// narrowing tracks it) answers false and falls back to the record form.
    /// The bounds of `array.range(n)` / `array.range_from(a, b)` when a
    /// single-binding `for` iterates one directly, so it can lower to a counting
    /// loop instead of walking a freshly-allocated array.
    ///
    /// Only a *direct* call qualifies. A range bound to a `let` first, or passed
    /// through anything, keeps the `for...of`: the value is a real array there
    /// and something else may hold it.
    ///
    /// Both bounds are emitted once, into the loop's own initializer, so a call
    /// in either position is evaluated exactly as often as `range` would have
    /// evaluated it. Emitting `i < f()` instead would call `f` every iteration.
    fn counting_range(&self, iter: &Expr) -> Result<Option<CountingRange>, EmitError> {
        let Expr::Call { callee, args, .. } = iter else {
            return Ok(None);
        };
        let Expr::Member { object, field, .. } = callee.as_ref() else {
            return Ok(None);
        };
        let Expr::Ident { name, .. } = object.as_ref() else {
            return Ok(None);
        };
        if !self.is_std_array_namespace(name.as_ref()) {
            return Ok(None);
        }
        match (field.as_ref(), args.len()) {
            ("range", 1) => Ok(Some(CountingRange {
                start: "0".to_string(),
                end: self.expr(&args[0])?,
            })),
            ("range_from", 2) => Ok(Some(CountingRange {
                start: self.expr(&args[0])?,
                end: self.expr(&args[1])?,
            })),
            _ => Ok(None),
        }
    }

    /// Whether `name` is this module's local binding for `std/array`.
    /// Mirrors `is_json_namespace`.
    fn is_std_array_namespace(&self, name: &str) -> bool {
        self.module.items.iter().any(|d| {
            let Decl::Import(im) = d else { return false };
            let path: Vec<&str> = im.path.segments.iter().map(|s| s.as_ref()).collect();
            if path != ["std", "array"] {
                return false;
            }
            match &im.kind {
                ImportKind::Namespace => {
                    im.path.segments.last().map(|s| s.as_ref()) == Some(name)
                }
                ImportKind::Aliased(alias) => alias.as_ref() == name,
                ImportKind::Named(_) | ImportKind::Default(_) => false,
            }
        })
    }

    /// Whether an imported name has no runtime binding in its source module, so
    /// its import must carry the inline `type` modifier (G114).
    ///
    /// Two populations. The hand-written standard library declares names with
    /// `export type` and ships no value for them. And a Glyph type emits a
    /// runtime descriptor `const` under its own name **only when it has one**:
    /// a record, a tagged union, a refined primitive. A plain alias
    /// (`type Board = Array<Cell>`) emits `export type Board` alone, so
    /// importing it by name across modules has the same problem.
    ///
    /// Anything else — a function, a const, a variant constructor — is a value
    /// and must stay bare, because marking it `type` would elide the import and
    /// remove a binding the program needs.
    fn import_name_is_type_only(&self, module_path: &str, name: &str) -> bool {
        if glyph_resolver::is_stdlib_type_only(module_path, name) {
            return true;
        }
        let key = (module_path.to_string(), name.to_string());
        self.ctx.descriptorless_aliases.contains_key(&key)
    }

    /// What a two-binding `for`'s iterand is, as far as the checker can tell.
    ///
    /// The three answers are genuinely different, and collapsing `Unknown` into
    /// either of the others is how a loop index silently became a string. A
    /// record type answers `Record`; the prelude `Array` answers `Array`;
    /// anything the checker did not resolve answers `Unknown` and is settled at
    /// run time instead of guessed here.
    fn iter_shape(&self, iter: &Expr) -> IterShape {
        let ty = self.types.get(iter.span());
        if matches!(&ty, Ty::App { base, .. }
            if matches!(base.as_ref(), Ty::Named { path, .. }
                if path.last().map(|n| n.as_ref()) == Some("Array")))
        {
            return IterShape::Array;
        }
        match &ty {
            Ty::Unknown => IterShape::Unknown,
            // A `Ty::Imported` crosses a module boundary with no shape attached,
            // so it is no more settled than `Unknown`.
            Ty::Imported { .. } => IterShape::Unknown,
            _ => IterShape::Record,
        }
    }

    fn union_variant_names(&self, ty: &Ty) -> Option<Vec<String>> {
        // A generic union applied to type arguments (`Box<string>`) is a
        // `Ty::App` over the union's `Ty::Named`; unwrap to the base so a match
        // on a generic union resolves its variants like a monomorphic one.
        let ty = match ty {
            Ty::App { base, .. } => base.as_ref(),
            other => other,
        };
        let Ty::Named { symbol, path } = ty else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(SymbolId(symbol.0))?;
        // Prelude and module symbol tables both number ids from 0, so a
        // prelude `Ty::Named` (e.g. a bare `Option`) could index an unrelated
        // module symbol here. Require the resolved symbol's name to match the
        // type's path, which a genuine prelude id never will (the same
        // collision `assign.rs::prelude_app` and `owned.rs` guard).
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return None;
        }
        let decl_idx = match &sym.kind {
            SymbolKind::Type { decl_idx } => *decl_idx,
            _ => return None,
        };
        let Decl::Type(td) = self.module.items.get(decl_idx as usize)? else {
            return None;
        };
        let TypeExpr::Union { variants, .. } = &td.body else {
            return None;
        };
        Some(variants.iter().map(|v| v.name.to_string()).collect())
    }

    /// Whether the variant named `variant` of the tagged union `ty` carries a
    /// record payload (spread flat into the scrutinee object as
    /// `{ tag, ...fields }`), as opposed to a single-value payload (stored under
    /// `value`) or no payload. A single-name binding `Variant(v)` must bind the
    /// whole scrutinee object for a record payload, but `scrutinee.value` for a
    /// single-value one. Prelude variants (`Ok`/`Err`/`Some`) are always
    /// single-value, so this returns false for them and for any type whose
    /// union declaration cannot be resolved (mirroring `union_variant_names`).
    fn variant_payload_is_record(&self, ty: &Ty, variant: &str) -> bool {
        let ty = match ty {
            Ty::App { base, .. } => base.as_ref(),
            other => other,
        };
        // The scrutinee is typed by an imported union, so its variant shapes
        // live in the project registry rather than in this module's AST. Keying
        // the lookup on the scrutinee's *own* module makes the namespace
        // spelling (`err.BadLeadByte(v)`) resolve exactly as the named-import
        // spelling does, which the by-name fallback below cannot: that spelling
        // never binds the variant name in the consumer's symbol table.
        //
        // This is the deciding answer and so it comes first. The fallback below
        // asks only whether *some* symbol of this name is a record-payload
        // import, never what is being matched, so it gets `b.Hit(n)` wrong
        // whenever an unrelated `a.Hit` is also in scope.
        if let Ty::Imported { module, .. } = ty {
            return self
                .ctx
                .record_payload_variants
                .contains(&(module.as_str().to_string(), variant.to_string()));
        }
        // Cross-module fallback for a scrutinee the checker did not pin to a
        // `Ty::Imported` (an inferred `let` bound to a cross-module call has no
        // type at all today). Resolve the variant to its source module via its
        // own `ImportNamed` symbol and consult the project registry.
        if self.imported_variant_is_record(variant) {
            return true;
        }
        let Ty::Named { symbol, path } = ty else {
            return false;
        };
        let Some(sym) = self.resolved.symbols.table.get(SymbolId(symbol.0)) else {
            return false;
        };
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return false;
        }
        let SymbolKind::Type { decl_idx } = &sym.kind else {
            return false;
        };
        let Some(Decl::Type(td)) = self.module.items.get(*decl_idx as usize) else {
            return false;
        };
        let TypeExpr::Union { variants, .. } = &td.body else {
            return false;
        };
        variants
            .iter()
            .find(|v| v.name.as_ref() == variant)
            .is_some_and(|v| matches!(v.payload, Some(TypeExpr::Record { .. })))
    }

    /// Whether `variant` is an imported record-payload variant, resolved via its
    /// own module's `ImportNamed` symbol and the project record-variant registry.
    /// A same-module variant name shadows an import in `by_name`, so this only
    /// fires for a genuinely imported one.
    fn imported_variant_is_record(&self, variant: &str) -> bool {
        let Some(&sym_id) = self.resolved.symbols.by_name.get(variant) else {
            return false;
        };
        let Some(sym) = self.resolved.symbols.table.get(sym_id) else {
            return false;
        };
        let SymbolKind::ImportNamed { path, original } = &sym.kind else {
            return false;
        };
        let module_path: String = path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        self.ctx
            .record_payload_variants
            .contains(&(module_path, original.to_string()))
    }

    /// The type of an outer variant's payload, for a scrutinee whose payload is
    /// a type argument (`Result<T, E>`: `Err` -> `E`, `Ok`/`Some` -> `T`). Used
    /// by `degroup_nested_arms` to record the synthesized inner scrutinee's type
    /// so the inner match binds a record payload as the whole object, not
    /// `.value`. Returns `None` for a non-App type (a user union's variant
    /// payloads are not carried as type arguments — the rarer nested case).
    fn outer_variant_payload_ty(&self, scrutinee_ty: &Ty, outer: &str) -> Option<Ty> {
        let Ty::App { args, .. } = scrutinee_ty else {
            return None;
        };
        match outer {
            "Ok" | "Some" => args.first().cloned(),
            "Err" => args.get(1).cloned(),
            _ => None,
        }
    }

    /// The type of a `match` scrutinee, consulting `synth_types` for synthesized
    /// temporaries the `TypeMap` doesn't know about, then the `TypeMap`.
    fn scrutinee_ty(&self, expr: &Expr) -> Ty {
        if let Expr::Ident { name, .. } = expr {
            if let Some(ty) = self.synth_types.borrow().get(name.as_ref()) {
                return ty.clone();
            }
        }
        self.types.get(expr.span()).clone()
    }

    /// The TypeScript type a read of `ty` must be re-asserted to, when
    /// TypeScript's assignment narrowing can pin it below the type Glyph gave
    /// it. `None` when it cannot, which is every other type.
    ///
    /// Glyph does not narrow a binding by what was last assigned to it: `let
    /// done = false` has type `bool` at every read, and `match done { true =>
    /// .., false => .. }` matches over both. TypeScript narrows, and its
    /// `boolean` is the union `true | false`, so the emitted `switch` gets a
    /// discriminant of type `false` and rejects the `true` arm with TS2678, an
    /// error naming a type the author never wrote, on a program Glyph itself
    /// found nothing wrong with. A D30 string-literal union is a union the same
    /// way, and a comparison against another member fails as TS2367 instead.
    /// An assignment inside a callback does not re-widen it either, which is
    /// what made the flag-set-in-a-`std/timers`-callback bridge uncompilable.
    ///
    /// The assertion re-states the type the checker already gave the value,
    /// so a literal outside the union still fails where it failed before
    /// (`m == "nope"` is still TS2367, and now names `Mode` rather than
    /// `"fast"`). It is not a no-op, though: `as` permits a downcast, so a
    /// value Glyph types as `bool` whose TypeScript type is `unknown` would
    /// now pass where the `switch` used to complain; a genuine model drift
    /// such as `string` against `bool` still errors, as TS2352 instead of
    /// TS2678. Asserting at the *write* instead (`"nope" as Mode`) is not an
    /// identity either, and would swallow the mismatch D30 leaves to `tsc`.
    fn narrowable_union_ts(&self, ty: &Ty) -> Option<String> {
        match ty {
            Ty::Prim(Primitive::Bool) => Some("boolean".to_string()),
            Ty::StringLiteralUnion(values) => Some(
                values
                    .iter()
                    .map(|v| escape_double_quoted(v.as_str()))
                    .collect::<Vec<_>>()
                    .join(" | "),
            ),
            // A named alias keeps its name (`mode as Mode`): the emitted cast
            // reads as the type the author declared, and the literal set stays
            // in one place.
            Ty::Named { path, .. } if self.named_alias_is_narrowable(ty) => {
                Some(path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("."))
            }
            _ => None,
        }
    }

    /// Whether a module-local `Ty::Named` aliases a type TypeScript treats as a
    /// union: `type Mode = "fast" | "slow"` (D30), or an alias of `bool`. Walks
    /// the same `Ty::Named` -> `TypeDecl` chain as `variant_payload_is_record`.
    fn named_alias_is_narrowable(&self, ty: &Ty) -> bool {
        let Ty::Named { symbol, path } = ty else {
            return false;
        };
        let Some(sym) = self.resolved.symbols.table.get(SymbolId(symbol.0)) else {
            return false;
        };
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return false;
        }
        let SymbolKind::Type { decl_idx } = &sym.kind else {
            return false;
        };
        let Some(Decl::Type(td)) = self.module.items.get(*decl_idx as usize) else {
            return false;
        };
        match &td.body {
            TypeExpr::StringLiteralUnion { .. } => true,
            TypeExpr::Path { segments, .. } => {
                segments.len() == 1 && segments[0].as_ref() == "bool"
            }
            _ => false,
        }
    }

    /// One side of a `==`/`!=`, pinned back to its own type. Equality is the
    /// second place TypeScript's stale narrowing surfaces (TS2367, "no
    /// overlap"); see `narrowable_union_ts`. Like the `match` scrutinee, this
    /// is a read-site pin and does not consult the other operand: `done ==
    /// failed` between two `bool` bindings fails the same way `done == true`
    /// does, and there is no rule under which one should compile and the other
    /// should not. Nothing useful is suppressed, because TS2367 never fires
    /// between two operands TypeScript types as `boolean`. A literal operand is
    /// left alone: it has no narrowing to undo.
    fn compared_operand(&self, e: &Expr) -> Result<String, EmitError> {
        let rendered = self.expr(e)?;
        if matches!(e, Expr::Bool { .. } | Expr::String { .. }) {
            return Ok(rendered);
        }
        let ty = self.scrutinee_ty(e);
        Ok(match self.narrowable_union_ts(&ty) {
            Some(t) => format!("({rendered} as {t})"),
            None => rendered,
        })
    }

    /// Whether an operand's type is a primitive, so `===` already means value
    /// equality for it.
    ///
    /// Deliberately conservative: anything the checker did not pin down
    /// (`Unknown`, `unknown`, a generic parameter, an imported or opaque type)
    /// is *not* treated as primitive, so it routes through the structural
    /// comparison. That helper's first line is `a === b`, so an operand that
    /// turns out to be a primitive at run time still costs one comparison and
    /// gives the same answer.
    fn is_primitive_operand(&self, e: &Expr) -> bool {
        match self.scrutinee_ty(e) {
            // `int` and `bigint` are named types over a primitive, and a
            // string-literal union is a set of strings; all compare correctly
            // with `===`.
            Ty::Prim(_) | Ty::StringLiteralUnion(_) => true,
            Ty::Named { ref symbol, ref path } => {
                // `int` and `bigint` are named types over a primitive.
                if matches!(
                    path.last().map(|p| p.as_ref()),
                    Some("int") | Some("bigint")
                ) {
                    return true;
                }
                // A module-local alias for something primitive, most often a
                // string-literal union (`type Tier = "free" | "pro"`). Resolving
                // it keeps `t == "pro"` a plain `===` in the emitted TypeScript
                // rather than a helper call that would give the same answer more
                // slowly and read worse.
                self.local_alias_is_primitive(*symbol, path)
            }
            _ => false,
        }
    }

    /// Whether a module-local `type X = ...` names a primitive or a
    /// string-literal union. One level of alias only: a chain is rare, and
    /// stopping keeps this from needing cycle detection for a formatting nicety.
    fn local_alias_is_primitive(&self, symbol: glyph_typechecker::ty::SymbolRef, path: &[Ident]) -> bool {
        let Some(sym) = self.resolved.symbols.table.get(SymbolId(symbol.0)) else {
            return false;
        };
        if path.last().map(|n| n.as_ref()) != Some(sym.name.as_ref()) {
            return false;
        }
        let SymbolKind::Type { decl_idx } = &sym.kind else {
            return false;
        };
        let Some(Decl::Type(td)) = self.module.items.get(*decl_idx as usize) else {
            return false;
        };
        match &td.body {
            TypeExpr::Path { segments, .. } => matches!(
                segments.last().map(|p| p.as_ref()),
                Some("string") | Some("number") | Some("bool") | Some("int") | Some("bigint")
            ),
            // A union of string literals and nothing else.
            TypeExpr::Union { variants, .. } => variants
                .iter()
                .all(|v| v.payload.is_none() && v.name.as_ref().starts_with('"')),
            TypeExpr::StringLiteralUnion { .. } => true,
            _ => false,
        }
    }

    /// Variant names of `outer_variant`'s payload union in `scrutinee_ty`, when
    /// that payload is itself a tagged union. Lets `degroup_nested_arms`
    /// recognize a nested *nullary* variant (`Err(Empty)` where `Empty` is a
    /// user variant): it parses as a `Pattern::Ident` and would otherwise be
    /// mistaken for a payload binding, producing a duplicate `case "Err"` that
    /// silently swallows every `Err`. Handles the prelude `Result`/`Option`
    /// shape, whose payload is a type argument (`Result<T, E>`: `Err` -> `E`).
    fn nested_payload_variants(&self, scrutinee_ty: &Ty, outer_variant: &str) -> Option<Vec<String>> {
        let Ty::App { args, .. } = scrutinee_ty else {
            return None;
        };
        let payload = match outer_variant {
            "Ok" | "Some" => args.first()?,
            "Err" => args.get(1)?,
            _ => return None,
        };
        self.union_variant_names(payload)
    }

    /// Lower a `match` over a tagged union to a `switch` on the `tag`
    /// discriminant. Handles constructor-pattern arms (`Ok(x)`,
    /// `NetworkError({ url })`, dotted `fs.ErrorKind.NotFound`), bare no-payload
    /// variant arms (`Idle`, disambiguated from bindings via the scrutinee
    /// type), `_`/`else`, and binding catch-alls (a bare identifier the
    /// scrutinee type does not confirm as a variant — lowered to a `default:`
    /// that binds the scrutinee to the name). In a `tag` switch a `default`
    /// catches exactly the variants no `case` lists, so a binding arm remains
    /// runtime-correct even when the scrutinee type is unknown. Value (literal)
    /// matches are handled too; `is`/array patterns route to their own chains.
    /// Rewrite arms so each outer variant carrying nested constructor patterns
    /// dispatches its payload through an inner `match`. `Err(NetworkError({ s
    /// }))` and `Err(DecodeError({ u }))` become a single `Err(__pN) => match
    /// __pN { NetworkError({ s }) => .., DecodeError({ u }) => .. }`. Arms with
    /// no nested argument are preserved in place; a nested group takes the
    /// position of its first arm and collects later arms of the same outer
    /// variant. Order is otherwise preserved. Deeper nesting is handled when the
    /// synthesized inner `match` is itself emitted.
    fn degroup_nested_arms(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> Vec<MatchArm> {
        // Owned so no borrow of `self.types` is held across the `&mut self`
        // `fresh_temp` call below. Uses `scrutinee_ty` so a deeper-nested match
        // (whose scrutinee is itself a synthesized temp) resolves too.
        let scrutinee_ty = self.scrutinee_ty(scrutinee);
        // Outer variant tag -> index in `out` of its synthesized grouping arm.
        let mut group_at: Vec<(String, usize)> = Vec::new();
        let mut out: Vec<MatchArm> = Vec::new();
        for arm in arms {
            let Pattern::Constructor { path, args, .. } = &arm.pattern else {
                out.push(arm.clone());
                continue;
            };
            let [arg] = args.as_slice() else {
                out.push(arm.clone());
                continue;
            };
            let outer = path.last().map(|s| s.as_ref()).unwrap_or("");
            let tag = path
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(".");
            let already_grouped = group_at.iter().any(|(t, _)| *t == tag);
            // The single arg is a nested variant when it is a constructor
            // pattern, a literal (`Ok(true)`), a bare prelude variant
            // (`Ok(None)`), or a bare *user* nullary variant of the outer
            // variant's payload union (`Err(Empty)`). Once a variant is grouped,
            // a later same-variant arm whose arg is a wildcard or a plain binding
            // is absorbed as the inner match's catch-all, so a `Some(0) => ..,
            // Some(_) => ..` pair stays exhaustive rather than emitting a second
            // `case "Some":` that shadows the value dispatch.
            let inner: Pattern = match arg {
                Pattern::Constructor { .. } | Pattern::Literal { .. } => arg.clone(),
                Pattern::Ident { name, span }
                    if is_prelude_variant(name)
                        || self
                            .nested_payload_variants(&scrutinee_ty, outer)
                            .is_some_and(|vs| vs.iter().any(|v| v == name.as_ref())) =>
                {
                    // Rewrite the binding-shaped ident into an explicit nullary
                    // constructor so the inner switch dispatches on its tag
                    // rather than binding (and swallowing) the whole payload.
                    Pattern::Constructor {
                        path: vec![Arc::clone(name)],
                        args: vec![],
                        span: *span,
                    }
                }
                // A wildcard or plain binding is a genuine payload binding on its
                // own, but the catch-all of an already-open group when it follows
                // one for the same variant.
                Pattern::Wildcard { .. } | Pattern::Ident { .. } if already_grouped => arg.clone(),
                _ => {
                    out.push(arm.clone());
                    continue;
                }
            };
            let inner_arm = MatchArm {
                pattern: inner,
                body: arm.body.clone(),
                span: arm.span,
            };
            if let Some((_, idx)) = group_at.iter().find(|(t, _)| *t == tag) {
                if let MatchArmBody::Expr(Expr::Match { arms, .. }) = &mut out[*idx].body {
                    arms.push(inner_arm);
                }
            } else {
                let p = self.fresh_temp("__p");
                // Record the grouping temp's type (the outer variant's payload)
                // so the synthesized inner `match` on it binds a record payload
                // as the whole object rather than a non-existent `.value`.
                if let Some(pty) = self.outer_variant_payload_ty(&scrutinee_ty, outer) {
                    self.synth_types.borrow_mut().insert(p.clone(), pty);
                }
                let bind = Arc::from(p.as_str());
                let new_arm = MatchArm {
                    pattern: Pattern::Constructor {
                        path: path.clone(),
                        args: vec![Pattern::Ident {
                            name: Arc::clone(&bind),
                            span: arm.span,
                        }],
                        span: arm.span,
                    },
                    body: MatchArmBody::Expr(Expr::Match {
                        scrutinee: Box::new(Expr::Ident {
                            name: bind,
                            span: arm.span,
                        }),
                        arms: vec![inner_arm],
                        span: arm.span,
                    }),
                    span: arm.span,
                };
                group_at.push((tag, out.len()));
                out.push(new_arm);
            }
        }
        out
    }

    /// `emit_match_dispatch`, optionally wrapped in a plain block so the arm
    /// bindings get a scope of their own.
    ///
    /// Needed when the statement around the match declares a name an arm also
    /// binds. The `switch` and `is`-chain lowerings already put their bindings
    /// inside a nested block, but the single-arm lowering (a `match` whose only
    /// arm is a binding) emits `const <name> = <scrutinee>;` at the statement's
    /// own level, where it collides with the declaration outright (TS2451). A
    /// `{ ... }` around the whole dispatch fixes every path at once and costs
    /// nothing: `break`, `continue` and `return` all pass through a plain block,
    /// and it is emitted only in the colliding case.
    fn emit_scoped_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        term: ArmTerm,
        scoped: bool,
    ) -> Result<(), EmitError> {
        if !scoped {
            return self.emit_match_dispatch(scrutinee, arms, term);
        }
        self.line("{");
        self.indent += 1;
        let res = self.emit_match_dispatch(scrutinee, arms, term);
        self.indent -= 1;
        self.line("}");
        res
    }

    fn emit_match_dispatch(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        term: ArmTerm,
    ) -> Result<(), EmitError> {
        // An `is TypeName` arm makes this a type-guard match, lowered to an
        // `if`/`else if` chain rather than a `switch`.
        if arms.iter().any(|a| matches!(a.pattern, Pattern::IsType { .. })) {
            return self.emit_is_chain(scrutinee, arms, term);
        }

        // An array pattern arm makes this an array match, lowered to a length-
        // and element-check `if`/`else if` chain (a primitive array has no tag
        // to switch on).
        if arms.iter().any(|a| matches!(a.pattern, Pattern::Array { .. })) {
            return self.emit_array_chain(scrutinee, arms, term);
        }

        // A nested constructor pattern (`Err(NetworkError({ status }))`) needs a
        // switch on the inner payload's tag. Rewrite each outer variant with
        // nested arms into one arm whose payload is dispatched by an inner
        // `match`, then re-emit: the inner match lowers through the tail-match
        // path, and deeper nesting recurses through this same rewrite.
        if arms.iter().any(arm_has_nested_constructor) {
            let rewritten = self.degroup_nested_arms(scrutinee, arms);
            return self.emit_match_dispatch(scrutinee, &rewritten, term);
        }

        // Variant names of the scrutinee's union, when its type is known.
        let scrutinee_ty = self.scrutinee_ty(scrutinee);
        let variants = self.union_variant_names(&scrutinee_ty);
        let is_variant = |name: &str| {
            is_prelude_variant(name)
                // A PascalCase bare ident is a variant reference (the resolver's
                // rule), so an *imported* union's nullary variant lowers to a
                // `case "V":` on `.tag` rather than being misread as a binding
                // catch-all. Its type is `Unknown` here, so the variant set below
                // is empty; the switch on `.tag` works regardless of provenance.
                || name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                || variants
                    .as_ref()
                    .is_some_and(|vs| vs.iter().any(|v| v == name))
        };

        for arm in arms {
            match &arm.pattern {
                Pattern::Constructor { args, span, .. } => match args.as_slice() {
                    []
                    | [Pattern::Ident { .. }]
                    | [Pattern::Wildcard { .. }]
                    | [Pattern::Object { .. }] => {}
                    _ => {
                        return Err(EmitError::Unsupported {
                            construct: "a nested or multi-argument pattern in a match arm",
                            span: *span,
                        })
                    }
                },
                Pattern::Wildcard { .. } | Pattern::Else { .. } => {}
                // A bare identifier is either a no-payload variant (when the
                // scrutinee type confirms it) or a binding catch-all. Both
                // lower below; the catch-all count guards against two bindings.
                Pattern::Ident { .. } => {}
                // A literal pattern makes this a value match (a `switch` on the
                // scrutinee value rather than its `tag`).
                Pattern::Literal { .. } => {}
                Pattern::Object { span, .. } | Pattern::Array { span, .. } => {
                    return Err(EmitError::Unsupported {
                        construct: "an object/array match pattern",
                        span: *span,
                    })
                }
                Pattern::IsType { span, .. } => {
                    return Err(EmitError::Unsupported {
                        construct: "an `is` type pattern in a match",
                        span: *span,
                    })
                }
            }
        }

        // A bare identifier that is not a variant is a binding catch-all,
        // equivalent to `_`/`else` but binding the scrutinee to its name. It
        // counts as a catch-all in the guards below.
        let is_catch_all = |a: &MatchArm| match &a.pattern {
            Pattern::Wildcard { .. } | Pattern::Else { .. } => true,
            Pattern::Ident { name, .. } => !is_variant(name),
            _ => false,
        };

        // Two catch-all arms would emit two `default:` clauses (invalid TS).
        // The typechecker does not yet reject the redundant arm, so guard here.
        if let Some(extra) = arms.iter().filter(|a| is_catch_all(a)).nth(1) {
            return Err(EmitError::Unsupported {
                construct: "a match with more than one catch-all arm",
                span: extra.span,
            });
        }

        // A match with no discriminating arm (only a catch-all) has nothing to
        // switch over. Evaluate the scrutinee for any effect (parenthesized so
        // an object-literal scrutinee isn't parsed as a block), then run the
        // lone catch-all arm.
        let has_variant_arm = arms.iter().any(|a| match &a.pattern {
            Pattern::Constructor { .. } => true,
            Pattern::Ident { name, .. } => is_variant(name),
            _ => false,
        });
        // A literal arm switches on the scrutinee value directly; a variant arm
        // switches on its `tag`. The two should never mix (a primitive has no
        // tag, a union no literal values) — but the typechecker does not yet
        // reject the mix, so guard rather than emit a switch that discriminates
        // some arms by value and others by tag.
        let is_value_match = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Literal { .. }));
        if has_variant_arm && is_value_match {
            let span = arms
                .iter()
                .find_map(|a| match &a.pattern {
                    Pattern::Literal { span, .. } => Some(*span),
                    _ => None,
                })
                .unwrap_or(arms[0].span);
            return Err(EmitError::Unsupported {
                construct: "a match mixing literal and variant patterns",
                span,
            });
        }
        if !has_variant_arm && !is_value_match {
            let scrut = self.expr(scrutinee)?;
            // A lone binding arm (`x => ...`) binds the scrutinee to its name;
            // a lone `_`/`else` evaluates it for effect (parenthesized so an
            // object-literal scrutinee isn't parsed as a block).
            match &arms[0].pattern {
                Pattern::Ident { name, .. } => self.line(&format!("const {name} = {scrut};")),
                _ => self.line(&format!("({scrut});")),
            }
            // No switch here, so no `break`.
            self.emit_arm_body(&arms[0].body, term, false)?;
            return Ok(());
        }

        let scrut = self.expr(scrutinee)?;
        // Pin a union-typed scrutinee back to its own type before the `switch`:
        // TypeScript would otherwise discriminate on whatever it last saw
        // assigned to it and reject every other arm. See `narrowable_union_ts`.
        let scrut_ty = self.scrutinee_ty(scrutinee);
        let scrut = match self.narrowable_union_ts(&scrut_ty) {
            Some(t) => format!("({scrut} as {t})"),
            None => scrut,
        };
        let m = self.fresh_temp("__m");
        self.line(&format!("const {m} = {scrut};"));
        let discriminant = if is_value_match {
            m.clone()
        } else {
            format!("{m}.{TAG}")
        };
        self.line(&format!("switch ({discriminant}) {{"));
        self.indent += 1;
        for arm in arms {
            match &arm.pattern {
                Pattern::Constructor { path, args, .. } => {
                    let variant = path.last().expect("constructor path is non-empty");
                    let record_payload =
                        self.variant_payload_is_record(&self.scrutinee_ty(scrutinee), variant);
                    self.line(&format!("case \"{variant}\": {{"));
                    self.indent += 1;
                    self.emit_arm_binds(&m, args, record_payload);
                    self.emit_arm_body(&arm.body, term, true)?;
                    self.indent -= 1;
                    self.line("}");
                }
                // A bare identifier is a no-payload variant when the scrutinee
                // type confirms it (a `case "Name":` with no payload binding),
                // otherwise a binding catch-all: a `default:` that binds the
                // scrutinee to the name so the arm body can read it.
                Pattern::Ident { name, .. } => {
                    if is_variant(name) {
                        self.line(&format!("case \"{name}\": {{"));
                        self.indent += 1;
                        self.emit_arm_body(&arm.body, term, true)?;
                        self.indent -= 1;
                        self.line("}");
                    } else {
                        self.line("default: {");
                        self.indent += 1;
                        self.line(&format!("const {name} = {m};"));
                        self.emit_arm_body(&arm.body, term, true)?;
                        self.indent -= 1;
                        self.line("}");
                    }
                }
                // A value-match literal: `case <literal>:`.
                Pattern::Literal { value, .. } => {
                    self.line(&format!("case {}: {{", literal_label(value)));
                    self.indent += 1;
                    self.emit_arm_body(&arm.body, term, true)?;
                    self.indent -= 1;
                    self.line("}");
                }
                Pattern::Wildcard { .. } | Pattern::Else { .. } => {
                    self.line("default: {");
                    self.indent += 1;
                    self.emit_arm_body(&arm.body, term, true)?;
                    self.indent -= 1;
                    self.line("}");
                }
                _ => unreachable!("patterns were validated above"),
            }
        }
        // Without a catch-all arm, append an exhaustiveness assertion: it makes
        // every path return-or-throw (so a value-position arrow infers `T`, not
        // `T | undefined`, and `noImplicitReturns` is satisfied) regardless of
        // how precisely TS types the scrutinee. For a tagged union the
        // typechecker has proven exhaustiveness, so the throw is unreachable;
        // for a value match without an `else` it is the runtime fallback for an
        // unlisted value (value-match exhaustiveness is not yet checked).
        let has_catch_all = arms.iter().any(is_catch_all);
        if !has_catch_all {
            let err = self.g("Error");
            self.line(&format!("default: throw new {err}(\"non-exhaustive match\");"));
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// Lower a type-guard `match` (`is TypeName` arms) to an `if`/`else if`
    /// chain. Each `is T` becomes a runtime check: `typeof __m === "..."` for a
    /// primitive, `T.is(__m)` for a record type (the Q8 descriptor), an object
    /// check for `Record<...>`, `Array.isArray` for `Array<...>`. The chain is
    /// exclusive, so no `break` is needed; a missing `else` throws.
    fn emit_is_chain(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        term: ArmTerm,
    ) -> Result<(), EmitError> {
        // Two catch-all arms would silently drop the earlier one (the chain
        // keeps only the last `else`); reject, as the switch path does.
        if let Some(extra) = arms
            .iter()
            .filter(|a| matches!(a.pattern, Pattern::Wildcard { .. } | Pattern::Else { .. }))
            .nth(1)
        {
            return Err(EmitError::Unsupported {
                construct: "a match with more than one catch-all arm",
                span: extra.span,
            });
        }

        // When the scrutinee is a plain identifier, run the `is` checks on it
        // directly rather than binding a temporary: a `typeof x === "..."` /
        // `Array.isArray(x)` / `T.is(x)` check on the identifier narrows it for
        // TypeScript, so the arm bodies (which reference the identifier) see the
        // narrowed type. A non-identifier scrutinee is bound to a temporary to
        // evaluate it once; the arm bodies cannot name it, so narrowing it would
        // not help anyway.
        let m = match scrutinee {
            Expr::Ident { name, .. } => name.to_string(),
            _ => {
                let scrut = self.expr(scrutinee)?;
                let t = self.fresh_temp("__m");
                self.line(&format!("const {t} = {scrut};"));
                t
            }
        };

        let mut first = true;
        let mut else_arm: Option<&MatchArm> = None;
        for arm in arms {
            match &arm.pattern {
                Pattern::IsType { ty, span } => {
                    let check = self.is_check(ty, &m).ok_or(EmitError::Unsupported {
                        construct: "an `is` check on an unsupported type",
                        span: *span,
                    })?;
                    let opener = if first {
                        format!("if ({check}) {{")
                    } else {
                        format!("}} else if ({check}) {{")
                    };
                    first = false;
                    self.line(&opener);
                    self.indent += 1;
                    // No `break`: the if-chain is already exclusive.
                    self.emit_arm_body(&arm.body, term, false)?;
                    self.indent -= 1;
                }
                Pattern::Wildcard { .. } | Pattern::Else { .. } => else_arm = Some(arm),
                _ => {
                    return Err(EmitError::Unsupported {
                        construct: "a match mixing `is` and other patterns",
                        span: arm.span,
                    })
                }
            }
        }

        self.line("} else {");
        self.indent += 1;
        match else_arm {
            Some(arm) => self.emit_arm_body(&arm.body, term, false)?,
            None => {
                let err = self.g("Error");
                self.line(&format!("throw new {err}(\"non-exhaustive match\");"))
            }
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// Lower a `match` over an array scrutinee to an `if`/`else if` chain. Each
    /// `Pattern::Array` arm becomes a length check (`=== n` for a fixed-length
    /// pattern, `>= n` when a `...rest` element is present) plus an equality
    /// check for every literal element; identifier elements bind by index and a
    /// `...rest` binds `slice(n)`. The chain is exclusive — source order is
    /// match order — so no `break` is needed. A missing catch-all throws; the
    /// typechecker has proven array-length exhaustiveness, so for a well-typed
    /// match the throw is unreachable.
    fn emit_array_chain(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        term: ArmTerm,
    ) -> Result<(), EmitError> {
        // A second catch-all would drop the earlier one (the chain keeps only
        // the last `else`); reject, as the switch and `is`-chain paths do.
        if let Some(extra) = arms
            .iter()
            .filter(|a| matches!(a.pattern, Pattern::Wildcard { .. } | Pattern::Else { .. }))
            .nth(1)
        {
            return Err(EmitError::Unsupported {
                construct: "a match with more than one catch-all arm",
                span: extra.span,
            });
        }

        let scrut = self.expr(scrutinee)?;
        let m = self.fresh_temp("__m");
        self.line(&format!("const {m} = {scrut};"));

        let mut first = true;
        let mut else_arm: Option<&MatchArm> = None;
        for arm in arms {
            match &arm.pattern {
                Pattern::Array {
                    elements,
                    rest,
                    span,
                } => {
                    let cond = self.array_pattern_condition(&m, elements, rest, *span)?;
                    let opener = if first {
                        format!("if ({cond}) {{")
                    } else {
                        format!("}} else if ({cond}) {{")
                    };
                    first = false;
                    self.line(&opener);
                    self.indent += 1;
                    self.emit_array_binds(&m, elements, rest);
                    // No `break`: the if-chain is already exclusive.
                    self.emit_arm_body(&arm.body, term, false)?;
                    self.indent -= 1;
                }
                Pattern::Wildcard { .. } | Pattern::Else { .. } => else_arm = Some(arm),
                _ => {
                    return Err(EmitError::Unsupported {
                        construct: "a match mixing array and other patterns",
                        span: arm.span,
                    })
                }
            }
        }

        self.line("} else {");
        self.indent += 1;
        match else_arm {
            Some(arm) => self.emit_arm_body(&arm.body, term, false)?,
            None => {
                let err = self.g("Error");
                self.line(&format!("throw new {err}(\"non-exhaustive match\");"))
            }
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// Build the boolean guard for one array pattern: a length check joined with
    /// an equality check per literal element. Identifier and wildcard elements
    /// contribute no check (they bind, see `emit_array_binds`). A nested element
    /// pattern or a non-identifier rest is not supported yet.
    fn array_pattern_condition(
        &self,
        m: &str,
        elements: &[Pattern],
        rest: &Option<Box<Pattern>>,
        span: Span,
    ) -> Result<String, EmitError> {
        if let Some(r) = rest {
            if !matches!(r.as_ref(), Pattern::Ident { .. } | Pattern::Wildcard { .. }) {
                return Err(EmitError::Unsupported {
                    construct: "a non-identifier rest pattern in an array match",
                    span,
                });
            }
        }
        let n = elements.len();
        let len_check = if rest.is_some() {
            format!("{m}.length >= {n}")
        } else {
            format!("{m}.length === {n}")
        };
        let mut checks = vec![len_check];
        for (i, el) in elements.iter().enumerate() {
            match el {
                Pattern::Literal { value, .. } => {
                    checks.push(format!("{m}[{i}] === {}", literal_label(value)));
                }
                Pattern::Ident { .. } | Pattern::Wildcard { .. } => {}
                _ => {
                    return Err(EmitError::Unsupported {
                        construct: "a nested pattern inside an array match pattern",
                        span,
                    })
                }
            }
        }
        Ok(checks.join(" && "))
    }

    /// Bind the identifier elements and `...rest` of an array pattern from the
    /// scrutinee temporary `m`. Literal and wildcard elements bind nothing; a
    /// wildcard rest binds nothing. Element validity was checked while building
    /// the condition.
    fn emit_array_binds(&mut self, m: &str, elements: &[Pattern], rest: &Option<Box<Pattern>>) {
        for (i, el) in elements.iter().enumerate() {
            if let Pattern::Ident { name, .. } = el {
                self.line(&format!("const {name} = {m}[{i}];"));
            }
        }
        if let Some(r) = rest {
            if let Pattern::Ident { name, .. } = r.as_ref() {
                self.line(&format!("const {name} = {m}.slice({});", elements.len()));
            }
        }
    }

    /// The runtime check for an `is T` pattern against the temporary `m`, or
    /// None for a type the emitter cannot check yet (a union, a generic, an
    /// imported or non-record named type).
    fn is_check(&self, ty: &TypeExpr, m: &str) -> Option<String> {
        match ty {
            TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                if let Some(jt) = js_typeof(ty) {
                    Some(format!("typeof {m} === \"{jt}\""))
                } else if self.has_descriptor(segments[0].as_ref()) {
                    Some(format!("{}.is({m})", segments[0]))
                } else {
                    None
                }
            }
            TypeExpr::Generic { base, args, .. } => match base.as_ref() {
                TypeExpr::Path { segments, .. } => match segments.last().map(|s| s.as_ref()) {
                    // A Glyph record is a plain object, not an array; exclude
                    // arrays so an `is Array<...>` arm after `is Record<...>`
                    // isn't dead. Emit the check as a type-predicate IIFE so it
                    // narrows the scrutinee to the record type (indexable), not
                    // just to `{}` — a bare `typeof x === "object"` would leave
                    // `x[key]` an implicit-any index error.
                    Some("Record") => {
                        let rec = self.ty(ty).ok()?;
                        Some(format!(
                            "((__x: unknown): __x is {rec} => typeof __x === \"object\" && __x !== null && !{}.isArray(__x))({m})",
                            self.g("Array")
                        ))
                    }
                    // Element-check the array so `is Array<E>` is as sound as the
                    // descriptor's `Array<E>` check; fall back to a shallow
                    // `Array.isArray` when the element type has no checkable form.
                    Some("Array") => match args.as_slice() {
                        [elem] => match self.is_check(elem, "__e") {
                            Some(ec) => Some(format!(
                                "{}.isArray({m}) && ({m} as ReadonlyArray<unknown>).every((__e: unknown) => {ec})",
                                self.g("Array")
                            )),
                            None => Some(format!("{}.isArray({m})", self.g("Array"))),
                        },
                        _ => Some(format!("{}.isArray({m})", self.g("Array"))),
                    },
                    // `is Paginated<User>` on a generic descriptor: call its `is`
                    // with the type arguments (to narrow) and a synthesized checker
                    // per argument. The descriptor may be module-local or imported
                    // from another project module; `generic_descriptor_arity`
                    // resolves both (local-first, then the arity registry), so a
                    // cross-module `is` narrows the same way `parse` does rather
                    // than hard-erroring.
                    Some(gname) if self.generic_descriptor_arity(gname) > 0 => {
                        let mut targ_strs = Vec::with_capacity(args.len());
                        for a in args {
                            targ_strs.push(self.ty(a).ok()?);
                        }
                        let checkers = args
                            .iter()
                            .map(|a| self.checker_lambda(a))
                            .collect::<Vec<_>>()
                            .join(", ");
                        Some(format!(
                            "{gname}.is<{}>({m}, {checkers})",
                            targ_strs.join(", ")
                        ))
                    }
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    /// True if `name` resolves to a non-generic type with an emitted runtime
    /// descriptor whose `is`/`schema` members this check can call: a record, a
    /// tagged union whose descriptor `const` name is free, or a D39 refined
    /// primitive. Mirrors the emission guards in `emit_decl`/`emit_union`
    /// through the shared [`emits_plain_descriptor`] predicate.
    ///
    /// Resolves a module-local declaration first; on a miss, resolves `name`
    /// through its `ImportNamed` symbol and consults the project-wide descriptor
    /// registry, exactly the way `generic_descriptor_arity` resolves an imported
    /// generic descriptor. A module-local name shadows an import in `by_name`,
    /// so the registry lookup only fires for a genuinely imported type. Without
    /// the import half, a field typed by an imported record fell to the
    /// `!== undefined` presence floor while the type promised a full check.
    fn has_descriptor(&self, name: &str) -> bool {
        if let Some(local) = self.module.items.iter().find_map(|d| match d {
            Decl::Type(t) if t.name.as_ref() == name && t.generics.is_empty() => Some(t),
            _ => None,
        }) {
            return emits_plain_descriptor(local);
        }
        let Some(&sym_id) = self.resolved.symbols.by_name.get(name) else {
            return false;
        };
        let Some(sym) = self.resolved.symbols.table.get(sym_id) else {
            return false;
        };
        let SymbolKind::ImportNamed { path, original } = &sym.kind else {
            return false;
        };
        let module_path: String = path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        self.ctx
            .plain_descriptors
            .contains(&(module_path, original.to_string()))
    }

    /// True if the type named by the two-segment path `ns.name` (a namespace or
    /// aliased module import, `import types` then `types.Inner`) has an emitted
    /// descriptor in the module `ns` binds. The same registry lookup as
    /// [`Self::has_descriptor`]'s import half, reached through
    /// `namespace_module_path` rather than an `ImportNamed` symbol.
    fn has_namespaced_descriptor(&self, ns: &str, name: &str) -> bool {
        let Some(module_path) = self.namespace_module_path(ns) else {
            return false;
        };
        self.ctx
            .plain_descriptors
            .contains(&(module_path, name.to_string()))
    }

    /// The project module an imported *name* came from, or `None` for a
    /// module-local name. Mirrors the walk in `has_descriptor`.
    fn import_module_path(&self, name: &str) -> Option<String> {
        let &sym_id = self.resolved.symbols.by_name.get(name)?;
        let sym = self.resolved.symbols.table.get(sym_id)?;
        let SymbolKind::ImportNamed { path, .. } = &sym.kind else {
            return None;
        };
        Some(
            path.segments
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// Resolve a type imported from a sibling module to the leaf a field check
    /// can be built from, following alias hops *inside that module*. The local
    /// twin is `resolve_alias_leaf`; without this one a `"text" | "int"` union
    /// keeps its membership check at home and loses it the moment it is
    /// imported, so the boundary check is weaker than the type declares.
    /// Stops at a type that has its own descriptor (that path is already
    /// handled) and guards against a cycle.
    fn resolve_imported_alias_leaf(&self, module_path: &str, name: &str) -> Option<TypeExpr> {
        let mut current = name.to_string();
        let mut seen = std::collections::HashSet::new();
        loop {
            if !seen.insert(current.clone()) {
                return None; // cyclic alias
            }
            let body = self
                .ctx
                .descriptorless_aliases
                .get(&(module_path.to_string(), current.clone()))?;
            match body {
                // Another bare name in the same module: follow it, unless it
                // has a descriptor of its own.
                TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                    let next = segments[0].as_ref().to_string();
                    if self
                        .ctx
                        .plain_descriptors
                        .contains(&(module_path.to_string(), next.clone()))
                    {
                        return None;
                    }
                    if !self
                        .ctx
                        .descriptorless_aliases
                        .contains_key(&(module_path.to_string(), next.clone()))
                    {
                        // A prelude type (`int`, `string`) or a name this module
                        // does not export: the body is already the leaf.
                        return Some(body.clone());
                    }
                    current = next;
                }
                other => return Some(other.clone()),
            }
        }
    }

    /// Resolve a module-local *descriptorless* type alias to its leaf body, so a
    /// field typed by the alias validates like the inline type. Follows a chain
    /// of alias hops (`type A = B; type B = "x" | "y"`), stopping at any type
    /// that `has_descriptor` accepts (a record, a tagged union, or a D39 refined
    /// primitive — each resolves through its own descriptor instead) and at any
    /// type that is not a local alias at all (a prelude type, a literal union,
    /// etc.). Stopping at a refined alias is load-bearing: resolving through it
    /// to the base type would emit the base leaf-check and silently drop the
    /// `where` predicate. Returns `None` for a name that is not a local alias,
    /// and guards against a cycle. The returned leaf is never a followable
    /// alias, so `field_value_check` on it terminates.
    fn resolve_alias_leaf(&self, name: &str) -> Option<TypeExpr> {
        let alias_body = |n: &str| -> Option<TypeExpr> {
            self.module.items.iter().find_map(|d| match d {
                Decl::Type(t) if t.name.as_ref() == n && t.generics.is_empty() => {
                    Some(t.body.clone())
                }
                _ => None,
            })
        };
        let mut current = name.to_string();
        let mut seen = std::collections::HashSet::new();
        let mut body = alias_body(&current)?;
        loop {
            if !seen.insert(current.clone()) {
                return None; // cyclic alias
            }
            // Follow only a single-segment reference to another local alias that
            // has no descriptor of its own (a record short-circuits to `.is`).
            let next = match &body {
                TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                    let n = segments[0].as_ref();
                    (!self.has_descriptor(n) && n != current)
                        .then(|| n.to_string())
                        .filter(|n| alias_body(n).is_some())
                }
                _ => None,
            };
            match next {
                Some(n) => {
                    current = n.clone();
                    body = alias_body(&n)?;
                }
                None => return Some(body),
            }
        }
    }

    /// The guard variable (`__is_T`) for a type parameter `name` in scope while a
    /// generic descriptor's field checks are generated, or `None` when `name` is
    /// not one of the current descriptor's parameters.
    fn param_guard(&self, name: &str) -> Option<String> {
        self.desc_param_guards
            .borrow()
            .iter()
            .find(|(p, _)| p == name)
            .map(|(_, g)| g.clone())
    }

    /// A runtime checker `(v) => boolean` validating a value against `ty`, for
    /// passing as a generic descriptor's per-parameter checker argument. Reuses
    /// `field_value_check`, so a concrete type argument (`User`, `number`,
    /// `Array<User>`, a nested generic) is validated as deeply as a field of that
    /// type would be; an opaque argument falls back to the presence floor.
    fn checker_lambda(&self, ty: &TypeExpr) -> String {
        format!("(__cv: unknown) => {}", self.field_value_check(ty, "__cv"))
    }

    /// The guard-parameter name a generic descriptor threads for type parameter
    /// `param` (`T` -> `__is_T`). Single-sourced so emission and call sites agree.
    fn guard_param_name(param: &str) -> String {
        format!("__is_{param}")
    }

    /// One field's runtime check inside a record descriptor's `is` predicate. A
    /// required field must match its type (a missing field reads as `undefined`,
    /// which every check below rejects); an optional field also passes when it is
    /// absent.
    fn record_field_check(&self, field: &RecordTypeField) -> String {
        let access = format!("(value as Record<string, unknown>).{}", field.name);
        let check = self.field_value_check(&field.ty, &access);
        if field.optional {
            // Same rule as `emit_field_parse_check`: null is absence.
            let present = format!("{access} !== undefined && {access} !== null");
            format!("(!({present}) || {check})")
        } else {
            check
        }
    }

    /// The payload check for a tagged-union variant whose tag has already
    /// matched (used in the union descriptor's `is`): a record payload's fields
    /// (spread onto `value`) are checked like a record's, a single-value
    /// payload's `value` field is checked against its type, and a no-payload
    /// variant passes (`true`).
    fn union_variant_check(&self, v: &UnionVariant) -> String {
        match &v.payload {
            None => "true".to_string(),
            Some(TypeExpr::Record { fields, .. }) => {
                if fields.is_empty() {
                    "true".to_string()
                } else {
                    fields
                        .iter()
                        .map(|f| self.record_field_check(f))
                        .collect::<Vec<_>>()
                        .join(" && ")
                }
            }
            Some(ty) => self.field_value_check(ty, "(value as Record<string, unknown>).value"),
        }
    }

    /// A boolean expression validating that the unknown value `access` matches
    /// `ty` (G4 — the descriptor recurses rather than checking only one level):
    /// - a primitive by `typeof`;
    /// - a named type with a descriptor by `T.is(...)` (recurses into nested
    ///   records and unions);
    /// - `Array<E>` by `Array.isArray` plus an element check on every item;
    /// - `Option<E>` by its `{ tag: "None" } | { tag: "Some", value: E }` shape,
    ///   validating the payload of a `Some`;
    /// - a function-typed field by `typeof === "function"`;
    /// - anything else (an imported type, a bare generic parameter) conservatively
    ///   by "not undefined", the remaining shallow cases.
    fn field_value_check(&self, ty: &TypeExpr, access: &str) -> String {
        match self.field_check(ty, access) {
            FieldCheck::Deep(c) => c,
            FieldCheck::PresenceOnly | FieldCheck::Unverifiable => {
                format!("{access} !== undefined")
            }
        }
    }

    /// The deep predicate for `ty`, or `None` for the two cases that have none.
    /// Used by the recursive arms, where an `unknown` element and an
    /// unverifiable one are equally unusable as a sub-check.
    fn field_value_check_opt(&self, ty: &TypeExpr, access: &str) -> Option<String> {
        match self.field_check(ty, access) {
            FieldCheck::Deep(c) => Some(c),
            _ => None,
        }
    }

    /// The real check for a field of type `ty`, or `None` when there is nothing
    /// to check it with.
    ///
    /// `None` is the honest answer for a type the emitter cannot see into: a
    /// host handle, an `extern_ts` type, a bare `unknown`, a generic tagged
    /// union (only generic *records* carry a descriptor). It propagates, so an
    /// `Array<Socket>` is unverifiable even though its array-ness is checkable:
    /// what a descriptor claims is the field's declared type, and checking half
    /// of it while reporting all of it is the thing worth refusing.
    ///
    /// `field_value_check` substitutes the old presence floor so the emitted
    /// code is unchanged for callers that only want a boolean. What changed is
    /// that the caller can now *ask* whether the boolean means anything, which
    /// is what E0304 is built on. Deciding it here rather than in a parallel
    /// predicate is deliberate: an error that disagreed with the emitted check
    /// would either refuse a checkable type or let a lie through.
    fn field_check(&self, ty: &TypeExpr, access: &str) -> FieldCheck {
        // `unknown` is satisfied by every value, so a required field of it is
        // fully checked by being there. Emitting a type branch as well would be
        // a branch that can never fire, which is what this whole change is about.
        if is_named_type(ty, "unknown") {
            return FieldCheck::PresenceOnly;
        }
        // `int` is a whole `number`: the leaf check that `number` cannot express.
        if is_named_type(ty, "int") {
            return FieldCheck::Deep(format!(
                "(typeof {access} === \"number\" && {}.isInteger({access}))",
                self.g("Number")
            ));
        }
        if let Some(jt) = js_typeof(ty) {
            return FieldCheck::Deep(format!("typeof {access} === \"{jt}\""));
        }
        match ty {
            TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                let name = segments[0].as_ref();
                if let Some(guard) = self.param_guard(name) {
                    // A field typed as one of the enclosing generic descriptor's
                    // parameters: validate it with the checker threaded in at the
                    // call site, not a presence check.
                    FieldCheck::Deep(format!("{guard}({access})"))
                } else if self.has_descriptor(name) {
                    FieldCheck::Deep(format!("{name}.is({access})"))
                } else if let Some(leaf) = self.resolve_alias_leaf(name) {
                    // A non-record type alias (`type Tier = "free" | "pro"`,
                    // `type Count = int`): resolve to its leaf so a field typed by
                    // the alias gets the same runtime check as the inline type
                    // (membership, isInteger, …), not a bare presence check.
                    self.field_check(&leaf, access)
                } else if let Some(leaf) = self
                    .import_module_path(name)
                    .and_then(|m| self.resolve_imported_alias_leaf(&m, name))
                {
                    // The same alias, imported from a sibling module. Without
                    // this the D30 membership check survives at home and
                    // evaporates across the import.
                    self.field_check(&leaf, access)
                } else {
                    FieldCheck::Unverifiable
                }
            }
            // A namespaced reference to another project module's type
            // (`import types` then a field typed `types.Inner`): the descriptor
            // is reachable as `types.Inner`, the same binding the emitted type
            // annotation uses, so the check is as deep as the named-import form.
            TypeExpr::Path { segments, .. } if segments.len() == 2 => {
                let ns = segments[0].as_ref();
                let name = segments[1].as_ref();
                if self.has_namespaced_descriptor(ns, name) {
                    FieldCheck::Deep(format!("{ns}.{name}.is({access})"))
                } else if let Some(leaf) = self
                    .namespace_module_path(ns)
                    .and_then(|m| self.resolve_imported_alias_leaf(&m, name))
                {
                    // `import catalog` then a field typed `catalog.ColType`:
                    // the same descriptorless alias reached through a namespace.
                    self.field_check(&leaf, access)
                } else {
                    FieldCheck::Unverifiable
                }
            }
            TypeExpr::Generic { base, args, .. } => {
                let base_name = match base.as_ref() {
                    TypeExpr::Path { segments, .. } => segments.last().map(|s| s.as_ref()),
                    _ => None,
                };
                match (base_name, args.as_slice()) {
                    (Some("Array"), [elem]) => {
                        let Some(elem_check) = self.field_value_check_opt(elem, "__e") else {
                            return FieldCheck::Unverifiable;
                        };
                        FieldCheck::Deep(format!(
                            "{}.isArray({access}) && ({access} as ReadonlyArray<unknown>).every((__e: unknown) => {elem_check})",
                            self.g("Array")
                        ))
                    }
                    (Some("Option"), [inner]) => {
                        let tag = format!("(({access}) as {{ tag?: unknown }}).tag");
                        let value = format!("(({access}) as {{ value?: unknown }}).value");
                        let Some(inner_check) = self.field_value_check_opt(inner, &value) else {
                            return FieldCheck::Unverifiable;
                        };
                        FieldCheck::Deep(format!(
                            "(typeof {access} === \"object\" && {access} !== null && ({tag} === \"None\" || ({tag} === \"Some\" && {inner_check})))"
                        ))
                    }
                    // `Record<K, V>` is a structural object map: reject non-objects
                    // (a string, an array, null) and recurse the value type over
                    // every entry, so a `Record<string, number>` field can never
                    // bind to a string or an object whose values are not numbers.
                    (Some("Record"), [_key, value]) => {
                        let Some(value_check) = self.field_value_check_opt(value, "__v") else {
                            return FieldCheck::Unverifiable;
                        };
                        FieldCheck::Deep(format!(
                            "(typeof {access} === \"object\" && {access} !== null && !{arr}.isArray({access}) && {obj}.values({access} as Record<string, unknown>).every((__v: unknown) => {value_check}))",
                            arr = self.g("Array"),
                            obj = self.g("Object")
                        ))
                    }
                    // A field typed as a generic record (`Paginated<User>`): call
                    // its descriptor's `is` with a synthesized checker per type
                    // argument. Type arguments are omitted here because the call is
                    // used only as a boolean. The descriptor may be module-local or
                    // imported; `generic_descriptor_arity` resolves both, so a
                    // nested cross-module argument (`Box.parse<Box<User>>`)
                    // validates deeply instead of falling to the presence floor.
                    (Some(gname), _) if self.generic_descriptor_arity(gname) > 0 => {
                        let checkers = args
                            .iter()
                            .map(|a| self.checker_lambda(a))
                            .collect::<Vec<_>>()
                            .join(", ");
                        FieldCheck::Deep(format!("{gname}.is({access}, {checkers})"))
                    }
                    _ => FieldCheck::Unverifiable,
                }
            }
            // A function-typed field (`run: fn(x: number) -> number`) is
            // validated by `typeof === "function"`. Arity and parameter types
            // are unobservable at runtime, but function-ness is the sound floor
            // and strictly better than the old presence-only check (a `run: 5`
            // would have passed).
            TypeExpr::Fn { .. } => FieldCheck::Deep(format!("typeof {access} === \"function\"")),
            // An inline record type (`{ a: number, b: T }`) as a field's type:
            // validate it is a non-null object and recurse into each field, so
            // the descriptor's recursion is not silently shallow here.
            TypeExpr::Record { fields, .. } => {
                let mut checks = vec![
                    format!("typeof {access} === \"object\""),
                    format!("{access} !== null"),
                ];
                for f in fields {
                    let sub = format!("({access} as Record<string, unknown>).{}", f.name);
                    let Some(c) = self.field_value_check_opt(&f.ty, &sub) else {
                        return FieldCheck::Unverifiable;
                    };
                    if f.optional {
                        let present = format!("\"{}\" in ({access} as object)", f.name);
                        checks.push(format!("(!({present}) || {c})"));
                    } else {
                        checks.push(c);
                    }
                }
                FieldCheck::Deep(format!("({})", checks.join(" && ")))
            }
            // A string-literal union field validates by membership: the value
            // must be one of the declared literals, not merely a string. This is
            // the leaf-value check that a bare `string` cannot express.
            TypeExpr::StringLiteralUnion { values, .. } => {
                if values.is_empty() {
                    return FieldCheck::Deep(format!("typeof {access} === \"string\""));
                }
                let arms: Vec<String> = values
                    .iter()
                    .map(|v| format!("{access} === {}", escape_double_quoted(v)))
                    .collect();
                FieldCheck::Deep(format!("({})", arms.join(" || ")))
            }
            _ => FieldCheck::Unverifiable,
        }
    }

    /// Bind a constructor arm's payload from the scrutinee temporary `m`: an
    /// object pattern reads each spread field by name; a single identifier binds
    /// the whole scrutinee object when the variant's payload is a record (its
    /// fields are spread flat as `{ tag, ...fields }`, so the object itself
    /// carries them), or reads the `value` field for a single-value payload
    /// (`Ok(x)`, `Some(x)`); no args and a `_` wildcard (`Err(_)`) bind nothing.
    fn emit_arm_binds(&mut self, m: &str, args: &[Pattern], record_payload: bool) {
        match args {
            [Pattern::Ident { name, .. }] => {
                if record_payload {
                    self.line(&format!("const {name} = {m};"));
                } else {
                    self.line(&format!("const {name} = {m}.{PAYLOAD};"));
                }
            }
            [Pattern::Object { fields, .. }] => {
                for f in fields {
                    let binding = f.binding.as_ref().unwrap_or(&f.key);
                    self.line(&format!("const {binding} = {m}.{};", f.key));
                }
            }
            _ => {}
        }
    }

    /// Emit the statements of a value block (a function body, or a block match
    /// arm). Every statement but the last emits plainly; the last sits in tail
    /// position, handled by `emit_tail_stmt` per `term`:
    /// - `Return`: the block's value is its final expression (an implicit
    ///   return, like Rust) — a tail bare expression becomes `return expr`, a
    ///   tail `match` lowers in return position, a tail `E?` returns its `Ok`
    ///   payload.
    /// - `Break`: the block runs for effect; a non-diverging tail gets a
    ///   trailing `break;` when `break_on_fall` (inside a `switch` case).
    fn emit_value_block_stmts(
        &mut self,
        stmts: &[Stmt],
        term: ArmTerm,
        break_on_fall: bool,
    ) -> Result<(), EmitError> {
        // 0.1.16 `defer`: the first defer wraps the statements after it in
        // `try { ... } finally { <deferred>; }`. The wrapped rest keeps this
        // block's terminal context (`term`), so a tail `return`/value inside the
        // `try` still produces the block's value and the deferred runs after it
        // on every path. Nested defers recurse for last-in-first-out order.
        if let Some(i) = stmts.iter().position(|s| matches!(s, Stmt::Defer(_))) {
            for s in &stmts[..i] {
                self.emit_stmt(s)?;
            }
            let Stmt::Defer(d) = &stmts[i] else { unreachable!() };
            self.line("try {");
            self.indent += 1;
            self.emit_value_block_stmts(&stmts[i + 1..], term, break_on_fall)?;
            self.indent -= 1;
            self.line("} finally {");
            self.indent += 1;
            let v = self.emit_value(&d.expr)?;
            self.line(&format!("{v};"));
            self.indent -= 1;
            self.line("}");
            return Ok(());
        }
        let Some((last, init)) = stmts.split_last() else {
            // An empty block yields nothing, so it emits no terminating statement
            // of its own. Inside a `switch` case (`break_on_fall`) that means the
            // case would fall through into the next one, so it needs an explicit
            // `break` regardless of position: in statement position it exits the
            // switch after running for effect, and in return position the void
            // arm has no value to `return`, so it breaks out of the switch and
            // the function falls off its end, yielding `void`.
            if break_on_fall {
                self.line("break;");
            }
            return Ok(());
        };
        for stmt in init {
            self.emit_stmt(stmt)?;
        }
        self.emit_tail_stmt(last, term, break_on_fall)
    }

    /// Emit the final statement of a value block in tail position. See
    /// `emit_value_block_stmts` for the `term` contract.
    fn emit_tail_stmt(
        &mut self,
        stmt: &Stmt,
        term: ArmTerm,
        break_on_fall: bool,
    ) -> Result<(), EmitError> {
        match stmt {
            // A tail `match` inherits the position: its arms `return` the value
            // in return position or run for effect in statement position. It
            // breaks its own arms; a statement-position nested switch still
            // needs the outer break after it.
            Stmt::Expr(Expr::Match { scrutinee, arms, .. }) => {
                self.emit_match_dispatch(scrutinee, arms, term)?;
                // Regardless of `term`: the nested switch breaks its own arms,
                // so control reaches here whenever any inner arm produced no
                // value, and without a break it runs on into the outer switch's
                // next case or its `default: throw`. When every inner arm does
                // diverge this break is unreachable, which is valid.
                if break_on_fall {
                    self.line("break;");
                }
            }
            // A tail `E?`: propagate an `Err`; in value position the block's
            // value is the unwrapped `Ok` payload.
            Stmt::Expr(Expr::Postfix {
                op: PostfixOp::Try,
                operand,
                ..
            }) => {
                let r = self.emit_try_unwrap(operand)?;
                match term {
                    ArmTerm::Return => self.line(&format!("return {r}.{PAYLOAD};")),
                    ArmTerm::Break => {
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                    ArmTerm::Assign => {
                        let t = self.assign_target.borrow().clone().unwrap_or_default();
                        self.line(&format!("{t} = {r}.{PAYLOAD};"));
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                }
            }
            // A tail bare expression is the block's value.
            Stmt::Expr(e) => {
                let v = self.emit_value(e)?;
                match term {
                    ArmTerm::Return => self.emit_return(&v),
                    ArmTerm::Break => {
                        self.line(&format!("{v};"));
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                    ArmTerm::Assign => {
                        let t = self.assign_target.borrow().clone().unwrap_or_default();
                        let v = pin_empty_array(v, e);
                        self.line(&format!("{t} = {v};"));
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                }
            }
            // A tail that already exits the function or loop emits unchanged; no
            // break is reachable after it.
            Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_) => self.emit_stmt(stmt)?,
            // Any other tail (let/mut/for/loop) yields no value; emit it and, in
            // a `switch` case, break afterward.
            //
            // The break does not depend on `term`, for the same reason the empty
            // block above breaks regardless of position: this tail produces no
            // value, so return position emits no `return` either, and without a
            // `break` the case runs on into whatever follows it — the next case,
            // or the generated `default: throw new Error("non-exhaustive
            // match")`. That threw at run time on a match that was exhaustive,
            // in code that compiled clean and passed `tsc --strict`. In return
            // position the arm instead breaks out of the switch and the function
            // falls off its end, yielding `void`, which is what an arm with no
            // value means.
            other => {
                self.emit_stmt(other)?;
                if break_on_fall {
                    self.line("break;");
                }
            }
        }
        Ok(())
    }

    /// Emit a match-arm body. `break_on_fall` adds a `break;` after a
    /// fall-through (statement-position) arm — needed inside a `switch` case,
    /// but not in the exclusive `if`/`else if` chain of an `is`-match.
    fn emit_arm_body(
        &mut self,
        body: &MatchArmBody,
        term: ArmTerm,
        break_on_fall: bool,
    ) -> Result<(), EmitError> {
        match body {
            // A nested `match` that is the whole arm body sits in tail position:
            // it inherits the arm's termination (Return stays Return so its arms
            // `return` the value; Break stays Break) and lowers as a statement
            // switch, not a value IIFE. This is what lets the inner arms use
            // block bodies or `return` (e.g. example 04's `Ok(cmd) => match
            // await run(cmd) { Ok(_) => return 0, Err(m) => { ...; return 1 } }`),
            // which the IIFE path rejects. A `match` used as a sub-expression (an
            // argument, an operand) is not an arm body and still routes through
            // `expr`'s value IIFE.
            MatchArmBody::Expr(Expr::Match { scrutinee, arms, .. }) => {
                self.emit_match_dispatch(scrutinee, arms, term)?;
                // Inside a `switch` case, break the OUTER switch after the nested
                // one: the nested arms only `break` themselves. When the nested
                // match diverges (every arm returns/throws) this break is
                // unreachable but valid.
                //
                // Not conditional on `term`. In return position an inner arm
                // that produces no value emits neither a `return` nor anything
                // else, so control arrives here and would otherwise fall into
                // the outer `default: throw new Error("non-exhaustive match")`.
                if break_on_fall {
                    self.line("break;");
                }
            }
            MatchArmBody::Expr(e) => {
                // `emit_value`, not `expr`: an arm body is a statement value
                // like a `let`/`return` value, so a `?` in it hoists to an
                // unwrap emitted just above the arm's value line, inside the
                // `case { ... }` block. `expr` would reject the `?` outright.
                let s = self.emit_value(e)?;
                match term {
                    ArmTerm::Return => self.line(&format!("return {s};")),
                    ArmTerm::Break => {
                        self.line(&format!("{s};"));
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                    ArmTerm::Assign => {
                        let t = self.assign_target.borrow().clone().unwrap_or_default();
                        let s = pin_empty_array(s, e);
                        self.line(&format!("{t} = {s};"));
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                }
            }
            // A block arm emits its statements into the case/branch as a value
            // block: in return position its final expression is the matched
            // value (implicit return); in statement position it runs for effect
            // and, inside a `switch`, breaks afterward. Block arms are rejected
            // in value position (the IIFE) by the caller, since a block `return`
            // there means function-return.
            MatchArmBody::Block(b) => self.emit_value_block_stmts(&b.stmts, term, break_on_fall)?,
        }
        Ok(())
    }

    // ----- expressions -----

    /// The emitted call suffix: optional `<T, ...>` type arguments followed by
    /// the `(arg, ...)` list. Shared by plain call emission and the await-spine
    /// walk so both render a call the same way.
    fn call_suffix(&self, type_args: &[TypeExpr], args: &[Expr]) -> Result<String, EmitError> {
        let targs = if type_args.is_empty() {
            String::new()
        } else {
            let mut ts = Vec::with_capacity(type_args.len());
            for t in type_args {
                ts.push(self.ty(t)?);
            }
            format!("<{}>", ts.join(", "))
        };
        let mut a = Vec::with_capacity(args.len());
        for arg in args {
            a.push(self.expr(arg)?);
        }
        Ok(format!("{targs}({})", a.join(", ")))
    }

    /// The first field of record type `name` that has no runtime check, if any.
    ///
    /// Drives E0304. A record whose field is itself such a record is
    /// unverifiable too, which `field_value_check_opt` already gives for free:
    /// a field typed by a descriptor-bearing record resolves to `T.is(...)`,
    /// and that call is only as good as `T`'s own descriptor, so the walk
    /// recurses into it here. `seen` stops a recursive type from spinning.
    fn first_unverifiable_field(
        &self,
        name: &str,
        seen: &mut std::collections::HashSet<String>,
    ) -> Option<(String, String)> {
        if !seen.insert(name.to_string()) {
            return None; // recursive type: its own fields are checked once
        }
        let decl = self.module.items.iter().find_map(|d| match d {
            Decl::Type(td) if td.name.as_ref() == name && td.generics.is_empty() => Some(td),
            _ => None,
        })?;
        let TypeExpr::Record { fields, .. } = &decl.body else {
            return None;
        };
        for f in fields {
            // `PresenceOnly` is not an error: an `unknown` field claims
            // nothing, so being there is the whole of what the descriptor said.
            if self.field_check(&f.ty, "__x") == FieldCheck::Unverifiable {
                return Some((f.name.to_string(), type_label(&f.ty)));
            }
            // A field typed by another local record is checked by that record's
            // `is`, so the claim is only as strong as its descriptor.
            if let TypeExpr::Path { segments, .. } = &f.ty {
                if segments.len() == 1 {
                    let inner = segments[0].as_ref();
                    if self.has_descriptor(inner) {
                        if let Some((sub, ty)) = self.first_unverifiable_field(inner, seen) {
                            return Some((format!("{}.{sub}", f.name), ty));
                        }
                    }
                }
            }
        }
        None
    }

    /// Refuse `T.parse`/`T.is` when `T` cannot be validated (E0304). Returns the
    /// error to raise, or `None` when the call is fine or is not one of these.
    fn unverifiable_descriptor_use(&self, callee: &Expr) -> Option<EmitError> {
        let Expr::Member { object, field, optional: false, span } = callee else {
            return None;
        };
        if field.as_ref() != "parse" && field.as_ref() != "is" {
            return None;
        }
        let Expr::Ident { name, .. } = object.as_ref() else {
            return None;
        };
        if !self.has_descriptor(name.as_ref()) {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        let (field_name, field_ty) = self.first_unverifiable_field(name.as_ref(), &mut seen)?;
        Some(EmitError::UnverifiableDescriptorUse {
            type_name: name.to_string(),
            field: field_name,
            field_ty,
            span: *span,
        })
    }

    /// Rewrite `json.parse<T>(text)` to the validating `json.parse_with(text,
    /// T.schema)` when `T` is a local type with a runtime descriptor (G3). The
    /// plain `json.parse<T>` casts the decoded JSON to `T` without checking it;
    /// routing through the descriptor validates the shape instead. Returns `None`
    /// (so the caller emits the call normally) for any non-matching call —
    /// including a type argument with no descriptor, where the cast escape hatch
    /// is the intended behavior.
    fn try_json_parse_validating(
        &self,
        callee: &Expr,
        type_args: &[TypeExpr],
        args: &[Expr],
    ) -> Result<Option<String>, EmitError> {
        let Expr::Member { object, field, optional: false, .. } = callee else {
            return Ok(None);
        };
        if field.as_ref() != "parse" {
            return Ok(None);
        }
        let Expr::Ident { name, .. } = object.as_ref() else {
            return Ok(None);
        };
        if !self.is_json_namespace(name) {
            return Ok(None);
        }
        let ([type_arg], [arg]) = (type_args, args) else {
            return Ok(None);
        };
        let Some(schema) = self.schema_expr_for(type_arg) else {
            return Ok(None);
        };
        let obj = self.expr(object)?;
        let arg_str = self.expr(arg)?;
        Ok(Some(format!("{obj}.parse_with({arg_str}, {schema})")))
    }

    /// A `Schema<T>` expression for `json.parse<T>`'s type argument, when one can
    /// be derived: a record/union type's descriptor `T.schema`, or `Array<T>`
    /// (and nested arrays) via the schema factory's `.array()` combinator.
    /// `None` for a type with no descriptor (a primitive, an imported or generic
    /// type), where the casting `parse<T>` is kept as the escape hatch.
    fn schema_expr_for(&self, ty: &TypeExpr) -> Option<String> {
        match ty {
            TypeExpr::Path { segments, .. } => {
                let [name] = segments.as_slice() else {
                    return None;
                };
                self.has_descriptor(name.as_ref())
                    .then(|| format!("{name}.schema"))
            }
            TypeExpr::Generic { base, args, .. } => {
                let TypeExpr::Path { segments, .. } = base.as_ref() else {
                    return None;
                };
                if segments.last().map(|s| s.as_ref()) != Some("Array") {
                    return None;
                }
                let [elem] = args.as_slice() else {
                    return None;
                };
                Some(format!("{}.array()", self.schema_expr_for(elem)?))
            }
            _ => None,
        }
    }

    /// Rewrite `Paginated.parse<User>(v)` on a generic descriptor into
    /// `Paginated.parse<User>(v, <checker for User>)`, appending one synthesized
    /// checker per type argument (its `parse`/`is` need a runtime checker per type
    /// parameter). The receiver may be a bare name (`Box`, module-local or a named
    /// import) or a qualified namespace access (`bm.Box`, where `bm` is a
    /// namespace/aliased module import); both resolve their arity so neither drops
    /// the checker argument. Returns `None` for any non-matching call — a
    /// non-descriptor receiver, a wrong arity, or missing type arguments — where
    /// the call is emitted verbatim.
    fn try_generic_descriptor_parse(
        &self,
        callee: &Expr,
        type_args: &[TypeExpr],
        args: &[Expr],
    ) -> Result<Option<String>, EmitError> {
        let Expr::Member { object, field, optional: false, .. } = callee else {
            return Ok(None);
        };
        if field.as_ref() != "parse" {
            return Ok(None);
        }
        // Resolve the descriptor receiver and its arity. A bare `Box` resolves
        // local-first then through the import registry; a qualified `bm.Box`
        // resolves `bm` to its module path and looks that type up in the registry
        // directly (a namespace member is never a module-local declaration).
        let (receiver, arity) = match object.as_ref() {
            Expr::Ident { name, .. } => {
                (name.to_string(), self.generic_descriptor_arity(name.as_ref()))
            }
            Expr::Member { object: inner, field: type_name, optional: false, .. } => {
                let Expr::Ident { name: ns, .. } = inner.as_ref() else {
                    return Ok(None);
                };
                let Some(module_path) = self.namespace_module_path(ns.as_ref()) else {
                    return Ok(None);
                };
                let arity = self
                    .ctx
                    .generic_descriptor_arities
                    .get(&(module_path, type_name.to_string()))
                    .copied()
                    .unwrap_or(0);
                (format!("{ns}.{type_name}"), arity)
            }
            _ => return Ok(None),
        };
        // The type arguments must be given explicitly and match the arity; the
        // checker for each is synthesized from it. Anything else is not a call we
        // can complete soundly, so leave it for `tsc` to judge.
        if arity == 0 || type_args.len() != arity {
            return Ok(None);
        }
        let mut targ_strs = Vec::with_capacity(type_args.len());
        for ta in type_args {
            targ_strs.push(self.ty(ta)?);
        }
        let targs = format!("<{}>", targ_strs.join(", "));
        let mut call_args = Vec::with_capacity(args.len() + type_args.len());
        for arg in args {
            call_args.push(self.expr(arg)?);
        }
        for ta in type_args {
            call_args.push(self.checker_lambda(ta));
        }
        Ok(Some(format!("{receiver}.parse{targs}({})", call_args.join(", "))))
    }

    /// The imported module path for a namespace/aliased import binding
    /// (`import boxmod` -> `boxmod` under `boxmod`, `import a/b as bm` -> `a/b`
    /// under `bm`), or `None` when `binding` is not such an import. Used to
    /// resolve a qualified descriptor receiver (`bm.Box`) to its declaring module
    /// so its arity can be looked up in the project registry.
    fn namespace_module_path(&self, binding: &str) -> Option<String> {
        self.module.items.iter().find_map(|d| {
            let Decl::Import(im) = d else { return None };
            let matches = match &im.kind {
                ImportKind::Namespace => {
                    im.path.segments.last().map(|s| s.as_ref()) == Some(binding)
                }
                ImportKind::Aliased(alias) => alias.as_ref() == binding,
                // A default binding is a value, not a namespace: `app.Box` is a
                // member read on it, never a module-qualified type.
                ImportKind::Named(_) | ImportKind::Default(_) => false,
            };
            if !matches {
                return None;
            }
            Some(
                im.path
                    .segments
                    .iter()
                    .map(|s| s.as_ref())
                    .collect::<Vec<_>>()
                    .join("/"),
            )
        })
    }

    /// The number of type parameters of a generic record descriptor `name`, or
    /// `0` when `name` is not one. Resolves a module-local descriptor first; on a
    /// miss, resolves `name` through its `ImportNamed` symbol and consults the
    /// project-wide arity registry, so an *imported* generic descriptor's
    /// `Imported.parse<T>(v)` call threads its checker argument.
    fn generic_descriptor_arity(&self, name: &str) -> usize {
        if let Some(n) = self.module.items.iter().find_map(|d| match d {
            Decl::Type(t)
                if t.name.as_ref() == name
                    && !t.generics.is_empty()
                    && matches!(&t.body, TypeExpr::Record { .. }) =>
            {
                Some(t.generics.len())
            }
            _ => None,
        }) {
            return n;
        }
        // Imported generic descriptor: resolve via its own module's `ImportNamed`
        // symbol and consult the project registry. A same-module type name shadows
        // an import in `by_name`, so this only fires for a genuinely imported one.
        let Some(&sym_id) = self.resolved.symbols.by_name.get(name) else {
            return 0;
        };
        let Some(sym) = self.resolved.symbols.table.get(sym_id) else {
            return 0;
        };
        let SymbolKind::ImportNamed { path, original } = &sym.kind else {
            return 0;
        };
        let module_path: String = path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        self.ctx
            .generic_descriptor_arities
            .get(&(module_path, original.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// The local binding names of every namespace/aliased import
    /// (`import std/http` -> `http`, `import x as h` -> `h`). Used to tell a
    /// namespaced function call (`http.get(...)`) from a value method call
    /// (`cursor.to_array(...)`) when placing an `await`.
    fn namespace_bindings(&self) -> Vec<&str> {
        self.module
            .items
            .iter()
            .filter_map(|d| {
                let Decl::Import(im) = d else { return None };
                match &im.kind {
                    ImportKind::Namespace => im.path.segments.last().map(|s| s.as_ref()),
                    ImportKind::Aliased(alias) => Some(alias.as_ref()),
                    ImportKind::Named(_) | ImportKind::Default(_) => None,
                }
            })
            .collect()
    }

    /// Whether `name` is the local binding of a `std/json` namespace import
    /// (`import std/json` -> `json`, or `import std/json as j` -> `j`), so a
    /// `<name>.parse<T>` call is the stdlib JSON parse and not a user method.
    fn is_json_namespace(&self, name: &str) -> bool {
        self.module.items.iter().any(|d| {
            let Decl::Import(im) = d else { return false };
            let path: Vec<&str> = im.path.segments.iter().map(|s| s.as_ref()).collect();
            if path != ["std", "json"] {
                return false;
            }
            match &im.kind {
                ImportKind::Namespace => {
                    im.path.segments.last().map(|s| s.as_ref()) == Some(name)
                }
                ImportKind::Aliased(alias) => alias.as_ref() == name,
                ImportKind::Named(_) | ImportKind::Default(_) => false,
            }
        })
    }

    /// Emit the operand of an `await`, inserting the `await` at the async call
    /// that heads the receiver spine rather than around the whole chain. Returns
    /// the emitted string and whether an `await` was inserted.
    ///
    /// Glyph async is colorless (a call's type is its awaited type), so
    /// `await load(p).map_err(f)` parses with `await` wrapping the chain, but
    /// the async call is `load(p)`; the chained `.map_err` runs on the awaited
    /// `Result`. Walking the receiver spine (a call's callee, a member/index's
    /// object) to the innermost call and awaiting it there yields
    /// `(await load(p)).map_err(f)`. A spine with no call (e.g. `await x`) is
    /// reported as not-awaited so the caller wraps it directly.
    fn emit_await_spine(&self, e: &Expr) -> Result<(String, bool), EmitError> {
        match e {
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                let (callee_str, awaited) = self.emit_await_spine(callee)?;
                let call = format!("{callee_str}{}", self.call_suffix(type_args, args)?);
                // If a deeper call already took the `await`, leave it; otherwise
                // this call is the spine head — await it.
                if awaited {
                    Ok((call, true))
                } else {
                    Ok((format!("(await {call})"), true))
                }
            }
            Expr::Member {
                object,
                field,
                optional,
                ..
            } => {
                let (obj, awaited) = self.emit_await_spine(object)?;
                let dot = if *optional { "?." } else { "." };
                Ok((format!("{obj}{dot}{field}"), awaited))
            }
            Expr::Index { object, index, .. } => {
                let (obj, awaited) = self.emit_await_spine(object)?;
                Ok((
                    format!("__glyph_index({obj}, {})", self.expr(index)?),
                    awaited,
                ))
            }
            // Spine bottom (an identifier, a literal, a parenthesized
            // expression): no call here, so no `await` is inserted.
            _ => Ok((self.expr(e)?, false)),
        }
    }

    fn expr(&self, e: &Expr) -> Result<String, EmitError> {
        Ok(match e {
            Expr::Number { raw, .. } => raw.clone(),
            Expr::String { value, .. } => escape_double_quoted(value),
            Expr::TemplateString { parts, .. } => self.template(parts)?,
            Expr::Bool { value, .. } => value.to_string(),
            Expr::Void { .. } => "undefined".to_string(),
            Expr::Ident { name, .. } => name.to_string(),
            Expr::Binary {
                op, left, right, ..
            } => {
                // `==` and `!=` are value equality (D42). `===` delivers that
                // for primitives and nothing else: on a record, a tagged union
                // or an array it compares references, so `Some("a") ==
                // Some("a")` was false with no diagnostic, while the identical
                // expression written as an `@example` compared structurally and
                // passed. A test that reports success on code that does not
                // work is the worst outcome the example gate can produce.
                //
                // `===` is still emitted whenever both sides are known
                // primitives, which is most comparisons, so the common case is
                // byte-identical to what it always was.
                if matches!(op, BinOp::Eq | BinOp::NotEq)
                    && !(self.is_primitive_operand(left) && self.is_primitive_operand(right))
                {
                    let l = self.expr(left)?;
                    let r = self.expr(right)?;
                    let bang = if matches!(op, BinOp::NotEq) { "!" } else { "" };
                    format!("({bang}__glyph_eq({l}, {r}))")
                } else if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    format!(
                        "({} {} {})",
                        self.compared_operand(left)?,
                        bin_op(*op),
                        self.compared_operand(right)?
                    )
                } else {
                    format!(
                        "({} {} {})",
                        self.expr(left)?,
                        bin_op(*op),
                        self.expr(right)?
                    )
                }
            }
            Expr::Unary { op, operand, .. } => {
                let op = match op {
                    UnaryOp::Not => "!",
                    UnaryOp::Neg => "-",
                    UnaryOp::BitNot => "~",
                };
                format!("({op}{})", self.expr(operand)?)
            }
            Expr::Postfix { op, operand, span } => match op {
                // `?` lowers to a hoisted unwrap: a `const` binding plus an
                // early `return` emitted BEFORE the statement it appears in.
                // `emit_value` does that hoisting, so every `?` a statement can
                // reach is already rewritten by the time it gets here. Landing
                // here means the `?` sits in a position rendered as a bare
                // expression, with no statement slot to hoist into (a `match`
                // scrutinee, for instance). Positional, not unimplemented.
                PostfixOp::Try => {
                    let _ = operand;
                    return Err(EmitError::TryInUnhoistablePosition { span: *span });
                }
            },
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                if let Some(err) = self.unverifiable_descriptor_use(callee) {
                    return Err(err);
                }
                if let Some(rewritten) = self.try_json_parse_validating(callee, type_args, args)? {
                    rewritten
                } else if let Some(rewritten) =
                    self.try_generic_descriptor_parse(callee, type_args, args)?
                {
                    rewritten
                } else {
                    format!("{}{}", self.expr(callee)?, self.call_suffix(type_args, args)?)
                }
            }
            // Interop constructor (D37): emits verbatim as a TS `new`. `tsc`
            // checks it against the imported constructor's signature.
            Expr::New {
                callee,
                type_args,
                args,
                ..
            } => {
                format!(
                    "new {}{}",
                    self.expr(callee)?,
                    self.call_suffix(type_args, args)?
                )
            }
            Expr::Member {
                object,
                field,
                optional,
                ..
            } => {
                let dot = if *optional { "?." } else { "." };
                format!("{}{dot}{field}", self.expr(object)?)
            }
            // A read goes through the bounds check (G30); an assignment target
            // does not, and is emitted by `lvalue` below.
            Expr::Index { object, index, .. } => {
                format!(
                    "__glyph_index({}, {})",
                    self.expr(object)?,
                    self.expr(index)?
                )
            }
            // Glyph async is colorless: a call's declared type is its awaited
            // type and `await` may syntactically wrap a whole method chain
            // (`await load(p).map_err(f)`). The emitted async function returns a
            // `Promise`, so the `await` must apply to the async call at the head
            // of the receiver spine, not the chain as a whole — otherwise the
            // chained method is called on a `Promise`. See `emit_await_spine`.
            Expr::Await { expr, .. } => {
                // A fluent chain (`cursor.find({}).to_array()`) awaits the whole
                // chain (JS semantics); the Result idiom (`load(p).map_err(f)`)
                // awaits the innermost call. See `await_wraps_whole_chain`.
                if await_wraps_whole_chain(expr, &self.namespace_bindings()) {
                    format!("(await {})", self.expr(expr)?)
                } else {
                    let (chain, awaited) = self.emit_await_spine(expr)?;
                    if awaited {
                        chain
                    } else {
                        format!("(await {chain})")
                    }
                }
            }
            Expr::Array { elements, .. } => {
                let mut els = Vec::with_capacity(elements.len());
                for el in elements {
                    els.push(match el {
                        ArrayElem::Expr(e) => self.expr(e)?,
                        ArrayElem::Spread(e) => format!("...{}", self.expr(e)?),
                    });
                }
                format!("[{}]", els.join(", "))
            }
            Expr::Object { fields, .. } => {
                let mut fs = Vec::with_capacity(fields.len());
                for f in fields {
                    fs.push(match f {
                        ObjectField::KeyValue { key, value, .. } => {
                            format!("{}: {}", glyph_ast::render_object_key(key), self.expr(value)?)
                        }
                        ObjectField::Spread { value, .. } => format!("...{}", self.expr(value)?),
                    });
                }
                if fs.is_empty() {
                    "{}".to_string()
                } else {
                    format!("{{ {} }}", fs.join(", "))
                }
            }
            Expr::Lambda {
                params,
                return_ty,
                body,
                is_async,
                ..
            } => {
                let params = self.lambda_params(params)?;
                let prefix = if *is_async { "async " } else { "" };
                // An async arrow returns a Promise, so an annotated return type
                // wraps in `Promise<T>` exactly like an async `fn` declaration.
                let ret = match return_ty {
                    Some(te) if *is_async => format!(": {}<{}>", self.g("Promise"), self.ty(te)?),
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                // Like a function, a lambda yields its tail expression (Glyph
                // block value). A `void`-annotated lambda runs its tail for
                // effect; any other (including an unannotated lambda) returns
                // it. Returning a `void` value stays valid TS, so defaulting an
                // unannotated lambda to "returns a value" is safe.
                let rv = match return_ty {
                    Some(te) => !is_void_type(te),
                    None => true,
                };
                // A lambda has no generic parameters of its own, so its returns
                // never need the enclosing function's generic return cast.
                let mut sub = self.sub(self.indent);
                sub.emit_fn_block(body, rv, None)?;
                format!("{prefix}({params}){ret} => {}", sub.out)
            }
            // A `match` genuinely nested inside a larger expression (an argument,
            // an operand) wraps the statement lowering in an immediately-invoked
            // arrow. Each arm `return`s from the arrow, so the IIFE evaluates to
            // the matched value. (A `match` that is the whole value of a `let`,
            // `mut`, `return`, or arm body does NOT come through here: those
            // lower to a flat `switch` statement.)
            Expr::Match { scrutinee, arms, .. } => {
                // A block arm's `return` means function-return; inside the IIFE
                // arrow it would return from the arrow instead, so value-position
                // block arms are rejected.
                if let Some(b) = arms.iter().find_map(|a| match &a.body {
                    MatchArmBody::Block(b) => Some(b),
                    _ => None,
                }) {
                    return Err(EmitError::Unsupported {
                        construct: "a block body in an arm of a match nested inside a larger expression",
                        span: b.span,
                    });
                }
                // Same reason, for `?`: the hoisted unwrap ends in `return
                // Err(...)`, which inside the arrow returns from the arrow
                // instead of the enclosing function. Rejecting it here keeps
                // that from becoming a silent miscompile.
                if let Some(span) = arm_try_span(arms) {
                    return Err(EmitError::TryInNestedExpressionMatch { span });
                }
                let mut sub = self.sub(self.indent + 1);
                sub.emit_match_dispatch(scrutinee, arms, ArmTerm::Return)?;
                let pad = "  ".repeat(self.indent);
                // An `await` in an arm cannot run inside a synchronous arrow
                // (TS1308), so the wrapper becomes an awaited async arrow. It is
                // parenthesized as a whole so a following `.field` or `(...)`
                // binds to the match's value, not to `await`'s operand.
                if arms.iter().any(|a| match &a.body {
                    MatchArmBody::Expr(e) => contains_await(e),
                    MatchArmBody::Block(_) => false,
                }) {
                    format!("(await (async () => {{\n{}{pad}}})())", sub.out)
                } else {
                    format!("(() => {{\n{}{pad}}})()", sub.out)
                }
            }
            Expr::Jsx(j) => self.emit_jsx(j)?,
            // The escape hatch emits its raw TypeScript verbatim, parenthesized
            // so it composes safely in any expression position; `tsc` checks it.
            Expr::Extern { raw, .. } => format!("({raw})"),
        })
    }

    fn template(&self, parts: &[TemplatePart]) -> Result<String, EmitError> {
        let mut out = String::from("`");
        for part in parts {
            match part {
                TemplatePart::Text { content, .. } => out.push_str(&escape_template_text(content)),
                TemplatePart::Expr { value, .. } => {
                    out.push_str("${");
                    out.push_str(&self.expr(value)?);
                    out.push('}');
                }
            }
        }
        out.push('`');
        Ok(out)
    }

    // ----- JSX (D6) + components (D19) -----

    /// Emit a `component` declaration (D19) as a React function component. The
    /// body returns JSX, so it emits with implicit tail returns like a non-void
    /// function.
    fn emit_component(&mut self, c: &ComponentDecl) -> Result<(), EmitError> {
        let generics = self.generics(&c.generics)?;
        let params = self.params(&c.params)?;
        let ret = match &c.return_ty {
            Some(te) => format!(": {}", self.ty(te)?),
            None => String::new(),
        };
        self.pad();
        self.out
            .push_str(&format!("{}function {}{generics}({params}){ret} ", self.emit_export, c.name));
        let cast = self.fn_return_cast(&c.return_ty)?;
        self.emit_fn_block(&c.body, true, cast)?;
        self.out.push('\n');
        Ok(())
    }

    /// Lower a JSX element (D6). Intrinsic (`<div>`) and component (`<Foo>`)
    /// elements become `React.createElement` calls; the `<if>`/`<for>`/`<match>`
    /// directives lower to a ternary / `.map` / a switch-returning IIFE.
    /// `<else>` and `<case>` are only meaningful inside their directive and are
    /// consumed there.
    fn emit_jsx(&self, j: &JsxElement) -> Result<String, EmitError> {
        match JsxKind::classify(&j.name) {
            JsxKind::Match => self.emit_jsx_match(j),
            JsxKind::For => self.emit_jsx_for(j),
            // A standalone `<if>` (not paired with a sibling `<else>`, which is
            // handled in `jsx_children`) has an empty alternative.
            JsxKind::If => {
                let cond = self.jsx_attr_expr(j, "cond")?;
                let then = self.jsx_branch_node(&j.children)?;
                Ok(format!("({cond} ? {then} : null)"))
            }
            JsxKind::Else => Err(EmitError::MisplacedElse { span: j.span }),
            JsxKind::Case => Err(EmitError::Unsupported {
                construct: "a `<case>` outside a `<match>`",
                span: j.span,
            }),
            JsxKind::Intrinsic | JsxKind::Component | JsxKind::Fragment => {
                self.emit_jsx_element(j, None)
            }
        }
    }

    /// Emit an intrinsic or component element as `React.createElement(tag,
    /// props, ...children)`. An intrinsic's tag is its name as a string
    /// literal; a component's tag is the identifier. `extra_prop` injects an
    /// extra prop (used to push a `<for key={...}>` onto the mapped element).
    fn emit_jsx_element(
        &self,
        j: &JsxElement,
        extra_prop: Option<(&str, String)>,
    ) -> Result<String, EmitError> {
        let kind = JsxKind::classify(&j.name);
        let tag = match kind {
            JsxKind::Intrinsic => escape_double_quoted(&j.name),
            JsxKind::Component => j.name.to_string(),
            JsxKind::Fragment => "React.Fragment".to_string(),
            _ => unreachable!("directives route through emit_jsx"),
        };
        // Attribute-name remapping (`class`->`className`, `on_click`->`onClick`)
        // applies only to intrinsic DOM elements. On a component, an attribute is
        // a user-defined prop name passed through verbatim (e.g. `on_select`).
        let is_intrinsic = kind == JsxKind::Intrinsic;
        let props = self.jsx_props(&j.attrs, extra_prop, is_intrinsic)?;
        let children = self.jsx_children(&j.children)?;
        if children.is_empty() {
            Ok(format!("React.createElement({tag}, {props})"))
        } else {
            Ok(format!(
                "React.createElement({tag}, {props}, {})",
                children.join(", ")
            ))
        }
    }

    /// Build the props object for an element: `{ name: value, ... }`, or `null`
    /// when there are no attributes. A string attribute becomes a quoted value;
    /// an expression attribute emits its expression. A positional attribute is
    /// only valid on a directive (handled there), so it is rejected here.
    fn jsx_props(
        &self,
        attrs: &[JsxAttr],
        extra_prop: Option<(&str, String)>,
        is_intrinsic: bool,
    ) -> Result<String, EmitError> {
        let mut fields: Vec<String> = Vec::new();
        if let Some((k, v)) = extra_prop {
            fields.push(format!("{k}: {v}"));
        }
        for a in attrs {
            match a {
                JsxAttr::String { name, value, .. } => fields.push(format!(
                    "{}: {}",
                    jsx_prop_key(&react_dom_prop(name, is_intrinsic)),
                    escape_double_quoted(value)
                )),
                JsxAttr::Expr { name, value, .. } => fields.push(format!(
                    "{}: {}",
                    jsx_prop_key(&react_dom_prop(name, is_intrinsic)),
                    self.expr(value)?
                )),
                JsxAttr::Positional { span, .. } => {
                    return Err(EmitError::Unsupported {
                        construct: "a positional attribute on a non-directive JSX element",
                        span: *span,
                    })
                }
                // `{...expr}` becomes an object spread inside the props literal,
                // which React merges just like `<input {...register()} />`.
                JsxAttr::Spread { value, .. } => {
                    fields.push(format!("...{}", self.expr(value)?))
                }
            }
        }
        if fields.is_empty() {
            Ok("null".to_string())
        } else {
            Ok(format!("{{ {} }}", fields.join(", ")))
        }
    }

    /// Emit a child list, pairing an `<if>` with a following `<else>` sibling
    /// (skipping the whitespace between) into a single ternary. Whitespace-only
    /// text is dropped; other text becomes a quoted string; an `{expr}` child
    /// emits its expression.
    fn jsx_children(&self, children: &[JsxChild]) -> Result<Vec<String>, EmitError> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < children.len() {
            match &children[i] {
                JsxChild::Text { content, .. } => {
                    let t = normalize_jsx_text(content);
                    if !t.is_empty() {
                        out.push(escape_double_quoted(&t));
                    }
                }
                JsxChild::Expr(e) => out.push(self.expr(e)?),
                JsxChild::Element(el) => match JsxKind::classify(&el.name) {
                    JsxKind::If => {
                        let cond = self.jsx_attr_expr(el, "cond")?;
                        let then = self.jsx_branch_node(&el.children)?;
                        // A following `<else>` sibling (past whitespace) is the
                        // alternative; otherwise the alternative is `null`.
                        let (alt, else_idx) = self.find_else(children, i + 1)?;
                        out.push(format!("({cond} ? {then} : {alt})"));
                        if let Some(e) = else_idx {
                            i = e;
                        }
                    }
                    JsxKind::Else => return Err(EmitError::MisplacedElse { span: el.span }),
                    _ => out.push(self.emit_jsx(el)?),
                },
            }
            i += 1;
        }
        Ok(out)
    }

    /// Scan from `start` past whitespace-only text for an `<else>`; return its
    /// emitted node and index when found, else (`"null"`, None).
    fn find_else(
        &self,
        children: &[JsxChild],
        start: usize,
    ) -> Result<(String, Option<usize>), EmitError> {
        let mut j = start;
        while j < children.len() {
            match &children[j] {
                JsxChild::Text { content, .. } if normalize_jsx_text(content).is_empty() => {
                    j += 1
                }
                JsxChild::Element(el) if JsxKind::classify(&el.name) == JsxKind::Else => {
                    return Ok((self.jsx_branch_node(&el.children)?, Some(j)));
                }
                _ => break,
            }
        }
        Ok(("null".to_string(), None))
    }

    /// Combine a child list into a single React node: `null` for none, the lone
    /// child for one, an array literal for several. Used for the branches of an
    /// `<if>`/`<else>` and the body of a `<for>`.
    fn jsx_node(&self, children: &[JsxChild]) -> Result<String, EmitError> {
        let parts = self.jsx_children(children)?;
        Ok(match parts.len() {
            0 => "null".to_string(),
            1 => parts.into_iter().next().expect("len checked"),
            _ => format!("[{}]", parts.join(", ")),
        })
    }

    /// Combine a conditional (`<if>`/`<else>`) or match (`<case>`) branch body
    /// into a single React node. Identical to `jsx_node` except that several
    /// children are wrapped in `React.createElement(React.Fragment, null, ...)`
    /// rather than a bare array literal. A branch occupies exactly one node slot,
    /// so a keyless Fragment is the correct grouping; a bare array child would
    /// trip React's "unique key" dev warning. (`<for>` bodies keep `jsx_node`:
    /// they map to sibling list entries and forward `key=` instead.)
    fn jsx_branch_node(&self, children: &[JsxChild]) -> Result<String, EmitError> {
        let parts = self.jsx_children(children)?;
        Ok(match parts.len() {
            0 => "null".to_string(),
            1 => parts.into_iter().next().expect("len checked"),
            _ => format!("React.createElement(React.Fragment, null, {})", parts.join(", ")),
        })
    }

    /// Lower `<match value={v}> <case V bind={x}>..</case> .. </match>` to a
    /// switch-returning IIFE: `((__v) => { switch (__v.tag) { case "V": {
    /// const x = __v.x; return ..; } .. } })(v)`. A `<case Variant>` with no
    /// `bind` returns its node directly; `bind={x}` binds `x` to the same-named
    /// payload field (variant payloads are spread flat onto the value).
    fn emit_jsx_match(&self, j: &JsxElement) -> Result<String, EmitError> {
        let value = self.jsx_attr_expr(j, "value")?;
        let mut cases = String::new();
        for child in &j.children {
            match child {
                JsxChild::Text { content, .. } if normalize_jsx_text(content).is_empty() => {}
                JsxChild::Element(el) if JsxKind::classify(&el.name) == JsxKind::Case => {
                    let variant = first_positional(&el.attrs).ok_or(EmitError::Unsupported {
                        construct: "a `<case>` without a variant name",
                        span: el.span,
                    })?;
                    let node = self.jsx_branch_node(&el.children)?;
                    match find_expr_attr(&el.attrs, "bind") {
                        Some(Expr::Ident { name, .. }) => cases.push_str(&format!(
                            "case \"{variant}\": {{ const {name} = __v.{name}; return {node}; }} "
                        )),
                        _ => cases.push_str(&format!("case \"{variant}\": return {node}; ")),
                    }
                }
                _ => {
                    return Err(EmitError::Unsupported {
                        construct: "a non-`<case>` child in a `<match>`",
                        span: j.span,
                    })
                }
            }
        }
        Ok(format!(
            "((__v) => {{ switch (__v.tag) {{ {cases}default: throw new Error(\"non-exhaustive match\"); }} }})({value})"
        ))
    }

    /// Lower `<for x in={xs} key={k}>BODY</for>` to `xs.map((x) => BODY)`. When
    /// a `key` is present and the body is a single element, the key is pushed
    /// onto that element's props (React keys map entries).
    fn emit_jsx_for(&self, j: &JsxElement) -> Result<String, EmitError> {
        let var = first_positional(&j.attrs).ok_or(EmitError::Unsupported {
            construct: "a `<for>` without a loop variable",
            span: j.span,
        })?;
        let iter = self.jsx_attr_expr(j, "in")?;
        let key = match find_expr_attr(&j.attrs, "key") {
            Some(e) => Some(self.expr(e)?),
            None => None,
        };
        let body = match (key, single_element_child(&j.children)) {
            (Some(k), Some(el)) => self.emit_jsx_element(el, Some(("key", k)))?,
            _ => self.jsx_node(&j.children)?,
        };
        Ok(format!("{iter}.map(({var}) => {body})"))
    }

    /// Emit the named expression attribute of `el`, or reject if it is missing.
    fn jsx_attr_expr(&self, el: &JsxElement, name: &str) -> Result<String, EmitError> {
        match find_expr_attr(&el.attrs, name) {
            Some(e) => self.expr(e),
            None => Err(EmitError::Unsupported {
                construct: "a directive missing its required attribute",
                span: el.span,
            }),
        }
    }

    // ----- types -----

    fn ty(&self, te: &TypeExpr) -> Result<String, EmitError> {
        Ok(match te {
            TypeExpr::Path { segments, .. } => {
                let joined = segments
                    .iter()
                    .map(|s| s.as_ref())
                    .collect::<Vec<_>>()
                    .join(".");
                // Glyph `bool` is TS `boolean`; `int` is TS `number` (TypeScript
                // has no integer type; the integer check is in the descriptor);
                // the rest map by name.
                if joined == "bool" {
                    "boolean".to_string()
                } else if joined == "int" {
                    "number".to_string()
                } else {
                    joined
                }
            }
            TypeExpr::Generic { base, args, .. } => {
                let mut a = Vec::with_capacity(args.len());
                for arg in args {
                    a.push(self.ty(arg)?);
                }
                // `infer_output<S>` (D28) lowers to the injected mapped-type
                // alias; the emitter never writes inline `{ [K in keyof S]... }`
                // so Glyph source stays free of mapped-type syntax.
                if is_infer_output_base(base) {
                    self.used_infer_output.set(true);
                    format!("{INFER_OUTPUT_ALIAS}<{}>", a.join(", "))
                } else {
                    format!("{}<{}>", self.ty(base)?, a.join(", "))
                }
            }
            TypeExpr::Fn {
                params,
                return_ty,
                is_async,
                ..
            } => {
                let mut ps = Vec::with_capacity(params.len());
                for (i, p) in params.iter().enumerate() {
                    let name = p
                        .name
                        .as_ref()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| format!("a{i}"));
                    ps.push(format!("{name}: {}", self.ty(&p.ty)?));
                }
                let ret = match return_ty {
                    Some(te) => self.ty(te)?,
                    None => "void".to_string(),
                };
                // D40: an `async fn(...) -> T` type is a function returning
                // `Promise<T>`, matching what the declaration path writes for an
                // `async fn` and what an async lambda's inferred type is.
                let ret = if *is_async {
                    format!("{}<{ret}>", self.g("Promise"))
                } else {
                    ret
                };
                format!("({}) => {ret}", ps.join(", "))
            }
            TypeExpr::Record { fields, .. } => {
                let mut fs = Vec::with_capacity(fields.len());
                for f in fields {
                    let opt = if f.optional { "?" } else { "" };
                    fs.push(format!("{}{opt}: {}", f.name, self.ty(&f.ty)?));
                }
                format!("{{ {} }}", fs.join("; "))
            }
            TypeExpr::Union { variants, span } => {
                // An inline structural union of type references (`string |
                // number`, `A | B`) emits as a TS union. A variant that carries a
                // payload is a nominal tagged-union constructor, which has no
                // anonymous inline type: that still needs a named `type`
                // declaration (emitted as a discriminated union with a descriptor).
                if variants.iter().any(|v| v.payload.is_some()) {
                    return Err(EmitError::Unsupported {
                        construct: "an inline union with a payload-carrying variant (declare a named type)",
                        span: *span,
                    });
                }
                variants
                    .iter()
                    .map(|v| match v.name.as_ref() {
                        // Same primitive mapping as `TypeExpr::Path`.
                        "bool" => "boolean".to_string(),
                        "int" => "number".to_string(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
            // The escape hatch emits its raw TypeScript verbatim; `tsc` checks it.
            TypeExpr::Extern { raw, .. } => raw.clone(),
            // A string-literal union emits as the TS literal union, so `tsc`
            // enforces the narrowed type at every use.
            TypeExpr::StringLiteralUnion { values, .. } => values
                .iter()
                .map(|v| escape_double_quoted(v))
                .collect::<Vec<_>>()
                .join(" | "),
            // `typeof value` emits as the TS type query; `tsc` reduces it (a
            // `z.infer<typeof s>` becomes the schema's inferred type).
            TypeExpr::TypeOf { path, .. } => {
                let joined = path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".");
                format!("typeof {joined}")
            }
        })
    }
}

/// True when a tagged union's descriptor `const <name>` would not collide with
/// any variant constructor `const`/`function`. A union with a variant sharing
/// its own name cannot also carry a descriptor under that name, so the
/// descriptor is skipped in that degenerate case.
fn union_descriptor_name_free(name: &str, variants: &[UnionVariant]) -> bool {
    variants.iter().all(|v| v.name.as_ref() != name)
}

/// True when a type declaration emits a *non-generic* runtime descriptor whose
/// `is`/`parse`/`schema` members a caller can use directly (no threaded
/// checkers): a record, a tagged union whose descriptor name is free, or a D39
/// refined primitive. Single-sourced so the emitter's descriptor resolution and
/// the CLI's project-wide registry agree on what "has a descriptor" means; a
/// generic record is excluded because its members take one checker per type
/// parameter (see `generic_descriptor_arities`).
pub fn emits_plain_descriptor(t: &glyph_ast::TypeDecl) -> bool {
    if !t.generics.is_empty() {
        return false;
    }
    match &t.body {
        TypeExpr::Record { .. } => true,
        TypeExpr::Union { variants, .. } => {
            union_descriptor_name_free(t.name.as_ref(), variants)
        }
        _ => t.refinement.is_some(),
    }
}

/// True if the type parameter `name` appears anywhere in the type `te`.
fn type_mentions(te: &TypeExpr, name: &str) -> bool {
    match te {
        TypeExpr::Path { segments, .. } => {
            segments.len() == 1 && segments[0].as_ref() == name
        }
        TypeExpr::Generic { base, args, .. } => {
            type_mentions(base, name) || args.iter().any(|a| type_mentions(a, name))
        }
        TypeExpr::Fn { params, return_ty, .. } => {
            params.iter().any(|p| type_mentions(&p.ty, name))
                || return_ty.as_ref().is_some_and(|r| type_mentions(r, name))
        }
        TypeExpr::Record { fields, .. } => fields.iter().any(|f| type_mentions(&f.ty, name)),
        TypeExpr::Union { variants, .. } => variants
            .iter()
            .any(|v| v.payload.as_ref().is_some_and(|p| type_mentions(p, name))),
        // A generic parameter mentioned only inside raw TS is not tracked; the
        // escape hatch is opaque, so treat it as not mentioning the parameter.
        TypeExpr::Extern { .. } => false,
        // String literals mention no type parameter.
        TypeExpr::StringLiteralUnion { .. } => false,
        TypeExpr::TypeOf { .. } => false,
    }
}

/// Render `Name<...>` applying each generic parameter as itself when `used` is
/// true, else widening it to `never`. A non-generic union is just its name.
fn apply_generics(name: &str, generics: &[GenericParam], used: &[bool]) -> String {
    if generics.is_empty() {
        return name.to_string();
    }
    let args = generics
        .iter()
        .zip(used)
        .map(|(g, &u)| if u { g.name.as_ref() } else { "never" })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}<{args}>")
}

/// The JS `typeof` string for a Glyph primitive type, or None for any
/// non-primitive (which the descriptor checks by presence instead).
fn js_typeof(te: &TypeExpr) -> Option<&'static str> {
    let TypeExpr::Path { segments, .. } = te else {
        return None;
    };
    match segments.as_slice() {
        [seg] => match seg.as_ref() {
            "string" => Some("string"),
            "number" => Some("number"),
            "bigint" => Some("bigint"),
            "bool" => Some("boolean"),
            "void" => Some("undefined"),
            _ => None,
        },
        _ => None,
    }
}

/// Render a literal pattern as a TS `case` label.
fn literal_label(value: &LiteralPattern) -> String {
    match value {
        LiteralPattern::Number(raw) => raw.clone(),
        LiteralPattern::String(s) => escape_double_quoted(s),
        LiteralPattern::Bool(b) => b.to_string(),
        LiteralPattern::Void => "undefined".to_string(),
    }
}

fn bin_op(op: BinOp) -> &'static str {
    match op {
        BinOp::NullishCoalesce => "??",
        BinOp::LogicalOr => "||",
        BinOp::LogicalAnd => "&&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        // Glyph `==`/`!=` are value equality; emit the strict TS forms.
        BinOp::Eq => "===",
        BinOp::NotEq => "!==",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::UShr => ">>>",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
    }
}

/// Render a de-escaped string value as a double-quoted TS string literal.
/// Format a JSX attribute name as an object-literal key: bare when it is a valid
/// JS identifier, quoted otherwise. Hyphenated names (`aria-label`, `data-testid`)
/// are not identifiers, so they must be quoted for the emitted props object to
/// be valid TypeScript.
/// Map Glyph's documented snake_case JSX idiom to the React DOM prop names an
/// intrinsic element needs. `class` becomes `className`; an `on_<event>` handler
/// becomes the camelCased `on<Event>` (`on_click`->`onClick`,
/// `on_input`->`onInput`, `on_change`->`onChange`). Hyphenated attributes such as
/// `data-testid` and `aria-label` carry no `on_` prefix and are left verbatim, so
/// React passes them through as raw DOM/ARIA attributes. On a component (not an
/// intrinsic element), every attribute is a user-defined prop passed through
/// unchanged, so no remapping happens there.
fn react_dom_prop(name: &str, is_intrinsic: bool) -> String {
    if !is_intrinsic {
        return name.to_string();
    }
    if name == "class" {
        return "className".to_string();
    }
    // `on_<event>` -> `on` + CamelCase, splitting the event on underscores so a
    // multi-word event (`on_mouse_down` -> `onMouseDown`) capitalizes each part.
    if let Some(event) = name.strip_prefix("on_") {
        if !event.is_empty() {
            let mut out = String::from("on");
            for part in event.split('_') {
                let mut chars = part.chars();
                if let Some(first) = chars.next() {
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                }
            }
            return out;
        }
    }
    name.to_string()
}

fn jsx_prop_key(name: &str) -> String {
    let mut chars = name.chars();
    let is_ident = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if is_ident {
        name.to_string()
    } else {
        escape_double_quoted(name)
    }
}

fn escape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // U+2028 / U+2029 are JS LineTerminators and illegal raw inside a
            // string literal; the remaining C0 controls (NUL, vertical tab,
            // form feed, ...) are also unsafe. Escape all of them as `\uXXXX`.
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a type the way the declaration spells it, for a boundary rejection
/// message. This is deliberately Glyph's spelling and not the emitted
/// TypeScript one (`int` stays `int`, `bool` stays `bool`), so the message a
/// caller reads greps back to the source line that imposed the rule. A shape
/// with no useful short spelling reads as a noun phrase instead (`a function`,
/// `an object`).
fn type_label(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Path { segments, .. } => segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("."),
        TypeExpr::Generic { base, args, .. } => format!(
            "{}<{}>",
            type_label(base),
            args.iter().map(type_label).collect::<Vec<_>>().join(", ")
        ),
        TypeExpr::Fn { .. } => "a function".to_string(),
        TypeExpr::Record { .. } => "an object".to_string(),
        TypeExpr::Union { variants, .. } => variants
            .iter()
            .map(|v| v.name.to_string())
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::StringLiteralUnion { values, .. } => values
            .iter()
            .map(|v| escape_double_quoted(v))
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Extern { raw, .. } => raw.clone(),
        TypeExpr::TypeOf { path, .. } => format!(
            "typeof {}",
            path.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(".")
        ),
    }
}

/// Drop one redundant enclosing paren pair from an emitted expression, so a
/// predicate reads `value.length >= 8` rather than `(value.length >= 8)` inside
/// a rejection message. Only strips when the opening paren's match is the final
/// character, so `(a) && (b)` is left alone.
fn strip_outer_parens(expr: &str) -> &str {
    let bytes = expr.as_bytes();
    if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
        return expr;
    }
    let mut depth = 0usize;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return if i + 1 == bytes.len() {
                        &expr[1..bytes.len() - 1]
                    } else {
                        expr
                    };
                }
            }
            _ => {}
        }
    }
    expr
}

/// Whether `arm` is a constructor pattern carrying a single nested constructor
/// argument (`Err(NetworkError({ status }))`), which needs an inner switch on
/// the payload's tag. A whole-payload bind (`Err(e)`), an object destructure
/// (`Err({ ... })`), or a wildcard (`Err(_)`) is not nested.
/// Whether a loop with this body needs a TS label so user `break`/`continue`
/// can target it. A `match` lowers to a `switch`, and an unlabeled `break`
/// inside a `switch` escapes only the switch — so a jump buried in a match arm
/// needs a labeled jump to the loop. Nested loops are not descended into: their
/// `break`/`continue` target themselves, not this loop.
fn loop_body_needs_label(block: &Block) -> bool {
    block.stmts.iter().any(|s| stmt_has_captured_jump(s, false))
}

fn stmt_has_captured_jump(stmt: &Stmt, in_switch: bool) -> bool {
    match stmt {
        Stmt::Break(_) | Stmt::Continue(_) => in_switch,
        // A nested loop owns its own break/continue.
        Stmt::Loop(_) | Stmt::For(_) => false,
        Stmt::Expr(e) => expr_has_captured_jump(e),
        Stmt::Return(r) => r.value.as_ref().is_some_and(expr_has_captured_jump),
        Stmt::Defer(d) => expr_has_captured_jump(&d.expr),
        // A `match` that is the whole value of a `let`/`mut` lowers to a
        // statement `switch`, so a `break`/`continue` in one of its arms lands
        // inside that switch and needs the loop's label.
        Stmt::Let(l) => expr_has_captured_jump(&l.value),
        Stmt::Mut(m) => match &m.kind {
            MutKind::Assign { value, .. } => expr_has_captured_jump(value),
            MutKind::MethodCall { .. } => false,
        },
    }
}

/// Whether emitting `e` puts a `break`/`continue` inside a `switch`. Only a
/// `match` (which lowers to a `switch`) does so; its arm bodies are scanned with
/// the jump now considered captured.
fn expr_has_captured_jump(e: &Expr) -> bool {
    match e {
        Expr::Match { arms, .. } => arms.iter().any(|a| match &a.body {
            MatchArmBody::Expr(e) => expr_has_captured_jump(e),
            MatchArmBody::Block(b) => b.stmts.iter().any(|s| stmt_has_captured_jump(s, true)),
        }),
        _ => false,
    }
}

fn arm_has_nested_constructor(arm: &MatchArm) -> bool {
    matches!(
        &arm.pattern,
        Pattern::Constructor { args, .. } if matches!(args.as_slice(), [a] if is_nested_variant_arg(a))
    )
}

/// Whether any binder this pattern lowers to inside a `switch` case is called
/// `name`. Covers every shape that emits a `const <binder> = ...` line:
/// `Pattern::Ident` (the payload binding and the binding catch-all), the args of
/// a constructor pattern, object-pattern fields (exactly the one name each field
/// binds, mirroring `emit_arm_binds`: the renamed binding when there is one, the
/// key otherwise — a renamed field never binds its key, so checking the key
/// would report a collision that cannot happen), and array-pattern elements plus
/// `...rest`. Wildcards, `else`, literals and `is T` bind nothing.
fn pattern_binds_name(p: &Pattern, name: &str) -> bool {
    match p {
        Pattern::Ident { name: n, .. } => n.as_ref() == name,
        Pattern::Constructor { args, .. } => args.iter().any(|a| pattern_binds_name(a, name)),
        Pattern::Object { fields, .. } => fields
            .iter()
            .any(|f| f.binding.as_ref().unwrap_or(&f.key).as_ref() == name),
        Pattern::Array { elements, rest, .. } => {
            elements.iter().any(|e| pattern_binds_name(e, name))
                || rest.as_deref().is_some_and(|r| pattern_binds_name(r, name))
        }
        Pattern::Wildcard { .. }
        | Pattern::Else { .. }
        | Pattern::Literal { .. }
        | Pattern::IsType { .. } => false,
    }
}

/// Whether lowering `arms` declares anything called `name` in a scope that
/// encloses the arm's own assignment statement.
///
/// This is the guard for the shadowing hazard in `let x = match ... { }`: the
/// statement form declares `x` outside the `switch` and each arm assigns it, so
/// an arm that *also* binds `x` (a destructured field of the same name, say)
/// would emit `const x = __m0.x; x = x;` — an assignment to a `const`, and, in
/// the shapes TypeScript accepts, a value dropped on the floor.
///
/// Three sources of binders are walked: the arm patterns, a top-level `let` in a
/// block arm body (the same collision through a different door), and a nested
/// `match` that *is* the arm body, since that one lowers into the same case block
/// rather than a nested scope. A `for` binder is not a source: it lowers to
/// `for (const i of ...)`, whose binding is scoped to the loop head and cannot
/// reach the case block the assignment sits in.
fn match_binds_name(arms: &[MatchArm], name: &str) -> bool {
    arms.iter().any(|a| {
        pattern_binds_name(&a.pattern, name)
            || match &a.body {
                MatchArmBody::Expr(Expr::Match { arms, .. }) => match_binds_name(arms, name),
                MatchArmBody::Expr(_) => false,
                MatchArmBody::Block(b) => b
                    .stmts
                    .iter()
                    .any(|s| matches!(s, Stmt::Let(l) if l.name.as_ref() == name)),
            }
    })
}

/// Whether a `mut <lvalue> = match ... { }` needs the assignment routed through
/// a temporary because an arm binds a name the rendered lvalue mentions.
///
/// The lvalue is already rendered TypeScript here (`x`, `a.b`, `a[i]`), so its
/// identifiers are recovered by splitting on everything that cannot appear in
/// one. That over-approximates (`a.text` counts as mentioning `text`, which is
/// harmless — it costs a temporary, never correctness) and cannot under-report,
/// which is the direction that matters: `mut a.b = match { X({ a }) => ... }`
/// would otherwise assign through the arm's `a`, not the outer one.
fn lvalue_mentions_match_binding(arms: &[MatchArm], lvalue: &str) -> bool {
    lvalue
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .filter(|s| !s.is_empty())
        .any(|ident| match_binds_name(arms, ident))
}

/// Whether an `await` over `e` should apply to the *whole chain* (JavaScript
/// semantics) rather than to the innermost call of its receiver spine.
///
/// The default (await the innermost call) is right for the Result idiom, where
/// the async call heads the chain and the rest are synchronous combinators:
/// `await load(p).map_err(f)` means `(await load(p)).map_err(f)`. It is wrong for
/// a *fluent* API, where a synchronous call precedes the async terminal:
/// `await cursor.find({}).to_array()` must await `to_array()`, not `find()`.
///
/// The distinguishing signal, with no type information (colorless async erases
/// which call is async): the innermost call is a **value method**
/// (`recv.method(...)` where `recv` is a plain value, not a namespace and not a
/// bare function). A bare function call (`load(...)`) or a namespaced function
/// call (`http.get(...)`) keeps the innermost-await behavior; only a value-method
/// head switches to awaiting the whole chain.
fn await_wraps_whole_chain(e: &Expr, namespaces: &[&str]) -> bool {
    // Descend the receiver spine to the innermost call.
    let mut cur = e;
    let innermost = loop {
        match cur {
            Expr::Call { callee, .. } => {
                if spine_has_call(callee) {
                    cur = callee;
                } else {
                    break cur;
                }
            }
            Expr::Member { object, .. } | Expr::Index { object, .. } => cur = object,
            _ => return false,
        }
    };
    let Expr::Call { callee, .. } = innermost else {
        return false;
    };
    match callee.as_ref() {
        // A method call: fluent unless the receiver is a namespace (`http.get`).
        Expr::Member { object, .. } => match object.as_ref() {
            Expr::Ident { name, .. } => !namespaces.iter().any(|n| *n == name.as_ref()),
            // A deeper receiver chain (`a.b.c(...)`) is a value method.
            _ => true,
        },
        // A bare function call (`load(...)`): the Result-idiom head.
        _ => false,
    }
}

/// Whether the receiver spine of `e` (a call's callee, a member/index's
/// object, a `?`'s operand) bottoms out at a call.
fn spine_has_call(e: &Expr) -> bool {
    match e {
        Expr::Call { .. } => true,
        Expr::Member { object, .. } | Expr::Index { object, .. } => spine_has_call(object),
        Expr::Postfix { operand, .. } => spine_has_call(operand),
        _ => false,
    }
}

/// Wrap the head call of `e`'s receiver spine in an `await`, descending through
/// a call's callee, a member/index's object, and a `?`'s operand to reach it.
/// Glyph async is colorless and `await` may syntactically wrap a whole chain,
/// but the async call is the spine head; awaiting it there keeps a mid-chain
/// `?` unwrapping the AWAITED result. A spine with no call awaits the whole
/// expression. Only the receiver spine is followed, never arguments, so a
/// second async call in an argument keeps its own `await`.
fn await_head(e: &Expr, await_span: Span) -> Expr {
    match e {
        Expr::Call {
            callee,
            type_args,
            args,
            span,
        } if spine_has_call(callee) => Expr::Call {
            callee: Box::new(await_head(callee, await_span)),
            type_args: type_args.clone(),
            args: args.clone(),
            span: *span,
        },
        Expr::Member {
            object,
            field,
            optional,
            span,
        } if spine_has_call(object) => Expr::Member {
            object: Box::new(await_head(object, await_span)),
            field: field.clone(),
            optional: *optional,
            span: *span,
        },
        Expr::Index {
            object,
            index,
            span,
        } if spine_has_call(object) => Expr::Index {
            object: Box::new(await_head(object, await_span)),
            index: index.clone(),
            span: *span,
        },
        Expr::Postfix { op, operand, span } => Expr::Postfix {
            op: *op,
            operand: Box::new(await_head(operand, await_span)),
            span: *span,
        },
        // The spine head (a call with no deeper spine call) or a non-call: this
        // is what the `await` applies to.
        _ => Expr::Await {
            expr: Box::new(e.clone()),
            span: await_span,
        },
    }
}

/// Relocate every `await` in `e` onto the async call at the head of its spine
/// (see `await_head`). Run before `hoist_tries` on a statement value that
/// contains a `?`, so a `?` whose operand is an awaited call hoists the awaited
/// result. Only `await` nodes move; the tree is otherwise preserved.
fn place_awaits(e: &Expr) -> Expr {
    match e {
        Expr::Await { expr, span } => await_head(&place_awaits(expr), *span),
        Expr::Postfix { op, operand, span } => Expr::Postfix {
            op: *op,
            operand: Box::new(place_awaits(operand)),
            span: *span,
        },
        Expr::Binary {
            op,
            left,
            right,
            span,
        } => Expr::Binary {
            op: *op,
            left: Box::new(place_awaits(left)),
            right: Box::new(place_awaits(right)),
            span: *span,
        },
        Expr::Unary { op, operand, span } => Expr::Unary {
            op: *op,
            operand: Box::new(place_awaits(operand)),
            span: *span,
        },
        Expr::Call {
            callee,
            type_args,
            args,
            span,
        } => Expr::Call {
            callee: Box::new(place_awaits(callee)),
            type_args: type_args.clone(),
            args: args.iter().map(place_awaits).collect(),
            span: *span,
        },
        Expr::Member {
            object,
            field,
            optional,
            span,
        } => Expr::Member {
            object: Box::new(place_awaits(object)),
            field: field.clone(),
            optional: *optional,
            span: *span,
        },
        Expr::Index {
            object,
            index,
            span,
        } => Expr::Index {
            object: Box::new(place_awaits(object)),
            index: Box::new(place_awaits(index)),
            span: *span,
        },
        Expr::Array { elements, span } => Expr::Array {
            elements: elements
                .iter()
                .map(|el| match el {
                    ArrayElem::Expr(e) => ArrayElem::Expr(place_awaits(e)),
                    ArrayElem::Spread(e) => ArrayElem::Spread(place_awaits(e)),
                })
                .collect(),
            span: *span,
        },
        Expr::Object { fields, span } => Expr::Object {
            fields: fields
                .iter()
                .map(|f| match f {
                    ObjectField::KeyValue { key, value, span } => ObjectField::KeyValue {
                        key: key.clone(),
                        value: place_awaits(value),
                        span: *span,
                    },
                    ObjectField::Spread { value, span } => ObjectField::Spread {
                        value: place_awaits(value),
                        span: *span,
                    },
                })
                .collect(),
            span: *span,
        },
        Expr::TemplateString { parts, span } => Expr::TemplateString {
            parts: parts
                .iter()
                .map(|p| match p {
                    TemplatePart::Text { content, span } => TemplatePart::Text {
                        content: content.clone(),
                        span: *span,
                    },
                    TemplatePart::Expr { value, span } => TemplatePart::Expr {
                        value: place_awaits(value),
                        span: *span,
                    },
                })
                .collect(),
            span: *span,
        },
        // Leaves, and the opaque lambda/match/JSX constructs (their `await`s
        // belong to their own statement context).
        other => other.clone(),
    }
}

/// Whether `e` contains a `?` operator that must be hoisted before the
/// enclosing statement (any `?`, since `hoist_tries`/`emit_value` treat a
/// whole-value `?` the same as a nested one). Does not look inside a lambda
/// body or a nested `match`/JSX — those carry their own statement context.
fn contains_hoistable_try(e: &Expr) -> bool {
    try_span(e, false).is_some()
}

/// Pin an empty array literal that an arm assigns into a lowered `match`'s
/// binding. A bare `[]` assigned to an unannotated `let` starts TypeScript's
/// evolving-array inference, and every later read of the binding is then an
/// implicit `any[]` (TS7034/TS7005). The value IIFE this lowering replaced
/// inferred `never[]` in the same spot, so the cast keeps the emitted type what
/// it always was; `never[]` is assignable to any annotated array type, so an
/// annotated binding is unaffected.
fn pin_empty_array(rendered: String, e: &Expr) -> String {
    match e {
        Expr::Array { elements, .. } if elements.is_empty() => format!("{rendered} as never[]"),
        _ => rendered,
    }
}

/// The span of a `?` in an arm body of `arms`, if there is one. Used by the
/// nested-expression (IIFE) match lowering, which cannot host a `?`: the hoisted
/// unwrap returns from the enclosing function, and the arrow would swallow it.
///
/// Descends into a nested `match`'s arm bodies (the way `contains_await` does),
/// because a nested match is emitted into the same arrow. A lambda body is not
/// descended into: its `?` belongs to the lambda's own statement context.
fn arm_try_span(arms: &[MatchArm]) -> Option<Span> {
    arms.iter().find_map(|a| match &a.body {
        MatchArmBody::Expr(e) => try_span(e, true),
        MatchArmBody::Block(_) => None,
    })
}

/// The span of the first `?` in `e` in evaluation order, or `None`.
///
/// The single walk behind both `?` questions the emitter asks, since two
/// hand-maintained walks over the same grammar drift (these two did, over
/// `Expr::New`, before they were a week old):
///
/// - `descend_into_match: false` — "is there a `?` the enclosing statement must
///   hoist?" (`contains_hoistable_try`). A nested `match` is opaque: it carries
///   its own statement context and hoists its arms' `?` itself.
/// - `descend_into_match: true` — "is there a `?` anywhere that would end up
///   inside this IIFE?" (`arm_try_span`). A nested match is emitted into the
///   same arrow, so its arms count.
///
/// A lambda body is never descended into (its `?` is hoisted by the lambda's own
/// statement emission), and neither is JSX. `hoist_tries` mirrors the
/// `descend_into_match: false` shape and must be extended alongside this walk;
/// a `?` this walk finds and `hoist_tries` cannot rewrite (inside a `new`, for
/// one) reaches `expr` and is reported as E0303 rather than miscompiled.
fn try_span(e: &Expr, descend_into_match: bool) -> Option<Span> {
    let go = |x: &Expr| try_span(x, descend_into_match);
    match e {
        Expr::Postfix {
            op: PostfixOp::Try,
            span,
            ..
        } => Some(*span),
        Expr::Binary { left, right, .. } => go(left).or_else(|| go(right)),
        Expr::Index {
            object: a,
            index: b,
            ..
        } => go(a).or_else(|| go(b)),
        Expr::Unary { operand: x, .. }
        | Expr::Await { expr: x, .. }
        | Expr::Member { object: x, .. } => go(x),
        Expr::Call { callee, args, .. } | Expr::New { callee, args, .. } => {
            go(callee).or_else(|| args.iter().find_map(go))
        }
        Expr::Array { elements, .. } => elements.iter().find_map(|el| match el {
            ArrayElem::Expr(e) | ArrayElem::Spread(e) => go(e),
        }),
        Expr::Object { fields, .. } => fields.iter().find_map(|f| match f {
            ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => go(value),
        }),
        Expr::TemplateString { parts, .. } => parts.iter().find_map(|p| match p {
            TemplatePart::Expr { value, .. } => go(value),
            TemplatePart::Text { .. } => None,
        }),
        Expr::Match { arms, .. } if descend_into_match => arm_try_span(arms),
        // Leaves, lambdas (own statement context), JSX, and — when not
        // descending — a nested match.
        _ => None,
    }
}

/// Whether `e` contains an `await` that would run in the current function's
/// async context. A nested lambda body is NOT descended into: it carries its own
/// async-ness, and its `await` belongs to that lambda, not to the expression
/// wrapping it. Used to decide whether a nested value-position `match` needs an
/// awaited async arrow rather than a synchronous one.
fn contains_await(e: &Expr) -> bool {
    match e {
        Expr::Await { .. } => true,
        Expr::Binary { left, right, .. } => contains_await(left) || contains_await(right),
        Expr::Index {
            object: a,
            index: b,
            ..
        } => contains_await(a) || contains_await(b),
        Expr::Unary { operand: x, .. }
        | Expr::Postfix { operand: x, .. }
        | Expr::Member { object: x, .. } => contains_await(x),
        Expr::Call { callee, args, .. } => {
            contains_await(callee) || args.iter().any(contains_await)
        }
        Expr::New { callee, args, .. } => contains_await(callee) || args.iter().any(contains_await),
        Expr::Array { elements, .. } => elements.iter().any(|el| match el {
            ArrayElem::Expr(e) | ArrayElem::Spread(e) => contains_await(e),
        }),
        Expr::Object { fields, .. } => fields.iter().any(|f| match f {
            ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => {
                contains_await(value)
            }
        }),
        Expr::TemplateString { parts, .. } => parts.iter().any(|p| match p {
            TemplatePart::Expr { value, .. } => contains_await(value),
            TemplatePart::Text { .. } => false,
        }),
        // A nested value-position `match` lowers to its own wrapper; an `await`
        // in its arms makes that wrapper awaited, so this one must be async too.
        Expr::Match { arms, .. } => arms.iter().any(|a| match &a.body {
            MatchArmBody::Expr(e) => contains_await(e),
            MatchArmBody::Block(_) => false,
        }),
        // Leaves, lambdas (own async context), and JSX.
        _ => false,
    }
}

/// Classification of a JSX element name (mirrors the resolver's `JsxKind`):
/// the compiler-owned directives, an intrinsic (lowercase HTML element), or a
/// component reference (capitalized).
#[derive(PartialEq, Eq)]
enum JsxKind {
    Intrinsic,
    Component,
    Fragment,
    If,
    Else,
    For,
    Match,
    Case,
}

impl JsxKind {
    fn classify(name: &Ident) -> Self {
        match name.as_ref() {
            "" => JsxKind::Fragment,
            "if" => JsxKind::If,
            "else" => JsxKind::Else,
            "for" => JsxKind::For,
            "match" => JsxKind::Match,
            "case" => JsxKind::Case,
            other => {
                if other.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    JsxKind::Intrinsic
                } else {
                    JsxKind::Component
                }
            }
        }
    }
}

/// The value expression of the named `name={expr}` attribute, if present.
fn find_expr_attr<'a>(attrs: &'a [JsxAttr], name: &str) -> Option<&'a Expr> {
    attrs.iter().find_map(|a| match a {
        JsxAttr::Expr { name: n, value, .. } if n.as_ref() == name => Some(value),
        _ => None,
    })
}

/// The name of the first positional attribute (`<case Loaded>` → `Loaded`,
/// `<for user ...>` → `user`), if any.
fn first_positional(attrs: &[JsxAttr]) -> Option<&Ident> {
    attrs.iter().find_map(|a| match a {
        JsxAttr::Positional { name, .. } => Some(name),
        _ => None,
    })
}

/// Normalize JSX text following the JSX whitespace rules (Babel's
/// `cleanJSXElementLiteralChild`): split into lines, strip leading whitespace
/// on every line after the first and trailing whitespace on every line before
/// the last, tabs become spaces, and non-empty lines join with a single space.
/// A whitespace-only run that spans a newline (the indentation between tags)
/// collapses to empty and is dropped by the caller. A single-line run keeps its
/// significant leading/trailing space, so text abutting an interpolated `{expr}`
/// child on the same line (`Hello {name} and welcome`) preserves the space
/// separating them, matching Babel/tsc.
fn normalize_jsx_text(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let last_non_empty = lines
        .iter()
        .rposition(|l| l.chars().any(|c| c != ' ' && c != '\t'))
        .unwrap_or(0);
    let line_count = lines.len();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let mut trimmed = line.replace('\t', " ");
        if i != 0 {
            trimmed = trimmed.trim_start_matches(' ').to_string();
        }
        if i != line_count - 1 {
            trimmed = trimmed.trim_end_matches(' ').to_string();
        }
        if !trimmed.is_empty() {
            if i != last_non_empty {
                trimmed.push(' ');
            }
            out.push_str(&trimmed);
        }
    }
    out
}

/// The single element child of a child list, ignoring whitespace-only text;
/// None if there is not exactly one element child.
fn single_element_child(children: &[JsxChild]) -> Option<&JsxElement> {
    let mut found = None;
    for c in children {
        match c {
            JsxChild::Text { content, .. } if normalize_jsx_text(content).is_empty() => {}
            JsxChild::Element(el) if found.is_none() => found = Some(el),
            // A second element, or any non-whitespace text / expr child.
            _ => return None,
        }
    }
    found
}

/// Whether `te`'s base is the single-segment path `infer_output` (D28). Written
/// like an ordinary generic application, so it is recognized structurally here.
fn is_infer_output_base(base: &TypeExpr) -> bool {
    matches!(base, TypeExpr::Path { segments, .. } if segments.len() == 1 && segments[0].as_ref() == "infer_output")
}

/// Whether `te` mentions `infer_output<S>` (D28) anywhere. A function whose
/// declared return type does asserts a dynamically-built value matches the
/// shape-derived type (`object_schema<Shape> -> Schema<infer_output<Shape>>`);
/// the emitter casts exactly those returns. Every other generic return is
/// checked precisely by `tsc` with no cast.
fn type_mentions_infer_output(te: &TypeExpr) -> bool {
    match te {
        TypeExpr::Path { .. } => false,
        TypeExpr::Generic { base, args, .. } => {
            is_infer_output_base(base) || args.iter().any(type_mentions_infer_output)
        }
        TypeExpr::Fn {
            params, return_ty, ..
        } => {
            params
                .iter()
                .any(|p: &FnTypeParam| type_mentions_infer_output(&p.ty))
                || return_ty
                    .as_ref()
                    .is_some_and(|r| type_mentions_infer_output(r))
        }
        TypeExpr::Record { fields, .. } => {
            fields.iter().any(|f| type_mentions_infer_output(&f.ty))
        }
        TypeExpr::Union { .. } => false,
        TypeExpr::Extern { .. } => false,
        TypeExpr::StringLiteralUnion { .. } => false,
        TypeExpr::TypeOf { .. } => false,
    }
}

/// Whether `te` is the single-segment type named `name` (`void`, `unknown`).
fn is_named_type(te: &TypeExpr, name: &str) -> bool {
    matches!(te, TypeExpr::Path { segments, .. } if segments.len() == 1 && segments[0].as_ref() == name)
}

/// Whether `te` is the `void` type.
fn is_void_type(te: &TypeExpr) -> bool {
    is_named_type(te, "void")
}

/// Whether `te` is the `unknown` type (what the parser records for an
/// un-annotated lambda parameter).
fn is_unknown_type(te: &TypeExpr) -> bool {
    is_named_type(te, "unknown")
}

/// Whether a function with this return type yields a value through its tail
/// expression (an implicit return). A `void` or unannotated return does not:
/// its body runs for effect.
fn returns_value(return_ty: &Option<TypeExpr>) -> bool {
    match return_ty {
        Some(te) => !is_void_type(te),
        None => false,
    }
}

/// Escape the literal-text segment of a template so backticks, backslashes,
/// and `${` do not start an interpolation in the emitted TS.
fn escape_template_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse, resolve, and typecheck `src`, then return the emitter's
    /// (resolved module, type map) — tolerating resolve/type errors so test
    /// snippets can reference undefined helpers (`log`, `fetch`); the emitter
    /// only consults types where they are known.
    fn pipeline(
        src: &str,
    ) -> (
        glyph_ast::Module,
        glyph_resolver::ResolvedModule,
        glyph_typechecker::TypeMap,
        glyph_resolver::Prelude,
    ) {
        let module = glyph_parser::parse(src).expect("parse failed");
        let syms = glyph_resolver::collect_module_symbols(&module).expect("collect failed");
        let prelude = glyph_resolver::build_prelude();
        let (resolved, _errs) = glyph_resolver::resolve_module(&module, syms, &prelude);
        let (types, _ty_errs) = glyph_typechecker::assign_types(&module, &resolved, &prelude);
        (module, resolved, types, prelude)
    }

    fn emit(src: &str) -> String {
        let (module, resolved, types, prelude) = pipeline(src);
        emit_module(&module, &resolved, &types, &prelude, EmitContext::single()).expect("emit failed")
    }

    fn emit_err(src: &str) -> EmitError {
        let (module, resolved, types, prelude) = pipeline(src);
        emit_module(&module, &resolved, &types, &prelude, EmitContext::single())
            .expect_err("expected emit error")
    }

    #[test]
    fn try_in_an_expression_form_arm_hoists_inside_the_case() {
        // An arm body is a statement value, so a `?` in one hoists to an unwrap
        // above the arm's value line. Before this, `expr` rejected it outright
        // with "not implemented yet" while the same `?` in a block arm compiled.
        let ts = emit(
            "module x\ntype Id = | None | Some(number)\n\
             fn f(b: string) -> Result<string, string> { return Ok(b) }\n\
             fn g(b: string, n: number) -> Result<string, string> { return Ok(b) }\n\
             fn pick(id: Id, b: string) -> Result<string, string> {\n  \
               let x = match id {\n    None => f(b)?,\n    Some(n) => g(b, n)?,\n  }\n  \
               return Ok(x)\n}\n",
        );
        assert!(ts.contains("const __r1 = f(b);"), "{ts}");
        assert!(
            ts.contains("if (__r1.tag === \"Err\") { return __glyph_err(__r1.value); }"),
            "{ts}"
        );
        assert!(ts.contains("x = __r1.value;"), "{ts}");
        assert!(ts.contains("const __r2 = g(b, n);"), "{ts}");
        assert!(ts.contains("x = __r2.value;"), "{ts}");
    }

    #[test]
    fn try_in_a_nested_expression_match_is_e0302_not_e0300() {
        // A match inside a larger expression lowers to an arrow, where the
        // hoisted `return Err(...)` would return from the arrow. It is rejected
        // with the rule (E0302), not a false "not implemented yet" (E0300).
        let err = emit_err(
            "module x\ntype Id = | None | Some(number)\n\
             fn f(b: string) -> Result<string, string> { return Ok(b) }\n\
             fn shout(s: string) -> string { return s }\n\
             fn pick(id: Id, b: string) -> Result<string, string> {\n  \
               return Ok(shout(match id {\n    None => f(b)?,\n    Some(n) => b,\n  }))\n}\n",
        );
        assert!(
            matches!(err, EmitError::TryInNestedExpressionMatch { .. }),
            "{err:?}"
        );
        assert_eq!(err.code(), "E0302");
    }

    #[test]
    fn try_in_a_match_scrutinee_is_e0303_not_a_false_not_implemented() {
        // The scrutinee is rendered through `expr`, which has no statement slot
        // to hoist the unwrap into. That is a positional rule, so it reports as
        // one instead of claiming the `?` operator is unimplemented (E0300).
        let err = emit_err(
            "module x\ntype Id = | None | Some(number)\n\
             fn load(p: string) -> Result<Id, string> { return Ok(None) }\n\
             fn pick(p: string) -> Result<string, string> {\n  \
               let x = match load(p)? {\n    None => \"n\",\n    Some(n) => \"s\",\n  }\n  \
               return Ok(x)\n}\n",
        );
        assert!(
            matches!(err, EmitError::TryInUnhoistablePosition { .. }),
            "{err:?}"
        );
        assert_eq!(err.code(), "E0303");
        assert!(!err.to_string().contains("not implemented"), "{err}");
    }

    #[test]
    fn fn_with_params_and_body() {
        let ts = emit("module x\npub fn add(a: number, b: number) -> number { return a + b }\n");
        assert_eq!(
            ts,
            "import \"./.glyph-runtime/glyph-bootstrap\";\n\n\
             export function add(a: number, b: number): number {\n  return (a + b);\n}\n"
        );
    }

    #[test]
    fn hyphenated_jsx_attr_keys_are_quoted() {
        // A bare `aria-label:` object key is invalid TS; it must be quoted.
        let ts = emit(
            "module x\n@pure\npub fn t() -> string { return \"x\" }\npub component C() -> Component { return <button aria-label=\"Delete\" data-testid={t()}>x</button> }\n",
        );
        assert!(ts.contains("\"aria-label\": \"Delete\""), "{ts}");
        assert!(ts.contains("\"data-testid\": t()"), "{ts}");
        // A plain identifier key stays unquoted.
        assert!(ts.contains("className:") || !ts.contains("\"className\""), "{ts}");
    }

    #[test]
    fn every_module_imports_the_runtime_bootstrap_first() {
        // The bootstrap installs the ambient `number`/`par`/`print` globals; a
        // module must pull it in so an external bundler's entry has them (the bug
        // Serhiy hit: a Vite build never loaded it, so `number` was undefined).
        let ts = emit("module x\npub fn noop() -> number { return 0 }\n");
        assert!(
            ts.starts_with("import \"./.glyph-runtime/glyph-bootstrap\";"),
            "{ts}"
        );
    }

    #[test]
    fn nested_module_reaches_the_bootstrap_with_parent_hops() {
        // A module one directory deep needs `../` to reach the output-root runtime.
        let modules: std::collections::BTreeSet<String> =
            ["sub/a".to_string()].into_iter().collect();
        let (module, resolved, types, prelude) =
            pipeline("module sub/a\npub fn noop() -> number { return 0 }\n");
        let ctx = EmitContext {
            module_path: "sub/a",
            project_modules: &modules,
            record_payload_variants: &EMPTY_VARIANTS,
            generic_descriptor_arities: &EMPTY_ARITIES,
            plain_descriptors: &EMPTY_DESCRIPTORS,
            descriptorless_aliases: &EMPTY_ALIASES,
        };
        let ts = emit_module(&module, &resolved, &types, &prelude, ctx).expect("emit failed");
        assert!(
            ts.starts_with("import \"../.glyph-runtime/glyph-bootstrap\";"),
            "{ts}"
        );
    }

    #[test]
    fn nested_module_reaches_the_std_runtime_with_parent_hops() {
        // Every runtime specifier is relative for the host-toolchain reason on
        // `runtime_specifier` (G122): a written `std` import, the auto-imported
        // prelude constructors, and the injected `?` machinery all hop out of
        // the module's directory to the output-root `.glyph-runtime/std/`.
        let modules: std::collections::BTreeSet<String> =
            ["sub/a".to_string()].into_iter().collect();
        let (module, resolved, types, prelude) = pipeline(
            "module sub/a\nimport std/io\n\
             pub fn go(p: string) -> Result<number, string> {\n  \
               io.print(p)\n  let n = parse(p)?\n  return Ok(n)\n}\n\
             fn parse(p: string) -> Result<number, string> { return Ok(0) }\n",
        );
        let ctx = EmitContext {
            module_path: "sub/a",
            project_modules: &modules,
            record_payload_variants: &EMPTY_VARIANTS,
            generic_descriptor_arities: &EMPTY_ARITIES,
            plain_descriptors: &EMPTY_DESCRIPTORS,
            descriptorless_aliases: &EMPTY_ALIASES,
        };
        let ts = emit_module(&module, &resolved, &types, &prelude, ctx).expect("emit failed");
        assert!(
            ts.contains("import * as io from \"../.glyph-runtime/std/io\";"),
            "{ts}"
        );
        assert!(
            ts.contains("from \"../.glyph-runtime/std/result\";"),
            "{ts}"
        );
        assert!(!ts.contains("from \"std/"), "{ts}");
    }

    #[test]
    fn bool_maps_to_boolean_and_eq_is_strict() {
        let ts = emit("module x\npub fn p(a: number, b: number) -> bool { return a == b }\n");
        assert!(ts.contains("): boolean {"), "{ts}");
        assert!(ts.contains("(a === b)"), "{ts}");
    }

    #[test]
    fn async_fn_and_await() {
        let ts = emit("module x\npub async fn run() -> number { return await fetch() }\n");
        assert!(
            ts.contains("export async function run(): Promise<number> {"),
            "{ts}"
        );
        assert!(ts.contains("return (await fetch());"), "{ts}");
    }

    #[test]
    fn async_closure_emits_an_async_arrow() {
        // F11/F12: `async fn() { await ... }` emits an async arrow, and an
        // annotated return type wraps in `Promise<T>` (an async arrow returns a
        // Promise), so a task thunk can await inside a closure. The thunk takes
        // no parameters, which is what makes it the shape `task.all` accepts and
        // `array.map` can build.
        let ts = emit(
            "module x\nimport std/task\nfn t(n: number) -> async fn() -> number {\n  return async fn() -> number { return await work(n) }\n}\npub async fn run(xs: Array<number>) -> Array<number> {\n  return await task.all(array.map(xs, t))\n}\n",
        );
        assert!(ts.contains("async (): Promise<number> =>"), "async arrow with Promise return:\n{ts}");
        // The unannotated form emits a bare async arrow.
        let ts2 = emit(
            "module x\nimport std/task\nfn t(n: number) -> async fn() -> number {\n  return async fn() { await work(n) }\n}\npub async fn run(xs: Array<number>) -> Array<number> {\n  return await task.all(array.map(xs, t))\n}\n",
        );
        assert!(ts2.contains("async () =>"), "bare async arrow:\n{ts2}");
    }

    #[test]
    fn template_literal_passes_through() {
        let ts = emit("module x\npub fn greet(name: string) -> string { return \"hi ${name}\" }\n");
        assert!(ts.contains("return `hi ${name}`;"), "{ts}");
    }

    #[test]
    fn escaped_dollar_is_a_literal_not_an_interpolation() {
        // `\${x}` is a literal `${x}`; a real `${x}` still interpolates. The
        // emitted JS template escapes the literal `${` so it doesn't interpolate
        // in JS either.
        let ts = emit(
            "module x\npub fn f(x: string) -> string { return \"lit \\${x} and ${x}\" }\n",
        );
        assert!(ts.contains("`lit \\${x} and ${x}`"), "{ts}");
    }

    #[test]
    fn non_ascii_template_text_is_not_mangled() {
        // The template splitter must be char-aware: multi-byte text used to be
        // pushed byte-by-byte, producing mojibake.
        let ts = emit(
            "module x\npub fn f(n: string) -> string { return \"café ${n} 日本\" }\n",
        );
        assert!(ts.contains("`café ${n} 日本`"), "{ts}");
    }

    #[test]
    fn plain_string_with_escaped_dollar_resolves() {
        // A non-interpolating string with `\${` becomes a literal `${`.
        let ts = emit("module x\npub const P: string = \"price: \\${5}\"\n");
        assert!(ts.contains("\"price: ${5}\""), "{ts}");
    }

    #[test]
    fn const_and_type_alias() {
        let ts = emit("module x\npub const MAX: number = 10\npub type Sku = string\n");
        assert!(ts.contains("export const MAX: number = 10;"), "{ts}");
        assert!(ts.contains("export type Sku = string;"), "{ts}");
    }

    #[test]
    fn record_type_alias_and_void_value() {
        let ts = emit("module x\npub type User = { name: string, age?: number }\n");
        assert!(
            ts.contains("export type User = { name: string; age?: number };"),
            "{ts}"
        );
    }

    #[test]
    fn record_type_emits_an_is_descriptor() {
        let ts = emit(
            "module x\npub type User = { id: string, age: number, admin?: bool, parent: User }\n",
        );
        assert!(ts.contains("export const User = {"), "{ts}");
        assert!(ts.contains("is(value: unknown): value is User {"), "{ts}");
        assert!(
            ts.contains("typeof (value as Record<string, unknown>).id === \"string\""),
            "{ts}"
        );
        assert!(
            ts.contains("typeof (value as Record<string, unknown>).age === \"number\""),
            "{ts}"
        );
        // Optional field: passes when absent, where absent is either JSON
        // spelling — the key omitted, or the key holding `null`.
        assert!(
            ts.contains("(!((value as Record<string, unknown>).admin !== undefined && (value as Record<string, unknown>).admin !== null) || typeof (value as Record<string, unknown>).admin === \"boolean\")"),
            "{ts}"
        );
        // Nested record field: recurses through the field type's descriptor
        // (G4), not a shallow presence check.
        assert!(
            ts.contains("User.is((value as Record<string, unknown>).parent)"),
            "{ts}"
        );
    }

    #[test]
    fn record_field_map_is_structurally_validated() {
        // BUG-2: a `Record<K, V>` field must be checked for object-ness (and its
        // value type recursed over every entry), not merely presence-checked, so
        // a string can never bind where a `Record<string, number>` is required.
        let ts = emit("module x\npub type Config = { name: string, limits: Record<string, number> }\n");
        // No shallow `!== undefined` presence check for the Record field.
        assert!(
            !ts.contains("(value as Record<string, unknown>).limits !== undefined"),
            "Record field must not be presence-checked only: {ts}"
        );
        // Structural object guard plus a per-value recursion into `number`.
        assert!(
            ts.contains("typeof (value as Record<string, unknown>).limits === \"object\""),
            "{ts}"
        );
        assert!(
            ts.contains("!Array.isArray((value as Record<string, unknown>).limits)"),
            "{ts}"
        );
        assert!(
            ts.contains("Object.values((value as Record<string, unknown>).limits as Record<string, unknown>).every((__v: unknown) => typeof __v === \"number\")"),
            "{ts}"
        );
    }

    #[test]
    fn record_descriptor_parse_returns_a_real_result() {
        let ts = emit("module x\npub type User = { id: string }\n");
        // `parse` returns the prelude `Result` under its aliased type name, with
        // an `Issue[]` error (the documented `Result<T, Array<Issue>>` contract).
        assert!(
            ts.contains("parse(value: unknown): __GlyphResult<User, Issue[]> {"),
            "{ts}"
        );
        // It validates field by field, naming the offending field in the issue,
        // and it separates "absent" from "present but the wrong type".
        assert!(
            ts.contains("if ((value as Record<string, unknown>).id === undefined) {"),
            "{ts}"
        );
        assert!(
            ts.contains(
                "__issues.push({ path: [\"id\"], message: \"field `id` is required\", code: \"missing\" });"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "} else if (!(typeof (value as Record<string, unknown>).id === \"string\")) {"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "__issues.push({ path: [\"id\"], message: \"field `id` must be string\", code: \"type\" });"
            ),
            "{ts}"
        );
        // Both arms go through the prelude constructors, so the value carries
        // `map`/`map_err` and is assignable to a declared `Result`.
        assert!(
            ts.contains(
                "return __issues.length === 0 ? __glyph_ok(value as User) : __glyph_err(__issues);"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "return __glyph_err([{ path: [], message: \"expected User (an object)\", code: \"type\" }]);"
            ),
            "{ts}"
        );
        // An array is an object to `typeof`, so it is rejected by name rather
        // than answered with one misleading issue per declared field.
        assert!(
            ts.contains(
                "return __glyph_err([{ path: [], message: \"expected User (an object), got an array\", code: \"type\" }]);"
            ),
            "{ts}"
        );
        // No stale inlined wire format remains.
        assert!(!ts.contains("{ tag: \"Ok\", value: value as User }"), "{ts}");
        assert!(!ts.contains("value: \"expected User\""), "{ts}");
        // Exactly one aliased `std/result` import, carrying both constructors
        // and the type.
        assert!(
            ts.contains(
                "import { Ok as __glyph_ok, Err as __glyph_err, type Result as __GlyphResult } from \"./.glyph-runtime/std/result\";"
            ),
            "{ts}"
        );
        assert_eq!(ts.matches("from \"./.glyph-runtime/std/result\"").count(), 1, "{ts}");
    }

    /// A JSON `null` is absence for an optional field.
    ///
    /// This is the whole of G91's practical half. Every real payload spells a
    /// missing value as `null` rather than by omitting the key — a Discord
    /// gateway frame carries `"s": null` in every HELLO — and the descriptor
    /// rejected it, so the natural spelling of a wire record was unparseable.
    /// `glyph gen openapi` had documented this exact mapping (`nullable` to an
    /// optional field, a literal null treated as absent) while the runtime did
    /// not implement it, so a generated type rejected the payload it was
    /// generated from.
    #[test]
    fn an_optional_field_accepts_a_json_null_as_absent() {
        let ts = emit("module x\npub type P = { id: string, nick?: string }\n");
        assert!(
            ts.contains("(value as Record<string, unknown>).nick !== null"),
            "absence must include an explicit null, got: {ts}"
        );
        // And a required field is unaffected: null there is still a value of the
        // wrong type, not a licence to omit it.
        let req = emit("module x\npub type Q = { id: string }\n");
        assert!(
            req.contains("(value as Record<string, unknown>).id === undefined"),
            "a required field still reports missing on absence, got: {req}"
        );
    }

    #[test]
    fn optional_field_is_never_reported_as_missing() {
        // `f?: T` says absence is legal, so `parse` tests the value only when the
        // key is present and never emits the `"missing"` branch for it.
        //
        // Present means neither omitted nor `null`: JSON spells absence both
        // ways, and a real payload uses the second. `null` is not a value of
        // `T`, so reading it as absence is the only coherent option.
        let ts = emit("module x\npub type P = { id: string, nick?: string }\n");
        assert!(
            ts.contains(
                "if ((value as Record<string, unknown>).nick !== undefined && (value as Record<string, unknown>).nick !== null && !(typeof (value as Record<string, unknown>).nick === \"string\")) {"
            ),
            "{ts}"
        );
        assert!(
            !ts.contains("message: \"field `nick` is required\""),
            "{ts}"
        );
        // The required sibling still gets both branches.
        assert!(ts.contains("message: \"field `id` is required\""), "{ts}");
    }

    #[test]
    fn field_with_a_descriptor_delegates_and_prefixes_the_path() {
        // A field whose type has its own descriptor is validated by that type's
        // `parse`, and its issues arrive with the field name prepended to `path`.
        // That is what carries a refinement's constraint message out to the
        // caller instead of flattening it into one "wrong type" string.
        let ts = emit(
            "module x\npub type Password = string where value.length >= 8\npub type Signup = { password: Password }\n",
        );
        assert!(
            ts.contains("const __r0 = Password.parse((value as Record<string, unknown>).password);"),
            "{ts}"
        );
        assert!(ts.contains("if (__r0.tag === \"Err\") {"), "{ts}");
        assert!(
            ts.contains(
                "__issues.push({ path: [\"password\", ...__i0.path], message: __i0.message, code: __i0.code });"
            ),
            "{ts}"
        );
        // The absent case is still distinguished from the failing one.
        assert!(
            ts.contains("message: \"field `password` is required\", code: \"missing\""),
            "{ts}"
        );
        // No collapsed message survives anywhere.
        assert!(!ts.contains("is missing or has the wrong type"), "{ts}");
    }

    #[test]
    fn leaf_typed_field_keeps_the_flat_check() {
        // The delegation is scoped: a leaf type, an unconstrained type parameter,
        // and an imported type have no descriptor to call, so they keep the
        // inline `field_value_check` and report `code: "type"`.
        let ts = emit("module x\npub type Box<T> = { item: T, label: string }\n");
        assert!(!ts.contains(".parse("), "{ts}");
        assert!(ts.contains("!(__is_T((value as Record<string, unknown>).item))"), "{ts}");
        assert!(
            ts.contains("message: \"field `item` must be T\", code: \"type\""),
            "{ts}"
        );
    }

    #[test]
    fn try_and_a_descriptor_share_one_result_import() {
        // `?` binds `__glyph_err` and so does a descriptor's `parse`; a second
        // import line would redeclare it. One merged line covers both.
        let ts = emit(
            "module x\npub type User = { id: string }\nfn f(r: Result<int, string>) -> Result<int, string> {\n  let v = r?\n  return Ok(v)\n}\n",
        );
        assert_eq!(ts.matches("from \"./.glyph-runtime/std/result\";").count(), 2, "{ts}");
        assert!(
            ts.contains(
                "import { Ok as __glyph_ok, Err as __glyph_err, type Result as __GlyphResult } from \"./.glyph-runtime/std/result\";"
            ),
            "{ts}"
        );
        assert!(ts.matches("__glyph_err").count() > 1, "{ts}");
    }

    #[test]
    fn try_alone_still_imports_only_err() {
        let ts = emit(
            "module x\nfn f(r: Result<int, string>) -> Result<int, string> {\n  let v = r?\n  return Ok(v)\n}\n",
        );
        assert!(
            ts.contains("import { Err as __glyph_err } from \"./.glyph-runtime/std/result\";"),
            "{ts}"
        );
        assert!(!ts.contains("__glyph_ok"), "{ts}");
    }

    #[test]
    fn refinement_and_union_descriptors_parse_to_a_real_result() {
        let ts = emit("module x\npub type Amount = int where value >= 0\n");
        assert!(
            ts.contains("parse(value: unknown): __GlyphResult<Amount, Issue[]> {"),
            "{ts}"
        );
        assert!(ts.contains("? __glyph_ok(value as Amount)"), "{ts}");
        // A value that is not an `int` at all failed the base type, not the
        // refinement, and the message says so.
        assert!(
            ts.contains(
                "return __glyph_err([{ path: [], message: \"expected Amount (int)\", code: \"type\" }]);"
            ),
            "{ts}"
        );
        // The rejection names the constraint, base type and predicate as
        // written, so the message greps back to the declaration.
        assert!(
            ts.contains(
                ": __glyph_err([{ path: [], message: \"expected Amount (int where value >= 0)\", code: \"refinement\" }]);"
            ),
            "{ts}"
        );

        let ts = emit("module x\npub type Shape = | Circle(int) | Square(int)\n");
        assert!(
            ts.contains("parse(value: unknown): __GlyphResult<Shape, Issue[]> {"),
            "{ts}"
        );
        assert!(ts.contains("? __glyph_ok(value)"), "{ts}");
    }

    #[test]
    fn record_descriptor_emits_a_schema_member() {
        let ts = emit("module x\npub type User = { id: string }\n");
        // `T.schema` is a `Schema<T>` built by the prelude factory from both
        // halves of the descriptor, referenced by name in lazy closures since
        // `this` is not the descriptor object inside the object literal. The
        // deep `parse` is what gives `json.parse<T>` the same field paths
        // `T.parse` reports (G68); built from the guard alone the schema could
        // only answer yes or no and every failure read `expected User`.
        assert!(
            ts.contains(
                "schema: __glyph_schema<User>(\"User\", (v): v is User => User.is(v), \
                 (v: unknown): __GlyphResult<User, Issue[]> => User.parse(v)),"
            ),
            "{ts}"
        );
        // The module that emits a descriptor gets the aliased factory import.
        assert!(
            ts.contains("import { schema as __glyph_schema } from \"./.glyph-runtime/std/schema\";"),
            "{ts}"
        );
    }

    #[test]
    fn parse_does_not_shadow_a_record_named_value() {
        // A record literally named `value` collides with the `parse`/`is`
        // parameter. `parse` validates fields directly against the `value`
        // parameter and never calls the descriptor by name, so no shadow arises.
        let ts = emit("module x\npub type value = { id: string }\n");
        assert!(ts.contains("const __issues: Issue[] = [];"), "{ts}");
        assert!(
            ts.contains("(value as Record<string, unknown>).id"),
            "{ts}"
        );
        assert!(!ts.contains("value.is(value)"), "{ts}");
    }

    #[test]
    fn primitive_alias_gets_no_descriptor() {
        let ts = emit("module x\npub type Sku = string\n");
        assert!(ts.contains("export type Sku = string;"), "{ts}");
        assert!(!ts.contains("export const Sku"), "{ts}");
    }

    #[test]
    fn generic_record_emits_a_checker_threaded_descriptor() {
        // A generic record now emits a descriptor whose `is`/`parse` take one
        // runtime checker per type parameter, and a `T`-typed field is validated
        // with that checker (not presence). It omits the `schema` member.
        let ts = emit("module x\npub type Box<T> = { value: T }\n");
        assert!(ts.contains("export type Box<T> = { value: T };"), "{ts}");
        assert!(ts.contains("export const Box = {"), "{ts}");
        assert!(
            ts.contains("is<T>(value: unknown, __is_T: (v: unknown) => boolean): value is Box<T> {"),
            "{ts}"
        );
        assert!(
            ts.contains("parse<T>(value: unknown, __is_T: (v: unknown) => boolean):"),
            "{ts}"
        );
        // The `value: T` field is validated by the threaded checker, in both the
        // `is` guard and `parse`'s per-field issue check.
        assert!(
            ts.contains("__is_T((value as Record<string, unknown>).value)"),
            "T-typed field uses the checker: {ts}"
        );
        assert!(
            ts.contains("__issues.push({ path: [\"value\"], message:"),
            "generic parse names the offending field: {ts}"
        );
        // No `schema` member for a generic descriptor.
        assert!(!ts.contains("schema: __glyph_schema<Box"), "{ts}");
    }

    #[test]
    fn generic_descriptor_parse_call_threads_a_checker() {
        // `Box.parse<User>(v)` appends a checker synthesized from `User`.
        let ts = emit(
            "module x\npub type User = { name: string }\npub type Box<T> = { value: T }\npub fn f(v: unknown) -> string {\n  return match Box.parse<User>(v) {\n    Ok(_) => \"ok\",\n    Err(_) => \"no\",\n  }\n}\n",
        );
        assert!(
            ts.contains("Box.parse<User>(v, (__cv: unknown) => User.is(__cv))"),
            "{ts}"
        );
    }

    #[test]
    fn imported_generic_descriptor_parse_call_threads_a_checker() {
        // `Box.parse<User>(v)` where `Box` is imported from another module still
        // appends the checker: the arity resolves through the import's
        // `ImportNamed` symbol and the project-wide registry, not just a
        // module-local scan (which sees the import as arity 0 and would drop it).
        let (module, resolved, types, prelude) = pipeline(
            "module app\nimport boxmod { Box }\npub type User = { name: string }\npub fn f(v: unknown) -> string {\n  return match Box.parse<User>(v) {\n    Ok(_) => \"ok\",\n    Err(_) => \"no\",\n  }\n}\n",
        );
        let mut arities: std::collections::BTreeMap<(String, String), usize> = Default::default();
        arities.insert(("boxmod".to_string(), "Box".to_string()), 1);
        let ctx = EmitContext {
            module_path: "app",
            project_modules: &EMPTY_MODULES,
            record_payload_variants: &EMPTY_VARIANTS,
            generic_descriptor_arities: &arities,
            plain_descriptors: &EMPTY_DESCRIPTORS,
            descriptorless_aliases: &EMPTY_ALIASES,
        };
        let ts = emit_module(&module, &resolved, &types, &prelude, ctx).expect("emit failed");
        assert!(
            ts.contains("Box.parse<User>(v, (__cv: unknown) => User.is(__cv))"),
            "{ts}"
        );
    }

    #[test]
    fn is_pattern_on_a_generic_descriptor_threads_a_checker() {
        // `is Box<User>` narrows via `Box.is<User>(m, checker)`.
        let ts = emit(
            "module x\npub type User = { name: string }\npub type Box<T> = { value: T }\npub fn f(v: unknown) -> string {\n  return match v {\n    is Box<User> => \"box\",\n    else => \"no\",\n  }\n}\n",
        );
        assert!(
            ts.contains("Box.is<User>(") && ts.contains("(__cv: unknown) => User.is(__cv)"),
            "{ts}"
        );
    }

    #[test]
    fn function_typed_field_is_checked_by_typeof_function() {
        // A function-typed field is validated by `typeof === "function"`, not
        // the old presence-only `!== undefined` (which let `run: 5` pass).
        let ts = emit("module x\npub type Handler = { run: fn(x: number) -> number }\n");
        assert!(
            ts.contains("typeof (value as Record<string, unknown>).run === \"function\""),
            "{ts}"
        );
        assert!(
            !ts.contains(".run !== undefined"),
            "function field must not be presence-only: {ts}"
        );
    }

    #[test]
    fn imports_three_forms() {
        let ts = emit(
            "module x\nimport std/result { Ok, Err }\nimport std/io\nimport std/http as h\npub fn noop() -> void { return void }\n",
        );
        assert!(ts.contains("import { Ok, Err } from \"./.glyph-runtime/std/result\";"), "{ts}");
        assert!(ts.contains("import * as io from \"./.glyph-runtime/std/io\";"), "{ts}");
        assert!(ts.contains("import * as h from \"./.glyph-runtime/std/http\";"), "{ts}");
    }

    #[test]
    fn loop_for_and_array_object() {
        let ts = emit(
            "module x\npub fn f(xs: Array<number>) -> void {\n  for x in xs {\n    log(x)\n  }\n  let o = { a: 1, b: 2 }\n  return void\n}\n",
        );
        assert!(ts.contains("for (const x of xs) {"), "{ts}");
        assert!(ts.contains("let o = { a: 1, b: 2 };"), "{ts}");
        assert!(ts.contains("return undefined;"), "{ts}");
    }

    #[test]
    fn break_inside_match_inside_loop_is_labeled() {
        // G2: a `break`/`continue` buried in a `match` (a `switch`) must target
        // the loop, not the switch — otherwise the loop spins forever. The loop
        // is labeled and the jumps carry the label.
        let ts = emit(
            "module x\npub fn f(c: bool) -> void {\n  loop {\n    match c {\n      true => break,\n      false => continue,\n    }\n  }\n  return void\n}\n",
        );
        assert!(ts.contains("__loop0: while (true) {"), "{ts}");
        assert!(ts.contains("break __loop0;"), "{ts}");
        assert!(ts.contains("continue __loop0;"), "{ts}");
    }

    #[test]
    fn break_directly_in_loop_body_is_not_labeled() {
        // A `break` directly in the loop body (not inside a `match`) breaks the
        // loop already, so no label is emitted.
        let ts = emit(
            "module x\npub fn f() -> void {\n  loop {\n    log(1)\n    break\n  }\n  return void\n}\n",
        );
        assert!(ts.contains("while (true) {"), "{ts}");
        assert!(!ts.contains("__loop"), "should not label a plain loop:\n{ts}");
        assert!(ts.contains("break;"), "{ts}");
    }

    #[test]
    fn break_in_match_in_for_loop_labels_the_for() {
        // The same applies to `for` loops.
        let ts = emit(
            "module x\npub fn f(xs: Array<bool>) -> void {\n  for c in xs {\n    match c {\n      true => break,\n      false => log(0),\n    }\n  }\n  return void\n}\n",
        );
        assert!(ts.contains("__loop0: for (const c of xs) {"), "{ts}");
        assert!(ts.contains("break __loop0;"), "{ts}");
    }

    #[test]
    fn relative_specifier_handles_nesting() {
        // Same directory.
        assert_eq!(relative_specifier("a", "b"), "./b");
        assert_eq!(relative_specifier("sub/a", "sub/b"), "./b");
        // Into a subdirectory.
        assert_eq!(relative_specifier("a", "sub/b"), "./sub/b");
        // Up and over.
        assert_eq!(relative_specifier("sub/a", "top"), "../top");
        assert_eq!(relative_specifier("x/y/a", "x/z/b"), "../z/b");
        assert_eq!(relative_specifier("x/y/a", "top"), "../../top");
    }

    #[test]
    fn project_sibling_import_emits_a_relative_specifier() {
        // A project module import becomes a relative specifier; `std/*` stays
        // bare (tsconfig-mapped) and an unknown module stays bare (external npm).
        let (module, resolved, types, prelude) = pipeline(
            "module a\nimport helpers { greet }\nimport std/io\npub fn f() -> void {\n  return void\n}\n",
        );
        let mut project = std::collections::BTreeSet::new();
        project.insert("a".to_string());
        project.insert("helpers".to_string());
        let ctx = EmitContext {
            module_path: "a",
            project_modules: &project,
            record_payload_variants: &EMPTY_VARIANTS,
            generic_descriptor_arities: &EMPTY_ARITIES,
            plain_descriptors: &EMPTY_DESCRIPTORS,
            descriptorless_aliases: &EMPTY_ALIASES,
        };
        let ts = emit_module(&module, &resolved, &types, &prelude, ctx).expect("emit");
        assert!(ts.contains("from \"./helpers\""), "{ts}");
        assert!(ts.contains("from \"./.glyph-runtime/std/io\""), "{ts}");
    }

    #[test]
    fn record_descriptor_recurses_into_nested_fields() {
        // G4: the `is` guard validates nested records (via `T.is`), array
        // elements (via `.every`), and option payloads (by tag + value type),
        // not just one level.
        let ts = emit(
            "module x\npub type Item = { name: string }\npub type Bag = { items: Array<Item>, note: Option<string> }\npub fn f(b: Bag) -> number {\n  return 0\n}\n",
        );
        assert!(ts.contains(".every((__e: unknown) => Item.is(__e))"), "{ts}");
        assert!(ts.contains("Array.isArray("), "{ts}");
        // Option<string> payload validated by tag and value type.
        assert!(ts.contains(".tag === \"Some\""), "{ts}");
        assert!(ts.contains(".value === \"string\""), "{ts}");
    }

    #[test]
    fn inline_record_field_descriptor_recurses() {
        // A field typed as an inline record validates its nested fields, not
        // just presence (review finding #11).
        let ts = emit(
            "module x\npub type Shape = { name: string, origin: { x: number, y: number } }\npub fn f(s: Shape) -> number {\n  return 0\n}\n",
        );
        assert!(
            ts.contains("typeof ((value as Record<string, unknown>).origin as Record<string, unknown>).x === \"number\""),
            "{ts}"
        );
    }

    #[test]
    fn json_parse_array_routes_through_schema_array() {
        // `json.parse<Array<T>>` validates via `T.schema.array()` (review #12).
        let ts = emit(
            "module x\nimport std/json\npub type User = { name: string }\npub fn load(text: string) -> Result<Array<User>, Array<Issue>> {\n  return json.parse<Array<User>>(text)\n}\n",
        );
        assert!(ts.contains("json.parse_with(text, User.schema.array())"), "{ts}");
    }

    #[test]
    fn json_parse_with_descriptor_routes_through_schema() {
        // G3: `json.parse<T>(text)` for a type with a descriptor validates via
        // `T.schema`; a type without one keeps the plain (casting) parse.
        let ts = emit(
            "module x\nimport std/json\npub type User = { name: string }\npub fn load(text: string) -> Result<User, Array<Issue>> {\n  return json.parse<User>(text)\n}\npub fn loose(text: string) -> Result<number, Array<Issue>> {\n  return json.parse<number>(text)\n}\n",
        );
        assert!(ts.contains("json.parse_with(text, User.schema)"), "{ts}");
        assert!(ts.contains("json.parse<number>(text)"), "{ts}");
    }

    #[test]
    fn prelude_values_used_without_import_are_auto_imported() {
        // G7: a module that uses prelude `Ok`/`Result`/`None`/`Option` without an
        // explicit import still needs the runtime `import` in the emitted TS.
        let ts = emit(
            "module x\npub fn f(n: number) -> Result<number, string> {\n  return Ok(n)\n}\npub fn g() -> Option<number> {\n  return None\n}\n",
        );
        // `Result` and `Option` carry the inline `type` modifier: the runtime
        // declares them with `export type`, so without it the name survives type
        // stripping and the import fails to link (G114). The constructors are
        // real values and stay bare.
        assert!(ts.contains("import { Ok, type Result } from \"./.glyph-runtime/std/result\";"), "{ts}");
        assert!(ts.contains("import { None, type Option } from \"./.glyph-runtime/std/option\";"), "{ts}");
    }

    #[test]
    fn explicitly_imported_prelude_values_are_not_double_imported() {
        // The explicit import resolves to a module symbol, not the prelude, so
        // no second generated import is injected.
        let ts = emit(
            "module x\nimport std/result { Result, Ok }\npub fn f(n: number) -> Result<number, string> {\n  return Ok(n)\n}\n",
        );
        // Exactly one import line mentioning std/result (the user's), no injected one.
        assert_eq!(ts.matches("from \"./.glyph-runtime/std/result\"").count(), 1, "{ts}");
    }

    #[test]
    fn await_on_a_method_chain_awaits_the_head_call() {
        // Glyph async is colorless: `await load().map_err(id)` must await the
        // async call `load()`, not the whole chain, so the chained `.map_err`
        // runs on the awaited `Result` and not on a `Promise`.
        let ts = emit(
            "module x\npub async fn load() -> Result<number, string> { return Ok(0) }\npub fn id(e: string) -> string { return e }\npub async fn run() -> Result<number, string> {\n  let r = await load().map_err(id)\n  return r\n}\n",
        );
        assert!(ts.contains("(await load()).map_err(id)"), "{ts}");
        assert!(!ts.contains("(await load().map_err"), "{ts}");
    }

    #[test]
    fn plain_await_of_a_call_is_unchanged() {
        // A bare `await f()` still awaits the call directly.
        let ts = emit(
            "module x\npub async fn f() -> number { return 1 }\npub async fn run() -> number {\n  return await f()\n}\n",
        );
        assert!(ts.contains("return (await f());"), "{ts}");
    }

    #[test]
    fn await_on_a_fluent_value_chain_awaits_the_whole_chain() {
        // The inverse of the Result idiom: a fluent API whose synchronous call
        // (`find`) precedes the async terminal (`to_array`). `cursor` is a value,
        // not a namespace, so the whole chain is awaited (JS semantics), not the
        // inner `find`. Awaiting `find` would leave a `Promise` for `to_array`.
        let ts = emit(
            "module x\npub async fn run(cursor: unknown) -> void {\n  let docs = await cursor.find(0).to_array()\n  return void\n}\n",
        );
        assert!(
            ts.contains("(await cursor.find(0).to_array())"),
            "fluent chain awaits the whole chain: {ts}"
        );
        assert!(!ts.contains("(await cursor.find(0)).to_array"), "{ts}");
    }

    #[test]
    fn await_on_a_namespaced_call_chain_still_awaits_the_head() {
        // A namespaced function call (`http.get(...)`) is the async head, like a
        // bare function call, so a trailing sync combinator still runs on the
        // awaited value: awaits `http.get`, not the whole chain.
        let ts = emit(
            "module x\nimport std/http\npub fn id(e: string) -> string { return e }\npub async fn run() -> void {\n  let r = await http.get(\"u\").map_err(id)\n  return void\n}\n",
        );
        assert!(ts.contains("(await http.get(\"u\")).map_err(id)"), "{ts}");
    }

    #[test]
    fn async_fn_type_emits_a_promise_returning_function_type() {
        // D40: `async fn() -> T` is the type of an async thunk. Without it the
        // only spellable type was `fn() -> T`, which an async value does not fit.
        let ts = emit(
            "module x\npub type Fetched = { url: string }\npub fn task_for(u: string) -> async fn() -> Fetched {\n  return async fn() -> Fetched { return { url: u } }\n}\n",
        );
        assert!(ts.contains("() => Promise<Fetched>"), "{ts}");
    }

    #[test]
    fn async_fn_type_without_a_return_promises_void() {
        let ts = emit(
            "module x\npub fn f(cb: async fn(string)) -> void {\n  return void\n}\n",
        );
        assert!(ts.contains("(a0: string) => Promise<void>"), "{ts}");
    }

    #[test]
    fn plain_fn_type_still_emits_a_bare_return() {
        // The flag defaults to false, so no existing function type changed.
        let ts = emit("module x\npub fn f(cb: fn(string) -> number) -> void {\n  return void\n}\n");
        assert!(ts.contains("(a0: string) => number"), "{ts}");
        assert!(!ts.contains("Promise"), "{ts}");
    }

    #[test]
    fn a_for_over_a_direct_array_range_lowers_to_a_counting_loop() {
        // `array.range(n)` builds an n-element array, so the idiom that reads
        // like a counting loop allocated one per execution and was the slowest
        // of the three ways to scan: 168 ms against `array.filter`'s 72 and a
        // direct `for c in cells`'s 40, over an 81-element scan. It lowers to a
        // counting `for` now, which allocates nothing (G117).
        let ts = emit(
            "module m\n\
             import std/array\n\
             import std/io\n\
             pub fn f(xs: Array<string>) -> void {\n\
             \x20 for i in array.range(array.len(xs)) { io.println(xs[i]) }\n\
             }\n",
        );
        assert!(
            ts.contains("for (let i = 0, ") && ts.contains("; i < ") && ts.contains("i++)"),
            "a direct `array.range` must count, not walk an array; got:\n{ts}"
        );
        assert!(!ts.contains("of array.range("), "got:\n{ts}");
        // The bound is evaluated once, in the initializer, not per iteration.
        assert!(
            ts.matches("array.len(xs)").count() == 1,
            "the bound must be hoisted, not re-evaluated each step; got:\n{ts}"
        );
    }

    #[test]
    fn a_range_that_is_not_iterated_directly_stays_an_array() {
        // Bound to a `let` it is a real array that something else may hold, so
        // the loop keeps walking it. Only the direct call is a counting loop.
        let ts = emit(
            "module m\n\
             import std/array\n\
             import std/io\n\
             pub fn f() -> void {\n\
             \x20 let ns = array.range(3)\n\
             \x20 for i in ns { io.println(number.to_string(i)) }\n\
             }\n",
        );
        assert!(ts.contains("for (const i of ns)"), "got:\n{ts}");
        assert!(!ts.contains("i++"), "got:\n{ts}");
    }

    #[test]
    fn range_from_counts_between_its_two_bounds() {
        let ts = emit(
            "module m\n\
             import std/array\n\
             import std/io\n\
             pub fn f() -> void {\n\
             \x20 for i in array.range_from(2, 7) { io.println(number.to_string(i)) }\n\
             }\n",
        );
        assert!(ts.contains("for (let i = 2, "), "got:\n{ts}");
        assert!(ts.contains("; i < ") && ts.contains("i++)"), "got:\n{ts}");
    }

    #[test]
    fn a_type_only_stdlib_import_carries_the_inline_type_modifier() {
        // `Option` is `export type` in the runtime, so it has no runtime
        // binding. Emitted bare, `tsc` elides it and the build is green, while
        // a type *stripper* leaves it and the import fails to link against a
        // module with no such export (G114). The inline modifier is the
        // spelling a tool with no type information can act on.
        //
        // A Glyph-declared type is not affected and must not be marked: it
        // emits a runtime descriptor `const` under its own name.
        let ts = emit(
            "module m\n\
             import std/option { Option, Some, None }\n\
             pub fn first(xs: Array<string>) -> Option<string> {\n\
             \x20 return match xs { [] => None, [h, ..._r] => Some(h), }\n\
             }\n",
        );
        assert!(
            ts.contains("import { type Option, Some, None } from \"./.glyph-runtime/std/option\";"),
            "the type-only name must carry `type`, the value names must not; got:\n{ts}"
        );
    }

    #[test]
    fn only_the_standard_library_needs_the_type_modifier() {
        // A Glyph-declared type ships a runtime descriptor `const` under its own
        // name, so an import of it has a real binding and must NOT be marked:
        // marking it would elide the import and lose `Point.parse`. Only the
        // hand-written standard library has names with no value behind them,
        // which is why the table is scoped to `std/*`.
        assert!(glyph_resolver::is_stdlib_type_only("std/option", "Option"));
        assert!(glyph_resolver::is_stdlib_type_only("std/result", "Result"));
        assert!(!glyph_resolver::is_stdlib_type_only("std/option", "Some"));
        assert!(!glyph_resolver::is_stdlib_type_only("std/array", "map"));
        // A project module, whatever the name.
        assert!(!glyph_resolver::is_stdlib_type_only("shapes", "Point"));
        assert!(!glyph_resolver::is_stdlib_type_only("shapes", "Option"));
    }

    #[test]
    fn a_default_import_emits_the_form_tsc_demands_for_a_callable_package() {
        // A CommonJS package whose export *is* a function (`module.exports = f`,
        // `export = f` in its `.d.ts`) has nothing else to import. Every other
        // import kind emits a named or namespace import, which `tsc` answers
        // with TS2595 "can only be imported by using a default import" or leaves
        // uncallable. express, lodash, debug, chalk@4 and most of the pre-ESM
        // registry are exactly this shape.
        let ts = emit(
            "module m\n\
             import express { default as express }\n\
             pub fn make() -> unknown { return express() }\n",
        );
        assert!(
            ts.contains("import express from \"express\";"),
            "a default import must emit a default import; got:\n{ts}"
        );
        assert!(
            !ts.contains("import { express }") && !ts.contains("import * as express"),
            "neither of the forms tsc rejects; got:\n{ts}"
        );
    }

    #[test]
    fn a_for_over_an_iterand_of_unknown_type_decides_its_protocol_at_run_time() {
        // An array's pairs bind a NUMBER index, a record's bind a STRING key, so
        // guessing wrong changes what the program computes. This used to default
        // to the record form whenever the checker had not settled the type,
        // which made `index + 1` evaluate `"0" + 1 === "01"` in a build
        // reporting no diagnostics and a clean `tsc --strict`.
        //
        // The iterand here is a generic `parse` with **no** explicit type
        // arguments, which stays `Unknown` on purpose: `parse` takes an
        // `unknown`, so there is nothing to infer an instantiation from and
        // guessing one would put a wrong shape behind a boundary check. Written
        // with type arguments it is typed now, takes the direct array form, and
        // never reaches the helper — which is what the fix to that half did.
        let ts = emit(
            "module m\n\
             type Wire<V> = { keys: Array<string>, values: Array<V> }\n\
             import std/result { Ok, Err }\n\
             import std/io\n\
             fn f(raw: unknown) -> void {\n\
             \x20 match Wire.parse(raw) {\n\
             \x20\x20\x20 Ok(w) => {\n\
             \x20\x20\x20\x20\x20 for i, k in w.keys { io.println(k) }\n\
             \x20\x20\x20 },\n\
             \x20\x20\x20 Err(_e) => void,\n\
             \x20 }\n\
             }\n",
        );
        assert!(
            ts.contains("__glyph_pairs(w.keys)"),
            "an unsettled iterand must decide at run time, not silently take the \
             record form; got:\n{ts}"
        );
        assert!(
            !ts.contains("Object.entries(w.keys)"),
            "the record form binds a string index and is a miscompile here; got:\n{ts}"
        );
    }

    #[test]
    fn a_typed_generic_parse_takes_the_direct_array_form() {
        // The other half: with explicit type arguments the parsed value's shape
        // is known, so the loop needs no run-time decision at all.
        let ts = emit(
            "module m\n\
             type Wire<V> = { keys: Array<string>, values: Array<V> }\n\
             import std/result { Ok, Err }\n\
             import std/io\n\
             fn f(raw: unknown) -> void {\n\
             \x20 match Wire.parse<number>(raw) {\n\
             \x20\x20\x20 Ok(w) => {\n\
             \x20\x20\x20\x20\x20 for i, k in w.keys { io.println(k) }\n\
             \x20\x20\x20 },\n\
             \x20\x20\x20 Err(_e) => void,\n\
             \x20 }\n\
             }\n",
        );
        assert!(ts.contains("w.keys.entries()"), "got:\n{ts}");
        assert!(!ts.contains("__glyph_pairs"), "got:\n{ts}");
    }

    #[test]
    fn a_for_over_a_known_array_or_record_still_emits_directly() {
        // The run-time helper is the fallback, not the new default: a type the
        // checker did settle must keep its direct emit, or every typed loop pays
        // for the one that could not be typed.
        let arr = emit(
            "module m\n\
             import std/io\n\
             fn f(xs: Array<string>) -> void { for i, x in xs { io.println(x) } }\n",
        );
        assert!(arr.contains("xs.entries()"), "got:\n{arr}");
        assert!(!arr.contains("__glyph_pairs"), "got:\n{arr}");

        let rec = emit(
            "module m\n\
             import std/io\n\
             fn f(r: Record<string, string>) -> void { for k, v in r { io.println(v) } }\n",
        );
        assert!(rec.contains("Object.entries(r)"), "got:\n{rec}");
        assert!(!rec.contains("__glyph_pairs"), "got:\n{rec}");
    }

    #[test]
    fn two_binding_for_over_a_stdlib_call_uses_numeric_entries() {
        // The G37 shape: iterating a stdlib call's result directly. Before the
        // call carried a type the iterand was `Unknown` and lowered to
        // `Object.entries`, which binds a STRING index, so the loop counter was
        // silently the wrong type unless the call's result was hoisted into an
        // annotated `let`.
        let ts = emit(
            "module x\nimport std/string\npub fn f(text: string) -> void {\n  for i, raw in string.split(text, \",\") {\n    log(i)\n  }\n  return void\n}\n",
        );
        assert!(
            ts.contains("for (const [i, raw] of string.split(text, \",\").entries()) {"),
            "{ts}"
        );
        assert!(!ts.contains("Object.entries"), "{ts}");
    }

    #[test]
    fn two_binding_for_iterates_record_entries() {
        // `for k, v in rec` over a record lowers to `Object.entries` with an
        // array-destructure binding. This is example 01's `for key, sub_schema
        // in shape` shape.
        let ts = emit(
            "module x\npub fn f(rec: Record<string, number>) -> void {\n  for k, v in rec {\n    log(k)\n    log(v)\n  }\n  return void\n}\n",
        );
        assert!(
            ts.contains("for (const [k, v] of Object.entries(rec)) {"),
            "{ts}"
        );
    }

    #[test]
    fn two_binding_for_over_an_array_uses_numeric_entries() {
        // An array's key/value pairs are `xs.entries()` with a NUMERIC index,
        // not `Object.entries(xs)` (string keys). The iterand type picks the
        // form.
        let ts = emit(
            "module x\npub fn f(xs: Array<string>) -> void {\n  for i, item in xs {\n    log(i)\n  }\n  return void\n}\n",
        );
        assert!(
            ts.contains("for (const [i, item] of xs.entries()) {"),
            "{ts}"
        );
    }

    #[test]
    fn two_binding_for_over_a_let_bound_array_uses_numeric_entries() {
        // A `let`-bound array literal is an inferred (unannotated) array. Its
        // key/value pairs are still `.entries()` with a NUMERIC index, matching
        // a directly-typed `Array<T>` param. Before array-literal type
        // inference the binding typed `Unknown` and misclassified as a record
        // (`Object.entries`, string keys), which miscompiled `match i == 0`.
        let ts = emit(
            "module x\npub fn f() -> void {\n  let xs = [\"a\", \"b\"]\n  for i, item in xs {\n    log(i)\n  }\n  return void\n}\n",
        );
        assert!(
            ts.contains("for (const [i, item] of xs.entries()) {"),
            "{ts}"
        );
        assert!(!ts.contains("Object.entries"), "{ts}");
    }

    #[test]
    fn string_escapes_line_separators_and_controls() {
        // The lexer de-escapes `\u{2028}` to a raw LINE SEPARATOR, which is an
        // unterminated-string error in TS unless re-escaped.
        let ts = emit("module x\npub const s: string = \"a\\u{2028}b\\u{0}c\"\n");
        assert!(ts.contains("\"a\\u2028b\\u0000c\""), "{ts}");
        assert!(!ts.contains('\u{2028}'), "raw U+2028 leaked: {ts}");
    }

    #[test]
    fn empty_object_literal_has_no_double_space() {
        let ts = emit("module x\npub const o = {}\n");
        assert!(ts.contains("export const o = {};"), "{ts}");
    }

    #[test]
    fn return_match_lowers_to_switch_on_tag() {
        let ts = emit(
            "module x\npub fn classify(r: Result<number, string>) -> number {\n  return match r {\n    Ok(value) => value,\n    Err(msg) => 0,\n  }\n}\n",
        );
        assert!(ts.contains("const __m0 = r;"), "{ts}");
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("case \"Ok\": {"), "{ts}");
        assert!(ts.contains("const value = __m0.value;"), "{ts}");
        assert!(ts.contains("return value;"), "{ts}");
        assert!(ts.contains("case \"Err\": {"), "{ts}");
        // No catch-all → an exhaustiveness assertion makes the switch total
        // from TS's view (so the function/arrow provably returns).
        assert!(
            ts.contains("default: throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
    }

    #[test]
    fn try_operator_in_let_unwraps_and_propagates() {
        let ts = emit(
            "module x\npub fn parse(n: number) -> Result<number, string> { return Ok(n) }\npub fn load(n: number) -> Result<number, string> {\n  let x = parse(n)?\n  return Ok(x)\n}\n",
        );
        // A module using `?` gets the aliased `Err` import the re-wrap needs.
        assert!(
            ts.contains("import { Err as __glyph_err } from \"./.glyph-runtime/std/result\";"),
            "{ts}"
        );
        assert!(ts.contains("const __r0 = parse(n);"), "{ts}");
        assert!(
            ts.contains("if (__r0.tag === \"Err\") { return __glyph_err(__r0.value); }"),
            "{ts}"
        );
        assert!(ts.contains("let x = __r0.value;"), "{ts}");
    }

    #[test]
    fn nested_nullary_error_variant_groups_under_one_case() {
        // A Result error union with a nullary variant (`Empty`) beside a payload
        // variant (`BadQty`) must lower to ONE `case "Err"` with an inner switch
        // on the payload tag. The pre-fix miscompile emitted TWO `case "Err"`
        // labels: the first bound the nullary name (`const Empty = __m.value`)
        // and swallowed every `Err`, silently misdispatching or crashing.
        let ts = emit(
            "module x\nimport std/result { Result, Ok, Err }\npub type OrderErr = Empty | BadQty({ sku: string })\npub fn describe(r: Result<number, OrderErr>) -> string {\n  return match r {\n    Ok(v) => \"ok\",\n    Err(Empty) => \"empty\",\n    Err(BadQty({ sku })) => sku,\n  }\n}\n",
        );
        assert_eq!(
            ts.matches("case \"Err\"").count(),
            1,
            "expected a single `case \"Err\"` (not the duplicate-label miscompile):\n{ts}"
        );
        assert!(ts.contains("case \"Empty\""), "{ts}");
        assert!(ts.contains("case \"BadQty\""), "{ts}");
        // The nullary arm must NOT bind the payload as a catch-all.
        assert!(!ts.contains("const Empty ="), "{ts}");
    }

    #[test]
    fn nested_literal_payload_degroups_to_a_value_match() {
        // F4: `Ok(true)`/`Ok(false)` (a nested literal payload) lowers to a
        // single `case "Ok"` whose payload dispatches through an inner
        // value-match, instead of an E0300. A later same-variant catch-all is
        // absorbed into that inner match so it stays exhaustive.
        let ts = emit(
            "module x\nimport std/result { Result, Ok, Err }\npub fn describe(r: Result<bool, string>) -> string {\n  return match r {\n    Ok(true) => \"t\",\n    Ok(false) => \"f\",\n    Err(e) => e,\n  }\n}\n",
        );
        assert_eq!(
            ts.matches("case \"Ok\"").count(),
            1,
            "one `case \"Ok\"` with an inner value switch, not a duplicate:\n{ts}"
        );
        // The payload is bound to a temp and switched on its value.
        assert!(ts.contains("case true:"), "inner value dispatch on the payload:\n{ts}");
    }

    #[test]
    fn nested_literal_absorbs_a_trailing_catch_all() {
        // `Some(0) => .., Some(_) => ..` must fold the wildcard into the inner
        // value-match (a single `case "Some"`), so the payload switch is
        // exhaustive rather than emitting a second, shadowed `case "Some"`.
        let ts = emit(
            "module x\nimport std/option { Option, Some, None }\npub fn classify(o: Option<number>) -> string {\n  return match o {\n    Some(0) => \"zero\",\n    Some(_) => \"nonzero\",\n    None => \"none\",\n  }\n}\n",
        );
        assert_eq!(
            ts.matches("case \"Some\"").count(),
            1,
            "the trailing Some(_) must be absorbed, not a second case:\n{ts}"
        );
        assert!(ts.contains("case 0:"), "inner value switch on 0:\n{ts}");
    }

    #[test]
    fn try_operator_as_statement_propagates_only() {
        let ts = emit(
            "module x\npub fn step() -> Result<number, string> { return Ok(0) }\npub fn run() -> Result<number, string> {\n  step()?\n  return Ok(1)\n}\n",
        );
        assert!(ts.contains("const __r0 = step();"), "{ts}");
        assert!(
            ts.contains("if (__r0.tag === \"Err\") { return __glyph_err(__r0.value); }"),
            "{ts}"
        );
        // A bare `?` statement discards the `Ok` payload: no `= __r0.value`
        // binding (the re-wrap still reads `__r0.value` for the propagated Err).
        assert!(!ts.contains("= __r0.value"), "{ts}");
    }

    #[test]
    fn value_match_switches_on_the_scrutinee() {
        let ts = emit(
            "module x\npub fn sign(n: number) -> string {\n  return match n {\n    0 => \"zero\",\n    1 => \"one\",\n    else => \"many\",\n  }\n}\n",
        );
        assert!(ts.contains("const __m0 = n;"), "{ts}");
        assert!(ts.contains("switch (__m0) {"), "{ts}");
        assert!(ts.contains("case 0: {"), "{ts}");
        assert!(ts.contains("return \"zero\";"), "{ts}");
        assert!(ts.contains("default: {"), "{ts}");
        // Switches on the value, not `.tag`.
        assert!(!ts.contains(".tag"), "{ts}");
    }

    #[test]
    fn bool_value_match_gets_exhaustiveness_default() {
        let ts = emit(
            "module x\npub fn flag(b: bool) -> number {\n  return match b {\n    true => 1,\n    false => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case true: {"), "{ts}");
        assert!(ts.contains("case false: {"), "{ts}");
        assert!(
            ts.contains("default: throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
    }

    #[test]
    fn empty_block_arm_in_a_return_switch_breaks_instead_of_falling_through() {
        // A void-typed `match` whose arm is an empty block `{}` must emit a
        // `break` in its `switch` case; otherwise the case falls through and
        // runs the next arm's body (here: unbounded recursion via `f`).
        let ts = emit(
            "module x\npub fn f(n: number) -> void {\n  return match n >= 3 {\n    true => {},\n    false => f(n + 1),\n  }\n}\n",
        );
        assert!(ts.contains("case true: {"), "{ts}");
        // The empty-block arm breaks out of the switch rather than emitting
        // nothing and falling into `case false`.
        assert!(
            ts.contains("case true: {\n      break;\n    }"),
            "empty-block arm should break, not fall through: {ts}"
        );
        assert!(ts.contains("return f((n + 1));"), "{ts}");
    }

    #[test]
    fn is_match_lowers_to_an_if_chain_and_calls_the_descriptor() {
        let ts = emit(
            "module x\npub type User = { id: string }\npub fn check(v: unknown) -> string {\n  return match v {\n    is string => \"str\",\n    is number => \"num\",\n    is User => \"user\",\n    else => \"other\",\n  }\n}\n",
        );
        // An identifier scrutinee is checked directly (no temporary) so the
        // checks narrow it for the arm bodies.
        assert!(ts.contains("if (typeof v === \"string\") {"), "{ts}");
        assert!(ts.contains("} else if (typeof v === \"number\") {"), "{ts}");
        // The `is User` arm consumes the Q8 record descriptor.
        assert!(ts.contains("} else if (User.is(v)) {"), "{ts}");
        assert!(ts.contains("} else {"), "{ts}");
        assert!(ts.contains("return \"other\";"), "{ts}");
        // It is an if-chain, not a switch; no scrutinee temporary for an ident.
        assert!(!ts.contains("switch"), "{ts}");
        assert!(!ts.contains("const __m0 = v;"), "{ts}");
    }

    #[test]
    fn is_match_without_else_throws() {
        let ts = emit(
            "module x\npub fn f(v: unknown) -> string {\n  return match v {\n    is string => \"s\",\n    is number => \"n\",\n  }\n}\n",
        );
        assert!(
            ts.contains("} else {\n    throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
    }

    #[test]
    fn array_match_lowers_to_a_length_and_element_if_chain() {
        let ts = emit(
            "module x\npub fn f(argv: Array<string>) -> string {\n  return match argv {\n    [] => \"empty\",\n    [\"add\", ...rest] => \"add\",\n    [\"list\", \"--all\"] => \"la\",\n    [\"get\", id] => id,\n    [other, ..._] => other,\n  }\n}\n",
        );
        // Empty array: exact length zero.
        assert!(ts.contains("if (__m0.length === 0) {"), "{ts}");
        // Literal head + `...rest`: a `>=` length check, and `rest` binds slice.
        assert!(
            ts.contains("} else if (__m0.length >= 1 && __m0[0] === \"add\") {"),
            "{ts}"
        );
        assert!(ts.contains("const rest = __m0.slice(1);"), "{ts}");
        // Two fixed literals: exact length and both elements checked.
        assert!(
            ts.contains(
                "} else if (__m0.length === 2 && __m0[0] === \"list\" && __m0[1] === \"--all\") {"
            ),
            "{ts}"
        );
        // Literal head + identifier element: the identifier binds by index.
        assert!(
            ts.contains("} else if (__m0.length === 2 && __m0[0] === \"get\") {"),
            "{ts}"
        );
        assert!(ts.contains("const id = __m0[1];"), "{ts}");
        // Identifier head + wildcard rest: head binds, rest does not.
        assert!(ts.contains("const other = __m0[0];"), "{ts}");
        assert!(!ts.contains("const _ ="), "{ts}");
        // No `_`/`else` arm, so the chain ends with the exhaustiveness throw.
        assert!(
            ts.contains("} else {\n    throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
        // It is an if-chain, not a switch.
        assert!(!ts.contains("switch"), "{ts}");
    }

    #[test]
    fn array_match_with_an_else_arm_omits_the_throw() {
        let ts = emit(
            "module x\npub fn f(argv: Array<string>) -> string {\n  return match argv {\n    [] => \"empty\",\n    else => \"other\",\n  }\n}\n",
        );
        assert!(ts.contains("if (__m0.length === 0) {"), "{ts}");
        assert!(ts.contains("} else {"), "{ts}");
        assert!(ts.contains("return \"other\";"), "{ts}");
        assert!(!ts.contains("non-exhaustive match"), "{ts}");
    }

    #[test]
    fn is_record_and_array_checks() {
        let ts = emit(
            "module x\npub fn f(v: unknown) -> string {\n  return match v {\n    is Array<string> => \"arr\",\n    is Record<string, unknown> => \"obj\",\n    else => \"x\",\n  }\n}\n",
        );
        // Identifier scrutinee is checked directly so it narrows in the arms;
        // `is Array<string>` also element-checks (sound, like the descriptor).
        assert!(
            ts.contains(
                "if (Array.isArray(v) && (v as ReadonlyArray<unknown>).every((__e: unknown) => typeof __e === \"string\")) {"
            ),
            "{ts}"
        );
        // `is Record` excludes arrays so an `is Array` arm isn't shadowed, and
        // is a type-predicate IIFE so the scrutinee narrows to the record type
        // (indexable), not just `{}`.
        assert!(
            ts.contains(
                "} else if (((__x: unknown): __x is Record<string, unknown> => typeof __x === \"object\" && __x !== null && !Array.isArray(__x))(v)) {"
            ),
            "{ts}"
        );
    }

    #[test]
    fn is_match_with_two_catch_alls_is_rejected() {
        let err = emit_err(
            "module x\npub fn f(v: unknown) -> number {\n  return match v {\n    is string => 1,\n    else => 2,\n    else => 3,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("catch-all")),
            "got {err:?}"
        );
    }

    #[test]
    fn is_check_on_unsupported_type_is_rejected() {
        // A generic union has no runtime descriptor (its type arguments live at
        // the call site), so `is S` over one is still unsupported.
        let err = emit_err(
            "module x\npub type S<T> = A | B(T)\npub fn f(v: unknown) -> number {\n  return match v {\n    is S => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("`is` check")),
            "got {err:?}"
        );
    }

    #[test]
    fn union_type_emits_an_is_descriptor() {
        let ts = emit("module x\npub type S = A | B\npub fn f() {}\n");
        assert!(ts.contains("export const S = {"), "{ts}");
        assert!(ts.contains("is(value: unknown): value is S {"), "{ts}");
        // No-payload variants: the tag switch returns true for each.
        assert!(ts.contains("case \"A\": return true;"), "{ts}");
        assert!(ts.contains("case \"B\": return true;"), "{ts}");
    }

    #[test]
    fn union_descriptor_validates_variant_payloads() {
        // A record-payload variant's fields are validated (not just the tag),
        // and a no-payload variant passes on the tag alone.
        let ts = emit("module x\npub type Msg =\n  | Ping\n  | Say({ text: string })\npub fn f() {}\n");
        assert!(ts.contains("case \"Ping\": return true;"), "{ts}");
        assert!(
            ts.contains("case \"Say\": return typeof (value as Record<string, unknown>).text === \"string\";"),
            "{ts}"
        );
    }

    #[test]
    fn union_descriptor_emits_parse_and_schema() {
        let ts = emit("module x\npub type S = A | B\npub fn f() {}\n");
        assert!(ts.contains("parse(value: unknown):"), "{ts}");
        assert!(ts.contains("return this.is(value)"), "{ts}");
        assert!(ts.contains("schema: __glyph_schema<S>(\"S\""), "{ts}");
    }

    #[test]
    fn is_union_type_calls_its_descriptor() {
        let ts = emit(
            "module x\npub type S = A | B\npub fn f(v: unknown) -> number {\n  return match v {\n    is S => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(ts.contains("S.is("), "{ts}");
    }

    #[test]
    fn generic_union_emits_no_descriptor() {
        // The alias and constructors are generic; no `const S = {` descriptor.
        let ts = emit("module x\npub type S<T> = A | B(T)\npub fn f() {}\n");
        assert!(!ts.contains("export const S = {"), "{ts}");
    }

    #[test]
    fn union_descriptor_name_free_guards_self_named_variant() {
        // A variant sharing the union's name would make the descriptor `const`
        // collide with that variant's constructor `const`. (Such a module is
        // already rejected at collection as a duplicate name, so this guard is
        // defensive — exercised directly here rather than through the pipeline.)
        let span = glyph_ast::Span::new(0, 0);
        let collide = [
            UnionVariant { name: "S".into(), payload: None, span },
            UnionVariant { name: "B".into(), payload: None, span },
        ];
        let free = [
            UnionVariant { name: "A".into(), payload: None, span },
            UnionVariant { name: "B".into(), payload: None, span },
        ];
        assert!(!union_descriptor_name_free("S", &collide));
        assert!(union_descriptor_name_free("S", &free));
    }

    #[test]
    fn mixed_literal_and_variant_match_is_rejected() {
        // A literal arm and a variant arm in one match would switch some arms
        // on the value and others on the tag; reject rather than misemit.
        let err = emit_err(
            "module x\npub type S = Idle | Busy\npub fn f(s: S) -> number {\n  return match s {\n    0 => 1,\n    Idle => 2,\n    else => 9,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("mixing")),
            "got {err:?}"
        );
    }

    #[test]
    fn string_value_match_quotes_case_labels() {
        let ts = emit(
            "module x\npub fn parse(s: string) -> number {\n  return match s {\n    \"yes\" => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case \"yes\": {"), "{ts}");
    }

    #[test]
    fn nested_try_in_an_argument_is_hoisted() {
        // A `?` nested inside a call argument hoists its unwrap before the
        // statement and substitutes the `Ok` payload.
        let ts = emit(
            "module x\npub fn p() -> Result<number, string> { return Ok(0) }\npub fn run() -> Result<number, string> {\n  return Ok(p()?)\n}\n",
        );
        assert!(ts.contains("const __r0 = p();"), "{ts}");
        assert!(
            ts.contains("if (__r0.tag === \"Err\") { return __glyph_err(__r0.value); }"),
            "{ts}"
        );
        assert!(ts.contains("return Ok(__r0.value);"), "{ts}");
    }

    #[test]
    fn mid_chain_try_under_await_is_hoisted() {
        // Example 02's shape: `await get(url)?` then `.map_err(f)` on the next
        // line — the `?` is mid-chain (on `get(url)`, before `.map_err`), not the
        // trailing postfix, and not the `?.` optional-chaining token. The `await`
        // is placed on the async head call `get(url)` so the hoisted temp holds
        // the AWAITED `Result` (its `tag`/`value` are real, not a Promise's), and
        // `.map_err` runs on the unwrapped payload with no outer await.
        let ts = emit(
            "module x\npub async fn run(url: string) -> Result<number, string> {\n  let response = await get(url)?\n    .map_err(fn(e) { return e })\n  return Ok(0)\n}\n",
        );
        assert!(ts.contains("const __r0 = (await get(url));"), "{ts}");
        assert!(
            ts.contains("if (__r0.tag === \"Err\") { return __glyph_err(__r0.value); }"),
            "{ts}"
        );
        assert!(ts.contains("__r0.value.map_err"), "{ts}");
        // The chain past the `?` is not re-awaited.
        assert!(!ts.contains("(await __r0.value"), "{ts}");
    }

    #[test]
    fn multiple_tries_in_arguments_hoist_in_evaluation_order() {
        // `s(a()?, b()?)` hoists the left argument's `?` before the right's, so
        // the unwraps run in source order.
        let ts = emit(
            "module x\npub fn a() -> Result<number, string> { return Ok(1) }\npub fn b() -> Result<number, string> { return Ok(2) }\npub fn s(x: number, y: number) -> number { return x }\npub fn f() -> Result<number, string> {\n  return Ok(s(a()?, b()?))\n}\n",
        );
        let i0 = ts.find("const __r0 = a();").expect("r0 hoist");
        let i1 = ts.find("const __r1 = b();").expect("r1 hoist");
        assert!(i0 < i1, "left arg hoists first: {ts}");
        assert!(
            ts.contains("return Ok(s(__r0.value, __r1.value));"),
            "{ts}"
        );
    }

    #[test]
    fn try_inside_an_array_literal_is_hoisted() {
        let ts = emit(
            "module x\npub fn a() -> Result<number, string> { return Ok(1) }\npub fn f() -> Result<Array<number>, string> {\n  return Ok([a()?, a()?])\n}\n",
        );
        assert!(ts.contains("return Ok([__r0.value, __r1.value]);"), "{ts}");
    }

    #[test]
    fn empty_jsx_element_emits_null_props_and_no_children() {
        let ts = emit(
            "module x\nimport react { Component }\npub component V() -> Component {\n  return <div></div>\n}\n",
        );
        assert!(ts.contains("React.createElement(\"div\", null)"), "{ts}");
    }

    #[test]
    fn extern_ts_expression_emits_parenthesized_raw() {
        let ts = emit(
            "module x\npub fn f() -> unknown {\n  return extern_ts(\"Date.now()\")\n}\n",
        );
        assert!(ts.contains("return (Date.now());"), "{ts}");
    }

    #[test]
    fn aliased_literal_union_and_int_fields_are_validated_through_the_alias() {
        // A field typed by a named alias to a string-literal union or `int` gets
        // the same leaf check as the inline type, not a bare presence check.
        let ts = emit(
            "module x\npub type Tier = \"free\" | \"pro\"\npub type Count = int\n\
             type Item = { tier: Tier, qty: Count }\n",
        );
        assert!(
            ts.contains(r#".tier === "free" || (value as Record<string, unknown>).tier === "pro""#),
            "aliased literal-union membership: {ts}"
        );
        assert!(ts.contains("Number.isInteger("), "aliased int check: {ts}");
    }

    #[test]
    fn int_emits_ts_number_with_an_isinteger_check() {
        let ts = emit("module x\npub type Item = { qty: int }\n");
        // `int` is TS `number` (TypeScript has no integer type).
        assert!(ts.contains("qty: number"), "{ts}");
        // The descriptor adds the whole-number check a bare `number` can't.
        assert!(ts.contains("Number.isInteger("), "{ts}");
    }

    #[test]
    fn string_literal_union_emits_ts_union_and_membership_check() {
        let ts = emit(
            "module x\npub type Account = { id: string, tier: \"free\" | \"pro\" }\n",
        );
        // The TS type is the literal union, so tsc enforces the narrowed type.
        assert!(ts.contains(r#"tier: "free" | "pro""#), "{ts}");
        // The descriptor checks membership on the field, not just `typeof`.
        assert!(
            ts.contains(r#".tier === "free" || (value as Record<string, unknown>).tier === "pro""#),
            "{ts}"
        );
    }

    #[test]
    fn inline_structural_union_emits_a_ts_union() {
        // F3: `string | number` in a signature (and as a type argument) emits as
        // a TS union, mapping primitives (`bool` -> `boolean`). A payload-carrying
        // variant only appears in a named `type` declaration (the parser rejects
        // it inline in a type position), so the ty()-level payload guard is
        // defensive and cannot be reached by well-formed input.
        let ts = emit(
            "module x\npub fn seg(p: string | number) -> string {\n  return match p {\n    is string => p,\n    is number => number.to_string(p),\n  }\n}\npub fn f(xs: Array<bool | number>) -> number {\n  return 0\n}\n",
        );
        assert!(ts.contains("p: string | number"), "inline union param:\n{ts}");
        assert!(ts.contains("Array<boolean | number>"), "union in a type arg, bool mapped:\n{ts}");
    }

    #[test]
    fn value_derived_type_emits_typeof_query() {
        let ts = emit(
            "module x\nimport zod { z }\npub type User = z.infer<typeof user_schema>\n",
        );
        assert!(ts.contains("export type User = z.infer<typeof user_schema>;"), "{ts}");
    }

    #[test]
    fn extern_ts_type_emits_raw_typescript() {
        // The type-level escape hatch emits its raw TS verbatim as the aliased
        // type; `tsc` (not Glyph) checks it.
        let ts = emit(
            "module x\npub type User = extern_ts(\"z.infer<typeof user_schema>\")\n",
        );
        assert!(
            ts.contains("export type User = z.infer<typeof user_schema>;"),
            "{ts}"
        );
    }

    #[test]
    fn jsx_prop_spread_lowers_to_an_object_spread() {
        // `{...register("email")}` merges into the props object, alongside the
        // `class` -> `className` remap, exactly like `<input {...register()} />`.
        let ts = emit(
            "module x\nimport react { Component }\npub fn register(n: string) -> unknown { return n }\n\
             component F() -> Component {\n  return <input {...register(\"email\")} class=\"field\" />\n}\n",
        );
        assert!(
            ts.contains("React.createElement(\"input\", { ...register(\"email\"), className: \"field\" })"),
            "{ts}"
        );
    }

    #[test]
    fn nested_jsx_for_inside_if_lowers() {
        // A `<for>` nested inside an `<if>` branch, paired with an `<else>`.
        let ts = emit(
            "module x\nimport react { Component }\npub component V(xs: Array<string>) -> Component {\n  return <ul>\n    <if cond={true}>\n      <for x in={xs}><li>{x}</li></for>\n    </if>\n    <else><p>empty</p></else>\n  </ul>\n}\n",
        );
        assert!(
            ts.contains("(true ? xs.map((x) => React.createElement(\"li\", null, x)) : React.createElement(\"p\", null, \"empty\"))"),
            "{ts}"
        );
    }

    #[test]
    fn let_bound_match_lowers_to_a_flat_switch() {
        // A `match` that is the whole initializer never wraps in an arrow: the
        // binding is declared, the switch assigns it.
        let ts = emit(
            "module x\npub fn f(r: Result<number, string>) -> string {\n  let label = match r {\n    Ok(n) => \"ok\",\n    Err(e) => \"err\",\n  }\n  return label\n}\n",
        );
        assert!(ts.contains("let label;"), "{ts}");
        assert!(!ts.contains("=> {"), "no arrow wrapper:\n{ts}");
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("label = \"ok\";"), "{ts}");
        assert!(ts.contains("return label;"), "{ts}");
    }

    #[test]
    fn annotated_let_bound_match_keeps_its_type_on_the_declaration() {
        let ts = emit(
            "module x\npub fn f(r: Result<number, string>) -> string {\n  let label: string = match r {\n    Ok(n) => \"ok\",\n    Err(e) => \"err\",\n  }\n  return label\n}\n",
        );
        assert!(ts.contains("let label: string;"), "{ts}");
    }

    #[test]
    fn match_nested_in_an_expression_still_wraps_in_an_iife() {
        // Only a `match` that is NOT the whole statement value goes through the
        // value IIFE: here it is a call argument.
        let ts = emit(
            "module x\npub fn f(r: Result<number, string>) -> string {\n  return show(match r {\n    Ok(n) => \"ok\",\n    Err(e) => \"err\",\n  })\n}\n",
        );
        assert!(ts.contains("show((() => {"), "{ts}");
        assert!(ts.contains("return \"ok\";"), "{ts}");
        assert!(ts.contains("})())"), "{ts}");
    }

    #[test]
    fn awaited_arm_in_a_let_bound_match_needs_no_arrow() {
        // Gap 1: the arm's `await` used to land inside a synchronous IIFE
        // (TS1308). The flat switch puts it in the enclosing async function.
        let ts = emit(
            "module x\npub async fn f(flag: bool) -> number {\n  let n = match flag {\n    true => await slow(),\n    false => 0,\n  }\n  return n\n}\n",
        );
        assert!(ts.contains("let n;"), "{ts}");
        assert!(ts.contains("n = (await slow());"), "{ts}");
        assert!(!ts.contains("() => {"), "no arrow wrapper at all:\n{ts}");
    }

    #[test]
    fn awaited_arm_in_a_nested_match_uses_an_async_arrow() {
        // A `match` nested in a larger expression keeps the IIFE, but an `await`
        // in an arm makes it an awaited async arrow rather than a sync one.
        let ts = emit(
            "module x\npub async fn f(flag: bool) -> number {\n  return use_it(match flag {\n    true => await slow(),\n    false => 0,\n  })\n}\n",
        );
        assert!(ts.contains("(await (async () => {"), "{ts}");
        assert!(ts.contains("return (await slow());"), "{ts}");
    }

    #[test]
    fn await_inside_a_lambda_arm_does_not_make_the_wrapper_async() {
        // The lambda carries its own async-ness; hoisting its `await` to the
        // wrapper would be wrong.
        let ts = emit(
            "module x\npub fn f(flag: bool) -> number {\n  return use_it(match flag {\n    true => async fn() -> number { return await slow() },\n    false => async fn() -> number { return 0 },\n  })\n}\n",
        );
        assert!(ts.contains("(() => {"), "{ts}");
        assert!(!ts.contains("(await (async () => {"), "{ts}");
    }

    #[test]
    fn mut_assigned_match_lowers_to_a_flat_switch() {
        // G25: `mut x = match` with a block arm was a hard EmitError while the
        // `let` form worked. It now mirrors `let`: no declaration, the rendered
        // lvalue is the assign target.
        let ts = emit(
            "module x\npub fn f(r: Result<number, string>) -> string {\n  let label = \"\"\n  mut label = match r {\n    Ok(n) => \"ok\",\n    Err(e) => { log(e)\n      \"err\" },\n  }\n  return label\n}\n",
        );
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("label = \"ok\";"), "{ts}");
        assert!(ts.contains("label = \"err\";"), "{ts}");
        assert!(!ts.contains("let label;"), "no re-declaration:\n{ts}");
    }

    #[test]
    fn mut_assigned_match_targets_a_field_lvalue() {
        let ts = emit(
            "module x\npub fn f(s: State, flag: bool) -> void {\n  mut s.count = match flag {\n    true => 1,\n    false => 0,\n  }\n}\n",
        );
        assert!(ts.contains("s.count = 1;"), "{ts}");
        assert!(ts.contains("s.count = 0;"), "{ts}");
    }

    #[test]
    fn empty_array_arm_is_pinned_so_the_binding_is_not_an_evolving_any() {
        // `let xs = match ... { [] => [], ... }`: a bare `[]` assigned to an
        // unannotated `let` makes TypeScript infer an evolving `any[]` and reject
        // every later read (TS7034/TS7005).
        let ts = emit(
            "module x\npub fn f(xs: Array<string>) -> Array<string> {\n  let out = match xs {\n    [] => [],\n    [head, ...rest] => rest,\n  }\n  return out\n}\n",
        );
        assert!(ts.contains("out = [] as never[];"), "{ts}");
        assert!(ts.contains("out = rest;"), "{ts}");
    }

    // Round-18 shadowing: an arm binder that shares a name with the binding the
    // match assigns to.
    const TOK_UNION: &str = "module x\npub type Tok =\n  | TPunct({ text: string })\n  | TWord({ word: string })\n";

    #[test]
    fn destructured_arm_binding_does_not_shadow_the_let_it_assigns() {
        // `const text = __m0.text; text = text;` assigned a `const` to itself and
        // dropped the value. The declaration moves to a synthesized temporary.
        let ts = emit(&format!(
            "{TOK_UNION}pub fn f(t: Tok) -> string {{\n  let text = match t {{\n    TPunct({{ text }}) => text,\n    TWord({{ word }}) => word,\n  }}\n  return text\n}}\n"
        ));
        assert!(!ts.contains("text = text;"), "{ts}");
        assert!(ts.contains("let __a0;"), "{ts}");
        assert!(ts.contains("const text = __m1.text;"), "{ts}");
        assert!(ts.contains("__a0 = text;"), "{ts}");
        assert!(ts.contains("let text = __a0;"), "{ts}");
    }

    #[test]
    fn destructured_arm_binding_does_not_shadow_the_mut_target() {
        let ts = emit(&format!(
            "{TOK_UNION}pub fn f(t: Tok) -> string {{\n  let text = \"\"\n  mut text = match t {{\n    TPunct({{ text }}) => text,\n    TWord({{ word }}) => word,\n  }}\n  return text\n}}\n"
        ));
        assert!(!ts.contains("text = text;"), "{ts}");
        assert!(ts.contains("let __a0;"), "{ts}");
        assert!(ts.contains("__a0 = text;"), "{ts}");
        assert!(ts.contains("text = __a0;"), "{ts}");
    }

    #[test]
    fn block_arm_let_does_not_shadow_the_binding_the_match_assigns() {
        // The same collision through a different door: the arm's own `let` is
        // declared in the case block and would swallow the assignment.
        let ts = emit(&format!(
            "{TOK_UNION}pub fn f(t: Tok) -> string {{\n  let text = match t {{\n    TPunct({{ text: p }}) => p,\n    TWord({{ word }}) => {{\n      let text = word\n      text\n    }},\n  }}\n  return text\n}}\n"
        ));
        assert!(!ts.contains("text = text;"), "{ts}");
        assert!(ts.contains("let __a0;"), "{ts}");
        assert!(ts.contains("let text = __a0;"), "{ts}");
    }

    #[test]
    fn a_non_colliding_let_match_emits_the_unchanged_shape() {
        // Diff stability: only a real collision changes the output. Rebuilding
        // `examples/apps/` against the 0.1.57 binary emits byte-identical TS.
        let ts = emit(&format!(
            "{TOK_UNION}pub fn f(t: Tok) -> string {{\n  let out = match t {{\n    TPunct({{ text }}) => text,\n    TWord({{ word }}) => word,\n  }}\n  return out\n}}\n"
        ));
        assert!(ts.contains("let out;"), "{ts}");
        assert!(ts.contains("out = text;"), "{ts}");
        assert!(!ts.contains("__a0"), "{ts}");
    }

    #[test]
    fn a_renamed_object_field_does_not_count_as_binding_its_key() {
        // `TPunct({ text: p })` binds `p` and never `text`, so a `let text`
        // outside it is not a collision. Checking the key too was a false
        // positive that cost a temporary and, because the temp counter shifts,
        // renumbered every later temporary in the file.
        let ts = emit(&format!(
            "{TOK_UNION}pub fn f(t: Tok) -> string {{\n  let text = match t {{\n    TPunct({{ text: p }}) => p,\n    TWord({{ word }}) => word,\n  }}\n  return text\n}}\n"
        ));
        assert!(ts.contains("let text;"), "{ts}");
        assert!(ts.contains("const p = __m0.text;"), "{ts}");
        assert!(!ts.contains("__a0"), "{ts}");
    }

    #[test]
    fn a_for_binder_in_a_block_arm_is_not_a_collision() {
        // `for i in ...` lowers to `for (const i of ...)`, whose binding is
        // scoped to the loop and cannot reach the case block the assignment sits
        // in. Treating it as a collision cost a needless temporary and block.
        let ts = emit(&format!(
            "{TOK_UNION}pub fn f(t: Tok) -> string {{\n  let i = match t {{\n    TPunct({{ text }}) => text,\n    TWord({{ word }}) => {{\n      for i in [1, 2] {{\n        log(i)\n      }}\n      word\n    }},\n  }}\n  return i\n}}\n"
        ));
        assert!(ts.contains("let i;"), "{ts}");
        assert!(ts.contains("for (const i of [1, 2])"), "{ts}");
        assert!(!ts.contains("__a0"), "{ts}");
    }

    #[test]
    fn a_lone_binding_arm_named_like_the_let_gets_its_own_scope() {
        // The single-arm lowering emits `const <name> = <scrut>;` at the
        // statement's own level, so a temporary alone still leaves two
        // declarations of `text` in one scope (TS2451). The block is what
        // separates them.
        let ts = emit(
            "module x\npub fn f(t: string) -> string {\n  let text = match t {\n    text => text,\n  }\n  return text\n}\n",
        );
        assert!(ts.contains("let __a0;"), "{ts}");
        assert!(ts.contains("  {\n"), "expected a scoping block:\n{ts}");
        assert!(ts.contains("const text = t;"), "{ts}");
        assert!(ts.contains("let text = __a0;"), "{ts}");
    }

    #[test]
    fn an_arm_binding_named_like_the_mut_lvalue_root_is_routed_through_a_temp() {
        // `mut s.count = match ... { Ok(s) => ... }` would assign through the
        // arm's `s`, not the outer one.
        let ts = emit(
            "module x\npub type S = { count: number }\npub fn f(s: S, r: Result<number, string>) -> void {\n  mut s.count = match r {\n    Ok(s) => s,\n    Err(e) => 0,\n  }\n  return void\n}\n",
        );
        assert!(ts.contains("let __a0;"), "{ts}");
        assert!(ts.contains("s.count = __a0;"), "{ts}");
    }

    #[test]
    fn break_in_a_mut_bound_match_arm_labels_the_loop() {
        // The `mut x = match` statement lowering puts the arm's `break` inside a
        // `switch`, so the loop needs a label for the jump to reach it.
        let ts = emit(
            "module x\nimport std/option { Option, Some, None }\npub fn f(xs: Array<Option<string>>) -> string {\n  let found = \"\"\n  for x in xs {\n    mut found = match x {\n      Some(s) => s,\n      None => break,\n    }\n  }\n  return found\n}\n",
        );
        assert!(ts.contains("__loop0: for (const x of xs) {"), "{ts}");
        assert!(ts.contains("break __loop0;"), "{ts}");
    }

    #[test]
    fn break_in_a_let_bound_match_arm_labels_the_loop() {
        let ts = emit(
            "module x\nimport std/option { Option, Some, None }\npub fn f(xs: Array<Option<string>>) -> string {\n  for x in xs {\n    let found = match x {\n      Some(s) => s,\n      None => break,\n    }\n    log(found)\n  }\n  return \"\"\n}\n",
        );
        assert!(ts.contains("__loop0: for (const x of xs) {"), "{ts}");
        assert!(ts.contains("break __loop0;"), "{ts}");
    }

    #[test]
    fn self_referential_mut_match_in_a_loop_has_no_circular_inference() {
        // Gap 2 / TS7024: `mut on = match on { ... }` inside a loop used to emit
        // an untyped IIFE whose inferred return type referenced the variable
        // being inferred. A flat assignment has no inference cycle.
        let ts = emit(
            "module x\npub fn f(xs: Array<number>) -> bool {\n  let on = false\n  for i in xs {\n    mut on = match on {\n      true => false,\n      false => true,\n    }\n  }\n  return on\n}\n",
        );
        assert!(ts.contains("on = false;"), "{ts}");
        assert!(ts.contains("on = true;"), "{ts}");
        assert!(!ts.contains("() => {"), "no arrow wrapper:\n{ts}");
    }

    #[test]
    fn match_object_pattern_binds_spread_fields() {
        let ts = emit(
            "module x\npub type E =\n  | NetworkError({ url: string, status: number })\n  | NotFound({ id: string })\npub fn show(e: E) -> string {\n  return match e {\n    NetworkError({ url, status }) => url,\n    NotFound({ id }) => id,\n  }\n}\n",
        );
        assert!(ts.contains("case \"NetworkError\": {"), "{ts}");
        assert!(ts.contains("const url = __m0.url;"), "{ts}");
        assert!(ts.contains("const status = __m0.status;"), "{ts}");
        assert!(ts.contains("return url;"), "{ts}");
    }

    #[test]
    fn two_match_statements_use_distinct_temporaries() {
        let ts = emit(
            "module x\npub fn f(a: Result<number, string>, b: Result<number, string>) -> number {\n  match a {\n    Ok(x) => log(x),\n    Err(e) => log(e),\n  }\n  return match b {\n    Ok(y) => y,\n    Err(e) => 0,\n  }\n}\n",
        );
        assert!(ts.contains("const __m0 = a;"), "{ts}");
        assert!(ts.contains("const __m1 = b;"), "{ts}");
    }

    #[test]
    fn two_catch_all_arms_are_rejected() {
        // Two `else` arms would emit two `default:` clauses (TS1113).
        let err = emit_err(
            "module x\npub type E =\n  | A({ x: number })\n  | B({ y: number })\npub fn f(e: E) -> number {\n  return match e {\n    A({ x }) => x,\n    else => 1,\n    else => 2,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("catch-all")),
            "got {err:?}"
        );
    }

    #[test]
    fn statement_block_arm_emits_block_statements() {
        let ts = emit(
            "module x\npub type E = A | B\npub fn f(e: E) -> number {\n  match e {\n    A => {\n      let x = 1\n      return x\n    },\n    B => {\n      return 2\n    },\n  }\n  return 0\n}\n",
        );
        assert!(ts.contains("case \"A\": {"), "{ts}");
        assert!(ts.contains("let x = 1;"), "{ts}");
        assert!(ts.contains("return x;"), "{ts}");
        // The block returns, so no dead `break;` is appended after the return.
        assert!(!ts.contains("return x;\n      break;"), "{ts}");
    }

    #[test]
    fn statement_block_arm_without_return_gets_break() {
        let ts = emit(
            "module x\npub type E = A | B\npub fn nop(n: number) -> void { return void }\npub fn f(e: E) -> void {\n  match e {\n    A => {\n      nop(1)\n    },\n    B => {\n      nop(2)\n    },\n  }\n  return void\n}\n",
        );
        assert!(ts.contains("nop(1);"), "{ts}");
        assert!(ts.contains("break;"), "{ts}");
    }

    #[test]
    fn return_match_block_arm_implicitly_returns_its_tail() {
        // A block arm in a `return match` whose last statement is a bare
        // expression implicitly returns that value (like Rust), rather than
        // being rejected for not ending in `return`.
        let ts = emit(
            "module x\npub type E = A | B\npub fn f(e: E) -> number {\n  return match e {\n    A => {\n      let x = 1\n      x\n    },\n    B => 2,\n  }\n}\n",
        );
        assert!(ts.contains("case \"A\": {"), "{ts}");
        assert!(ts.contains("let x = 1;"), "{ts}");
        assert!(ts.contains("return x;"), "{ts}");
        assert!(ts.contains("return 2;"), "{ts}");
    }

    #[test]
    fn function_body_implicitly_returns_its_tail_expression() {
        // A non-void function whose body ends in a bare expression returns that
        // value (implicit tail return). Without this the value is dropped and
        // the function falls off the end, which `tsc --strict` rejects (TS2355).
        let ts = emit("module x\npub fn f() -> number {\n  let y = 1\n  y + 41\n}\n");
        assert!(ts.contains("let y = 1;"), "{ts}");
        assert!(ts.contains("return (y + 41);"), "{ts}");
    }

    #[test]
    fn tail_match_in_a_function_body_returns_each_arm_value() {
        // Example 04's `run` shape: the function body is a bare `match` whose
        // arms end in bare expressions. The match is in tail position, so each
        // arm `return`s its value rather than dropping it.
        let ts = emit(
            "module x\npub type E = A | B\npub fn f(e: E) -> number {\n  match e {\n    A => 0,\n    B => 1,\n  }\n}\n",
        );
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("return 0;"), "{ts}");
        assert!(ts.contains("return 1;"), "{ts}");
    }

    #[test]
    fn void_function_runs_its_tail_for_effect() {
        // A `void` function does not implicitly return; its tail expression
        // runs for effect.
        let ts = emit("module x\npub fn f() -> void {\n  log(1)\n}\n");
        assert!(ts.contains("log(1);"), "{ts}");
        assert!(!ts.contains("return"), "{ts}");
    }

    #[test]
    fn honest_generic_return_is_not_cast() {
        // A function whose return type is just its own generic parameter is
        // checked precisely by tsc — no cast. The pre-0.1.10 blanket cast on
        // any generic return is gone (D28).
        let ts = emit("module x\npub fn id<T>(x: T) -> T { return x }\n");
        assert!(ts.contains("return x;"), "{ts}");
        assert!(!ts.contains("as T"), "honest generic return is cast-free: {ts}");
    }

    #[test]
    fn infer_output_return_carries_the_one_cast_and_alias() {
        // A return type mentioning `infer_output<S>` (D28) is the single case that
        // still casts: the combinator asserts a dynamically-built value matches
        // the shape-derived type. The module also gets the injected mapped-type
        // alias `infer_output` lowers to.
        let ts = emit(
            "module x\npub fn s<Shape: Record<string, Schema<unknown>>>(shape: Shape) -> Schema<infer_output<Shape>> {\n  return shape\n}\n",
        );
        assert!(
            ts.contains("type __GlyphInferOutput<S> = { [K in keyof S]: S[K] extends { parse(input: unknown): infer R } ? (Extract<R, { tag: \"Ok\" }> extends { value: infer V } ? V : never) : never };"),
            "{ts}"
        );
        assert!(
            ts.contains(": Schema<__GlyphInferOutput<Shape>> {"),
            "return type lowers infer_output: {ts}"
        );
        assert!(
            ts.contains("as Schema<__GlyphInferOutput<Shape>>;"),
            "the one boundary cast: {ts}"
        );
    }

    #[test]
    fn bounded_generic_emits_an_extends_clause() {
        // `<T: Bound>` lowers to a TS `extends` clause; tsc enforces the bound.
        let ts = emit(
            "module x\npub type Named = { name: string }\npub fn label<T: Named>(x: T) -> string {\n  return x.name\n}\n",
        );
        assert!(
            ts.contains("export function label<T extends Named>(x: T): string {"),
            "{ts}"
        );
    }

    #[test]
    fn unbounded_generic_has_no_extends() {
        let ts = emit("module x\npub fn id<T>(x: T) -> T { return x }\n");
        assert!(ts.contains("export function id<T>("), "{ts}");
        assert!(!ts.contains("extends"), "no bound, no extends: {ts}");
    }

    #[test]
    fn non_generic_return_is_not_cast() {
        // A non-generic return type is checked precisely, no cast.
        let ts = emit("module x\npub fn f() -> number { return 1 }\n");
        assert!(ts.contains("return 1;"), "{ts}");
        assert!(!ts.contains(" as number"), "{ts}");
    }

    #[test]
    fn a_returned_lambda_body_does_not_inherit_the_infer_output_cast() {
        // The one `infer_output` boundary cast sits on the function's own
        // returned value; a lambda the function returns keeps its own (un-cast)
        // returns — the sub-emitter resets `return_cast`.
        let ts = emit(
            "module x\npub fn mk<Shape: Record<string, Schema<unknown>>>(v: Shape) -> fn() -> infer_output<Shape> {\n  return fn() { v }\n}\n",
        );
        // The function return is cast; the lambda's `v` is not.
        assert!(ts.contains("as () => __GlyphInferOutput<Shape>;"), "{ts}");
        assert!(!ts.contains("v as "), "{ts}");
    }

    #[test]
    fn value_position_block_arm_lowers_to_a_switch() {
        // F5: `let x = match { ... None => return Err(...) }` (a block arm that
        // returns from the function) lowers to a statement `switch` that declares
        // and assigns the binding, not a value IIFE that would capture the
        // `return`.
        let ts = emit(
            "module x\nimport std/result { Result, Ok, Err }\nimport std/option { Option, Some, None }\npub fn f(o: Option<string>) -> Result<string, string> {\n  let x = match o {\n    Some(s) => s,\n    None => return Err(\"none\"),\n  }\n  return Ok(x)\n}\n",
        );
        assert!(ts.contains("let x"), "the binding is declared up front:\n{ts}");
        assert!(ts.contains("switch"), "lowered to a statement switch:\n{ts}");
        assert!(ts.contains("x = s;"), "a value arm assigns the binding:\n{ts}");
        assert!(
            ts.contains("return Err(\"none\");"),
            "the return arm returns from the function, not an arrow:\n{ts}"
        );
    }

    #[test]
    fn bare_variant_match_lowers_to_cases() {
        // With the scrutinee type known, `Idle`/`Busy` are recognized as
        // no-payload variants and become `case` labels (not bindings).
        let ts = emit(
            "module x\npub type S = Idle | Busy\npub fn f(s: S) -> number {\n  return match s {\n    Idle => 0,\n    Busy => 1,\n  }\n}\n",
        );
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("case \"Idle\": {"), "{ts}");
        assert!(ts.contains("case \"Busy\": {"), "{ts}");
    }

    #[test]
    fn mixed_bare_and_payload_variant_match_lowers() {
        // Example 03's SearchState shape: bare `Idle`/`Loading` plus payload
        // `Loaded({ users })` / `Failed({ message })`.
        let ts = emit(
            "module x\npub type State =\n  | Idle\n  | Loading\n  | Loaded({ users: number })\n  | Failed({ message: string })\npub fn show(s: State) -> number {\n  return match s {\n    Idle => 0,\n    Loading => 1,\n    Loaded({ users }) => users,\n    Failed({ message }) => 2,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Idle\": {"), "{ts}");
        assert!(ts.contains("case \"Loaded\": {"), "{ts}");
        assert!(ts.contains("const users = __m0.users;"), "{ts}");
    }

    #[test]
    fn single_name_bind_of_record_payload_binds_the_whole_object() {
        // `Valid(v)` over a record-payload variant must bind `v` to the flat
        // scrutinee object (the fields are spread as `{ tag, ...fields }`), NOT
        // read `.value`, which does not exist. A single-value payload (`Ok(x)`)
        // still reads `.value`.
        let ts = emit(
            "module x\npub type Row =\n  | Valid({ id: string, amount: number })\n  | Invalid({ id: string })\npub fn amt(r: Row) -> number {\n  return match r {\n    Valid(v) => v.amount,\n    Invalid(_) => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Valid\": {"), "{ts}");
        assert!(ts.contains("const v = __m0;"), "{ts}");
        assert!(!ts.contains("const v = __m0.value;"), "{ts}");
    }

    #[test]
    fn nested_single_name_bind_of_record_payload_binds_the_whole_object() {
        // The nested case: `Err(BadQty(b))` over a `Result` whose `E` is a union
        // with a record-payload variant. The inner match's scrutinee is a
        // synthesized temp the TypeMap doesn't know, so without the payload-type
        // side table the bind would wrongly read `.value` off the flat object.
        let ts = emit(
            "module x\nimport std/result { Result, Ok, Err }\npub type OrderError =\n  | Empty\n  | BadQty({ sku: string, qty: number })\npub fn describe(r: Result<number, OrderError>) -> string {\n  return match r {\n    Ok(v) => \"ok\",\n    Err(Empty) => \"empty\",\n    Err(BadQty(b)) => b.sku,\n  }\n}\n",
        );
        assert!(ts.contains("case \"BadQty\": {"), "{ts}");
        // The whole flattened payload object, not a non-existent `.value`.
        assert!(
            ts.contains("const b = __m") && !ts.contains("const b = __m0.value") && !ts.contains("const b = __m1.value") && !ts.contains("const b = __m2.value") && !ts.contains("const b = __m3.value"),
            "expected `const b = __mN;` (whole object), not `.value`:\n{ts}"
        );
    }

    #[test]
    fn single_name_bind_of_single_value_payload_reads_value() {
        // A user variant whose payload is a single (non-record) value stores it
        // under `value`, so `Wrap(v)` must read `.value`.
        let ts = emit(
            "module x\npub type Box =\n  | Wrap(number)\n  | Empty\npub fn f(b: Box) -> number {\n  return match b {\n    Wrap(v) => v,\n    Empty => 0,\n  }\n}\n",
        );
        assert!(ts.contains("const v = __m0.value;"), "{ts}");
    }

    #[test]
    fn nested_constructor_pattern_emits_a_grouped_inner_switch() {
        // Example 02's shape: `Err(NetworkError({ status }))` over `Result<T,
        // FeedError>` dispatches the outer Ok/Err tag, then the Err payload's
        // inner FeedError tag. The three `Err(..)` arms collapse to one outer
        // `case "Err"` carrying an inner switch.
        let ts = emit(
            "module x\npub type FeedError =\n  | NetworkError({ status: number })\n  | DecodeError({ reason: string })\npub fn handle(r: Result<number, FeedError>) -> number {\n  return match r {\n    Ok(v) => v,\n    Err(NetworkError({ status })) => status,\n    Err(DecodeError({ reason })) => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Ok\": {"), "{ts}");
        assert!(ts.contains("case \"NetworkError\": {"), "{ts}");
        assert!(ts.contains("case \"DecodeError\": {"), "{ts}");
        assert!(ts.contains("const status = "), "{ts}");
        // The three `Err(..)` arms collapse to a single outer `case "Err"`.
        assert_eq!(ts.matches("case \"Err\"").count(), 1, "{ts}");
    }

    #[test]
    fn lambda_returns_its_tail_and_infers_unannotated_params() {
        // A lambda yields its tail expression like a function, and an
        // un-annotated parameter emits without a type so TS infers it from the
        // call-site context rather than being pinned to `unknown`.
        let ts = emit(
            "module x\npub fn apply(f: fn(n: number) -> number) -> number { return f(1) }\npub fn use_it() -> number {\n  return apply(fn(n) { n + 1 })\n}\n",
        );
        assert!(ts.contains("(n) => {"), "{ts}");
        assert!(ts.contains("return (n + 1);"), "{ts}");
    }

    #[test]
    fn explicitly_typed_lambda_param_keeps_its_annotation() {
        let ts = emit(
            "module x\npub fn apply(f: fn(n: number) -> number) -> number { return f(1) }\npub fn use_it() -> number {\n  return apply(fn(n: number) { n + 1 })\n}\n",
        );
        assert!(ts.contains("(n: number) => {"), "{ts}");
    }

    #[test]
    fn component_emits_a_react_function_with_create_element() {
        let ts = emit(
            "module x\npub component Greeting(name: string) -> Component {\n  return <div class=\"g\">{name}</div>\n}\n",
        );
        assert!(ts.contains("import * as React from \"react\";"), "{ts}");
        assert!(
            ts.contains("export function Greeting(name: string): Component {"),
            "{ts}"
        );
        assert!(
            ts.contains("return React.createElement(\"div\", { className: \"g\" }, name);"),
            "{ts}"
        );
    }

    #[test]
    fn jsx_fragment_lowers_to_react_fragment() {
        let ts = emit(
            "module x\npub component P(name: string) -> Component {\n  return <>\n    <h1>{name}</h1>\n    <p>{\"body\"}</p>\n  </>\n}\n",
        );
        assert!(ts.contains("import * as React from \"react\";"), "{ts}");
        assert!(
            ts.contains("React.createElement(React.Fragment, null,"),
            "fragment lowers to React.Fragment: {ts}"
        );
        assert!(ts.contains("React.createElement(\"h1\", null, name)"), "{ts}");
    }

    #[test]
    fn member_expression_jsx_name_emits_the_dotted_type() {
        // `<Ctx.Provider value={x}>` — a namespaced component (React Context).
        let ts = emit(
            "module x\npub component T(v: string) -> Component {\n  return <Ctx.Provider value={v}>\n    <span>{\"c\"}</span>\n  </Ctx.Provider>\n}\n",
        );
        assert!(
            ts.contains("React.createElement(Ctx.Provider, { value: v },"),
            "dotted element name emits as the createElement type: {ts}"
        );
    }

    #[test]
    fn jsx_text_keeps_the_space_between_text_and_an_interpolated_expr() {
        // JSX whitespace rules keep a single significant space between text and
        // an `{expr}` child on the same line. Trimming both would render
        // "HelloAlicewelcome"; the space before and after `{name}` must survive.
        let ts = emit(
            "module x\nimport react { Component }\npub component W(name: string) -> Component {\n  return <p>Hello {name} and welcome</p>\n}\n",
        );
        assert!(
            ts.contains("React.createElement(\"p\", null, \"Hello \", name, \" and welcome\")"),
            "{ts}"
        );
    }

    #[test]
    fn jsx_text_keeps_a_bare_dollar_sign_verbatim() {
        // A literal `$` in JSX text (e.g. a price) is ordinary text: JSX uses
        // single-brace `{expr}` interpolation, so `$` carries no meaning and
        // must not be a lex error. It should survive into the emitted text run.
        let ts = emit(
            "module x\nimport react { Component }\npub component Price() -> Component {\n  return <div><span>Cost: $5 today</span></div>\n}\n",
        );
        assert!(
            ts.contains("React.createElement(\"span\", null, \"Cost: $5 today\")"),
            "{ts}"
        );
    }

    #[test]
    fn intrinsic_jsx_class_and_on_event_attrs_map_to_react_dom_props() {
        // On an intrinsic DOM element, Glyph's snake_case idiom must lower to the
        // React DOM prop names, or the handler never wires up and `class` is
        // dropped by React. `class` -> `className`, `on_click` -> `onClick`,
        // `on_input` -> `onInput`.
        let ts = emit(
            "module x\npub component B() -> Component {\n  return <button class=\"x\" on_click={fn() { void }}>hi</button>\n}\n",
        );
        assert!(ts.contains("className: \"x\""), "class not remapped: {ts}");
        assert!(ts.contains("onClick: () =>"), "on_click not remapped: {ts}");
        assert!(!ts.contains("class: "), "verbatim class leaked: {ts}");
        assert!(!ts.contains("on_click:"), "verbatim on_click leaked: {ts}");
    }

    #[test]
    fn component_attrs_are_not_remapped_to_react_dom_props() {
        // On a component (not an intrinsic element), an attribute is a
        // user-defined prop name and must pass through verbatim; remapping
        // `on_select` -> `onSelect` would break the component that reads
        // `props.on_select`.
        let ts = emit(
            "module x\npub type P = { on_select: fn() -> void }\npub component Row(props: P) -> Component { return <button>x</button> }\npub component List() -> Component {\n  return <Row on_select={fn() { void }} />\n}\n",
        );
        assert!(ts.contains("on_select: () =>"), "component prop remapped: {ts}");
        assert!(!ts.contains("onSelect"), "component prop remapped: {ts}");
    }

    #[test]
    fn jsx_match_lowers_to_a_switch_returning_iife() {
        let ts = emit(
            "module x\npub type S =\n  | Idle\n  | Loaded({ items: number })\npub component V(s: S) -> Component {\n  return <match value={s}>\n    <case Idle><p>idle</p></case>\n    <case Loaded bind={items}><p>{items}</p></case>\n  </match>\n}\n",
        );
        assert!(ts.contains("((__v) => { switch (__v.tag) {"), "{ts}");
        assert!(
            ts.contains("case \"Idle\": return React.createElement(\"p\", null, \"idle\");"),
            "{ts}"
        );
        assert!(
            ts.contains("case \"Loaded\": { const items = __v.items; return"),
            "{ts}"
        );
        assert!(ts.contains("})(s)"), "{ts}");
    }

    #[test]
    fn jsx_if_else_lowers_to_a_ternary() {
        let ts = emit(
            "module x\npub component V(flag: bool) -> Component {\n  return <div>\n    <if cond={flag}><p>yes</p></if>\n    <else><p>no</p></else>\n  </div>\n}\n",
        );
        assert!(
            ts.contains("(flag ? React.createElement(\"p\", null, \"yes\") : React.createElement(\"p\", null, \"no\"))"),
            "{ts}"
        );
    }

    #[test]
    fn non_adjacent_else_reports_the_adjacency_rule_not_a_missing_feature() {
        // A sibling element between an `<if>` and its `<else>` breaks the
        // pairing (D6 adjacency rule). The diagnostic must name that rule and
        // must not frame it as an unimplemented feature.
        let err = emit_err(
            "module f\npub component Sep(show: bool) -> Component {\n  return <div>\n    <if cond={show}><span>yes</span></if>\n    <p>middle</p>\n    <else><span>no</span></else>\n  </div>\n}\n",
        );
        assert!(matches!(err, EmitError::MisplacedElse { .. }), "{err:?}");
        assert_eq!(err.code(), "E0301");
        let msg = format!("{err}");
        assert_eq!(msg, "an `<else>` must immediately follow its `<if>`");
        assert!(!msg.contains("not implemented"), "{msg}");
        assert!(err.note().unwrap().contains("immediately following sibling"));
    }

    #[test]
    fn jsx_for_lowers_to_map_with_key_merged() {
        let ts = emit(
            "module x\npub component V(xs: Array<string>) -> Component {\n  return <ul>\n    <for x in={xs} key={x}><li>{x}</li></for>\n  </ul>\n}\n",
        );
        assert!(
            ts.contains("xs.map((x) => React.createElement(\"li\", { key: x }, x))"),
            "{ts}"
        );
    }

    #[test]
    fn nested_match_in_an_arm_tail_lowers_as_a_statement_switch() {
        // Example 04's `main` shape: a `match` that is the whole body of an arm
        // sits in tail position and inherits the arm's termination, lowering as
        // a nested statement `switch` rather than a value IIFE. That is what
        // lets the inner arms use `return`/block bodies; the IIFE path rejects
        // them.
        let ts = emit(
            "module x\npub type C = A | B\npub fn run(c: C) -> number {\n  return match c {\n    A => match c {\n      A => 0,\n      B => 1,\n    },\n    B => 2,\n  }\n}\n",
        );
        // Two match switches (outer + nested) on the scrutinee temporaries, no
        // value IIFE wrapper. (The union descriptor also emits a tag switch;
        // count the `__m`-keyed match switches specifically.)
        assert_eq!(ts.matches("switch (__m").count(), 2, "{ts}");
        assert!(!ts.contains("(() =>"), "{ts}");
        assert!(ts.contains("return 0;") && ts.contains("return 1;"), "{ts}");
    }

    #[test]
    fn prelude_none_lowers_to_a_case_even_when_untyped() {
        // G1: even when the scrutinee type is unknown (`find()` is untyped), the
        // prelude `None` is a fixed runtime tag, so it lowers to `case "None":`,
        // not a binding `default:`. The old behavior bound `const None = __m0`
        // and relied on default fall-through, which broke nested patterns.
        let ts = emit(
            "module x\npub fn f() -> number {\n  return match find() {\n    None => 0,\n    Some(_) => 1,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Some\": {"), "{ts}");
        assert!(ts.contains("case \"None\": {"), "{ts}");
        // No junk binding, and no `default` binding catch-all for a variant.
        assert!(!ts.contains("const None"), "{ts}");
        assert!(!ts.contains("default: {"), "{ts}");
    }

    #[test]
    fn binding_arm_alongside_a_variant_lowers_to_a_default() {
        // A genuine binding catch-all (a non-variant name) alongside a payload
        // variant: the payload `Some(_)` arm stays a `case`, and the bare `rest`
        // binding lowers to a `default:` that binds the scrutinee.
        let ts = emit(
            "module x\npub fn f() -> number {\n  return match find() {\n    Some(_) => 1,\n    rest => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Some\": {"), "{ts}");
        assert!(ts.contains("default: {"), "{ts}");
        assert!(ts.contains("const rest = __m0;"), "{ts}");
        // The binding catch-all is the only `default`; no synthetic throw.
        assert!(!ts.contains("non-exhaustive match"), "{ts}");
    }

    #[test]
    fn lone_binding_arm_binds_the_scrutinee() {
        // A match whose only arm is a binding has no tag to switch on: bind the
        // scrutinee to the name and run the body.
        let ts = emit(
            "module x\npub fn f() -> number {\n  return match find() {\n    other => other,\n  }\n}\n",
        );
        assert!(ts.contains("const other = find();"), "{ts}");
        assert!(!ts.contains("switch"), "{ts}");
    }

    #[test]
    fn two_binding_arms_are_rejected_as_two_catch_alls() {
        // Without scrutinee type information two bare bindings are both
        // catch-alls, which would emit two `default:` clauses; reject instead.
        let err = emit_err(
            "module x\npub fn f() -> number {\n  return match find() {\n    a => 0,\n    b => 1,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("catch-all")),
            "{err:?}"
        );
    }

    #[test]
    fn nested_ok_none_does_not_miscompile() {
        // G1: `Ok(None)` must group with `Ok(Some(x))` under one `case "Ok"` and
        // dispatch the payload by tag — not emit a duplicate `case "Ok"` that
        // binds `None` as a payload and throws on the `None` value at runtime.
        let ts = emit(
            "module x\npub fn f(r: unknown) -> number {\n  return match r {\n    Ok(Some(x)) => x,\n    Ok(None) => 0,\n    Err(_) => -1,\n  }\n}\n",
        );
        // Exactly one outer `case "Ok"` (no duplicate from the un-grouped arm).
        assert_eq!(ts.matches("case \"Ok\":").count(), 1, "{ts}");
        // The inner dispatch handles both Some and None as tags.
        assert!(ts.contains("case \"Some\": {"), "{ts}");
        assert!(ts.contains("case \"None\": {"), "{ts}");
        // `None` is never treated as a binding.
        assert!(!ts.contains("const None"), "{ts}");
    }

    #[test]
    fn wildcard_constructor_arg_binds_nothing() {
        // `Ok(_)` matches the variant and discards its payload: a `case` with
        // no binding, like a no-payload variant.
        let ts = emit(
            "module x\npub type R =\n  | Ok(number)\n  | Bad(string)\npub fn f(r: R) -> string {\n  return match r {\n    Ok(_) => \"ok\",\n    Bad(msg) => msg,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Ok\": {"), "{ts}");
        // No payload binding is emitted for the discarded `_`.
        assert!(!ts.contains("__m0.value;\n      return \"ok\""), "{ts}");
        assert!(ts.contains("const msg = __m0.value;"), "{ts}");
    }

    #[test]
    fn tagged_union_emits_discriminated_union_and_constructors() {
        let ts = emit(
            "module x\npub type SearchState =\n  | Idle\n  | Loaded({ users: number })\n  | Failed({ message: string })\n",
        );
        assert!(ts.contains("export type SearchState ="), "{ts}");
        assert!(ts.contains("| { tag: \"Idle\" }"), "{ts}");
        assert!(
            ts.contains("| { tag: \"Loaded\"; users: number }"),
            "{ts}"
        );
        assert!(
            ts.contains("| { tag: \"Failed\"; message: string };"),
            "{ts}"
        );
        // No-payload variant → const; payload variant → constructor function.
        assert!(
            ts.contains("export const Idle: SearchState = { tag: \"Idle\" };"),
            "{ts}"
        );
        assert!(
            ts.contains("export function Loaded(fields: { users: number }): SearchState { return { ...fields, tag: \"Loaded\" }; }"),
            "{ts}"
        );
    }

    #[test]
    fn payload_field_named_tag_is_rejected() {
        let err = emit_err(
            "module x\npub type T =\n  | V({ tag: string })\n  | W\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("tag")),
            "got {err:?}"
        );
    }

    #[test]
    fn single_line_no_payload_union_emits_consts() {
        let ts = emit("module x\npub type Color = Red | Green | Blue\n");
        assert!(ts.contains("export const Red: Color = { tag: \"Red\" };"), "{ts}");
        assert!(ts.contains("| { tag: \"Blue\" };"), "{ts}");
    }

    #[test]
    fn generic_tagged_union_emits_with_type_params() {
        let ts = emit("module x\npub type Box<T> =\n  | Full({ value: T })\n  | Empty\n");
        assert!(ts.contains("export type Box<T> ="), "{ts}");
        assert!(ts.contains("| { tag: \"Full\"; value: T }"), "{ts}");
        // Payload constructor is generic and returns the applied type.
        assert!(
            ts.contains("export function Full<T>(fields: { value: T }): Box<T> { return { ...fields, tag: \"Full\" }; }"),
            "{ts}"
        );
        // No-payload variant is a `const` widened to `Box<never>`.
        assert!(
            ts.contains("export const Empty: Box<never> = { tag: \"Empty\" };"),
            "{ts}"
        );
    }

    #[test]
    fn generic_union_constructors_are_generic_only_over_used_params() {
        let ts = emit(
            "module x\npub type Either<A, B> =\n  | Left({ a: A })\n  | Right({ b: B })\n  | Neither\n",
        );
        // Each constructor is generic over only the param it uses; the rest
        // are widened to `never` in the return type.
        assert!(
            ts.contains("export function Left<A>(fields: { a: A }): Either<A, never>"),
            "{ts}"
        );
        assert!(
            ts.contains("export function Right<B>(fields: { b: B }): Either<never, B>"),
            "{ts}"
        );
        assert!(
            ts.contains("export const Neither: Either<never, never> = { tag: \"Neither\" };"),
            "{ts}"
        );
    }

    #[test]
    fn match_on_a_generic_union_resolves_bare_variants() {
        let ts = emit(
            "module x\npub type Box<T> =\n  | Full({ value: T })\n  | Empty\npub fn f(b: Box<string>) -> string {\n  return match b {\n    Full({ value }) => value,\n    Empty => \"\",\n  }\n}\n",
        );
        assert!(ts.contains("case \"Full\": {"), "{ts}");
        // `Empty` (a bare no-payload variant) resolves even though the
        // scrutinee type is `Box<string>` (a `Ty::App`).
        assert!(ts.contains("case \"Empty\": {"), "{ts}");
    }

    #[test]
    fn bitwise_operators_emit_and_precede_correctly() {
        // `& | ^ ~` emit verbatim; precedence is JS's (| looser than ^ looser
        // than &, all tighter than && and looser than ==).
        let ts = emit("module x\npub fn f(a: number, b: number) -> number { return a & b | a ^ ~b }\n");
        assert!(ts.contains("(a & b)"), "{ts}");
        assert!(ts.contains("(a ^ (~b))"), "{ts}");
        // | is the loosest bitwise, so it wraps the & and ^ subtrees.
        assert!(ts.contains("((a & b) | (a ^ (~b)))"), "{ts}");
    }

    #[test]
    fn shift_operators_emit_and_precede_correctly() {
        // `<< >> >>>` emit verbatim. The lexer keeps `<`/`>` single, so these
        // are recognized from adjacent angle tokens in the parser (D36).
        let ts = emit(
            "module x\npub fn f(a: number, b: number, c: number) -> number { return (a << b) + (a >> c) + (a >>> b) }\n",
        );
        assert!(ts.contains("(a << b)"), "{ts}");
        assert!(ts.contains("(a >> c)"), "{ts}");
        assert!(ts.contains("(a >>> b)"), "{ts}");

        // Shift binds tighter than comparison and looser than additive (JS):
        // `a + b << c` is `(a + b) << c`, and `a << b < c` is `(a << b) < c`.
        let prec = emit(
            "module x\npub fn g(a: number, b: number, c: number) -> bool { return a + b << c < a << b }\n",
        );
        assert!(prec.contains("((a + b) << c)"), "additive tighter: {prec}");
        assert!(prec.contains("(a << b)"), "{prec}");
        assert!(
            prec.contains("(((a + b) << c) < (a << b))"),
            "comparison loosest: {prec}"
        );
    }

    // ----- 0.1.16 language features -----

    #[test]
    fn private_decl_omits_export_and_pub_and_main_keep_it() {
        // Module-private by default: a plain `fn`/`type` emits with no `export`.
        // `pub` exports; `fn main` always exports (the runner imports it).
        let ts = emit(
            "module x\nfn helper() -> number { return 1 }\npub fn api() -> number { return 2 }\ntype Local = { a: number }\npub type Wire = { b: number }\nfn main() -> void { return void }\n",
        );
        assert!(ts.contains("function helper("), "{ts}");
        assert!(!ts.contains("export function helper("), "helper is private: {ts}");
        assert!(ts.contains("export function api("), "{ts}");
        assert!(ts.contains("type Local ="), "{ts}");
        assert!(!ts.contains("export type Local ="), "Local is private: {ts}");
        // A private record's runtime descriptor is private too.
        assert!(ts.contains("const Local = {"), "{ts}");
        assert!(!ts.contains("export const Local = {"), "descriptor is private: {ts}");
        assert!(ts.contains("export type Wire ="), "{ts}");
        assert!(ts.contains("export const Wire = {"), "{ts}");
        assert!(ts.contains("export function main("), "main always exports: {ts}");
    }

    #[test]
    fn interface_emits_typescript_interface_and_bound() {
        let ts = emit(
            "module x\npub interface Named {\n  fn name() -> string\n  id: number\n}\npub fn label<T: Named>(x: T) -> string {\n  return x.name()\n}\n",
        );
        assert!(ts.contains("export interface Named {"), "{ts}");
        assert!(ts.contains("name(): string;"), "method signature: {ts}");
        assert!(ts.contains("id: number;"), "property signature: {ts}");
        // The interface is a purely type-level construct — no runtime descriptor.
        assert!(!ts.contains("const Named = {"), "no descriptor for an interface: {ts}");
        // Used as a bound it lowers to a TS `extends` clause.
        assert!(
            ts.contains("export function label<T extends Named>(x: T): string {"),
            "{ts}"
        );
    }

    #[test]
    fn private_interface_omits_export() {
        let ts = emit("module x\ninterface Show {\n  fn show() -> string\n}\n");
        assert!(ts.contains("interface Show {"), "{ts}");
        assert!(!ts.contains("export interface Show"), "private interface: {ts}");
    }

    #[test]
    fn where_refinement_weaves_the_predicate_into_the_descriptor() {
        // D39: `type Amount = int where value >= 0` emits the base alias plus a
        // descriptor whose `is` runs the leaf-check AND the predicate, so a
        // negative value is rejected at the boundary. The base check narrows
        // `value` first, so the predicate sees the base type (tsc-clean).
        let ts = emit("module x\npub type Amount = int where value >= 0\n");
        assert!(ts.contains("export type Amount = number;"), "alias: {ts}");
        assert!(ts.contains("const Amount = {"), "descriptor: {ts}");
        assert!(
            ts.contains(
                "return (typeof value === \"number\" && Number.isInteger(value)) && (value >= 0);"
            ),
            "leaf-check AND predicate: {ts}"
        );
        assert!(ts.contains("parse(value: unknown)"), "has parse: {ts}");
        // A refinement on a record type is a clear error in v1, not a silent drop.
        let err = emit_err("module x\npub type Bad = {\n  x: int,\n} where value.x > 0\n");
        assert!(matches!(err, EmitError::Unsupported { .. }), "record refinement errors: {err:?}");
    }

    #[test]
    fn refined_alias_in_field_position_calls_its_descriptor() {
        // The D39 promise has to hold wherever the type is used, not only at a
        // direct `Instant.parse`. A field typed by a refined alias must call
        // `Instant.is`, which runs the predicate; the old resolver knew only
        // records and unions, so the field fell to `typeof ... === "string"` and
        // `Block.parse({ start: "no" })` returned Ok on data the type rejects.
        let ts = emit(
            "module x\npub type Instant = string where value.length > 3\n\
             pub type Block = {\n  start: Instant,\n  tags: Array<Instant>,\n  note: Option<Instant>,\n}\n",
        );
        assert!(
            ts.contains("Instant.is((value as Record<string, unknown>).start)"),
            "refined field calls the descriptor: {ts}"
        );
        assert!(
            ts.contains("(__e: unknown) => Instant.is(__e)"),
            "refined array element calls the descriptor: {ts}"
        );
        assert!(
            ts.contains(
                "Instant.is((((value as Record<string, unknown>).note) as { value?: unknown }).value)"
            ),
            "refined option payload calls the descriptor: {ts}"
        );
        // The presence floor must be gone for these fields.
        assert!(
            !ts.contains("(value as Record<string, unknown>).start !== undefined"),
            "no presence floor for a refined field: {ts}"
        );
    }

    #[test]
    fn json_parse_of_a_refined_type_uses_its_schema() {
        // `json.parse<Instant>` had no descriptor to find, so it degraded to the
        // casting `parse<T>` and validated nothing at all. It now lowers to the
        // schema form, which runs the predicate.
        let ts = emit(
            "module x\nimport std/json\npub type Instant = string where value.length > 3\n\
             pub fn decode(s: string) -> unknown {\n  return json.parse<Instant>(s)\n}\n",
        );
        assert!(
            ts.contains("json.parse_with(s, Instant.schema)"),
            "refined json.parse uses the descriptor schema: {ts}"
        );
    }

    #[test]
    fn imported_record_in_field_position_calls_its_descriptor() {
        // Every non-generic cross-module record composition used to validate by
        // `!== undefined`: the descriptor resolver scanned only the current
        // module. It now resolves the import through its `ImportNamed` symbol and
        // the project registry, the same way generic descriptors already did.
        let (module, resolved, types, prelude) = pipeline(
            "module app\nimport inner { Inner }\npub type Outer = {\n  i: Inner,\n}\n",
        );
        let mut project = std::collections::BTreeSet::new();
        project.insert("app".to_string());
        project.insert("inner".to_string());
        let mut descriptors = std::collections::BTreeSet::new();
        descriptors.insert(("inner".to_string(), "Inner".to_string()));
        let ctx = EmitContext {
            module_path: "app",
            project_modules: &project,
            record_payload_variants: &EMPTY_VARIANTS,
            generic_descriptor_arities: &EMPTY_ARITIES,
            plain_descriptors: &descriptors,
            descriptorless_aliases: &EMPTY_ALIASES,
        };
        let ts = emit_module(&module, &resolved, &types, &prelude, ctx).expect("emit failed");
        assert!(
            ts.contains("Inner.is((value as Record<string, unknown>).i)"),
            "imported record field calls the descriptor: {ts}"
        );
        assert!(
            !ts.contains("(value as Record<string, unknown>).i !== undefined"),
            "no presence floor for an imported record field: {ts}"
        );
        // Regression guard for the binding the check depends on: `Inner` is used
        // only in type position here, so an "emit `import type` for type-only
        // uses" optimization would erase the value and break `Inner.is` at
        // runtime with no `tsc` complaint. The import must stay a value import.
        assert!(
            ts.contains("import { Inner } from \"./inner\";"),
            "descriptor reference needs a value import, not `import type`: {ts}"
        );
        assert!(
            !ts.contains("import type"),
            "a type-only import would erase the descriptor binding: {ts}"
        );
    }

    #[test]
    fn imported_type_without_a_descriptor_keeps_the_presence_floor() {
        // The registry is the authority: a type the project does not emit a
        // descriptor for (an `interface`, a `.d.ts` type, a bare alias) must not
        // grow a bogus `X.is` call that would be a runtime ReferenceError.
        let (module, resolved, types, prelude) = pipeline(
            "module app\nimport inner { Opaque }\npub type Outer = {\n  o: Opaque,\n}\n",
        );
        let ts = emit_module(&module, &resolved, &types, &prelude, EmitContext::single())
            .expect("emit failed");
        assert!(
            ts.contains("(value as Record<string, unknown>).o !== undefined"),
            "unknown imported type keeps the presence floor: {ts}"
        );
    }

    #[test]
    fn bigint_type_emits_and_validates_distinctly_from_number() {
        let ts = emit("module x\npub type Account = {\n  id: bigint,\n  balance: int,\n}\n");
        assert!(ts.contains("id: bigint;"), "bigint type emits verbatim: {ts}");
        assert!(ts.contains("=== \"bigint\""), "descriptor checks typeof bigint: {ts}");
        // A `123n` literal and a `bigint` return type both emit verbatim.
        let lit = emit("module x\npub fn f() -> bigint {\n  return 123n\n}\n");
        assert!(lit.contains("return 123n;"), "bigint literal verbatim: {lit}");
        assert!(lit.contains("): bigint {"), "bigint return type: {lit}");
    }

    #[test]
    fn new_expression_emits_a_typescript_constructor() {
        // Interop constructor (D37): `new` emits a verbatim TS `new`, callee and
        // args passed through, and a method chains after it because `new Foo()`
        // carries its own parentheses.
        let ts = emit(
            "module x\npub fn go(url: string) -> void {\n  let c = new Client(url)\n  c.ping()\n}\n",
        );
        assert!(ts.contains("new Client(url)"), "verbatim new: {ts}");
        assert!(ts.contains("c.ping();"), "method chains after new: {ts}");
        // A member callee (`pkg.Kafka`) with an object argument, then a chained
        // method on the fresh instance.
        let ts2 = emit(
            "module x\npub fn go2() -> void {\n  let _ = new pkg.Kafka({ id: \"a\", }).producer()\n}\n",
        );
        assert!(
            ts2.contains("new pkg.Kafka({ id: \"a\" }).producer()"),
            "member callee + chain: {ts2}"
        );
    }

    #[test]
    fn defer_lowers_to_try_finally_around_the_tail() {
        // The deferred expression runs on scope exit; the statements after it
        // (including the tail `return`) go inside the `try`.
        let ts = emit(
            "module x\npub fn read() -> string {\n  defer log()\n  return \"r\"\n}\npub fn log() -> void { return void }\n",
        );
        let read = ts.split("function read(").nth(1).unwrap_or("");
        let read = read.split("function log").next().unwrap_or("");
        assert!(read.contains("try {"), "{read}");
        assert!(read.contains("return \"r\";"), "{read}");
        assert!(read.contains("} finally {"), "{read}");
        assert!(read.contains("log();"), "{read}");
        // The return sits before the finally block (inside the try).
        let try_pos = read.find("try {").unwrap();
        let ret_pos = read.find("return \"r\";").unwrap();
        let fin_pos = read.find("} finally {").unwrap();
        assert!(try_pos < ret_pos && ret_pos < fin_pos, "ordering: {read}");
    }

    #[test]
    fn nested_defers_run_last_in_first_out() {
        // Two defers: the second wraps inside the first, so it runs first.
        let ts = emit(
            "module x\npub fn f() -> void {\n  defer a()\n  defer b()\n  return void\n}\npub fn a() -> void { return void }\npub fn b() -> void { return void }\n",
        );
        let f = ts.split("function f(").nth(1).unwrap_or("");
        let f = f.split("function a").next().unwrap_or("");
        // b()'s finally is nested inside a()'s try, so b() runs before a().
        let b_pos = f.find("b();").expect("b in finally");
        let a_pos = f.find("a();").expect("a in finally");
        assert!(b_pos < a_pos, "LIFO: b before a: {f}");
    }

    /// `==` is value equality, so a record or a tagged union compares by
    /// structure rather than by reference.
    ///
    /// `===` gave reference equality the moment either side was an aggregate,
    /// so `Some("a") == Some("a")` was false with no diagnostic, while the
    /// identical expression written as an `@example` compared structurally and
    /// passed. A test reporting success on code that does not work is the worst
    /// thing the example gate can produce.
    #[test]
    fn equality_on_an_aggregate_compares_by_value() {
        let ts = emit(
            "module x\n\
             pub type Point = { x: int, y: int }\n\
             pub fn same(a: Point, b: Point) -> bool { return a == b }\n\
             pub fn differs(a: Point, b: Point) -> bool { return a != b }\n",
        );
        assert!(ts.contains("__glyph_eq(a, b)"), "got: {ts}");
        assert!(ts.contains("(!__glyph_eq(a, b))"), "`!=` negates it, got: {ts}");
    }

    /// Primitives keep `===`. That is most comparisons, so the emitted
    /// TypeScript for ordinary code is unchanged, and the helper is not paid for
    /// where it cannot change the answer.
    #[test]
    fn equality_on_primitives_stays_strict() {
        let ts = emit(
            "module x\n\
             pub fn checks(n: int, s: string, b: bool) -> bool {\n\
             \x20 return n == 1 && s == \"a\" && b == true\n\
             }\n",
        );
        assert!(ts.contains("n === 1"), "got: {ts}");
        assert!(ts.contains("s === \"a\""), "got: {ts}");
        // The `bool` side is pinned to its own type first: `boolean` is a union
        // to TypeScript, so a binding it has narrowed to one member compares
        // against the other as TS2367. See `narrowable_union_ts`. `number` and
        // `string` are not unions and are rendered bare.
        assert!(ts.contains("(b as boolean) === true"), "got: {ts}");
        assert!(!ts.contains("__glyph_eq"), "no helper for primitives, got: {ts}");
    }

    /// A local alias for a string-literal union is still a primitive
    /// comparison, so `tier == "pro"` reads as `===` in the output. The alias
    /// is re-asserted first (`t as Tier`) because a union is what TypeScript
    /// narrows; see `narrowable_union_ts`.
    #[test]
    fn equality_on_a_string_literal_alias_stays_strict() {
        let ts = emit(
            "module x\n\
             pub type Tier = \"free\" | \"pro\"\n\
             pub fn paid(t: Tier) -> bool { return t == \"pro\" }\n",
        );
        assert!(ts.contains("(t as Tier) === \"pro\""), "got: {ts}");
        assert!(!ts.contains("__glyph_eq"), "got: {ts}");
    }

    /// `let _ = expr` discards rather than binding.
    ///
    /// `_` is the spelling the unused-binding lint tells you to use, so a
    /// function that ignores two results writes it twice, and two `const _` in
    /// one scope is a `tsc` redeclaration error naming a variable the author
    /// never meant to declare. A named `_foo` still binds.
    #[test]
    fn a_bare_underscore_binding_discards_instead_of_declaring() {
        let ts = emit(
            "module x\npub fn f() -> void {\n\
             \x20 let _ = g(1)\n\
             \x20 let _ = g(2)\n\
             \x20 let _kept = g(3)\n\
             \x20 return void\n\
             }\npub fn g(n: int) -> int { return n }\n",
        );
        let body = ts.split("function f(").nth(1).unwrap_or("");
        let body = body.split("function g").next().unwrap_or("");
        assert!(
            !body.contains("let _ ") && !body.contains("let _:") && !body.contains("let _="),
            "a bare `_` must not be declared, got: {body}"
        );
        assert!(body.contains("g(1);") && body.contains("g(2);"), "got: {body}");
        assert!(body.contains("let _kept"), "a named `_foo` still binds, got: {body}");
    }

    /// A match arm whose last statement produces no value must still `break`
    /// out of its `switch`, even in return position.
    ///
    /// A lambda body is a value block in return position, so an arm ending in a
    /// `mut` emitted neither a `return` (there is no value) nor a `break`, and
    /// the case ran straight on into the generated
    /// `default: throw new Error("non-exhaustive match")`. The program compiled
    /// clean, passed `tsc --strict`, and threw at run time on a match that was
    /// exhaustive. Found by a Discord bot whose socket callback did exactly
    /// this.
    #[test]
    fn a_valueless_arm_breaks_its_switch_in_return_position() {
        let ts = emit(
            "module x\npub fn go(flag: bool, n: int) -> void {\n\
             \x20 let run = fn(v: bool) {\n\
             \x20   match v {\n\
             \x20     false => {},\n\
             \x20     true => {\n\
             \x20       mut n = n + 1\n\
             \x20     },\n\
             \x20   }\n\
             \x20 }\n\
             \x20 run(flag)\n\
             \x20 return void\n\
             }\n",
        );
        let arm = ts
            .split("n = (n + 1);")
            .nth(1)
            .expect("the valueless arm is emitted");
        let brk = arm.find("break;");
        let dflt = arm.find("default:");
        assert!(
            brk.is_some() && (dflt.is_none() || brk < dflt),
            "the arm must break before reaching `default: throw`, got: {ts}"
        );
    }

    /// The same, one level down: a nested match in an arm body breaks only its
    /// own arms, so the outer case needs its own break whatever the position.
    #[test]
    fn a_nested_match_breaks_the_outer_switch_in_return_position() {
        let ts = emit(
            "module x\npub fn go(flag: bool, n: int) -> void {\n\
             \x20 let run = fn(v: bool) {\n\
             \x20   match v {\n\
             \x20     false => {},\n\
             \x20     true => match v {\n\
             \x20       false => { mut n = n + 1 },\n\
             \x20       true => { mut n = n + 2 },\n\
             \x20     },\n\
             \x20   }\n\
             \x20 }\n\
             \x20 run(flag)\n\
             \x20 return void\n\
             }\n",
        );
        // Two switches; after the inner one closes, the outer case must break
        // rather than fall into the outer `default: throw`.
        let after_inner = ts
            .split("n = (n + 2);")
            .nth(1)
            .expect("the inner arm is emitted");
        let brk = after_inner.find("break;");
        let dflt = after_inner.find("default:");
        assert!(
            brk.is_some(),
            "the outer case must break after the nested switch, got: {ts}"
        );
        // The first `default:` after the inner arm belongs to the inner switch,
        // and the outer break must come before the outer one. Counting breaks
        // is the robust check: inner arm, inner default-guard, outer.
        assert!(
            after_inner.matches("break;").count() >= 2,
            "expected a break for the inner arm and one for the outer case, got: {ts}"
        );
        let _ = dflt;
    }
}
