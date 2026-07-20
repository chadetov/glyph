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

use std::path::Path;

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
    (
        ".glyph-runtime/glyph-node-shims.d.ts",
        include_str!("../../../runtime/glyph-node-shims.d.ts"),
    ),
];

/// The generated `tsconfig.json`. `paths` resolves `std/*` imports to the
/// bundled runtime; `include` covers the emitted output, the runtime, and any
/// project-supplied ambient declarations copied from `<src>/.types/`. The
/// relative `paths` entry resolves against the tsconfig's own directory (TS
/// 4.1+), so no `baseUrl` is needed — and `baseUrl` is deprecated as of
/// TypeScript 6, which would make `--check` fail on a current toolchain.
const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "noEmit": true,
    "target": "es2022",
    "lib": ["es2022", "dom"],
    "module": "esnext",
    "moduleResolution": "bundler",
    "skipLibCheck": true,
    "types": [],
    "paths": { "std/*": ["./.glyph-runtime/std/*"] }
  },
  "include": [
    "**/*.ts",
    ".glyph-runtime/**/*.ts",
    ".glyph-runtime/**/*.d.ts",
    ".types/**/*.d.ts"
  ]
}
"#;

/// Write the bundled runtime, a `tsconfig.json`, and any `<src>/.types/`
/// ambient declarations into `out`, so `tsc -p <out>/tsconfig.json` can type
/// the emitted TypeScript.
pub fn write_build_support(out: &Path, src: &Path) -> std::io::Result<()> {
    for (rel, contents) in RUNTIME_FILES {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    std::fs::write(out.join("tsconfig.json"), TSCONFIG)?;

    // A project may supply ambient declarations for its external dependencies
    // (npm packages, sibling app modules) in `<src>/.types/`; copy them so the
    // generated config picks them up.
    let src_types = src.join(".types");
    if src_types.is_dir() {
        copy_dir(&src_types, &out.join(".types"))?;
    }
    Ok(())
}

/// Result of running `tsc` over a build's generated `tsconfig.json`.
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

#[cfg(test)]
mod tests {
    use super::RUNTIME_FILES;
    use glyph_resolver::StdlibStubs;
    use std::collections::BTreeSet;

    /// Top-level names a runtime `.ts` exports, parsed from `export <kind> NAME`
    /// declarations. Covers the direct forms the bundled stdlib uses (`function`,
    /// `async function`, `const`, `let`, `type`, `class`, `interface`); a type
    /// and a value sharing a name (e.g. `fs.ErrorKind`) collapse to one entry.
    fn exported_names(ts: &str) -> BTreeSet<String> {
        const KINDS: [&str; 7] = [
            "async function ",
            "function ",
            "const ",
            "let ",
            "type ",
            "class ",
            "interface ",
        ];
        let mut out = BTreeSet::new();
        for line in ts.lines() {
            let Some(rest) = line.trim_start().strip_prefix("export ") else {
                continue;
            };
            for kw in KINDS {
                if let Some(after) = rest.strip_prefix(kw) {
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.insert(name);
                    }
                    break;
                }
            }
        }
        out
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
}
