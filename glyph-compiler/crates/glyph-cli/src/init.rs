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
    /// The entry file to run or build (`src/main.glyph`, or `src/lib.glyph` for a
    /// library), plus whether it is runnable (has a `main`).
    pub entry: PathBuf,
    pub runnable: bool,
}

/// The starter shape `glyph init --template <T>` scaffolds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// A command-line program (`fn main` returning an exit code). The default.
    Cli,
    /// An HTTP server over `std/http`.
    Web,
    /// A library of `pub` functions with an `@example`, no `main`.
    Lib,
}

impl Template {
    pub fn parse(s: &str) -> Option<Template> {
        match s {
            "cli" => Some(Template::Cli),
            "web" => Some(Template::Web),
            "lib" => Some(Template::Lib),
            _ => None,
        }
    }
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

/// Toolchain versions pinned into a scaffolded project's `devDependencies`, so
/// `glyph run`/`build` (which shell out to `tsx`/`tsc`) resolve a consistent
/// TypeScript across a team. `typescript` tracks the major the compiler's own CI
/// type-checks against (the classic checker); `tsx` is the runner.
const SCAFFOLD_TYPESCRIPT: &str = "^6.0.0";
const SCAFFOLD_TSX: &str = "^4.19.0";

/// The compiler itself, pinned like any other build tool.
///
/// A scaffolded project used to record which TypeScript built it and not which
/// *Glyph* did, so the one tool that decides whether the source compiles at all
/// was whatever happened to be on each developer's PATH. Two people on the same
/// repository could get different results from the same commit and have nothing
/// to compare. Pinning it also means `npm install` is enough to build a checkout
/// (the `scripts` below resolve `glyph` from `node_modules/.bin`), so a
/// contributor needs no global install and CI needs no separate setup step.
///
/// Tracked against the compiler's own version so a scaffold never asks for a
/// release older than the binary that wrote it.
///
/// **Exact, not a caret.** A caret on a `0.x` version still floats the patch
/// (`^0.1.72` accepts every 0.1.x), and the stability policy says plainly that
/// 0.1.x releases may "reject code that previously compiled (that is usually the
/// point)". Those two together mean a scaffolded project's build can go red on
/// an `npm install` run for an unrelated reason, with no change to its source.
/// A caret advertises a semver compatibility this line does not offer, so the
/// pin is exact and moving it is `glyph upgrade`, a thing the developer does on
/// purpose.
const SCAFFOLD_GLYPH: &str = env!("CARGO_PKG_VERSION");

const MAIN_GLYPH: &str = "module main\n\
\n\
import std/io\n\
\n\
fn main(argv: Array<string>) -> number {\n\
\x20 io.println(\"hello from glyph\")\n\
\x20 return 0\n\
}\n";

const WEB_GLYPH: &str = "module main\n\
\n\
import std/http { listen, text, Request, Response }\n\
import std/net\n\
import std/result { Result, Ok, Err }\n\
import std/io\n\
\n\
async fn main(argv: Array<string>) -> number {\n\
\x20 // `listen` resolves once the port is bound, so the line below is only\n\
\x20 // printed when it is true, and a port already in use is a value here\n\
\x20 // rather than a throw. \"127.0.0.1\" accepts only local connections;\n\
\x20 // use \"0.0.0.0\" to accept from the network.\n\
\x20 return match await listen(\"127.0.0.1\", 8080, fn(req: Request) -> Result<Response, string> {\n\
\x20\x20\x20 Ok(text(200, \"hello from glyph\"))\n\
\x20 }) {\n\
\x20\x20\x20 Ok(server) => {\n\
\x20\x20\x20\x20\x20 io.println(\"listening on http://localhost:${number.to_string(net.port(server))}\")\n\
\x20\x20\x20\x20\x20 0\n\
\x20\x20\x20 },\n\
\x20\x20\x20 Err(e) => {\n\
\x20\x20\x20\x20\x20 io.eprintln(\"cannot listen: ${e.message}\")\n\
\x20\x20\x20\x20\x20 1\n\
\x20\x20\x20 },\n\
\x20 }\n\
}\n";

const LIB_GLYPH: &str = "module lib\n\
\n\
// A library module: `pub` functions are importable by other modules. There is\n\
// no `main`, so this package is built (`glyph build`), not run. The `@example`\n\
// is checked at build time.\n\
\n\
@example greet(\"world\") == \"hello, world\"\n\
pub fn greet(name: string) -> string {\n\
\x20 return \"hello, ${name}\"\n\
}\n";

const TYPES_README: &str = "# Type declarations for modules you import\n\
\n\
Put `*.d.ts` files here to give the type-checker types for the npm packages and\n\
Node builtins you import. Anything matching `.types/**/*.d.ts` is auto-discovered\n\
when you build. For a worked example, see\n\
<https://github.com/chadetov/glyph/blob/main/docs/guide/external-imports.md>.\n\
\n\
Module declarations only. A `declare var`, `declare function` or `declare class`\n\
here is a global, and Glyph resolves names from modules, so the global satisfies\n\
`tsc` and stays invisible to Glyph: using it is `[E0103] unresolved name`. A host\n\
global the standard library does not wrap is a gap in the standard library, so\n\
file it and it gets a typed wrapper the way timers and WebSocket did.\n";

const GITIGNORE: &str = "dist/\n\
node_modules/\n";

/// Scaffold a starter project into `dir` (created if absent). The npm package
/// name is derived from the directory name. `Template::Cli` is the default shape.
pub fn scaffold(dir: &Path) -> Result<InitReport, InitError> {
    scaffold_template(dir, Template::Cli)
}

/// Scaffold the given starter `template` into `dir`.
pub fn scaffold_template(dir: &Path, template: Template) -> Result<InitReport, InitError> {
    std::fs::create_dir_all(dir.join("src").join(".types"))
        .map_err(|e| InitError::Io(format!("cannot create {}: {e}", dir.display())))?;

    // The entry file, its content, and whether the package is run or built.
    let (entry_name, entry_content, runnable) = match template {
        Template::Cli => ("main.glyph", MAIN_GLYPH, true),
        Template::Web => ("main.glyph", WEB_GLYPH, true),
        Template::Lib => ("lib.glyph", LIB_GLYPH, false),
    };
    let start_script = if runnable {
        {
            let _ = entry_name;
            "glyph run".to_string()
        }
    } else {
        "glyph build src --out dist".to_string()
    };

    let name = project_name(dir);
    let package_json = format!(
        "{{\n\
\x20 \"name\": \"{name}\",\n\
\x20 \"version\": \"0.1.0\",\n\
\x20 \"private\": true,\n\
\x20 \"scripts\": {{\n\
\x20\x20\x20 \"start\": \"{start_script}\",\n\
\x20\x20\x20 \"build\": \"glyph build src --out dist\"\n\
\x20 }},\n\
\x20 \"glyph\": {{\n\
\x20\x20\x20 \"src\": \"src\"\n\
\x20 }},\n\
\x20 \"devDependencies\": {{\n\
\x20\x20\x20 \"@glyphlang/glyph\": \"{SCAFFOLD_GLYPH}\",\n\
\x20\x20\x20 \"typescript\": \"{SCAFFOLD_TYPESCRIPT}\",\n\
\x20\x20\x20 \"tsx\": \"{SCAFFOLD_TSX}\"\n\
\x20 }}\n\
}}\n"
    );

    let entry = dir.join("src").join(entry_name);
    let files: [(PathBuf, &str); 4] = [
        (entry.clone(), entry_content),
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

    Ok(InitReport { root: dir.to_path_buf(), created, skipped, entry, runnable })
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
