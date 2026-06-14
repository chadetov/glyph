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
    ArrayElem, BinOp, Block, ComponentDecl, Decl, Expr, FnTypeParam, GenericParam, Ident,
    ImportDecl, ImportKind, JsxAttr, JsxChild, JsxElement, LiteralPattern, MatchArm, MatchArmBody,
    Module, MutKind, ObjectField, Param, Pattern, PostfixOp, RecordTypeField, Span, Stmt,
    TemplatePart, TypeExpr, UnaryOp, UnionVariant,
};
use glyph_resolver::{ResolvedModule, SymbolId, SymbolKind};
use glyph_typechecker::{Ty, TypeMap};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

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

    /// Stable diagnostic code (emit range `E03xx`; see `docs/error-codes.md`).
    pub fn code(&self) -> &'static str {
        match self {
            EmitError::Unsupported { .. } => "E0300",
        }
    }

    /// A one-line, actionable fix.
    pub fn help(&self) -> Option<&'static str> {
        match self {
            EmitError::Unsupported { .. } => {
                Some("Rewrite using a construct the v1 emitter supports; see the spec for the supported forms.")
            }
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

/// The local name a record descriptor binds the prelude `schema` factory to,
/// for its auto-generated `T.schema` member (`T.schema = __glyph_schema<T>(...)`).
/// Aliased so it never collides with a user binding; a module that emits any
/// record descriptor gets `import { schema as __glyph_schema } from "std/schema"`.
const SCHEMA_FACTORY: &str = "__glyph_schema";

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
        used_try: Rc::new(Cell::new(false)),
        used_schema: Rc::new(Cell::new(false)),
        return_cast: None,
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
    /// Set once any `?` is lowered (including inside a lambda or value-position
    /// match rendered by a sub-emitter), so the module gets the generated `Err`
    /// import the re-wrap needs. Shared across the main emitter and every
    /// sub-emitter via the `Rc<Cell>`.
    used_try: Rc<Cell<bool>>,
    /// Set once any record descriptor is emitted, so the module gets the
    /// generated `schema` factory import its `T.schema` member needs.
    used_schema: Rc<Cell<bool>>,
    /// The declared return type (rendered) the current function's `return`
    /// values must be cast to, set only when that type references one of the
    /// function's generic parameters (the infer_shape stand-in). `None`
    /// otherwise, and reset for lambdas and value-position match IIFEs (their
    /// returns are not the enclosing function's return).
    return_cast: Option<String>,
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
            used_try: Rc::clone(&self.used_try),
            used_schema: Rc::clone(&self.used_schema),
            // A sub-emitter (lambda body, value-position match IIFE) does not
            // inherit the function's return cast; its returns are not the
            // function's. `emit_fn_block` sets it from the lambda's own type.
            return_cast: None,
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
        // A module that emitted any record descriptor needs the `schema`
        // factory for its `T.schema` member; and a module that lowered any `?`
        // re-wraps the propagated error with the prelude `Err`. Prepend the
        // (aliased) imports now that emission has set the flags. `?`'s import is
        // inserted last so it ends up first.
        if self.used_schema.get() {
            self.out.insert_str(
                0,
                &format!("import {{ schema as {SCHEMA_FACTORY} }} from \"std/schema\";\n\n"),
            );
        }
        if self.used_try.get() {
            self.out.insert_str(
                0,
                &format!("import {{ {RESULT_ERR} as {ERR_CTOR} }} from \"std/result\";\n\n"),
            );
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
                let cast = self.fn_return_cast(&f.return_ty, &f.generics)?;
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
            Decl::Component(c) => self.emit_component(c),
        }
    }

    /// Emit the Q8 runtime descriptor for a record type: an `is` type guard
    /// doing shallow validation (each primitive field checked by `typeof`,
    /// each other field checked for presence), plus a `parse` entry point that
    /// validates an `unknown` and returns a `Result` (`Ok` of the value, or an
    /// `Err` describing the failure). Deep/recursive validation is later work.
    ///
    /// `parse` is deliberately self-contained: it inlines the `Result`
    /// wire-format (the same `tag`/`value` contract the union lowering uses,
    /// single-sourced via `RESULT_OK`/`RESULT_ERR`) rather than referencing the
    /// prelude `Ok`/`Err` constructors, so the descriptor compiles even in a
    /// module that never imports `std/result`. It reaches the sibling `is`
    /// guard through `this` rather than by the descriptor's name, so it stays
    /// correct even for a record whose name shadows the `parse` parameter (a
    /// type literally named `value`).
    ///
    /// **Soundness limitation**: because a non-primitive field is only checked
    /// for presence, the `value is X` narrowing is stronger than the runtime
    /// proof — `User.is({ parent: 42, ... })` returns true even though `parent`
    /// is not a `User` (and `User.parse` inherits the same gap). This is the
    /// documented v1 "shallow validation" scope (`docs/roadmap/04-transpiler.md`);
    /// recursing into a named-record field's own `is` would close the gap and is
    /// the path to full soundness.
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
        // The `parse` entry point reuses the `is` guard, then wraps the value in
        // a `Result`. The return type and the values are the inline `Result`
        // shape; `Ok`'s payload is the narrowed value, `Err`'s is a message.
        let ok_ty = format!("{{ {TAG}: \"{RESULT_OK}\"; {PAYLOAD}: {name} }}");
        let err_ty = format!("{{ {TAG}: \"{RESULT_ERR}\"; {PAYLOAD}: string }}");
        self.line(&format!("parse(value: unknown): {ok_ty} | {err_ty} {{"));
        self.indent += 1;
        // Call the sibling guard through `this`, not by the descriptor's name:
        // a record named after the `value` parameter (or any name) would
        // otherwise be shadowed by the parameter and `.is` would dispatch on
        // the `unknown` argument. `this` is the descriptor object at every call
        // site the compiler emits (`T.parse(x)`).
        self.line("return this.is(value)");
        self.indent += 1;
        self.line(&format!("? {{ {TAG}: \"{RESULT_OK}\", {PAYLOAD}: value }}"));
        self.line(&format!(
            ": {{ {TAG}: \"{RESULT_ERR}\", {PAYLOAD}: \"expected {name}\" }};"
        ));
        self.indent -= 1;
        self.indent -= 1;
        self.line("},");
        // Q8/Q40 `T.schema`: a `Schema<T>` built from the `is` guard by the
        // prelude factory (the factory carries the recursive `array()`). The
        // guard references the descriptor by name in a lazy closure — `this` is
        // not the descriptor object inside this object literal, but the closure
        // only runs once the `const` is initialized.
        self.used_schema.set(true);
        self.line(&format!(
            "schema: {SCHEMA_FACTORY}<{name}>(\"{name}\", (v): v is {name} => {name}.is(v)),"
        ));
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
    /// type guard that checks `value` is an object whose `tag` is one of the
    /// union's variant tags, a self-contained `parse` returning a `Result`, and
    /// a `T.schema` member. Mirrors `emit_record_descriptor`.
    ///
    /// **Soundness limitation** (same as the record descriptor): the guard is
    /// shallow — it checks the discriminant tag but not each variant's payload,
    /// so `X.is({ tag: "A" })` holds even when `A`'s payload fields are absent
    /// or wrong. This is the documented v1 shallow-validation scope; recursing
    /// into each variant's payload would close the gap (later work).
    fn emit_union_descriptor(&mut self, name: &str, variants: &[UnionVariant]) {
        self.line(&format!("export const {name} = {{"));
        self.indent += 1;
        // is(): an object whose discriminant tag names a variant.
        self.line(&format!("is(value: unknown): value is {name} {{"));
        self.indent += 1;
        self.line("if (typeof value !== \"object\" || value === null) {");
        self.indent += 1;
        self.line("return false;");
        self.indent -= 1;
        self.line("}");
        self.line(&format!(
            "const {TAG} = (value as {{ {TAG}?: unknown }}).{TAG};"
        ));
        let tag_checks: Vec<String> = variants
            .iter()
            .map(|v| format!("{TAG} === \"{}\"", v.name))
            .collect();
        self.line(&format!("return {};", tag_checks.join(" || ")));
        self.indent -= 1;
        self.line("},");
        // parse(): reuse the guard, wrap the narrowed value in a Result shape.
        // Inlined wire-format (not the prelude Ok/Err) so the descriptor
        // compiles without a `std/result` import, exactly like the record one.
        let ok_ty = format!("{{ {TAG}: \"{RESULT_OK}\"; {PAYLOAD}: {name} }}");
        let err_ty = format!("{{ {TAG}: \"{RESULT_ERR}\"; {PAYLOAD}: string }}");
        self.line(&format!("parse(value: unknown): {ok_ty} | {err_ty} {{"));
        self.indent += 1;
        self.line("return this.is(value)");
        self.indent += 1;
        self.line(&format!("? {{ {TAG}: \"{RESULT_OK}\", {PAYLOAD}: value }}"));
        self.line(&format!(
            ": {{ {TAG}: \"{RESULT_ERR}\", {PAYLOAD}: \"expected {name}\" }};"
        ));
        self.indent -= 1;
        self.indent -= 1;
        self.line("},");
        // Q8/Q40 `T.schema`: a `Schema<T>` built from the `is` guard.
        self.used_schema.set(true);
        self.line(&format!(
            "schema: {SCHEMA_FACTORY}<{name}>(\"{name}\", (v): v is {name} => {name}.is(v)),"
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
        match &v.payload {
            None => self.line(&format!("export const {name}: {ret} = {{ {TAG}: \"{name}\" }};")),
            // Spread the fields FIRST so the discriminant always wins, even if
            // the record (somehow) carried a colliding key.
            Some(payload @ TypeExpr::Record { .. }) => self.line(&format!(
                "export function {name}{ctor_generics}(fields: {}): {ret} {{ return {{ ...fields, {TAG}: \"{name}\" }}; }}",
                self.ty(payload)?
            )),
            Some(other) => self.line(&format!(
                "export function {name}{ctor_generics}({PAYLOAD}: {}): {ret} {{ return {{ {TAG}: \"{name}\", {PAYLOAD} }}; }}",
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
        for stmt in &block.stmts {
            self.emit_stmt(stmt)?;
        }
        self.indent -= 1;
        self.pad();
        self.out.push('}');
        Ok(())
    }

    /// Emit a function body, applying implicit tail returns when the function
    /// yields a value (a non-`void` return type). The body's final expression
    /// is the returned value (`return expr`); a `void` or unannotated function
    /// runs its tail for effect. A function body is never inside a `switch`, so
    /// no fall-through break is emitted.
    /// The rendered return type a function's `return` values are cast to, or
    /// `None`. A cast is emitted only when the function yields a value AND its
    /// declared return type references one of its own generic parameters — the
    /// infer_shape stand-in (Q1). Non-generic returns stay precisely checked.
    fn fn_return_cast(
        &self,
        return_ty: &Option<TypeExpr>,
        generics: &[GenericParam],
    ) -> Result<Option<String>, EmitError> {
        match return_ty {
            Some(te) if returns_value(return_ty) && type_references_generic(te, generics) => {
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
        match stmt {
            Stmt::Let(l) => {
                // `let` (not `const`): a `mut` statement may reassign it later.
                let ty = match &l.ty {
                    Some(te) => format!(": {}", self.ty(te)?),
                    None => String::new(),
                };
                // `emit_value` hoists any `?` in the initializer first, so both
                // a whole-value `?` (`let x = E?`) and a mid-chain `?` (`let x =
                // await f()?.g()`) propagate the `Err` and bind the `Ok` payload.
                let value = self.emit_value(&l.value)?;
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
                    let v = self.emit_value(v)?;
                    self.emit_return(&v);
                }
                None => self.line("return;"),
            },
            Stmt::For(f) => {
                let iter = self.expr(&f.iter)?;
                self.pad();
                match f.bindings.as_slice() {
                    // `for x in xs` over an array/iterable: a `for...of`.
                    [v] => self.out.push_str(&format!("for (const {v} of {iter}) ")),
                    // `for k, v in it` over key/value pairs. An array's pairs are
                    // `it.entries()` — the index is a NUMBER. A record is a plain
                    // object, so its pairs are `Object.entries(it)` — the key is a
                    // STRING. The two differ (numeric vs string index), so pick by
                    // the iterand type, defaulting to a record when it is unknown.
                    [k, v] => {
                        let pairs = if self.iter_is_array(&f.iter) {
                            format!("{iter}.entries()")
                        } else {
                            format!("Object.entries({iter})")
                        };
                        self.out
                            .push_str(&format!("for (const [{k}, {v}] of {pairs}) "));
                    }
                    _ => {
                        return Err(EmitError::Unsupported {
                            construct: "a `for` loop with more than two bindings",
                            span: f.span,
                        })
                    }
                }
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
                let s = self.emit_value(e)?;
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
    fn iter_is_array(&self, iter: &Expr) -> bool {
        matches!(
            self.types.get(iter.span()),
            Ty::App { base, .. }
                if matches!(base.as_ref(), Ty::Named { path, .. }
                    if path.last().map(|n| n.as_ref()) == Some("Array"))
        )
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
    fn degroup_nested_arms(&mut self, arms: &[MatchArm]) -> Vec<MatchArm> {
        // Outer variant tag -> index in `out` of its synthesized grouping arm.
        let mut group_at: Vec<(String, usize)> = Vec::new();
        let mut out: Vec<MatchArm> = Vec::new();
        for arm in arms {
            let (path, inner) = match &arm.pattern {
                Pattern::Constructor { path, args, .. }
                    if matches!(args.as_slice(), [Pattern::Constructor { .. }]) =>
                {
                    (path, &args[0])
                }
                // Not a nested-constructor arm: keep it as is.
                _ => {
                    out.push(arm.clone());
                    continue;
                }
            };
            let tag = path
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(".");
            let inner_arm = MatchArm {
                pattern: inner.clone(),
                body: arm.body.clone(),
                span: arm.span,
            };
            if let Some((_, idx)) = group_at.iter().find(|(t, _)| *t == tag) {
                if let MatchArmBody::Expr(Expr::Match { arms, .. }) = &mut out[*idx].body {
                    arms.push(inner_arm);
                }
            } else {
                let p = self.fresh_temp("__p");
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
            let rewritten = self.degroup_nested_arms(arms);
            return self.emit_match_dispatch(scrutinee, &rewritten, term);
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
            None => self.line("throw new Error(\"non-exhaustive match\");"),
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
            None => self.line("throw new Error(\"non-exhaustive match\");"),
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
                } else if self.has_local_descriptor(segments[0].as_ref()) {
                    Some(format!("{}.is({m})", segments[0]))
                } else {
                    None
                }
            }
            TypeExpr::Generic { base, .. } => match base.as_ref() {
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
                            "((__x: unknown): __x is {rec} => typeof __x === \"object\" && __x !== null && !Array.isArray(__x))({m})"
                        ))
                    }
                    Some("Array") => Some(format!("Array.isArray({m})")),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    /// True if `name` is a module-local type with an emitted runtime descriptor
    /// whose `is` guard this `is` check can call — a non-generic record, or a
    /// non-generic tagged union (whose descriptor `const` name is free). Mirrors
    /// the emission guards in `emit_decl`/`emit_union`.
    fn has_local_descriptor(&self, name: &str) -> bool {
        self.module.items.iter().any(|d| match d {
            Decl::Type(t) if t.name.as_ref() == name && t.generics.is_empty() => match &t.body {
                TypeExpr::Record { .. } => true,
                TypeExpr::Union { variants, .. } => union_descriptor_name_free(name, variants),
                _ => false,
            },
            _ => false,
        })
    }

    /// Bind a constructor arm's payload from the scrutinee temporary `m`: an
    /// object pattern reads each spread field by name; a single identifier
    /// reads the non-record `value` field; no args and a `_` wildcard
    /// (`Err(_)`) bind nothing.
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
        let Some((last, init)) = stmts.split_last() else {
            // An empty block yields nothing; only a `switch` case needs a break.
            if matches!(term, ArmTerm::Break) && break_on_fall {
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
                if matches!(term, ArmTerm::Break) && break_on_fall {
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
                }
            }
            // A tail that already exits the function or loop emits unchanged; no
            // break is reachable after it.
            Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_) => self.emit_stmt(stmt)?,
            // Any other tail (let/mut/for/loop) yields no value; emit it and, in
            // a `switch` case, break afterward.
            other => {
                self.emit_stmt(other)?;
                if matches!(term, ArmTerm::Break) && break_on_fall {
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
                if matches!(term, ArmTerm::Break) && break_on_fall {
                    self.line("break;");
                }
            }
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
                Ok((format!("{obj}[{}]", self.expr(index)?), awaited))
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
            } => format!("{}{}", self.expr(callee)?, self.call_suffix(type_args, args)?),
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
            // Glyph async is colorless: a call's declared type is its awaited
            // type and `await` may syntactically wrap a whole method chain
            // (`await load(p).map_err(f)`). The emitted async function returns a
            // `Promise`, so the `await` must apply to the async call at the head
            // of the receiver spine, not the chain as a whole — otherwise the
            // chained method is called on a `Promise`. See `emit_await_spine`.
            Expr::Await { expr, .. } => {
                let (chain, awaited) = self.emit_await_spine(expr)?;
                if awaited {
                    chain
                } else {
                    format!("(await {chain})")
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
                let params = self.lambda_params(params)?;
                let ret = match return_ty {
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
            Expr::Jsx(j) => self.emit_jsx(j)?,
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
        let generics = self.generics(&c.generics);
        let params = self.params(&c.params)?;
        let ret = match &c.return_ty {
            Some(te) => format!(": {}", self.ty(te)?),
            None => String::new(),
        };
        self.pad();
        self.out
            .push_str(&format!("export function {}{generics}({params}){ret} ", c.name));
        let cast = self.fn_return_cast(&c.return_ty, &c.generics)?;
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
                let then = self.jsx_node(&j.children)?;
                Ok(format!("({cond} ? {then} : null)"))
            }
            JsxKind::Else => Err(EmitError::Unsupported {
                construct: "an `<else>` without a preceding `<if>`",
                span: j.span,
            }),
            JsxKind::Case => Err(EmitError::Unsupported {
                construct: "a `<case>` outside a `<match>`",
                span: j.span,
            }),
            JsxKind::Intrinsic | JsxKind::Component => self.emit_jsx_element(j, None),
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
        let tag = match JsxKind::classify(&j.name) {
            JsxKind::Intrinsic => escape_double_quoted(&j.name),
            JsxKind::Component => j.name.to_string(),
            _ => unreachable!("directives route through emit_jsx"),
        };
        let props = self.jsx_props(&j.attrs, extra_prop)?;
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
    ) -> Result<String, EmitError> {
        let mut fields: Vec<String> = Vec::new();
        if let Some((k, v)) = extra_prop {
            fields.push(format!("{k}: {v}"));
        }
        for a in attrs {
            match a {
                JsxAttr::String { name, value, .. } => {
                    fields.push(format!("{name}: {}", escape_double_quoted(value)))
                }
                JsxAttr::Expr { name, value, .. } => {
                    fields.push(format!("{name}: {}", self.expr(value)?))
                }
                JsxAttr::Positional { span, .. } => {
                    return Err(EmitError::Unsupported {
                        construct: "a positional attribute on a non-directive JSX element",
                        span: *span,
                    })
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
                        let then = self.jsx_node(&el.children)?;
                        // A following `<else>` sibling (past whitespace) is the
                        // alternative; otherwise the alternative is `null`.
                        let (alt, else_idx) = self.find_else(children, i + 1)?;
                        out.push(format!("({cond} ? {then} : {alt})"));
                        if let Some(e) = else_idx {
                            i = e;
                        }
                    }
                    JsxKind::Else => {
                        return Err(EmitError::Unsupported {
                            construct: "an `<else>` without a preceding `<if>`",
                            span: el.span,
                        })
                    }
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
                    return Ok((self.jsx_node(&el.children)?, Some(j)));
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
                    let node = self.jsx_node(&el.children)?;
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

/// True when a tagged union's descriptor `const <name>` would not collide with
/// any variant constructor `const`/`function`. A union with a variant sharing
/// its own name cannot also carry a descriptor under that name, so the
/// descriptor is skipped in that degenerate case.
fn union_descriptor_name_free(name: &str, variants: &[UnionVariant]) -> bool {
    variants.iter().all(|v| v.name.as_ref() != name)
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

/// Whether `arm` is a constructor pattern carrying a single nested constructor
/// argument (`Err(NetworkError({ status }))`), which needs an inner switch on
/// the payload's tag. A whole-payload bind (`Err(e)`), an object destructure
/// (`Err({ ... })`), or a wildcard (`Err(_)`) is not nested.
fn arm_has_nested_constructor(arm: &MatchArm) -> bool {
    matches!(
        &arm.pattern,
        Pattern::Constructor { args, .. } if matches!(args.as_slice(), [Pattern::Constructor { .. }])
    )
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
    match e {
        Expr::Postfix {
            op: PostfixOp::Try, ..
        } => true,
        Expr::Binary { left, right, .. } => {
            contains_hoistable_try(left) || contains_hoistable_try(right)
        }
        Expr::Index {
            object: a,
            index: b,
            ..
        } => contains_hoistable_try(a) || contains_hoistable_try(b),
        Expr::Unary { operand: x, .. } | Expr::Await { expr: x, .. } | Expr::Member { object: x, .. } => {
            contains_hoistable_try(x)
        }
        Expr::Call { callee, args, .. } => {
            contains_hoistable_try(callee) || args.iter().any(contains_hoistable_try)
        }
        Expr::Array { elements, .. } => elements.iter().any(|el| match el {
            ArrayElem::Expr(e) | ArrayElem::Spread(e) => contains_hoistable_try(e),
        }),
        Expr::Object { fields, .. } => fields.iter().any(|f| match f {
            ObjectField::KeyValue { value, .. } | ObjectField::Spread { value, .. } => {
                contains_hoistable_try(value)
            }
        }),
        Expr::TemplateString { parts, .. } => parts.iter().any(|p| match p {
            TemplatePart::Expr { value, .. } => contains_hoistable_try(value),
            TemplatePart::Text { .. } => false,
        }),
        // Leaves and opaque constructs (lambda/match/JSX).
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
    If,
    Else,
    For,
    Match,
    Case,
}

impl JsxKind {
    fn classify(name: &Ident) -> Self {
        match name.as_ref() {
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

/// Normalize JSX text: whitespace-only runs (the newlines and indentation
/// between tags) become empty and are dropped by the caller; other text is
/// trimmed with internal whitespace runs collapsed to a single space.
fn normalize_jsx_text(content: &str) -> String {
    if content.trim().is_empty() {
        return String::new();
    }
    content.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// Whether `te` references any of `generics` by name. A function whose declared
/// return type mentions one of its own type parameters and whose body builds a
/// concrete value (e.g. `object_schema<Out>` returning a `Record` as `Out`)
/// asserts that value matches the caller's type parameter — the v1 stand-in for
/// `infer_shape` (Q1). The emitter casts such a return to its declared type;
/// non-generic returns are checked precisely with no cast.
fn type_references_generic(te: &TypeExpr, generics: &[GenericParam]) -> bool {
    if generics.is_empty() {
        return false;
    }
    let is_gen = |name: &str| generics.iter().any(|g| g.name.as_ref() == name);
    match te {
        TypeExpr::Path { segments, .. } => segments.iter().any(|s| is_gen(s)),
        TypeExpr::Generic { base, args, .. } => {
            type_references_generic(base, generics)
                || args.iter().any(|a| type_references_generic(a, generics))
        }
        TypeExpr::Fn {
            params, return_ty, ..
        } => {
            params
                .iter()
                .any(|p: &FnTypeParam| type_references_generic(&p.ty, generics))
                || return_ty
                    .as_ref()
                    .is_some_and(|r| type_references_generic(r, generics))
        }
        TypeExpr::Record { fields, .. } => {
            fields.iter().any(|f| type_references_generic(&f.ty, generics))
        }
        TypeExpr::Union { .. } => false,
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
    fn record_descriptor_emits_a_self_contained_parse() {
        let ts = emit("module x\ntype User = { id: string }\n");
        // `parse` returns the inline `Result` shape — no `std/result` import,
        // no `Ok`/`Err` constructor reference; the only name it mentions is the
        // record type itself.
        assert!(
            ts.contains(
                "parse(value: unknown): { tag: \"Ok\"; value: User } | { tag: \"Err\"; value: string } {"
            ),
            "{ts}"
        );
        // It reuses the `is` guard (reached via `this`, never by name) and
        // wraps the value in `Ok`/`Err` literals.
        assert!(ts.contains("return this.is(value)"), "{ts}");
        assert!(ts.contains("? { tag: \"Ok\", value: value }"), "{ts}");
        assert!(
            ts.contains(": { tag: \"Err\", value: \"expected User\" };"),
            "{ts}"
        );
        // `parse` itself pulls in no `std/result` import (it inlines the shape).
        assert!(!ts.contains("from \"std/result\""), "{ts}");
    }

    #[test]
    fn record_descriptor_emits_a_schema_member() {
        let ts = emit("module x\ntype User = { id: string }\n");
        // `T.schema` is a `Schema<T>` built by the prelude factory from the `is`
        // guard (referenced by name in a lazy closure, since `this` is not the
        // descriptor object inside the object literal).
        assert!(
            ts.contains(
                "schema: __glyph_schema<User>(\"User\", (v): v is User => User.is(v)),"
            ),
            "{ts}"
        );
        // The module that emits a descriptor gets the aliased factory import.
        assert!(
            ts.starts_with("import { schema as __glyph_schema } from \"std/schema\";"),
            "{ts}"
        );
    }

    #[test]
    fn parse_does_not_shadow_a_record_named_value() {
        // A record literally named `value` collides with the `parse` parameter.
        // Reaching the guard via `this` (not `value.is(...)`) keeps the emitted
        // TS valid: the parameter no longer shadows the descriptor binding.
        let ts = emit("module x\ntype value = { id: string }\n");
        assert!(ts.contains("return this.is(value)"), "{ts}");
        assert!(!ts.contains("return value.is(value)"), "{ts}");
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
            "module x\nimport std/result { Ok, Err }\nimport std/io\nimport std/http as h\nfn noop() -> void { return void }\n",
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
    fn await_on_a_method_chain_awaits_the_head_call() {
        // Glyph async is colorless: `await load().map_err(id)` must await the
        // async call `load()`, not the whole chain, so the chained `.map_err`
        // runs on the awaited `Result` and not on a `Promise`.
        let ts = emit(
            "module x\nasync fn load() -> Result<number, string> { return Ok(0) }\nfn id(e: string) -> string { return e }\nasync fn run() -> Result<number, string> {\n  let r = await load().map_err(id)\n  return r\n}\n",
        );
        assert!(ts.contains("(await load()).map_err(id)"), "{ts}");
        assert!(!ts.contains("(await load().map_err"), "{ts}");
    }

    #[test]
    fn plain_await_of_a_call_is_unchanged() {
        // A bare `await f()` still awaits the call directly.
        let ts = emit(
            "module x\nasync fn f() -> number { return 1 }\nasync fn run() -> number {\n  return await f()\n}\n",
        );
        assert!(ts.contains("return (await f());"), "{ts}");
    }

    #[test]
    fn two_binding_for_iterates_record_entries() {
        // `for k, v in rec` over a record lowers to `Object.entries` with an
        // array-destructure binding. This is example 01's `for key, sub_schema
        // in shape` shape.
        let ts = emit(
            "module x\nfn f(rec: Record<string, number>) -> void {\n  for k, v in rec {\n    log(k)\n    log(v)\n  }\n  return void\n}\n",
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
            "module x\nfn f(xs: Array<string>) -> void {\n  for i, item in xs {\n    log(i)\n  }\n  return void\n}\n",
        );
        assert!(
            ts.contains("for (const [i, item] of xs.entries()) {"),
            "{ts}"
        );
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
        // A module using `?` gets the aliased `Err` import the re-wrap needs.
        assert!(
            ts.starts_with("import { Err as __glyph_err } from \"std/result\";"),
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
    fn try_operator_as_statement_propagates_only() {
        let ts = emit(
            "module x\nfn step() -> Result<number, string> { return Ok(0) }\nfn run() -> Result<number, string> {\n  step()?\n  return Ok(1)\n}\n",
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
            "module x\nfn f(v: unknown) -> string {\n  return match v {\n    is string => \"s\",\n    is number => \"n\",\n  }\n}\n",
        );
        assert!(
            ts.contains("} else {\n    throw new Error(\"non-exhaustive match\");"),
            "{ts}"
        );
    }

    #[test]
    fn array_match_lowers_to_a_length_and_element_if_chain() {
        let ts = emit(
            "module x\nfn f(argv: Array<string>) -> string {\n  return match argv {\n    [] => \"empty\",\n    [\"add\", ...rest] => \"add\",\n    [\"list\", \"--all\"] => \"la\",\n    [\"get\", id] => id,\n    [other, ..._] => other,\n  }\n}\n",
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
            "module x\nfn f(argv: Array<string>) -> string {\n  return match argv {\n    [] => \"empty\",\n    else => \"other\",\n  }\n}\n",
        );
        assert!(ts.contains("if (__m0.length === 0) {"), "{ts}");
        assert!(ts.contains("} else {"), "{ts}");
        assert!(ts.contains("return \"other\";"), "{ts}");
        assert!(!ts.contains("non-exhaustive match"), "{ts}");
    }

    #[test]
    fn is_record_and_array_checks() {
        let ts = emit(
            "module x\nfn f(v: unknown) -> string {\n  return match v {\n    is Array<string> => \"arr\",\n    is Record<string, unknown> => \"obj\",\n    else => \"x\",\n  }\n}\n",
        );
        // Identifier scrutinee is checked directly so it narrows in the arms.
        assert!(ts.contains("if (Array.isArray(v)) {"), "{ts}");
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
            "module x\nfn f(v: unknown) -> number {\n  return match v {\n    is string => 1,\n    else => 2,\n    else => 3,\n  }\n}\n",
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
            "module x\ntype S<T> = A | B(T)\nfn f(v: unknown) -> number {\n  return match v {\n    is S => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("`is` check")),
            "got {err:?}"
        );
    }

    #[test]
    fn union_type_emits_an_is_descriptor() {
        let ts = emit("module x\ntype S = A | B\nfn f() {}\n");
        assert!(ts.contains("export const S = {"), "{ts}");
        assert!(ts.contains("is(value: unknown): value is S {"), "{ts}");
        assert!(ts.contains("=== \"A\""), "{ts}");
        assert!(ts.contains("=== \"B\""), "{ts}");
    }

    #[test]
    fn union_descriptor_emits_parse_and_schema() {
        let ts = emit("module x\ntype S = A | B\nfn f() {}\n");
        assert!(ts.contains("parse(value: unknown):"), "{ts}");
        assert!(ts.contains("return this.is(value)"), "{ts}");
        assert!(ts.contains("schema: __glyph_schema<S>(\"S\""), "{ts}");
    }

    #[test]
    fn is_union_type_calls_its_descriptor() {
        let ts = emit(
            "module x\ntype S = A | B\nfn f(v: unknown) -> number {\n  return match v {\n    is S => 1,\n    else => 0,\n  }\n}\n",
        );
        assert!(ts.contains("S.is("), "{ts}");
    }

    #[test]
    fn generic_union_emits_no_descriptor() {
        // The alias and constructors are generic; no `const S = {` descriptor.
        let ts = emit("module x\ntype S<T> = A | B(T)\nfn f() {}\n");
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
    fn nested_try_in_an_argument_is_hoisted() {
        // A `?` nested inside a call argument hoists its unwrap before the
        // statement and substitutes the `Ok` payload.
        let ts = emit(
            "module x\nfn p() -> Result<number, string> { return Ok(0) }\nfn run() -> Result<number, string> {\n  return Ok(p()?)\n}\n",
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
            "module x\nasync fn run(url: string) -> Result<number, string> {\n  let response = await get(url)?\n    .map_err(fn(e) { return e })\n  return Ok(0)\n}\n",
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
            "module x\nfn a() -> Result<number, string> { return Ok(1) }\nfn b() -> Result<number, string> { return Ok(2) }\nfn s(x: number, y: number) -> number { return x }\nfn f() -> Result<number, string> {\n  return Ok(s(a()?, b()?))\n}\n",
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
            "module x\nfn a() -> Result<number, string> { return Ok(1) }\nfn f() -> Result<Array<number>, string> {\n  return Ok([a()?, a()?])\n}\n",
        );
        assert!(ts.contains("return Ok([__r0.value, __r1.value]);"), "{ts}");
    }

    #[test]
    fn empty_jsx_element_emits_null_props_and_no_children() {
        let ts = emit(
            "module x\nimport react { Component }\ncomponent V() -> Component {\n  return <div></div>\n}\n",
        );
        assert!(ts.contains("React.createElement(\"div\", null)"), "{ts}");
    }

    #[test]
    fn nested_jsx_for_inside_if_lowers() {
        // A `<for>` nested inside an `<if>` branch, paired with an `<else>`.
        let ts = emit(
            "module x\nimport react { Component }\ncomponent V(xs: Array<string>) -> Component {\n  return <ul>\n    <if cond={true}>\n      <for x in={xs}><li>{x}</li></for>\n    </if>\n    <else><p>empty</p></else>\n  </ul>\n}\n",
        );
        assert!(
            ts.contains("(true ? xs.map((x) => React.createElement(\"li\", null, x)) : React.createElement(\"p\", null, \"empty\"))"),
            "{ts}"
        );
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
    fn return_match_block_arm_implicitly_returns_its_tail() {
        // A block arm in a `return match` whose last statement is a bare
        // expression implicitly returns that value (like Rust), rather than
        // being rejected for not ending in `return`.
        let ts = emit(
            "module x\ntype E = A | B\nfn f(e: E) -> number {\n  return match e {\n    A => {\n      let x = 1\n      x\n    },\n    B => 2,\n  }\n}\n",
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
        let ts = emit("module x\nfn f() -> number {\n  let y = 1\n  y + 41\n}\n");
        assert!(ts.contains("let y = 1;"), "{ts}");
        assert!(ts.contains("return (y + 41);"), "{ts}");
    }

    #[test]
    fn tail_match_in_a_function_body_returns_each_arm_value() {
        // Example 04's `run` shape: the function body is a bare `match` whose
        // arms end in bare expressions. The match is in tail position, so each
        // arm `return`s its value rather than dropping it.
        let ts = emit(
            "module x\ntype E = A | B\nfn f(e: E) -> number {\n  match e {\n    A => 0,\n    B => 1,\n  }\n}\n",
        );
        assert!(ts.contains("switch (__m0.tag) {"), "{ts}");
        assert!(ts.contains("return 0;"), "{ts}");
        assert!(ts.contains("return 1;"), "{ts}");
    }

    #[test]
    fn void_function_runs_its_tail_for_effect() {
        // A `void` function does not implicitly return; its tail expression
        // runs for effect.
        let ts = emit("module x\nfn f() -> void {\n  log(1)\n}\n");
        assert!(ts.contains("log(1);"), "{ts}");
        assert!(!ts.contains("return"), "{ts}");
    }

    #[test]
    fn generic_return_casts_the_returned_value() {
        // A function whose return type references its own generic parameter
        // casts its return value to that type — the v1 infer_shape stand-in, so
        // a value the body cannot prove matches the caller's type parameter
        // (e.g. `object_schema<Out>` returning a `Record` as `Out`) type-checks.
        let ts = emit("module x\nfn id<T>(x: T) -> T { return x }\n");
        assert!(ts.contains("return x as T;"), "{ts}");
    }

    #[test]
    fn non_generic_return_is_not_cast() {
        // A non-generic return type is checked precisely, no cast.
        let ts = emit("module x\nfn f() -> number { return 1 }\n");
        assert!(ts.contains("return 1;"), "{ts}");
        assert!(!ts.contains(" as number"), "{ts}");
    }

    #[test]
    fn a_returned_lambda_body_does_not_inherit_the_generic_cast() {
        // The cast sits on the function's own returned value; a lambda the
        // function returns keeps its own (un-cast) returns — the outer cast
        // already covers the lambda field's mismatch.
        let ts = emit(
            "module x\nfn mk<T>(v: T) -> fn() -> T {\n  return fn() { v }\n}\n",
        );
        // The function return is cast; the lambda's `v` is not.
        assert!(ts.contains("as () => T;"), "{ts}");
        assert!(!ts.contains("v as T"), "{ts}");
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
    fn nested_constructor_pattern_emits_a_grouped_inner_switch() {
        // Example 02's shape: `Err(NetworkError({ status }))` over `Result<T,
        // FeedError>` dispatches the outer Ok/Err tag, then the Err payload's
        // inner FeedError tag. The three `Err(..)` arms collapse to one outer
        // `case "Err"` carrying an inner switch.
        let ts = emit(
            "module x\ntype FeedError =\n  | NetworkError({ status: number })\n  | DecodeError({ reason: string })\nfn handle(r: Result<number, FeedError>) -> number {\n  return match r {\n    Ok(v) => v,\n    Err(NetworkError({ status })) => status,\n    Err(DecodeError({ reason })) => 0,\n  }\n}\n",
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
            "module x\nfn apply(f: fn(n: number) -> number) -> number { return f(1) }\nfn use_it() -> number {\n  return apply(fn(n) { n + 1 })\n}\n",
        );
        assert!(ts.contains("(n) => {"), "{ts}");
        assert!(ts.contains("return (n + 1);"), "{ts}");
    }

    #[test]
    fn explicitly_typed_lambda_param_keeps_its_annotation() {
        let ts = emit(
            "module x\nfn apply(f: fn(n: number) -> number) -> number { return f(1) }\nfn use_it() -> number {\n  return apply(fn(n: number) { n + 1 })\n}\n",
        );
        assert!(ts.contains("(n: number) => {"), "{ts}");
    }

    #[test]
    fn component_emits_a_react_function_with_create_element() {
        let ts = emit(
            "module x\ncomponent Greeting(name: string) -> Component {\n  return <div class=\"g\">{name}</div>\n}\n",
        );
        assert!(ts.contains("import * as React from \"react\";"), "{ts}");
        assert!(
            ts.contains("export function Greeting(name: string): Component {"),
            "{ts}"
        );
        assert!(
            ts.contains("return React.createElement(\"div\", { class: \"g\" }, name);"),
            "{ts}"
        );
    }

    #[test]
    fn jsx_match_lowers_to_a_switch_returning_iife() {
        let ts = emit(
            "module x\ntype S =\n  | Idle\n  | Loaded({ items: number })\ncomponent V(s: S) -> Component {\n  return <match value={s}>\n    <case Idle><p>idle</p></case>\n    <case Loaded bind={items}><p>{items}</p></case>\n  </match>\n}\n",
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
            "module x\ncomponent V(flag: bool) -> Component {\n  return <div>\n    <if cond={flag}><p>yes</p></if>\n    <else><p>no</p></else>\n  </div>\n}\n",
        );
        assert!(
            ts.contains("(flag ? React.createElement(\"p\", null, \"yes\") : React.createElement(\"p\", null, \"no\"))"),
            "{ts}"
        );
    }

    #[test]
    fn jsx_for_lowers_to_map_with_key_merged() {
        let ts = emit(
            "module x\ncomponent V(xs: Array<string>) -> Component {\n  return <ul>\n    <for x in={xs} key={x}><li>{x}</li></for>\n  </ul>\n}\n",
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
            "module x\ntype C = A | B\nfn run(c: C) -> number {\n  return match c {\n    A => match c {\n      A => 0,\n      B => 1,\n    },\n    B => 2,\n  }\n}\n",
        );
        // Two switches (outer + nested), no value IIFE wrapper.
        assert_eq!(ts.matches("switch (").count(), 2, "{ts}");
        assert!(!ts.contains("(() =>"), "{ts}");
        assert!(ts.contains("return 0;") && ts.contains("return 1;"), "{ts}");
    }

    #[test]
    fn binding_arm_alongside_a_variant_lowers_to_a_default() {
        // Example 04's shape: `array.find` is untyped (no stdlib), so the
        // scrutinee type is unknown and the bare `None` arm cannot be proven a
        // variant. It lowers as a binding catch-all (`default`) while the
        // payload `Some(_)` arm stays a `case`. At runtime the `default` catches
        // exactly the tags no `case` lists, so a `None` value still routes here.
        let ts = emit(
            "module x\nfn f() -> number {\n  return match find() {\n    None => 0,\n    Some(_) => 1,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Some\": {"), "{ts}");
        assert!(ts.contains("default: {"), "{ts}");
        assert!(ts.contains("const None = __m0;"), "{ts}");
        // The binding catch-all is the only `default`; no synthetic throw.
        assert!(!ts.contains("non-exhaustive match"), "{ts}");
    }

    #[test]
    fn lone_binding_arm_binds_the_scrutinee() {
        // A match whose only arm is a binding has no tag to switch on: bind the
        // scrutinee to the name and run the body.
        let ts = emit(
            "module x\nfn f() -> number {\n  return match find() {\n    other => other,\n  }\n}\n",
        );
        assert!(ts.contains("const other = find();"), "{ts}");
        assert!(!ts.contains("switch"), "{ts}");
    }

    #[test]
    fn two_binding_arms_are_rejected_as_two_catch_alls() {
        // Without scrutinee type information two bare bindings are both
        // catch-alls, which would emit two `default:` clauses; reject instead.
        let err = emit_err(
            "module x\nfn f() -> number {\n  return match find() {\n    a => 0,\n    b => 1,\n  }\n}\n",
        );
        assert!(
            matches!(err, EmitError::Unsupported { construct, .. } if construct.contains("catch-all")),
            "{err:?}"
        );
    }

    #[test]
    fn wildcard_constructor_arg_binds_nothing() {
        // `Ok(_)` matches the variant and discards its payload: a `case` with
        // no binding, like a no-payload variant.
        let ts = emit(
            "module x\ntype R =\n  | Ok(number)\n  | Bad(string)\nfn f(r: R) -> string {\n  return match r {\n    Ok(_) => \"ok\",\n    Bad(msg) => msg,\n  }\n}\n",
        );
        assert!(ts.contains("case \"Ok\": {"), "{ts}");
        // No payload binding is emitted for the discarded `_`.
        assert!(!ts.contains("__m0.value;\n      return \"ok\""), "{ts}");
        assert!(ts.contains("const msg = __m0.value;"), "{ts}");
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

    #[test]
    fn generic_union_constructors_are_generic_only_over_used_params() {
        let ts = emit(
            "module x\ntype Either<A, B> =\n  | Left({ a: A })\n  | Right({ b: B })\n  | Neither\n",
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
            "module x\ntype Box<T> =\n  | Full({ value: T })\n  | Empty\nfn f(b: Box<string>) -> string {\n  return match b {\n    Full({ value }) => value,\n    Empty => \"\",\n  }\n}\n",
        );
        assert!(ts.contains("case \"Full\": {"), "{ts}");
        // `Empty` (a bare no-payload variant) resolves even though the
        // scrutinee type is `Box<string>` (a `Ty::App`).
        assert!(ts.contains("case \"Empty\": {"), "{ts}");
    }
}
