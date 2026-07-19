//! `glyph run` — build a single Glyph program and execute it.
//!
//! The program's directory is built into a fresh temporary directory (emitted
//! TypeScript + the bundled `std/*` runtime + a generated `tsconfig.json`), a
//! small entrypoint is generated, and `tsx` runs it. `tsx` is pointed at the
//! generated tsconfig (`--tsconfig`) so the `std/*` path aliases resolve, while
//! the process keeps the caller's working directory so the program's own
//! relative paths (a config file, say) resolve where the user invoked it.
//!
//! The program's `main(argv) -> number` entry is called with the trailing CLI
//! arguments; its return value becomes the process exit code. A program that
//! returns `void` (or nothing) exits 0.

use std::path::Path;
use std::process::Command;

use crate::build::{build_project_inner, BuildError, BuildReport};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("run target does not exist: {0}")]
    FileMissing(std::path::PathBuf),
    #[error("run target is not a `.glyph` file: {0}")]
    NotGlyph(std::path::PathBuf),
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error("io error preparing the run directory: {0}")]
    Io(#[from] std::io::Error),
}

/// The result of attempting to run a program.
pub enum RunOutcome {
    /// The program ran; carries its process exit code.
    Ran(i32),
    /// The build did not produce the target module (it or a dependency was
    /// rejected). The report's diagnostics explain why; nothing ran.
    BuildFailed(BuildReport),
    /// Type-checking the emitted output with `tsc` reported errors; carries the
    /// `tsc` output. Nothing ran — the type errors are surfaced instead of
    /// becoming runtime crashes.
    TypeCheckFailed(String),
    /// `tsx` was not found on `PATH`.
    TsxNotFound,
}

/// Build `file`'s directory and run the program's `main` with `args`.
///
/// The build covers the whole directory containing `file` (so sibling modules
/// and a `.types/` directory resolve); the program runs only if `file`'s own
/// module emitted cleanly. Sibling modules that failed to compile are not a
/// hard error here — they simply are not available to import at run time.
///
/// When `check` is set (the default), the emitted output is type-checked with
/// `tsc` before running: type errors are reported and nothing runs, so they
/// surface as diagnostics rather than runtime crashes. If `tsc` is not on
/// `PATH` a warning is printed and the program runs anyway.
pub fn run_file(
    file: &Path,
    args: &[String],
    with_color: bool,
    check: bool,
) -> Result<RunOutcome, RunError> {
    if !file.exists() {
        return Err(RunError::FileMissing(file.to_path_buf()));
    }
    if file.extension().and_then(|e| e.to_str()) != Some("glyph") {
        return Err(RunError::NotGlyph(file.to_path_buf()));
    }

    // `src` is the program's directory; with `src` as the root the target's
    // module path is just its file stem, which names its emitted `.ts`.
    let src = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("main");

    let target_rel = format!("{stem}.ts");

    // Cache the build so a repeated `glyph run` of an unchanged program skips
    // both the rebuild and the `tsc` type-check (the dominant per-invocation
    // cost). The cache directory is keyed by a fingerprint of the sources plus
    // the compiler binary, so any source or compiler change rebuilds. Without a
    // fingerprint (e.g. an unreadable source tree) fall back to a fresh
    // pid-scoped dir, the previous always-rebuild behavior.
    let fingerprint = crate::build::source_fingerprint(src).ok();
    let out = match &fingerprint {
        Some(fp) => std::env::temp_dir().join("glyph-run-cache").join(fp),
        None => std::env::temp_dir().join(format!("glyph-run-{stem}-{}", std::process::id())),
    };
    let tsc_marker = out.join(".glyph-tsc-ok");
    // Cache validity is signalled by a marker written only after a build runs to
    // completion — not by the target `.ts` merely existing, so a build that
    // errored after writing the target (or a partially-deleted dir) is not
    // mistaken for a hit.
    let build_marker = out.join(".glyph-build-ok");
    let build_cached = fingerprint.is_some() && build_marker.exists();

    // Source maps from a fresh build, so a `tsc` failure below remaps onto Glyph
    // source. Empty for a cached build (the maps aren't rebuilt), in which case
    // any `tsc` output passes through unmapped.
    let mut module_maps: Vec<crate::tscmap::ModuleMap> = Vec::new();
    if !build_cached {
        // Fresh build: clean the dir first so no stale emitted or runtime file
        // lingers (a failed earlier build, a renamed module).
        if out.exists() {
            std::fs::remove_dir_all(&out)?;
        }
        let report = build_project_inner(src, &out, with_color)?;
        if !report.emitted.iter().any(|e| e == &target_rel) {
            return Ok(RunOutcome::BuildFailed(report));
        }
        module_maps = report.module_maps;
        // The fresh output has not been type-checked yet; mark the build complete.
        let _ = std::fs::remove_file(&tsc_marker);
        let _ = std::fs::write(&build_marker, b"");
    }

    // Type-check before running so type errors surface as diagnostics rather
    // than runtime crashes. Skip it when a cached build already passed `tsc`
    // (recorded by the marker). A missing `tsc` is a warning, not a hard stop —
    // `tsx` can still run the program.
    if check && !(build_cached && tsc_marker.exists()) {
        use crate::runtime::TscOutcome;
        match crate::runtime::check_with_tsc(&out)? {
            TscOutcome::Passed => {
                let _ = std::fs::write(&tsc_marker, b"");
            }
            TscOutcome::Failed(msg) => {
                let remapped = crate::tscmap::remap_tsc_output(&msg, &module_maps, with_color);
                return Ok(RunOutcome::TypeCheckFailed(remapped));
            }
            TscOutcome::NotFound => {
                eprintln!(
                    "glyph run: tsc not found on PATH; running without a type check. \
                     Install TypeScript (`npm install -g typescript`) or pass --no-check."
                );
            }
        }
    }

    let entry = out.join("__glyph_run.ts");
    std::fs::write(&entry, entrypoint_source(stem))?;

    // Run from the caller's cwd (program-relative paths resolve there), but
    // point `tsx` at the generated tsconfig so `std/*` resolves.
    let tsconfig = out.join("tsconfig.json");
    match Command::new("tsx")
        .arg("--tsconfig")
        .arg(&tsconfig)
        .arg(&entry)
        .args(args)
        .status()
    {
        Ok(status) => Ok(RunOutcome::Ran(status.code().unwrap_or(1))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RunOutcome::TsxNotFound),
        Err(e) => Err(RunError::Io(e)),
    }
}

/// The generated entrypoint: install the prelude globals (side-effect import),
/// then call the program's `main` with the trailing argv and exit with its
/// numeric return. An async IIFE rather than top-level `await` so it runs under
/// either CommonJS or ESM resolution. Relative imports are resolved against the
/// entrypoint's own location, so the absolute path passed to `tsx` works
/// regardless of the caller's working directory.
fn entrypoint_source(stem: &str) -> String {
    format!(
        "import \"./.glyph-runtime/glyph-bootstrap.ts\";\n\
         import {{ main }} from \"./{stem}.ts\";\n\
         (async () => {{\n\
         \x20 try {{\n\
         \x20   const code = await main(process.argv.slice(2));\n\
         \x20   process.exit(typeof code === \"number\" ? code : 0);\n\
         \x20 }} catch (e) {{\n\
         \x20   console.error(e);\n\
         \x20   process.exit(1);\n\
         \x20 }}\n\
         }})();\n"
    )
}
