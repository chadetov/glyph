//! Module graph and import verification.
//!
//! The resolver intra-module pass (`resolve_module`) is sound on its own —
//! every identifier resolves to a local binding, a top-level symbol (which
//! includes the `ImportNamed` wrapper that records "this name was imported
//! from path P"), or a prelude built-in. What it cannot do alone is verify
//! that an imported name actually *exists* in the target module. That's the
//! `verify_imports` pass below.
//!
//! Phase 1 week 2 day 4 slice scope:
//! - A `ModuleGraph` trait the verifier walks once per import declaration.
//! - A `StdlibStubs` implementation seeded with the stdlib surface the four
//!   example files use. Q21 (stdlib migration pattern) and Q40 (`glyph regen`
//!   metadata) will eventually replace this with parsed Glyph stdlib sources
//!   compiled at install time; the synthesis layer is a stand-in until the
//!   stdlib actually exists.
//! - A `verify_imports` pass that runs after `collect_module_symbols` and
//!   emits `ResolveError::UnknownExportedName` for any `import M { N }` where
//!   `M` is in the graph but doesn't export `N`.
//!
//! Permissive about unknown modules in v1 day 4: third-party packages
//! (`react`) and project-local modules (`api/users`) won't be in the stub
//! graph, and the verifier silently skips them. Once Phase 5 ships package
//! metadata (the `"glyph"` key in `package.json`), the verifier will graduate
//! to "every import path must be declared in either stdlib or the package
//! manifest." Until then the typechecker still gets `Ty::Unknown` for member
//! access through these imports; nothing breaks.

use std::collections::{BTreeSet, HashMap};

use glyph_ast::{Decl, Ident, ImportKind, Module, ModulePath};

use crate::error::ResolveError;

/// Exports surface for a single module. The `names` set is the union of
/// every top-level decl name and every imported-and-re-exported name; for the
/// stdlib stubs in this slice it's just the top-level decls.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ModuleExports {
    pub names: BTreeSet<Ident>,
}

impl ModuleExports {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Ident>,
    {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        // `Arc<str>: Borrow<str>` lets the BTreeSet do its own O(log n) lookup;
        // a linear iter().any(...) would be O(n) for the same answer.
        self.names.contains(name)
    }
}

/// Lookup interface used by `verify_imports`. Implementations decide how to
/// answer "what does module `path` export?" — stdlib stubs hard-code the
/// answer, a future filesystem-backed graph would parse the target file.
pub trait ModuleGraph {
    /// `Some(exports)` if the module is known, `None` if the verifier should
    /// skip cross-module verification for this path (permissive default).
    fn exports_of(&self, path: &ModulePath) -> Option<&ModuleExports>;
}

// ============================================================================
// StdlibStubs
// ============================================================================

/// Synthetic stdlib surface, hand-coded for the four example files.
///
/// The list is intentionally minimal — every name here is referenced by at
/// least one example, the Q3 stdlib bootstrap list, or both. Anything that
/// gets added later goes through the same path: add to the appropriate stub,
/// add a test, ship in the same commit.
#[derive(Debug, Default, Clone)]
pub struct StdlibStubs {
    by_path: HashMap<String, ModuleExports>,
}

impl StdlibStubs {
    /// Seeded with the stdlib surface that the example files require.
    pub fn new() -> Self {
        let mut s = Self::default();
        // Q3 stdlib bootstrap list — the eight v1 modules. The names below are
        // the exported surface as of the brainstorm resolution; they will grow
        // as Phase 1 week 5 lands real stdlib sources.
        s.add("std/result", &["Result", "Ok", "Err"]);
        s.add("std/option", &["Option", "Some", "None"]);
        s.add(
            "std/array",
            &[
                "map", "filter", "find", "zip", "len", "push", "concat", "reverse", "slice", "any",
                "contains", "sort",
            ],
        );
        s.add(
            "std/string",
            &[
                "from", "join", "split", "len", "trim", "lower", "upper", "contains", "starts_with",
                "ends_with",
            ],
        );
        s.add(
            "std/io",
            &["println", "eprintln", "read_line", "read_to_string"],
        );
        s.add("std/json", &["parse", "stringify"]);
        s.add(
            "std/fs",
            &["read_text", "write_text", "exists", "remove", "ErrorKind"],
        );
        s.add("std/time", &["debounce", "Duration", "now", "sleep"]);
        // A `fetch`-based client (`get`/`post`/`json`) plus a small server
        // (`serve`/`Handler` and the `text`/`query`/`path` helpers).
        s.add(
            "std/http",
            &[
                "get", "post", "json", "text", "serve", "query", "path",
                "Request", "Response", "HttpError", "Handler",
            ],
        );
        s.add(
            "std/process",
            &["args", "exit", "env", "cwd"],
        );
        // Property testing (Q11 -> Option A): `test.property` over a `Stream<T>`
        // generator. Invoked inside `@example`/`@doc @run` and executed at
        // build time.
        s.add("std/test", &["property"]);
        // `Record<K, V>` is the v1 associative collection (indexing + `for k, v`
        // iteration are built in); `std/record` adds absence-aware reads and
        // value-oriented updates.
        s.add(
            "std/record",
            &["get", "has", "keys", "values", "set", "remove"],
        );
        s.add("std/stream", &["Stream", "ints", "bools", "from"]);
        s
    }

    /// Build with no entries; useful in tests that want a permissive default.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Insert a module. In debug builds, panics if the same path is seeded
    /// twice — silent overwrites turn one bug (a duplicate seed) into another
    /// (an UnknownExportedName on names the earlier seed had supplied).
    pub fn add(&mut self, path: &str, names: &[&str]) {
        let exports = ModuleExports::from_names(names.iter().map(|n| Ident::from(*n)));
        let prev = self.by_path.insert(path.to_string(), exports);
        debug_assert!(
            prev.is_none(),
            "StdlibStubs::add: duplicate seed for `{path}` — earlier exports dropped"
        );
    }

    /// True if `path` is registered (regardless of whether exports is empty).
    pub fn knows(&self, path: &ModulePath) -> bool {
        self.by_path.contains_key(&path_key(path))
    }

    /// Iterate every seeded module path and its export surface. The runtime
    /// reconciliation test uses this to assert every promised name is actually
    /// implemented by a bundled `.ts`, so the stub surface and the runtime
    /// cannot drift (the resolver's "this name exists" must imply it really does).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ModuleExports)> {
        self.by_path.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl ModuleGraph for StdlibStubs {
    fn exports_of(&self, path: &ModulePath) -> Option<&ModuleExports> {
        self.by_path.get(&path_key(path))
    }
}

/// Compose two module graphs, checking `first` then `second`. Useful for
/// test setups that want stdlib stubs plus project-local module stubs.
pub struct CompositeGraph<'a> {
    pub first: &'a dyn ModuleGraph,
    pub second: &'a dyn ModuleGraph,
}

impl<'a> ModuleGraph for CompositeGraph<'a> {
    fn exports_of(&self, path: &ModulePath) -> Option<&ModuleExports> {
        self.first
            .exports_of(path)
            .or_else(|| self.second.exports_of(path))
    }
}

// ============================================================================
// verify_imports
// ============================================================================

/// Walk every `import` declaration in `module` and emit
/// `ResolveError::UnknownExportedName` for any named import that references a
/// name the target module doesn't export.
///
/// Permissive on unknown modules: if `graph.exports_of(path)` returns `None`
/// the verifier skips the import. This keeps third-party packages (`react`)
/// and project-local modules (`api/users`) from breaking until package
/// metadata lands in Phase 5.
pub fn verify_imports(module: &Module, graph: &dyn ModuleGraph) -> Vec<ResolveError> {
    let mut errors = Vec::new();
    for item in &module.items {
        let Decl::Import(imp) = item else { continue };
        let Some(exports) = graph.exports_of(&imp.path) else {
            continue;
        };
        if let ImportKind::Named(names) = &imp.kind {
            for n in names {
                if !exports.contains(n) {
                    errors.push(ResolveError::UnknownExportedName {
                        name: n.to_string(),
                        module: path_key(&imp.path),
                        span: imp.span,
                    });
                }
            }
        }
    }
    errors
}

/// Canonical string form of a `ModulePath`. Doubles as the `HashMap` key
/// inside `StdlibStubs` and as the `module` field of `UnknownExportedName`,
/// so the lookup form and the user-visible form cannot drift apart on a
/// future canonicalization change.
/// Canonical string form of a `ModulePath`. Doubles as the `HashMap` key
/// inside `StdlibStubs` and as the `module` field of `UnknownExportedName`
/// errors. Exposed `pub` since day 9 — `glyph-db`'s `ProjectGraph` also
/// needs to hash `ModulePath` values consistently with this crate.
pub fn path_key(path: &ModulePath) -> String {
    path.segments
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_parser::parse;

    fn verify(src: &str, graph: &dyn ModuleGraph) -> Vec<ResolveError> {
        let m = parse(src).expect("parse failed");
        verify_imports(&m, graph)
    }

    #[test]
    fn known_named_imports_pass() {
        let errs = verify(
            "module x\nimport std/result { Result, Ok, Err }\n",
            &StdlibStubs::new(),
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn unknown_name_in_known_module_errors() {
        let errs = verify(
            "module x\nimport std/result { Result, Boom }\n",
            &StdlibStubs::new(),
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                ResolveError::UnknownExportedName { name, module, .. }
                    if name == "Boom" && module == "std/result"
            )),
            "errs: {errs:?}"
        );
        // The valid `Result` import should not also error.
        assert_eq!(errs.len(), 1, "errs: {errs:?}");
    }

    #[test]
    fn namespace_import_does_not_check_names() {
        // `import std/array` brings `array` into scope; member access goes
        // through the typechecker, so the verifier has nothing to check.
        let errs = verify("module x\nimport std/array\n", &StdlibStubs::new());
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn aliased_import_does_not_check_names() {
        let errs = verify("module x\nimport std/http as h\n", &StdlibStubs::new());
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn unknown_module_silently_passes() {
        // `react` isn't in the stdlib stubs. Permissive in v1 day 4 — third
        // party modules don't error until package metadata lands.
        let errs = verify(
            "module x\nimport react { use_state, use_effect }\n",
            &StdlibStubs::new(),
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn composite_graph_falls_through() {
        let stdlib = StdlibStubs::new();
        let mut project = StdlibStubs::empty();
        project.add("react", &["use_state", "use_effect"]);
        let composite = CompositeGraph {
            first: &stdlib,
            second: &project,
        };
        let errs = verify(
            "module x\nimport react { use_state }\nimport std/result { Ok }\n",
            &composite,
        );
        assert!(errs.is_empty(), "errs: {errs:?}");
    }

    #[test]
    fn composite_graph_surfaces_unknown_name_in_registered_module() {
        let stdlib = StdlibStubs::new();
        let mut project = StdlibStubs::empty();
        project.add("react", &["use_state"]);
        let composite = CompositeGraph {
            first: &stdlib,
            second: &project,
        };
        let errs = verify(
            "module x\nimport react { use_state, use_effect }\n",
            &composite,
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                ResolveError::UnknownExportedName { name, .. } if name == "use_effect"
            )),
            "errs: {errs:?}"
        );
    }

    #[test]
    fn stdlib_stubs_seed_has_q3_modules() {
        let s = StdlibStubs::new();
        for m in [
            "std/result",
            "std/option",
            "std/array",
            "std/string",
            "std/io",
            "std/json",
            "std/fs",
            "std/time",
        ] {
            let path = parse(&format!("module x\nimport {m}\n")).unwrap();
            let imp = match &path.items[0] {
                Decl::Import(i) => i,
                _ => panic!(),
            };
            assert!(
                s.knows(&imp.path),
                "stdlib stub missing the Q3 module: {m}"
            );
        }
    }
}
