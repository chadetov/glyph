//! `glyph init` — scaffold a runnable starter project.
//!
//! Writes a hello-world `src/main.glyph`, a `.types/` directory for ambient
//! declarations, a `package.json` carrying the `"glyph"` key (so `glyph publish`
//! works), and a `.gitignore`. Existing files are never overwritten — they are
//! reported as skipped — so `glyph init` is safe to run in a non-empty directory.

use std::path::{Path, PathBuf};

/// What `scaffold` did.
pub struct InitReport {
    pub root: PathBuf,
    pub created: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum InitError {
    Io(String),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitError::Io(m) => write!(f, "{m}"),
        }
    }
}

const MAIN_GLYPH: &str = "module main\n\
\n\
import std/io\n\
\n\
fn main(argv: Array<string>) -> number {\n\
\x20 io.println(\"hello from glyph\")\n\
\x20 return 0\n\
}\n";

const TYPES_README: &str = "# Ambient type declarations\n\
\n\
Put `*.d.ts` files here to give the type-checker types for the npm packages and\n\
Node builtins you import. Anything matching `.types/**/*.d.ts` is auto-discovered\n\
when you build. For a worked example, see\n\
<https://github.com/chadetov/glyph/blob/main/docs/guide/external-imports.md>.\n";

const GITIGNORE: &str = "dist/\n\
node_modules/\n";

/// Scaffold a starter project into `dir` (created if absent). The npm package
/// name is derived from the directory name.
pub fn scaffold(dir: &Path) -> Result<InitReport, InitError> {
    std::fs::create_dir_all(dir.join("src").join(".types"))
        .map_err(|e| InitError::Io(format!("cannot create {}: {e}", dir.display())))?;

    let name = project_name(dir);
    let package_json = format!(
        "{{\n\
\x20 \"name\": \"{name}\",\n\
\x20 \"version\": \"0.1.0\",\n\
\x20 \"private\": true,\n\
\x20 \"scripts\": {{\n\
\x20\x20\x20 \"start\": \"glyph run src/main.glyph\",\n\
\x20\x20\x20 \"build\": \"glyph build src --out dist\"\n\
\x20 }},\n\
\x20 \"glyph\": {{\n\
\x20\x20\x20 \"src\": \"src\"\n\
\x20 }}\n\
}}\n"
    );

    let files: [(PathBuf, &str); 4] = [
        (dir.join("src").join("main.glyph"), MAIN_GLYPH),
        (dir.join("src").join(".types").join("README.md"), TYPES_README),
        (dir.join("package.json"), package_json.as_str()),
        (dir.join(".gitignore"), GITIGNORE),
    ];

    let mut created = Vec::new();
    let mut skipped = Vec::new();
    for (path, contents) in files {
        if path.exists() {
            skipped.push(path);
            continue;
        }
        std::fs::write(&path, contents)
            .map_err(|e| InitError::Io(format!("cannot write {}: {e}", path.display())))?;
        created.push(path);
    }

    Ok(InitReport { root: dir.to_path_buf(), created, skipped })
}

/// A filesystem-safe npm package name derived from the directory name.
fn project_name(dir: &Path) -> String {
    let raw = dir
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .or_else(|| dir.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let sanitized: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "glyph-app".to_string()
    } else {
        trimmed.to_string()
    }
}
