//! Glyph AST — Phase 1 week 1 (day 1–2 slice).
//!
//! Node categories implemented this slice:
//! - `Module`   top-level container
//! - `Decl`     declarations (import, fn, type, const)
//! - `Stmt`     statements (let, mut, return, expression)
//! - `Expr`     expressions (literal, ident, binary, unary, call, member, ...)
//! - `TypeExpr` type expressions (path, generic, ...)
//! - `Pattern`  patterns (literal, ident, wildcard, ...)  — minimal v0
//! - `Annotation` `@<name>` decoration above a declaration (D27)
//!
//! Every node carries a `Span` reused from `glyph-lexer`. Identifiers use
//! `Arc<str>` (no interning for v0 per `docs/implementation-plan.md §P2`).
//!
//! Deferred to week 1 day 3+:
//! - JSX expressions (D6)
//! - Generic parameters on declarations
//! - Pattern matching (`match` expressions, exhaustive constructor/object/array patterns)
//! - Tagged union type expressions (D8 multi-line / single-line forms)
//! - `loop` / `for` / `break` / `continue` (D21) statement forms
//! - `mut` statement (D5)
//! - `owned` modifier (D25)

#![forbid(unsafe_code)]

use std::sync::Arc;

pub use glyph_lexer::{Comment, Span};

pub type Ident = Arc<str>;

// ============================================================================
// Module
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub module_path: Option<ModulePath>,
    pub items: Vec<Decl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePath {
    pub segments: Vec<Ident>,
    pub span: Span,
}

// ============================================================================
// Declarations
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    Import(ImportDecl),
    Fn(FnDecl),
    Type(TypeDecl),
    Const(ConstDecl),
    Component(ComponentDecl),
    Interface(InterfaceDecl),
}

impl Decl {
    /// The declaration's source span (its whole extent).
    pub fn span(&self) -> Span {
        match self {
            Decl::Import(d) => d.span,
            Decl::Fn(d) => d.span,
            Decl::Type(d) => d.span,
            Decl::Const(d) => d.span,
            Decl::Component(d) => d.span,
            Decl::Interface(d) => d.span,
        }
    }

    /// Whether this declaration is exported from its module (`pub`). Visibility
    /// is module-private by default (0.1.16); an `import` has no visibility of
    /// its own (it re-binds another module's name) and is treated as non-public.
    /// A `fn main` is always exported regardless of `pub` — it is the program
    /// entrypoint the generated runner imports.
    pub fn is_public(&self) -> bool {
        match self {
            Decl::Fn(d) => d.is_public || d.name.as_ref() == "main",
            Decl::Type(d) => d.is_public,
            Decl::Const(d) => d.is_public,
            Decl::Component(d) => d.is_public,
            Decl::Interface(d) => d.is_public,
            Decl::Import(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    /// `import std/http` → `ImportKind::Namespace`
    /// `import std/result { Ok, Err }` → `ImportKind::Named(vec![Ok, Err])`
    /// `import std/http as h` → `ImportKind::Aliased(h)`
    /// `import express { default as app }` → `ImportKind::Default(app)`
    pub path: ModulePath,
    pub kind: ImportKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    Namespace,
    Named(Vec<Ident>),
    Aliased(Ident),
    /// `import express { default as app }`: the module's *default* export bound
    /// to a local name.
    ///
    /// A CommonJS package whose export is a function (`module.exports = f`, so
    /// `export = f` in its `.d.ts`) has nothing else to import: express, lodash,
    /// debug, chalk@4 and most of the pre-ESM registry are entirely this. The
    /// other three forms all emit a named or namespace import, which `tsc`
    /// rejects with TS2595 or leaves uncallable, so those packages were
    /// unreachable.
    ///
    /// The `as` is legal only after `default`, never for an arbitrary name, so
    /// this does not open general named-import renaming: a name in the file
    /// still matches the name at the source, which is what makes an import
    /// greppable. Binding it through the *aliased* form instead was rejected —
    /// that would give one spelling two meanings depending on the package's
    /// module format, which is the defect G111 was fixed to remove.
    Default(Ident),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
    /// `pub fn` (0.1.16). Module-private by default; `main` is always exported.
    pub is_public: bool,
    pub is_async: bool,
    /// Generic type parameters: `fn name<T, U>(args)` produces two `GenericParam`s.
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_ty: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    pub name: Ident,
    /// Constraints (`<T: SomeBound>`). v1 parses a single bound; it lowers to a
    /// TS `extends` clause, which `tsc` enforces. Empty when unbounded.
    pub bounds: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Ident,
    /// D25: an `owned` parameter takes ownership of its argument. Passing an
    /// `owned`-bound resource handle to an `owned` parameter is the single
    /// consume (a move); the binding cannot be used afterward. Non-`owned`
    /// parameters borrow and do not consume.
    pub owned: bool,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
    /// `pub type` (0.1.16). Module-private by default.
    pub is_public: bool,
    pub generics: Vec<GenericParam>,
    /// D25: `resource type X = ...`. A value of a resource type may be bound
    /// with `let owned` and is then tracked for single-consumption. Plain
    /// `type X = ...` leaves this `false`.
    pub is_resource: bool,
    pub body: TypeExpr,
    /// D39: a `where <predicate>` refinement. `type Amount = int where value >= 0`
    /// carries the boolean predicate as an expression over a bound `value`. The
    /// refinement is enforced at the boundary: the type's runtime descriptor
    /// (`is`/`parse`) runs the base leaf-check *and* this predicate, so a value
    /// that fails it is rejected by `.parse`. `None` for an ordinary type.
    pub refinement: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
    /// `pub const` (0.1.16). Module-private by default.
    pub is_public: bool,
    pub ty: Option<TypeExpr>,
    pub value: Expr,
    pub span: Span,
}

/// D19: `component Name(props: T) -> Component { body }`. Grammatically
/// identical to `fn` except for the keyword and the implied JSX-returning
/// body. Return type is optional and defaults to `Component`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
    /// `pub component` (0.1.16). Module-private by default.
    pub is_public: bool,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_ty: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

/// A structural interface (0.1.16): a named set of member signatures usable as a
/// generic bound (`fn label<T: Show>(x: T)`) and as an ordinary type. Structural,
/// like Glyph's records and like a TypeScript `interface`: any value with the
/// members satisfies it, no explicit conformance declaration. Emits to an
/// `export interface`/`interface` in TypeScript, which `tsc` enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
    pub is_public: bool,
    pub generics: Vec<GenericParam>,
    pub members: Vec<InterfaceMember>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceMember {
    /// `fn name(params) -> ret` — a method signature (no body).
    Method {
        name: Ident,
        params: Vec<Param>,
        return_ty: Option<TypeExpr>,
        span: Span,
    },
    /// `name: Type` or `name?: Type` — a property signature.
    Field(RecordTypeField),
}

// ============================================================================
// Annotations (D27)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// `@example`, `@pure`, `@redact`, `@doc`, `@generate`, ...
    /// Unknown annotations are rejected by the typechecker, not by the parser.
    pub name: Ident,
    /// Rest-of-line tokens after `@name`. Annotation-specific parsing happens
    /// in the typechecker pass.
    pub raw_args: String,
    pub span: Span,
}

/// Whether a declaration carries `@open` — the opt-out that lets a record type's
/// runtime descriptor accept a value with keys beyond the declared fields.
/// Without it, a record descriptor's `is`/`parse` rejects a value that has an
/// undeclared key (mass-assignment / leaked-field protection at a boundary).
pub fn is_open_record(annotations: &[Annotation]) -> bool {
    annotations.iter().any(|a| a.name.as_ref() == "open")
}

/// D24: the field names named by a `@redact fields: [a, b]` annotation, if the
/// declaration carries one. The single source of truth for parsing the
/// `fields: [...]` list, shared by the typechecker (which validates the names
/// against the type) and the emitter (which masks those fields in the runtime
/// descriptor). Returns `None` when there is no `@redact`, `Some(vec![])` when
/// the list is empty or malformed. Parsed leniently: everything between the
/// first `[` and the next `]` is split on commas and trimmed.
pub fn redact_fields(annotations: &[Annotation]) -> Option<Vec<String>> {
    let ann = annotations.iter().find(|a| a.name.as_ref() == "redact")?;
    let inner = ann
        .raw_args
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| list);
    let fields = inner
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    Some(fields)
}

// ============================================================================
// Statements & blocks
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let(LetStmt),
    Mut(MutStmt),
    Return(ReturnStmt),
    For(ForStmt),
    Loop(LoopStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Defer(DeferStmt),
    Expr(Expr),
}

/// `defer <expr>` (0.1.16): the expression runs on exit of the enclosing block,
/// on every path (normal completion, `return`, or a thrown error). Lowered to a
/// `try`/`finally` wrapping the statements that follow it; multiple defers in one
/// block run last-in-first-out. Composes with `owned`/`resource` handles for
/// deterministic cleanup (`defer file.close()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferStmt {
    pub expr: Expr,
    pub span: Span,
}

/// D5: `mut` is a statement prefix restricted by the grammar to two shapes:
/// assignment (with optional index/field) and method call. The typechecker
/// does NOT verify method-call mutation (Q7 resolution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutStmt {
    pub kind: MutKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutKind {
    /// `mut <lvalue> = expr`, where the target is an assignable place: a bare
    /// name, or any chain of field accesses and index subscripts bottoming out
    /// at one (`x`, `x.field`, `x[k]`, `x.items[0].name`, `r.a.b`).
    Assign { target: Expr, value: Expr },
    /// `mut x.method(args)` — the typechecker doesn't verify the method
    /// actually mutates (Q7). `call` is the whole method-call expression.
    MethodCall { call: Expr },
}

/// One name in a `for` binding list, with its own def-site span.
///
/// D21's two-binding form `for K, V in expr` needs each name to carry a
/// distinct span: the resolver keys a local binding by its def-site span
/// start, and two bindings sharing one key (the whole statement's span, the
/// only span `ForStmt` used to carry) collapse onto the same entry. That left
/// the typechecker unable to give `V` the iterand's element type in the
/// two-binding form without also mistyping `K` as the element (G37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForBinding {
    pub name: Ident,
    pub span: Span,
}

/// D21: `for X in expr { body }` and the two-binding form
/// `for K, V in expr { body }` (used for iterating record entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForStmt {
    pub bindings: Vec<ForBinding>,
    pub iter: Expr,
    pub body: Block,
    pub span: Span,
}

/// D21: `loop { body }` with break/continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopStmt {
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakStmt {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueStmt {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetStmt {
    pub name: Ident,
    /// `D25 owned` modifier (deferred from this slice; parser will accept the
    /// keyword and set this to `true` in week 1 day 4+).
    pub owned: bool,
    pub ty: Option<TypeExpr>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

// ============================================================================
// Expressions
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Number {
        raw: String,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    /// D22 template literal: `"hello ${name}, count is ${n + 1}"` parses to
    /// a `TemplateString` with alternating `Text` and `Expr` parts.
    ///
    /// **V1 limitation**: literal `${` requires concatenation workaround
    /// because `\${` and `${` lex to the same content. Will be fixed when the
    /// lexer gains a proper template-literal mode (v1.1).
    TemplateString {
        parts: Vec<TemplatePart>,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Void {
        span: Span,
    },
    Ident {
        name: Ident,
        span: Span,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Postfix {
        op: PostfixOp,
        operand: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        /// Explicit type arguments: `json.parse<TodoFile>(text)` produces
        /// `type_args: [Path("TodoFile")]`. Empty for non-generic calls.
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
        span: Span,
    },
    Member {
        object: Box<Expr>,
        field: Ident,
        /// `?.` (D18) vs `.`
        optional: bool,
        span: Span,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Await {
        expr: Box<Expr>,
        span: Span,
    },
    /// Constructor call for an imported/external class: `new Kafka(args)`.
    /// Interop-only. Glyph has no class *definitions*; `new` exists solely to
    /// instantiate a type that comes from an npm package, a `.types` ambient
    /// declaration, or `extern_ts`. It emits `new <callee><type_args>(<args>)`
    /// and is type-checked by `tsc` against the imported constructor, exactly
    /// like an imported function call, so it is a checked seam, not an escape
    /// hatch.
    New {
        callee: Box<Expr>,
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
        span: Span,
    },
    Array {
        elements: Vec<ArrayElem>,
        span: Span,
    },
    /// Object literal: `{ field: expr, ... }`. Shorthand is forbidden per D10
    /// (parser requires the colon).
    Object {
        fields: Vec<ObjectField>,
        span: Span,
    },
    /// Match expression (D3). Each arm is a `MatchArm`. Trailing comma on the
    /// last arm is required by D2; the parser enforces it.
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// Lambda expression: `fn(args) -> T { body }` or `fn(args) { body }`, with
    /// an optional `async` prefix (`async fn(args) { await ... }`). Anonymous
    /// form per D4; body is a block.
    Lambda {
        params: Vec<Param>,
        return_ty: Option<TypeExpr>,
        body: Block,
        is_async: bool,
        span: Span,
    },
    /// JSX element in expression position (D6).
    Jsx(JsxElement),
    /// `extern_ts("<raw TypeScript expression>")` — the expression-level escape
    /// hatch (D29). The raw string is emitted verbatim (parenthesized) and typed
    /// `unknown` at the Glyph seam, so a grammar-hostile runtime idiom stays
    /// reachable without an adapter. Greppable by `extern_ts`.
    Extern {
        raw: String,
        span: Span,
    },
}

/// D6: a JSX element. May be a normal HTML-like element (`<div>`), a
/// component reference (`<UserSearch>`), or a directive (`<if>`, `<else>`,
/// `<for>`, `<match>`, `<case>` — recognized by name; the typechecker
/// treats directives specially).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxElement {
    pub name: Ident,
    pub attrs: Vec<JsxAttr>,
    pub children: Vec<JsxChild>,
    /// `<name ... />` form. When `self_closing` is true, `children` is empty.
    pub self_closing: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxAttr {
    /// `name="literal"`
    String { name: Ident, value: String, span: Span },
    /// `name={expr}`
    Expr { name: Ident, value: Expr, span: Span },
    /// `<case Loaded>` — `Loaded` is a positional attribute (no name, no
    /// value). Allowed before any named attributes (D6).
    Positional { name: Ident, span: Span },
    /// `{...expr}` — spread the object `expr` into the element's props (the
    /// react-hook-form `{...register("name")}` idiom). Lowers to an object
    /// spread inside the `createElement` props.
    Spread { value: Expr, span: Span },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxChild {
    Element(JsxElement),
    /// `{expr}` child.
    Expr(Expr),
    /// Raw text between tags, sliced from the source verbatim. The
    /// typechecker may normalize whitespace; the parser preserves it.
    Text { content: String, span: Span },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplatePart {
    /// Literal text between interpolations. Empty text parts are elided by
    /// the parser; consecutive text parts cannot occur.
    Text { content: String, span: Span },
    /// `${expr}` interpolation. The inner expression is parsed normally;
    /// its span is approximate (mapped into the string's overall span).
    Expr { value: Expr, span: Span },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectField {
    /// `key: expr`. `key` holds the field name verbatim; a key that is not a
    /// valid identifier (e.g. `"Content-Type"`) was written quoted in source and
    /// is re-quoted on output (see `render_object_key`).
    KeyValue { key: Ident, value: Expr, span: Span },
    /// `...expr` (D11)
    Spread { value: Expr, span: Span },
}

/// Whether `s` can be written as a bareword object key (a JS/Glyph identifier
/// name), so it needs no quoting on output.
pub fn is_bareword_key(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Render an object-literal key for emitted or formatted output: bareword when
/// it is a valid identifier, otherwise a double-quoted, escaped string. This is
/// the single source of the canonical form, shared by the emitter and the
/// formatter so a quoted key round-trips identically.
pub fn render_object_key(s: &str) -> String {
    if is_bareword_key(s) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: MatchArmBody,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchArmBody {
    Expr(Expr),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrayElem {
    Expr(Expr),
    Spread(Expr), // ...x  (D11)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Level 10
    NullishCoalesce, // ??
    // Level 9
    LogicalOr, // ||
    // Level 8
    LogicalAnd, // &&
    // Bitwise (between logical and equality, JS precedence): `|` looser than `^`
    // looser than `&`.
    BitOr,  // |
    BitXor, // ^
    BitAnd, // &
    // Level 7
    Eq,    // ==
    NotEq, // !=
    // Level 6
    Lt, // <
    Gt, // >
    LtEq,
    GtEq,
    // Between comparison and additive (JS precedence): shifts bind tighter than
    // `< > <= >=` and looser than `+ -`. The lexer keeps `<`/`>` as single angle
    // tokens (so nested generics close cleanly), so these are recognized in the
    // parser from adjacent angle-token runs.
    Shl,  // <<
    Shr,  // >>
    UShr, // >>>
    // Level 5
    Add,
    Sub,
    // Level 4
    Mul,
    Div,
    Rem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,    // !
    Neg,    // -
    BitNot, // ~
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostfixOp {
    /// `?` Result-propagation postfix (D18).
    Try,
}

// ============================================================================
// Type expressions
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// `string`, `number`, `bool`, `void`, `unknown`, or any user-defined path.
    Path {
        segments: Vec<Ident>,
        span: Span,
    },
    Generic {
        base: Box<TypeExpr>,
        args: Vec<TypeExpr>,
        span: Span,
    },
    /// `fn(x: T) -> U` function type, or `async fn(x: T) -> U` (D40). The
    /// `is_async` flag is `false` for the plain form, so adding it changed no
    /// existing parse tree.
    Fn {
        params: Vec<FnTypeParam>,
        return_ty: Option<Box<TypeExpr>>,
        is_async: bool,
        span: Span,
    },
    /// Inline record type literal: `{ field: type, ... }`.
    Record {
        fields: Vec<RecordTypeField>,
        span: Span,
    },
    /// Tagged union (D8): `A | B({ field: T }) | C`. The parser produces this
    /// from both single-line and multi-line forms.
    Union {
        variants: Vec<UnionVariant>,
        span: Span,
    },
    /// `extern_ts("<raw TypeScript type>")` — the type-level escape hatch. The
    /// raw string is emitted verbatim as the TypeScript type, so an idiom Glyph
    /// cannot spell (a value-derived `z.infer<typeof s>`, a conditional type) is
    /// still nameable. Glyph's own checker treats it as opaque (`unknown`, no
    /// descriptor); `tsc` checks every use of it. Greppable by `extern_ts`.
    Extern {
        raw: String,
        span: Span,
    },
    /// A union of string literals as a type (`"free" | "pro"`). A single literal
    /// (`"free"`) is a one-element union. Emitted as the TypeScript literal union
    /// (`tsc` enforces the narrowed type), and a record field of this type gets a
    /// runtime membership check in its descriptor. Distinct from a D8 tagged
    /// union, whose members are named constructors.
    StringLiteralUnion {
        values: Vec<String>,
        span: Span,
    },
    /// A `typeof value` type query: the type of a value binding, referenced by a
    /// (possibly dotted) path. It emits as TypeScript `typeof <path>` and its
    /// operand is resolved as a real value reference, so it is the first-class,
    /// greppable way to write a value-derived type such as
    /// `type User = z.infer<typeof user_schema>` (a `z.infer<...>` generic over a
    /// `typeof`). Opaque to Glyph's own checker (`tsc` reduces it), like an
    /// imported `.d.ts` type.
    TypeOf {
        path: Vec<Ident>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnTypeParam {
    pub name: Option<Ident>,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordTypeField {
    pub name: Ident,
    pub ty: TypeExpr,
    /// `field?: T` makes the field optional. Currently lexed and accepted; the
    /// typechecker handles semantics.
    pub optional: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionVariant {
    pub name: Ident,
    /// Variants may have no payload, or a payload type expression. The corpus
    /// shows `Name({ field: T })` (a record-typed payload) and `Name` (no
    /// payload). Other type expressions are also legal.
    pub payload: Option<TypeExpr>,
    pub span: Span,
}

// ============================================================================
// Patterns
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `_` — match anything, bind nothing (D9).
    Wildcard {
        span: Span,
    },
    /// `else` — catch-all arm pattern (D9). Only legal as the entire pattern
    /// of a `match` arm; the parser enforces this position.
    Else {
        span: Span,
    },
    /// Identifier binding: `x` binds the matched value to `x`.
    Ident {
        name: Ident,
        span: Span,
    },
    /// Literal pattern: `0`, `"hello"`, `true`, `false`, `void`.
    Literal {
        value: LiteralPattern,
        span: Span,
    },
    /// Variant constructor pattern. Two shapes:
    /// - **With args:** `Ok(x)`, `Err(_)`, `NetworkError({ url, status })`.
    ///   `path` is one or more segments; `args` is non-empty.
    /// - **Bare path:** `fs.ErrorKind.NotFound`. `path` has 2+ segments;
    ///   `args` is empty. Single-segment bare names (`Foo`) are
    ///   `Pattern::Ident` — the typechecker disambiguates "binding `Foo`"
    ///   from "no-payload variant `Foo`" using scrutinee type info.
    Constructor {
        path: Vec<Ident>,
        args: Vec<Pattern>,
        span: Span,
    },
    /// `{ name, email }` — object destructure. The shorthand binds an
    /// identifier of the same name; a field may instead carry any pattern
    /// (`{ name: n }`, `{ color: Black }`, `{ left: Node({ value: v }) }`,
    /// `{ items: [a, b] }`). A field pattern that tests a value makes the whole
    /// object pattern refutable, so it no longer covers the variant it sits
    /// under and the arms after it stay reachable.
    Object {
        fields: Vec<ObjectPatternField>,
        span: Span,
    },
    /// Array pattern (D9 + D11). `[]`, `[head, ...rest]`, `[a, b, c]`,
    /// `["help", ..._]`. `rest` is `None` if there is no `...` element.
    Array {
        elements: Vec<Pattern>,
        /// `Some(rest_pattern)` for `[a, b, ...rest]` style; the rest pattern
        /// is typically `Pattern::Ident` or `Pattern::Wildcard`. `None` if
        /// no `...` element appears.
        rest: Option<Box<Pattern>>,
        span: Span,
    },
    /// `is TypeName` guard pattern. Matches when the runtime descriptor of
    /// the value is compatible with `ty` (Q8 resolution).
    IsType {
        ty: TypeExpr,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralPattern {
    Number(String),
    String(String),
    Bool(bool),
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectPatternField {
    pub key: Ident,
    /// The sub-pattern the field is matched against. `{ name }` → `None` (the
    /// shorthand binds `name` itself). `{ name: alias }` → the renamed binding.
    /// `{ color: Black }`, `{ left: Node({ .. }) }`, `{ items: [a, b] }` → the
    /// nested pattern, which may test the field's value and so can fail.
    pub pattern: Option<Pattern>,
    pub span: Span,
}

impl ObjectPatternField {
    /// The single name this field binds when it is a plain binding: the
    /// shorthand's own key, or the renamed identifier. `None` when the field
    /// carries a structured or value-testing sub-pattern, which binds through
    /// its own sub-patterns instead.
    pub fn bound_name(&self) -> Option<&Ident> {
        match &self.pattern {
            None => Some(&self.key),
            Some(Pattern::Ident { name, .. }) if !is_variant_shaped(name) => Some(name),
            _ => None,
        }
    }

    /// Whether matching this field can fail (see `Pattern::is_refutable`).
    pub fn is_refutable(&self) -> bool {
        self.pattern.as_ref().is_some_and(Pattern::is_refutable)
    }
}

/// Whether a bare identifier in pattern position names a union variant rather
/// than a fresh binding: a PascalCase name is a variant reference (D9).
///
/// The resolver and the typechecker each still carry their own copy of this
/// check, spelled `is_constructor_shaped` (the parser's is gone). Both depend on
/// `glyph-ast`, so this is the home they should collapse into; it is public for
/// that reason.
pub fn is_variant_shaped(name: &Ident) -> bool {
    name.as_ref()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
}

// ============================================================================
// Convenience: span accessors
// ============================================================================

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. }
            | Expr::String { span, .. }
            | Expr::TemplateString { span, .. }
            | Expr::Bool { span, .. }
            | Expr::Void { span, .. }
            | Expr::Ident { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Postfix { span, .. }
            | Expr::Call { span, .. }
            | Expr::Member { span, .. }
            | Expr::Index { span, .. }
            | Expr::Await { span, .. }
            | Expr::New { span, .. }
            | Expr::Array { span, .. }
            | Expr::Object { span, .. }
            | Expr::Match { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::Extern { span, .. } => *span,
            Expr::Jsx(e) => e.span,
        }
    }
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Path { span, .. }
            | TypeExpr::Generic { span, .. }
            | TypeExpr::Fn { span, .. }
            | TypeExpr::Record { span, .. }
            | TypeExpr::Union { span, .. }
            | TypeExpr::Extern { span, .. }
            | TypeExpr::StringLiteralUnion { span, .. }
            | TypeExpr::TypeOf { span, .. } => *span,
        }
    }
}

impl Pattern {
    /// Whether matching this pattern against a value of its own type can fail.
    /// A binding or `_` always matches; a literal, a variant reference, a
    /// constructor, an array pattern and an `is T` guard all test something. An
    /// object pattern is refutable exactly when one of its fields is.
    pub fn is_refutable(&self) -> bool {
        match self {
            Pattern::Wildcard { .. } | Pattern::Else { .. } => false,
            Pattern::Ident { name, .. } => is_variant_shaped(name),
            Pattern::Literal { .. }
            | Pattern::Constructor { .. }
            | Pattern::Array { .. }
            | Pattern::IsType { .. } => true,
            Pattern::Object { fields, .. } => fields.iter().any(ObjectPatternField::is_refutable),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard { span }
            | Pattern::Else { span }
            | Pattern::Ident { span, .. }
            | Pattern::Literal { span, .. }
            | Pattern::Constructor { span, .. }
            | Pattern::Object { span, .. }
            | Pattern::Array { span, .. }
            | Pattern::IsType { span, .. } => *span,
        }
    }
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let(s) => s.span,
            Stmt::Mut(s) => s.span,
            Stmt::Return(s) => s.span,
            Stmt::For(s) => s.span,
            Stmt::Loop(s) => s.span,
            Stmt::Break(s) => s.span,
            Stmt::Continue(s) => s.span,
            Stmt::Defer(s) => s.span,
            Stmt::Expr(e) => e.span(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_construction_compiles() {
        let m = Module {
            module_path: None,
            items: vec![],
            span: Span::new(0, 0),
        };
        assert_eq!(m.items.len(), 0);
    }

    #[test]
    fn bareword_keys_are_recognized() {
        assert!(is_bareword_key("foo"));
        assert!(is_bareword_key("_x9"));
        assert!(is_bareword_key("$ref"));
        assert!(!is_bareword_key("Content-Type"));
        assert!(!is_bareword_key("9lives"));
        assert!(!is_bareword_key(""));
        assert!(!is_bareword_key("a b"));
    }

    #[test]
    fn keys_render_canonically() {
        assert_eq!(render_object_key("foo"), "foo");
        assert_eq!(render_object_key("Content-Type"), "\"Content-Type\"");
        assert_eq!(render_object_key(""), "\"\"");
        assert_eq!(render_object_key("a\"b"), "\"a\\\"b\"");
    }
}

/// Structural traversal helpers.
///
/// Every analysis over the tree needs "visit the sub-expressions of this
/// expression" and "visit the blocks this statement owns", and each one that
/// hand-writes the match re-derives the same 19-variant list. `owned.rs` has one
/// such match; the third copy of a traversal is where a new `Expr` variant
/// starts getting missed by one pass and not the others. These are that list,
/// written once, with no wildcard arm so a new variant forces a decision here.
pub mod visit {
    use super::*;

    /// Call `f` on each direct sub-expression of `e`. Not recursive: a caller
    /// that wants the whole subtree recurses inside `f`, which is what lets one
    /// helper serve both "find any await" and "collect every read".
    pub fn child_exprs<'a>(e: &'a Expr, f: &mut impl FnMut(&'a Expr)) {
        match e {
            Expr::Number { .. }
            | Expr::String { .. }
            | Expr::Bool { .. }
            | Expr::Void { .. }
            | Expr::Ident { .. }
            | Expr::Extern { .. } => {}
            Expr::TemplateString { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr { value, .. } = p {
                        f(value);
                    }
                }
            }
            Expr::Binary { left, right, .. } => {
                f(left);
                f(right);
            }
            Expr::Unary { operand, .. } | Expr::Postfix { operand, .. } => f(operand),
            Expr::Call { callee, args, .. } | Expr::New { callee, args, .. } => {
                f(callee);
                for a in args {
                    f(a);
                }
            }
            Expr::Member { object, .. } => f(object),
            Expr::Index { object, index, .. } => {
                f(object);
                f(index);
            }
            Expr::Await { expr, .. } => f(expr),
            Expr::Array { elements, .. } => {
                for el in elements {
                    match el {
                        ArrayElem::Expr(v) | ArrayElem::Spread(v) => f(v),
                    }
                }
            }
            Expr::Object { fields, .. } => {
                for field in fields {
                    match field {
                        ObjectField::KeyValue { value, .. }
                        | ObjectField::Spread { value, .. } => f(value),
                    }
                }
            }
            Expr::Match { scrutinee, arms, .. } => {
                f(scrutinee);
                for arm in arms {
                    if let MatchArmBody::Expr(v) = &arm.body {
                        f(v);
                    }
                }
            }
            Expr::Lambda { .. } => {}
            Expr::Jsx(_) => {}
        }
    }

    /// Call `f` on each block an expression owns (a lambda body, a `match` arm
    /// written as a block). Separate from `child_exprs` because a block is a new
    /// scope and most analyses treat it differently from a sub-expression.
    pub fn child_blocks<'a>(e: &'a Expr, f: &mut impl FnMut(&'a Block)) {
        match e {
            Expr::Lambda { body, .. } => f(body),
            Expr::Match { arms, .. } => {
                for arm in arms {
                    if let MatchArmBody::Block(b) = &arm.body {
                        f(b);
                    }
                }
            }
            _ => {}
        }
    }

    /// Call `f` on each expression a statement holds directly.
    pub fn stmt_exprs<'a>(s: &'a Stmt, f: &mut impl FnMut(&'a Expr)) {
        match s {
            Stmt::Let(l) => f(&l.value),
            Stmt::Mut(m) => match &m.kind {
                MutKind::Assign { target, value } => {
                    f(target);
                    f(value);
                }
                MutKind::MethodCall { call } => f(call),
            },
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    f(v);
                }
            }
            Stmt::For(fo) => f(&fo.iter),
            Stmt::Expr(e) => f(e),
            Stmt::Defer(d) => f(&d.expr),
            Stmt::Loop(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    /// Call `f` on each block a statement owns.
    pub fn stmt_blocks<'a>(s: &'a Stmt, f: &mut impl FnMut(&'a Block)) {
        match s {
            Stmt::For(fo) => f(&fo.body),
            Stmt::Loop(l) => f(&l.body),
            Stmt::Let(_)
            | Stmt::Mut(_)
            | Stmt::Return(_)
            | Stmt::Expr(_)
            | Stmt::Defer(_)
            | Stmt::Break(_)
            | Stmt::Continue(_) => {}
        }
    }
}
