//! Module-level symbol collection.
//!
//! Walks a `Module`'s top-level declarations and interns one or more `Symbol`s
//! per declaration into a `SymbolTable`. Local bindings (function parameters,
//! `let`s inside blocks, match-arm bindings) are not collected here; those
//! live in transient scopes during the resolution walk (see `resolve.rs`).
//!
//! D15 import-form mapping:
//! - `import std/io`               → one `ImportNamespace` symbol named `io`
//!   (the last path segment)
//! - `import std/http as h`        → one `ImportAlias` symbol named `h`
//! - `import std/result { Ok, Err }` → one `ImportNamed` symbol per name
//!
//! D20 enforcement (`const` module-level, `let` function-level) is the
//! parser's responsibility; the collector trusts the grammar.

use std::collections::HashMap;

use glyph_ast::{Decl, Ident, ImportKind, Module, Span};

use crate::error::ResolveError;
use crate::symbol::{Symbol, SymbolId, SymbolKind, SymbolTable};

/// The result of walking a `Module` for top-level symbols. Carries the
/// `SymbolTable` itself plus a name → id index for fast lookup during the
/// resolution walk.
#[derive(Debug, Clone)]
pub struct ModuleSymbols {
    pub table: SymbolTable,
    /// Names visible at the top level of the module. Duplicates produce a
    /// `ResolveError::DuplicateName` during collection.
    pub by_name: HashMap<Ident, SymbolId>,
}

impl ModuleSymbols {
    /// Look up a top-level name. Returns `None` if the name isn't declared in
    /// this module.
    pub fn lookup(&self, name: &str) -> Option<SymbolId> {
        self.by_name.get(name).copied()
    }
}

/// Walk a module's top-level declarations and produce a `ModuleSymbols`.
/// Duplicate names (two top-level decls or imports sharing the same identifier)
/// are reported as errors but the walk continues so the caller sees all of
/// them.
pub fn collect_module_symbols(module: &Module) -> Result<ModuleSymbols, Vec<ResolveError>> {
    let mut table = SymbolTable::new();
    let mut by_name: HashMap<Ident, SymbolId> = HashMap::new();
    let mut errors: Vec<ResolveError> = Vec::new();

    let mut ctx = CollectCtx {
        table: &mut table,
        by_name: &mut by_name,
        errors: &mut errors,
    };

    for (idx, item) in module.items.iter().enumerate() {
        let decl_idx = idx as u32;
        match item {
            Decl::Fn(f) => ctx.intern(f.name.clone(), f.span, SymbolKind::Function { decl_idx }),
            Decl::Type(t) => {
                ctx.intern(t.name.clone(), t.span, SymbolKind::Type { decl_idx });
                // Tagged-union variants hoist into module scope alongside the
                // type itself so `NetworkError({ ... })` resolves directly.
                if let glyph_ast::TypeExpr::Union { variants, .. } = &t.body {
                    for v in variants {
                        ctx.intern(v.name.clone(), v.span, SymbolKind::Variant { decl_idx });
                    }
                }
            }
            Decl::Const(c) => ctx.intern(c.name.clone(), c.span, SymbolKind::Const { decl_idx }),
            Decl::Component(c) => {
                ctx.intern(c.name.clone(), c.span, SymbolKind::Component { decl_idx })
            }
            Decl::Import(imp) => {
                if path_is_relative(&imp.path) {
                    ctx.errors
                        .push(ResolveError::RelativeImport { span: imp.span });
                }
                match &imp.kind {
                    ImportKind::Namespace => {
                        if let Some(last) = imp.path.segments.last().cloned() {
                            ctx.intern(
                                last,
                                imp.span,
                                SymbolKind::ImportNamespace {
                                    path: imp.path.clone(),
                                },
                            );
                        }
                    }
                    ImportKind::Aliased(alias) => ctx.intern(
                        alias.clone(),
                        imp.span,
                        SymbolKind::ImportAlias {
                            path: imp.path.clone(),
                            alias: alias.clone(),
                        },
                    ),
                    ImportKind::Named(names) => {
                        for n in names {
                            ctx.intern(
                                n.clone(),
                                imp.span,
                                SymbolKind::ImportNamed {
                                    path: imp.path.clone(),
                                    original: n.clone(),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(ModuleSymbols { table, by_name })
    } else {
        Err(errors)
    }
}

struct CollectCtx<'a> {
    table: &'a mut SymbolTable,
    by_name: &'a mut HashMap<Ident, SymbolId>,
    errors: &'a mut Vec<ResolveError>,
}

impl CollectCtx<'_> {
    fn intern(&mut self, name: Ident, span: Span, kind: SymbolKind) {
        let id = self.table.intern(Symbol {
            name: name.clone(),
            kind,
            span,
        });
        if let Some(prev) = self.by_name.insert(name.clone(), id) {
            // First declaration wins for downstream resolution; the duplicate
            // is still reported.
            self.by_name.insert(name.clone(), prev);
            self.errors.push(ResolveError::DuplicateName {
                name: name.to_string(),
                second_span: span,
            });
        }
    }
}

fn path_is_relative(path: &glyph_ast::ModulePath) -> bool {
    // D15 forbids relative imports. Any segment that's `.` or `..` flags it.
    // The parser currently accepts those as identifiers; if dogfooding shows
    // a case the parser already rejects, this check is harmless redundancy.
    path.segments
        .iter()
        .any(|s| s.as_ref() == "." || s.as_ref() == "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_parser::parse;

    fn collect(src: &str) -> ModuleSymbols {
        let m = parse(src).expect("parse failed");
        collect_module_symbols(&m).expect("collect failed")
    }

    #[test]
    fn collect_fn_decl() {
        let s = collect("module x\nfn main() {}\n");
        assert!(s.lookup("main").is_some());
        let id = s.lookup("main").unwrap();
        let sym = s.table.get(id).unwrap();
        assert!(matches!(sym.kind, SymbolKind::Function { .. }));
    }

    #[test]
    fn collect_type_const_component_fn() {
        let src = r#"module x
type User = { name: string }
const TODO = "x"
fn add(a: number, b: number) -> number { return a + b }
"#;
        let s = collect(src);
        assert!(matches!(
            s.table.get(s.lookup("User").unwrap()).unwrap().kind,
            SymbolKind::Type { .. }
        ));
        assert!(matches!(
            s.table.get(s.lookup("TODO").unwrap()).unwrap().kind,
            SymbolKind::Const { .. }
        ));
        assert!(matches!(
            s.table.get(s.lookup("add").unwrap()).unwrap().kind,
            SymbolKind::Function { .. }
        ));
    }

    #[test]
    fn collect_component_decl() {
        let src = "module x\ncomponent Btn() -> Component { return <button></button> }\n";
        let s = collect(src);
        assert!(matches!(
            s.table.get(s.lookup("Btn").unwrap()).unwrap().kind,
            SymbolKind::Component { .. }
        ));
    }

    #[test]
    fn collect_import_namespace_introduces_last_segment() {
        let s = collect("module x\nimport std/io\n");
        let id = s.lookup("io").expect("io should be in scope");
        match &s.table.get(id).unwrap().kind {
            SymbolKind::ImportNamespace { path } => {
                assert_eq!(path.segments.len(), 2);
                assert_eq!(path.segments[1].as_ref(), "io");
            }
            other => panic!("expected ImportNamespace, got {other:?}"),
        }
    }

    #[test]
    fn collect_import_aliased_uses_alias_name() {
        let s = collect("module x\nimport std/http as h\n");
        assert!(s.lookup("h").is_some());
        assert!(s.lookup("http").is_none(), "alias hides original name");
    }

    #[test]
    fn collect_import_named_introduces_each() {
        let s = collect("module x\nimport std/result { Result, Ok, Err }\n");
        for name in ["Result", "Ok", "Err"] {
            assert!(s.lookup(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn collect_duplicate_name_errors() {
        let m = parse("module x\nfn dup() {}\nfn dup() {}\n").unwrap();
        let result = collect_module_symbols(&m);
        let errs = result.expect_err("expected duplicate-name error");
        assert!(
            errs.iter()
                .any(|e| matches!(e, ResolveError::DuplicateName { name, .. } if name == "dup")),
            "errors were: {errs:?}"
        );
    }
}
