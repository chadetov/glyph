//! Integration tests for `glyph build`.
//!
//! Each test writes a small multi-file fixture to a unique temp
//! directory, calls `build_project` directly (no subprocess), and asserts
//! on the `BuildReport`. Cleanup is best-effort — `std::env::temp_dir()`
//! is the OS temp dir, periodically cleaned by the system.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_cli::{build_project, build::build_project_inner};

/// Build a uniquely-named temp directory rooted at the OS temp dir.
/// Returns the path; the test is responsible for not relying on
/// cleanup. Uniqueness comes from `process::id()` plus a strictly
/// monotonic per-process counter — using wall-clock nanoseconds would
/// invite collisions when two tests happen to fire inside the same
/// nanosecond, sharing a temp dir and stomping each other's fixtures.
fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_cli_test_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Write a file with `text` at `dir/relpath`, creating parent dirs.
fn write_file(dir: &Path, relpath: &str, text: &str) {
    let p = dir.join(relpath);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(&p, text).expect("write file");
}

#[test]
fn build_reports_no_diagnostics_on_clean_project() {
    let root = unique_tmp("clean");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "lib.glyph",
        "module lib\nfn helper() -> number { return 1 }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\nimport lib { helper }\nfn main() -> number { return helper() }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "expected no diagnostics; got: {:?}",
        report.diagnostics
    );
    assert_eq!(report.modules.len(), 2);
    assert!(out.exists(), "out/ should be created");
}

#[test]
fn build_emits_typescript_for_a_clean_module() {
    let root = unique_tmp("emit");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn add(a: number, b: number) -> number { return a + b }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    assert_eq!(report.emitted, vec!["main.ts".to_string()]);

    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    assert!(
        ts.contains("export function add(a: number, b: number): number {"),
        "{ts}"
    );
    assert!(ts.contains("return (a + b);"), "{ts}");
}

#[test]
fn build_reports_emit_diagnostic_for_unsupported_construct() {
    let root = unique_tmp("emit_unsupported");
    let src = root.join("src");
    let out = root.join("dist");
    // A value-position block-body match arm is a later week-4 day; the build
    // should surface a diagnostic and NOT write a .ts file for this module.
    write_file(
        &src,
        "main.glyph",
        "module main\ntype E = A | B\nfn f(e: E) -> number {\n  let x = match e {\n    A => { return 0 },\n    B => { return 1 },\n  }\n  return x\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(report.has_errors(), "expected an emit diagnostic");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("emit")),
        "diags: {:?}",
        report.diagnostics
    );
    assert!(report.emitted.is_empty());
    assert!(!out.join("main.ts").exists(), "no .ts for a rejected module");
}

#[test]
fn build_flags_unknown_cross_module_export() {
    let root = unique_tmp("badimport");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "lib.glyph",
        "module lib\nfn helper() -> number { return 1 }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\nimport lib { helper, bogus }\nfn run() -> number { return helper() }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("bogus")),
        "expected a diagnostic mentioning `bogus`; got: {:?}",
        report.diagnostics
    );
}

#[test]
fn build_recurses_into_subdirectories() {
    let root = unique_tmp("subdir");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "lib/users.glyph",
        "module lib/users\nfn find() -> number { return 1 }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\nimport lib/users { find }\nfn run() -> number { return find() }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "expected no diagnostics; got: {:?}",
        report.diagnostics
    );
    assert!(
        report.modules.iter().any(|m| m == "lib/users"),
        "modules: {:?}",
        report.modules
    );
}

#[test]
fn build_fails_for_missing_src_directory() {
    let root = unique_tmp("missing");
    let bad_src = root.join("does_not_exist");
    let out = root.join("dist");
    let err = build_project(&bad_src, &out).expect_err("should fail");
    assert!(
        matches!(err, glyph_cli::BuildError::SrcMissing(_)),
        "got: {err:?}"
    );
}

#[test]
fn build_fails_for_empty_directory() {
    let root = unique_tmp("empty");
    let src = root.join("src");
    let out = root.join("dist");
    std::fs::create_dir_all(&src).unwrap();
    let err = build_project(&src, &out).expect_err("empty dir should fail");
    assert!(matches!(err, glyph_cli::BuildError::NoSources(_)), "got: {err:?}");
}

#[test]
fn diagnostics_include_source_context_via_ariadne() {
    // Day-13 acceptance: instead of a one-line `app.glyph: import: ...`,
    // diagnostics now show the failing source line with a caret pointer.
    // We run with color disabled so the assertions are stable across
    // terminals and CI environments.
    let root = unique_tmp("ariadne");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "lib.glyph",
        "module lib\nfn helper() -> number { return 1 }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\nimport lib { helper, bogus }\nfn run() -> number { return helper() }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert_eq!(report.diagnostics.len(), 1, "diagnostics: {:?}", report.diagnostics);
    let d = &report.diagnostics[0];
    // The message itself.
    assert!(d.contains("bogus"), "missing offending name in:\n{d}");
    assert!(d.contains("import"), "missing stage tag in:\n{d}");
    // The source path appears in ariadne's location header.
    assert!(d.contains("app"), "missing path in:\n{d}");
    // The actual source line should appear — that's the whole point of
    // ariadne rendering. With color disabled, the line text is literal.
    assert!(
        d.contains("import lib { helper, bogus }"),
        "missing source line in:\n{d}"
    );
}

#[test]
fn build_flags_non_exhaustive_match_on_tagged_union() {
    // Day-14 acceptance: typechecker diagnostics flow through
    // type_map → BuildReport. A non-exhaustive match on a tagged union
    // surfaces in `glyph build` output.
    let root = unique_tmp("nonexhaustive");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         type Feed = | Loading | Loaded | Failed\n\
         fn show(f: Feed) -> number {\n  \
           return match f {\n    \
             Loading => 1,\n    \
             Loaded => 2,\n  \
           }\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("Feed") && d.contains("Failed")),
        "expected non-exhaustive match diagnostic mentioning Feed + Failed; got: {:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| d.contains("typecheck")),
        "expected `typecheck` stage tag; got: {:?}",
        report.diagnostics
    );
}

#[test]
fn build_flags_question_operator_outside_result_fn() {
    // Day-15 acceptance: the `?`-operator typing rule flows through
    // type_map → BuildReport. A `?` in a function that does not return
    // `Result` surfaces in `glyph build` output.
    let root = unique_tmp("question");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         fn unwrap(r: Result<string, string>) -> number {\n  \
           let v = r?\n  \
           return 1\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("typecheck") && d.contains("`?`")),
        "expected a `?`-operator typecheck diagnostic; got: {:?}",
        report.diagnostics
    );
}

#[test]
fn build_flags_non_exhaustive_prelude_result_match() {
    // Day-19 acceptance: a `match` over a prelude `Result` (here imported,
    // as the example files do) that misses a variant surfaces through
    // type_map → BuildReport.
    let root = unique_tmp("preludeexhaustive");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         import std/result { Result, Ok, Err }\n\
         fn run(r: Result<number, string>) -> number {\n  \
           return match r {\n    \
             Ok(n) => n,\n  \
           }\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("typecheck") && d.contains("Result") && d.contains("Err")),
        "expected a non-exhaustive Result diagnostic mentioning Err; got: {:?}",
        report.diagnostics
    );
}

#[test]
fn build_flags_return_type_mismatch() {
    // Day-21 acceptance: a `return` whose value is a concrete primitive
    // that differs from the declared primitive return type surfaces through
    // type_map -> BuildReport.
    let root = unique_tmp("returnmismatch");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\nfn count() -> number {\n  return \"nope\"\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("typecheck")
                && d.contains("expected `number`")
                && d.contains("found `string`")),
        "expected a return type-mismatch diagnostic; got: {:?}",
        report.diagnostics
    );
}

#[test]
fn build_skips_hidden_and_target_directories() {
    let root = unique_tmp("skipped");
    let src = root.join("src");
    let out = root.join("dist");
    // A real source file that should be checked.
    write_file(
        &src,
        "main.glyph",
        "module app\nfn main() -> number { return 1 }\n",
    );
    // Files under skipped roots — if the walker descended into them the
    // build would fail on the deliberately-malformed source.
    write_file(&src, ".git/decoy.glyph", "module decoy\nfn main(\n");
    write_file(&src, "target/decoy.glyph", "module decoy\nfn main(\n");

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "decoy files under .git/ and target/ should be skipped; got: {:?}",
        report.diagnostics
    );
    assert_eq!(
        report.modules,
        vec!["main".to_string()],
        "only the real source should be visited"
    );
}

#[test]
fn repo_examples_emit_typescript_without_diagnostics() {
    // Every program under the repo's `examples/` tree — the four hard-case
    // examples plus the self-contained `corpus/` programs — must build and emit
    // TypeScript with no diagnostics. This is the Phase 1 Week 4 emission gate;
    // it guards against an emitter regression silently breaking an example.
    let examples = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../examples"));
    assert!(
        examples.is_dir(),
        "examples dir not found at {examples:?}"
    );
    let out = unique_tmp("examples").join("dist");

    let report = build_project_inner(examples, &out, false).expect("build examples ok");
    assert!(
        !report.has_errors(),
        "examples produced diagnostics: {:?}",
        report.diagnostics
    );
    // Every clean module emits a `.ts`, so emitted count matches module count.
    assert_eq!(
        report.emitted.len(),
        report.modules.len(),
        "every checked module should emit; modules={:?} emitted={:?}",
        report.modules,
        report.emitted
    );
    // The four canonical hard-case examples specifically must be present.
    for name in [
        "01_validator.ts",
        "02_async_errors.ts",
        "03_react_component.ts",
        "04_cli_tool.ts",
    ] {
        assert!(
            report.emitted.iter().any(|e| e == name),
            "missing {name} in emitted: {:?}",
            report.emitted
        );
    }
    // The corpus is exercised too.
    assert!(
        report.emitted.iter().any(|e| e == "corpus/shapes.ts"),
        "corpus not emitted: {:?}",
        report.emitted
    );
    // The build is self-checking: it writes the runtime, a generated tsconfig,
    // and the examples' external (`.types/`) stubs so `tsc -p` can type it.
    assert!(out.join("tsconfig.json").is_file(), "tsconfig.json missing");
    assert!(
        out.join(".glyph-runtime/std/result.ts").is_file(),
        "bundled runtime missing"
    );
    assert!(
        out.join(".types/glyph-externals.d.ts").is_file(),
        "examples/.types not copied into the output"
    );
}

#[test]
fn build_writes_the_runtime_and_a_tsconfig() {
    let root = unique_tmp("support");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn add(a: number, b: number) -> number { return a + b }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    // The generated config and bundled runtime sit next to the emitted output.
    assert!(out.join("tsconfig.json").is_file(), "tsconfig.json");
    for rel in [
        ".glyph-runtime/std/result.ts",
        ".glyph-runtime/std/option.ts",
        ".glyph-runtime/std/schema.ts",
        ".glyph-runtime/glyph-prelude.d.ts",
        ".glyph-runtime/glyph-stdlib.d.ts",
    ] {
        assert!(out.join(rel).is_file(), "missing bundled runtime file {rel}");
    }
}

#[test]
fn build_copies_src_types_into_the_output() {
    let root = unique_tmp("dottypes");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn f() -> number { return 1 }\n",
    );
    // A project supplies ambient declarations for its external deps in
    // `<src>/.types/`; the build copies them alongside the output.
    write_file(
        &src,
        ".types/ext.d.ts",
        "declare module \"ext\" { export const x: number; }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    assert!(
        out.join(".types/ext.d.ts").is_file(),
        ".types/ not copied into the output"
    );
}

#[test]
fn run_executes_main_and_propagates_exit_code() {
    // A program's `main(argv) -> number` return value becomes the process exit
    // code. Requires `tsx` (and node) on PATH; when absent the run is skipped so
    // CI without a JS toolchain stays green.
    let root = unique_tmp("run");
    write_file(
        &root,
        "runprog.glyph",
        "module runprog\nfn main(argv: Array<string>) -> number {\n  return 7\n}\n",
    );
    let file = root.join("runprog.glyph");
    match glyph_cli::run::run_file(&file, &[], false).expect("run_file ok") {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 7, "main's return value should be the exit code");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping run assertion: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
    }
}

#[test]
fn run_reports_build_failure_for_a_broken_target() {
    // A non-exhaustive match makes the module fail to compile, so it never
    // emits and the program is never run. This path is reached before `tsx` is
    // invoked, so it holds with or without a JS toolchain.
    let root = unique_tmp("runbad");
    write_file(
        &root,
        "brokenprog.glyph",
        "module brokenprog\n\
         type Feed = | Loading | Loaded | Failed\n\
         fn pick(f: Feed) -> number {\n  return match f {\n    Loading => 1,\n  }\n}\n\
         fn main(argv: Array<string>) -> number {\n  return 0\n}\n",
    );
    let file = root.join("brokenprog.glyph");
    match glyph_cli::run::run_file(&file, &[], false).expect("run_file ok") {
        glyph_cli::run::RunOutcome::BuildFailed(report) => {
            assert!(
                !report.diagnostics.is_empty(),
                "a build failure should carry diagnostics"
            );
        }
        glyph_cli::run::RunOutcome::Ran(code) => {
            panic!("a broken program should not run; got exit {code}");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            panic!("build failure must be detected before invoking tsx");
        }
    }
}
