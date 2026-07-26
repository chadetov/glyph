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
#[derive(Debug, Clone, PartialEq, Eq)]
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

    // D15 barrel-file detection: a module whose only top-level items are
    // imports re-exports nothing (Glyph imports stay locally bound) and is the
    // forbidden barrel-file pattern. Track whether any value declaration
    // appears and the first import's span for the diagnostic.
    let mut has_value_decl = false;
    let mut first_import_span: Option<Span> = None;
    for item in &module.items {
        match item {
            Decl::Import(imp) => {
                first_import_span.get_or_insert(imp.span);
            }
            _ => has_value_decl = true,
        }
    }

    for (idx, item) in module.items.iter().enumerate() {
        let decl_idx = idx as u32;
        match item {
            Decl::Fn(f) => ctx.intern_vis(
                f.name.clone(),
                f.span,
                SymbolKind::Function { decl_idx },
                item.is_public(),
            ),
            Decl::Type(t) => {
                ctx.intern_vis(
                    t.name.clone(),
                    t.span,
                    SymbolKind::Type { decl_idx },
                    t.is_public,
                );
                // Tagged-union variants hoist into module scope alongside the
                // type itself so `NetworkError({ ... })` resolves directly; they
                // share the type's visibility.
                if let glyph_ast::TypeExpr::Union { variants, .. } = &t.body {
                    for v in variants {
                        ctx.intern_vis(
                            v.name.clone(),
                            v.span,
                            SymbolKind::Variant { decl_idx },
                            t.is_public,
                        );
                    }
                }
            }
            Decl::Const(c) => ctx.intern_vis(
                c.name.clone(),
                c.span,
                SymbolKind::Const { decl_idx },
                c.is_public,
            ),
            Decl::Component(c) => ctx.intern_vis(
                c.name.clone(),
                c.span,
                SymbolKind::Component { decl_idx },
                c.is_public,
            ),
            // An interface is a type name: usable as a generic bound and as an
            // ordinary type. Intern it in the same namespace as `type`.
            Decl::Interface(i) => ctx.intern_vis(
                i.name.clone(),
                i.span,
                SymbolKind::Type { decl_idx },
                i.is_public,
            ),
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

    // A module with at least one import and no value declarations is a barrel
    // file (D15). An empty module (no imports, no decls) is not flagged.
    if !has_value_decl {
        if let Some(span) = first_import_span {
            errors.push(ResolveError::BarrelFile { span });
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
        self.intern_vis(name, span, kind, false);
    }

    /// Intern a symbol with an explicit visibility (0.1.16). Top-level decls pass
    /// their `pub` status; imported bindings are never public (they re-bind
    /// another module's name and are not re-exported, D15).
    fn intern_vis(&mut self, name: Ident, span: Span, kind: SymbolKind, is_public: bool) {
        // Glyph permits TS reserved words as identifiers, but they emit an
        // illegal TS binding name. Reject them here (D: stricter than TS).
        if crate::reserved::is_reserved_ts_word(name.as_ref()) {
            self.errors.push(ResolveError::ReservedWordName {
                name: name.to_string(),
                span,
            });
        }
        let id = self.table.intern(Symbol {
            name: name.clone(),
            kind,
            span,
            is_public,
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
        let s = collect("module x\nimport std/io\nfn f() {}\n");
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
        let s = collect("module x\nimport std/http as h\nfn f() {}\n");
        assert!(s.lookup("h").is_some());
        assert!(s.lookup("http").is_none(), "alias hides original name");
    }

    #[test]
    fn collect_import_named_introduces_each() {
        let s = collect("module x\nimport std/result { Result, Ok, Err }\nfn f() {}\n");
        for name in ["Result", "Ok", "Err"] {
            assert!(s.lookup(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn imports_only_file_is_a_barrel() {
        let m = parse("module x\nimport std/result { Result }\nimport std/io\n").unwrap();
        let errs = collect_module_symbols(&m).expect_err("expected a barrel-file error");
        assert!(
            errs.iter().any(|e| matches!(e, ResolveError::BarrelFile { .. })),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn import_plus_a_declaration_is_not_a_barrel() {
        let s = collect("module x\nimport std/io\nfn main() {}\n");
        assert!(s.lookup("main").is_some());
    }

    #[test]
    fn empty_module_is_not_a_barrel() {
        // No imports and no decls: an empty stub, not a re-export barrel.
        let m = parse("module x\n").unwrap();
        assert!(
            collect_module_symbols(&m).is_ok(),
            "an empty module must not be flagged as a barrel file"
        );
    }

    #[test]
    fn reserved_word_fn_name_is_rejected() {
        // `switch` is a TS reserved word Glyph's lexer permits as an identifier;
        // emitting `function switch()` would fail `tsc`, so reject it here.
        let m = parse("module x\nfn switch() {}\n").unwrap();
        let errs = collect_module_symbols(&m).expect_err("expected a reserved-word error");
        assert!(
            errs.iter().any(
                |e| matches!(e, ResolveError::ReservedWordName { name, .. } if name == "switch")
            ),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn reserved_word_type_and_variant_names_are_rejected() {
        let src = "module x\ntype class = | delete | Ok({ v: number })\n";
        let m = parse(src).unwrap();
        let errs = collect_module_symbols(&m).expect_err("expected reserved-word errors");
        // The type name `class` and the variant name `delete` are both reserved
        // TS words Glyph lexes as identifiers. (`new` is now a Glyph keyword,
        // rejected at parse time, so it no longer round-trips through here.)
        for bad in ["class", "delete"] {
            assert!(
                errs.iter().any(
                    |e| matches!(e, ResolveError::ReservedWordName { name, .. } if name == bad)
                ),
                "missing reserved-word error for `{bad}`; errors were: {errs:?}"
            );
        }
    }

    #[test]
    fn ordinary_names_are_not_flagged_as_reserved() {
        let s = collect("module x\nfn evaluate() {}\nfn klass() {}\n");
        assert!(s.lookup("evaluate").is_some());
        assert!(s.lookup("klass").is_some());
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
