//! `glyph publish` — prepare and verify a Glyph package for npm.
//!
//! v1 scope: read the `"glyph"` key from `package.json`, run the Q22
//! audit-currency gate over its declared imports, then build and type-check the
//! project into `dist/`. A clean run leaves a verified `dist/` and reports the
//! package as ready; the maintainer runs `npm publish` (which needs their npm
//! account). Producing a fully standalone tarball — rewriting `std/*` import
//! specifiers so the emitted JS resolves without the build's `tsconfig` paths,
//! i.e. bundling — is the remaining piece and shares that work with `glyph run`;
//! it is deferred until there are Glyph libraries to publish.

use std::path::{Path, PathBuf};

use crate::config::{self, StaleImport};
use crate::runtime::{check_with_tsc, TscOutcome};

/// Outcome of a successful `prepare` (the build may still carry diagnostics).
pub struct PublishReport {
    pub package_name: Option<String>,
    pub src: PathBuf,
    pub dist: PathBuf,
    /// Stale third-party imports surfaced as warnings (only when the policy does
    /// not enforce; an enforcing policy turns these into `AuditFailed`).
    pub warnings: Vec<StaleImport>,
    pub modules_checked: usize,
    pub emitted: usize,
    /// Build diagnostics; non-empty means the project did not compile.
    pub diagnostics: Vec<String>,
    pub has_build_errors: bool,
    pub tsc: TscStatus,
}

pub enum TscStatus {
    Passed,
    Failed(String),
    Skipped,
}

pub enum PublishError {
    NoPackageJson(PathBuf),
    Config(String),
    AuditFailed(Vec<StaleImport>),
    Build(String),
}

/// Prepare `dir` (a directory containing `package.json`) for publishing.
pub fn prepare(dir: &Path, with_color: bool) -> Result<PublishReport, PublishError> {
    let pkg = config::read_package_json(dir)
        .map_err(PublishError::Config)?
        .ok_or_else(|| PublishError::NoPackageJson(dir.join("package.json")))?;
    let glyph = pkg.glyph.unwrap_or_default();

    // Q22 audit-currency gate.
    let stale = config::check_audit_currency(&glyph, config::today());
    if !stale.is_empty() && glyph.audit.enforce {
        return Err(PublishError::AuditFailed(stale));
    }

    let src = resolve_src(dir, &glyph);
    let dist = dir.join("dist");
    let report = crate::build::build_project_inner(&src, &dist, with_color)
        .map_err(|e| PublishError::Build(e.to_string()))?;

    if report.has_errors() {
        return Ok(PublishReport {
            package_name: pkg.name,
            src,
            dist,
            warnings: stale,
            modules_checked: report.modules.len(),
            emitted: report.emitted.len(),
            diagnostics: report.diagnostics,
            has_build_errors: true,
            tsc: TscStatus::Skipped,
        });
    }

    let tsc = match check_with_tsc(&dist) {
        Ok(TscOutcome::Passed) => TscStatus::Passed,
        Ok(TscOutcome::Failed(msg)) => TscStatus::Failed(msg),
        Ok(TscOutcome::NotFound) => TscStatus::Skipped,
        Err(e) => TscStatus::Failed(format!("failed to run tsc: {e}")),
    };

    Ok(PublishReport {
        package_name: pkg.name,
        src,
        dist,
        warnings: stale,
        modules_checked: report.modules.len(),
        emitted: report.emitted.len(),
        diagnostics: report.diagnostics,
        has_build_errors: false,
        tsc,
    })
}

/// The source directory: an explicit `glyph.src`, else `src/` if it exists, else
/// the project directory itself.
fn resolve_src(dir: &Path, glyph: &config::GlyphConfig) -> PathBuf {
    if let Some(src) = &glyph.src {
        return dir.join(src);
    }
    let conventional = dir.join("src");
    if conventional.is_dir() {
        conventional
    } else {
        dir.to_path_buf()
    }
}

/// Render a stale import as a one-line human message.
pub fn describe_stale(s: &StaleImport) -> String {
    use config::StaleReason::*;
    match &s.reason {
        NeverReviewed => format!("{}: third-party, never reviewed", s.path),
        ReviewExpired { months_ago, max } => {
            format!("{}: third-party, last reviewed {months_ago} months ago (max {max})", s.path)
        }
        BadDate(d) => format!("{}: last_reviewed `{d}` is not a YYYY-MM-DD date", s.path),
    }
}
