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
//! Monomorphic tagged unions lower to a TS discriminated union on a `tag`
//! field plus a constructor per variant (a `const` for a no-payload variant,
//! a function for a payload variant; record payloads spread their fields).
//!
//! Deferred to later week-4 days, surfaced as `EmitError::Unsupported` rather
//! than emitting invalid TS: `match` lowering to `switch`, the `?` operator's
//! inlined unwrapping, generic tagged unions, the Q8 runtime descriptors that
//! accompany type declarations, `component` + D6 JSX directive lowering, and
//! the two-binding `for K, V in`.
//!
//! ## Known gap: reserved-word identifiers
//!
//! Glyph's lexer permits TS reserved words (`class`, `default`, `new`, ...) as
//! soft-keyword identifiers, and this emitter copies a binding/parameter/import
//! name verbatim, so such a name produces TS that `tsc` rejects. (Object keys,
//! record fields, and member access are safe — only binding positions break.)
//! The right fix is a resolver-level "stricter-than-TS" rule that rejects TS
//! reserved words as identifier names, not emit-time mangling (which would
//! break import name matching). Tracked for a later day; no example trips it.

#![forbid(unsafe_code)]

use glyph_ast::{
    ArrayElem, BinOp, Block, Decl, Expr, GenericParam, ImportDecl, ImportKind, Module, MutKind,
    ObjectField, Param, PostfixOp, Span, Stmt, TemplatePart, TypeExpr, UnaryOp, UnionVariant,
};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// A construct whose emission lands in a later week-4 day. Carries the
    /// construct name (for the diagnostic) and the offending span.
    #[error("TS emission for {construct} is not implemented yet")]
    Unsupported { construct: &'static str, span: Span },
}

impl EmitError {
    pub fn span(&self) -> Span {
        match self {
            EmitError::Unsupported { span, .. } => *span,
        }
    }
}

/// Emit a whole module to a TypeScript source string.
pub fn emit_module(module: &Module) -> Result<String, EmitError> {
    let mut e = Emitter {
        out: String::new(),
        indent: 0,
    };
    e.emit_module(module)?;
    Ok(e.out)
}

struct Emitter {
    out: String,
    indent: usize,
}

impl Emitter {
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

    // ----- declarations -----

    fn emit_module(&mut self, module: &Module) -> Result<(), EmitError> {
        for (i, decl) in module.items.iter().enumerate() {
            if i > 0 {
                self.out.push('\n');
            }
            self.emit_decl(decl)?;
        }
        Ok(())
    }

    fn emit_decl(&mut self, decl: &Decl) -> Result<(), EmitError> {
        match decl {
            Decl::Import(im) => self.emit_import(im),
            Decl::Fn(f) => {
                let generics = self.generics(&f.generics);
                let params = self.params(&f.params)?;
                // Glyph's `async fn -> T` awaits to `T`; TS annotates the
                // wrapper, so the emitted return type is `Promise<T>`.
                let ret = match &f.return_ty {
                    Some(te) if f.is_async => format!(": Promise<{}>", self.ty(te)?),
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                let prefix = if f.is_async {
                    "export async function"
                } else {
                    "export function"
                };
                self.pad();
                self.out
                    .push_str(&format!("{prefix} {}{generics}({params}){ret} ", f.name));
                self.emit_block(&f.body)?;
                self.out.push('\n');
                Ok(())
            }
            Decl::Const(c) => {
                let ty = match &c.ty {
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                let value = self.expr(&c.value)?;
                self.line(&format!("export const {}{ty} = {value};", c.name));
                Ok(())
            }
            Decl::Type(t) => {
                if let TypeExpr::Union { variants, .. } = &t.body {
                    // Generic tagged unions (no-payload variants of a generic
                    // union need a widened constructor type) land on a later
                    // day; the corpus's user unions are all monomorphic.
                    if !t.generics.is_empty() {
                        return Err(EmitError::Unsupported {
                            construct: "generic tagged union type declaration",
                            span: t.span,
                        });
                    }
                    return self.emit_union(&t.name, variants);
                }
                let generics = self.generics(&t.generics);
                let body = self.ty(&t.body)?;
                self.line(&format!("export type {}{generics} = {body};", t.name));
                Ok(())
            }
            Decl::Component(c) => Err(EmitError::Unsupported {
                construct: "component declaration",
                span: c.span,
            }),
        }
    }

    fn emit_import(&mut self, im: &ImportDecl) -> Result<(), EmitError> {
        let spec = im
            .path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        let line = match &im.kind {
            ImportKind::Named(names) => {
                let names = names
                    .iter()
                    .map(|n| n.as_ref())
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
        };
        self.line(&line);
        Ok(())
    }

    /// Emit a (monomorphic) tagged union as a TS discriminated union plus a
    /// constructor per variant. The discriminant is a `tag` string literal.
    /// A record payload's fields are spread alongside the tag; a no-payload
    /// variant becomes a `const`, a payload variant a constructor function.
    fn emit_union(&mut self, name: &str, variants: &[UnionVariant]) -> Result<(), EmitError> {
        self.line(&format!("export type {name} ="));
        self.indent += 1;
        for (i, v) in variants.iter().enumerate() {
            let term = if i + 1 == variants.len() { ";" } else { "" };
            let members = self.variant_members(v)?;
            self.line(&format!("| {{ {members} }}{term}"));
        }
        self.indent -= 1;
        self.out.push('\n');
        for v in variants {
            self.emit_variant_constructor(name, v)?;
        }
        Ok(())
    }

    /// The object-type members of a variant: the `tag` literal, plus a record
    /// payload's fields spread inline, or a non-record payload under `value`.
    fn variant_members(&self, v: &UnionVariant) -> Result<String, EmitError> {
        let mut s = format!("tag: \"{}\"", v.name);
        match &v.payload {
            None => {}
            Some(TypeExpr::Record { fields, .. }) => {
                for f in fields {
                    let opt = if f.optional { "?" } else { "" };
                    s.push_str(&format!("; {}{opt}: {}", f.name, self.ty(&f.ty)?));
                }
            }
            Some(other) => s.push_str(&format!("; value: {}", self.ty(other)?)),
        }
        Ok(s)
    }

    fn emit_variant_constructor(
        &mut self,
        union: &str,
        v: &UnionVariant,
    ) -> Result<(), EmitError> {
        let name = &v.name;
        match &v.payload {
            None => self.line(&format!(
                "export const {name}: {union} = {{ tag: \"{name}\" }};"
            )),
            Some(payload @ TypeExpr::Record { .. }) => self.line(&format!(
                "export function {name}(fields: {}): {union} {{ return {{ tag: \"{name}\", ...fields }}; }}",
                self.ty(payload)?
            )),
            Some(other) => self.line(&format!(
                "export function {name}(value: {}): {union} {{ return {{ tag: \"{name}\", value }}; }}",
                self.ty(other)?
            )),
        }
        Ok(())
    }

    fn generics(&self, generics: &[GenericParam]) -> String {
        if generics.is_empty() {
            return String::new();
        }
        let names = generics
            .iter()
            .map(|g| g.name.as_ref())
            .collect::<Vec<_>>()
            .join(", ");
        format!("<{names}>")
    }

    fn params(&self, params: &[Param]) -> Result<String, EmitError> {
        let mut out = Vec::with_capacity(params.len());
        for p in params {
            out.push(format!("{}: {}", p.name, self.ty(&p.ty)?));
        }
        Ok(out.join(", "))
    }

    // ----- statements -----

    fn emit_block(&mut self, block: &Block) -> Result<(), EmitError> {
        self.out.push_str("{\n");
        self.indent += 1;
        for stmt in &block.stmts {
            self.emit_stmt(stmt)?;
        }
        self.indent -= 1;
        self.pad();
        self.out.push('}');
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), EmitError> {
        match stmt {
            Stmt::Let(l) => {
                // `let` (not `const`): a `mut` statement may reassign it later.
                let ty = match &l.ty {
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                let value = self.expr(&l.value)?;
                self.line(&format!("let {}{ty} = {value};", l.name));
            }
            Stmt::Mut(m) => {
                let s = match &m.kind {
                    MutKind::Assign { target, value } => {
                        format!("{} = {};", target, self.expr(value)?)
                    }
                    MutKind::AssignIndex {
                        target,
                        index,
                        value,
                    } => format!(
                        "{}[{}] = {};",
                        target,
                        self.expr(index)?,
                        self.expr(value)?
                    ),
                    MutKind::AssignField {
                        target,
                        field,
                        value,
                    } => {
                        format!("{}.{} = {};", target, field, self.expr(value)?)
                    }
                    MutKind::MethodCall { call, .. } => format!("{};", self.expr(call)?),
                };
                self.line(&s);
            }
            Stmt::Return(r) => match &r.value {
                Some(v) => {
                    let v = self.expr(v)?;
                    self.line(&format!("return {v};"));
                }
                None => self.line("return;"),
            },
            Stmt::For(f) => {
                if f.bindings.len() != 1 {
                    return Err(EmitError::Unsupported {
                        construct: "two-binding `for K, V in`",
                        span: f.span,
                    });
                }
                let iter = self.expr(&f.iter)?;
                self.pad();
                self.out
                    .push_str(&format!("for (const {} of {iter}) ", f.bindings[0]));
                self.emit_block(&f.body)?;
                self.out.push('\n');
            }
            Stmt::Loop(l) => {
                self.pad();
                self.out.push_str("while (true) ");
                self.emit_block(&l.body)?;
                self.out.push('\n');
            }
            Stmt::Break(_) => self.line("break;"),
            Stmt::Continue(_) => self.line("continue;"),
            Stmt::Expr(e) => {
                let s = self.expr(e)?;
                self.line(&format!("{s};"));
            }
        }
        Ok(())
    }

    // ----- expressions -----

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
                format!(
                    "({} {} {})",
                    self.expr(left)?,
                    bin_op(*op),
                    self.expr(right)?
                )
            }
            Expr::Unary { op, operand, .. } => {
                let op = match op {
                    UnaryOp::Not => "!",
                    UnaryOp::Neg => "-",
                };
                format!("({op}{})", self.expr(operand)?)
            }
            Expr::Postfix { op, operand, span } => match op {
                // `expr?` lowers to an inlined Result unwrap; a later day.
                PostfixOp::Try => {
                    let _ = operand;
                    return Err(EmitError::Unsupported {
                        construct: "the `?` operator",
                        span: *span,
                    });
                }
            },
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
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
                format!("{}{targs}({})", self.expr(callee)?, a.join(", "))
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
            Expr::Index { object, index, .. } => {
                format!("{}[{}]", self.expr(object)?, self.expr(index)?)
            }
            Expr::Await { expr, .. } => format!("(await {})", self.expr(expr)?),
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
                            format!("{key}: {}", self.expr(value)?)
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
                ..
            } => {
                let params = self.params(params)?;
                let ret = match return_ty {
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                let mut sub = Emitter {
                    out: String::new(),
                    indent: self.indent,
                };
                sub.emit_block(body)?;
                format!("({params}){ret} => {}", sub.out)
            }
            Expr::Match { span, .. } => {
                return Err(EmitError::Unsupported {
                    construct: "`match` expression",
                    span: *span,
                })
            }
            Expr::Jsx(j) => {
                return Err(EmitError::Unsupported {
                    construct: "JSX",
                    span: j.span,
                })
            }
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

    // ----- types -----

    fn ty(&self, te: &TypeExpr) -> Result<String, EmitError> {
        Ok(match te {
            TypeExpr::Path { segments, .. } => {
                let joined = segments
                    .iter()
                    .map(|s| s.as_ref())
                    .collect::<Vec<_>>()
                    .join(".");
                // Glyph `bool` is TS `boolean`; the rest map by name.
                if joined == "bool" {
                    "boolean".to_string()
                } else {
                    joined
                }
            }
            TypeExpr::Generic { base, args, .. } => {
                let mut a = Vec::with_capacity(args.len());
                for arg in args {
                    a.push(self.ty(arg)?);
                }
                format!("{}<{}>", self.ty(base)?, a.join(", "))
            }
            TypeExpr::Fn {
                params, return_ty, ..
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
            TypeExpr::Union { span, .. } => {
                return Err(EmitError::Unsupported {
                    construct: "tagged union type",
                    span: *span,
                })
            }
        })
    }
}

fn bin_op(op: BinOp) -> &'static str {
    match op {
        BinOp::NullishCoalesce => "??",
        BinOp::LogicalOr => "||",
        BinOp::LogicalAnd => "&&",
        // Glyph `==`/`!=` are value equality; emit the strict TS forms.
        BinOp::Eq => "===",
        BinOp::NotEq => "!==",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
    }
}

/// Render a de-escaped string value as a double-quoted TS string literal.
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

    fn emit(src: &str) -> String {
        let module = glyph_parser::parse(src).expect("parse failed");
        emit_module(&module).expect("emit failed")
    }

    fn emit_err(src: &str) -> EmitError {
        let module = glyph_parser::parse(src).expect("parse failed");
        emit_module(&module).expect_err("expected emit error")
    }

    #[test]
    fn fn_with_params_and_body() {
        let ts = emit("module x\nfn add(a: number, b: number) -> number { return a + b }\n");
        assert_eq!(
            ts,
            "export function add(a: number, b: number): number {\n  return (a + b);\n}\n"
        );
    }

    #[test]
    fn bool_maps_to_boolean_and_eq_is_strict() {
        let ts = emit("module x\nfn p(a: number, b: number) -> bool { return a == b }\n");
        assert!(ts.contains("): boolean {"), "{ts}");
        assert!(ts.contains("(a === b)"), "{ts}");
    }

    #[test]
    fn async_fn_and_await() {
        let ts = emit("module x\nasync fn run() -> number { return await fetch() }\n");
        assert!(
            ts.starts_with("export async function run(): Promise<number> {"),
            "{ts}"
        );
        assert!(ts.contains("return (await fetch());"), "{ts}");
    }

    #[test]
    fn template_literal_passes_through() {
        let ts = emit("module x\nfn greet(name: string) -> string { return \"hi ${name}\" }\n");
        assert!(ts.contains("return `hi ${name}`;"), "{ts}");
    }

    #[test]
    fn const_and_type_alias() {
        let ts = emit("module x\nconst MAX: number = 10\ntype Sku = string\n");
        assert!(ts.contains("export const MAX: number = 10;"), "{ts}");
        assert!(ts.contains("export type Sku = string;"), "{ts}");
    }

    #[test]
    fn record_type_alias_and_void_value() {
        let ts = emit("module x\ntype User = { name: string, age?: number }\n");
        assert!(
            ts.contains("export type User = { name: string; age?: number };"),
            "{ts}"
        );
    }

    #[test]
    fn imports_three_forms() {
        let ts = emit(
            "module x\nimport std/result { Ok, Err }\nimport std/io\nimport std/http as h\n",
        );
        assert!(ts.contains("import { Ok, Err } from \"std/result\";"), "{ts}");
        assert!(ts.contains("import * as io from \"std/io\";"), "{ts}");
        assert!(ts.contains("import * as h from \"std/http\";"), "{ts}");
    }

    #[test]
    fn loop_for_and_array_object() {
        let ts = emit(
            "module x\nfn f(xs: Array<number>) -> void {\n  for x in xs {\n    log(x)\n  }\n  let o = { a: 1, b: 2 }\n  return void\n}\n",
        );
        assert!(ts.contains("for (const x of xs) {"), "{ts}");
        assert!(ts.contains("let o = { a: 1, b: 2 };"), "{ts}");
        assert!(ts.contains("return undefined;"), "{ts}");
    }

    #[test]
    fn string_escapes_line_separators_and_controls() {
        // The lexer de-escapes `\u{2028}` to a raw LINE SEPARATOR, which is an
        // unterminated-string error in TS unless re-escaped.
        let ts = emit("module x\nconst s: string = \"a\\u{2028}b\\u{0}c\"\n");
        assert!(ts.contains("\"a\\u2028b\\u0000c\""), "{ts}");
        assert!(!ts.contains('\u{2028}'), "raw U+2028 leaked: {ts}");
    }

    #[test]
    fn empty_object_literal_has_no_double_space() {
        let ts = emit("module x\nconst o = {}\n");
        assert!(ts.contains("export const o = {};"), "{ts}");
    }

    #[test]
    fn match_is_unsupported_for_now() {
        let err = emit_err("module x\nfn f(n: number) -> number { return match n { else => 0 } }\n");
        assert!(matches!(
            err,
            EmitError::Unsupported {
                construct: "`match` expression",
                ..
            }
        ));
    }

    #[test]
    fn tagged_union_emits_discriminated_union_and_constructors() {
        let ts = emit(
            "module x\ntype SearchState =\n  | Idle\n  | Loaded({ users: number })\n  | Failed({ message: string })\n",
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
            ts.contains("export function Loaded(fields: { users: number }): SearchState { return { tag: \"Loaded\", ...fields }; }"),
            "{ts}"
        );
    }

    #[test]
    fn single_line_no_payload_union_emits_consts() {
        let ts = emit("module x\ntype Color = Red | Green | Blue\n");
        assert!(ts.contains("export const Red: Color = { tag: \"Red\" };"), "{ts}");
        assert!(ts.contains("| { tag: \"Blue\" };"), "{ts}");
    }

    #[test]
    fn generic_tagged_union_is_unsupported_for_now() {
        let err = emit_err(
            "module x\ntype Box<T> =\n  | Full({ value: T })\n  | Empty\n",
        );
        assert!(matches!(
            err,
            EmitError::Unsupported {
                construct: "generic tagged union type declaration",
                ..
            }
        ));
    }
}
