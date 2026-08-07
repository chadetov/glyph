//! `TypeExpr → Ty` lowering.
//!
//! Given a `glyph_ast::TypeExpr` plus the resolver's `ResolvedModule` +
//! `Prelude`, produce a `Ty`. Used in two places:
//! - typing function signatures (params + return type)
//! - typing `type X = ...` declarations and `const NAME: T = ...` annotations
//!
//! Multi-segment paths (`http.Response`) lower to `Ty::Unknown` for now —
//! resolving them needs the cross-module pass deferred to week 2 day 3+.

use std::sync::Arc;

use glyph_ast::{Decl, Ident, Param, TypeExpr};
use glyph_resolver::{Prelude, PreludeKind, ResolvedModule, ResolvedRef, SymbolKind};

use crate::ty::{FnParam, ParamOwner, Primitive, RecordField, Ty, UnionVariant};

/// Holds the resolver-side context a `TypeExpr → Ty` recursion needs. Cheap
/// to construct (two references); avoids threading `(resolved, prelude)`
/// through every recursive call.
pub struct Lowerer<'a> {
    pub resolved: &'a ResolvedModule,
    pub prelude: &'a Prelude,
}

impl<'a> Lowerer<'a> {
    pub fn new(resolved: &'a ResolvedModule, prelude: &'a Prelude) -> Self {
        Self { resolved, prelude }
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
                    return self.stdlib_path_ty(segments, *span).unwrap_or(Ty::Unknown);
                }
                let head = &segments[0];
                match self.resolved.resolutions.get(*span) {
                    Some(ResolvedRef::Prelude(id)) => self.prelude_ty(id, head),
                    Some(ResolvedRef::Module(id)) => {
                        let sym = self.resolved.symbols.table.get(id).expect("symbol id valid");
                        match &sym.kind {
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
                            SymbolKind::ImportNamed { original, .. } => {
                                self.imported_prelude_container(original).unwrap_or(Ty::Unknown)
                            }
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
        let ResolvedRef::Module(id) = self.resolved.resolutions.get(span)? else {
            return None;
        };
        let sym = self.resolved.symbols.table.get(id)?;
        let path = match &sym.kind {
            SymbolKind::ImportNamespace { path } | SymbolKind::ImportAlias { path, .. } => path,
            _ => return None,
        };
        let key: Vec<&str> = path.segments.iter().map(|s| s.as_ref()).collect();
        let ["std", module] = key.as_slice() else {
            return None;
        };
        let name = segments.get(1)?;
        crate::assign::stdlib_modeled_type(module, name.as_ref())
            .or_else(|| self.imported_prelude_container(name))
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
}
