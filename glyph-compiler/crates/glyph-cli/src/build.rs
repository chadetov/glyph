//! `glyph build` — walk a source tree, register every `.glyph` file on a
//! salsa-backed `CompilerDb`, run the analysis pipeline (parse, collect,
//! verify-imports, resolve, type_map) for each, and report diagnostics.
//!
//! TS emission is still phase 1 week 4 work, so the `--out` directory is
//! created if missing but no files are written yet. The command's purpose
//! today is "check the source tree" — analogous to `cargo check`.
//!
//! The function is split out into a library entry point so integration
//! tests can call it directly (no subprocess) and assert on the returned
//! `BuildReport`.

use std::path::{Path, PathBuf};

use glyph_db::{
    import_diagnostics, module_symbols, parse_module, resolve, type_map, CompilerDb, SourceFile,
};

use crate::render::{
    render_emit_error, render_parse_error, render_resolve_error, render_type_error,
};

/// Outcome of a build. Carries the rendered diagnostic strings so the
/// binary can print them and the integration tests can assert on them.
#[derive(Debug, Default)]
pub struct BuildReport {
    /// One entry per diagnostic. Each entry is a pre-rendered string
    /// like `lib/foo.glyph: collect: name 'dup' declared more than once`.
    pub diagnostics: Vec<String>,
    /// Module paths the build saw, in deterministic (lexicographic) order.
    pub modules: Vec<String>,
    /// Relative paths of the `.ts` files written to the out directory, in the
    /// same order. A module is emitted only when it produced no diagnostics.
    pub emitted: Vec<String>,
}

impl BuildReport {
    /// True if any diagnostic was emitted.
    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
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

    // Prepare the out/ directory. Today it's just created; future work
    // writes emitted .ts files into it. Reject the case where `out` is
    // an existing regular file — current code wouldn't notice, but
    // week-4 TS emission would fail with a confusing IO error.
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

    // Run the pipeline for each file. Collect diagnostics in the same
    // order as the file walk so the report is reproducible. Each
    // diagnostic is ariadne-rendered against the file's source so the
    // output includes the failing line + a caret pointer.
    let mut report = BuildReport::default();
    for (module_path, sf) in &entries {
        report.modules.push(module_path.clone());
        let diag_start = report.diagnostics.len();

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
        }

        let diags = import_diagnostics(&db, *sf);
        for e in diags.errors() {
            report.diagnostics.push(render_resolve_error(
                module_path,
                &source,
                e,
                with_color,
            ));
        }

        let r = resolve(&db, *sf);
        for e in r.errors() {
            report.diagnostics.push(render_resolve_error(
                module_path,
                &source,
                e,
                with_color,
            ));
        }

        // Day-14: type_map.errors() carries non-exhaustive match
        // diagnostics. Future week-3 days add `?` mismatches, owned
        // single-consumption violations, and the bidirectional
        // checker's type errors.
        let types = type_map(&db, *sf);
        for e in types.errors() {
            report.diagnostics.push(render_type_error(
                module_path,
                &source,
                e,
                with_color,
            ));
        }

        // Emit TS only for a module that produced no diagnostics — never
        // write code derived from a program the compiler rejected.
        if report.diagnostics.len() != diag_start {
            continue;
        }
        let Some(ast) = parsed.module() else { continue };
        match glyph_emit::emit_module(ast) {
            Ok(ts) => {
                let rel = format!("{module_path}.ts");
                let ts_path = out.join(&rel);
                if let Some(parent) = ts_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| BuildError::Io {
                        path: parent.to_path_buf(),
                        source: e,
                    })?;
                }
                std::fs::write(&ts_path, ts).map_err(|e| BuildError::Io {
                    path: ts_path.clone(),
                    source: e,
                })?;
                report.emitted.push(rel);
            }
            Err(e) => {
                report
                    .diagnostics
                    .push(render_emit_error(module_path, &source, &e, with_color));
            }
        }
    }

    Ok(report)
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
}
