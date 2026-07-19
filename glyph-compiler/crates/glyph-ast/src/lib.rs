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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    /// `import std/http` → `ImportKind::Namespace`
    /// `import std/result { Ok, Err }` → `ImportKind::Named(vec![Ok, Err])`
    /// `import std/http as h` → `ImportKind::Aliased(h)`
    pub path: ModulePath,
    pub kind: ImportKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    Namespace,
    Named(Vec<Ident>),
    Aliased(Ident),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
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
    /// Constraints (e.g. `<T: SomeBound>`) — deferred to v1.1 per the brainstorm
    /// "generics with simple bounds" being a step-5 substep 5a deliverable.
    /// Currently always empty.
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
    pub generics: Vec<GenericParam>,
    /// D25: `resource type X = ...`. A value of a resource type may be bound
    /// with `let owned` and is then tracked for single-consumption. Plain
    /// `type X = ...` leaves this `false`.
    pub is_resource: bool,
    pub body: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDecl {
    pub name: Ident,
    pub annotations: Vec<Annotation>,
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
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_ty: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
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
    Expr(Expr),
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

/// D21: `for X in expr { body }` and the two-binding form
/// `for K, V in expr { body }` (used for iterating record entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForStmt {
    pub bindings: Vec<Ident>,
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
    /// Lambda expression: `fn(args) -> T { body }` or `fn(args) { body }`.
    /// Anonymous form per D4; body is a block.
    Lambda {
        params: Vec<Param>,
        return_ty: Option<TypeExpr>,
        body: Block,
        span: Span,
    },
    /// JSX element in expression position (D6).
    Jsx(JsxElement),
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
    // Level 7
    Eq,    // ==
    NotEq, // !=
    // Level 6
    Lt, // <
    Gt, // >
    LtEq,
    GtEq,
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
    Not, // !
    Neg, // -
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
    /// `fn(x: T) -> U` function type. Day 4+ may extend; the v0 shape is final.
    Fn {
        params: Vec<FnTypeParam>,
        return_ty: Option<Box<TypeExpr>>,
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
    /// `{ name, email }` — object destructure. Each field binds an identifier
    /// of the same name; renamed binding (`{ name: n }`) is recognized but
    /// the typechecker decides semantics.
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
    /// `{ name: alias }` → `binding = Some(alias)`. `{ name }` → `binding = None`.
    pub binding: Option<Ident>,
    pub span: Span,
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
            | Expr::Array { span, .. }
            | Expr::Object { span, .. }
            | Expr::Match { span, .. }
            | Expr::Lambda { span, .. } => *span,
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
            | TypeExpr::Union { span, .. } => *span,
        }
    }
}

impl Pattern {
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
