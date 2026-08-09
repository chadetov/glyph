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

use glyph_ast::{Decl, Ident, Param, TypeExpr};
use glyph_resolver::{Prelude, PreludeKind, ResolvedModule, ResolvedRef, SymbolKind};

use crate::assign::DeclTyResolver;
use crate::ty::{
    FnParam, ImportedTypeDecl, ModuleKey, ParamOwner, Primitive, RecordField, Ty, UnionVariant,
};

/// Holds the resolver-side context a `TypeExpr → Ty` recursion needs. Cheap
/// to construct (two references); avoids threading `(resolved, prelude)`
/// through every recursive call.
pub struct Lowerer<'a> {
    pub resolved: &'a ResolvedModule,
    pub prelude: &'a Prelude,
    /// Cross-module lookup for types whose declaration lives in a sibling
    /// module. `None` for callers with no project context (every db-less
    /// caller), which keeps those at module-local lowering.
    imports: Option<&'a dyn DeclTyResolver>,
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
            imports: None,
            export_module: None,
        }
    }

    /// A `Lowerer` that can reach across a module boundary through the
    /// supplied `DeclTyResolver`. Used where an annotation's lowered `Ty` has
    /// to be right for an imported type: the Assigner's walk (param and `let`
    /// annotations) and the `decl_ty` query (fn signatures).
    pub fn with_imports(
        resolved: &'a ResolvedModule,
        prelude: &'a Prelude,
        imports: &'a dyn DeclTyResolver,
    ) -> Self {
        Self {
            resolved,
            prelude,
            imports: Some(imports),
            export_module: None,
        }
    }

    /// The **export view** of a module: lowering a declaration as another
    /// module will see it. Identical to `with_imports` except that a
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
        imports: &'a dyn DeclTyResolver,
        module_path: &'a str,
    ) -> ExportLowerer<'a> {
        ExportLowerer(Self {
            resolved,
            prelude,
            imports: Some(imports),
            export_module: Some(module_path),
        })
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
                    // `import catalog as c`) gets the same D30 treatment as the
                    // named-import spelling below: the sibling module's
                    // string-literal union keeps its literal set, so a `match`
                    // over it stays exhaustive without an `else`.
                    return self
                        .stdlib_path_ty(segments, *span)
                        .or_else(|| self.qualified_string_literal_union(segments, *span))
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
                            // Anything else keeps its identity across the
                            // boundary as a `Ty::Imported`, keyed on the source
                            // module and the name that module declares, so the
                            // three legal import spellings agree.
                            SymbolKind::ImportNamed { original, path } => self
                                .imported_prelude_container(original)
                                .or_else(|| {
                                    self.imported_string_literal_union(path, original)
                                })
                                .unwrap_or_else(|| Ty::Imported {
                                    module: module_key(path),
                                    name: original.clone(),
                                }),
                            _ => Ty::Unknown,
                        }
                    }
                    Some(ResolvedRef::Local(_)) => Ty::Param {
                        name: head.clone(),
                        owner: ParamOwner::Unresolved,
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
                    })
                    .collect(),
                return_ty: Arc::new(
                    return_ty
                        .as_deref()
                        .map(|rt| self.lower(rt))
                        .unwrap_or(Ty::Prim(Primitive::Void)),
                ),
                is_async: *is_async,
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
    /// from `assign.rs` for `Expr::Lambda` and from `lower_decl_signature`;
    /// downstream crates should call `lower_decl_signature` rather than this
    /// helper directly.
    pub(crate) fn lower_callable_signature(
        &self,
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
            })
            .collect();
        let return_ty = return_ty
            .map(|rt| self.lower(rt))
            .unwrap_or(Ty::Prim(Primitive::Void));
        Ty::Fn {
            params,
            return_ty: Arc::new(return_ty),
            is_async,
        }
    }

    /// Lower the signature of a top-level declaration. `Fn` and `Component`
    /// produce a `Ty::Fn`; `Import`/`Type`/`Const` are `Ty::Unknown` here
    /// (their type information is fed into expression-typing via different
    /// paths during week 3's bidirectional checker). No wildcard arm — when
    /// a new `Decl` variant lands the compiler must force a decision here.
    pub fn lower_decl_signature(&self, decl: &Decl) -> Ty {
        match decl {
            Decl::Fn(f) => self.lower_callable_signature(&f.params, f.return_ty.as_ref(), f.is_async),
            Decl::Component(c) => self.lower_callable_signature(&c.params, c.return_ty.as_ref(), false),
            Decl::Import(_) | Decl::Type(_) | Decl::Const(_) | Decl::Interface(_) => Ty::Unknown,
        }
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

    /// The `Ty` for `ns.Name` when `ns` is a namespace import of a project
    /// sibling and `Name` is a string-literal union declared there. Keeps D30's
    /// exhaustiveness guarantee alive across the two namespace spellings, the
    /// same way `imported_string_literal_union` does for the named spelling.
    fn qualified_string_literal_union(
        &self,
        segments: &[Ident],
        span: glyph_ast::Span,
    ) -> Option<Ty> {
        if segments.len() != 2 {
            return None;
        }
        let path = self.namespace_import_path(span)?;
        self.imported_string_literal_union(path, segments.get(1)?)
    }

    /// The `Ty` for an imported name that a project sibling declares as a
    /// string-literal union (`pub type Kind = "a" | "b"`). Returning the same
    /// `Ty::StringLiteralUnion` the local declaration lowers to is what makes
    /// the match-exhaustiveness check work unchanged across the boundary:
    /// verifiability does not get to stop at a file edge.
    fn imported_string_literal_union(
        &self,
        path: &glyph_ast::ModulePath,
        name: &Ident,
    ) -> Option<Ty> {
        let module = module_key(path);
        self.imports?
            .imported_string_literal_union(module.as_str(), name.as_ref())
            .map(Ty::StringLiteralUnion)
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
    fn imported_prelude_container(&self, name: &Ident) -> Option<Ty> {
        let id = self.prelude.lookup(name.as_ref())?;
        let sym = self.prelude.table.get(id)?;
        let SymbolKind::Prelude { kind } = sym.kind else { return None };
        matches!(
            kind,
            PreludeKind::Result
                | PreludeKind::Option
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
            // `bigint` is permissive in Glyph's own checker (like `int`); `tsc`
            // enforces the real bigint/number separation and rejects `123n`
            // misuse, and the descriptor checks `typeof === "bigint"`.
            PreludeKind::BigInt => Ty::Prim(Primitive::Number),
            PreludeKind::Bool => Ty::Prim(Primitive::Bool),
            PreludeKind::Void => Ty::Prim(Primitive::Void),
            PreludeKind::UnknownTop => Ty::UnknownTop,
            PreludeKind::Never => Ty::Never,
            PreludeKind::Result
            | PreludeKind::Option
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
                assert!(matches!(owner, ParamOwner::Unresolved));
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

    /// A `DeclTyResolver` with no overrides: every method takes the trait's
    /// default. Proves `with_imports` over such a resolver is identical to
    /// `new` — the cross-module answers come from the impl, never the default.
    struct NoImports;

    impl DeclTyResolver for NoImports {
        fn decl_ty(&self, _decl_idx: u32) -> Ty {
            Ty::Unknown
        }
    }

    #[test]
    fn trait_default_yields_the_same_ty_as_no_import_context() {
        let src = "module x\nimport catalog { Kind }\nfn label(k: Kind) -> string { return \"a\" }\n";
        let m = glyph_parser::parse(src).unwrap();
        let syms = collect_module_symbols(&m).unwrap();
        let prelude = build_prelude();
        let (resolved, _) = resolve_module(&m, syms, &prelude);
        let f = match &m.items[1] {
            glyph_ast::Decl::Fn(f) => f,
            other => panic!("expected Fn, got {other:?}"),
        };
        let imports = NoImports;
        let with = Lowerer::with_imports(&resolved, &prelude, &imports).lower(&f.params[0].ty);
        let without = Lowerer::new(&resolved, &prelude).lower(&f.params[0].ty);
        assert_eq!(with, without, "the trait default must not be load-bearing");
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
        let imports = NoImports;
        let decl = Lowerer::for_export(&resolved, &prelude, &imports, "catalog")
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
        let imports = NoImports;
        let decl = Lowerer::for_export(&resolved, &prelude, &imports, "catalog")
            .lower_exported_type(td);
        assert_eq!(
            decl.generics.iter().map(|g| g.as_ref()).collect::<Vec<_>>(),
            vec!["T"]
        );
    }
}
