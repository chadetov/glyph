//! `TypeExpr → Ty` lowering.
//!
//! Given a `glyph_ast::TypeExpr` plus the resolver's `ResolvedModule` +
//! `Prelude`, produce a `Ty`. Used in two places:
//! - typing function signatures (params + return type)
//! - typing `type X = ...` declarations and `const NAME: T = ...` annotations
//!
//! A type declared in another module lowers to `Ty::Imported { module, name }`
//! under every legal import spelling. Nothing is expanded here: the declaration
//! is fetched on demand through `DeclTyResolver::imported_type_decl`, which is
//! why a self-referential sibling type needs no cycle guard.

use std::sync::Arc;

use glyph_ast::{Decl, GenericParam, Ident, LambdaParam, Param, TypeExpr};
use glyph_resolver::{Prelude, PreludeKind, ResolvedModule, ResolvedRef, SymbolKind};

use crate::ty::{
    DeclBounds, FnParam, ImportedTypeDecl, ModuleKey, ParamOwner, Primitive, RecordField, Ty, UnionVariant,
};

/// Holds the resolver-side context a `TypeExpr → Ty` recursion needs. Cheap
/// to construct (two references); avoids threading `(resolved, prelude)`
/// through every recursive call.
pub struct Lowerer<'a> {
    pub resolved: &'a ResolvedModule,
    pub prelude: &'a Prelude,
    /// Set only on a `for_export` lowerer: the module path of the file being
    /// lowered, used to render a *module-local* type as a `Ty::Imported`
    /// anchored on this module. That is what keeps this module's `SymbolId`s
    /// out of a consumer's `Ty`, where they would index unrelated symbols.
    export_module: Option<&'a str>,
}

impl<'a> Lowerer<'a> {
    pub fn new(resolved: &'a ResolvedModule, prelude: &'a Prelude) -> Self {
        Self {
            resolved,
            prelude,
            export_module: None,
        }
    }

    /// The **export view** of a module: lowering a declaration as another
    /// module will see it. Identical to `new` except that a
    /// module-local `type` name lowers to `Ty::Imported { module_path, name }`
    /// instead of a `Ty::Named` carrying this module's `SymbolId` — a foreign
    /// id would index an unrelated symbol in the consumer's table.
    ///
    /// `module_path` is the slash-joined registry path of the module being
    /// lowered (`"catalog"`, `"db/catalog"`), the same spelling
    /// `DeclTyResolver`'s keys use, so a `Ty::Imported` and a query key can
    /// never disagree.
    ///
    /// Returns an `ExportLowerer`, not a `Lowerer`: `lower_exported_type` is
    /// only sound on the export view, and that is expressed as a type rather
    /// than as an assertion.
    pub fn for_export(
        resolved: &'a ResolvedModule,
        prelude: &'a Prelude,
        module_path: &'a str,
    ) -> ExportLowerer<'a> {
        ExportLowerer(Self {
            resolved,
            prelude,
            export_module: Some(module_path),
        })
    }

    /// The declaration that owns the type parameter bound at `start`, the
    /// definition-site offset a `ResolvedRef::Local` carries: the `fn`,
    /// `component`, `type` or `interface` whose span holds it. A type name
    /// that resolves to a local is always one of those `<...>` bindings, since
    /// a value binding is never in scope in type position.
    ///
    /// Under the export view the owner is named by `(module, name)`, as every
    /// other declaration of this module is, so a consumer never holds this
    /// module's `SymbolRef`.
    pub fn param_owner(&self, start: u32) -> ParamOwner {
        let table = &self.resolved.symbols.table;
        for i in 0..table.len() {
            let id = glyph_resolver::SymbolId(i as u32);
            let Some(sym) = table.get(id) else {
                continue;
            };
            if !(sym.span.start <= start && start < sym.span.end) {
                continue;
            }
            let callable = match sym.kind {
                SymbolKind::Function { .. } | SymbolKind::Component { .. } => true,
                SymbolKind::Type { .. } => false,
                _ => continue,
            };
            return match (self.export_module, callable) {
                (Some(module), true) => ParamOwner::ImportedCallable {
                    module: module.into(),
                    name: sym.name.clone(),
                },
                (Some(module), false) => ParamOwner::ImportedTypeDecl {
                    module: module.into(),
                    name: sym.name.clone(),
                },
                (None, true) => ParamOwner::Callable(id.into()),
                (None, false) => ParamOwner::TypeDecl(id.into()),
            };
        }
        ParamOwner::Unresolved
    }

    /// `owner` as another module sees it: a declaration of this module is
    /// named by `(module, name)` instead of by its `SymbolRef`.
    fn export_owner(&self, owner: &ParamOwner) -> ParamOwner {
        let module = self.export_module.expect("export view");
        let name_of = |symbol: &crate::ty::SymbolRef| {
            self.resolved
                .symbols
                .table
                .get(glyph_resolver::SymbolId(symbol.0))
                .map(|sym| sym.name.clone())
        };
        match owner {
            ParamOwner::Callable(symbol) => match name_of(symbol) {
                Some(name) => ParamOwner::ImportedCallable {
                    module: module.into(),
                    name,
                },
                None => ParamOwner::Unresolved,
            },
            ParamOwner::TypeDecl(symbol) => match name_of(symbol) {
                Some(name) => ParamOwner::ImportedTypeDecl {
                    module: module.into(),
                    name,
                },
                None => ParamOwner::Unresolved,
            },
            other => other.clone(),
        }
    }

    pub fn lower(&self, te: &TypeExpr) -> Ty {
        match te {
            TypeExpr::Path { segments, span } => {
                if segments.len() != 1 {
                    // A two-segment path through a stdlib namespace import
                    // (`fs.FsError`) names a type the runtime ships. The checker
                    // models the shape of a few of them, and lowering those to
                    // the same synthetic `Ty::Named` the stdlib return-type
                    // table produces is what makes `e.kind` on a declared
                    // `fs.FsError` parameter resolve to the closed `ErrorKind`
                    // union instead of `Unknown`. Everything else stays
                    // `Unknown`, which is the pre-existing behaviour.
                    //
                    // A two-segment path through a *project* namespace import
                    // (`catalog.ColType`, or `c.ColType` through
                    // `import catalog as c`) is a `Ty::Imported`, the same type
                    // the named-import spelling below produces for the same
                    // declaration. A string-literal union declared in the
                    // sibling is no exception (G233): the literal set is read
                    // from the declaration on demand
                    // (`imported_string_literal_union_values`), so the type
                    // keeps the name its author gave it and D30's
                    // exhaustiveness still crosses the boundary.
                    return self
                        .stdlib_path_ty(segments, *span)
                        .or_else(|| self.qualified_imported_ty(segments, *span))
                        .unwrap_or(Ty::Unknown);
                }
                let head = &segments[0];
                match self.resolved.resolutions.get(*span) {
                    Some(ResolvedRef::Prelude(id)) => self.prelude_ty(id, head),
                    Some(ResolvedRef::Module(id)) => {
                        let sym = self.resolved.symbols.table.get(id).expect("symbol id valid");
                        match &sym.kind {
                            // Under the export view a module-local `type` name
                            // is rendered as the consumer will see it: by
                            // (module, name), never by this module's SymbolId.
                            // A `Variant` in type position is not a valid Glyph
                            // type and has no honest cross-module rendering, so
                            // it sanitizes to `Unknown` rather than widening.
                            SymbolKind::Type { .. } if self.export_module.is_some() => {
                                let module = self.export_module.expect("export view");
                                Ty::Imported {
                                    module: module.into(),
                                    name: sym.name.clone(),
                                }
                            }
                            SymbolKind::Variant { .. } if self.export_module.is_some() => {
                                Ty::Unknown
                            }
                            SymbolKind::Type { .. } | SymbolKind::Variant { .. } => Ty::Named {
                                symbol: id.into(),
                                path: segments.clone(),
                            },
                            // `import std/result { Result }` resolves `Result`
                            // to a module-level import symbol, but the name is
                            // a prelude container (Q3: the stdlib re-exports
                            // the prelude built-ins). Lower it to the same
                            // prelude `Ty::Named` the un-imported reference
                            // would produce, so `Result`/`Option` are
                            // recognizable regardless of how they were brought
                            // into scope.
                            //
                            // Only when the import comes from `std/`. An
                            // explicit import binding shadows the prelude name
                            // (G239): `import orders { Result }` for a project
                            // module's own `pub type Result` is the project's
                            // declaration, and reading it as the prelude
                            // container gave the match over it no variant list,
                            // so D9 exhaustiveness was switched off for every
                            // union a project named `Result`, `Option` or
                            // `Nullable` and imported. A local `type Result`
                            // already shadows (G213) and `import std/string`
                            // already binds the name it spells (G225); this is
                            // the third spelling of one rule.
                            //
                            // Anything else keeps its identity across the
                            // boundary as a `Ty::Imported`, keyed on the source
                            // module and the name that module declares, so the
                            // three legal import spellings agree.
                            SymbolKind::ImportNamed { original, path } => self
                                .stdlib_prelude_container(path, original)
                                .or_else(|| self.imported_stdlib_modeled_ty(path, original))
                                .unwrap_or_else(|| Ty::Imported {
                                    module: module_key(path),
                                    name: original.clone(),
                                }),
                            // A bare name bound to a namespace, an alias or a
                            // default import is not a type: the resolver has
                            // one symbol table, so `import std/string` binds
                            // `string` and the annotation `string` resolves
                            // to that binding. It is the prelude type the
                            // name spells, the rule the arm above applies to
                            // `import std/result { Result }`. Answering
                            // `Unknown` here switched the checker off for
                            // every `string` annotation in a module that used
                            // the namespace form (G225), and the emitter
                            // reads the binding the same way
                            // (`prelude_kind_of_name`), so the two agree.
                            SymbolKind::ImportNamespace { .. }
                            | SymbolKind::ImportAlias { .. }
                            | SymbolKind::ImportDefault { .. } => self
                                .prelude
                                .lookup(head.as_ref())
                                .map(|id| self.prelude_ty(id, head))
                                .unwrap_or(Ty::Unknown),
                            _ => Ty::Unknown,
                        }
                    }
                    Some(ResolvedRef::Local(start)) => Ty::Param {
                        name: head.clone(),
                        owner: self.param_owner(start),
                    },
                    None => Ty::Unknown,
                }
            }
            TypeExpr::Generic { base, args, .. } => Ty::App {
                base: Arc::new(self.lower(base)),
                args: args.iter().map(|a| self.lower(a)).collect(),
            },
            TypeExpr::Fn {
                params,
                return_ty,
                is_async,
                ..
            } => Ty::Fn {
                params: params
                    .iter()
                    .map(|p| FnParam {
                        name: p.name.clone(),
                        // Function-type params (`fn(x: T) -> U`) borrow in v1;
                        // `fn(owned x: T)` type syntax is forward-compatible.
                        owned: false,
                        ty: self.lower(&p.ty),
                optional: false,
                    })
                    .collect(),
                return_ty: Arc::new(
                    return_ty
                        .as_deref()
                        .map(|rt| self.lower(rt))
                        .unwrap_or(Ty::Prim(Primitive::Void)),
                ),
                is_async: *is_async,
                // A written function type has no `<...>` of its own, so it
                // takes no type arguments, as `(x: T) => U` takes none in TS.
                type_params: Some(Vec::new()),
            },
            TypeExpr::Record { fields, .. } => Ty::Record {
                fields: fields
                    .iter()
                    .map(|f| RecordField {
                        name: f.name.clone(),
                        ty: self.lower(&f.ty),
                        optional: f.optional,
                    })
                    .collect(),
            },
            TypeExpr::Union { variants, .. } => Ty::Union {
                variants: variants
                    .iter()
                    .map(|v| UnionVariant {
                        name: v.name.clone(),
                        payload: v.payload.as_ref().map(|p| self.lower(p)),
                    })
                    .collect(),
            },
            // The escape hatch is opaque to Glyph's own checker (like an
            // imported `.d.ts` type): `tsc` type-checks its uses against the
            // real emitted TypeScript, but Glyph neither reduces it nor gives it
            // a runtime descriptor.
            TypeExpr::Extern { .. } => Ty::Unknown,
            // A string-literal union carries its literal set so a `match` over it
            // can be exhaustive without an `else`. It behaves like `string` for
            // assignability; `tsc` enforces the narrowed literal type on the
            // emitted TS, and a record field of this type gets a runtime
            // membership check in its descriptor.
            TypeExpr::StringLiteralUnion { values, .. } => {
                Ty::StringLiteralUnion(values.clone())
            }
            // `typeof value` is opaque to Glyph's checker (`tsc` reduces it, e.g.
            // `z.infer<typeof s>`), like an imported `.d.ts` type: no descriptor.
            TypeExpr::TypeOf { .. } => Ty::Unknown,
        }
    }

    /// Lower a callable signature (`fn` or `component`) to a `Ty::Fn`. Used
    /// from `lower_decl_signature`; downstream crates should call that rather
    /// than this helper directly. `generics` is the declaration's `<...>`
    /// list. A lambda's signature is `lower_lambda_signature`.
    pub(crate) fn lower_callable_signature(
        &self,
        generics: &[GenericParam],
        params: &[Param],
        return_ty: Option<&TypeExpr>,
        is_async: bool,
    ) -> Ty {
        let params = params
            .iter()
            .map(|p| FnParam {
                name: Some(p.name.clone()),
                owned: p.owned,
                ty: self.lower(&p.ty),
                optional: false,
            })
            .collect();
        let return_ty = return_ty
            .map(|rt| self.lower(rt))
            .unwrap_or(Ty::Prim(Primitive::Void));
        Ty::Fn {
            params,
            return_ty: Arc::new(return_ty),
            is_async,
            type_params: Some(generics.iter().map(|g| g.name.clone()).collect()),
        }
    }

    /// Lower a lambda's signature to a `Ty::Fn`. A parameter written without
    /// an annotation is `Ty::Unknown` ("not typed here"), never the `unknown`
    /// a program can write, which lowers to `Ty::UnknownTop` like any other
    /// written type. A lambda declares no generics and no `owned` parameter.
    pub(crate) fn lower_lambda_signature(
        &self,
        params: &[LambdaParam],
        return_ty: Option<&TypeExpr>,
        is_async: bool,
    ) -> Ty {
        let params = params
            .iter()
            .map(|p| FnParam {
                name: Some(p.name.clone()),
                owned: false,
                ty: p.ty.as_ref().map_or(Ty::Unknown, |te| self.lower(te)),
                optional: false,
            })
            .collect();
        let return_ty = return_ty
            .map(|rt| self.lower(rt))
            .unwrap_or(Ty::Prim(Primitive::Void));
        Ty::Fn {
            params,
            return_ty: Arc::new(return_ty),
            is_async,
            type_params: Some(Vec::new()),
        }
    }

    /// Lower the signature of a top-level declaration. `Fn` and `Component`
    /// produce a `Ty::Fn`; `Import`/`Type`/`Const` are `Ty::Unknown` here
    /// (their type information is fed into expression-typing via different
    /// paths during week 3's bidirectional checker). No wildcard arm — when
    /// a new `Decl` variant lands the compiler must force a decision here.
    pub fn lower_decl_signature(&self, decl: &Decl) -> Ty {
        match decl {
            Decl::Fn(f) => self.lower_callable_signature(
                &f.generics,
                &f.params,
                f.return_ty.as_ref(),
                f.is_async,
            ),
            // D19 gives a component an implied JSX-returning body, so a
            // component written without `-> Component` returns the prelude
            // `Component` all the same. Reading the absence as `void`, as a
            // plain `fn` does, told hover and `glyph_symbol` that a component
            // returns nothing and refused its element as a `void`.
            Decl::Component(c) => {
                let sig = self.lower_callable_signature(
                    &c.generics,
                    &c.params,
                    c.return_ty.as_ref(),
                    false,
                );
                match (sig, c.return_ty.is_none()) {
                    (
                        Ty::Fn {
                            params,
                            is_async,
                            type_params,
                            ..
                        },
                        true,
                    ) => Ty::Fn {
                        params,
                        return_ty: Arc::new(self.prelude_component()),
                        is_async,
                        type_params,
                    },
                    (sig, _) => sig,
                }
            }
            // G39. An annotated `const` is its annotation, lowered through the
            // same path a `let`'s annotation takes. Without it a module-level
            // `const` was `Ty::Unknown`, so the checks that read a receiver's
            // type went silent at one: `ORIGIN.rowz` drew no E0210 and `g(N)`
            // no E0211, while the identical program written with an annotated
            // `let` inside the function reported both. A guarantee is not
            // allowed to depend on which binding form the author reached for.
            //
            // Exhaustiveness was never in that set. `match M` over a `const` of
            // a union already reported E0200, because the coverage check
            // resolves the union from the arm heads rather than from the
            // scrutinee's type. That is worth stating: the silence here was
            // narrower than "everything downstream", and a comment that claimed
            // more than was measured would be the wrong kind of confidence.
            //
            // An unannotated `const` has its initializer's type (G433, decision
            // record 158), under the widening `tsc` applies to the `const` the
            // emitter writes: a string literal standing as the whole value
            // keeps its literal type, and a literal inside an object or an
            // array widens. The same principle as above decides it. Leaving it
            // `Unknown` made every check that reads a receiver's type go
            // silent at a `const` the author did not annotate, so `ITEMS.push(3)`
            // drew no E0240 where the same array in an annotated `let` did.
            // Reading the type `tsc` infers is not a guess: an unannotated
            // `let` has been checked against its initializer's type all along,
            // and `tsc` reports against that same type.
            //
            // The lowerer has no expression walk, so the type is not produced
            // here: `assign::decl_signature_ty` answers an unannotated `const`
            // through `assign::unannotated_const_ty`, and this arm only ever
            // sees the annotation. An initializer the walk cannot decide stays
            // `Unknown`.
            Decl::Const(c) => c
                .ty
                .as_ref()
                .map(|t| self.lower(t))
                .unwrap_or(Ty::Unknown),
            Decl::Import(_) | Decl::Type(_) | Decl::Interface(_) => Ty::Unknown,
        }
    }

    /// The `Ty` for a stdlib type brought into scope **by name**
    /// (`import std/http { HttpError }`), matching what the namespaced spelling
    /// (`import std/http` + `http.HttpError`) already produces via
    /// `stdlib_path_ty`.
    ///
    /// Without it the named spelling lowered to a `Ty::Imported`, which the
    /// field tables in `assign` cannot key on: they read a two-segment path, and
    /// a named import has one segment. The consequence was not a missing
    /// convenience but two checks silently switched off. `e.kind` on an
    /// `HttpError` typed `Unknown`, so a `match` covering all three of its
    /// literals was E0218 "non-exhaustive, add an `else`" — advice to disable
    /// the check — while the same body under the other spelling compiled. And
    /// `e.nope` produced no Glyph diagnostic at all, falling through to a
    /// `tsc` TS2339 where the other spelling gives E0210.
    ///
    /// This is the class the module already states as a rule three doc comments
    /// up: a guarantee must not depend on which legal spelling brought the type
    /// into scope.
    fn imported_stdlib_modeled_ty(
        &self,
        path: &glyph_ast::ModulePath,
        original: &Ident,
    ) -> Option<Ty> {
        let key: Vec<&str> = path.segments.iter().map(|s| s.as_ref()).collect();
        let ["std", module] = key.as_slice() else {
            return None;
        };
        crate::assign::stdlib_modeled_type(module, original.as_ref())
    }

    /// The `Ty` for `ns.Name` when `ns` is a namespace import of a stdlib
    /// module (`import std/fs`, `import std/fs as f`) and `Name` is one of the
    /// stdlib types the checker models the shape of. The module is read from the
    /// import's own path, so an alias resolves the same as the plain form.
    /// `None` for any other two-segment path, which keeps lowering to `Unknown`.
    ///
    /// A prelude container reached through the same spelling (`option.Option`,
    /// `result.Result`) lowers to the prelude `Ty::Named`, the identical value
    /// `import std/option { Option }` produces. Without it `option.Option<T>` was
    /// `Unknown`, which sent every `match` over it down the imported-union path
    /// that only understands `ImportNamed` variants, so a missing `None` was
    /// never reported (verifiability: D9 sealed unions must hold regardless of
    /// which legal import spelling brought the type into scope).
    fn stdlib_path_ty(&self, segments: &[Ident], span: glyph_ast::Span) -> Option<Ty> {
        let path = self.namespace_import_path(span)?;
        let key: Vec<&str> = path.segments.iter().map(|s| s.as_ref()).collect();
        let ["std", module] = key.as_slice() else {
            return None;
        };
        let name = segments.get(1)?;
        crate::assign::stdlib_modeled_type(module, name.as_ref())
            .or_else(|| self.imported_prelude_container(name))
    }

    /// The module path a two-segment type path's *head* was imported from, when
    /// that head is a namespace import (`import catalog`) or an aliased one
    /// (`import catalog as c`). `None` for any other head. Shared by
    /// `stdlib_path_ty` and `qualified_string_literal_union` so the two agree on
    /// what "reached through a namespace import" means.
    fn namespace_import_path(
        &self,
        span: glyph_ast::Span,
    ) -> Option<&'a glyph_ast::ModulePath> {
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        match &sym.kind {
            SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. } => {
                Some(path)
            }
            _ => None,
        }
    }

    /// The `Ty` for `ns.Name` when `ns` is a namespace import (`import catalog`)
    /// or an aliased one (`import catalog as c`). Produces the same
    /// `Ty::Imported` the named spelling produces for the same declaration:
    /// which of the three legal spellings brought a type into scope must not
    /// change what the checker knows about it.
    ///
    /// A name whose module is not a project sibling (`http.Response`) is fine
    /// here: `imported_type_decl` answers `None` for it, so member access falls
    /// through to the stdlib tables exactly as before. (`fs.FsError` never
    /// reaches this method at all — `stdlib_path_ty` models its shape and
    /// answers first.)
    fn qualified_imported_ty(&self, segments: &[Ident], span: glyph_ast::Span) -> Option<Ty> {
        if segments.len() != 2 {
            return None;
        }
        let path = self.namespace_import_path(span)?;
        Some(Ty::Imported {
            module: module_key(path),
            name: segments.get(1)?.clone(),
        })
    }

    /// If `name` is a prelude container type (`Result`, `Option`, `Array`,
    /// `Record`, `Schema`, `Component`), return its prelude `Ty::Named`.
    /// Used to unify an imported reference (`import std/result { Result }`)
    /// with the prelude built-in of the same name. Returns None for any
    /// other imported name (a genuinely user-defined cross-module type),
    /// which stays `Ty::Unknown` until cross-module type resolution lands.
    /// The prelude container a named import brings into scope, and `None` when
    /// the import is not from a `std/` module.
    ///
    /// The prelude is a curated re-export of the stdlib (Q3, G231), so
    /// `import std/result { Result }` and a bare `Result` are one declaration.
    /// A project module's `Result` is a different declaration with the same
    /// spelling, and the import binding is what says which one this module
    /// means. Reading the name first and the path second gave the prelude the
    /// last word over an explicit import, which is the one place a name can be
    /// pinned down (G239).
    fn stdlib_prelude_container(
        &self,
        path: &glyph_ast::ModulePath,
        name: &Ident,
    ) -> Option<Ty> {
        let first = path.segments.first()?;
        if first.as_ref() != "std" {
            return None;
        }
        self.imported_prelude_container(name)
    }

    fn imported_prelude_container(&self, name: &Ident) -> Option<Ty> {
        let id = self.prelude.lookup(name.as_ref())?;
        let sym = self.prelude.table.get(id)?;
        let SymbolKind::Prelude { kind } = sym.kind else { return None };
        matches!(
            kind,
            PreludeKind::Result
                | PreludeKind::Option
                | PreludeKind::Nullable
                | PreludeKind::Array
                | PreludeKind::Record
                | PreludeKind::Schema
                | PreludeKind::Component
        )
        .then(|| Ty::Named {
            symbol: id.into(),
            path: vec![name.clone()],
        })
    }

    /// The prelude `Component`, the type an unannotated `component` returns
    /// and the one `-> Component` names when the module has not bound the
    /// name to something else.
    pub fn prelude_component(&self) -> Ty {
        let name: Ident = Arc::from("Component");
        match self.prelude.lookup(name.as_ref()) {
            Some(id) => self.prelude_ty(id, &name),
            None => Ty::Unknown,
        }
    }

    fn prelude_ty(&self, id: glyph_resolver::SymbolId, name: &Ident) -> Ty {
        let sym = self.prelude.table.get(id).expect("prelude id valid");
        let SymbolKind::Prelude { kind } = sym.kind else {
            return Ty::Unknown;
        };
        match kind {
            PreludeKind::String => Ty::Prim(Primitive::String),
            PreludeKind::Number => Ty::Prim(Primitive::Number),
            // `int` is a `number` to Glyph's checker; its integer-ness is a
            // runtime descriptor check, not a static type (TS has no `int`).
            PreludeKind::Int => Ty::Prim(Primitive::Number),
            // `bigint` is a type of its own (D38), never a `number`: `tsc`
            // refuses one where the other is declared, in both directions.
            PreludeKind::BigInt => Ty::Prim(Primitive::BigInt),
            PreludeKind::Bool => Ty::Prim(Primitive::Bool),
            PreludeKind::Void => Ty::Prim(Primitive::Void),
            PreludeKind::UnknownTop => Ty::UnknownTop,
            PreludeKind::Never => Ty::Never,
            // `Nullable` (D45) is a container of its own: the same `Ty::Named`
            // shape as `Option`, under its own prelude symbol, so the two are
            // never assignable to each other and the conversion has to be
            // written out.
            PreludeKind::Result
            | PreludeKind::Option
            | PreludeKind::Nullable
            | PreludeKind::Array
            | PreludeKind::Record
            | PreludeKind::Schema
            | PreludeKind::Component
            | PreludeKind::Issue => Ty::Named {
                symbol: id.into(),
                path: vec![name.clone()],
            },
            PreludeKind::Ok
            | PreludeKind::Err
            | PreludeKind::Some
            | PreludeKind::None
            | PreludeKind::Par
            | PreludeKind::Print
            | PreludeKind::Assert
            // `infer_output<S>` (D28) is a type-level operator the checker does
            // not reduce; the emitter lowers it to a TS mapped type and `tsc`
            // reduces and enforces it. Left `Unknown` here so it neither
            // resolves as a nominal type nor trips a diagnostic.
            | PreludeKind::InferOutput => Ty::Unknown,
        }
    }
}

/// A `Lowerer` restricted to the export view of one module, obtained only from
/// `Lowerer::for_export`. It is the sole holder of `lower_exported_type`,
/// because that method is sound only when a module-local `type` name lowers to
/// `Ty::Imported`: off the export view the body would carry the declaring
/// module's `SymbolId`s into a consumer's table, where they index unrelated
/// symbols. That failure is silent, cross-module, and was previously guarded by
/// a `debug_assert!` — a no-op in release. Now it does not compile.
pub struct ExportLowerer<'a>(Lowerer<'a>);

impl ExportLowerer<'_> {
    /// Lower a `type` declaration as another module sees it: its name, its
    /// generic parameter names, and its body lowered against the *declaring*
    /// module's resolutions.
    pub fn lower_exported_type(&self, td: &glyph_ast::TypeDecl) -> ImportedTypeDecl {
        ImportedTypeDecl {
            name: td.name.clone(),
            generics: td.generics.iter().map(|g| g.name.clone()).collect(),
            body: self.0.lower(&td.body),
            open: glyph_ast::is_open_record(&td.annotations),
            refined: td.refinement.is_some(),
        }
    }

    /// The type parameters of `decl` and the bound each writes, as another
    /// module sees them: [`written_bounds`] over the declaration's generics, lowered
    /// on the export view so a bound naming a sibling type reaches the
    /// consumer as `Ty::Imported`. `None` for a declaration with no type
    /// parameter list (a `const`, an `import`).
    pub fn lower_exported_bounds(
        &self,
        decl: &glyph_ast::Decl,
    ) -> Option<DeclBounds> {
        let generics = match decl {
            Decl::Fn(f) => &f.generics,
            Decl::Component(c) => &c.generics,
            Decl::Type(t) => &t.generics,
            Decl::Interface(i) => &i.generics,
            Decl::Const(_) | Decl::Import(_) => return None,
        };
        Some(DeclBounds {
            generics: generics.iter().map(|g| g.name.clone()).collect(),
            bounds: written_bounds(generics, &|t| self.0.lower(t)),
        })
    }

    /// Lower a `fn`/`component` declaration's signature as another module
    /// sees it: the callable counterpart of `lower_exported_type`. A `pub fn`
    /// whose return type (or a param type) names a sibling `type` needs the
    /// same export view a `type` body does, so that sibling renders as
    /// `Ty::Imported { module, name }` rather than a `Ty::Named` carrying this
    /// module's own `SymbolId` — an id a consumer's table cannot resolve.
    /// Delegates to `lower_decl_signature`, which is already shape-correct
    /// for every `Decl` variant; the only thing the export view changes is
    /// what a local-type reference *inside* that signature lowers to.
    pub fn lower_exported_fn_signature(&self, decl: &glyph_ast::Decl) -> Ty {
        self.0.lower_decl_signature(decl)
    }

    /// Lower a `const` declaration's annotation as another module sees it:
    /// the value counterpart of `lower_exported_fn_signature`. `pub const
    /// ORIGIN: Sheet = ...` reaches a consumer as `Ty::Imported` for `Sheet`,
    /// so a field read on the imported const is checked against the record it
    /// names. An unannotated const is `Ty::Unknown` here; its type is the one
    /// its initializer has in its own module (G433), which the caller reads
    /// through `assign::decl_signature_ty` and renders with
    /// [`ExportLowerer::export_inferred`].
    pub fn lower_exported_const(&self, c: &glyph_ast::ConstDecl) -> Ty {
        c.ty
            .as_ref()
            .map(|t| self.0.lower(t))
            .unwrap_or(Ty::Unknown)
    }

    /// A type this module's checker *inferred* (an unannotated `const`'s,
    /// G433), rendered as another module sees it.
    ///
    /// The type was built against this module's symbol table, so it can hold
    /// a `Ty::Named` carrying one of this module's `SymbolId`s, which would
    /// index an unrelated symbol in a consumer's table. Each such name is
    /// rewritten the way `lower` renders it on the export view: a module-local
    /// `type` becomes `Ty::Imported { module, name }`, and anything else that
    /// cannot cross (a variant used as a type, an id this table does not hold)
    /// becomes `Ty::Unknown`, which no check reads. A prelude container and a
    /// modeled stdlib type are the same everywhere and pass unchanged, as
    /// does a `Ty::Imported`, which is already keyed by module. A generic
    /// parameter keeps its name, and an owner of this module is named by
    /// `(module, name)`, as a lowered signature on the export view names it.
    ///
    /// The result is the type a consumer reads under either import spelling,
    /// so `import lib { MODE }` and `lib.MODE` are judged against one answer.
    pub fn export_inferred(&self, ty: &Ty) -> Ty {
        let lowerer = &self.0;
        let module = lowerer.export_module.expect("export view");
        match ty {
            Ty::Named { symbol, path } => {
                let id = glyph_resolver::SymbolId(symbol.0);
                let name = path.last();
                if symbol.0 == u32::MAX
                    || name.is_some_and(|n| lowerer.prelude.lookup(n.as_ref()) == Some(id))
                {
                    return ty.clone();
                }
                match lowerer.resolved.symbols.table.get(id) {
                    Some(sym)
                        if matches!(sym.kind, SymbolKind::Type { .. })
                            && name == Some(&sym.name) =>
                    {
                        Ty::Imported {
                            module: module.into(),
                            name: sym.name.clone(),
                        }
                    }
                    _ => Ty::Unknown,
                }
            }
            Ty::Param { name, owner } => Ty::Param {
                name: name.clone(),
                owner: lowerer.export_owner(owner),
            },
            Ty::App { base, args } => Ty::App {
                base: Arc::new(self.export_inferred(base)),
                args: args.iter().map(|a| self.export_inferred(a)).collect(),
            },
            Ty::Record { fields } => Ty::Record {
                fields: fields
                    .iter()
                    .map(|f| RecordField {
                        name: f.name.clone(),
                        ty: self.export_inferred(&f.ty),
                        optional: f.optional,
                    })
                    .collect(),
            },
            Ty::Fn {
                params,
                return_ty,
                is_async,
                type_params,
            } => Ty::Fn {
                params: params
                    .iter()
                    .map(|p| FnParam {
                        name: p.name.clone(),
                        owned: p.owned,
                        ty: self.export_inferred(&p.ty),
                        optional: p.optional,
                    })
                    .collect(),
                return_ty: Arc::new(self.export_inferred(return_ty)),
                is_async: *is_async,
                type_params: type_params.clone(),
            },
            Ty::Union { variants } => Ty::Union {
                variants: variants
                    .iter()
                    .map(|v| UnionVariant {
                        name: v.name.clone(),
                        payload: v.payload.as_ref().map(|p| self.export_inferred(p)),
                    })
                    .collect(),
            },
            Ty::Pending(inner) => Ty::Pending(Arc::new(self.export_inferred(inner))),
            Ty::Unknown
            | Ty::Prim(_)
            | Ty::UnknownTop
            | Ty::Never
            | Ty::StringLiteralUnion(_)
            | Ty::Imported { .. } => ty.clone(),
        }
    }
}

/// The registry key for a module path: its segments joined with `/`
/// (`["db", "catalog"]` → `"db/catalog"`). The single spelling of the key every
/// cross-module query is looked up by, so a `Ty::Imported`'s `module` and a
/// `DeclTyResolver` argument can never disagree.
pub(crate) fn module_key(path: &glyph_ast::ModulePath) -> ModuleKey {
    path.segments
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("/")
        .into()
}

/// Convenience free function over `Lowerer::lower`. Useful at call sites that
/// only lower one `TypeExpr`; recursive callers should construct a `Lowerer`
/// once and reuse it.
pub fn lower_type_expr(te: &TypeExpr, resolved: &ResolvedModule, prelude: &Prelude) -> Ty {
    Lowerer::new(resolved, prelude).lower(te)
}

/// The names in `generics` that write a bound (`<T: Named>`), each with the
/// bound `lower` gives it, or `None` where the checker does not read it (an
/// `extern_ts` or `typeof` bound, which lowers to nothing decidable, or a
/// cycle of parameters bounding one another, which has no bound to read).
/// A parameter written with no bound is absent.
///
/// One reading for both sides of a module edge: the body of a generic
/// declaration reads its own parameters through it (G263), and a caller or
/// an application reads the declaration's bounds through it (G264), local
/// or imported.
pub(crate) fn written_bounds(
    generics: &[GenericParam],
    lower: &dyn Fn(&TypeExpr) -> Ty,
) -> std::collections::HashMap<Ident, Option<Ty>> {
    let lowered: std::collections::HashMap<Ident, Ty> = generics
        .iter()
        .filter_map(|g| Some((g.name.clone(), lower(g.bounds.first()?))))
        .collect();
    let names: Vec<&Ident> = generics.iter().map(|g| &g.name).collect();
    lowered
        .iter()
        .map(|(name, bound)| {
            // Follow a chain of bounds that are each a bare parameter of this
            // list (`<T: U, U: V>`); one that comes back on itself has no
            // bound to read.
            let mut seen = vec![name];
            let mut cur = bound;
            let cyclic = loop {
                let Ty::Param { name: next, .. } = cur else {
                    break false;
                };
                if !names.contains(&next) {
                    break false;
                }
                if seen.contains(&next) {
                    break true;
                }
                seen.push(next);
                match lowered.get(next) {
                    Some(b) => cur = b,
                    None => break false,
                }
            };
            let readable = !cyclic && !matches!(bound, Ty::Unknown);
            (name.clone(), readable.then(|| bound.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};

    /// Parse `src`, resolve, then return the field-type lowering for the
    /// `decl_idx`-th type decl's `field_idx`-th record-field. Panics if the
    /// shape doesn't match — tests are responsible for matching the source.
    fn lower_field(src: &str, decl_idx: usize, field_idx: usize) -> Ty {
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let t = match &m.items[decl_idx] {
            glyph_ast::Decl::Type(t) => t,
            _ => panic!("decl {decl_idx} is not a Type"),
        };
        let fields = match &t.body {
            TypeExpr::Record { fields, .. } => fields,
            _ => panic!("decl {decl_idx} body is not a Record"),
        };
        Lowerer::new(&resolved, &prelude).lower(&fields[field_idx].ty)
    }

    #[test]
    fn primitive_string_lowers() {
        assert!(matches!(
            lower_field("module x\ntype T = { f: string }\n", 0, 0),
            Ty::Prim(Primitive::String)
        ));
    }

    #[test]
    fn primitive_number_lowers() {
        assert!(matches!(
            lower_field("module x\ntype T = { f: number }\n", 0, 0),
            Ty::Prim(Primitive::Number)
        ));
    }

    #[test]
    fn primitive_bigint_lowers_to_its_own_type() {
        assert!(matches!(
            lower_field("module x\ntype T = { f: bigint }\n", 0, 0),
            Ty::Prim(Primitive::BigInt)
        ));
    }

    #[test]
    fn unknown_top_lowers() {
        assert!(matches!(
            lower_field("module x\ntype T = { f: unknown }\n", 0, 0),
            Ty::UnknownTop
        ));
    }

    #[test]
    fn array_of_string_lowers_to_app() {
        match lower_field("module x\ntype T = { f: Array<string> }\n", 0, 0) {
            Ty::App { base, args } => {
                assert!(matches!(&*base, Ty::Named { .. }));
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], Ty::Prim(Primitive::String)));
            }
            other => panic!("expected App, got {other:?}"),
        }
    }

    #[test]
    fn result_of_user_feed_error_lowers() {
        let src = r#"module x
type User = { id: string }
type FeedError = | NotFound
type T = { f: Result<User, FeedError> }
"#;
        match lower_field(src, 2, 0) {
            Ty::App { args, .. } => {
                assert_eq!(args.len(), 2);
                assert!(matches!(args[0], Ty::Named { .. }));
                assert!(matches!(args[1], Ty::Named { .. }));
            }
            other => panic!("expected App<Result, [User, FeedError]>, got {other:?}"),
        }
    }

    #[test]
    fn fn_type_lowers() {
        match lower_field("module x\ntype T = { f: fn(a: string) -> number }\n", 0, 0) {
            Ty::Fn {
                params, return_ty, ..
            } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0].ty, Ty::Prim(Primitive::String)));
                assert!(matches!(&*return_ty, Ty::Prim(Primitive::Number)));
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn lower_decl_signature_for_fn() {
        let src = "module x\nfn add(a: number, b: number) -> number { return a + b }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty());
        let ty = Lowerer::new(&resolved, &prelude).lower_decl_signature(&m.items[0]);
        match ty {
            Ty::Fn {
                params, return_ty, ..
            } => {
                assert_eq!(params.len(), 2);
                assert!(matches!(params[0].ty, Ty::Prim(Primitive::Number)));
                assert!(matches!(params[1].ty, Ty::Prim(Primitive::Number)));
                assert!(matches!(&*return_ty, Ty::Prim(Primitive::Number)));
            }
            other => panic!("expected Ty::Fn, got {other:?}"),
        }
    }

    #[test]
    fn lower_decl_signature_for_type_is_unknown() {
        let src = "module x\ntype User = { name: string }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _) = resolve_module(&m, syms, &prelude);
        let ty = Lowerer::new(&resolved, &prelude).lower_decl_signature(&m.items[0]);
        assert!(matches!(ty, Ty::Unknown));
    }

    #[test]
    fn generic_param_lowers_to_param() {
        // `fn id<T>(x: T) -> T { return x }` — `T` in the param type position
        // resolves to a Local in the resolver, which lowers to `Ty::Param`.
        let m = glyph_parser::parse("module x\nfn id<T>(x: T) -> T { return x }\n").unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty());
        let f = match &m.items[0] {
            glyph_ast::Decl::Fn(f) => f,
            _ => panic!(),
        };
        let ty = Lowerer::new(&resolved, &prelude).lower(&f.params[0].ty);
        match ty {
            Ty::Param { name, owner } => {
                assert_eq!(name.as_ref(), "T");
                // The owner is `id`, the declaration whose `<T>` binds it
                // (G314, step 1).
                let id = resolved.symbols.lookup("id").expect("fn id");
                assert_eq!(owner, ParamOwner::Callable(id.into()));
            }
            other => panic!("expected Param, got {other:?}"),
        }
    }

    #[test]
    fn imported_type_lowers_to_an_imported_ty_without_import_context() {
        // Lowering emits `Ty::Imported` from the import's own path and the
        // original name, with no cross-module query involved. That is what makes
        // a self-referential or mutually-referential sibling type terminate: no
        // declaration is expanded here, so there is nothing to recurse into.
        let src = "module x\nimport catalog { Kind }\nfn label(k: Kind) -> string { return \"a\" }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _) = resolve_module(&m, syms, &prelude);
        let f = match &m.items[1] {
            glyph_ast::Decl::Fn(f) => f,
            other => panic!("expected Fn, got {other:?}"),
        };
        let ty = Lowerer::new(&resolved, &prelude).lower(&f.params[0].ty);
        match &ty {
            Ty::Imported { module, name } => {
                assert_eq!(module.as_str(), "catalog");
                assert_eq!(name.as_ref(), "Kind");
            }
            other => panic!("expected Imported, got {other:?}"),
        }
    }

    #[test]
    fn the_namespace_spelling_lowers_to_the_same_ty_as_the_named_one() {
        // The three legal spellings are the named import, `import catalog` +
        // `catalog.Sheet`, and `import catalog as c` + `c.Sheet`. All three must
        // produce the same `Ty` for the same declaration, or a guarantee starts
        // depending on how a type was brought into scope. (Glyph has no
        // per-name import alias, so `original` is always the declared name.)
        fn param_ty(src: &str) -> Ty {
            let m = glyph_parser::parse(src).unwrap();
            let syms = collect_module_symbols(&m).unwrap();
            let prelude = build_prelude();
            let (resolved, _) = resolve_module(&m, syms, &prelude);
            let f = match &m.items[1] {
                glyph_ast::Decl::Fn(f) => f,
                other => panic!("expected Fn, got {other:?}"),
            };
            Lowerer::new(&resolved, &prelude).lower(&f.params[0].ty)
        }
        let expected = Ty::Imported {
            module: "catalog".into(),
            name: "Sheet".into(),
        };
        assert_eq!(
            param_ty("module x\nimport catalog { Sheet }\nfn f(s: Sheet) -> string { return \"a\" }\n"),
            expected
        );
        assert_eq!(
            param_ty("module x\nimport catalog\nfn f(s: catalog.Sheet) -> string { return \"a\" }\n"),
            expected
        );
        assert_eq!(
            param_ty("module x\nimport catalog as c\nfn f(s: c.Sheet) -> string { return \"a\" }\n"),
            expected
        );
    }

    #[test]
    fn a_stdlib_type_the_tables_do_not_model_takes_the_imported_fall_through() {
        // `stdlib_path_ty` answers first for the handful of stdlib types whose
        // shape the checker models (`fs.FsError`), so those never reach
        // `qualified_imported_ty`. Everything else does, and gets an identity
        // nothing can resolve — which is what leaves member access on it exactly
        // as permissive as before. Pinned because the negative test in
        // `glyph-cli` used to pick a modeled type and so never entered here.
        let src = "module x\nimport std/http\nfn f(r: http.Response) -> number { return 1 }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _) = resolve_module(&m, syms, &prelude);
        let f = match &m.items[1] {
            glyph_ast::Decl::Fn(f) => f,
            other => panic!("expected Fn, got {other:?}"),
        };
        assert_eq!(
            Lowerer::new(&resolved, &prelude).lower(&f.params[0].ty),
            Ty::Imported {
                module: "std/http".into(),
                name: "Response".into(),
            }
        );
    }

    #[test]
    fn a_db_less_caller_never_checks_a_field_on_an_imported_type() {
        // The identity is available to every caller; the *declaration* is not,
        // because resolving it needs the cross-module query only a project-aware
        // resolver implements. So a db-less walk stays exactly as permissive as
        // it was when an imported type lowered to `Ty::Unknown` — a typo'd field
        // draws nothing rather than a false `UnknownField`.
        let src = "module x\nimport catalog { Sheet }\nfn f(s: Sheet) -> string { return s.rowz }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _) = resolve_module(&m, syms, &prelude);
        let (_tm, errs) = crate::assign::assign_types(&m, &resolved, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn the_export_view_never_carries_a_local_symbol_id() {
        // A module-local record named inside another declaration's body renders
        // as `Ty::Imported` under the export view. A `Ty::Named` here would ship
        // this module's `SymbolId` into a consumer's table, where it indexes an
        // unrelated symbol.
        let src = "module catalog\ntype Sheet = { rows: Array<string> }\ntype Book = { sheet: Sheet }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let td = match &m.items[1] {
            glyph_ast::Decl::Type(td) => td,
            other => panic!("expected Type, got {other:?}"),
        };
        let decl = Lowerer::for_export(&resolved, &prelude, "catalog")
            .lower_exported_type(td);
        assert_eq!(decl.name.as_ref(), "Book");
        let Ty::Record { fields } = &decl.body else {
            panic!("expected a record body, got {:?}", decl.body)
        };
        assert_eq!(
            fields[0].ty,
            Ty::Imported {
                module: "catalog".into(),
                name: "Sheet".into(),
            },
            "got {:?}",
            fields[0].ty
        );
    }

    #[test]
    fn the_export_view_keeps_generic_parameter_names() {
        let src = "module catalog\ntype Box<T> = { value: T }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let td = match &m.items[0] {
            glyph_ast::Decl::Type(td) => td,
            other => panic!("expected Type, got {other:?}"),
        };
        let decl = Lowerer::for_export(&resolved, &prelude, "catalog")
            .lower_exported_type(td);
        assert_eq!(
            decl.generics.iter().map(|g| g.as_ref()).collect::<Vec<_>>(),
            vec!["T"]
        );
    }

    /// The lowered type of the first parameter of the first `fn` in `src`.
    fn first_fn_param_ty(src: &str) -> Ty {
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, errs) = resolve_module(&m, syms, &prelude);
        assert!(errs.is_empty(), "errs: {errs:?}");
        let f = m
            .items
            .iter()
            .find_map(|d| match d {
                glyph_ast::Decl::Fn(f) => Some(f),
                _ => None,
            })
            .expect("a fn");
        Lowerer::new(&resolved, &prelude).lower(&f.params[0].ty)
    }

    // G225. `import std/string` binds `string`, and a `string` annotation in
    // that module resolves to the binding rather than to the prelude. It is
    // the prelude type the name spells, under the namespace form and the
    // aliased form alike, the way `import std/result { Result }` already was.
    #[test]
    fn a_prelude_name_shadowed_by_a_namespace_import_lowers_to_the_prelude_type() {
        let shadowed = first_fn_param_ty(
            "module x\nimport std/string\nfn f(s: string) -> string {\n  return string.trim(s)\n}\n",
        );
        assert_eq!(shadowed, Ty::Prim(Primitive::String));
        let aliased = first_fn_param_ty(
            "module x\nimport std/number as number\nfn f(n: number) -> string {\n  return number.to_string(n)\n}\n",
        );
        assert_eq!(aliased, Ty::Prim(Primitive::Number));
    }

    #[test]
    fn a_prelude_container_shadowed_by_a_namespace_import_is_the_prelude_container() {
        // `import std/result` binds `result`, not `Result`, so this spelling
        // never shadowed; an alias can. Either way the annotation is the
        // prelude's own `Result`, the value the un-imported spelling lowers to.
        let plain = first_fn_param_ty(
            "module x\nfn f(r: Result<number, string>) -> number {\n  return 1\n}\n",
        );
        let aliased = first_fn_param_ty(
            "module x\nimport std/result as Result\nfn f(r: Result<number, string>) -> number {\n  return 1\n}\n",
        );
        assert_eq!(aliased, plain, "the alias must not change what `Result` is");
        assert!(matches!(aliased, Ty::App { .. }), "got {aliased:?}");
    }

    #[test]
    fn a_namespace_binding_that_spells_no_prelude_name_stays_unknown() {
        // `io` is not a type. A bare `io` in type position resolves to the
        // namespace and lowers to nothing, exactly as before.
        let ty = first_fn_param_ty(
            "module x\nimport std/io\nfn f(x: io) -> number {\n  io.println(\"a\")\n  return 1\n}\n",
        );
        assert_eq!(ty, Ty::Unknown);
    }
}

