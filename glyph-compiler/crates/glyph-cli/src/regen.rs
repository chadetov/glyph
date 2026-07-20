//! `glyph regen` — refresh `gen`-family output from its source specs.
//!
//! Every file `glyph gen ...` writes carries its exact, re-runnable invocation
//! in a header comment (`Regenerate with `glyph gen openapi spec.yaml --out
//! src/api --client``). `glyph regen [PATH]` scans `PATH` (a directory, walked
//! recursively, or a single file — default: the current directory) for those
//! headers, collects the unique commands, and runs each once. So when a spec
//! changes, one command brings every generated file back in sync — the
//! deterministic, human-owned-contract half of Q40 (an LLM regenerating a
//! *body* from a prompt is a separate, non-deterministic concern).
//!
//! Paths in the recorded command are exactly as the user typed them at `gen`
//! time (relative to the project root), so `regen` is meant to run from the
//! same directory `gen` was. Commands are re-run by calling the `gen` entry
//! points directly, not by shelling out.

use std::path::{Path, PathBuf};

use crate::gen;

/// One recovered, re-runnable `gen` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GenCommand {
    /// `openapi` | `dts` | `zod`.
    target: String,
    /// The source spec/declaration path, as recorded.
    source: PathBuf,
    /// The `--out` directory, as recorded.
    out: PathBuf,
    client: bool,
    handlers: bool,
    /// The verbatim command line, for reporting and de-duplication.
    raw: String,
}

#[derive(Debug)]
pub enum RegenError {
    /// The scan path does not exist.
    NotFound { path: PathBuf },
    /// No file under the path carried a `glyph gen` provenance header.
    NoGenerated { path: PathBuf },
    /// A recovered command could not be parsed back into a `gen` invocation.
    Unparseable { raw: String },
    /// Re-running a command failed.
    Failed { raw: String, source: gen::GenError },
}

impl std::fmt::Display for RegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegenError::NotFound { path } => write!(f, "no such path: {}", path.display()),
            RegenError::NoGenerated { path } => write!(
                f,
                "no generated files found under {} (nothing carries a `glyph gen` header to regenerate)",
                path.display()
            ),
            RegenError::Unparseable { raw } => {
                write!(f, "could not parse the recorded command: `{raw}`")
            }
            RegenError::Failed { raw, source } => write!(f, "`{raw}` failed: {source}"),
        }
    }
}

/// What `regen` did, for the CLI to report.
#[derive(Debug)]
pub struct RegenReport {
    /// One `(command, types-written)` per re-run invocation, in run order.
    pub ran: Vec<(String, usize)>,
}

/// Scan `path` for `gen` provenance headers and re-run each unique command.
pub fn regen(path: &Path) -> Result<RegenReport, RegenError> {
    if !path.exists() {
        return Err(RegenError::NotFound {
            path: path.to_path_buf(),
        });
    }

    let mut files = Vec::new();
    collect_glyph_files(path, &mut files);

    // Recover commands, de-duplicated by their verbatim text and kept in a
    // stable order (sorted) so a run is deterministic regardless of walk order.
    let mut raws: Vec<String> = Vec::new();
    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        if let Some(cmd) = extract_regen_command(&source) {
            if !raws.contains(&cmd) {
                raws.push(cmd);
            }
        }
    }
    if raws.is_empty() {
        return Err(RegenError::NoGenerated {
            path: path.to_path_buf(),
        });
    }
    raws.sort();

    let mut ran = Vec::new();
    for raw in raws {
        let cmd = parse_gen_command(&raw).ok_or_else(|| RegenError::Unparseable { raw: raw.clone() })?;
        let report = run(&cmd).map_err(|source| RegenError::Failed {
            raw: raw.clone(),
            source,
        })?;
        ran.push((cmd.raw, report.type_count));
    }
    Ok(RegenReport { ran })
}

/// Re-run one recovered command through the matching `gen` entry point.
fn run(cmd: &GenCommand) -> Result<gen::GenReport, gen::GenError> {
    match cmd.target.as_str() {
        "openapi" => gen::openapi(&cmd.source, &cmd.out, cmd.client, cmd.handlers),
        "dts" => gen::dts(&cmd.source, &cmd.out),
        "zod" => gen::zod(&cmd.source, &cmd.out),
        // parse_gen_command only accepts the three known targets.
        _ => unreachable!("unknown gen target survived parsing: {}", cmd.target),
    }
}

/// Pull the backtick-quoted `glyph gen ...` command out of a generated file's
/// header. Returns the command text without the backticks.
fn extract_regen_command(source: &str) -> Option<String> {
    const NEEDLE: &str = "`glyph gen ";
    let start = source.find(NEEDLE)? + 1; // step past the opening backtick
    let rest = &source[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Parse `glyph gen <target> <source> --out <dir> [--client] [--handlers]` into
/// a `GenCommand`. Whitespace-tokenized, so recorded paths must not contain
/// spaces (they never do for `gen` output).
fn parse_gen_command(raw: &str) -> Option<GenCommand> {
    let toks: Vec<&str> = raw.split_whitespace().collect();
    // ["glyph", "gen", target, source, ...]
    if toks.len() < 4 || toks[0] != "glyph" || toks[1] != "gen" {
        return None;
    }
    let target = toks[2].to_string();
    if !matches!(target.as_str(), "openapi" | "dts" | "zod") {
        return None;
    }
    let source = PathBuf::from(toks[3]);

    let mut out: Option<PathBuf> = None;
    let mut client = false;
    let mut handlers = false;
    let mut i = 4;
    while i < toks.len() {
        match toks[i] {
            "--out" => {
                out = Some(PathBuf::from(toks.get(i + 1)?));
                i += 2;
            }
            "--client" => {
                client = true;
                i += 1;
            }
            "--handlers" => {
                handlers = true;
                i += 1;
            }
            _ => return None,
        }
    }

    Some(GenCommand {
        target,
        source,
        out: out?,
        client,
        handlers,
        raw: raw.to_string(),
    })
}

/// Collect every `.glyph` file at or under `path`, skipping hidden and `target`
/// directories (matching the build walker's conventions).
fn collect_glyph_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|e| e == "glyph") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let skip = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == "target" || n.starts_with('.'));
            if !skip {
                collect_glyph_files(&p, out);
            }
        } else if p.extension().is_some_and(|e| e == "glyph") {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_backtick_command() {
        let src = "module api\n// Generated from spec.yaml. Regenerate with \
                   `glyph gen openapi spec.yaml --out src/api --client`.\n";
        assert_eq!(
            extract_regen_command(src).as_deref(),
            Some("glyph gen openapi spec.yaml --out src/api --client")
        );
    }

    #[test]
    fn extracts_nothing_from_a_hand_written_file() {
        assert!(extract_regen_command("module m\nfn f() -> number { return 1 }\n").is_none());
    }

    #[test]
    fn parses_openapi_with_flags() {
        let cmd = parse_gen_command("glyph gen openapi spec.yaml --out src/api --client --handlers")
            .expect("parses");
        assert_eq!(cmd.target, "openapi");
        assert_eq!(cmd.source, PathBuf::from("spec.yaml"));
        assert_eq!(cmd.out, PathBuf::from("src/api"));
        assert!(cmd.client && cmd.handlers);
    }

    #[test]
    fn parses_dts_without_flags() {
        let cmd = parse_gen_command("glyph gen dts types.d.ts --out out").expect("parses");
        assert_eq!(cmd.target, "dts");
        assert!(!cmd.client && !cmd.handlers);
    }

    #[test]
    fn rejects_an_unknown_target_or_missing_out() {
        assert!(parse_gen_command("glyph gen bogus x --out o").is_none());
        assert!(parse_gen_command("glyph gen openapi x").is_none());
    }
}
