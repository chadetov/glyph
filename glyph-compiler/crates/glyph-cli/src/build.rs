//! `glyph build` — walk a source tree, register every `.glyph` file on a
//! salsa-backed `CompilerDb`, run the analysis pipeline (parse, collect,
//! verify-imports, resolve, type_map) for each, and report diagnostics.
//!
//! After a module type-checks cleanly, its TypeScript is emitted to a
//! `.ts` file under the `--out` directory; a module that produced any
//! diagnostic (or uses a construct the emitter does not support yet) is
//! reported and not written.
//!
//! The function is split out into a library entry point so integration
//! tests can call it directly (no subprocess) and assert on the returned
//! `BuildReport`.

use std::path::{Path, PathBuf};

use glyph_db::{
    import_diagnostics, module_symbols, parse_module, resolve, type_map, CompilerDb, Db, SourceFile,
};

use crate::render::{
    render_emit_error, render_parse_error, render_resolve_error, render_type_error,
};

/// Outcome of a build. Carries the rendered diagnostic strings so the
/// binary can print them and the integration tests can assert on them.
#[derive(Debug, Default)]
pub struct BuildReport {
    /// One entry per diagnostic (errors and warnings), pre-rendered.
    pub diagnostics: Vec<String>,
    /// The same diagnostics in structured form (for `--json`), in the same
    /// order as `diagnostics`.
    pub structured: Vec<crate::diagnostic::Diagnostic>,
    /// How many of `diagnostics` are error-severity (the rest are warnings).
    /// Only errors fail the build or block a module's emission.
    pub error_count: usize,
    /// Module paths the build saw, in deterministic (lexicographic) order.
    pub modules: Vec<String>,
    /// Relative paths of the `.ts` files written to the out directory, in the
    /// same order. A module is emitted only when it produced no *errors*
    /// (warnings do not block emission).
    pub emitted: Vec<String>,
    /// Per-module source maps, so `tsc` diagnostics can be remapped onto Glyph
    /// source (see `tscmap`). One entry per emitted module.
    pub module_maps: Vec<crate::tscmap::ModuleMap>,
}

impl BuildReport {
    /// True if any error-severity diagnostic was emitted. Warnings alone do not
    /// make a build fail.
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    /// How many diagnostics are warnings (surfaced but non-fatal).
    pub fn warning_count(&self) -> usize {
        self.diagnostics.len() - self.error_count
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("source directory does not exist: {0}")]
    SrcMissing(PathBuf),
    #[error("source path is not a directory: {0}")]
    SrcNotDir(PathBuf),
    #[error("output path exists but is not a directory: {0}")]
    OutNotDir(PathBuf),
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("source directory contains no `.glyph` files: {0}")]
    NoSources(PathBuf),
}

/// Walk `src/`, register every `.glyph` file on a fresh `CompilerDb`, run
/// the analysis pipeline, and return a report of any diagnostics. Creates
/// the `out/` directory if it doesn't exist (TS emission lands later).
///
/// Diagnostics are ariadne-rendered with color enabled (multi-line, with
/// caret pointers). For testing or non-TTY output, call
/// `build_project_inner` directly with `with_color: false` — the public
/// entry preserves the current binary behavior.
pub fn build_project(src: &Path, out: &Path) -> Result<BuildReport, BuildError> {
    build_project_inner(src, out, true)
}

/// Internal entry point that lets tests opt out of ANSI color so
/// rendered diagnostics are stable text. Production callers go through
/// `build_project` which leaves color on.
pub fn build_project_inner(
    src: &Path,
    out: &Path,
    with_color: bool,
) -> Result<BuildReport, BuildError> {
    if !src.exists() {
        return Err(BuildError::SrcMissing(src.to_path_buf()));
    }
    if !src.is_dir() {
        return Err(BuildError::SrcNotDir(src.to_path_buf()));
    }

    // Walk for .glyph files. Deterministic order so diagnostic output is
    // stable across runs (good for tests, good for human readability).
    let mut files = Vec::new();
    walk_glyph_files(src, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(BuildError::NoSources(src.to_path_buf()));
    }

    // Prepare the out/ directory; emitted `.ts` files are written into it
    // below. Reject the case where `out` is an existing regular file so the
    // per-module write fails up front rather than with a confusing IO error.
    if out.exists() {
        if !out.is_dir() {
            return Err(BuildError::OutNotDir(out.to_path_buf()));
        }
    } else {
        std::fs::create_dir_all(out).map_err(|e| BuildError::Io {
            path: out.to_path_buf(),
            source: e,
        })?;
    }

    // Build the db and register every file.
    let mut db = CompilerDb::with_default_stdlib();
    let mut entries: Vec<(String, SourceFile)> = Vec::with_capacity(files.len());
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        let module_path = derive_module_path(src, path);
        let virtual_path = path.to_string_lossy().into_owned();
        let sf = SourceFile::new(&db, virtual_path, text);
        entries.push((module_path, sf));
    }
    db.set_project(entries.clone());

    // The set of project module paths lets the emitter tell a sibling import
    // (which needs a relative specifier) from a `std/*` or external one.
    let project_modules: std::collections::BTreeSet<String> =
        entries.iter().map(|(p, _)| p.clone()).collect();

    // Run the pipeline for each file. Collect diagnostics in the same
    // order as the file walk so the report is reproducible. Each
    // diagnostic is ariadne-rendered against the file's source so the
    // output includes the failing line + a caret pointer.
    let mut report = BuildReport::default();
    for (module_path, sf) in &entries {
        report.modules.push(module_path.clone());
        let err_start = report.error_count;

        // Cache the source text once per file; rendering may use it
        // multiple times if the file produces multiple diagnostics.
        let source = sf.text(&db).clone();

        let parsed = parse_module(&db, *sf);
        if let Some(err) = parsed.error() {
            report.diagnostics.push(render_parse_error(
                module_path,
                &source,
                err,
                with_color,
            ));
            report
                .structured
                .push(crate::diagnostic::from_parse_error(module_path, &source, err));
            report.error_count += 1;
            // Downstream queries gracefully degrade on parse failure;
            // their results are necessarily empty. Skip them so the
            // report doesn't pile up redundant cascade-errors.
            continue;
        }

        let syms = module_symbols(&db, *sf);
        for e in syms.errors() {
            report.diagnostics.push(render_resolve_error(
                module_path,
                &source,
                e,
                with_color,
            ));
            report.structured.push(crate::diagnostic::from_resolve_error(
                module_path,
                &source,
                e,
                crate::render::stage_label_for(e),
            ));
            report.error_count += 1;
        }

        let diags = import_diagnostics(&db, *sf);
        for e in diags.errors() {
            report.diagnostics.push(render_resolve_error(
                module_path,
                &source,
                e,
                with_color,
            ));
            report.structured.push(crate::diagnostic::from_resolve_error(
                module_path,
                &source,
                e,
                crate::render::stage_label_for(e),
            ));
            report.error_count += 1;
        }

        let r = resolve(&db, *sf);
        for e in r.errors() {
            report.diagnostics.push(render_resolve_error(
                module_path,
                &source,
                e,
                with_color,
            ));
            report.structured.push(crate::diagnostic::from_resolve_error(
                module_path,
                &source,
                e,
                crate::render::stage_label_for(e),
            ));
            report.error_count += 1;
        }

        // Typecheck diagnostics carry a severity: errors fail the build, the
        // `Result` must-use lint (E0217) is a warning that is surfaced but does
        // not block emission.
        let types = type_map(&db, *sf);
        for e in types.errors() {
            report.diagnostics.push(render_type_error(
                module_path,
                &source,
                e,
                with_color,
            ));
            report
                .structured
                .push(crate::diagnostic::from_type_error(module_path, &source, e));
            if e.severity() == glyph_typechecker::Severity::Error {
                report.error_count += 1;
            }
        }

        // Emit TS only for a module that produced no *errors* — never write
        // code derived from a rejected program. Warnings do not block emission.
        if report.error_count != err_start {
            continue;
        }
        let Some(ast) = parsed.module() else { continue };
        let Some(resolved) = r.resolved() else { continue };

        // Advisory lints (unused import/binding, unreachable code): warnings
        // that surface but never block emission. Computed only here, on a
        // module that resolved with no errors, so the resolution map is
        // complete and a used binding can't be mistaken for a dead one.
        for e in glyph_resolver::module_lints(ast, resolved) {
            report
                .diagnostics
                .push(render_resolve_error(module_path, &source, &e, with_color));
            report.structured.push(crate::diagnostic::from_resolve_error(
                module_path,
                &source,
                &e,
                crate::render::stage_label_for(&e),
            ));
        }

        let ctx = glyph_emit::EmitContext {
            module_path: module_path.as_str(),
            project_modules: &project_modules,
        };
        match glyph_emit::emit_module_mapped(ast, resolved, types.type_map(), db.prelude(), ctx) {
            Ok(output) => {
                let rel = format!("{module_path}.ts");
                let ts_path = out.join(&rel);
                if let Some(parent) = ts_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| BuildError::Io {
                        path: parent.to_path_buf(),
                        source: e,
                    })?;
                }
                // Runtime source map (v3): map the emitted `.ts` back to the
                // `.glyph`. Written as a sidecar `.ts.map` with a trailing
                // `sourceMappingURL` comment on the `.ts`. The comment is appended
                // last, so it shifts no existing offset (the tscmap offset math
                // stays correct against `output.ts` without the comment).
                let ts_basename = file_basename(&rel);
                let map_rel = format!("{rel}.map");
                let map_basename = file_basename(&map_rel);
                let glyph_rel = format!("{module_path}.glyph");
                let map_json = crate::sourcemap::build_v3_map(
                    &output.ts,
                    &source,
                    &glyph_rel,
                    &ts_basename,
                    &output.source_map,
                );
                std::fs::write(out.join(&map_rel), &map_json).map_err(|e| BuildError::Io {
                    path: out.join(&map_rel),
                    source: e,
                })?;
                let ts_with_map =
                    format!("{}\n//# sourceMappingURL={}\n", output.ts, map_basename);
                std::fs::write(&ts_path, ts_with_map).map_err(|e| BuildError::Io {
                    path: ts_path.clone(),
                    source: e,
                })?;
                report.emitted.push(rel.clone());
                report.module_maps.push(crate::tscmap::ModuleMap {
                    ts_rel: rel,
                    glyph_path: module_path.clone(),
                    glyph_source: source.clone(),
                    ts_source: output.ts,
                    source_map: output.source_map,
                });
            }
            Err(e) => {
                report
                    .diagnostics
                    .push(render_emit_error(module_path, &source, &e, with_color));
                report
                    .structured
                    .push(crate::diagnostic::from_emit_error(module_path, &source, &e));
                report.error_count += 1;
            }
        }
    }

    // Write the bundled runtime + a generated `tsconfig.json` next to the
    // emitted output (plus any `<src>/.types/` ambient declarations) so
    // `tsc -p <out>/tsconfig.json` can type-check the result. Skip it when
    // nothing emitted — there is no output to check.
    if !report.emitted.is_empty() {
        // Prune project `.ts` left from a previous build of the same out dir: a
        // module renamed or removed from the source would otherwise leave a
        // stale `.ts` that `tsc` and sibling imports still pick up (G17). Only
        // emitted module files are pruned — the bundled `.glyph-runtime/` tree,
        // any `.d.ts`, and the `glyph run` entrypoint are left untouched.
        prune_stale_outputs(out, &report.emitted)?;
        crate::runtime::write_build_support(out, src).map_err(|e| BuildError::Io {
            path: out.to_path_buf(),
            source: e,
        })?;
    }

    Ok(report)
}

/// Delete emitted-module `.ts` files (and their `.ts.map` source-map sidecars)
/// in `out` that are not in `kept` (the set of rel paths emitted by this build),
/// so a removed/renamed module does not leave a stale file behind. Recurses the
/// out tree but skips dot-directories (the bundled `.glyph-runtime/`,
/// `.types/`); never touches `.d.ts`, the `glyph run` entrypoint, or any
/// non-emitted file the user placed alongside the output.
fn prune_stale_outputs(out: &Path, kept: &[String]) -> Result<(), BuildError> {
    let kept: std::collections::HashSet<&str> = kept.iter().map(String::as_str).collect();
    let mut found = Vec::new();
    collect_ts_outputs(out, out, &mut found)?;
    for (abs, rel) in found {
        // A `.ts` is kept iff this build emitted it; a `.ts.map` sidecar is kept
        // iff its `.ts` is (a removed module must not orphan its source map).
        let stale = match rel.strip_suffix(".map") {
            Some(ts_rel) => !kept.contains(ts_rel),
            None => !kept.contains(rel.as_str()),
        };
        if stale {
            let _ = std::fs::remove_file(&abs);
        }
    }
    Ok(())
}

fn collect_ts_outputs(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, String)>,
) -> Result<(), BuildError> {
    let entries = std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if meta.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            collect_ts_outputs(root, &path, out)?;
        } else if meta.is_file()
            && (name.ends_with(".ts") || name.ends_with(".ts.map"))
            && !name.ends_with(".d.ts")
            && name != "__glyph_run.ts"
        {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((path.clone(), rel));
        }
    }
    Ok(())
}

/// Recursive walker for `.glyph` files. Skips directories whose names
/// start with `.` (so `.git`, `.cache`, etc. are ignored) and the
/// conventional `target/` Cargo output directory.
///
/// Uses `symlink_metadata` (NOT `is_dir`, which follows symlinks) so a
/// cyclic symlink like `src/foo -> ../src` doesn't trigger unbounded
/// recursion. Symlinks of any kind are skipped — Phase 5's package
/// metadata work can decide whether to follow them once the project
/// graph has explicit boundary information.
/// A content fingerprint of every `.glyph` source under `src`, plus the running
/// compiler binary's mtime. `glyph run` keys its build cache on this so an
/// unchanged program reuses the previous build (and its `tsc` result) instead of
/// rebuilding and re-type-checking on every invocation; the binary mtime busts
/// the cache when the compiler itself changes.
pub fn source_fingerprint(src: &Path) -> Result<String, BuildError> {
    use std::hash::{Hash, Hasher};
    let mut files = Vec::new();
    walk_glyph_files(src, &mut files)?;
    files.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    // Bust the cache when the compiler binary is rebuilt (its emitted output may
    // change even when sources do not).
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(meta) = std::fs::metadata(&exe) {
            if let Ok(mtime) = meta.modified() {
                if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    d.as_nanos().hash(&mut hasher);
                }
            }
        }
    }
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        let rel = path.strip_prefix(src).unwrap_or(path);
        rel.to_string_lossy().hash(&mut hasher);
        text.hash(&mut hasher);
    }
    // `<src>/.types/**/*.d.ts` ambient declarations are build inputs too — they
    // are copied into the out dir and type-checked — so a change to them must
    // bust the cache. `walk_glyph_files` skips dot-directories and non-`.glyph`
    // files, so collect them separately.
    let types_dir = src.join(".types");
    if types_dir.is_dir() {
        let mut dts = Vec::new();
        collect_dts_files(&types_dir, &mut dts)?;
        dts.sort();
        for path in &dts {
            let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
                path: path.clone(),
                source: e,
            })?;
            let rel = path.strip_prefix(src).unwrap_or(path);
            rel.to_string_lossy().hash(&mut hasher);
            text.hash(&mut hasher);
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

/// Collect every `.d.ts` file under `dir` (recursively) into `out`.
fn collect_dts_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BuildError> {
    for entry in std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })? {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        if meta.is_dir() {
            collect_dts_files(&path, out)?;
        } else if meta.is_file()
            && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".d.ts"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn walk_glyph_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BuildError> {
    let entries = std::fs::read_dir(dir).map_err(|e| BuildError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| BuildError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| BuildError::Io {
            path: path.clone(),
            source: e,
        })?;
        // `file_type()` from the DirEntry's own metadata uses
        // `symlink_metadata` semantics — a symlink reports as a symlink,
        // not as the target's kind. Skip symlinks entirely.
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            walk_glyph_files(&path, out)?;
        } else if meta.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("glyph")
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Turn `src/foo/bar.glyph` (under root `src/`) into the module path
/// string `"foo/bar"`. Strips the `src` prefix, drops the `.glyph`
/// extension, and replaces native path separators with `/` so the result
/// matches what `import foo/bar` produces during parsing.
fn derive_module_path(src_root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(src_root).unwrap_or(file);
    let no_ext = rel.with_extension("");
    no_ext.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_module_path_drops_extension_and_normalizes_separators() {
        let src = Path::new("/tmp/proj/src");
        let file = Path::new("/tmp/proj/src/app/users.glyph");
        assert_eq!(derive_module_path(src, file), "app/users");
    }

    #[test]
    fn derive_module_path_handles_top_level_file() {
        let src = Path::new("/tmp/proj/src");
        let file = Path::new("/tmp/proj/src/main.glyph");
        assert_eq!(derive_module_path(src, file), "main");
    }

    #[test]
    fn fingerprint_changes_when_types_dts_changes() {
        // A change to `<src>/.types/*.d.ts` (a real build input) must bust the
        // `glyph run` cache fingerprint, not just a change to a `.glyph` file.
        let root = std::env::temp_dir().join(format!("glyph-fp-test-{}", std::process::id()));
        let types = root.join(".types");
        std::fs::create_dir_all(&types).unwrap();
        std::fs::write(root.join("main.glyph"), "module main\n").unwrap();
        std::fs::write(types.join("ext.d.ts"), "declare module \"ext\" { export const v: number; }\n").unwrap();
        let fp1 = source_fingerprint(&root).unwrap();
        // Same .glyph, changed ambient declaration.
        std::fs::write(types.join("ext.d.ts"), "declare module \"ext\" { export const v: string; }\n").unwrap();
        let fp2 = source_fingerprint(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(fp1, fp2, "fingerprint must change when a .types/*.d.ts changes");
    }
}

/// The last path component of a `/`-separated relative path (`sub/mod.ts` ->
/// `mod.ts`). Emitted module rels always use `/`.
fn file_basename(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}
