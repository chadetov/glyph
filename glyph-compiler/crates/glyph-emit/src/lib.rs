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
//! A `match` over a tagged union lowers to a `switch` on the `tag`
//! discriminant, with constructor-pattern arms (`Ok(x)`, `NetworkError({ url })`)
//! binding the payload and `_`/`else` becoming `default`. In statement position
//! (`return match`, or a bare `match` statement) the switch is emitted directly
//! so `return` keeps its function semantics; in value position (`let x = match`,
//! nested) it is wrapped in an immediately-invoked arrow.
//!
//! Deferred to later week-4 days, surfaced as `EmitError::Unsupported` rather
//! than emitting invalid TS: bare-identifier variant arms and value (literal)
//! matches (both need the scrutinee type), block arm bodies, nested/`is`/array
//! match patterns, the
//! `?` operator's inlined unwrapping, generic tagged unions, the Q8 runtime
//! descriptors that accompany type declarations, `component` + D6 JSX
//! directive lowering, and the two-binding `for K, V in`.
//!
//! ## Known gap: reserved-word identifiers
//!
//! Glyph's lexer permits TS reserved words (`class`, `default`, `new`, ...) as
//! soft-keyword identifiers, and this emitter copies a binding/parameter/import
//! name (and a tagged-union variant's constructor name) verbatim, so such a
//! name produces TS that `tsc` rejects. (Object keys, record fields, and member
//! access are safe — only binding positions break.)
//! The right fix is a resolver-level "stricter-than-TS" rule that rejects TS
//! reserved words as identifier names, not emit-time mangling (which would
//! break import name matching). Tracked for a later day; no example trips it.
//!
//! Two more gaps in the same family, both fixed once type context is threaded
//! into the emitter (or by a resolver rule):
//! - A single-identifier payload bind `Variant(x)` reads `.value`, which is
//!   correct for a non-record payload (`Ok(x)`, `Some(x)`) but wrong for a
//!   record payload bound whole (`Variant(p)`), where the fields are spread
//!   flat. The corpus binds records with object patterns, so no example trips
//!   it; the whole-record reconstruction needs the variant's declared shape.
//! - `match` lowering synthesizes `__mN` scrutinee temporaries; a user
//!   identifier literally named `__m0` would collide. A resolver rule
//!   reserving the `__` prefix is the proper fix.

#![forbid(unsafe_code)]

use glyph_ast::{
    ArrayElem, BinOp, Block, Decl, Expr, GenericParam, ImportDecl, ImportKind, MatchArm,
    MatchArmBody, Module, MutKind, ObjectField, Param, Pattern, PostfixOp, Span, Stmt,
    TemplatePart, TypeExpr, UnaryOp, UnionVariant,
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

/// How a lowered `match` arm yields control: `return` its value (the match is
/// in return position) or run it for effect and `break` (statement position).
#[derive(Clone, Copy)]
enum ArmTerm {
    Return,
    Break,
}

/// Emit a whole module to a TypeScript source string.
pub fn emit_module(module: &Module) -> Result<String, EmitError> {
    let mut e = Emitter {
        out: String::new(),
        indent: 0,
        tmp_counter: 0,
    };
    e.emit_module(module)?;
    Ok(e.out)
}

struct Emitter {
    out: String,
    indent: usize,
    /// Counter for synthesized scrutinee temporaries (`__m0`, `__m1`, ...), so
    /// two `match` statements in one function body don't redeclare the name.
    tmp_counter: usize,
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
        v: &UnionVariant,
    ) -> Result<(), EmitError> {
        let name = &v.name;
        match &v.payload {
            None => self.line(&format!(
                "export const {name}: {union} = {{ {TAG}: \"{name}\" }};"
            )),
            // Spread the fields FIRST so the discriminant always wins, even if
            // the record (somehow) carried a colliding key.
            Some(payload @ TypeExpr::Record { .. }) => self.line(&format!(
                "export function {name}(fields: {}): {union} {{ return {{ ...fields, {TAG}: \"{name}\" }}; }}",
                self.ty(payload)?
            )),
            Some(other) => self.line(&format!(
                "export function {name}({PAYLOAD}: {}): {union} {{ return {{ {TAG}: \"{name}\", {PAYLOAD} }}; }}",
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
                // A `return match { ... }` lowers to a `switch` statement so
                // that `return` keeps its function-return semantics (an IIFE
                // would capture the return). Each arm returns its value.
                Some(Expr::Match { scrutinee, arms, .. }) => {
                    self.emit_match_dispatch(scrutinee, arms, ArmTerm::Return)?;
                }
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
            Stmt::Expr(Expr::Match { scrutinee, arms, .. }) => {
                // A statement-position `match` runs each arm for its effects
                // and `break`s out of the switch.
                self.emit_match_dispatch(scrutinee, arms, ArmTerm::Break)?;
            }
            Stmt::Expr(e) => {
                let s = self.expr(e)?;
                self.line(&format!("{s};"));
            }
        }
        Ok(())
    }

    /// Lower a statement-position `match` over a tagged union to a `switch` on
    /// the `tag` discriminant. Scoped to constructor-pattern arms (`Ok(x)`,
    /// `NetworkError({ url })`, dotted `fs.ErrorKind.NotFound`) plus
    /// `_`/`else`, with expression arm bodies. Bare-identifier variant arms
    /// (which the resolver cannot distinguish from bindings without the
    /// scrutinee type), value matches, block arm bodies, nested/`is`/array
    /// patterns, and match in value position are deferred.
    fn emit_match_dispatch(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        term: ArmTerm,
    ) -> Result<(), EmitError> {
        for arm in arms {
            match &arm.pattern {
                Pattern::Constructor { args, span, .. } => match args.as_slice() {
                    [] | [Pattern::Ident { .. }] | [Pattern::Object { .. }] => {}
                    _ => {
                        return Err(EmitError::Unsupported {
                            construct: "a nested or multi-argument pattern in a match arm",
                            span: *span,
                        })
                    }
                },
                Pattern::Wildcard { .. } | Pattern::Else { .. } => {}
                Pattern::Ident { span, .. } => {
                    return Err(EmitError::Unsupported {
                        construct: "a bare-identifier match arm (needs the scrutinee type)",
                        span: *span,
                    })
                }
                Pattern::Literal { span, .. } => {
                    return Err(EmitError::Unsupported {
                        construct: "a value match (literal pattern)",
                        span: *span,
                    })
                }
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
            if let MatchArmBody::Block(b) = &arm.body {
                return Err(EmitError::Unsupported {
                    construct: "a block body in a match arm",
                    span: b.span,
                });
            }
        }

        // Two catch-all arms would emit two `default:` clauses (invalid TS).
        // The typechecker does not yet reject the redundant arm, so guard here.
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

        // A match with no constructor arm has nothing to discriminate on, so
        // there is no `.tag` to switch over. Evaluate the scrutinee for any
        // effect (parenthesized so an object-literal scrutinee isn't parsed as
        // a block), then run the lone catch-all arm.
        let has_ctor = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Constructor { .. }));
        if !has_ctor {
            let scrut = self.expr(scrutinee)?;
            self.line(&format!("({scrut});"));
            self.emit_arm_body(&arms[0].body, term)?;
            return Ok(());
        }

        let scrut = self.expr(scrutinee)?;
        let m = format!("__m{}", self.tmp_counter);
        self.tmp_counter += 1;
        self.line(&format!("const {m} = {scrut};"));
        self.line(&format!("switch ({m}.{TAG}) {{"));
        self.indent += 1;
        for arm in arms {
            match &arm.pattern {
                Pattern::Constructor { path, args, .. } => {
                    let variant = path.last().expect("constructor path is non-empty");
                    self.line(&format!("case \"{variant}\": {{"));
                    self.indent += 1;
                    self.emit_arm_binds(&m, args);
                    self.emit_arm_body(&arm.body, term)?;
                    self.indent -= 1;
                    self.line("}");
                }
                Pattern::Wildcard { .. } | Pattern::Else { .. } => {
                    self.line("default: {");
                    self.indent += 1;
                    self.emit_arm_body(&arm.body, term)?;
                    self.indent -= 1;
                    self.line("}");
                }
                _ => unreachable!("patterns were validated above"),
            }
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// Bind a constructor arm's payload from the scrutinee temporary `m`: an
    /// object pattern reads each spread field by name; a single identifier
    /// reads the non-record `value` field; no args binds nothing.
    fn emit_arm_binds(&mut self, m: &str, args: &[Pattern]) {
        match args {
            [Pattern::Ident { name, .. }] => self.line(&format!("const {name} = {m}.{PAYLOAD};")),
            [Pattern::Object { fields, .. }] => {
                for f in fields {
                    let binding = f.binding.as_ref().unwrap_or(&f.key);
                    self.line(&format!("const {binding} = {m}.{};", f.key));
                }
            }
            _ => {}
        }
    }

    fn emit_arm_body(&mut self, body: &MatchArmBody, term: ArmTerm) -> Result<(), EmitError> {
        let MatchArmBody::Expr(e) = body else {
            unreachable!("block bodies were rejected above")
        };
        let s = self.expr(e)?;
        match term {
            ArmTerm::Return => self.line(&format!("return {s};")),
            ArmTerm::Break => {
                self.line(&format!("{s};"));
                self.line("break;");
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
                    tmp_counter: self.tmp_counter,
                };
                sub.emit_block(body)?;
                format!("({params}){ret} => {}", sub.out)
            }
            // A value-position `match` (`let x = match ...`, or nested in an
            // expression) wraps the same statement lowering in an
            // immediately-invoked arrow. Each arm `return`s from the arrow, so
            // the IIFE evaluates to the matched value. (Expression arm bodies
            // cannot contain a function-level `return`, so capturing it in the
            // arrow is sound.)
            Expr::Match { scrutinee, arms, .. } => {
                let mut sub = Emitter {
                    out: String::new(),
                    indent: self.indent + 1,
                    tmp_counter: self.tmp_counter,
                };
                sub.emit_match_dispatch(scrutinee, arms, ArmTerm::Return)?;
                let pad = "  ".repeat(self.indent);
                format!("(() => {{\n{}{pad}}})()", sub.out)
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
    fn return_match_lowers_to_switch_on_tag() {
        let ts = emit(
            "module x\nfn classify(r: Result<number, string>) -> number {\n  return match r {\n    Ok(value) => value,\n    Err(msg) => 0,\n  }\n}\n",
        );
        assert!(ts.contains("const __m0 = r;"), "{ts}");
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("case \"Ok\": {"), "{ts}");
        assert!(ts.contains("const value = __m0.value;"), "{ts}");
        assert!(ts.contains("return value;"), "{ts}");
        assert!(ts.contains("case \"Err\": {"), "{ts}");
    }

    #[test]
    fn value_position_match_wraps_in_an_iife() {
        let ts = emit(
            "module x\nfn f(r: Result<number, string>) -> string {\n  let label = match r {\n    Ok(n) => \"ok\",\n    Err(e) => \"err\",\n  }\n  return label\n}\n",
        );
        assert!(ts.contains("let label = (() => {"), "{ts}");
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("return \"ok\";"), "{ts}");
        assert!(ts.contains("})();"), "{ts}");
    }

    #[test]
    fn match_object_pattern_binds_spread_fields() {
        let ts = emit(
            "module x\ntype E =\n  | NetworkError({ url: string, status: number })\n  | NotFound({ id: string })\nfn show(e: E) -> string {\n  return match e {\n    NetworkError({ url, status }) => url,\n    NotFound({ id }) => id,\n  }\n}\n",
        );
        assert!(ts.contains("case \"NetworkError\": {"), "{ts}");
        assert!(ts.contains("const url = __m0.url;"), "{ts}");
        assert!(ts.contains("const status = __m0.status;"), "{ts}");
        assert!(ts.contains("return url;"), "{ts}");
    }

    #[test]
    fn two_match_statements_use_distinct_temporaries() {
        let ts = emit(
            "module x\nfn f(a: Result<number, string>, b: Result<number, string>) -> number {\n  match a {\n    Ok(x) => log(x),\n    Err(e) => log(e),\n  }\n  return match b {\n    Ok(y) => y,\n    Err(e) => 0,\n  }\n}\n",
        );
        assert!(ts.contains("const __m0 = a;"), "{ts}");
        assert!(ts.contains("const __m1 = b;"), "{ts}");
    }

    #[test]
    fn two_catch_all_arms_are_rejected() {
        // Two `else` arms would emit two `default:` clauses (TS1113).
        let err = emit_err(
            "module x\ntype E =\n  | A({ x: number })\n  | B({ y: number })\nfn f(e: E) -> number {\n  return match e {\n    A({ x }) => x,\n    else => 1,\n    else => 2,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("catch-all")),
            "got {err:?}"
        );
    }

    #[test]
    fn value_match_is_unsupported_for_now() {
        let err = emit_err(
            "module x\nfn f(n: number) -> number { return match n { 0 => 1, else => 2 } }\n",
        );
        assert!(matches!(
            err,
            EmitError::Unsupported {
                construct: "a value match (literal pattern)",
                ..
            }
        ));
    }

    #[test]
    fn bare_variant_match_is_unsupported_for_now() {
        let err = emit_err(
            "module x\ntype S = Idle | Busy\nfn f(s: S) -> number {\n  return match s {\n    Idle => 0,\n    Busy => 1,\n  }\n}\n",
        );
        assert!(matches!(
            err,
            EmitError::Unsupported {
                construct: "a bare-identifier match arm (needs the scrutinee type)",
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
            ts.contains("export function Loaded(fields: { users: number }): SearchState { return { ...fields, tag: \"Loaded\" }; }"),
            "{ts}"
        );
    }

    #[test]
    fn payload_field_named_tag_is_rejected() {
        let err = emit_err(
            "module x\ntype T =\n  | V({ tag: string })\n  | W\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("tag")),
            "got {err:?}"
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
