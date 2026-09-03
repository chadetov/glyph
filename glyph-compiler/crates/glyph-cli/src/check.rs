//! `glyph check` — type-check a Glyph file or tree without running it and
//! without writing output anywhere the user can see.
//!
//! Before this existed the only way to type-check a single file was `glyph run
//! path.glyph`, which executes the program — so "does this compile?" cost you a
//! side effect. `glyph build` refuses a file (`source path is not a
//! directory`), and it writes emitted TypeScript into `--out`. `check` closes
//! both: it takes a file or a directory, it emits into a temporary directory it
//! deletes on the way out, and it never generates or runs an entrypoint. That
//! refusal in `build` now names this command, since the person who needs it is
//! standing there when they read it.
//!
//! The engine is `glyph build`'s, not a second pipeline. `check_path` resolves
//! the source root the way `glyph run` does (a `.glyph` file means its
//! directory), calls `build::build_project_inner`, optionally type-checks the
//! emitted TypeScript with `tsc --strict`, and hands back the same diagnostics
//! `build` would have reported on the same tree.
//!
//! Two properties are worth stating because they are choices, not accidents:
//!
//! - **Scope is the containing directory.** `glyph check one.glyph` reports
//!   every diagnostic in `one.glyph`'s directory, exactly as `glyph build` and
//!   `glyph run` do on that tree. A sibling's error is your error here; that
//!   keeps the three commands from disagreeing about whether a tree is clean.
//! - **`@example` / `@doc @run` tests do not run.** `glyph build` runs them by
//!   default (D23/D26), but running them executes the program, which is the one
//!   thing `check` exists to avoid. A green `check` therefore promises less
//!   than a green `build` on the same tree: it promises the types, not the
//!   colocated tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use crate::build::{build_tree, BuildError};
use crate::run::remove_dir_all_retry;
use crate::runtime::TscOutcome;

/// Per-process counter making each check's scratch directory unique even across
/// threads in one process (two tests checking the same tree would otherwise
/// share, and race on, one path).
static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("check target does not exist: {0}")]
    Missing(PathBuf),
    #[error("check target is not a `.glyph` file: {0}")]
    NotGlyph(PathBuf),
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error("io error preparing the check directory: {0}")]
    Io(#[from] std::io::Error),
}

/// What a check found.
///
/// `diagnostics` is the Glyph stage rendered for humans; `structured` is the
/// same set for `--json`, **plus** the remapped `tsc` errors when the
/// TypeScript stage ran and failed. `error_count` counts the error-severity
/// entries in `structured`, so it answers the only question the command is
/// asked: did the check find errors, from any stage.
#[derive(Debug)]
pub struct CheckReport {
    /// The project source roots this check covered, so the caller can run each
    /// project's `@example` gate in the root its imports resolve from (D41).
    pub project_srcs: Vec<PathBuf>,
    /// Module paths the check saw, in deterministic order.
    pub modules: Vec<String>,
    /// One entry per Glyph-stage diagnostic (errors and warnings), pre-rendered.
    /// `tsc`'s own text lives on `tsc`, already remapped onto Glyph source.
    pub diagnostics: Vec<String>,
    /// Every diagnostic in structured form: the Glyph stage first, then the
    /// remapped `tsc` errors if there were any.
    pub structured: Vec<crate::diagnostic::Diagnostic>,
    /// How many of `structured` are error-severity.
    pub error_count: usize,
    /// What the TypeScript stage did. `NotFound` when `tsc` is absent from
    /// `PATH`; `Failed` carries the output already remapped onto Glyph source.
    /// Meaningless unless `tsc_ran` is true.
    pub tsc: TscOutcome,
    /// Whether the TypeScript stage ran at all. False when the caller opted out
    /// of it, and false when the Glyph stage already had errors (there was no
    /// coherent output to check). Kept separate rather than smuggled into
    /// `tsc`, because "we did not look" and "we looked and found nothing" are
    /// the two answers this command must never confuse.
    pub tsc_ran: bool,
    /// Problems with the tree rather than with any source file: a `package.json`
    /// that would not parse, so its directory is not a project root (D41).
    pub notices: Vec<String>,
}

impl CheckReport {
    /// True if any stage produced an error. Warnings alone do not fail a check.
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    /// How many entries in `structured` are warnings.
    pub fn warning_count(&self) -> usize {
        self.structured.len() - self.error_count
    }

    /// The `tsc` field as the stable string the `--json` output uses, matching
    /// `glyph build --json`'s `tsc` key.
    pub fn tsc_status(&self) -> &'static str {
        if !self.tsc_ran {
            return "not-run";
        }
        match &self.tsc {
            TscOutcome::Passed => "passed",
            TscOutcome::Failed(_) => "failed",
            TscOutcome::NotFound => "not-found",
        }
    }
}

/// Type-check `path` (a `.glyph` file or a directory) and report what it found.
///
/// The emitted TypeScript goes to a scratch directory under the system temp
/// dir, which is removed before returning, so nothing is written into the
/// user's tree and nothing is left behind. When `run_tsc` is set (the default
/// for the CLI) the emitted output is type-checked with `tsc --strict` and its
/// errors are remapped back onto Glyph source; the caller decides what a
/// missing `tsc` means.
///
/// Nothing is executed: no entrypoint is generated, `tsx` is never spawned, and
/// the `@example` / `@doc @run` gate is not run.
pub fn check_path(path: &Path, with_color: bool, run_tsc: bool) -> Result<CheckReport, CheckError> {
    if !path.exists() {
        return Err(CheckError::Missing(path.to_path_buf()));
    }
    // A directory is checked whole; a file is checked in the context of its
    // directory (siblings have to compile for imports to resolve), the same
    // root `glyph run` uses.
    let src: PathBuf = if path.is_dir() {
        path.to_path_buf()
    } else {
        if path.extension().and_then(|e| e.to_str()) != Some("glyph") {
            return Err(CheckError::NotGlyph(path.to_path_buf()));
        }
        // A file is checked in the context of its *project* (D41, P6): the
        // nearest enclosing directory carrying a `package.json` with a `"glyph"`
        // key, else its own directory, which is the pre-D41 behaviour and what
        // `glyph run` uses for the same file. Relative and absolute spellings of
        // the same file find the same project.
        crate::config::project_for_file(path).src
    };

    // Scratch output, named by pid + counter, which is all the uniqueness a
    // directory that is deleted on the way out needs: no two live checks, in
    // this process or another, can collide. It used to carry a
    // `source_fingerprint` too, which meant reading and hashing every source
    // file to name something that was about to be thrown away. `glyph run`
    // fingerprints because it keeps its build (`glyph-run-cache`); this one
    // does not keep anything, so it does not pay for a key.
    let n = SCRATCH_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join("glyph-check-scratch");
    let out = root.join(format!("{}-{n}", std::process::id()));
    let _ = remove_dir_all_retry(&out);

    let result = check_in(&src, &out, with_color, run_tsc, path.is_dir());
    let _ = remove_dir_all_retry(&out);
    // Take the parent too, so a machine that ran `glyph check` once is not left
    // with an empty directory under /tmp forever. Non-recursive on purpose: it
    // fails, harmlessly, while another check still has a directory in there.
    let _ = std::fs::remove_dir(&root);
    result
}

/// The body of a check, factored out so the scratch directory is removed on the
/// error paths as well as the happy one.
fn check_in(
    src: &Path,
    out: &Path,
    with_color: bool,
    run_tsc: bool,
    whole_tree: bool,
) -> Result<CheckReport, CheckError> {
    // A directory target may hold several Glyph projects (D41); each is built in
    // its own root, and the reports are concatenated in project order by the
    // same `flatten` the build command uses, so the two cannot drift.
    // A directory target may hold several projects and all of them are meant.
    // A *file* target belongs to exactly one, and the projects nested below that
    // project's root cannot be imported from it, so building them is work the
    // file did not ask for.
    let tree = if whole_tree {
        build_tree(src, out, with_color)?
    } else {
        crate::build::build_one_project(src, out, with_color)?
    };
    let report = tree.flatten();

    let mut structured = report.structured.clone();
    let mut error_count = report.error_count;
    let mut tsc = TscOutcome::Passed;
    let mut tsc_ran = false;

    // Only worth running `tsc` when the Glyph stage emitted something coherent;
    // a tree with errors has modules missing from the output, and the resulting
    // TypeScript errors would be noise about a cause already reported.
    if run_tsc && !report.has_errors() {
        tsc_ran = true;
        // Each project is type-checked in its own output subtree, because that
        // is where `build_project_inner` staged its runtime and wrote its
        // `tsconfig.json`. Same walk `glyph build` runs, so a missing `tsc` is
        // reported as missing in both.
        tsc = crate::runtime::check_tree_with_tsc(&tree, out)?;
        if let TscOutcome::Failed(msg) = &tsc {
            let remapped_text =
                crate::tscmap::remap_tsc_output(msg, &report.module_maps, with_color);
            let remapped = crate::tscmap::remap_tsc_to_diagnostics(msg, &report.module_maps);
            error_count += remapped.iter().filter(|d| d.severity == "error").count();
            structured.extend(remapped);
            tsc = TscOutcome::Failed(remapped_text);
        }
    }

    Ok(CheckReport {
        project_srcs: tree
            .projects
            .iter()
            .map(|p| p.project.src.clone())
            .collect(),
        modules: report.modules,
        diagnostics: report.diagnostics,
        structured,
        error_count,
        tsc,
        tsc_ran,
        notices: tree.notices,
    })
}
