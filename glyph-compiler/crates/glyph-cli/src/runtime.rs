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
        ".glyph-runtime/std/bytes.ts",
        include_str!("../../../runtime/std/bytes.ts"),
    ),
    (
        ".glyph-runtime/std/net.ts",
        include_str!("../../../runtime/std/net.ts"),
    ),
    (
        ".glyph-runtime/std/url.ts",
        include_str!("../../../runtime/std/url.ts"),
    ),
    (
        ".glyph-runtime/std/dns.ts",
        include_str!("../../../runtime/std/dns.ts"),
    ),
    (
        ".glyph-runtime/std/tls.ts",
        include_str!("../../../runtime/std/tls.ts"),
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
        ".glyph-runtime/std/task.ts",
        include_str!("../../../runtime/std/task.ts"),
    ),
    (
        ".glyph-runtime/std/regex.ts",
        include_str!("../../../runtime/std/regex.ts"),
    ),
    (
        ".glyph-runtime/std/set.ts",
        include_str!("../../../runtime/std/set.ts"),
    ),
    (
        ".glyph-runtime/std/path.ts",
        include_str!("../../../runtime/std/path.ts"),
    ),
    (
        ".glyph-runtime/std/crypto.ts",
        include_str!("../../../runtime/std/crypto.ts"),
    ),
    (
        ".glyph-runtime/std/math.ts",
        include_str!("../../../runtime/std/math.ts"),
    ),
    (
        ".glyph-runtime/std/random.ts",
        include_str!("../../../runtime/std/random.ts"),
    ),
    (
        ".glyph-runtime/std/encoding.ts",
        include_str!("../../../runtime/std/encoding.ts"),
    ),
    (
        ".glyph-runtime/std/log.ts",
        include_str!("../../../runtime/std/log.ts"),
    ),
    (
        ".glyph-runtime/std/collections.ts",
        include_str!("../../../runtime/std/collections.ts"),
    ),
    (
        ".glyph-runtime/std/sqlite.ts",
        include_str!("../../../runtime/std/sqlite.ts"),
    ),
    (
        ".glyph-runtime/std/decimal.ts",
        include_str!("../../../runtime/std/decimal.ts"),
    ),
    (
        ".glyph-runtime/std/taint.ts",
        include_str!("../../../runtime/std/taint.ts"),
    ),
    (
        ".glyph-runtime/std/intl.ts",
        include_str!("../../../runtime/std/intl.ts"),
    ),
    (
        ".glyph-runtime/std/timers.ts",
        include_str!("../../../runtime/std/timers.ts"),
    ),
    (
        ".glyph-runtime/std/websocket.ts",
        include_str!("../../../runtime/std/websocket.ts"),
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
/// `build.rs` also reads the shim's `declare module` names, so a `import fs`
/// is not mistaken for an unresolvable local import.
pub(crate) const NODE_SHIMS: (&str, &str) = (
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
/// `{excludes}` is filled with the output directories of any projects nested
/// inside this one (D41). `include`'s `**/*.ts` would otherwise reach down into
/// them, and the outer project would type-check a nested project's emitted
/// files under the *outer* project's configuration: without the nested
/// project's `.types/**/*.d.ts`, without its `node_modules`. That check is not
/// the one that project asked for, and it fails on code that is correct, while
/// the nested project's own `tsc` run passes. Excluding them leaves each
/// project checked exactly once, by its own config.
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
  ]{excludes}
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
fn tsconfig_json(
    node_modules: Option<&Path>,
    has_types_node: bool,
    nested_out_dirs: &[String],
) -> String {
    // Both the directory itself and everything under it: TypeScript's `exclude`
    // does not imply the subtree, and the emitted modules sit inside it.
    let excludes = if nested_out_dirs.is_empty() {
        String::new()
    } else {
        let entries: Vec<String> = nested_out_dirs
            .iter()
            .flat_map(|d| {
                let d = d.trim_end_matches('/').replace('\\', "/");
                [format!("    \"{d}\""), format!("    \"{d}/**/*\"")]
            })
            .collect();
        format!(",\n  \"exclude\": [\n{}\n  ]", entries.join(",\n"))
    };
    let node_modules_paths = match node_modules {
        Some(nm) => {
            let nm = nm.to_string_lossy().replace('\\', "\\\\");
            // `@types/*` is tried BEFORE the bare package. A `paths` entry
            // short-circuits on the first candidate that *resolves to a module*,
            // even one with no type declarations, so listing the package first
            // makes a typeless JS package (`pg`, `react`, `express`, `lodash`,
            // and the rest of the "ships JS, types live in `@types/*`"
            // ecosystem) resolve to its untyped `.js` and report an implicit
            // `any` (TS7016) instead of falling through to its `@types`
            // companion. Trying `@types/<pkg>` first fixes that; a package that
            // ships its own types has no `@types` entry to match, so it falls
            // through to the bare path and uses its bundled declarations.
            format!(",\n      \"*\": [\"{nm}/@types/*\", \"{nm}/*\"]")
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
        .replace("{excludes}", &excludes)
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
        // Reached the repository root with no `node_modules` at or below it:
        // stop rather than climb into an unrelated ancestor's dependencies (a
        // stray `$HOME/node_modules` is common). The stop is `.git` only, never
        // the first `package.json`: since D41 a nested app carries a marker
        // manifest, and stopping there would lose sight of a monorepo's hoisted
        // `node_modules` and leave every npm import untyped.
        if dir.join(".git").exists() {
            return None;
        }
        dir = dir.parent()?;
    }
}

/// The stem (e.g. `"array"`) of a `RUNTIME_FILES` entry under `.glyph-runtime/std/`,
/// or `None` for an entry outside `std/` (the bootstrap and the two `.d.ts`
/// files, which are not part of the reachability graph and are always written).
fn std_stem(rel: &str) -> Option<&str> {
    rel.strip_prefix(".glyph-runtime/std/")
        .and_then(|f| f.strip_suffix(".ts"))
}

/// Every `.ts`/`.tsx` file under `dir`, recursively, or an empty vector when
/// `dir` is not a directory.
///
/// Symlinks are followed, because a project that shares one shim between two
/// build roots is likely to link it into each `<src>/extern/`, and a shim the
/// walk skips is a shim whose std imports get pruned. `seen` holds the canonical
/// path of each directory already walked so a cycle (`extern/loop -> ..`)
/// terminates. A read that fails is skipped rather than raised: this walk only
/// decides how much of the runtime to keep, and the staging copy right after it
/// reports a genuinely unreadable `<src>/extern/`.
fn extern_shims(dir: &Path) -> Vec<std::path::PathBuf> {
    fn walk(
        dir: &Path,
        seen: &mut std::collections::HashSet<std::path::PathBuf>,
        out: &mut Vec<std::path::PathBuf>,
    ) {
        let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        if !seen.insert(canonical) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `metadata` resolves a link, so a link to a file reports as a file
            // and a broken link has no metadata at all and is skipped.
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                walk(&path, seen, out);
            } else if meta.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".ts") || n.ends_with(".tsx"))
            {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, &mut std::collections::HashSet::new(), &mut out);
    }
    out
}

/// The `std/*` modules a build must materialize: every module reachable from
/// what the emitted program actually imports, closed over each std module's own
/// sibling imports (`array.ts` imports `option.ts`, for instance), plus `result`
/// and the bootstrap, which every emitted module pulls in unconditionally (see
/// `bootstrap_specifier` in `glyph-emit`) regardless of what the *program*
/// imports.
///
/// This is what keeps a one-import program from materializing all 36 bundled
/// std modules on every build (G115) — seven of which (`dns`, `fs`, `http`,
/// `net`, `process`, `sqlite`, `tls`) are Node-only and unusable, dead weight at
/// best, in a browser bundle. `emitted` is the list of `.ts` rel paths this
/// build just wrote to `out` (`BuildReport::emitted`); their contents are read
/// back off disk rather than threaded through in memory, since `write_build_support`
/// runs after they are written and this keeps the reachability walk decoupled
/// from the emitter's in-memory representation.
///
/// The emitted program is not the only thing the generated tsconfig type-checks.
/// Hand-written TypeScript under `<src>/extern/` is staged into `<out>/extern/`,
/// is covered by the tsconfig `include`, and reaches the stdlib through the
/// deliberate `"std/*"` path mapping (D29). A std module only a shim imports is
/// therefore reachable, and pruning it turns a build that passed into
/// `TS2307: Cannot find module 'std/time'`. The shims are read from `<src>`
/// rather than from the staged copy in `<out>`: this runs before the staging
/// pass below, so `<out>/extern/` is either absent (a first build) or a previous
/// build's copy, and `copy_dir` never deletes, so a shim removed from `<src>`
/// would go on holding its std modules alive forever.
fn reachable_std_modules(
    out: &Path,
    src: &Path,
    emitted: &[String],
) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    // Every std module a bundled runtime file (not the program) imports from a
    // sibling, e.g. `std/array.ts`'s `import { ... } from "./option"`. Built by
    // scanning the embedded runtime sources themselves rather than hand-maintained,
    // so a new cross-module `std/*` import stays correct with no second place to
    // update.
    fn sibling_imports(ts: &str) -> HashSet<String> {
        let mut out = HashSet::new();
        let mut rest = ts;
        while let Some(idx) = rest.find("from \"./") {
            let after = &rest[idx + "from \"./".len()..];
            let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let len = name.len();
            if !name.is_empty() {
                out.insert(name);
            }
            rest = &after[len..];
        }
        out
    }

    // Same scan, applied to a program's own emitted `.ts`: what it imports is a
    // relative specifier ending in `.glyph-runtime/std/<name>` (see
    // `runtime_specifier` in `glyph-emit`), regardless of how many `../` reach it.
    fn program_std_imports(ts: &str) -> HashSet<String> {
        let mut out = HashSet::new();
        let mut rest = ts;
        const MARK: &str = ".glyph-runtime/std/";
        while let Some(idx) = rest.find(MARK) {
            let after = &rest[idx + MARK.len()..];
            let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let len = name.len();
            if !name.is_empty() {
                out.insert(name);
            }
            rest = &after[len..];
        }
        out
    }

    // What a hand-written `extern/*.ts` shim imports. A shim may spell a std
    // module either way: bare `std/<name>`, which the generated tsconfig maps
    // onto the bundled runtime, or the relative `../.glyph-runtime/std/<name>`
    // the emitter itself uses. Both are legal, so both count. Requiring the
    // match to sit right after a quote or after `.glyph-runtime/` is what keeps
    // an unrelated path like `./mystd/x` from registering as a std module.
    fn extern_std_imports(ts: &str) -> HashSet<String> {
        let mut out = HashSet::new();
        const MARK: &str = "std/";
        let mut from = 0;
        while let Some(idx) = ts[from..].find(MARK) {
            let at = from + idx;
            let after = &ts[at + MARK.len()..];
            let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            from = at + MARK.len() + name.len();
            if name.is_empty() {
                continue;
            }
            let before = &ts[..at];
            let quoted = before.ends_with('"') || before.ends_with('\'') || before.ends_with('`');
            if quoted || before.ends_with(".glyph-runtime/") {
                out.insert(name);
            }
        }
        out
    }

    let adjacency: Vec<(&str, HashSet<String>)> = RUNTIME_FILES
        .iter()
        .filter_map(|(rel, contents)| std_stem(rel).map(|stem| (stem, sibling_imports(contents))))
        .collect();

    // `result` is always reachable: the bootstrap (always written) imports it
    // unconditionally, and every emitted module imports the bootstrap.
    let mut reached: HashSet<String> = HashSet::new();
    reached.insert("result".to_string());
    for rel in emitted {
        if let Ok(ts) = std::fs::read_to_string(out.join(rel)) {
            reached.extend(program_std_imports(&ts));
        }
    }
    for shim in extern_shims(&src.join("extern")) {
        if let Ok(ts) = std::fs::read_to_string(&shim) {
            reached.extend(extern_std_imports(&ts));
        }
    }

    // Close over sibling imports until nothing new is added.
    loop {
        let mut added = false;
        for (stem, deps) in &adjacency {
            if reached.contains(*stem) {
                for dep in deps {
                    if reached.insert(dep.clone()) {
                        added = true;
                    }
                }
            }
        }
        if !added {
            break;
        }
    }
    reached
}

/// Write the bundled runtime, a `tsconfig.json`, and any `<src>/.types/`
/// ambient declarations into `out`, so `tsc -p <out>/tsconfig.json` can type
/// the emitted TypeScript.
///
/// `nested_out_dirs` names the output directories of projects nested inside this
/// one, relative to `out`, so the generated config does not type-check them; see
/// `TSCONFIG_TEMPLATE`. `emitted` is the rel paths of the `.ts` files this build
/// just wrote (`BuildReport::emitted`), used together with the `<src>/extern/`
/// shims staged below to compute which `std/*` modules this build actually
/// reaches (G115) — everything outside `std/` (the bootstrap, the two `.d.ts`
/// files) is written unconditionally.
pub fn write_build_support(
    out: &Path,
    src: &Path,
    nested_out_dirs: &[String],
    emitted: &[String],
) -> std::io::Result<()> {
    let reachable = reachable_std_modules(out, src, emitted);
    for (rel, contents) in RUNTIME_FILES {
        let path = out.join(rel);
        // An `std/*` module not reached from this build's program (directly or
        // through another std module) is skipped, and any file a *previous*
        // build left behind for it is removed, so shrinking a program's imports
        // shrinks its bundled runtime on the next build too.
        if let Some(stem) = std_stem(rel) {
            if !reachable.contains(stem) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
        }
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
    // When `@types/node` is present we also remove any shim a previous build
    // wrote while it was absent: a stale shim is still globbed in by the tsconfig
    // `include`, and its `declare module "node:crypto"` then merges with
    // `@types/node`'s, resolving `randomBytes(n).toString("hex")` to a 0-arg
    // `toString` and reddening the whole build (`std/crypto.ts` TS2554).
    let shim_path = out.join(NODE_SHIMS.0);
    if types_node {
        let _ = std::fs::remove_file(&shim_path);
    } else {
        if let Some(parent) = shim_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&shim_path, NODE_SHIMS.1)?;
    }

    std::fs::write(
        out.join("tsconfig.json"),
        tsconfig_json(node_modules.as_deref(), types_node, nested_out_dirs),
    )?;

    // A project may supply ambient declarations for its external dependencies
    // (npm packages, sibling app modules) in `<src>/.types/`; copy them so the
    // generated config picks them up.
    let src_types = src.join(".types");
    if src_types.is_dir() {
        copy_dir(&src_types, &out.join(".types"))?;
    }

    // Hand-written TypeScript a Glyph module reaches through an `import extern/*`
    // lives in `<src>/extern/`. Stage it verbatim into `<out>/extern/` so the
    // emitted relative specifier resolves and the tsconfig type-checks it; the
    // prune pass skips `extern/`, so it survives a rebuild.
    let src_extern = src.join("extern");
    if src_extern.is_dir() {
        mirror_dir(&src_extern, &out.join("extern"))?;
    }
    Ok(())
}

/// Copy `from` into `to`, and delete anything in `to` that `from` no longer has.
///
/// `copy_dir` only ever adds, which was harmless while every std module was
/// written unconditionally: a shim left behind after its source was deleted
/// still resolved its imports. Pruning changed that. The stale copy is still
/// inside the generated tsconfig's `include`, so it is still type-checked, and
/// the std module only it imported has just been pruned away. The build then
/// fails on a path that no longer exists in `src`, keeps failing on every
/// subsequent build, and only `rm -rf <out>` clears it. Under `--watch` the
/// session stays red for good.
///
/// Deleting is confined to this one call site rather than folded into
/// `copy_dir`, which also stages `.types` and backs the examples runner; making
/// that one prune would reach further than the problem.
fn mirror_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    copy_dir(from, to)?;
    prune_absent(from, to)
}

/// Remove every entry under `to` with no counterpart under `from`.
fn prune_absent(from: &Path, to: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(to) {
        Ok(e) => e,
        // Nothing staged yet, so nothing can be stale.
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let mirror = from.join(entry.file_name());
        let is_dir = entry.file_type()?.is_dir();
        if is_dir {
            if mirror.is_dir() {
                prune_absent(&mirror, &entry.path())?;
            } else {
                std::fs::remove_dir_all(entry.path())?;
            }
        } else if !mirror.is_file() {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Result of running `tsc` over a build's generated `tsconfig.json`.
#[derive(Debug)]
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

/// Type-check every project of a tree build (D41). Each project's `--out`
/// subtree carries its own staged runtime and `tsconfig.json` (that is what
/// `build_project_inner` writes), so `tsc` runs once per project.
///
/// A missing `tsc` ends the walk and is reported as `NotFound`: "the
/// type-checker is not installed" and "your code has type errors" are different
/// answers, and the first must never be reported as the second.
pub fn check_tree_with_tsc(
    tree: &crate::build::TreeReport,
    out: &Path,
) -> std::io::Result<TscOutcome> {
    let mut failures = String::new();
    for p in &tree.projects {
        if p.report.emitted.is_empty() {
            continue;
        }
        match check_with_tsc(&out.join(&p.project.out_rel))? {
            TscOutcome::Passed => {}
            TscOutcome::NotFound => return Ok(TscOutcome::NotFound),
            TscOutcome::Failed(msg) => failures.push_str(&msg),
        }
    }
    if failures.is_empty() {
        Ok(TscOutcome::Passed)
    } else {
        Ok(TscOutcome::Failed(failures))
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


/// One export declared by a runtime `std/*.ts` module.
pub struct ExportedItem {
    pub name: String,
    /// A `type`/`interface`, which ships no runtime binding.
    pub is_type: bool,
}

/// Every export a runtime stdlib module advertises, skipping `@internal` ones.
///
/// This exists because five separate gates were each scanning `runtime/std/*.ts`
/// for exports with their own slightly different prefix lists, and they have to
/// agree: one decides whether a name is documented, one whether the resolver
/// seeds it, one whether it needs the inline `type` modifier. A name that is
/// surface to one and not another is exactly the drift these gates exist to
/// catch, so they read it from here.
///
/// An export whose preceding comment carries `@internal` is one stdlib module
/// reaching into another (`net.adopt` is the bind `std/http` shares). It is not
/// any module's advertised surface: it stays out of the reference, out of the
/// resolver seed, and importing it from Glyph is correctly `E0105`.
pub fn exported_items(ts: &str) -> Vec<ExportedItem> {
    const VALUE_KINDS: [&str; 4] = ["async function ", "function ", "const ", "let "];
    const TYPE_KINDS: [&str; 2] = ["type ", "interface "];
    let mut out = Vec::new();
    let mut internal = false;
    for line in ts.lines() {
        let trimmed = line.trim_start();
        if trimmed.contains("@internal") {
            internal = true;
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("export ") else {
            // The marker attaches to the next export, not to code further down.
            if !trimmed.is_empty()
                && !trimmed.starts_with("//")
                && !trimmed.starts_with('*')
                && !trimmed.starts_with("/*")
            {
                internal = false;
            }
            continue;
        };
        let hit = TYPE_KINDS
            .iter()
            .find_map(|kw| rest.strip_prefix(kw).map(|a| (a, true)))
            .or_else(|| {
                VALUE_KINDS
                    .iter()
                    .find_map(|kw| rest.strip_prefix(kw).map(|a| (a, false)))
            });
        let Some((after, is_type)) = hit else { continue };
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        if internal {
            internal = false;
            continue;
        }
        out.push(ExportedItem { name, is_type });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{find_project_node_modules, reachable_std_modules, tsconfig_json, RUNTIME_FILES};
    use glyph_resolver::StdlibStubs;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A uniquely named temp directory for the `node_modules`-climb tests.
    fn climb_tmp(prefix: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "glyph_climb_{prefix}_{}_{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// A marked nested app finds the monorepo's hoisted `node_modules`. The
    /// climb used to stop at the first `package.json`, which since D41 is the
    /// app's own marker manifest, and every npm import in a nested app would
    /// have become an E0104.
    #[test]
    fn the_climb_passes_a_marker_manifest_to_reach_a_hoisted_node_modules() {
        let repo = climb_tmp("hoisted");
        std::fs::create_dir_all(repo.join(".git")).expect("git dir");
        std::fs::create_dir_all(repo.join("node_modules")).expect("node_modules");
        let app = repo.join("apps").join("app");
        std::fs::create_dir_all(app.join("src")).expect("app src");
        std::fs::write(
            app.join("package.json"),
            "{ \"name\": \"app\", \"private\": true, \"glyph\": {} }\n",
        )
        .expect("marker");

        // The temp dir is reached through a symlink on macOS, so compare
        // canonical paths rather than the spellings.
        let found = find_project_node_modules(&app.join("src")).expect("hoisted node_modules");
        assert_eq!(
            found.canonicalize().expect("canonicalize found"),
            repo.join("node_modules")
                .canonicalize()
                .expect("canonicalize expected")
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// The climb stops at the repository root, so a `node_modules` belonging to
    /// an unrelated ancestor (a stray one in `$HOME` is the common case) is not
    /// adopted.
    #[test]
    fn the_climb_stops_at_the_repository_root() {
        let outer = climb_tmp("stop_at_git");
        std::fs::create_dir_all(outer.join("node_modules")).expect("outer node_modules");
        let repo = outer.join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("git dir");
        std::fs::create_dir_all(repo.join("src")).expect("repo src");

        assert_eq!(find_project_node_modules(&repo.join("src")), None);
        let _ = std::fs::remove_dir_all(&outer);
    }

    /// With no project `node_modules`, the tsconfig is the plain form: `std/*`
    /// resolves to the bundled runtime and nothing else is wired into `paths`.
    /// This is byte-for-byte what every release before installed-package
    /// resolution emitted, so a dependency-free project is unaffected.
    #[test]
    fn tsconfig_without_node_modules_only_maps_std() {
        let ts = tsconfig_json(None, false, &[]);
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
        let ts = tsconfig_json(Some(nm), true, &[]);
        assert!(ts.contains(r#""types": ["node"],"#), "got: {ts}");
        assert!(ts.contains(r#""typeRoots": ["/proj/node_modules/@types"]"#), "got: {ts}");
    }

    /// With a project `node_modules`, a `"*"` entry points bare imports at the
    /// package's `@types` companion first and its package root second, so an
    /// installed package resolves without a hand-written stub whether its types
    /// ship in `@types/*` (tried first) or in the package itself (fallen through
    /// to). The `std/*` mapping stays, and it is more specific so `std/...` still
    /// resolves to the runtime rather than the wildcard.
    #[test]
    fn tsconfig_with_node_modules_wires_the_wildcard() {
        let nm = Path::new("/proj/node_modules");
        let ts = tsconfig_json(Some(nm), false, &[]);
        assert!(ts.contains(r#""std/*": ["./.glyph-runtime/std/*"]"#));
        assert!(ts.contains(
            r#""*": ["/proj/node_modules/@types/*", "/proj/node_modules/*"]"#
        ));
    }

    /// A Windows-style path with backslashes must stay valid JSON, so each
    /// backslash is doubled in the emitted config.
    #[test]
    fn tsconfig_escapes_backslashes_in_the_path() {
        let nm = Path::new(r"C:\proj\node_modules");
        let ts = tsconfig_json(Some(nm), false, &[]);
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
        super::exported_items(ts).into_iter().map(|e| e.name).collect()
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

    /// The other direction: every name a bundled `std/*` runtime exports must be
    /// in that module's stub, or `import std/json { parse_with }` is E0105 on a
    /// function that exists and works. That was true of `json.parse_with` and
    /// `fs.FsError` until this test was written. `std/schema` is deliberately
    /// unseeded: its `schema` export is the descriptor builder the emitter calls,
    /// not a module anyone imports by hand.
    #[test]
    fn the_bundled_runtime_exports_nothing_the_stubs_lack() {
        const UNSEEDED: [&str; 1] = ["std/schema"];
        let stubs = StdlibStubs::new();
        let seeded: BTreeMap<&str, BTreeSet<String>> = stubs
            .iter()
            .map(|(path, exports)| {
                (path, exports.names.iter().map(|n| n.to_string()).collect())
            })
            .collect();
        let mut unadvertised: Vec<String> = Vec::new();
        for (rel, src) in RUNTIME_FILES {
            let Some(stem) = rel
                .strip_prefix(".glyph-runtime/std/")
                .and_then(|f| f.strip_suffix(".ts"))
            else {
                continue;
            };
            let path = format!("std/{stem}");
            if UNSEEDED.contains(&path.as_str()) {
                continue;
            }
            let Some(names) = seeded.get(path.as_str()) else {
                unadvertised.push(format!("{path}: bundled runtime .ts with no stub entry"));
                continue;
            };
            for name in exported_names(src) {
                if !names.contains(&name) {
                    unadvertised.push(format!(
                        "{path}: runtime exports `{name}`, the stub does not advertise it"
                    ));
                }
            }
        }
        assert!(
            unadvertised.is_empty(),
            "runtime exports the stubs never learned about (a named import of one is E0105):\n{}\n\
             seed it in StdlibStubs::new, or add it to UNSEEDED if it is emitter-internal.",
            unadvertised.join("\n")
        );
    }

    /// `reachable_std_modules` closes over an emitted program's `std/*` imports:
    /// a program that imports only `std/array` also reaches `std/option`
    /// (`array.ts` imports it internally) and `std/result` (always, via the
    /// bootstrap every emitted module carries), but nothing else — least of
    /// all a Node-only module the program never touched (G115: an unfiltered
    /// bundle is exactly what breaks a browser deploy target).
    #[test]
    fn reachable_std_modules_closes_over_sibling_imports_but_stays_narrow() {
        let out = tmp_tree("reachable");
        std::fs::write(
            out.join("main.ts"),
            "import \"./.glyph-runtime/glyph-bootstrap\";\n\n\
             import * as array from \"./.glyph-runtime/std/array\";\n\n\
             export function main() {\n  array.map([1], (x) => x);\n}\n",
        )
        .expect("write fake emitted module");

        let src = tmp_tree("reachable_src");
        let reached = reachable_std_modules(&out, &src, &["main.ts".to_string()]);
        for want in ["array", "option", "result"] {
            assert!(reached.contains(want), "expected {want} reachable, got {reached:?}");
        }
        for unwanted in ["dns", "fs", "http", "net", "process", "sqlite", "tls", "schema", "json"] {
            assert!(
                !reached.contains(unwanted),
                "{unwanted} should not be reachable from an array-only import, got {reached:?}"
            );
        }
    }

    /// A program with no `std/*` import at all still reaches `result` (and
    /// nothing else): the bootstrap every emitted module carries imports it
    /// unconditionally for its `Result` wrapping.
    #[test]
    fn reachable_std_modules_with_no_program_imports_still_reaches_result() {
        let out = tmp_tree("reachable_empty");
        std::fs::write(
            out.join("main.ts"),
            "import \"./.glyph-runtime/glyph-bootstrap\";\n\nexport function main() {}\n",
        )
        .expect("write fake emitted module");

        let src = tmp_tree("reachable_empty_src");
        let reached = reachable_std_modules(&out, &src, &["main.ts".to_string()]);
        assert_eq!(
            reached,
            std::iter::once("result".to_string()).collect::<std::collections::HashSet<_>>()
        );
    }

    /// A hand-written `<src>/extern/*.ts` shim is type-checked alongside the
    /// emitted program and can import the stdlib either way the tsconfig allows,
    /// so both spellings hold their module (and whatever it imports) against the
    /// prune. A path that merely ends in `std/` is not one of them.
    #[test]
    fn reachable_std_modules_counts_both_spellings_an_extern_shim_can_use() {
        let out = tmp_tree("reachable_extern_out");
        std::fs::write(
            out.join("main.ts"),
            "import \"./.glyph-runtime/glyph-bootstrap\";\n\nexport function main() {}\n",
        )
        .expect("write fake emitted module");

        let src = tmp_tree("reachable_extern_src");
        std::fs::create_dir_all(src.join("extern/nested")).expect("mkdir extern");
        std::fs::write(
            src.join("extern/clock.ts"),
            "import * as time from \"std/time\";\nexport const t = time.now();\n",
        )
        .expect("write bare-spelling shim");
        std::fs::write(
            src.join("extern/nested/measure.tsx"),
            "import * as math from \"../../.glyph-runtime/std/math\";\nexport const h = math.floor(1);\n",
        )
        .expect("write relative-spelling shim");
        std::fs::write(
            src.join("extern/decoy.ts"),
            "import * as x from \"./mystd/http\";\nexport const y = x;\n",
        )
        .expect("write decoy shim");

        let reached = reachable_std_modules(&out, &src, &["main.ts".to_string()]);
        // `time` and `math` came from the shims; `option` is `time.ts`'s own
        // import, so the closure ran over the extern hits as well.
        for want in ["time", "math", "option", "result"] {
            assert!(reached.contains(want), "expected {want} reachable, got {reached:?}");
        }
        for unwanted in ["http", "dns", "sqlite", "tls", "schema"] {
            assert!(
                !reached.contains(unwanted),
                "{unwanted} should not be reachable, got {reached:?}"
            );
        }
    }
}
