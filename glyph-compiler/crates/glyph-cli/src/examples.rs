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

/// A failed equality or a thrown example.
pub const E_EXAMPLE_FAILED: &str = "E0400";
/// A ` ```glyph @run ``` ` block that threw.
pub const E_DOC_RUN_FAILED: &str = "E0401";
/// An `@example` whose argument is not a Glyph expression.
pub const E_EXAMPLE_MALFORMED: &str = "E0402";
/// The augmented project carrying the tests did not compile.
pub const E_EXAMPLES_NOT_COMPILED: &str = "E0403";
/// The gate could not run at all (`tsx` is absent from `PATH`).
pub const E_EXAMPLES_NOT_RUN: &str = "E0404";

/// One failed test, with the declaration it belongs to kept as structure.
///
/// The runner has always known which declaration a failure is about:
/// `ExampleCase::decl` plus the module path is exactly the `module::name`
/// identity the rest of the compiler uses. It used to render that identity
/// into a sentence and hand the caller the sentence, so every machine-facing
/// consumer had to parse it back out of prose whose shape nothing guaranteed.
/// The identity travels as itself now; `message` is rendered from it, so the
/// human output is unchanged.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExampleFailure {
    /// Stable code for what went wrong (`E04xx`).
    pub code: String,
    /// The `module::name` of the declaration this test sits on, in the same
    /// spelling a `--json` diagnostic's `entity` uses. `None` for a failure
    /// that belongs to no single declaration (the gate itself could not run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    /// `"example"`, `"doc-run"`, or `"gate"`.
    pub kind: String,
    /// 1-based position among that declaration's own tests of this kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nth: Option<usize>,
    /// What went wrong, carrying no identity prefix of its own.
    pub detail: String,
    /// The one-line rendering the text path prints.
    pub message: String,
}

impl ExampleFailure {
    /// A failure that belongs to no declaration: the gate itself could not run,
    /// or the augmented project did not compile.
    pub fn gate(code: &str, detail: String) -> Self {
        ExampleFailure {
            code: code.to_string(),
            entity: None,
            kind: "gate".to_string(),
            nth: None,
            message: detail.clone(),
            detail,
        }
    }

    fn malformed(module_path: &str, decl: &str, args: &str) -> Self {
        let entity = qualified(module_path, decl);
        let detail = format!("malformed @example `{args}`");
        ExampleFailure {
            code: E_EXAMPLE_MALFORMED.to_string(),
            message: format!("{entity}: {detail}"),
            entity: Some(entity),
            kind: "example".to_string(),
            nth: None,
            detail,
        }
    }

    /// A `FAIL` line the harness printed that no entry in the identity table
    /// claims. Only reachable if the generator and the parser below disagree,
    /// which is why the line is still reported rather than dropped.
    fn unattributed(detail: String) -> Self {
        ExampleFailure {
            code: E_EXAMPLE_FAILED.to_string(),
            entity: None,
            kind: "example".to_string(),
            nth: None,
            message: detail.clone(),
            detail,
        }
    }
}

/// Outcome of running a project's `@example`s.
#[derive(Debug, Default)]
pub struct ExampleReport {
    /// Total examples found across the project.
    pub total: usize,
    /// Every failure: a failed equality, a thrown example or `@run` block, or a
    /// malformed `@example` that did not parse.
    pub failures: Vec<ExampleFailure>,
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
    /// Every `@example` in the file, in declaration order.
    cases: Vec<ExampleCase>,
    /// Every ` ```glyph @run ``` ` block in a `@doc` (D26), in declaration order.
    runs: Vec<DocRun>,
    /// Every `@example` whose argument did not parse.
    malformed: Vec<MalformedExample>,
}

/// One `@example`, with the declaration it sits above.
///
/// A failure has to say which declaration it is about, so the binding the AST
/// already carries is kept rather than flattened away. `nth` counts within that
/// one declaration, which is what keeps a label stable: an `@example` added to
/// a different declaration used to renumber every label after it.
struct ExampleCase {
    /// The declaration the annotation is attached to, e.g. `triple`.
    decl: String,
    /// 1-based position among that declaration's own `@example`s.
    nth: usize,
    /// The left side of the equality, rendered back to Glyph text.
    lhs: String,
    /// The right side. A non-equality `@example` asserts its expression is
    /// `true`, and carries `true` here.
    rhs: String,
}

/// One ` ```glyph @run ``` ` block, with the declaration whose `@doc` holds it.
struct DocRun {
    decl: String,
    /// 1-based position among that declaration's own `@run` blocks.
    nth: usize,
    /// The block's Glyph code.
    code: String,
}

/// An `@example` whose argument did not parse, with the declaration it sits
/// above.
struct MalformedExample {
    decl: String,
    /// The annotation's raw argument text, verbatim.
    args: String,
}

/// A declaration's identity, the way the rest of the compiler names one:
/// `orders/pricing::total`. Glyph's module namespace is flat and single, so
/// `module::name` is unique across declaration kinds.
fn qualified(module_path: &str, decl: &str) -> String {
    format!("{module_path}::{decl}")
}

/// The identity of one test in the generated harness, in the order the harness
/// runs them.
///
/// The harness reports a failure by its index into this table rather than by
/// printing a label, so the identity crosses the process boundary as itself
/// and is never re-parsed out of the line `tsx` printed.
struct TestId {
    code: &'static str,
    /// `orders/pricing::total`.
    entity: String,
    /// `"example"` or `"doc-run"`.
    kind: &'static str,
    nth: usize,
}

impl TestId {
    /// How a failure names this test: `orders/pricing::total example #2`.
    fn label(&self) -> String {
        format!("{} {} #{}", self.entity, self.kind, self.nth)
    }

    fn failure(&self, detail: String) -> ExampleFailure {
        ExampleFailure {
            code: self.code.to_string(),
            entity: Some(self.entity.clone()),
            kind: self.kind.to_string(),
            nth: Some(self.nth),
            message: format!("{}: {detail}", self.label()),
            detail,
        }
    }
}

/// Walk the project and collect every file that carries a test, along with the
/// number of executable tests and of malformed `@example`s. Parsing only; it
/// neither copies the project nor builds anything.
fn collect_project(src: &Path) -> Result<(Vec<FileExamples>, usize, usize), ExampleError> {
    let mut files = Vec::new();
    collect_glyph_files(src, src, &mut files)?;
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
    Ok((per_file, total, malformed_total))
}

/// How many executable tests (`@example` equalities plus `@doc @run` blocks)
/// the project rooted at `src` carries. Used to report what `--no-test` skips
/// without paying for a run.
pub fn count_examples(src: &Path) -> Result<usize, ExampleError> {
    let (_, total, _) = collect_project(src)?;
    Ok(total)
}

/// Run every `@example` in the project rooted at `src`.
pub fn run_examples(src: &Path) -> Result<ExampleReport, ExampleError> {
    let (per_file, total, malformed_total) = collect_project(src)?;

    let mut report = ExampleReport {
        total,
        ran: true,
        ..Default::default()
    };
    for fe in &per_file {
        for m in &fe.malformed {
            report
                .failures
                .push(ExampleFailure::malformed(&fe.module_path, &m.decl, &m.args));
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
    // Remove it again on the way out, whichever way we leave. Every path below
    // can return early through `?`, so cleanup cannot sit at the end of the
    // function: it has to be a drop. Without this the copy survived the run,
    // and since the gate is on by default for `glyph check` and `glyph build`,
    // a project with one `@example` left a full copy of itself in the temp
    // directory on every invocation. Four thousand of them had accumulated on
    // one machine before anybody looked.
    let _cleanup = TempProject(root.clone());
    copy_dir(src, &tsrc)?;
    for fe in &per_file {
        if fe.cases.is_empty() && fe.runs.is_empty() {
            continue;
        }
        let path = tsrc.join(&fe.rel);
        let mut text = read(&path)?;
        // These synthesized functions are imported by the external harness, so
        // they must be `pub` to export (0.1.16 module-private default).
        for (i, case) in fe.cases.iter().enumerate() {
            text.push_str(&format!(
                "\npub fn __glyph_example_{i}() {{\n  return {{ lhs: {}, rhs: {} }}\n}}\n",
                case.lhs, case.rhs
            ));
        }
        for (i, run) in fe.runs.iter().enumerate() {
            text.push_str(&format!(
                "\npub fn __glyph_run_{i}() -> void {{\n{}\n}}\n",
                run.code
            ));
        }
        write(&path, &text)?;
    }

    let build = build_project_inner(&tsrc, &tout, false)?;
    if build.has_errors() {
        report.build_failed = Some(build.diagnostics);
        return Ok(report);
    }

    // Generate and run the harness. `ids` is how a `FAIL` line maps back to
    // the declaration it is about.
    let (harness, ids) = generate_harness(&per_file);
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
                let Some(rest) = line.strip_prefix("FAIL ") else {
                    continue;
                };
                // `<index>\t<detail>`: the index is ours, minted alongside the
                // harness, so the identity is looked up rather than recovered
                // from the text. Splitting on the first tab only, because a
                // thrown error's own message may carry one.
                let attributed = match rest.split_once('\t') {
                    Some((index, detail)) => index
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| ids.get(i))
                        .map(|id| id.failure(detail.to_string())),
                    None => None,
                };
                report.failures.push(
                    attributed
                        .unwrap_or_else(|| ExampleFailure::unattributed(rest.to_string())),
                );
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

/// For a module, collect its `@example`s, its `@doc` `@run` blocks, and any
/// `@example` whose argument failed to parse. Each one keeps the declaration it
/// was written above, and is numbered within that declaration.
fn collect_tests(
    module: &glyph_ast::Module,
) -> (Vec<ExampleCase>, Vec<DocRun>, Vec<MalformedExample>) {
    let mut cases = Vec::new();
    let mut runs = Vec::new();
    let mut malformed = Vec::new();
    for decl in &module.items {
        let Some((name, annotations)) = decl_target(decl) else {
            continue;
        };
        let mut examples_here = 0;
        let mut runs_here = 0;
        for ann in annotations {
            match ann.name.as_ref() {
                "example" => {
                    // Position among this declaration's `@example`s, counted
                    // whether or not the argument parses, so one that does not
                    // does not shift the numbering of the rest.
                    examples_here += 1;
                    let (lhs, rhs) = match glyph_parser::parse_expression(&ann.raw_args) {
                        Ok(Expr::Binary {
                            op: glyph_ast::BinOp::Eq,
                            left,
                            right,
                            ..
                        }) => (format_expr(&left), format_expr(&right)),
                        // A non-equality example asserts the expression is `true`.
                        Ok(other) => (format_expr(&other), "true".to_string()),
                        Err(_) => {
                            malformed.push(MalformedExample {
                                decl: name.to_string(),
                                args: ann.raw_args.clone(),
                            });
                            continue;
                        }
                    };
                    cases.push(ExampleCase {
                        decl: name.to_string(),
                        nth: examples_here,
                        lhs,
                        rhs,
                    });
                }
                "doc" => {
                    for code in extract_run_blocks(doc_body(&ann.raw_args)) {
                        runs_here += 1;
                        runs.push(DocRun {
                            decl: name.to_string(),
                            nth: runs_here,
                            code,
                        });
                    }
                }
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

/// A declaration's name together with its annotations. `None` for an `import`,
/// which declares no name of its own and carries no annotations, so there is no
/// case where an annotation is found with no declaration to attribute it to.
fn decl_target(d: &Decl) -> Option<(&str, &[glyph_ast::Annotation])> {
    match d {
        Decl::Fn(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Type(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Const(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Component(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Interface(x) => Some((x.name.as_ref(), &x.annotations)),
        Decl::Import(_) => None,
    }
}

/// The TypeScript harness: import each module's example functions, deep-compare
/// the two sides, and exit non-zero on any mismatch.
fn generate_harness(per_file: &[FileExamples]) -> (String, Vec<TestId>) {
    let mut out = String::new();
    let mut ids: Vec<TestId> = Vec::new();
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
        for (i, case) in fe.cases.iter().enumerate() {
            let t = ids.len();
            ids.push(TestId {
                code: E_EXAMPLE_FAILED,
                entity: qualified(&fe.module_path, &case.decl),
                kind: "example",
                nth: case.nth,
            });
            let detail = js_string(&format!(
                "({}) != ({})",
                one_line(&case.lhs),
                one_line(&case.rhs)
            ));
            out.push_str(&format!(
                "total++;\ntry {{\n  const __e = m{k}.__glyph_example_{i}();\n  \
                 if (!deepEqual(__e.lhs, __e.rhs)) {{ console.log(\"FAIL {t}\\t\" + {detail}); failed++; }}\n\
                 }} catch (err) {{ console.log(\"FAIL {t}\\tthrew \" + String(err)); failed++; }}\n"
            ));
        }
        for (i, run) in fe.runs.iter().enumerate() {
            let t = ids.len();
            ids.push(TestId {
                code: E_DOC_RUN_FAILED,
                entity: qualified(&fe.module_path, &run.decl),
                kind: "doc-run",
                nth: run.nth,
            });
            out.push_str(&format!(
                "total++;\ntry {{\n  m{k}.__glyph_run_{i}();\n\
                 }} catch (err) {{ console.log(\"FAIL {t}\\t\" + String(err)); failed++; }}\n"
            ));
        }
    }
    out.push_str(
        "console.log(\"__GLYPH_EXAMPLES__ \" + total + \" \" + failed);\nprocess.exit(failed ? 1 : 0);\n",
    );
    (out, ids)
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

/// Removes the throwaway project copy when the run leaves, successfully or not.
///
/// A failure to clean up is deliberately silent: the run's own result is what
/// the caller asked for, and losing it to report a temp-directory problem would
/// trade a real answer for a janitorial one.
struct TempProject(std::path::PathBuf);

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn remove_dir_all(path: &Path) -> Result<(), ExampleError> {
    std::fs::remove_dir_all(path).map_err(|e| ExampleError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Same boundary rule as the build walk: a nested Glyph project (D41) is not
/// part of the enclosing project's compilation, so its `@example`s are not run
/// from here either (they would not resolve against this root).
fn collect_glyph_files(
    dir: &Path,
    project_root: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), ExampleError> {
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
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path != project_root && crate::config::is_project_root(&path) {
                continue;
            }
            collect_glyph_files(&path, project_root, out)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Two functions, one `@example` each. `triple`'s is deliberately wrong:
    /// nothing here executes it, the tests read the label the harness would
    /// print when it fails.
    const ONE_EACH: &str = "\
@example double(2) == 4
fn double(n: number) -> number { return n * 2 }

@example triple(2) == 7
fn triple(n: number) -> number { return n * 3 }
";

    /// The same file with one more `@example` added to `double`.
    const TWO_ON_DOUBLE: &str = "\
@example double(2) == 4
@example double(3) == 6
fn double(n: number) -> number { return n * 2 }

@example triple(2) == 7
fn triple(n: number) -> number { return n * 3 }
";

    fn file_examples(module_path: &str, source: &str) -> FileExamples {
        let module = glyph_parser::parse(source).expect("the test source parses");
        let (cases, runs, malformed) = collect_tests(&module);
        FileExamples {
            rel: PathBuf::from(format!("{module_path}.glyph")),
            module_path: module_path.to_string(),
            cases,
            runs,
            malformed,
        }
    }

    /// Every `FAIL` line the generated harness can print, as `(label, the
    /// static part of the detail)`. The label is *not* read out of the harness
    /// any more: the harness prints an index, and the label comes from the
    /// identity table minted beside it, which is exactly the path
    /// `run_examples` takes. Assumes no test expression contains `);`.
    fn fail_lines(source: &str) -> Vec<(String, String)> {
        let (harness, ids) = generate_harness(&[file_examples("main", source)]);
        let mut out = Vec::new();
        for seg in harness.split("console.log(\"FAIL ").skip(1) {
            let call = &seg[..seg.find(");").expect("a complete console.log call")];
            // The opening quote was consumed by the split, so the first piece
            // of a `"` split is still inside the literal, and every second
            // piece after it is another literal.
            let mut pieces = call.split('"');
            let head = pieces.next().expect("the FAIL literal continues");
            let (idx, first) = head
                .split_once("\\t")
                .expect("the index is tab-separated from the detail");
            let id = &ids[idx.parse::<usize>().expect("a numeric test index")];
            let mut detail = first.to_string();
            for (i, piece) in pieces.enumerate() {
                if i % 2 == 1 {
                    detail.push_str(piece);
                }
            }
            out.push((id.label(), detail));
        }
        out
    }

    /// The label of the one `FAIL` line whose message mentions `needle`.
    fn label_of(source: &str, needle: &str) -> String {
        let lines = fail_lines(source);
        match lines.iter().find(|(_, msg)| msg.contains(needle)) {
            Some((label, _)) => label.clone(),
            None => panic!("no FAIL line mentions `{needle}`: {lines:?}"),
        }
    }

    #[test]
    fn an_example_failure_names_the_declaration_it_is_about() {
        let mut labels: Vec<String> = fail_lines(ONE_EACH).into_iter().map(|(l, _)| l).collect();
        labels.dedup();
        let expected = ["main::double example #1", "main::triple example #1"];
        assert_eq!(labels, expected);
    }

    /// The identity a failure carries is structure, not a sentence to parse.
    /// `--json` consumers read `entity`/`nth`/`code` off the failure; the
    /// prose `message` is rendered from those, not the other way round.
    #[test]
    fn a_failure_carries_the_declaration_as_structure() {
        let (_harness, ids) = generate_harness(&[file_examples("orders/pricing", ONE_EACH)]);
        let f = ids[1].failure("(triple(2)) != (7)".to_string());
        assert_eq!(f.entity.as_deref(), Some("orders/pricing::triple"));
        assert_eq!(f.nth, Some(1));
        assert_eq!(f.kind, "example");
        assert_eq!(f.code, E_EXAMPLE_FAILED);
        assert_eq!(f.detail, "(triple(2)) != (7)");
        assert_eq!(
            f.message,
            "orders/pricing::triple example #1: (triple(2)) != (7)"
        );
    }

    #[test]
    fn an_example_added_to_another_declaration_does_not_relabel_this_one() {
        let before = label_of(ONE_EACH, "triple(2)");
        let after = label_of(TWO_ON_DOUBLE, "triple(2)");
        assert_eq!(
            after, before,
            "an @example added to `double` renumbered `triple`'s label"
        );
        assert_eq!(before, "main::triple example #1");
    }
}
