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
use crate::resolve::QualifiedTypeRef;

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
        s.add("std/result", &["Result", "Ok", "Err", "all"]);
        s.add("std/option", &["Option", "Some", "None"]);
        s.add(
            "std/array",
            &[
                "map", "filter", "find", "zip", "len", "get", "push", "concat", "reverse", "slice", "any",
                "contains", "sort", "fold", "index_of", "flat_map", "range", "range_from",
            ],
        );
        s.add(
            "std/string",
            &[
                "from", "join", "split", "len", "trim", "lower", "upper", "contains", "starts_with",
                "ends_with", "repeat", "pad_start", "pad_end", "slice", "index_of", "replace_all",
                "trim_start", "trim_end",
            ],
        );
        s.add(
            "std/io",
            &[
                "println",
                "eprintln",
                "print",
                "eprint",
                "is_terminal",
                "stdin_is_terminal",
                "read_line",
                "read_to_string",
                "inspect",
                "render",
            ],
        );
        s.add("std/json", &["parse", "stringify", "discriminant", "parse_with"]);
        s.add(
            "std/fs",
            &[
                "read_text", "write_text", "append_text", "read_bytes", "write_bytes",
                "append_bytes", "make_dir", "exists", "remove", "read_dir", "is_dir", "stat",
                "ErrorKind", "FsError", "FileInfo",
            ],
        );
        // An immutable sequence of octets, and the codecs between octets and
        // text. The type every other boundary in this list was missing: a PNG's
        // first byte is not valid UTF-8 on its own, so a binary format read as
        // text is corrupt before the program sees it.
        s.add(
            "std/bytes",
            &[
                "Bytes", "BytesError", "empty", "from_array", "to_array", "from_text", "to_text",
                "len", "get", "slice", "concat", "join", "equals", "index_of", "starts_with",
                "to_hex", "from_hex", "to_base64", "from_base64", "to_base64url",
                "from_base64url", "to_base32", "from_base32",
            ],
        );
        s.add(
            "std/time",
            &[
                "debounce", "Duration", "now", "sleep", "format_iso", "parse_iso", "add_days",
                "add_hours", "year", "month", "day",
            ],
        );
        // A shared-state primitive: `create(initial)` returns a `Store<T>` whose
        // `get`/`set`/`update` methods read and mutate a value held in a closure.
        s.add("std/store", &["Store", "create"]);
        // Structured-concurrency helpers over Promises: `all` (fail-fast join),
        // `race` (first to settle), `all_settled` (one outcome per task), and the
        // bounded pair `pool`/`pool_settled`.
        s.add(
            "std/task",
            &["all", "race", "pool", "pool_settled", "all_settled", "Settled"],
        );
        // Regular expressions (stateless, one `RegExp` per call).
        s.add(
            "std/regex",
            &[
                "matches", "find_all", "find_first", "captures", "captures_all", "replace_all",
                "split",
            ],
        );
        // A hash set with value semantics for primitives; maps use `Record<K, V>`.
        s.add("std/set", &["Set", "create", "unique"]);
        // Cross-platform filesystem paths over node's `path`.
        s.add(
            "std/path",
            &["join", "dirname", "basename", "extname", "is_absolute", "normalize", "relative"],
        );
        // Hashing, HMAC, and randomness over node's `crypto`. Each digest has a
        // text form returning hex and a `_bytes` form over `std/bytes`, because
        // an HMAC key is arbitrary octets and a string cannot hold one.
        s.add(
            "std/crypto",
            &[
                "sha1", "sha256", "sha512", "sha1_bytes", "sha256_bytes", "sha512_bytes",
                "hmac_sha1", "hmac_sha256", "hmac_sha512", "hmac_sha1_bytes", "hmac_sha256_bytes",
                "hmac_sha512_bytes", "random_uuid", "random_hex", "random_bytes",
                "timing_safe_equal",
            ],
        );
        // Numeric helpers over JavaScript's `Math`.
        s.add(
            "std/math",
            &[
                "PI", "E", "abs", "min", "max", "floor", "ceil", "round", "trunc", "sqrt", "pow",
                "clamp", "sign", "imul",
            ],
        );
        // A seeded, reproducible PRNG (not cryptographic; use std/crypto for that).
        s.add("std/random", &["Rng", "seeded"]);
        // base64 / base64url / hex text encodings.
        s.add(
            "std/encoding",
            &[
                "base64_encode", "base64_decode", "base64url_encode", "base64url_decode",
                "hex_encode", "hex_decode",
            ],
        );
        // Structured (JSON-line) logging for observability pipelines.
        s.add("std/log", &["Level", "debug", "info", "warn", "error", "with_fields"]);
        // Ordered collections beyond Array/Record: a double-ended queue.
        s.add("std/collections", &["Deque", "deque"]);
        // A persisted SQL database over node's built-in synchronous SQLite.
        s.add("std/sqlite", &["Db", "Row", "open"]);
        // Exact base-10 fixed-point arithmetic (money) over BigInt, no floats.
        s.add("std/decimal", &["Decimal", "decimal", "from_int", "zero"]);
        // Locale-aware plurals, numbers, money, dates, lists and collation.
        // `Intl` is a namespace global, so no method form reached it and CLDR
        // plural data had no route at all; an app guessing `n == 1` is wrong in
        // most of the world. `plural_category` answers a string-literal union so
        // a `match` over it is exhaustive without a catch-all (D30).
        s.add(
            "std/intl",
            &[
                "plural_category",
                "ordinal_category",
                "format_number",
                "format_fixed",
                "format_currency",
                "format_percent",
                "format_list",
                "relative_time",
                "format_date",
                "format_datetime",
                "compare",
                "best_locale",
            ],
        );
        // Scheduling. A global in JavaScript, so this module is the only way a
        // Glyph program can reach it, and every long-running program needs one.
        s.add(
            "std/timers",
            &["Timer", "after", "every", "cancel", "unref", "sleep"],
        );
        // A WebSocket client. Each event is its own function taking what that
        // event carries, so no handler parameter is left to be narrowed.
        s.add(
            "std/websocket",
            &[
                "Socket",
                "Server",
                "listen",
                "stop",
                "port",
                "on_stop",
                "connect",
                "connect_with",
                "protocol",
                "on_open",
                "on_message",
                "on_binary",
                "on_close",
                "on_error",
                "send",
                "send_bytes",
                "close",
                "is_open",
            ],
        );
        // TCP. The last raw host call in the examples tree: a chat daemon
        // imported node's `net` and held an opaque `Socket` that E0304 would
        // not validate. Events are individual functions, as in `std/websocket`.
        s.add(
            "std/net",
            &[
                "Socket", "Server", "ServerError", "ServerErrorKind", "listen", "stop",
                "port", "on_stop", "on_server_error", "connect", "on_connect", "on_text",
                "on_data", "on_close", "on_error", "send", "send_bytes", "close", "destroy",
                "no_delay", "peer_address", "peer_port",
            ],
        );
        // URL parsing, resolution and percent-encoding, over the host's WHATWG
        // parser. A `Url` is a record because its parts are data; a `Socket` is
        // opaque because it is a live resource.
        s.add(
            "std/url",
            &[
                "Url", "Param", "parse", "join", "format", "query_params", "query_param",
                "to_query", "encode_component", "decode_component",
            ],
        );
        // Name lookups, every one async and returning a `Result`.
        s.add(
            "std/dns",
            &["MailHost", "lookup", "ipv4", "ipv6", "text", "mail"],
        );
        // TCP with the certificate checked. A `tls` connection is a `net.Socket`,
        // so `std/net`'s functions all apply to it.
        s.add("std/tls", &["connect"]);
        // Untrusted-input discipline as types: Tainted/Trusted with sanitize.
        s.add(
            "std/taint",
            &[
                "Tainted",
                "Trusted",
                "taint",
                "sanitize",
                "trust_unchecked",
                "expose",
                "reveal_tainted",
            ],
        );
        // A `fetch`-based client (`get`/`post`/`put`/`patch`/`del`/`json`) plus a
        // small server (`serve`/`Handler`, the `json`/`text`/`html`/`redirect`/
        // `with_header` response constructors, and the request accessors).
        s.add(
            "std/http",
            &[
                "get", "post", "put", "patch", "del", "json", "text", "html", "redirect",
                "with_header", "listen", "query", "path", "form",
                "raw", "header", "query_param", "segments",
                // The bounded client (G52): one request record, a timeout that
                // aborts, a redirect policy, and HEAD.
                "send", "head", "fetch_of",
                // G118: the client-side counterpart to `raw` above. `Response.body`
                // is `unknown` and already best-effort JSON-parsed, so this is the
                // only way to read it as text without stringifying a JSON object.
                "to_text",
                "Request", "Response", "HttpError", "HttpErrorKind", "Handler", "Fetch", "RedirectPolicy",
            ],
        );
        s.add(
            "std/process",
            &["args", "exit", "set_exit_code", "exit_code", "env", "cwd"],
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
                        suggestion: crate::error::export_suggestion(
                            n,
                            exports.names.iter().map(|e| e.as_ref()),
                        ),
                        span: imp.span,
                    });
                }
            }
        }
    }
    errors
}

/// Report a local import that names no module under the build root.
///
/// [`verify_imports`] is deliberately permissive about unknown modules, which
/// is right for npm packages but means a *local* import that fails to resolve
/// says nothing at all: the imported type degrades to `unknown` and the user
/// gets a non-exhaustive-match error, or a `tsc` complaint about generated
/// code, neither of which mentions imports or the build layout. This names the
/// module that could not be resolved, and where a file with that name actually
/// lives when there is one.
///
/// `resolve` answers what the build knows about an import path — a project
/// module, an ambient `declare module` name, an installed package, or nothing;
/// `locate` answers "is there a `.glyph` file under the root whose module path
/// ends in this one, and where". Both are supplied by the caller so this crate
/// stays free of filesystem access. `std/*` and `extern/*` are the compiler's
/// own paths and are never checked.
///
/// [`ModuleResolution::Unknown`] is what keeps this from firing on correct code.
/// A build with no view of the project's installed packages cannot tell a
/// misspelled local import from a dependency that is not installed yet, so it
/// reports only what it can prove: an import a `.glyph` file under the root
/// answers to, spelled from the wrong directory. With that view, a name nothing
/// answers to is reported too.
///
/// This changes no resolution semantics. It only makes the failure legible.
pub fn verify_local_imports(
    module: &Module,
    root: &str,
    resolve: &dyn Fn(&str) -> ModuleResolution,
    locate: &dyn Fn(&str) -> Option<ModuleSite>,
) -> Vec<ResolveError> {
    let mut errors = Vec::new();
    for item in &module.items {
        let Decl::Import(imp) = item else { continue };
        let first = imp.path.segments.first().map(|s| s.as_ref()).unwrap_or("");
        if first == "std" || first == "extern" {
            continue;
        }
        let key = path_key(&imp.path);
        let resolution = resolve(&key);
        if resolution == ModuleResolution::Resolved {
            continue;
        }
        let site = locate(&key);
        if site.is_none() && resolution == ModuleResolution::Unknown {
            continue;
        }
        errors.push(ResolveError::UnresolvedModule {
            path: key,
            root: root.to_string(),
            site,
            span: imp.span,
        });
    }
    errors
}

/// Where a build found a file that could answer to an unresolved import path.
///
/// The distinction is what lets E0104 tell "you spelled the path wrong" from
/// "that module belongs to a different Glyph project, and a project's imports
/// resolve within its own root only" (D41). The resolver stays free of
/// filesystem access: the caller does the looking and reports the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleSite {
    /// A file in the importing module's own project, at this root-relative path.
    ThisProject(String),
    /// A file belonging to another Glyph project (a sibling, a nested one, or an
    /// enclosing one), with that project's root for the message to name.
    OtherProject { file: String, project: String },
}

/// What a build knows about an import path, for [`verify_local_imports`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleResolution {
    /// A project module, an ambient `declare module` name, or an installed
    /// package. Nothing to report.
    Resolved,
    /// Nothing in the build answers to this name, and the build can see the
    /// project's installed packages, so npm is not the explanation.
    Unresolved,
    /// Nothing answers to it, but the build has no view of installed packages
    /// (no `node_modules` within the project), so an uninstalled dependency and
    /// a typo are indistinguishable.
    Unknown,
}

/// Hold every `ns.Name` type annotation to the same export rule
/// `import ns { Name }` is held to, and emit `ResolveError::UnknownExportedName`
/// where it fails.
///
/// Without this, which spelling brought a type into scope decided whether
/// visibility applied: `import catalog { Secret }` on a non-`pub` type was
/// E0105, while `import catalog` plus `catalog.Secret` reported nothing and the
/// checker went on to resolve the private type's field set. The refs come from
/// the resolver's own type walk, so the two spellings cover the same positions.
///
/// Permissive on unknown modules for the same reason `verify_imports` is: an
/// npm package or a module outside the project answers `None` and is skipped.
pub fn verify_qualified_type_refs(
    refs: &[QualifiedTypeRef],
    graph: &dyn ModuleGraph,
) -> Vec<ResolveError> {
    let mut errors = Vec::new();
    for r in refs {
        let Some(exports) = graph.exports_of(&r.module) else {
            continue;
        };
        if !exports.contains(&r.name) {
            errors.push(ResolveError::UnknownExportedName {
                name: r.name.to_string(),
                module: path_key(&r.module),
                suggestion: crate::error::export_suggestion(
                    &r.name,
                    exports.names.iter().map(|e| e.as_ref()),
                ),
                span: r.span,
            });
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

#[cfg(test)]
mod local_import_tests {
    use super::*;
    use glyph_parser::parse;

    /// A build that can see the project's installed packages: anything not in
    /// `project` is genuinely unresolved.
    fn errors(src: &str, project: &[&str], located: Option<&str>) -> Vec<ResolveError> {
        let module = parse(src).expect("parse");
        verify_local_imports(
            &module,
            "root",
            &|p| {
                if project.contains(&p) {
                    ModuleResolution::Resolved
                } else {
                    ModuleResolution::Unresolved
                }
            },
            &|_| located.map(|f| ModuleSite::ThisProject(f.to_string())),
        )
    }

    /// A build with no `node_modules` in sight: it cannot tell an uninstalled
    /// dependency from a typo.
    fn errors_without_npm_view(
        src: &str,
        project: &[&str],
        located: Option<&str>,
    ) -> Vec<ResolveError> {
        let module = parse(src).expect("parse");
        verify_local_imports(
            &module,
            "root",
            &|p| {
                if project.contains(&p) {
                    ModuleResolution::Resolved
                } else {
                    ModuleResolution::Unknown
                }
            },
            &|_| located.map(|f| ModuleSite::ThisProject(f.to_string())),
        )
    }

    #[test]
    fn std_and_extern_imports_are_never_checked() {
        // Those two paths are the compiler's own; it resolves them itself.
        let e = errors(
            "module m\nimport std/io\nimport extern/thing\n",
            &[],
            Some("io.glyph"),
        );
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn a_resolved_project_module_is_not_reported() {
        let e = errors("module m\nimport lib\n", &["lib"], Some("lib.glyph"));
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn an_unresolved_import_with_a_file_under_the_root_is_reported_once() {
        let e = errors(
            "module m\nimport model { Id }\n",
            &["app/model"],
            Some("app/model.glyph"),
        );
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].code(), "E0104");
        let msg = e[0].to_string();
        assert!(msg.contains("`model`"), "{msg}");
        assert!(msg.contains("`root`"), "{msg}");
        assert!(msg.contains("app/model.glyph"), "{msg}");
    }

    #[test]
    fn a_declared_or_installed_package_is_not_reported() {
        // `resolves` answers for ambient `declare module` names and installed
        // packages too, so an npm import is quiet even when an unrelated local
        // file shares its basename. That collision used to be the whole test
        // for "is this npm", and it errored on correct code.
        let e = errors(
            "module m\nimport tinylog { log }\n",
            &["tinylog"],
            Some("vendor/tinylog.glyph"),
        );
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn an_import_naming_nothing_at_all_is_reported_without_a_found_at_clause() {
        // A misspelling. Nothing in the build answers to it, npm included.
        let e = errors("module m\nimport modle { Id }\n", &["model"], None);
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].code(), "E0104");
        let msg = e[0].to_string();
        assert!(msg.contains("`modle`"), "{msg}");
        assert!(!msg.contains("There is a"), "{msg}");
    }

    #[test]
    fn an_unknown_name_stays_quiet_when_the_build_cannot_see_installed_packages() {
        // No `node_modules` anywhere: `import react` in a project whose deps are
        // not installed yet (or are hoisted above a package boundary) is not
        // something this check can call wrong, and an error on correct code is
        // worse than a late one from `tsc`.
        let e = errors_without_npm_view("module m\nimport react { Component }\n", &[], None);
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn the_layout_error_is_reported_even_without_an_npm_view() {
        // A `.glyph` file under the root answers to this name, which is provable
        // without knowing anything about npm.
        let e = errors_without_npm_view(
            "module m\nimport model { Id }\n",
            &["app/model"],
            Some("app/model.glyph"),
        );
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].code(), "E0104");
    }
}
