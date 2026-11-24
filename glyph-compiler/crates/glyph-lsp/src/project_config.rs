//! The `glyph` key of a project's `package.json`, read from disk for every
//! surface that checks a project: `glyph check` and `glyph build` (through
//! `glyph-cli`), the language server and the MCP server (D74).
//!
//! One reader, so the surfaces cannot disagree about which bans apply to a
//! file. The validation itself is `glyph_resolver::read_glyph_config`; this
//! module finds the manifest that governs a resolution root and supplies the
//! facts about the project the validation needs.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use glyph_resolver::{ConfigError, ImportPolicy, ModuleGraph, ProjectFacts, StdlibStubs};

use crate::diagnostic::{from_config_error, Diagnostic};

/// What the governing manifest says about a project.
#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    /// The `package.json` whose `glyph` key marks this root, or `None` for an
    /// unmarked tree, which has no policy.
    pub manifest: Option<PathBuf>,
    /// The manifest's text, which `errors` are spans of.
    pub text: String,
    pub policy: ImportPolicy,
    pub errors: Vec<ConfigError>,
}

impl ProjectConfig {
    /// The `E0116` diagnostics, with `file` written from `root`, the root the
    /// project's module paths are counted from.
    pub fn diagnostics(&self, root: &Path) -> Vec<Diagnostic> {
        let Some(manifest) = &self.manifest else {
            return Vec::new();
        };
        let label = manifest_label(root, manifest);
        self.errors
            .iter()
            .map(|e| from_config_error(&label, &self.text, e))
            .collect()
    }
}

/// The manifest's path written from `root`: `package.json` when the root is
/// the marked directory, `../package.json` under a `src/` root.
pub fn manifest_label(root: &Path, manifest: &Path) -> String {
    let dir = manifest.parent().unwrap_or(Path::new(""));
    let depth = root
        .strip_prefix(dir)
        .map(|rel| rel.components().count())
        .unwrap_or(0);
    format!("{}package.json", "../".repeat(depth))
}

/// The directory whose `package.json` marks `root` as its resolution root, the
/// nearest one at or above it (D41: nearest wins, and a ban is not inherited
/// from an outer root). `None` for a root no marker declares.
///
/// The climb stops at a directory holding `.git`, as every other marker climb
/// does.
pub fn marker_dir_for_root(root: &Path) -> Option<PathBuf> {
    let want = canonical(root);
    let mut dir = Some(root);
    while let Some(d) = dir {
        if let Some(src) = crate::marked_project_src(d) {
            if canonical(&src) == want {
                return Some(d.to_path_buf());
            }
        }
        if d.join(".git").exists() {
            break;
        }
        dir = d.parent();
    }
    None
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Read the `glyph` key governing `root`. Re-read on every call: the callers
/// decide when (each build, each MCP call, each language-server rescan), and
/// the read is one small file.
pub fn load_project_config(root: &Path) -> ProjectConfig {
    let Some(dir) = marker_dir_for_root(root) else {
        return ProjectConfig::default();
    };
    let manifest = dir.join("package.json");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return ProjectConfig::default();
    };
    let project_module = |p: &str| is_relative_module_path(p) && root.join(format!("{p}.glyph")).is_file();
    let project_dir = |p: &str| is_relative_module_path(p) && root.join(p).is_dir();
    let std_module = |p: &str| std_modules().iter().any(|m| m == p);
    let facts = ProjectFacts {
        version: env!("CARGO_PKG_VERSION"),
        project_module: &project_module,
        project_dir: &project_dir,
        std_module: &std_module,
    };
    let Some(read) = glyph_resolver::read_glyph_config(&text, &facts) else {
        return ProjectConfig::default();
    };
    ProjectConfig {
        manifest: Some(manifest),
        text,
        policy: read.policy,
        errors: read.errors,
    }
}

/// A module path written under the source root: no leading `/`, no `.` or `..`
/// segment, so a check of it cannot reach outside the root.
fn is_relative_module_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && p.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn std_modules() -> &'static [String] {
    static MODULES: OnceLock<Vec<String>> = OnceLock::new();
    MODULES.get_or_init(|| StdlibStubs::new().owned_modules())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glyph-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/http")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("src/http/client.glyph"), "module http/client\n").unwrap();
        dir
    }

    #[test]
    fn the_marker_of_a_src_root_is_found_and_its_errors_point_at_it() {
        let dir = tmp("src-root");
        std::fs::write(
            dir.join("package.json"),
            r#"{"glyph": {"src": "src", "banned_imports": {"axios": {"use": "http/clinet"}}}}"#,
        )
        .unwrap();
        let root = dir.join("src");
        assert_eq!(marker_dir_for_root(&root).map(|d| canonical(&d)), Some(canonical(&dir)));
        let config = load_project_config(&root);
        assert_eq!(config.policy.bans.len(), 1);
        let diags = config.diagnostics(&root);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E0116");
        assert_eq!(diags[0].file, "../package.json");
        assert_eq!(diags[0].stage, "config");
        assert!(diags[0].module.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unmarked_root_and_a_manifest_that_does_not_parse_have_no_policy() {
        let dir = tmp("unmarked");
        assert!(load_project_config(&dir.join("src")).manifest.is_none());
        std::fs::write(dir.join("package.json"), r#"{"glyph": {"banned_imports": {"axios": {"use": "x"}}},}"#).unwrap();
        let config = load_project_config(&dir.join("src"));
        assert!(config.policy.is_empty() && config.errors.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
