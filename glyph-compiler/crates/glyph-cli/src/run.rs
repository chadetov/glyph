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
//! returns `void` (or nothing) leaves the code where it stands, so a code
//! recorded with `std/process.set_exit_code` during the run survives; with
//! nothing recorded that is 0. The code is assigned rather than forced,
//! so a program that started a server or scheduled a timer keeps running until
//! that work is done instead of being killed the moment `main` returns; see
//! `entrypoint_source`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;

use serde::{Deserialize, Serialize};

use crate::build::{build_project_inner, BuildError, BuildReport};

/// Name of the diagnostics sidecar written into the run cache directory.
///
/// A build's diagnostics are cached next to the output they came from, so a
/// warm-cache run reports exactly what the cold run reported. Without it the
/// second `glyph run` of unchanged source would print nothing, and a warning
/// that appears once and then stops is read as a warning that went away.
const DIAGNOSTICS_CACHE: &str = ".glyph-diagnostics.json";

/// Per-process counter making each run's staging directory unique even across
/// threads (the pid alone repeats — the concurrent-run test shares one process).
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// `remove_dir_all` that tolerates the concurrent-run race: a missing directory
/// is success (someone already removed it), and a transient failure while
/// another process is writing into the same path (`DirectoryNotEmpty`, and the
/// Windows sharing-violation equivalent) is retried briefly before giving up.
pub(crate) fn remove_dir_all_retry(path: &Path) -> std::io::Result<()> {
    use std::io::ErrorKind;
    for attempt in 0..12 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(_) if attempt < 11 => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("run target does not exist: {0}")]
    FileMissing(std::path::PathBuf),
    #[error("run target is not a `.glyph` file: {0}")]
    NotGlyph(std::path::PathBuf),
    #[error(
        "no `main.glyph` in `{}`: `glyph run <dir>` runs that directory's `main.glyph`, and \
         there is none. Name a `.glyph` file instead, or add `main.glyph`.",
        .dir.display()
    )]
    NoMainGlyph { dir: std::path::PathBuf },
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error("io error preparing the run directory: {0}")]
    Io(#[from] std::io::Error),
}

/// The result of attempting to run a program.
#[derive(Debug)]
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
    /// `tsc` was not found on `PATH` while the type check was requested (the
    /// default). Rather than run unchecked, refuse: the guarantee we advertise
    /// must not silently evaporate. The user can opt out explicitly with
    /// `--no-check`.
    TscMissing,
    /// The target module has no `fn main`, so there is nothing to run (it's a
    /// library). Carries the module's top-level function names as a hint.
    /// Reported as `E0310` and exits non-zero, instead of letting the generated
    /// entrypoint call an undefined `main` and throw a raw Node `TypeError`.
    NoMain {
        exports: Vec<String>,
        /// The module that was actually looked at, so the diagnostic names
        /// `.../main.glyph` even when the user named its directory.
        module: PathBuf,
    },
}

/// The diagnostics a build produced, rendered and structured.
///
/// Doubles as the on-disk shape of the run cache's diagnostics sidecar, so the
/// cached form and the in-memory form cannot drift apart. `structured` is the
/// same `Diagnostic` the `--json` output uses.
#[derive(Debug, Default, Serialize, Deserialize)]
struct BuildDiagnostics {
    diagnostics: Vec<String>,
    structured: Vec<crate::diagnostic::Diagnostic>,
    error_count: usize,
    /// Whether the rendered strings carry ANSI color. Cached diagnostics are
    /// reusable only by a run that renders the same way, so a redirected run
    /// never replays escape codes captured by an earlier terminal run.
    with_color: bool,
}

impl BuildDiagnostics {
    fn of(report: &BuildReport, with_color: bool) -> Self {
        BuildDiagnostics {
            diagnostics: report.diagnostics.clone(),
            structured: report.structured.clone(),
            error_count: report.error_count,
            with_color,
        }
    }

    fn into_result(self, outcome: RunOutcome) -> RunResult {
        RunResult {
            outcome,
            diagnostics: self.diagnostics,
            structured: self.structured,
            error_count: self.error_count,
        }
    }
}

/// Read the diagnostics sidecar written beside a cached build. `None` when it is
/// missing, unparseable, or rendered for a different color setting; that makes
/// the whole cache entry invalid. Rebuilding costs a second, and reporting a
/// clean tree we never checked costs the user their trust in the command.
fn read_cached_diagnostics(path: &Path, with_color: bool) -> Option<BuildDiagnostics> {
    let cached: BuildDiagnostics = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    (cached.with_color == with_color).then_some(cached)
}

/// What a run produced: the outcome, plus every diagnostic the build computed.
///
/// The diagnostics ride along with every outcome, a successful `Ran` included,
/// so `glyph run` reports what `glyph build` reports on the same tree.
/// Rendering is the caller's job; `run_file` never prints.
#[derive(Debug)]
pub struct RunResult {
    pub outcome: RunOutcome,
    /// One entry per diagnostic (errors and warnings), pre-rendered.
    pub diagnostics: Vec<String>,
    /// The same diagnostics in structured form, in the same order.
    pub structured: Vec<crate::diagnostic::Diagnostic>,
    /// How many of `diagnostics` are error-severity (the rest are warnings).
    pub error_count: usize,
}

/// Build `file`'s directory and run the program's `main` with `args`.
///
/// The build covers the whole directory containing `file` (so sibling modules
/// and a `.types/` directory resolve); the program runs only if `file`'s own
/// module emitted cleanly. Sibling modules that failed to compile are not a
/// hard error here — they simply are not available to import at run time. Their
/// diagnostics are still reported: every diagnostic from the whole tree comes
/// back on `RunResult`, on the successful path as well as the failing ones, so
/// declining to block the run never means declining to tell the user.
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
) -> Result<RunResult, RunError> {
    if !file.exists() {
        return Err(RunError::FileMissing(file.to_path_buf()));
    }
    // A directory runs its `main.glyph`, matching `glyph build <dir>`: the two
    // commands a multi-module app needs are spelled the same way.
    let target: PathBuf = if file.is_dir() {
        // A *project* directory runs the `main.glyph` at its resolution root
        // (D41): `glyph run app` finds `app/src/main.glyph` when `app` carries a
        // marker. A directory with no marker is not a project and gets no `src/`
        // convention applied to it, so it still runs its own `main.glyph` —
        // which is what `glyph build <dir>` roots at, and the two commands must
        // agree.
        let root_main = crate::config::project_src(file).map(|src| src.join("main.glyph"));
        let main = match root_main {
            Some(m) if m.is_file() => m,
            _ => file.join("main.glyph"),
        };
        if !main.is_file() {
            return Err(RunError::NoMainGlyph {
                dir: file.to_path_buf(),
            });
        }
        main
    } else {
        file.to_path_buf()
    };
    let file: &Path = target.as_path();
    if file.extension().and_then(|e| e.to_str()) != Some("glyph") {
        return Err(RunError::NotGlyph(file.to_path_buf()));
    }

    // `src` is the program's *project* resolution root: the nearest enclosing
    // directory carrying a `package.json` with a `"glyph"` key, else the file's
    // own directory (D41, P6). Climbing is what makes a nested entry file
    // runnable — `glyph run app/src/queries/report.glyph` resolves `catalog` the
    // same way `glyph build app` does, and it does so whether the path was typed
    // relative or absolute (`project_for_file` compares canonicalized paths).
    let placed = crate::config::project_for_file(file);
    let src: &Path = placed.src.as_path();
    let file: &Path = placed.file.as_path();
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("main");

    // The target's module path within its project names its emitted `.ts`; for a
    // file sitting directly at the root this is still just the stem.
    let target_rel = format!("{}.ts", crate::build::derive_module_path(src, file));

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
    // A cached build is usable only if its diagnostics came back with it: the
    // cache hit is what makes the report unavailable, so the report is cached
    // too. A hit whose sidecar is missing or unreadable counts as a miss.
    let cached_diags = if fingerprint.is_some() && build_marker.exists() {
        read_cached_diagnostics(&out.join(DIAGNOSTICS_CACHE), with_color)
    } else {
        None
    };
    let build_cached = cached_diags.is_some();

    // We type-check unless a cached build already passed `tsc` (its marker
    // exists). When we will type-check, the emitter's source maps are needed to
    // remap any `tsc` error onto Glyph source — so build fresh even over a cache
    // hit if that hit has no passing-tsc marker (a prior build whose tsc failed),
    // rather than pass raw `.ts` errors through with no map.
    let will_typecheck = check && !(build_cached && tsc_marker.exists());
    let do_build = !build_cached || will_typecheck;

    let mut module_maps: Vec<crate::tscmap::ModuleMap> = Vec::new();
    let mut diags = cached_diags.unwrap_or_default();
    if do_build {
        // Build into a private staging dir, then move it onto the shared,
        // fingerprint-keyed `out` path. Two concurrent `glyph run`s of the same
        // program otherwise race on cleaning and writing the one shared dir
        // (`DirectoryNotEmpty`); with per-invocation staging each build is
        // isolated and the only shared step is an atomic-ish rename.
        let n = STAGING_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let parent = out.parent().unwrap_or_else(|| Path::new("."));
        let staging = parent.join(format!(".glyph-staging-{}-{}", std::process::id(), n));
        let _ = remove_dir_all_retry(&staging);

        let report = build_project_inner(src, &staging, with_color)?;
        diags = BuildDiagnostics::of(&report, with_color);
        if !report.emitted.iter().any(|e| e == &target_rel) {
            let _ = remove_dir_all_retry(&staging);
            return Ok(diags.into_result(RunOutcome::BuildFailed(report)));
        }
        module_maps = report.module_maps;

        // Cache the diagnostics beside the output they describe. Written into
        // staging so it moves into place with the rest of the build; a dir that
        // never finished moving cannot claim a clean tree.
        if let Ok(json) = serde_json::to_string(&diags) {
            let _ = std::fs::write(staging.join(DIAGNOSTICS_CACHE), json);
        }

        // Move staging into place. If `out` reappeared meanwhile (another run of
        // the identical fingerprint built it — same content), just drop staging
        // and use theirs.
        remove_dir_all_retry(&out)?;
        if std::fs::rename(&staging, &out).is_err() {
            let _ = remove_dir_all_retry(&staging);
        }
        // The fresh output has not been type-checked yet; mark the build complete.
        let _ = std::fs::remove_file(&tsc_marker);
        let _ = std::fs::write(&build_marker, b"");
    }

    // Type-check before running so type errors surface as diagnostics rather
    // than runtime crashes. A missing `tsc` is a warning, not a hard stop —
    // `tsx` can still run the program.
    if will_typecheck {
        use crate::runtime::TscOutcome;
        match crate::runtime::check_with_tsc(&out)? {
            TscOutcome::Passed => {
                let _ = std::fs::write(&tsc_marker, b"");
            }
            TscOutcome::Failed(msg) => {
                let remapped = crate::tscmap::remap_tsc_output(&msg, &module_maps, with_color);
                return Ok(diags.into_result(RunOutcome::TypeCheckFailed(remapped)));
            }
            TscOutcome::NotFound => {
                // The type check was requested (the default) but tsc is absent.
                // Refuse rather than run unchecked, so the guarantee never
                // silently evaporates. `--no-check` is the explicit opt-out.
                return Ok(diags.into_result(RunOutcome::TscMissing));
            }
        }
    }

    // A library module (no `fn main`) has nothing to run. Detect it here —
    // before generating the entrypoint that imports `{ main }` — so it reports
    // as a friendly diagnostic rather than a raw Node `TypeError` from calling
    // an undefined `main`. Fires on both the fresh and cached-build paths.
    // If the source doesn't parse, the build above already surfaced that.
    if let Ok(module) = glyph_parser::parse(&std::fs::read_to_string(file)?) {
        let has_main = module
            .items
            .iter()
            .any(|d| matches!(d, glyph_ast::Decl::Fn(f) if f.name.as_ref() == "main"));
        if !has_main {
            let exports = module
                .items
                .iter()
                .filter_map(|d| match d {
                    glyph_ast::Decl::Fn(f) => Some(f.name.to_string()),
                    _ => None,
                })
                .collect();
            return Ok(diags.into_result(RunOutcome::NoMain {
                exports,
                module: file.to_path_buf(),
            }));
        }
    }

    // The entrypoint is written into the shared, fingerprint-keyed `out`, which
    // a concurrent run of the same program may be swapping its own staging into
    // at this instant. That run removes `out` before its rename, so a write
    // landing in between fails with `NotFound` through no fault of this one:
    // the directory it is writing to was correct when it was chosen. Recreate
    // and retry rather than reporting a race as an IO error, which is how this
    // surfaced in CI as `concurrent run raced: Err(Io(...NotFound...))`.
    let entry = out.join("__glyph_run.ts");
    if let Err(e) = std::fs::write(&entry, entrypoint_source(stem)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
        std::fs::create_dir_all(&out)?;
        std::fs::write(&entry, entrypoint_source(stem))?;
    }

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
        Ok(status) => Ok(diags.into_result(RunOutcome::Ran(status.code().unwrap_or(1)))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(diags.into_result(RunOutcome::TsxNotFound))
        }
        Err(e) => Err(RunError::Io(e)),
    }
}

/// The generated entrypoint: install the prelude globals (side-effect import),
/// then call the program's `main` with the trailing argv and take its numeric
/// return as the exit code. A `main` that returns anything else leaves
/// `process.exitCode` untouched rather than writing 0 over it: a program that
/// recorded its verdict with `std/process.set_exit_code` has already set the
/// code, and overwriting it afterwards turned a reported failure into a
/// reported success. An async IIFE rather than top-level `await` so it
/// runs under either CommonJS or ESM resolution. Relative imports are resolved
/// against the entrypoint's own location, so the absolute path passed to `tsx`
/// works regardless of the caller's working directory.
///
/// The exit code is **assigned** (`process.exitCode`), not forced
/// (`process.exit`). The difference is the whole reason a server can be written
/// in Glyph: `process.exit` tears the process down the moment `main` returns,
/// while a listening socket, a timer, or any other pending handle is still
/// live, so `main` could start a server but never serve anything. Assigning the
/// code instead lets Node do what it normally does, which is exit once nothing
/// is left to wait for. A program that only computes and prints has nothing
/// pending and still exits immediately with the same code; a program that
/// started a listener stays up.
///
/// The `catch` arm still terminates: `main` throwing means the program failed
/// to start, and leaving a half-initialized process alive on a handle it opened
/// before it failed would hang instead of reporting the error. It waits for
/// stderr to drain first. `console.error` is asynchronous when stderr is a pipe
/// — which is every CI job and every agent capturing output — and calling
/// `process.exit` on the next line truncates the diagnostic it just wrote.
/// `process.exitCode` is set before the wait, so the exit code is right even if
/// the drain callback never runs.
fn entrypoint_source(stem: &str) -> String {
    format!(
        "import \"./.glyph-runtime/glyph-bootstrap.ts\";\n\
         import {{ main }} from \"./{stem}.ts\";\n\
         (async () => {{\n\
         \x20 try {{\n\
         \x20   const code = await main(process.argv.slice(2));\n\
         \x20   if (typeof code === \"number\") process.exitCode = code;\n\
         \x20 }} catch (e) {{\n\
         \x20   console.error(e);\n\
         \x20   process.exitCode = 1;\n\
         \x20   process.stderr.write(\"\", () => process.exit(1));\n\
         \x20 }}\n\
         }})();\n"
    )
}

#[cfg(test)]
mod entrypoint_tests {
    use super::entrypoint_source;

    /// The success path must *assign* the exit code, never force it.
    ///
    /// `process.exit` tears the process down as soon as `main` returns, while
    /// any pending handle is still live. A Glyph program that starts a TCP
    /// server therefore bound its port and died in the same tick: `glyph run`
    /// printed nothing and exited 0, and no client could ever connect. That is
    /// what made a multi-client server impossible to write, and it is invisible
    /// from the source, which looks correct and type-checks.
    ///
    /// Assigning `process.exitCode` leaves Node's own exit rule in place: leave
    /// when there is nothing left to wait for. A program that only computes
    /// still exits immediately with the same code.
    #[test]
    fn the_success_path_assigns_the_exit_code_rather_than_forcing_it() {
        let src = entrypoint_source("main");
        assert!(
            src.contains("if (typeof code === \"number\") process.exitCode = code;"),
            "expected an assignment to process.exitCode, got:\n{src}"
        );
        assert!(
            !src.contains("process.exit(typeof code"),
            "the success path must not call process.exit, got:\n{src}"
        );
    }

    /// A void `main` must leave `process.exitCode` alone.
    ///
    /// The success path used to write `typeof code === "number" ? code : 0`
    /// unconditionally, so a program whose `main` returns `void` had its exit
    /// code forced back to 0 after the fact. `std/process.set_exit_code` is
    /// documented to record a verdict that the process then leaves with, and
    /// this wrote over it with no diagnostic: a CLI that rejected its input and
    /// recorded 1 reported success to its caller.
    #[test]
    fn a_non_numeric_main_result_leaves_the_recorded_exit_code_alone() {
        let src = entrypoint_source("main");
        assert!(
            src.contains("if (typeof code === \"number\") process.exitCode = code;"),
            "the exit code must only be assigned when `main` returned one, got:\n{src}"
        );
        assert!(
            !src.contains("? code : 0"),
            "a non-numeric result must not force the exit code to 0, got:\n{src}"
        );
    }

    /// The failure path still terminates: a half-initialized process sitting on
    /// a handle it opened before it failed would hang instead of reporting the
    /// error. It sets the code first and terminates only once stderr has
    /// drained, so the diagnostic survives a piped stderr.
    #[test]
    fn the_failure_path_terminates_but_only_after_stderr_drains() {
        let src = entrypoint_source("main");
        assert!(
            src.contains("process.exitCode = 1;"),
            "the exit code must be set before the wait, got:\n{src}"
        );
        assert!(
            src.contains("process.stderr.write(\"\", () => process.exit(1));"),
            "a throwing main must terminate after stderr drains, got:\n{src}"
        );
        assert!(
            !src.contains("console.error(e);\n   process.exit(1);"),
            "process.exit must not immediately follow the write, got:\n{src}"
        );
    }
}
