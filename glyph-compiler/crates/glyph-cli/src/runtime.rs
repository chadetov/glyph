//! Build-output support: the runtime prelude + a generated `tsconfig.json`,
//! written into a build's `--out` directory so the emitted TypeScript can be
//! type-checked with `tsc -p <out>/tsconfig.json` against real types rather
//! than `any`.
//!
//! The runtime (`glyph-compiler/runtime/`) is embedded into the binary at
//! compile time, so a shipped `glyph` carries it. A program's own external
//! dependencies (npm packages, sibling modules) are not the compiler's to
//! provide; as a convention, ambient declarations placed in a `<src>/.types/`
//! directory are copied alongside the output and picked up by the generated
//! `tsconfig.json` (this is how the example programs supply their React and
//! `api/users` stubs).
//!
//! An installed npm package that ships its own types (or has an `@types/*`
//! companion) needs no such stub. The build emits `.ts` into an out directory
//! that is not inside the project, so a bare `import { z } from "zod"` cannot
//! reach the project's `node_modules` by the usual upward walk. To fix that,
//! `write_build_support` locates the project's `node_modules` (walking up from
//! the source directory) and injects a `"*"` `paths` entry pointing at it, so
//! `tsc` resolves installed packages against the project's real dependencies.
//! The emitter emits project-internal imports as *relative* specifiers, so the
//! `"*"` wildcard only ever catches external (bare) package imports.

use std::path::{Path, PathBuf};

/// (out-relative path, contents). The runtime prelude and stdlib type surface,
/// embedded from `glyph-compiler/runtime/`. Written under a dotted directory so
/// it never collides with a module named `std`.
const RUNTIME_FILES: &[(&str, &str)] = &[
    (
        ".glyph-runtime/std/result.ts",
        include_str!("../../../runtime/std/result.ts"),
    ),
    (
        ".glyph-runtime/std/option.ts",
        include_str!("../../../runtime/std/option.ts"),
    ),
    (
        ".glyph-runtime/std/schema.ts",
        include_str!("../../../runtime/std/schema.ts"),
    ),
    (
        ".glyph-runtime/std/array.ts",
        include_str!("../../../runtime/std/array.ts"),
    ),
    (
        ".glyph-runtime/std/string.ts",
        include_str!("../../../runtime/std/string.ts"),
    ),
    (
        ".glyph-runtime/std/io.ts",
        include_str!("../../../runtime/std/io.ts"),
    ),
    (
        ".glyph-runtime/std/json.ts",
        include_str!("../../../runtime/std/json.ts"),
    ),
    (
        ".glyph-runtime/std/fs.ts",
        include_str!("../../../runtime/std/fs.ts"),
    ),
    (
        ".glyph-runtime/std/process.ts",
        include_str!("../../../runtime/std/process.ts"),
    ),
    (
        ".glyph-runtime/std/stream.ts",
        include_str!("../../../runtime/std/stream.ts"),
    ),
    (
        ".glyph-runtime/std/test.ts",
        include_str!("../../../runtime/std/test.ts"),
    ),
    (
        ".glyph-runtime/std/record.ts",
        include_str!("../../../runtime/std/record.ts"),
    ),
    (
        ".glyph-runtime/std/time.ts",
        include_str!("../../../runtime/std/time.ts"),
    ),
    (
        ".glyph-runtime/std/http.ts",
        include_str!("../../../runtime/std/http.ts"),
    ),
    (
        ".glyph-runtime/std/store.ts",
        include_str!("../../../runtime/std/store.ts"),
    ),
    (
        ".glyph-runtime/glyph-bootstrap.ts",
        include_str!("../../../runtime/glyph-bootstrap.ts"),
    ),
    (
        ".glyph-runtime/glyph-prelude.d.ts",
        include_str!("../../../runtime/glyph-prelude.d.ts"),
    ),
    (
        ".glyph-runtime/glyph-stdlib.d.ts",
        include_str!("../../../runtime/glyph-stdlib.d.ts"),
    ),
];

/// The bundled Node builtin shim, written only when the project has no
/// `@types/node` (with `@types/node` installed we prefer it and skip this to
/// avoid a duplicate `declare module "fs"` conflict).
const NODE_SHIMS: (&str, &str) = (
    ".glyph-runtime/glyph-node-shims.d.ts",
    include_str!("../../../runtime/glyph-node-shims.d.ts"),
);

/// The generated `tsconfig.json`. `paths` resolves `std/*` imports to the
/// bundled runtime; `include` covers the emitted output, the runtime, and any
/// project-supplied ambient declarations copied from `<src>/.types/`. The
/// relative `paths` entry resolves against the tsconfig's own directory (TS
/// 4.1+), so no `baseUrl` is needed — and `baseUrl` is deprecated as of
/// TypeScript 6, which would make `--check` fail on a current toolchain.
///
/// `{node_modules_paths}` is filled with a `"*"` entry pointing at the
/// project's `node_modules` when one is found, so installed packages resolve;
/// it is empty otherwise (behavior identical to a project with no dependencies).
const TSCONFIG_TEMPLATE: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "noEmit": true,
    "target": "es2022",
    "lib": ["es2022", "dom"],
    "module": "esnext",
    "moduleResolution": "bundler",
    "skipLibCheck": true,
    "types": [{node_types}]{type_roots},
    "paths": {
      "std/*": ["./.glyph-runtime/std/*"]{node_modules_paths}
    }
  },
  "include": [
    "**/*.ts",
    ".glyph-runtime/**/*.ts",
    ".glyph-runtime/**/*.d.ts",
    ".types/**/*.d.ts"
  ]
}
"#;

/// Build the `tsconfig.json` text, wiring the project's `node_modules` into
/// `paths` when one was found so bare package imports resolve. Absolute path
/// values are used verbatim by TypeScript (no `baseUrl` required); backslashes
/// are escaped so a Windows path stays valid JSON.
///
/// When the project has `@types/node`, its full Node typings are loaded
/// (`types: ["node"]` with an explicit `typeRoots` pointing at the project's
/// `@types`, since the out dir sits outside the project). Otherwise `types: []`
/// keeps the ambient global surface minimal and the bundled Node shim (written
/// separately) covers the common builtins.
fn tsconfig_json(node_modules: Option<&Path>, has_types_node: bool) -> String {
    let node_modules_paths = match node_modules {
        Some(nm) => {
            let nm = nm.to_string_lossy().replace('\\', "\\\\");
            format!(",\n      \"*\": [\"{nm}/*\", \"{nm}/@types/*\"]")
        }
        None => String::new(),
    };
    let (node_types, type_roots) = if has_types_node {
        let nm = node_modules
            .expect("has_types_node implies a node_modules")
            .to_string_lossy()
            .replace('\\', "\\\\");
        (
            "\"node\"".to_string(),
            format!(",\n    \"typeRoots\": [\"{nm}/@types\"]"),
        )
    } else {
        (String::new(), String::new())
    };
    TSCONFIG_TEMPLATE
        .replace("{node_modules_paths}", &node_modules_paths)
        .replace("{node_types}", &node_types)
        .replace("{type_roots}", &type_roots)
}

/// Whether the project has `@types/node` installed (so its full Node typings can
/// be preferred over the bundled shim).
fn has_types_node(node_modules: Option<&Path>) -> bool {
    node_modules
        .map(|nm| nm.join("@types/node").join("package.json").is_file())
        .unwrap_or(false)
}

/// Find the *project's* `node_modules` by walking up from the (canonicalized)
/// source directory, returning the nearest one at or below the project root.
///
/// The walk stops at the project root — the nearest ancestor holding a `.git`
/// directory or a `package.json` — and never climbs above it. Without that
/// boundary the walk could reach an unrelated `node_modules` in a parent (a
/// stray one in `$HOME` is common) and point `tsc` at the wrong dependencies.
/// So: the nearest `node_modules` within the project wins; if the root is
/// reached with none found, the project simply has no installed dependencies in
/// scope and this returns `None` (the tsconfig then omits the wildcard, behaving
/// exactly as it did before installed-package resolution existed).
///
/// Shared with `gen dts <pkg>`, which resolves an installed package's types out
/// of the same project `node_modules`.
pub(crate) fn find_project_node_modules(src: &Path) -> Option<PathBuf> {
    let start = src.canonicalize().ok()?;
    let mut dir: &Path = &start;
    loop {
        // A `node_modules` at this level is the project's dependencies; nearest
        // wins. Checked before the root marker so a root that carries both
        // `package.json` and `node_modules` (the common case) resolves.
        let candidate = dir.join("node_modules");
        if candidate.is_dir() {
            return Some(candidate);
        }
        // Reached the project root with no `node_modules` at or below it: stop
        // rather than climb into an unrelated ancestor's dependencies.
        if dir.join(".git").exists() || dir.join("package.json").is_file() {
            return None;
        }
        dir = dir.parent()?;
    }
}

/// Write the bundled runtime, a `tsconfig.json`, and any `<src>/.types/`
/// ambient declarations into `out`, so `tsc -p <out>/tsconfig.json` can type
/// the emitted TypeScript.
pub fn write_build_support(out: &Path, src: &Path) -> std::io::Result<()> {
    for (rel, contents) in RUNTIME_FILES {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    let node_modules = find_project_node_modules(src);
    let types_node = has_types_node(node_modules.as_deref());

    // The bundled Node shim covers the common builtins out of the box. When the
    // project ships `@types/node`, prefer its full, exact typings and skip the
    // shim so its `declare module "fs"` does not collide with `@types/node`'s.
    if !types_node {
        let (rel, contents) = NODE_SHIMS;
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }

    std::fs::write(
        out.join("tsconfig.json"),
        tsconfig_json(node_modules.as_deref(), types_node),
    )?;

    // A project may supply ambient declarations for its external dependencies
    // (npm packages, sibling app modules) in `<src>/.types/`; copy them so the
    // generated config picks them up.
    let src_types = src.join(".types");
    if src_types.is_dir() {
        copy_dir(&src_types, &out.join(".types"))?;
    }
    Ok(())
}

/// Result of running `tsc` over a build's generated `tsconfig.json`.
pub enum TscOutcome {
    /// `tsc` ran and reported no errors.
    Passed,
    /// `tsc` ran and reported errors; carries its output.
    Failed(String),
    /// `tsc` was not found on `PATH`.
    NotFound,
}

/// Type-check `<out>` by running `tsc -p <out>/tsconfig.json`. Looks up `tsc`
/// on `PATH`; a project that installs TypeScript locally can instead run that
/// command itself against the generated config.
pub fn check_with_tsc(out: &Path) -> std::io::Result<TscOutcome> {
    let tsconfig = out.join("tsconfig.json");
    match std::process::Command::new("tsc")
        .arg("-p")
        .arg(&tsconfig)
        .output()
    {
        Ok(output) if output.status.success() => Ok(TscOutcome::Passed),
        Ok(output) => {
            let mut msg = String::from_utf8_lossy(&output.stdout).into_owned();
            msg.push_str(&String::from_utf8_lossy(&output.stderr));
            Ok(TscOutcome::Failed(msg))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TscOutcome::NotFound),
        Err(e) => Err(e),
    }
}

/// Recursively copy every file under `from` into `to`.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{tsconfig_json, RUNTIME_FILES};
    use glyph_resolver::StdlibStubs;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// With no project `node_modules`, the tsconfig is the plain form: `std/*`
    /// resolves to the bundled runtime and nothing else is wired into `paths`.
    /// This is byte-for-byte what every release before installed-package
    /// resolution emitted, so a dependency-free project is unaffected.
    #[test]
    fn tsconfig_without_node_modules_only_maps_std() {
        let ts = tsconfig_json(None, false);
        assert!(ts.contains(r#""std/*": ["./.glyph-runtime/std/*"]"#));
        assert!(!ts.contains(r#""*""#), "no wildcard mapping without node_modules");
        assert!(ts.contains(r#""types": [],"#), "no @types/node: empty types array");
        assert!(!ts.contains("typeRoots"), "no typeRoots without @types/node");
    }

    /// With `@types/node` installed, the tsconfig loads it (`types: ["node"]`)
    /// with an explicit `typeRoots` pointing at the project's `@types`, since the
    /// out dir lives outside the project and default type-root resolution would
    /// miss it.
    #[test]
    fn tsconfig_with_types_node_loads_it() {
        let nm = Path::new("/proj/node_modules");
        let ts = tsconfig_json(Some(nm), true);
        assert!(ts.contains(r#""types": ["node"],"#), "got: {ts}");
        assert!(ts.contains(r#""typeRoots": ["/proj/node_modules/@types"]"#), "got: {ts}");
    }

    /// With a project `node_modules`, a `"*"` entry points bare imports at both
    /// the package root and its `@types` companion, so an installed package that
    /// ships types (or has an `@types/*`) resolves without a hand-written stub.
    /// The `std/*` mapping stays, and it is more specific so `std/...` still
    /// resolves to the runtime rather than the wildcard.
    #[test]
    fn tsconfig_with_node_modules_wires_the_wildcard() {
        let nm = Path::new("/proj/node_modules");
        let ts = tsconfig_json(Some(nm), false);
        assert!(ts.contains(r#""std/*": ["./.glyph-runtime/std/*"]"#));
        assert!(ts.contains(
            r#""*": ["/proj/node_modules/*", "/proj/node_modules/@types/*"]"#
        ));
    }

    /// A Windows-style path with backslashes must stay valid JSON, so each
    /// backslash is doubled in the emitted config.
    #[test]
    fn tsconfig_escapes_backslashes_in_the_path() {
        let nm = Path::new(r"C:\proj\node_modules");
        let ts = tsconfig_json(Some(nm), false);
        assert!(ts.contains(r#""C:\\proj\\node_modules/*""#), "got: {ts}");
    }

    fn tmp_tree(prefix: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("glyph_nm_{prefix}_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir tmp tree");
        dir
    }

    /// The nearest `node_modules` at or below the project root is found.
    #[test]
    fn node_modules_found_within_the_project() {
        use super::find_project_node_modules;
        let root = tmp_tree("within");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();

        let found = find_project_node_modules(&src).expect("node_modules found");
        assert_eq!(found.file_name().unwrap(), "node_modules");
        assert_eq!(
            found.parent().unwrap().canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
    }

    /// The walk must stop at the project root (`.git`) and never climb into an
    /// unrelated ancestor's `node_modules` (the stray-`$HOME`-node_modules trap).
    #[test]
    fn node_modules_search_stops_at_the_project_root() {
        use super::find_project_node_modules;
        let home = tmp_tree("home");
        // An ancestor that DOES have node_modules — it must not be used.
        std::fs::create_dir_all(home.join("node_modules")).unwrap();
        // A git project nested inside it, with no node_modules of its own.
        let repo = home.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();

        assert!(
            find_project_node_modules(&src).is_none(),
            "must stop at repo/.git, not climb to the ancestor node_modules"
        );
    }

    /// Top-level names a runtime `.ts` exports, parsed from `export <kind> NAME`
    /// declarations. Covers the direct forms the bundled stdlib uses (`function`,
    /// `async function`, `const`, `let`, `type`, `class`, `interface`); a type
    /// and a value sharing a name (e.g. `fs.ErrorKind`) collapse to one entry.
    fn exported_names(ts: &str) -> BTreeSet<String> {
        const KINDS: [&str; 7] = [
            "async function ",
            "function ",
            "const ",
            "let ",
            "type ",
            "class ",
            "interface ",
        ];
        let mut out = BTreeSet::new();
        for line in ts.lines() {
            let Some(rest) = line.trim_start().strip_prefix("export ") else {
                continue;
            };
            for kw in KINDS {
                if let Some(after) = rest.strip_prefix(kw) {
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.insert(name);
                    }
                    break;
                }
            }
        }
        out
    }

    fn runtime_source(path: &str) -> Option<&'static str> {
        let rel = format!(".glyph-runtime/{path}.ts");
        RUNTIME_FILES
            .iter()
            .find(|(r, _)| *r == rel)
            .map(|(_, c)| *c)
    }

    /// Every name the resolver advertises for an `std/*` module must actually be
    /// exported by that module's bundled runtime `.ts`. This is the single guard
    /// that keeps `StdlibStubs` (what resolves) and the runtime (what exists)
    /// from drifting: a stub name with no implementation would be a "silent
    /// green" build that crashes at run time (gap G8).
    #[test]
    fn stdlib_stubs_match_the_bundled_runtime() {
        let stubs = StdlibStubs::new();
        let mut missing: Vec<String> = Vec::new();
        for (path, exports) in stubs.iter() {
            if !path.starts_with("std/") {
                continue;
            }
            let Some(src) = runtime_source(path) else {
                missing.push(format!("{path}: no bundled runtime .ts"));
                continue;
            };
            let actual = exported_names(src);
            for name in &exports.names {
                if !actual.contains(name.as_ref()) {
                    missing.push(format!("{path}: stub promises `{name}`, runtime does not export it"));
                }
            }
        }
        assert!(missing.is_empty(), "stdlib stub/runtime drift:\n{}", missing.join("\n"));
    }
}
