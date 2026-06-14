//! Compile-time `@example` execution (D23).
//!
//! Each `@example expr == expr` above a declaration is a test: the build runs it
//! and fails if the two sides are not equal. Rather than interpret Glyph, the
//! runner reuses the real toolchain — it splices both sides of every example
//! into the module as synthesized functions, builds the (augmented) project to
//! TypeScript, and runs a generated harness through `tsx` that **deep-compares**
//! the two values (structural equality, so `Result`/record examples work). This
//! keeps a single source of semantics: the emitter.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_ast::{Decl, Expr};
use glyph_formatter::format_expr;

use crate::build::{build_project_inner, BuildError};

#[derive(Debug, thiserror::Error)]
pub enum ExampleError {
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error("io error preparing example run at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Outcome of running a project's `@example`s.
#[derive(Debug, Default)]
pub struct ExampleReport {
    /// Total examples found across the project.
    pub total: usize,
    /// Human-readable failure lines (a failed equality, a thrown example, or a
    /// malformed `@example` that did not parse).
    pub failures: Vec<String>,
    /// False when execution was skipped because `tsx` is not on PATH.
    pub ran: bool,
    /// Set when the augmented project failed to compile; carries its
    /// diagnostics. Usually means an `@example` references something invalid.
    pub build_failed: Option<Vec<String>>,
}

impl ExampleReport {
    pub fn ok(&self) -> bool {
        self.failures.is_empty() && self.build_failed.is_none()
    }
}

struct FileExamples {
    rel: PathBuf,
    module_path: String,
    /// `(lhs_src, rhs_src)` for each `@example`, already rendered to Glyph text.
    cases: Vec<(String, String)>,
    /// Glyph code for each ` ```glyph @run ``` ` block in a `@doc` (D26).
    runs: Vec<String>,
    /// Malformed `@example` argument strings that did not parse.
    malformed: Vec<String>,
}

/// Run every `@example` in the project rooted at `src`.
pub fn run_examples(src: &Path) -> Result<ExampleReport, ExampleError> {
    let mut files = Vec::new();
    collect_glyph_files(src, &mut files)?;
    files.sort();

    let mut per_file = Vec::new();
    let mut total = 0;
    let mut malformed_total = 0;
    for f in &files {
        let source = read(f)?;
        let Ok(module) = glyph_parser::parse(&source) else {
            // A file that does not parse is reported by the real build; skip it
            // here so the example runner does not double-report.
            continue;
        };
        let (cases, runs, malformed) = collect_tests(&module);
        if cases.is_empty() && runs.is_empty() && malformed.is_empty() {
            continue;
        }
        total += cases.len() + runs.len();
        malformed_total += malformed.len();
        let rel = f.strip_prefix(src).unwrap_or(f).to_path_buf();
        let module_path = module_path_of(&rel);
        per_file.push(FileExamples {
            rel,
            module_path,
            cases,
            runs,
            malformed,
        });
    }

    let mut report = ExampleReport {
        total,
        ran: true,
        ..Default::default()
    };
    for fe in &per_file {
        for m in &fe.malformed {
            report
                .failures
                .push(format!("{}: malformed @example `{m}`", fe.module_path));
        }
    }
    if total == 0 {
        // Only malformed examples (or none at all); nothing to execute.
        report.ran = malformed_total == 0;
        return Ok(report);
    }

    // Augment a throwaway copy of the project and build it. The directory is
    // unique per call (pid + a monotonic counter) so concurrent runs — e.g.
    // parallel tests in one process — do not clobber each other.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("glyph-examples-{}-{n}", std::process::id()));
    let tsrc = root.join("src");
    let tout = root.join("out");
    if root.exists() {
        remove_dir_all(&root)?;
    }
    copy_dir(src, &tsrc)?;
    for fe in &per_file {
        if fe.cases.is_empty() && fe.runs.is_empty() {
            continue;
        }
        let path = tsrc.join(&fe.rel);
        let mut text = read(&path)?;
        for (i, (l, r)) in fe.cases.iter().enumerate() {
            text.push_str(&format!(
                "\nfn __glyph_example_{i}() {{\n  return {{ lhs: {l}, rhs: {r} }}\n}}\n"
            ));
        }
        for (i, code) in fe.runs.iter().enumerate() {
            text.push_str(&format!("\nfn __glyph_run_{i}() -> void {{\n{code}\n}}\n"));
        }
        write(&path, &text)?;
    }

    let build = build_project_inner(&tsrc, &tout, false)?;
    if build.has_errors() {
        report.build_failed = Some(build.diagnostics);
        return Ok(report);
    }

    // Generate and run the harness.
    let harness = generate_harness(&per_file);
    write(&tout.join("__glyph_examples.ts"), &harness)?;
    let tsconfig = tout.join("tsconfig.json");
    let entry = tout.join("__glyph_examples.ts");
    match Command::new("tsx")
        .arg("--tsconfig")
        .arg(&tsconfig)
        .arg(&entry)
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if let Some(rest) = line.strip_prefix("FAIL ") {
                    report.failures.push(rest.to_string());
                }
            }
            Ok(report)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.ran = false;
            Ok(report)
        }
        Err(e) => Err(ExampleError::Io {
            path: entry,
            source: e,
        }),
    }
}

/// For a module, collect: `@example` `(lhs, rhs)` pairs (rendered to Glyph
/// text), `@doc` `@run` code blocks, and any `@example` whose argument failed to
/// parse.
fn collect_tests(
    module: &glyph_ast::Module,
) -> (Vec<(String, String)>, Vec<String>, Vec<String>) {
    let mut cases = Vec::new();
    let mut runs = Vec::new();
    let mut malformed = Vec::new();
    for decl in &module.items {
        for ann in decl_annotations(decl) {
            match ann.name.as_ref() {
                "example" => match glyph_parser::parse_expression(&ann.raw_args) {
                    Ok(Expr::Binary {
                        op: glyph_ast::BinOp::Eq,
                        left,
                        right,
                        ..
                    }) => cases.push((format_expr(&left), format_expr(&right))),
                    // A non-equality example asserts the expression is `true`.
                    Ok(other) => cases.push((format_expr(&other), "true".to_string())),
                    Err(_) => malformed.push(ann.raw_args.clone()),
                },
                "doc" => runs.extend(extract_run_blocks(doc_body(&ann.raw_args))),
                _ => {}
            }
        }
    }
    (cases, runs, malformed)
}

/// Strip the surrounding `"""` from a `@doc` block's raw argument, leaving the
/// Markdown body.
fn doc_body(raw: &str) -> &str {
    raw.strip_prefix("\"\"\"")
        .and_then(|s| s.strip_suffix("\"\"\""))
        .unwrap_or(raw)
}

/// Extract the code of each ` ```glyph @run ``` ` fenced block from a Markdown
/// body. The opening fence is a line whose backtick run is tagged `glyph` and
/// `@run`; the block ends at the next bare ``` ``` `` line.
fn extract_run_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        if t.starts_with("```") && t.contains("glyph") && t.contains("@run") {
            let mut code = String::new();
            for inner in lines.by_ref() {
                if inner.trim() == "```" {
                    break;
                }
                code.push_str(inner);
                code.push('\n');
            }
            blocks.push(code);
        }
    }
    blocks
}

fn decl_annotations(d: &Decl) -> &[glyph_ast::Annotation] {
    match d {
        Decl::Fn(x) => &x.annotations,
        Decl::Type(x) => &x.annotations,
        Decl::Const(x) => &x.annotations,
        Decl::Component(x) => &x.annotations,
        Decl::Import(_) => &[],
    }
}

/// The TypeScript harness: import each module's example functions, deep-compare
/// the two sides, and exit non-zero on any mismatch.
fn generate_harness(per_file: &[FileExamples]) -> String {
    let mut out = String::new();
    out.push_str("import \"./.glyph-runtime/glyph-bootstrap.ts\";\n");
    let with_tests: Vec<&FileExamples> = per_file
        .iter()
        .filter(|f| !f.cases.is_empty() || !f.runs.is_empty())
        .collect();
    for (k, fe) in with_tests.iter().enumerate() {
        out.push_str(&format!("import * as m{k} from \"./{}.ts\";\n", fe.module_path));
    }
    out.push_str(DEEP_EQUAL);
    out.push_str("let failed = 0;\nlet total = 0;\n");
    for (k, fe) in with_tests.iter().enumerate() {
        for (i, (l, r)) in fe.cases.iter().enumerate() {
            let label = js_string(&format!("{} example #{i}", fe.module_path));
            let detail = js_string(&format!("({}) != ({})", one_line(l), one_line(r)));
            out.push_str(&format!(
                "total++;\ntry {{\n  const __e = m{k}.__glyph_example_{i}();\n  \
                 if (!deepEqual(__e.lhs, __e.rhs)) {{ console.log(\"FAIL \" + {label} + \": \" + {detail}); failed++; }}\n\
                 }} catch (err) {{ console.log(\"FAIL \" + {label} + \": threw \" + String(err)); failed++; }}\n"
            ));
        }
        for i in 0..fe.runs.len() {
            let label = js_string(&format!("{} doc-run #{i}", fe.module_path));
            out.push_str(&format!(
                "total++;\ntry {{\n  m{k}.__glyph_run_{i}();\n\
                 }} catch (err) {{ console.log(\"FAIL \" + {label} + \": \" + String(err)); failed++; }}\n"
            ));
        }
    }
    out.push_str(
        "console.log(\"__GLYPH_EXAMPLES__ \" + total + \" \" + failed);\nprocess.exit(failed ? 1 : 0);\n",
    );
    out
}

/// A structural-equality helper used by the harness.
const DEEP_EQUAL: &str = r#"
function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((x, i) => deepEqual(x, b[i]));
  }
  const ao = a as Record<string, unknown>;
  const bo = b as Record<string, unknown>;
  // Ignore function-valued properties: a value's methods (e.g. Result's
  // map/map_err) are behavior, not data, and differ by instance.
  const ak = Object.keys(ao).filter((k) => typeof ao[k] !== "function");
  const bk = Object.keys(bo).filter((k) => typeof bo[k] !== "function");
  if (ak.length !== bk.length) return false;
  return ak.every((k) => Object.prototype.hasOwnProperty.call(bo, k) && deepEqual(ao[k], bo[k]));
}
"#;

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn js_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn module_path_of(rel: &Path) -> String {
    rel.with_extension("")
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

// ----- small fs helpers -----

fn read(path: &Path) -> Result<String, ExampleError> {
    std::fs::read_to_string(path).map_err(|e| ExampleError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

fn write(path: &Path, contents: &str) -> Result<(), ExampleError> {
    std::fs::write(path, contents).map_err(|e| ExampleError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

fn remove_dir_all(path: &Path) -> Result<(), ExampleError> {
    std::fs::remove_dir_all(path).map_err(|e| ExampleError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

fn collect_glyph_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), ExampleError> {
    for entry in std::fs::read_dir(dir).map_err(|e| ExampleError::Io {
        path: dir.to_path_buf(),
        source: e,
    })? {
        let entry = entry.map_err(|e| ExampleError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| ExampleError::Io {
            path: path.clone(),
            source: e,
        })?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            collect_glyph_files(&path, out)?;
        } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path);
        }
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), ExampleError> {
    std::fs::create_dir_all(to).map_err(|e| ExampleError::Io {
        path: to.to_path_buf(),
        source: e,
    })?;
    for entry in std::fs::read_dir(from).map_err(|e| ExampleError::Io {
        path: from.to_path_buf(),
        source: e,
    })? {
        let entry = entry.map_err(|e| ExampleError::Io {
            path: from.to_path_buf(),
            source: e,
        })?;
        let dest = to.join(entry.file_name());
        let ft = entry.file_type().map_err(|e| ExampleError::Io {
            path: entry.path(),
            source: e,
        })?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest).map_err(|e| ExampleError::Io {
                path: entry.path(),
                source: e,
            })?;
        }
    }
    Ok(())
}
