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
//! carries its type parameters on the alias and the constructor functions, and
//! its no-payload `const`s are widened to `<never>` so they fit every
//! instantiation. A
//! non-generic record type additionally emits a Q8 runtime descriptor — an
//! `export const X = { is(v): v is X { ... } }` whose predicate shallowly
//! validates each field (primitives by `typeof`, others by presence) — so
//! `is TypeName` checks can hold at runtime.
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
//! The `?` operator unwraps a `Result` at statement position (`let x = E?`, or
//! a bare `E?`): it binds the operand to a temporary, returns it on `Err`, and
//! reads the `Ok` payload. `?` nested inside a larger expression is deferred
//! (it needs hoisting); `let x = await E?` is one such case (it parses with the
//! `?` under the `await`) and is not yet unwrapped.
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
//! Deferred to later week-4 days, surfaced as `EmitError::Unsupported` rather
//! than emitting invalid TS: binding catch-all arms,
//! value-position block arms, nested-constructor/array match patterns,
//! `is` checks on union/generic/imported types, a nested `?`, the Q8 runtime
//! descriptors that accompany type declarations, `component` + D6 JSX
//! directive lowering, and the two-binding `for K, V in`.
//!
//! ## Known gap: reserved-word identifiers
//!
//! Glyph's lexer permits TS reserved words (`class`, `default`, `new`, ...) as
//! soft-keyword identifiers, and this emitter copies a binding/parameter/import
//! name (a tagged-union variant's constructor name, and a record type's
//! descriptor `const` name) verbatim, so such a name produces TS that `tsc`
//! rejects. (Object keys, record fields, and member access are safe — only
//! binding positions break.)
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
//! - The lowering synthesizes `__`-prefixed temporaries (`__mN` for match
//!   scrutinees, `__rN` for `?` operands); a user identifier with one of those
//!   exact names would collide. A resolver rule reserving the `__` prefix is
//!   the proper fix.

#![forbid(unsafe_code)]

use glyph_ast::{
    ArrayElem, BinOp, Block, Decl, Expr, GenericParam, Ident, ImportDecl, ImportKind,
    LiteralPattern, MatchArm, MatchArmBody, Module, MutKind, ObjectField, Param, Pattern,
    PostfixOp, RecordTypeField, Span, Stmt, TemplatePart, TypeExpr, UnaryOp, UnionVariant,
};
use glyph_resolver::{ResolvedModule, SymbolId, SymbolKind};
use glyph_typechecker::{Ty, TypeMap};

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

/// The error variant tag of the prelude `Result`. The `?` lowering tests it to
/// propagate failures; single-sourced alongside `TAG`/`PAYLOAD` since it is
/// part of the same `Result` wire-format contract.
const RESULT_ERR: &str = "Err";

/// How a lowered `match` arm yields control: `return` its value (the match is
/// in return position) or run it for effect and `break` (statement position).
#[derive(Clone, Copy)]
enum ArmTerm {
    Return,
    Break,
}

/// Emit a whole module to a TypeScript source string. `resolved` and `types`
/// are the resolution and type-inference results for `module`; the emitter
/// consults them where lowering needs the scrutinee's type (e.g. to tell a
/// bare-identifier variant arm from a binding).
pub fn emit_module(
    module: &Module,
    resolved: &ResolvedModule,
    types: &TypeMap,
) -> Result<String, EmitError> {
    let mut e = Emitter {
        out: String::new(),
        indent: 0,
        tmp_counter: 0,
        module,
        resolved,
        types,
    };
    e.emit_module()?;
    Ok(e.out)
}

struct Emitter<'a> {
    out: String,
    indent: usize,
    /// Counter for synthesized scrutinee temporaries (`__m0`, `__m1`, ...), so
    /// two `match` statements in one function body don't redeclare the name.
    tmp_counter: usize,
    module: &'a Module,
    resolved: &'a ResolvedModule,
    types: &'a TypeMap,
}

impl<'a> Emitter<'a> {
    /// A fresh sub-emitter at the given indent, inheriting the temporary
    /// counter so synthesized names don't repeat. Used to render a lambda body
    /// or a value-position `match` into its own string before splicing it in.
    fn sub(&self, indent: usize) -> Emitter<'a> {
        Emitter {
            out: String::new(),
            indent,
            tmp_counter: self.tmp_counter,
            module: self.module,
            resolved: self.resolved,
            types: self.types,
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

    // ----- declarations -----

    fn emit_module(&mut self) -> Result<(), EmitError> {
        // Copy the `&Module` reference (references are `Copy`) so iterating it
        // doesn't borrow `self` across the `&mut self` emit calls.
        let module = self.module;
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
                    return self.emit_union(&t.name, &t.generics, variants);
                }
                let generics = self.generics(&t.generics);
                let body = self.ty(&t.body)?;
                self.line(&format!("export type {}{generics} = {body};", t.name));
                // Q8: a non-generic record type also emits a runtime descriptor
                // whose `is` predicate makes `is TypeName` checks work at
                // runtime (no type erasure). Generic records need their type
                // arguments at the call site and are deferred.
                if let TypeExpr::Record { fields, .. } = &t.body {
                    if t.generics.is_empty() {
                        self.emit_record_descriptor(&t.name, fields);
                    }
                }
                Ok(())
            }
            Decl::Component(c) => Err(EmitError::Unsupported {
                construct: "component declaration",
                span: c.span,
            }),
        }
    }

    /// Emit the Q8 runtime descriptor for a record type: an `is` type guard
    /// doing shallow validation (each primitive field checked by `typeof`,
    /// each other field checked for presence). Deep/recursive validation and
    /// the `parse` entry point (which returns a `Result`) are later work.
    ///
    /// **Soundness limitation**: because a non-primitive field is only checked
    /// for presence, the `value is X` narrowing is stronger than the runtime
    /// proof — `User.is({ parent: 42, ... })` returns true even though `parent`
    /// is not a `User`. This is the documented v1 "shallow validation" scope
    /// (`docs/roadmap/04-transpiler.md`); recursing into a named-record field's
    /// own `is` would close the gap and is the path to full soundness.
    fn emit_record_descriptor(&mut self, name: &Ident, fields: &[RecordTypeField]) {
        let checks: Vec<String> = fields.iter().map(record_field_check).collect();
        self.line(&format!("export const {name} = {{"));
        self.indent += 1;
        self.line(&format!("is(value: unknown): value is {name} {{"));
        self.indent += 1;
        if checks.is_empty() {
            self.line("return typeof value === \"object\" && value !== null;");
        } else {
            self.line("return typeof value === \"object\" && value !== null");
            self.indent += 1;
            for (i, c) in checks.iter().enumerate() {
                let term = if i + 1 == checks.len() { ";" } else { "" };
                self.line(&format!("&& {c}{term}"));
            }
            self.indent -= 1;
        }
        self.indent -= 1;
        self.line("},");
        self.indent -= 1;
        self.line("};");
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
        let generics_str = self.generics(generics);
        // The union applied to its own parameters (`Box<T>`) for constructor
        // return types, and applied to `never` (`Box<never>`) for the widened
        // no-payload `const`.
        let applied = format!("{name}{generics_str}");
        let widened = if generics.is_empty() {
            name.to_string()
        } else {
            let nevers = generics.iter().map(|_| "never").collect::<Vec<_>>().join(", ");
            format!("{name}<{nevers}>")
        };
        self.line(&format!("export type {name}{generics_str} ="));
        self.indent += 1;
        for (i, v) in variants.iter().enumerate() {
            let term = if i + 1 == variants.len() { ";" } else { "" };
            let members = self.variant_members(v)?;
            self.line(&format!("| {{ {members} }}{term}"));
        }
        self.indent -= 1;
        self.out.push('\n');
        for v in variants {
            self.emit_variant_constructor(&generics_str, &applied, &widened, v)?;
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
        generics_str: &str,
        applied: &str,
        widened: &str,
        v: &UnionVariant,
    ) -> Result<(), EmitError> {
        let name = &v.name;
        match &v.payload {
            None => self.line(&format!(
                "export const {name}: {widened} = {{ {TAG}: \"{name}\" }};"
            )),
            // Spread the fields FIRST so the discriminant always wins, even if
            // the record (somehow) carried a colliding key.
            Some(payload @ TypeExpr::Record { .. }) => self.line(&format!(
                "export function {name}{generics_str}(fields: {}): {applied} {{ return {{ ...fields, {TAG}: \"{name}\" }}; }}",
                self.ty(payload)?
            )),
            Some(other) => self.line(&format!(
                "export function {name}{generics_str}({PAYLOAD}: {}): {applied} {{ return {{ {TAG}: \"{name}\", {PAYLOAD} }}; }}",
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
                // `let x = E?` unwraps a `Result`: propagate `Err`, bind the
                // `Ok` payload.
                if let Expr::Postfix {
                    op: PostfixOp::Try,
                    operand,
                    ..
                } = &l.value
                {
                    let r = self.emit_try_unwrap(operand)?;
                    self.line(&format!("let {}{ty} = {r}.{PAYLOAD};", l.name));
                } else {
                    let value = self.expr(&l.value)?;
                    self.line(&format!("let {}{ty} = {value};", l.name));
                }
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
            Stmt::Expr(Expr::Postfix {
                op: PostfixOp::Try,
                operand,
                ..
            }) => {
                // A bare `E?` statement: propagate `Err`, discard the `Ok` value.
                self.emit_try_unwrap(operand)?;
            }
            Stmt::Expr(e) => {
                let s = self.expr(e)?;
                self.line(&format!("{s};"));
            }
        }
        Ok(())
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
        self.line(&format!("if ({r}.{TAG} === \"{RESULT_ERR}\") {{ return {r}; }}"));
        Ok(r)
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
    fn union_variant_names(&self, ty: &Ty) -> Option<Vec<String>> {
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

    /// Lower a `match` over a tagged union to a `switch` on the `tag`
    /// discriminant. Handles constructor-pattern arms (`Ok(x)`,
    /// `NetworkError({ url })`, dotted `fs.ErrorKind.NotFound`), bare no-payload
    /// variant arms (`Idle`, disambiguated from bindings via the scrutinee
    /// type), and `_`/`else`, with expression arm bodies. Value (literal)
    /// matches, binding catch-alls, block arm bodies, and nested/`is`/array
    /// patterns are deferred.
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

        // Variant names of the scrutinee's union, when its type is known.
        let variants = self.union_variant_names(self.types.get(scrutinee.span()));
        let is_variant = |name: &str| {
            variants
                .as_ref()
                .is_some_and(|vs| vs.iter().any(|v| v == name))
        };

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
                // A bare identifier is a no-payload variant only when the
                // scrutinee type confirms it; otherwise it is a binding
                // (catch-all), which is deferred.
                Pattern::Ident { name, span } => {
                    if !is_variant(name) {
                        return Err(EmitError::Unsupported {
                            construct: "a binding match arm (a bare identifier that is not a variant)",
                            span: *span,
                        });
                    }
                }
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
            self.line(&format!("({scrut});"));
            // No switch here, so no `break`.
            self.emit_arm_body(&arms[0].body, term, false)?;
            return Ok(());
        }

        let scrut = self.expr(scrutinee)?;
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
                    self.line(&format!("case \"{variant}\": {{"));
                    self.indent += 1;
                    self.emit_arm_binds(&m, args);
                    self.emit_arm_body(&arm.body, term, true)?;
                    self.indent -= 1;
                    self.line("}");
                }
                // A bare no-payload variant: a `case` with no payload binding.
                Pattern::Ident { name, .. } => {
                    self.line(&format!("case \"{name}\": {{"));
                    self.indent += 1;
                    self.emit_arm_body(&arm.body, term, true)?;
                    self.indent -= 1;
                    self.line("}");
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
        let has_catch_all = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Wildcard { .. } | Pattern::Else { .. }));
        if !has_catch_all {
            self.line("default: throw new Error(\"non-exhaustive match\");");
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

        let scrut = self.expr(scrutinee)?;
        let m = self.fresh_temp("__m");
        self.line(&format!("const {m} = {scrut};"));

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
            None => self.line("throw new Error(\"non-exhaustive match\");"),
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// The runtime check for an `is T` pattern against the temporary `m`, or
    /// None for a type the emitter cannot check yet (a union, a generic, an
    /// imported or non-record named type).
    fn is_check(&self, ty: &TypeExpr, m: &str) -> Option<String> {
        match ty {
            TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                if let Some(jt) = js_typeof(ty) {
                    Some(format!("typeof {m} === \"{jt}\""))
                } else if self.is_local_record_type(segments[0].as_ref()) {
                    Some(format!("{}.is({m})", segments[0]))
                } else {
                    None
                }
            }
            TypeExpr::Generic { base, .. } => match base.as_ref() {
                TypeExpr::Path { segments, .. } => match segments.last().map(|s| s.as_ref()) {
                    // A Glyph record is a plain object, not an array; exclude
                    // arrays so an `is Array<...>` arm after `is Record<...>`
                    // isn't dead.
                    Some("Record") => Some(format!(
                        "typeof {m} === \"object\" && {m} !== null && !Array.isArray({m})"
                    )),
                    Some("Array") => Some(format!("Array.isArray({m})")),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    /// True if `name` is a module-local non-generic record type — one with an
    /// emitted `is` descriptor this `is` check can call.
    fn is_local_record_type(&self, name: &str) -> bool {
        self.module.items.iter().any(|d| {
            matches!(d, Decl::Type(t)
                if t.name.as_ref() == name
                    && t.generics.is_empty()
                    && matches!(t.body, TypeExpr::Record { .. }))
        })
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
            MatchArmBody::Expr(e) => {
                let s = self.expr(e)?;
                match term {
                    ArmTerm::Return => self.line(&format!("return {s};")),
                    ArmTerm::Break => {
                        self.line(&format!("{s};"));
                        if break_on_fall {
                            self.line("break;");
                        }
                    }
                }
            }
            // A block arm emits its statements directly into the case/branch. A
            // block in a `return match` is expected to `return`; a statement-
            // position block runs for effect and, inside a `switch`, breaks
            // afterward. Block arms are rejected in value position (the IIFE) by
            // the caller, since a block `return` there means function-return.
            MatchArmBody::Block(b) => {
                // Conservative divergence check: does the block end in a
                // statement that exits? It under-approximates (a trailing
                // `loop {}` or exhaustive nested `match` also diverges), which
                // is safe — it only ever adds a redundant `break` or rejects a
                // valid arm, never falls through. A precise CFG check (cf.
                // `owned.rs`) is the proper future fix.
                let diverges = matches!(
                    b.stmts.last(),
                    Some(Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_))
                );
                // In return position the arm must yield the match value, so a
                // non-diverging block would fall through with no value. Reject
                // rather than emit that fall-through; the typechecker does not
                // yet require return-arm divergence.
                if matches!(term, ArmTerm::Return) && !diverges {
                    return Err(EmitError::Unsupported {
                        construct: "a `return match` block arm that does not end in `return`",
                        span: b.span,
                    });
                }
                for stmt in &b.stmts {
                    self.emit_stmt(stmt)?;
                }
                if matches!(term, ArmTerm::Break) && !diverges && break_on_fall {
                    self.line("break;");
                }
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
                let mut sub = self.sub(self.indent);
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
                // A block arm's `return` means function-return; inside the IIFE
                // arrow it would return from the arrow instead, so value-position
                // block arms are rejected.
                if let Some(b) = arms.iter().find_map(|a| match &a.body {
                    MatchArmBody::Block(b) => Some(b),
                    _ => None,
                }) {
                    return Err(EmitError::Unsupported {
                        construct: "a block body in a value-position match arm",
                        span: b.span,
                    });
                }
                let mut sub = self.sub(self.indent + 1);
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

/// One field's runtime check inside a record descriptor's `is` predicate.
/// Primitive fields are checked by `typeof`; other fields by presence
/// (shallow validation). An optional field passes when it is absent.
fn record_field_check(field: &RecordTypeField) -> String {
    let access = format!("(value as Record<string, unknown>).{}", field.name);
    let present = format!("\"{}\" in (value as object)", field.name);
    let check = match js_typeof(&field.ty) {
        Some(jt) => format!("typeof {access} === \"{jt}\""),
        None => present.clone(),
    };
    if field.optional {
        format!("(!({present}) || {check})")
    } else {
        check
    }
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
    ) {
        let module = glyph_parser::parse(src).expect("parse failed");
        let syms = glyph_resolver::collect_module_symbols(&module).expect("collect failed");
        let prelude = glyph_resolver::build_prelude();
        let (resolved, _errs) = glyph_resolver::resolve_module(&module, syms, &prelude);
        let (types, _ty_errs) = glyph_typechecker::assign_types(&module, &resolved, &prelude);
        (module, resolved, types)
    }

    fn emit(src: &str) -> String {
        let (module, resolved, types) = pipeline(src);
        emit_module(&module, &resolved, &types).expect("emit failed")
    }

    fn emit_err(src: &str) -> EmitError {
        let (module, resolved, types) = pipeline(src);
        emit_module(&module, &resolved, &types).expect_err("expected emit error")
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
    fn record_type_emits_an_is_descriptor() {
        let ts = emit(
            "module x\ntype User = { id: string, age: number, admin?: bool, parent: User }\n",
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
        // Optional field: passes when absent.
        assert!(
            ts.contains("(!(\"admin\" in (value as object)) || typeof (value as Record<string, unknown>).admin === \"boolean\")"),
            "{ts}"
        );
        // Non-primitive field: presence check only (shallow).
        assert!(ts.contains("&& \"parent\" in (value as object);"), "{ts}");
    }

    #[test]
    fn primitive_alias_gets_no_descriptor() {
        let ts = emit("module x\ntype Sku = string\n");
        assert!(ts.contains("export type Sku = string;"), "{ts}");
        assert!(!ts.contains("export const Sku"), "{ts}");
    }

    #[test]
    fn generic_record_gets_no_descriptor() {
        let ts = emit("module x\ntype Box<T> = { value: T }\n");
        assert!(ts.contains("export type Box<T> = { value: T };"), "{ts}");
        assert!(!ts.contains("export const Box"), "{ts}");
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
            "module x\nfn parse(n: number) -> Result<number, string> { return Ok(n) }\nfn load(n: number) -> Result<number, string> {\n  let x = parse(n)?\n  return Ok(x)\n}\n",
        );
        assert!(ts.contains("const __r0 = parse(n);"), "{ts}");
        assert!(
            ts.contains("if (__r0.tag === \"Err\") { return __r0; }"),
            "{ts}"
        );
        assert!(ts.contains("let x = __r0.value;"), "{ts}");
    }

    #[test]
    fn try_operator_as_statement_propagates_only() {
        let ts = emit(
            "module x\nfn step() -> Result<number, string> { return Ok(0) }\nfn run() -> Result<number, string> {\n  step()?\n  return Ok(1)\n}\n",
        );
        assert!(ts.contains("const __r0 = step();"), "{ts}");
        assert!(
            ts.contains("if (__r0.tag === \"Err\") { return __r0; }"),
            "{ts}"
        );
        // No value binding for a bare `?` statement.
        assert!(!ts.contains(".value"), "{ts}");
    }

    #[test]
    fn value_match_switches_on_the_scrutinee() {
        let ts = emit(
            "module x\nfn sign(n: number) -> string {\n  return match n {\n    0 => \"zero\",\n    1 => \"one\",\n    else => \"many\",\n  }\n}\n",
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
            "module x\nfn flag(b: bool) -> number {\n  return match b {\n    true => 1,\n    false => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case true: {"), "{ts}");
        assert!(ts.contains("case false: {"), "{ts}");
        assert!(
            ts.contains("default: throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
    }

    #[test]
    fn is_match_lowers_to_an_if_chain_and_calls_the_descriptor() {
        let ts = emit(
            "module x\ntype User = { id: string }\nfn check(v: unknown) -> string {\n  return match v {\n    is string => \"str\",\n    is number => \"num\",\n    is User => \"user\",\n    else => \"other\",\n  }\n}\n",
        );
        assert!(ts.contains("if (typeof __m0 === \"string\") {"), "{ts}");
        assert!(ts.contains("} else if (typeof __m0 === \"number\") {"), "{ts}");
        // The `is User` arm consumes the Q8 record descriptor.
        assert!(ts.contains("} else if (User.is(__m0)) {"), "{ts}");
        assert!(ts.contains("} else {"), "{ts}");
        assert!(ts.contains("return \"other\";"), "{ts}");
        // It is an if-chain, not a switch.
        assert!(!ts.contains("switch"), "{ts}");
    }

    #[test]
    fn is_match_without_else_throws() {
        let ts = emit(
            "module x\nfn f(v: unknown) -> string {\n  return match v {\n    is string => \"s\",\n    is number => \"n\",\n  }\n}\n",
        );
        assert!(
            ts.contains("} else {\n    throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
    }

    #[test]
    fn is_record_and_array_checks() {
        let ts = emit(
            "module x\nfn f(v: unknown) -> string {\n  return match v {\n    is Array<string> => \"arr\",\n    is Record<string, unknown> => \"obj\",\n    else => \"x\",\n  }\n}\n",
        );
        assert!(ts.contains("if (Array.isArray(__m0)) {"), "{ts}");
        // `is Record` excludes arrays so an `is Array` arm isn't shadowed.
        assert!(
            ts.contains("} else if (typeof __m0 === \"object\" && __m0 !== null && !Array.isArray(__m0)) {"),
            "{ts}"
        );
    }

    #[test]
    fn is_match_with_two_catch_alls_is_rejected() {
        let err = emit_err(
            "module x\nfn f(v: unknown) -> number {\n  return match v {\n    is string => 1,\n    else => 2,\n    else => 3,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("catch-all")),
            "got {err:?}"
        );
    }

    #[test]
    fn is_check_on_unsupported_type_is_rejected() {
        let err = emit_err(
            "module x\ntype S = A | B\nfn f(v: unknown) -> number {\n  return match v {\n    is S => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("`is` check")),
            "got {err:?}"
        );
    }

    #[test]
    fn mixed_literal_and_variant_match_is_rejected() {
        // A literal arm and a variant arm in one match would switch some arms
        // on the value and others on the tag; reject rather than misemit.
        let err = emit_err(
            "module x\ntype S = Idle | Busy\nfn f(s: S) -> number {\n  return match s {\n    0 => 1,\n    Idle => 2,\n    else => 9,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("mixing")),
            "got {err:?}"
        );
    }

    #[test]
    fn string_value_match_quotes_case_labels() {
        let ts = emit(
            "module x\nfn parse(s: string) -> number {\n  return match s {\n    \"yes\" => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(ts.contains("case \"yes\": {"), "{ts}");
    }

    #[test]
    fn nested_try_in_expression_is_unsupported_for_now() {
        let err = emit_err(
            "module x\nfn p() -> Result<number, string> { return Ok(0) }\nfn run() -> Result<number, string> {\n  return Ok(p()?)\n}\n",
        );
        assert!(matches!(
            err,
            EmitError::Unsupported {
                construct: "the `?` operator",
                ..
            }
        ));
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
    fn statement_block_arm_emits_block_statements() {
        let ts = emit(
            "module x\ntype E = A | B\nfn f(e: E) -> number {\n  match e {\n    A => {\n      let x = 1\n      return x\n    },\n    B => {\n      return 2\n    },\n  }\n  return 0\n}\n",
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
            "module x\ntype E = A | B\nfn nop(n: number) -> void { return void }\nfn f(e: E) -> void {\n  match e {\n    A => {\n      nop(1)\n    },\n    B => {\n      nop(2)\n    },\n  }\n  return void\n}\n",
        );
        assert!(ts.contains("nop(1);"), "{ts}");
        assert!(ts.contains("break;"), "{ts}");
    }

    #[test]
    fn return_match_block_arm_without_return_is_rejected() {
        // A non-returning block arm in a `return match` would fall through to
        // the next case; reject rather than emit that.
        let err = emit_err(
            "module x\ntype E = A | B\nfn nop(n: number) -> void { return void }\nfn f(e: E) -> number {\n  return match e {\n    A => { nop(1) },\n    B => { return 2 },\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("does not end in `return`")),
            "got {err:?}"
        );
    }

    #[test]
    fn value_position_block_arm_is_unsupported() {
        let err = emit_err(
            "module x\ntype E = A | B\nfn f(e: E) -> number {\n  let x = match e {\n    A => { return 0 },\n    B => { return 1 },\n  }\n  return x\n}\n",
        );
        assert!(matches!(
            err,
            EmitError::Unsupported {
                construct: "a block body in a value-position match arm",
                ..
            }
        ));
    }

    #[test]
    fn bare_variant_match_lowers_to_cases() {
        // With the scrutinee type known, `Idle`/`Busy` are recognized as
        // no-payload variants and become `case` labels (not bindings).
        let ts = emit(
            "module x\ntype S = Idle | Busy\nfn f(s: S) -> number {\n  return match s {\n    Idle => 0,\n    Busy => 1,\n  }\n}\n",
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
            "module x\ntype State =\n  | Idle\n  | Loading\n  | Loaded({ users: number })\n  | Failed({ message: string })\nfn show(s: State) -> number {\n  return match s {\n    Idle => 0,\n    Loading => 1,\n    Loaded({ users }) => users,\n    Failed({ message }) => 2,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Idle\": {"), "{ts}");
        assert!(ts.contains("case \"Loaded\": {"), "{ts}");
        assert!(ts.contains("const users = __m0.users;"), "{ts}");
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
    fn generic_tagged_union_emits_with_type_params() {
        let ts = emit("module x\ntype Box<T> =\n  | Full({ value: T })\n  | Empty\n");
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
}
