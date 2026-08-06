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
/// The pair is unique within a run but repeats across runs (the OS reuses
/// pids and the counter restarts), and nothing cleans these up, so the
/// directory is removed before it is created: an appending test that
/// inherited a previous run's file saw doubled content and failed.
fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_cli_test_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
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
        "module lib\npub fn helper() -> number { return 1 }\n",
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
fn build_warns_on_dropped_result_but_still_emits() {
    let root = unique_tmp("mustuse");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/fs { write_text }\n\
         fn save() -> number {\n\
         \x20 write_text(\"a.txt\", \"hi\")\n\
         \x20 return 0\n\
         }\n\
         fn main(argv: Array<string>) -> number { return save() }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    // A dropped `Result` is a warning (E0217), not an error: the build
    // succeeds, reports one warning, and still emits the TypeScript.
    assert!(
        !report.has_errors(),
        "must-use is a warning, not an error: {:?}",
        report.diagnostics
    );
    assert_eq!(
        report.warning_count(),
        1,
        "one must-use warning expected: {:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0217")),
        "{:?}",
        report.diagnostics
    );
    assert_eq!(
        report.emitted,
        vec!["main.ts".to_string()],
        "warnings do not block emission"
    );
}

#[test]
fn build_writes_a_v3_source_map() {
    let root = unique_tmp("srcmap");
    let src = root.join("src");
    let out = root.join("out");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn f() -> number {\n  return 1\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    // The emitted `.ts` links its sidecar map.
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    assert!(
        ts.contains("//# sourceMappingURL=main.ts.map"),
        "sourceMappingURL comment: {ts}"
    );
    // The map is a v3 map that names the Glyph source and embeds it.
    let map = std::fs::read_to_string(out.join("main.ts.map")).unwrap();
    assert!(map.contains("\"version\":3"), "v3: {map}");
    assert!(map.contains("\"sources\":[\"main.glyph\"]"), "names the source: {map}");
    assert!(map.contains("fn f"), "embeds sourcesContent: {map}");
    // Non-empty mappings (not the empty-string field).
    assert!(!map.contains("\"mappings\":\"\""), "non-empty mappings: {map}");
}

#[test]
fn build_produces_structured_diagnostics() {
    // Every diagnostic has a structured form (for `--json`): a stable code,
    // severity, stage, and a 1-based line/col range.
    let root = unique_tmp("structured");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\ntype F = A | B\nfn f(x: F) -> number {\n  return match x {\n    A => 1,\n  }\n}\nfn main(argv: Array<string>) -> number { return 0 }\n",
    );

    let report = build_project_inner(&src, &root.join("out"), false).expect("build");
    assert!(report.has_errors());
    // Structured and rendered diagnostics stay in lockstep.
    assert_eq!(report.structured.len(), report.diagnostics.len());
    let d = report
        .structured
        .iter()
        .find(|d| d.code == "E0200")
        .expect("a structured E0200");
    assert_eq!(d.severity, "error");
    assert_eq!(d.stage, "typecheck");
    assert!(d.range.start.line >= 1 && d.range.start.col >= 1, "1-based pos");
    assert!(d.help.is_some(), "carries the help");
}

#[test]
fn build_emits_typescript_for_a_clean_module() {
    let root = unique_tmp("emit");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\npub fn add(a: number, b: number) -> number { return a + b }\n",
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
fn record_descriptor_rejects_extra_keys_unless_open() {
    // C3b: a record's runtime descriptor is strict by default — its `is`/`parse`
    // reject a value with keys the type doesn't declare. `@open` opts out.
    let root = unique_tmp("strict");
    let out = root.join("dist");
    write_file(
        &root.join("src"),
        "main.glyph",
        "module main\n\
         pub type Point = { x: number, y: number }\n\
         @open\n\
         pub type Loose = { x: number }\n",
    );
    let report = build_project_inner(&root.join("src"), &out, false).expect("build ok");
    assert!(!report.has_errors(), "clean: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();

    // Point (strict) checks the key set; Loose (@open) does not.
    let point = ts.split("export const Point").nth(1).unwrap_or("");
    let point_is = point.split("parse(").next().unwrap_or("");
    assert!(point_is.contains("Object.keys"), "strict record checks its keys: {point_is}");
    let loose = ts.split("export const Loose").nth(1).unwrap_or("");
    let loose_is = loose.split("parse(").next().unwrap_or("");
    assert!(!loose_is.contains("Object.keys"), "@open record does not check keys: {loose_is}");
}

#[test]
fn redact_masks_fields_in_the_descriptor_and_flags_unknown_names() {
    // D24: `@redact fields: [...]` emits a `redact(value)` on the descriptor that
    // masks those fields, and an unknown field name is E0219.
    let root = unique_tmp("redact");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         @redact fields: [ssn]\n\
         type User = {\n  name: string,\n  ssn: string,\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "clean: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    assert!(ts.contains("redact(value: User)"), "emits a redact method: {ts}");
    assert!(ts.contains("\"ssn\": \"[REDACTED]\""), "masks the field: {ts}");
    // The other members are untouched — the redact method is additive.
    assert!(ts.contains("is(value: unknown): value is User"), "is intact: {ts}");
    assert!(ts.contains("schema:"), "schema intact: {ts}");

    // An unknown redacted field name is E0219.
    let bad = unique_tmp("redact_bad");
    write_file(
        &bad.join("src"),
        "main.glyph",
        "module main\n@redact fields: [sssn]\ntype U = {\n  ssn: string,\n}\n",
    );
    let report = build_project_inner(&bad.join("src"), &bad.join("out"), false).expect("build");
    assert!(report.has_errors(), "unknown redact field is an error");
    assert!(report.diagnostics.iter().any(|d| d.contains("E0219")), "{:?}", report.diagnostics);
}

#[test]
fn rebuild_prunes_a_renamed_modules_stale_ts_and_map() {
    // Build a module, then rename it and rebuild into the same out dir. The old
    // `.ts` and its `.ts.map` sidecar must be gone (no orphan tsc picks up),
    // while an unrelated file the user placed in the out dir is preserved.
    let root = unique_tmp("prune");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "old.glyph", "module old\nfn f() -> number { return 1 }\n");
    build_project_inner(&src, &out, false).expect("first build");
    assert!(out.join("old.ts").exists(), "old.ts emitted");
    assert!(out.join("old.ts.map").exists(), "old.ts.map emitted");

    // A file the user dropped in the out dir must survive the prune.
    std::fs::write(out.join("keep.me"), "hand-written").unwrap();

    // Rename the module and rebuild.
    std::fs::remove_file(src.join("old.glyph")).unwrap();
    write_file(&src, "new.glyph", "module new\nfn f() -> number { return 2 }\n");
    build_project_inner(&src, &out, false).expect("second build");

    assert!(out.join("new.ts").exists(), "new.ts emitted");
    assert!(!out.join("old.ts").exists(), "stale old.ts pruned");
    assert!(!out.join("old.ts.map").exists(), "stale old.ts.map pruned");
    assert!(out.join("keep.me").exists(), "unrelated user file preserved");
}

#[test]
fn regen_refreshes_generated_files_from_the_spec() {
    // `glyph gen openapi` records its full invocation in the output; `glyph
    // regen` recovers it and re-runs, so a spec change flows into the committed
    // Glyph without remembering the command. Absolute paths keep it
    // cwd-independent. openapi needs no external tools.
    let root = unique_tmp("regen");
    let spec = root.join("api.yaml");
    let out = root.join("src/api");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &spec,
        "openapi: 3.0.0\ninfo: { title: T, version: 1.0.0 }\n\
         components:\n  schemas:\n    Task:\n      type: object\n      properties:\n        \
         id: { type: integer }\n",
    )
    .unwrap();

    glyph_cli::gen::openapi(&spec, &out, false, false).expect("initial gen");
    let gen_file = out.join("api.glyph");
    let first = std::fs::read_to_string(&gen_file).unwrap();
    assert!(first.contains("Regenerate with"), "provenance header: {first}");
    assert!(!first.contains("title"), "spec has no title field yet");

    // Add a `title` field, then regen the whole tree.
    std::fs::write(
        &spec,
        "openapi: 3.0.0\ninfo: { title: T, version: 1.0.0 }\n\
         components:\n  schemas:\n    Task:\n      type: object\n      properties:\n        \
         id: { type: integer }\n        title: { type: string }\n",
    )
    .unwrap();

    let report = glyph_cli::regen::regen(&root).expect("regen");
    assert_eq!(report.ran.len(), 1, "one recorded command re-run");

    let second = std::fs::read_to_string(&gen_file).unwrap();
    assert!(second.contains("title"), "the new field flowed in: {second}");

    // Idempotent: a second regen with no spec change leaves the file identical.
    glyph_cli::regen::regen(&root).expect("regen again");
    assert_eq!(second, std::fs::read_to_string(&gen_file).unwrap(), "idempotent");
}

#[test]
fn regen_reports_when_nothing_is_generated() {
    let root = unique_tmp("regen_empty");
    write_file(&root, "hand.glyph", "module m\nfn f() -> number { return 1 }\n");
    let err = glyph_cli::regen::regen(&root).expect_err("no generated files");
    assert!(matches!(err, glyph_cli::regen::RegenError::NoGenerated { .. }));
}

#[test]
fn build_warns_on_unused_import_binding_and_unreachable_code() {
    // The lint tier (warnings): an unused import (E0106), an unused `let`
    // (E0107), and a statement after `return` (E0108). All are warnings — the
    // build succeeds and still emits.
    let root = unique_tmp("lints");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/array\n\
         fn f() -> number {\n  \
           let dead = 1\n  \
           return 2\n  \
           let after = 3\n\
         }\n\
         fn main(argv: Array<string>) -> number { return f() }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "lints are warnings: {:?}", report.diagnostics);
    let codes: Vec<&str> = ["E0106", "E0107", "E0108"]
        .into_iter()
        .filter(|c| report.diagnostics.iter().any(|d| d.contains(c)))
        .collect();
    assert_eq!(codes, vec!["E0106", "E0107", "E0108"], "{:?}", report.diagnostics);
    assert_eq!(report.emitted, vec!["main.ts".to_string()], "warnings still emit");
}

#[test]
fn build_does_not_flag_a_binding_used_only_in_a_template() {
    // Regression: two adjacent `${...}` interpolations must get distinct spans,
    // or the resolution map collides and drops one binding's usage — which would
    // make a used variable look unused. `mark` is read inside the template.
    let root = unique_tmp("tpl_use");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         fn label(name: string) -> string {\n  \
           let mark = \"x\"\n  \
           return \"${mark} ${name}${name}\"\n\
         }\n\
         fn main(argv: Array<string>) -> number {\n  \
           print(label(\"a\"))\n  \
           return 0\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0107")),
        "a binding used in a template must not be flagged unused: {:?}",
        report.diagnostics
    );
}

#[test]
fn build_accepts_a_shared_store() {
    // A module-level `const` store (std/store) that several functions read and
    // mutate: it typechecks, emits, and lowers `create`/`get`/`update` to the
    // runtime store without needing `mut` on the const binding.
    let root = unique_tmp("store");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/store { Store, create }\n\
         const total: Store<number> = create(0)\n\
         fn bump() -> void {\n  \
           total.update(fn(n: number) -> number { return n + 1 })\n\
         }\n\
         fn value() -> number {\n  return total.get()\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    assert!(ts.contains("create(0)"), "lowers create: {ts}");
    assert!(ts.contains("total.update("), "lowers update: {ts}");
    assert!(ts.contains("total.get()"), "lowers get: {ts}");
    // The store binding stays a plain const — no `mut`, no reassignment.
    assert!(ts.contains("const total"), "const binding: {ts}");
}

#[test]
fn build_emits_quoted_string_keys() {
    let root = unique_tmp("strkey");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         fn headers() -> Record<string, string> {\n\
         \x20 return { \"Content-Type\": \"json\", plain: \"ok\" }\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    // The non-identifier key is quoted; the identifier key stays bareword.
    assert!(ts.contains("\"Content-Type\": \"json\""), "{ts}");
    assert!(ts.contains("plain: \"ok\""), "{ts}");
}

#[test]
fn build_wraps_multi_child_conditional_branches_in_a_fragment() {
    // BUG-4: a conditional (`<if>`/`<else>`) or match (`<case>`) branch with more
    // than one child element occupies a single React node slot. Emitting a bare
    // JS array `[a, b]` there trips React's "unique key" dev warning (the author
    // cannot add keys — `key=` is not placeable on a branch). A multi-child
    // branch must lower to `React.createElement(React.Fragment, null, ...)`;
    // a single-child branch stays the lone element (no needless Fragment).
    let root = unique_tmp("frag");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module f\n\
         import react { Component }\n\
         component Panel(show: bool) -> Component {\n\
         \x20 return <div>\n\
         \x20   <if cond={show}>\n\
         \x20     <span>one child</span>\n\
         \x20   </if>\n\
         \x20   <else>\n\
         \x20     <h2>heading</h2>\n\
         \x20     <p>paragraph</p>\n\
         \x20   </else>\n\
         \x20 </div>\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    // The multi-child `<else>` branch is grouped in a keyless Fragment.
    assert!(
        ts.contains(
            "React.createElement(React.Fragment, null, \
             React.createElement(\"h2\", null, \"heading\"), \
             React.createElement(\"p\", null, \"paragraph\"))"
        ),
        "multi-child branch did not lower to a Fragment:\n{ts}"
    );
    // The single-child `<if>` branch stays the lone element (no Fragment).
    assert!(
        ts.contains("? React.createElement(\"span\", null, \"one child\") :"),
        "single-child branch should not be wrapped in a Fragment:\n{ts}"
    );
    // No bare keyless array child survives (the pre-fix, warning-prone form).
    assert!(
        !ts.contains(": [React.createElement"),
        "a bare keyless array child leaked into a conditional branch:\n{ts}"
    );
}

#[test]
fn build_reports_emit_diagnostic_for_unsupported_construct() {
    let root = unique_tmp("emit_unsupported");
    let src = root.join("src");
    let out = root.join("dist");
    // A block-body match arm in a sub-expression value position (a call
    // argument) still lowers to a value IIFE, which cannot capture a
    // function-level `return`; the build should surface a diagnostic and NOT
    // write a .ts file for this module. (A `let x = match { ... }` block arm is
    // supported via the statement-switch lowering; nested in a call it is not.)
    write_file(
        &src,
        "main.glyph",
        "module main\ntype E = A | B\nfn wrap(n: number) -> number {\n  return n\n}\nfn f(e: E) -> number {\n  let x = wrap(match e {\n    A => { return 0 },\n    B => { return 1 },\n  })\n  return x\n}\n",
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
        "module lib\npub fn helper() -> number { return 1 }\n",
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
        "module lib/users\npub fn find() -> number { return 1 }\n",
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
        "module lib\npub fn helper() -> number { return 1 }\n",
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
fn build_flags_unknown_variant_pattern_with_suggestion() {
    // A PascalCase arm head that names no variant of the union is E0220, not a
    // silent binding catch-all. Here `Loadign` is a typo for `Loading`; the
    // diagnostic escalates it AND suggests the nearest real variant. Because it
    // is no longer read as a catch-all, the genuinely missing `Failed` still
    // surfaces as a separate non-exhaustiveness error.
    let root = unique_tmp("unknownvariant");
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
             Loaded => 2,\n    \
             Loadign => 3,\n  \
           }\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0220") && d.contains("did you mean `Loading`?")),
        "expected E0220 with a `did you mean` suggestion; got: {:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "the previously-swallowed missing-variant error should surface; got: {:?}",
        report.diagnostics
    );
}

#[test]
fn cross_module_union_typo_is_module_local_scope_only() {
    // Pins the current scope boundary of E0220: the escalation is module-local.
    // When the union is defined in another module, the scrutinee's type lowers to
    // `Unknown` and coverage is checked by `check_imported_union_coverage`, which
    // counts any PascalCase head as covering a variant with no membership check.
    // So a cross-module typo (`Loadign`) draws NO E0220 today. The imported
    // full-union masking is a known architecture decision (fork C); this test
    // makes the boundary visible in code so the day it changes, it changes here
    // deliberately rather than by surprise.
    let root = unique_tmp("crossmoduletypo");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "lib.glyph",
        "module lib\npub type Feed = | Loading | Loaded | Failed\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import lib { Feed, Loading, Loaded }\n\
         fn show(f: Feed) -> number {\n  \
           return match f {\n    \
             Loading => 1,\n    \
             Loaded => 2,\n    \
             Loadign => 3,\n  \
           }\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0220")),
        "E0220 is module-local only; a cross-module typo must not draw it today \
         (see check_imported_union_coverage / fork C): {:?}",
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
    // build would fail on the deliberately-malformed source. `node_modules`
    // holds installed dependencies (a real one contains stray `.glyph`-named
    // files only by accident), never project sources to compile.
    write_file(&src, ".git/decoy.glyph", "module decoy\nfn main(\n");
    write_file(&src, "target/decoy.glyph", "module decoy\nfn main(\n");
    write_file(&src, "node_modules/pkg/decoy.glyph", "module decoy\nfn main(\n");

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "decoy files under .git/, target/, node_modules/ should be skipped; got: {:?}",
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
fn http_server_program_type_checks() {
    // The std/http server surface (serve / Handler / query / text) emits
    // TypeScript that passes tsc --strict. Requires tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping http server tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("httpserver");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/http { serve, query, text, Request, Response }
import std/record
import std/result { Result, Ok, Err }
import std/option { Some, None }

fn multiply(req: Request) -> Result<Response, string> {
  let a = match record.get(query(req), "a") {
    Some(v) => number.parse(v),
    None => Err("missing a"),
  }
  return match a {
    Ok(av) => Ok(text(200, number.to_string(av))),
    Err(e) => Ok(text(400, e)),
  }
}

async fn main(argv: Array<string>) -> number {
  let outcome = await serve(8080, multiply)
  return match outcome {
    Ok(_) => 0,
    Err(_) => 1,
  }
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("server program failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn http_server_raw_body_accessor_type_checks() {
    // F7: a handler can read the unparsed request body via `http.raw(req)`,
    // needed to verify a signature (HMAC) over the exact bytes received. Requires
    // tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping raw-body tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("httprawbody");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/http { serve, raw, header, text, Request, Response }
import std/crypto
import std/result { Result, Ok }
import std/option { Some, None }

fn verify(req: Request) -> Result<Response, string> {
  let expected = crypto.hmac_sha256("shared-secret", raw(req))
  return match header(req, "x-hook-signature") {
    Some(sig) => match sig == expected {
      true => Ok(text(202, "accepted")),
      false => Ok(text(401, "bad signature")),
    },
    None => Ok(text(401, "missing signature")),
  }
}

async fn main(argv: Array<string>) -> number {
  let outcome = await serve(8080, verify)
  return match outcome {
    Ok(_) => 0,
    Err(_) => 1,
  }
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("raw-body program failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn http_server_writes_html_redirects_and_custom_headers() {
    // G52: `std/http` can serve more than a JSON API. A handler built from
    // `html`/`redirect`/`with_header` must put those bytes on the wire: a 302
    // with a `location` header, a `text/html` content type, a custom header,
    // and CR/LF stripped out of a header value (response splitting). The
    // handler is written in Glyph and compiled; a small TypeScript driver in
    // the build output starts the server and checks the real responses,
    // exiting with the number of failed assertions. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping http response-header run: node/tsx not available");
        return;
    }
    let root = unique_tmp("httpheaders");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/http { serve, path, form, json, text, html, redirect, with_header, Request, Response }
import std/record
import std/result { Result, Ok, Err }
import std/option { Some, None }

pub fn route(req: Request) -> Result<Response, string> {
  return match path(req) {
    "/page" => Ok(html(200, "<h1>hello</h1>")),
    "/go" => Ok(redirect(302, "/page")),
    "/split" => Ok(redirect(302, "/page\r\nx-injected: yes")),
    "/astral" => Ok(redirect(302, "/p/\u{1F389}/x")),
    "/api" => Ok(json(200, { ok: true })),
    "/plain" => Ok(text(200, "plain body")),
    "/custom" => Ok(with_header(html(200, "<p>tagged</p>"), "x-glyph", "1")),
    "/boom" => Err("handler failed"),
    "/echo" => match record.get(form(req), "name") {
      Some(v) => Ok(text(200, v)),
      None => Ok(text(400, "no name")),
    },
    else => Ok(text(404, "not found")),
  }
}

async fn main(argv: Array<string>) -> number {
  let outcome = await serve(8080, route)
  return match outcome {
    Ok(_) => 0,
    Err(_) => 1,
  }
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    let port = free_port();
    let driver = format!(
        r#"import "./.glyph-runtime/glyph-bootstrap.ts";
import {{ route }} from "./main.ts";
import {{ serve }} from "std/http";

const port = {port};
const base = "http://127.0.0.1:" + port;
void serve(port, route);

let failures = 0;
function check(ok: boolean, what: string): void {{
  if (!ok) {{
    failures += 1;
    console.error("FAIL: " + what);
  }}
}}

async function drive(): Promise<void> {{
  for (let i = 0; i < 200; i++) {{
    try {{
      await fetch(base + "/page");
      break;
    }} catch {{
      await new Promise((r) => setTimeout(r, 25));
    }}
  }}

  const page = await fetch(base + "/page");
  check(page.status === 200, "html status " + page.status);
  check(
    (page.headers.get("content-type") ?? "").startsWith("text/html"),
    "html content-type " + page.headers.get("content-type"),
  );
  check((await page.text()) === "<h1>hello</h1>", "html body");

  const go = await fetch(base + "/go", {{ redirect: "manual" }});
  check(go.status === 302, "redirect status " + go.status);
  check(go.headers.get("location") === "/page", "location " + go.headers.get("location"));

  const split = await fetch(base + "/split", {{ redirect: "manual" }});
  check(
    split.headers.get("location") === "/pagex-injected: yes",
    "CR/LF stripped from a header value, got " + split.headers.get("location"),
  );
  check(split.headers.get("x-injected") === null, "no injected header");

  // Node rejects any header byte above U+00FF as well as CR/LF, and it throws
  // from `writeHead`, outside the handler's try/catch, which would kill the
  // process. The character is dropped and the server keeps serving.
  const astral = await fetch(base + "/astral", {{ redirect: "manual" }});
  check(astral.status === 302, "astral redirect status " + astral.status);
  check(
    astral.headers.get("location") === "/p//x",
    "astral char dropped from a header value, got " + astral.headers.get("location"),
  );
  const alive = await fetch(base + "/page");
  check(alive.status === 200, "server alive after an astral header value");

  const api = await fetch(base + "/api");
  check(
    api.headers.get("content-type") === "application/json",
    "json content-type " + api.headers.get("content-type"),
  );
  check((await api.text()) === JSON.stringify({{ ok: true }}), "json body");

  const plain = await fetch(base + "/plain");
  check(
    plain.headers.get("content-type") === "text/plain; charset=utf-8",
    "text content-type " + plain.headers.get("content-type"),
  );

  const custom = await fetch(base + "/custom");
  check(custom.headers.get("x-glyph") === "1", "custom header");
  check(
    (custom.headers.get("content-type") ?? "").startsWith("text/html"),
    "with_header keeps the content type",
  );

  const boom = await fetch(base + "/boom");
  check(boom.status === 500, "Err status " + boom.status);
  check(
    boom.headers.get("content-type") === "application/json",
    "Err content-type " + boom.headers.get("content-type"),
  );

  const echo = await fetch(base + "/echo", {{
    method: "POST",
    headers: {{ "content-type": "application/x-www-form-urlencoded" }},
    body: "name=hello+world%21&name=last+wins",
  }});
  check((await echo.text()) === "last wins", "form decoding");

  process.exit(failures);
}}

void drive();
"#
    );
    std::fs::write(out.join("__driver.ts"), driver).expect("write driver");

    let status = std::process::Command::new("tsx")
        .arg("--tsconfig")
        .arg(out.join("tsconfig.json"))
        .arg(out.join("__driver.ts"))
        .status()
        .expect("run tsx");
    assert_eq!(
        status.code(),
        Some(0),
        "std/http response assertions failed (exit code is the failure count)"
    );
}

/// An unused localhost port, found by binding one and letting it go. Racy in
/// principle; in a test process nothing else claims it in between.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

#[test]
fn value_position_match_type_checks() {
    // A `match` that is the whole value of a `let` or a `mut` assignment lowers
    // to a flat statement `switch`. Before that, an `await` arm landed inside a
    // synchronous arrow (TS1308), a self-referential accumulator tripped circular
    // inference (TS7024), and a block arm under `mut` was a hard emit error.
    // Requires tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping value-match tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("valuematch");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io
import std/result { Result, Ok, Err }

async fn slow(n: number) -> number {
  return n
}

fn double(n: number) -> number {
  return n * 2
}

async fn awaited(flag: bool) -> number {
  let n = match flag {
    true => await slow(1),
    false => 0,
  }
  return n
}

async fn nested(flag: bool) -> number {
  return double(match flag {
    true => await slow(3),
    false => 4,
  })
}

fn unannotated(r: Result<number, string>) -> string {
  let label = match r {
    Ok(n) => "ok",
    Err(e) => "err",
  }
  return label
}

fn reassigned(r: Result<number, string>) -> string {
  let label = ""
  mut label = match r {
    Ok(n) => "ok",
    Err(e) => {
      io.println(e)
      "err"
    },
  }
  return label
}

fn toggled(xs: Array<number>) -> bool {
  let on = false
  for x in xs {
    mut on = match on {
      true => false,
      false => true,
    }
  }
  return on
}

fn tail(xs: Array<string>) -> Array<string> {
  let rest = match xs {
    [] => [],
    [head, ...more] => more,
  }
  return rest
}

async fn main(argv: Array<string>) -> number {
  io.println(unannotated(Ok(1)))
  io.println(reassigned(Err("x")))
  io.println("${await awaited(true)}")
  io.println("${await nested(false)}")
  io.println("${toggled([1, 2, 3])}")
  let words = ["a", "b"]
  io.println("${tail(words)}")
  return 0
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    let ts = std::fs::read_to_string(out.join("main.ts")).expect("read emitted main.ts");
    assert!(
        ts.contains("n = (await slow(1));"),
        "the awaited arm assigns directly, with no arrow:\n{ts}"
    );
    assert!(
        ts.contains("(await (async () => {"),
        "a genuinely nested match with an await arm uses an async arrow:\n{ts}"
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("value-position match program failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn imported_generic_descriptor_parse_type_checks_and_rejects_at_runtime() {
    // Calling `parse<T>` on a generic descriptor imported from another module
    // must thread the runtime checker argument the imported `parse<T>(value,
    // __is_T)` demands. Without it the emitted call drops the checker, which both
    // fails `tsc --strict` (missing argument) and would skip nested validation at
    // runtime. This builds a two-module program, type-checks it with tsc, and
    // runs it: `main` returns 0 only when a well-shaped value is accepted and a
    // badly-shaped element (a numeric `name`) is rejected.
    if !js_toolchain_available() {
        eprintln!("skipping imported-generic-descriptor run: node/tsx not available");
        return;
    }
    let root = unique_tmp("impgeneric");
    let src = root.join("src");
    write_file(
        &src,
        "boxmod.glyph",
        "module boxmod\npub type Box<T> = { value: T }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import boxmod { Box }\n\
         import std/result { Ok, Err }\n\
         pub type User = { name: string }\n\
         fn describe(v: unknown) -> string {\n\
         \x20 return match Box.parse<User>(v) {\n\
         \x20   Ok(_) => \"ok\",\n\
         \x20   Err(_) => \"bad\",\n\
         \x20 }\n\
         }\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let good: unknown = { value: { name: \"ada\" } }\n\
         \x20 let bad: unknown = { value: { name: 42 } }\n\
         \x20 return match describe(good) == \"ok\" {\n\
         \x20   true => match describe(bad) == \"bad\" {\n\
         \x20     true => 0,\n\
         \x20     false => 3,\n\
         \x20   },\n\
         \x20   false => 2,\n\
         \x20 }\n\
         }\n",
    );

    let file = src.join("app.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "imported descriptor should accept the good value and reject the bad one \
                 (2 = good wrongly rejected, 3 = bad wrongly accepted)"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: `tsc` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("two-module program should build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("imported descriptor parse should type-check under tsc:\n{msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a `main`; got NoMain: {exports:?}");
        }
    }
}

#[test]
fn imported_generic_descriptor_is_narrows_cross_module() {
    // `match v { is Box<User> => .. }` on a generic descriptor imported from
    // another module must narrow the same way `Box.parse<User>(v)` does. Before
    // this fix `is_check` consulted only module-local descriptors, so an imported
    // `Box` fell through to a hard `EmitError::Unsupported` — while the website
    // claimed the `is` form "narrows the same way ... across module boundaries."
    // Assert the build no longer errors and emits the imported descriptor's
    // `is<T>` call with a threaded checker (not just a shape check).
    let root = unique_tmp("impgenericis");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "boxmod.glyph",
        "module boxmod\npub type Box<T> = { value: T }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import boxmod { Box }\n\
         pub type User = { name: string }\n\
         fn describe(v: unknown) -> string {\n\
         \x20 return match v {\n\
         \x20   is Box<User> => \"ok\",\n\
         \x20   else => \"bad\",\n\
         \x20 }\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "cross-module `is Box<User>` must not hard-error: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("Box.is<User>(v, "),
        "cross-module `is` should call the imported descriptor's is<T> with a \
         synthesized checker: {ts}"
    );
}

#[test]
fn imported_generic_descriptor_is_rejects_bad_element_at_runtime() {
    // The runtime half of the cross-module `is` narrowing: a well-shaped value is
    // accepted and a badly-shaped element (numeric `name`) is rejected, proving
    // the threaded checker validates deeply across the module boundary.
    if !js_toolchain_available() {
        eprintln!("skipping cross-module `is` run: node/tsx not available");
        return;
    }
    let root = unique_tmp("impgenericisrun");
    let src = root.join("src");
    write_file(
        &src,
        "boxmod.glyph",
        "module boxmod\npub type Box<T> = { value: T }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import boxmod { Box }\n\
         pub type User = { name: string }\n\
         fn describe(v: unknown) -> string {\n\
         \x20 return match v {\n\
         \x20   is Box<User> => \"ok\",\n\
         \x20   else => \"bad\",\n\
         \x20 }\n\
         }\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let good: unknown = { value: { name: \"ada\" } }\n\
         \x20 let bad: unknown = { value: { name: 42 } }\n\
         \x20 return match describe(good) == \"ok\" {\n\
         \x20   true => match describe(bad) == \"bad\" {\n\
         \x20     true => 0,\n\
         \x20     false => 3,\n\
         \x20   },\n\
         \x20   false => 2,\n\
         \x20 }\n\
         }\n",
    );

    let file = src.join("app.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "cross-module `is` should accept the good value and reject the bad \
                 element (2 = good wrongly rejected, 3 = bad wrongly accepted)"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => eprintln!("skipping: `tsx` not found"),
        glyph_cli::run::RunOutcome::TscMissing => eprintln!("skipping: `tsc` not found"),
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("two-module `is` program should build: {:?}", r.diagnostics)
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("cross-module `is` should type-check under tsc:\n{msg}")
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a `main`; got NoMain: {exports:?}")
        }
    }
}

#[test]
fn imported_generic_descriptor_parse_through_namespace_alias() {
    // `bm.Box.parse<User>(v)` where `bm` is an aliased module import must thread
    // the checker just like the bare `Box.parse<User>(v)` form. Before the fix the
    // receiver was a nested `Member` (`bm.Box`), not an `Expr::Ident`, so the
    // rewrite bailed and emitted the call with the checker dropped — a silent tsc
    // arity failure.
    let root = unique_tmp("impgenericalias");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "boxmod.glyph",
        "module boxmod\npub type Box<T> = { value: T }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import boxmod as bm\n\
         import std/result { Ok, Err }\n\
         pub type User = { name: string }\n\
         fn describe(v: unknown) -> string {\n\
         \x20 return match bm.Box.parse<User>(v) {\n\
         \x20   Ok(_) => \"ok\",\n\
         \x20   Err(_) => \"bad\",\n\
         \x20 }\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "aliased-module parse must build: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("bm.Box.parse<User>(v, "),
        "aliased-module receiver must thread the checker argument: {ts}"
    );
}

#[test]
fn imported_generic_descriptor_parse_multi_parameter_threads_both_checkers() {
    // A two-parameter cross-module descriptor (`Pair<A, B>`): `Pair.parse<User,
    // Item>(v)` must thread one checker per parameter, in order, so the registry's
    // real arity (2, not 1) is used and both element types validate. Asserts both
    // field checks (`.name` for User, `.sku` for Item) appear in the threaded
    // checkers.
    let root = unique_tmp("impgenericpair");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "pairmod.glyph",
        "module pairmod\npub type Pair<A, B> = { first: A, second: B }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import pairmod { Pair }\n\
         import std/result { Ok, Err }\n\
         pub type User = { name: string }\n\
         pub type Item = { sku: string }\n\
         fn describe(v: unknown) -> string {\n\
         \x20 return match Pair.parse<User, Item>(v) {\n\
         \x20   Ok(_) => \"ok\",\n\
         \x20   Err(_) => \"bad\",\n\
         \x20 }\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "multi-parameter cross-module parse must build: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("Pair.parse<User, Item>(v, "),
        "multi-parameter parse threads checkers: {ts}"
    );
    assert!(
        ts.contains(".name") && ts.contains(".sku"),
        "both parameter checkers validate their element's fields, in order: {ts}"
    );
}

#[test]
fn imported_generic_descriptor_parse_nested_type_argument_validates_deeply() {
    // A nested type argument through the imported path (`Box.parse<Box<User>>(v)`):
    // the outer checker must itself invoke the inner descriptor's `is`, not fall to
    // the presence floor. This is the deep-validation claim under test across a
    // module boundary; `field_value_check` now resolves an imported descriptor for
    // the nested argument too.
    let root = unique_tmp("impgenericnested");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "boxmod.glyph",
        "module boxmod\npub type Box<T> = { value: T }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import boxmod { Box }\n\
         import std/result { Ok, Err }\n\
         pub type User = { name: string }\n\
         fn describe(v: unknown) -> string {\n\
         \x20 return match Box.parse<Box<User>>(v) {\n\
         \x20   Ok(_) => \"ok\",\n\
         \x20   Err(_) => \"bad\",\n\
         \x20 }\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "nested-argument cross-module parse must build: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("Box.parse<Box<User>>(v, "),
        "nested parse threads a checker: {ts}"
    );
    assert!(
        ts.contains("Box.is("),
        "the nested checker calls the inner descriptor's is (deep validation), not \
         the presence floor: {ts}"
    );
}

#[test]
fn stale_node_shim_is_removed_when_types_node_appears() {
    // F15: a build with no @types/node writes the bundled node shim. If
    // @types/node is installed later, the next build must remove that stale shim.
    // The tsconfig `include` globs `.glyph-runtime/**/*.d.ts` unconditionally, so
    // a lingering shim's `declare module "node:crypto"` merges with
    // @types/node's and resolves `randomBytes(n).toString("hex")` to a 0-arg
    // `toString`, reddening the whole build (std/crypto.ts TS2554).
    let root = unique_tmp("stale_shim");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/crypto\n\nfn main(argv: Array<string>) -> number {\n  print(crypto.random_hex(8))\n  return 0\n}\n",
    );

    // First build, no @types/node: the shim is written.
    build_project_inner(&src, &out, false).expect("build ok");
    let shim = out.join(".glyph-runtime/glyph-node-shims.d.ts");
    assert!(shim.exists(), "shim is written when @types/node is absent");

    // @types/node appears in the project.
    write_file(
        &src,
        "node_modules/@types/node/package.json",
        r#"{ "name": "@types/node", "version": "26.0.0", "types": "index.d.ts" }"#,
    );
    write_file(&src, "node_modules/@types/node/index.d.ts", "// minimal\n");

    // The next build must remove the stale shim so it can't merge-conflict.
    build_project_inner(&src, &out, false).expect("build ok after @types/node");
    assert!(
        !shim.exists(),
        "stale bundled node shim must be removed once @types/node is present"
    );
}

#[test]
fn new_constructs_an_external_class_and_type_checks() {
    // D37 interop constructor: `new` on a class declared in a `.types` ambient
    // file type-checks against that constructor under `tsc --strict`, and a
    // method chains on the fresh instance. This is the class-based-npm-client
    // path (kafkajs, mongodb, ioredis, pg) reduced to a local ambient decl so
    // the test needs no network install.
    if !tsc_available() {
        eprintln!("skipping new-interop tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("newinterop");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src.join(".types"),
        "widgets.d.ts",
        "declare module \"widgets\" {\n  export class Widget {\n    constructor(name: string);\n    render(): string;\n  }\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main

import widgets { Widget }
import std/io { println }

fn main() -> void {
  let w = new Widget("gauge")
  println(w.render())
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("new-interop program failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn types_companion_package_resolves_over_the_typeless_js() {
    // A JS-only package (no bundled types) with a separate `@types/<pkg>`
    // companion must resolve to the `@types` declarations, not to the typeless
    // `.js` (which tsc reports as an implicit any, TS7016). Regression for the
    // tsconfig `paths` order: `@types/<pkg>` is tried before the bare package,
    // so the whole "ships JS, types live in `@types/*`" ecosystem (pg, react,
    // express, lodash) type-checks. Requires tsc.
    if !tsc_available() {
        eprintln!("skipping @types-companion tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("typescompanion");
    // A project boundary, so the node_modules walk stops here.
    write_file(&root, "package.json", "{\n  \"name\": \"proj\",\n  \"version\": \"1.0.0\"\n}\n");
    // A typeless JS package: has a JS entry, ships no declarations.
    write_file(
        &root,
        "node_modules/coolpkg/package.json",
        "{\n  \"name\": \"coolpkg\",\n  \"version\": \"1.0.0\",\n  \"main\": \"index.js\"\n}\n",
    );
    write_file(
        &root,
        "node_modules/coolpkg/index.js",
        "module.exports.greet = (n) => \"hi \" + n;\n",
    );
    // Its `@types` companion carries the declarations.
    write_file(
        &root,
        "node_modules/@types/coolpkg/index.d.ts",
        "declare module \"coolpkg\" {\n  export function greet(name: string): string;\n}\n",
    );
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport coolpkg { greet }\nimport std/io { println }\nfn main() -> void {\n  println(greet(\"world\"))\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("@types companion did not resolve over the typeless js:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn taint_discipline_blocks_untrusted_input_at_a_sink() {
    // std/taint: a `Trusted<string>` sink accepts a sanitized value but tsc
    // rejects a `Tainted<string>` handed to it directly (SQL injection as a
    // compile error). Glyph's own checker is permissive (opaque imported types),
    // so the guarantee is a tsc check; both directions are asserted. Needs tsc.
    if !tsc_available() {
        eprintln!("skipping taint tsc check: tsc not available");
        return;
    }
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};

    // POSITIVE: a sanitized value (and a trust_unchecked literal) reach the sink.
    let root = unique_tmp("taintok");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/taint { Tainted, Trusted, taint, sanitize, expose, trust_unchecked }
import std/io { println }

fn run_query(sql: Trusted<string>) -> void {
  println(expose(sql))
}

fn strip(raw: string) -> string {
  return raw
}

fn main() -> void {
  let user_input: Tainted<string> = taint("SELECT 1")
  run_query(sanitize(user_input, strip))
  run_query(trust_unchecked("SELECT 2"))
}
"#,
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "sanitized taint program: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("sanitized path should pass tsc:\n{msg}"),
        TscOutcome::NotFound => {
            eprintln!("skipping: tsc not found at check time");
            return;
        }
    }

    // NEGATIVE: a tainted value handed to the Trusted sink is a tsc error.
    let root2 = unique_tmp("taintbad");
    let src2 = root2.join("src");
    let out2 = root2.join("dist");
    write_file(
        &src2,
        "main.glyph",
        r#"module main

import std/taint { Tainted, Trusted, taint, expose }
import std/io { println }

fn run_query(sql: Trusted<string>) -> void {
  println(expose(sql))
}

fn main() -> void {
  let user_input: Tainted<string> = taint("DROP TABLE users")
  run_query(user_input)
}
"#,
    );
    // Glyph's own checker is permissive here (opaque imported types), so this
    // emits; tsc is where the discipline bites.
    let _ = build_project_inner(&src2, &out2, false).expect("build emits");
    match check_with_tsc(&out2).expect("run tsc") {
        TscOutcome::Failed(_) => {}
        TscOutcome::Passed => panic!("a tainted value reached a Trusted sink without sanitize"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn where_refinement_rejects_out_of_range_at_the_boundary() {
    // D39: a `where` predicate is enforced by the type's descriptor, so
    // `Amount.parse(-1)` and `Rating.parse(6)` fail while valid values pass. The
    // program returns its count of wrong outcomes as the exit code. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping where-refinement run: node/tsx not available");
        return;
    }
    let root = unique_tmp("refine");
    write_file(
        &root,
        "main.glyph",
        r#"module main

import std/result { Ok, Err }

pub type Amount = int where value >= 0
pub type Rating = int where value >= 1 && value <= 5

fn amt_ok(v: number) -> bool {
  return match Amount.parse(v) { Ok(a) => true, Err(e) => false, }
}
fn rat_ok(v: number) -> bool {
  return match Rating.parse(v) { Ok(a) => true, Err(e) => false, }
}
fn expect(got: bool, want: bool) -> number {
  return match got == want { true => 0, false => 1, }
}

fn main() -> number {
  let f = 0
  mut f = f + expect(amt_ok(5), true)
  mut f = f + expect(amt_ok(0), true)
  mut f = f + expect(amt_ok(-1), false)
  mut f = f + expect(amt_ok(3.5), false)
  mut f = f + expect(rat_ok(3), true)
  mut f = f + expect(rat_ok(6), false)
  mut f = f + expect(rat_ok(0), false)
  return f
}
"#,
    );
    let file = root.join("main.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "where-refinement had {code} wrong boundary outcome(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: toolchain not found at run time");
        }
        other => panic!("refinement program did not run: {other:?}"),
    }
}

#[test]
fn bigint_arithmetic_is_exact_past_2_53() {
    // `bigint` holds exact arbitrary-precision integers where a float `number`
    // silently rounds past 2^53. The program returns its count of wrong results
    // as the exit code, so Ran(0) proves every case is exact. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping bigint run: node/tsx not available");
        return;
    }
    let root = unique_tmp("bigint");
    write_file(
        &root,
        "main.glyph",
        r#"module main

fn check(got: string, want: string) -> number {
  return match got == want {
    true => 0,
    false => 1,
  }
}

fn main() -> number {
  let fails = 0
  let a: bigint = 9007199254740993n
  let sum = a + 2n
  mut fails = fails + check("${sum}", "9007199254740995")
  let big = 1000000000000000000n * 1000000000000000000n
  mut fails = fails + check("${big}", "1000000000000000000000000000000000000")
  return fails
}
"#,
    );
    let file = root.join("main.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "bigint produced {code} wrong result(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: toolchain not found at run time");
        }
        other => panic!("bigint program did not run: {other:?}"),
    }
}

#[test]
fn time_parse_iso_rejects_everything_that_is_not_iso() {
    // `time.parse_iso` is a boundary validator: it accepts a bare `YYYY-MM-DD`
    // or a datetime carrying an explicit `Z`/offset, and returns `None` for
    // everything else. The rejected forms below are all ones bare `Date.parse`
    // accepts: an offset-less datetime and a non-padded date are read in local
    // time (which would move the day the UTC accessors report), and an
    // impossible day is silently rolled over. The program returns its count of
    // wrong outcomes as the exit code. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping parse_iso run: node/tsx not available");
        return;
    }
    let root = unique_tmp("parseiso");
    write_file(
        &root,
        "main.glyph",
        r#"module main

import std/time

fn accepted(iso: string) -> bool {
  return match time.parse_iso(iso) {
    Some(t) => true,
    None => false,
  }
}

fn expect(got: bool, want: bool) -> number {
  return match got == want {
    true => 0,
    false => 1,
  }
}

fn expect_num(got: number, want: number) -> number {
  return match got == want {
    true => 0,
    false => 1,
  }
}

fn day_of(iso: string) -> number {
  return match time.parse_iso(iso) {
    Some(t) => time.day(t),
    None => 0,
  }
}

fn main() -> number {
  let fails = 0
  mut fails = fails + expect(accepted("2026-02-31"), false)
  mut fails = fails + expect(accepted("2026-1-3"), false)
  mut fails = fails + expect(accepted("January 5 2026"), false)
  mut fails = fails + expect(accepted("2026-13-01"), false)
  mut fails = fails + expect(accepted("2026-01-03T10:00"), false)
  mut fails = fails + expect(accepted("2026-02-29"), false)
  mut fails = fails + expect(accepted("2026-01-03"), true)
  mut fails = fails + expect(accepted("2026-07-25T18:33:08.000Z"), true)
  mut fails = fails + expect(accepted("2026-03-15T09:30:00-05:00"), true)
  mut fails = fails + expect(accepted("2028-02-29"), true)
  mut fails = fails + expect_num(day_of("2026-01-03"), 3)
  mut fails = fails + expect_num(day_of("2026-03-15T09:30:00-05:00"), 15)
  return fails
}
"#,
    );
    let file = root.join("main.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "parse_iso had {code} wrong outcome(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: toolchain not found at run time");
        }
        other => panic!("parse_iso program did not run: {other:?}"),
    }
}

#[test]
fn std_decimal_arithmetic_is_exact() {
    // Money correctness: std/decimal must be exact where IEEE-754 `number` is
    // not. The program returns its count of wrong results as the exit code, so a
    // clean run (Ran(0)) is proof every case matched. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping std/decimal run: node/tsx not available");
        return;
    }
    let root = unique_tmp("decimal");
    write_file(
        &root,
        "main.glyph",
        r#"module main

import std/decimal { Decimal, decimal, from_int }
import std/result { Ok, Err }

fn d(s: string) -> Decimal {
  return match decimal(s) {
    Ok(v) => v,
    Err(e) => from_int(0, 0),
  }
}

fn check(got: string, want: string) -> number {
  return match got == want {
    true => 0,
    false => 1,
  }
}

fn main() -> number {
  let fails = 0
  mut fails = fails + check(d("0.1").add(d("0.2")).to_string(), "0.3")
  mut fails = fails + check(d("10.50").add(d("2.25")).to_string(), "12.75")
  mut fails = fails + check(d("10.00").sub(d("0.01")).to_string(), "9.99")
  mut fails = fails + check(d("1.10").mul(d("1.10")).to_string(), "1.2100")
  mut fails = fails + check(d("2").div(d("3"), 2).to_string(), "0.67")
  mut fails = fails + check(d("0.125").round(2).to_string(), "0.13")
  mut fails = fails + check(d("-5.00").add(d("2.50")).to_string(), "-2.50")
  mut fails = fails + check(d("-3.14").abs().to_string(), "3.14")
  mut fails = fails + check(from_int(1050, 2).to_string(), "10.50")
  mut fails = fails + check(d("9007199254740993").add(d("2")).to_string(), "9007199254740995")
  return fails
}
"#,
    );
    let file = root.join("main.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "std/decimal produced {code} wrong result(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: toolchain not found at run time");
        }
        other => panic!("std/decimal program did not run: {other:?}"),
    }
}

#[test]
fn infer_output_guarantee_bites_on_shape_mismatch() {
    // D28: `object_schema<Shape> -> Schema<infer_output<Shape>>` derives the
    // output type from the shape. Annotating the result `Schema<Point>` when the
    // shape omits `y` must be REJECTED by tsc (mapped to Glyph source) — the
    // guarantee the pre-0.1.10 `<Out>` stand-in only trusted. Requires tsc.
    if !tsc_available() {
        eprintln!("skipping infer_output bite check: tsc not available");
        return;
    }
    let root = unique_tmp("inferoutput_bite");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "bite.glyph",
        r#"module bite

import std/result { Result, Ok, Err }

type Issue = {
  path: Array<string>,
  message: string,
}

type Schema<T> = {
  name: string,
  parse: fn(input: unknown) -> Result<T, Array<Issue>>,
}

fn number_schema() -> Schema<number> {
  return { name: "number", parse: fn(input) {
    match input {
      is number => Ok(input),
      else => Err([{ path: [], message: "expected number" }]),
    }
  } }
}

fn object_schema<Shape: Record<string, Schema<unknown>>>(shape: Shape) -> Schema<infer_output<Shape>> {
  return { name: "object", parse: fn(input) {
    match input {
      is Record<string, unknown> => Err([{ path: [], message: "stub" }]),
      else => Err([{ path: [], message: "expected object" }]),
    }
  } }
}

type Point = {
  x: number,
  y: number,
}

const bad: Schema<Point> = object_schema({ x: number_schema() })
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(
        !report.has_errors(),
        "the program itself is well-formed Glyph; the mismatch is a tsc-level check: {:?}",
        report.diagnostics
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {
            panic!("infer_output guarantee did NOT bite: a shape missing `y` was accepted as Schema<Point>")
        }
        TscOutcome::Failed(msg) => {
            assert!(
                msg.contains("age") || msg.contains('y') || msg.to_lowercase().contains("assignable"),
                "expected a shape/type mismatch, got:\n{msg}"
            );
        }
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn infer_output_is_independent_of_the_validator_type_name() {
    // The generalized operator (Linus 2nd-pass follow-up) unwraps a parser-shaped
    // field structurally, so a validator type named anything but `Schema` still
    // has its output type derived. Here the wrapper is `Codec<T>`; a shape whose
    // codecs produce `{ x: number }` must type-check as `Codec<Point>` and a
    // wrong output type must be rejected by tsc.
    if !tsc_available() {
        eprintln!("skipping infer_output name-independence check: tsc not available");
        return;
    }
    let root = unique_tmp("inferoutput_name");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "codec.glyph",
        r#"module codec

import std/result { Result, Ok, Err }

type Issue = {
  path: Array<string>,
  message: string,
}

type Codec<T> = {
  parse: fn(input: unknown) -> Result<T, Array<Issue>>,
}

fn number_codec() -> Codec<number> {
  return { parse: fn(input) {
    match input {
      is number => Ok(input),
      else => Err([{ path: [], message: "expected number" }]),
    }
  } }
}

fn object_codec<Shape: Record<string, Codec<unknown>>>(shape: Shape) -> Codec<infer_output<Shape>> {
  return { parse: fn(input) {
    match input {
      is Record<string, unknown> => Err([{ path: [], message: "stub" }]),
      else => Err([{ path: [], message: "expected object" }]),
    }
  } }
}

type Point = {
  x: number,
}

const good: Codec<Point> = object_codec({ x: number_codec() })
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!(
            "infer_output failed to unwrap a validator NOT named `Schema` (`Codec`):\n{msg}"
        ),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

/// Phase 1 of the interop work (Q43): an installed npm package that ships its own
/// types resolves and type-checks with no hand-written `.types/` stub. The build
/// emits into an out directory outside the project, so this only works because
/// `write_build_support` wires the project's `node_modules` into the generated
/// tsconfig's `paths`. Here a fake package `widgets` is installed at
/// `<src>/node_modules/widgets` with a `.d.ts`; a correct call type-checks, and a
/// wrong-typed call is rejected by tsc, proving the types are actually loaded and
/// enforced rather than falling back to `any`.
#[test]
fn installed_package_types_resolve_without_a_stub() {
    if !tsc_available() {
        eprintln!("skipping installed-package resolution check: tsc not available");
        return;
    }
    let root = unique_tmp("installed_pkg");
    let src = root.join("src");

    // A fake installed package that ships its own types via package.json.
    write_file(
        &src,
        "node_modules/widgets/package.json",
        r#"{ "name": "widgets", "version": "1.0.0", "types": "index.d.ts" }"#,
    );
    write_file(
        &src,
        "node_modules/widgets/index.d.ts",
        "export declare function make_widget(label: string): string;\n",
    );

    // Correct usage: the string argument matches the package's declared type.
    write_file(
        &src,
        "good.glyph",
        r#"module good

import widgets { make_widget }

fn main(argv: Array<string>) -> number {
  let w = make_widget("hello")
  print(w)
  return 0
}
"#,
    );

    let out = root.join("dist-good");
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(
        !report.has_errors(),
        "importing an installed package is well-formed Glyph: {:?}",
        report.diagnostics
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!(
            "installed package `widgets` did not resolve/type-check (no stub, node_modules wired into tsconfig):\n{msg}"
        ),
        TscOutcome::NotFound => panic!("tsc vanished mid-test"),
    }

    // Wrong usage: a number where the package declares a string. If the types
    // were not really loaded (resolved as `any`), tsc would accept this; it must
    // fail, proving the declared types are enforced across the seam.
    write_file(
        &src,
        "bad.glyph",
        r#"module bad

import widgets { make_widget }

fn main(argv: Array<string>) -> number {
  let w = make_widget(42)
  print(w)
  return 0
}
"#,
    );
    std::fs::remove_file(src.join("good.glyph")).expect("drop good module");

    let out_bad = root.join("dist-bad");
    let report = build_project_inner(&src, &out_bad, false).expect("build ok");
    assert!(
        !report.has_errors(),
        "the Glyph is well-formed; the type error is a tsc-level check: {:?}",
        report.diagnostics
    );
    match check_with_tsc(&out_bad).expect("run tsc") {
        TscOutcome::Passed => {
            panic!("passing a number to make_widget(label: string) was accepted: package types were NOT enforced")
        }
        TscOutcome::Failed(_) => {}
        TscOutcome::NotFound => panic!("tsc vanished mid-test"),
    }
}

/// Node builtins imported by their bare name (`import fs`/`path`/`os`) must
/// type-check out of the box, with no `.types/` stub and no `@types/node`
/// installed. The bundled Node shim (written when the project has no
/// `@types/node`) declares the common builtins under their bare names, which is
/// what a user's `import fs` emits.
#[test]
fn node_builtins_typecheck_out_of_the_box() {
    if !tsc_available() {
        eprintln!("skipping node-builtins check: tsc not available");
        return;
    }
    let root = unique_tmp("builtins");
    let src = root.join("src");
    write_file(
        &src,
        "m.glyph",
        r#"module m

import fs { existsSync }
import path { join }
import os { platform }

fn main(argv: Array<string>) -> number {
  let p = join("a", "b")
  let here = platform()
  match existsSync(p) {
    true => print(here),
    false => print(p),
  }
  return 0
}
"#,
    );

    let out = root.join("dist");
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diagnostics: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => {
            panic!("bare node builtins did not type-check with the bundled shim:\n{msg}")
        }
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
    // The bundled shim is written when there is no @types/node.
    assert!(
        out.join(".glyph-runtime/glyph-node-shims.d.ts").is_file(),
        "bundled node shim should be present without @types/node"
    );
}

/// A binary codec crosses the string/byte boundary through `Buffer`: it reads
/// the UTF-8 bytes of a string (`Array.from(Buffer.from(s, "utf8"))`) and
/// rebuilds a string from a byte array (`Buffer.from(bytes)`). The bundled node
/// shim must type-check both directions without `@types/node`, so `GlyphBuffer`
/// is iterable/index-addressable and `Buffer.from` accepts a byte array.
#[test]
fn buffer_byte_boundary_typechecks_with_the_shim() {
    if !tsc_available() {
        eprintln!("skipping buffer-boundary check: tsc not available");
        return;
    }
    let root = unique_tmp("buffer_bytes");
    let src = root.join("src");
    write_file(
        &src,
        "m.glyph",
        r#"module m

fn to_bytes(s: string) -> Array<number> {
  return extern_ts("Array.from(Buffer.from(s, 'utf8')) as number[]")
}

fn first_byte(bytes: Array<number>) -> number {
  return extern_ts("(Buffer.from(bytes)[0] ?? 0) as number")
}

fn from_bytes(bytes: Array<number>) -> string {
  return extern_ts("Buffer.from(bytes).toString('utf8')")
}

fn main(argv: Array<string>) -> number {
  let bytes = to_bytes("hi")
  print(from_bytes(bytes))
  return first_byte(bytes)
}
"#,
    );

    let out = root.join("dist");
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diagnostics: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => {
            panic!("Buffer byte boundary did not type-check with the bundled shim:\n{msg}")
        }
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

/// True only when both `node` and `tsx` are runnable. `glyph run` shells out to
/// `tsx`, which itself needs `node`; a box with `tsx` but no `node` would make a
/// run fail for environmental reasons, not a real defect.
fn js_toolchain_available() -> bool {
    fn ok(cmd: &str) -> bool {
        std::process::Command::new(cmd)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    ok("node") && ok("tsx")
}

fn tsc_available() -> bool {
    std::process::Command::new("tsc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn examples_run_and_report_pass_and_fail() {
    // `@example expr == expr` runs at build time; a passing one is counted, a
    // failing one is reported. Requires node + tsx; skipped otherwise.
    if !js_toolchain_available() {
        eprintln!("skipping example assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("examples");
    let src = root.join("src");
    write_file(
        &src,
        "calc.glyph",
        "module calc\n\
         import std/result { Result, Ok, Err }\n\
         @example add(2, 3) == 5\n\
         @example add(1, 1) == 3\n\
         fn add(a: number, b: number) -> number { return a + b }\n\
         @example wrap(7) == Ok(7)\n\
         fn wrap(n: number) -> Result<number, string> { return Ok(n) }\n",
    );
    let report = glyph_cli::examples::run_examples(&src).expect("run_examples ok");
    assert!(report.ran, "examples should have run");
    assert!(report.build_failed.is_none(), "augmented build should compile");
    assert_eq!(report.total, 3, "three @example lines");
    assert_eq!(
        report.failures.len(),
        1,
        "exactly the `add(1,1) == 3` example fails: {:?}",
        report.failures
    );
    assert!(
        report.failures[0].contains("add(1, 1)"),
        "failure should name the bad example: {:?}",
        report.failures
    );
}

#[test]
fn property_tests_run_through_examples() {
    // `test.property(pred, gen) == Ok(void)` is an `@example`; a property that
    // holds passes, one that doesn't fails. Requires node + tsx.
    if !js_toolchain_available() {
        eprintln!("skipping property assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("props");
    let src = root.join("src");
    write_file(
        &src,
        "p.glyph",
        "module p\n\
         import std/result { Result, Ok }\n\
         import std/test\n\
         import std/stream\n\
         @example test.property(fn(n) { n + 0 == n }, stream.ints()) == Ok(void)\n\
         @example test.property(fn(n) { n > 0 }, stream.ints()) == Ok(void)\n\
         fn x() -> bool { return true }\n",
    );
    let report = glyph_cli::examples::run_examples(&src).expect("run ok");
    assert!(report.ran);
    assert!(report.build_failed.is_none(), "should compile: {:?}", report.build_failed);
    assert_eq!(report.total, 2, "two property @examples");
    assert_eq!(
        report.failures.len(),
        1,
        "the `n > 0` property should fail (ints() yields 0 and negatives): {:?}",
        report.failures
    );
}

#[test]
fn doc_run_blocks_execute_and_assert() {
    // A ```glyph @run``` block in a @doc executes; a failing `assert` is a
    // failure. Requires node + tsx; skipped otherwise.
    if !js_toolchain_available() {
        eprintln!("skipping doc-run assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("docrun");
    let src = root.join("src");
    write_file(
        &src,
        "m.glyph",
        "module m\n\
         @doc \"\"\"\n```glyph @run\nassert(double(3) == 6)\nassert(double(2) == 5)\n```\n\"\"\"\n\
         fn double(n: number) -> number { return n * 2 }\n",
    );
    let report = glyph_cli::examples::run_examples(&src).expect("run ok");
    assert!(report.ran);
    assert!(report.build_failed.is_none(), "augmented build should compile");
    assert_eq!(report.total, 1, "one @run block");
    assert_eq!(
        report.failures.len(),
        1,
        "the block's second assert fails: {:?}",
        report.failures
    );
    assert!(report.failures[0].contains("doc-run"), "{:?}", report.failures);
}

#[test]
fn run_executes_main_and_propagates_exit_code() {
    // A program's `main(argv) -> number` return value becomes the process exit
    // code. Requires `node` + `tsx`; when absent the run is skipped so CI
    // without a JS toolchain stays green.
    if !js_toolchain_available() {
        eprintln!("skipping run assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("run");
    write_file(
        &root,
        "runprog.glyph",
        "module runprog\nfn main(argv: Array<string>) -> number {\n  return 7\n}\n",
    );
    let file = root.join("runprog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 7, "main's return value should be the exit code");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping run assertion: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("unexpected type-check failure (run was --no-check): {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("a program with a `main` should run, not report NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check, so tsc is never required");
        }
    }
}

#[test]
fn extern_http_server_type_checks_against_bundled_shim() {
    // F14: a hand-written extern .ts that runs an http server (the common shape)
    // type-checks against the bundled node shim with no @types/node installed:
    // req.on("error"), server.listen(port, callback), res.writeHead/end.
    if !tsc_available() {
        eprintln!("skipping extern-server tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("externserver");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "extern/server.ts",
        r#"import { createServer, type IncomingMessage, type ServerResponse } from "http";
export function serve(port: number): Promise<void> {
  return new Promise<void>((resolve) => {
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      let body = "";
      req.setEncoding("utf8");
      req.on("data", (c: string) => { body = body + c; });
      req.on("end", () => {
        res.writeHead(200, { "content-type": "text/plain" });
        res.end(`${req.method ?? "GET"} ${req.url ?? "/"} ${body.length}`);
      });
      req.on("error", () => res.end("err"));
    });
    server.on("close", () => resolve());
    server.listen(port, () => { console.log(`up on ${port}`); });
  });
}
"#,
    );
    write_file(
        &src,
        "main.glyph",
        "module main\nimport extern/server { serve }\nasync fn main(argv: Array<string>) -> number {\n  let _ = await serve(8080)\n  return 0\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "extern server is well-formed: {:?}", report.diagnostics);
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("extern http server failed tsc against the shim:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn extern_ts_module_is_imported_typed_and_preserved() {
    // F8/F16: a Glyph module reaches hand-written TypeScript through
    // `import extern/*`. The `.ts` in `<src>/extern/` is staged into the output,
    // type-checked (its types enforce the Glyph call), and preserved across a
    // rebuild (the prune pass must not delete it).
    if !tsc_available() {
        eprintln!("skipping extern tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("externimport");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "extern/mathx.ts",
        "export function triple(n: number): string {\n  return `x3=${n * 3}`;\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/io\nimport extern/mathx { triple }\nfn main(argv: Array<string>) -> number {\n  io.println(triple(7))\n  return 0\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "extern import is well-formed: {:?}", report.diagnostics);
    let staged = out.join("extern").join("mathx.ts");
    assert!(staged.exists(), "the extern .ts is staged into the output");

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("extern program failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }

    // A rebuild must preserve the staged extern (F16: the prune pass used to
    // delete any output .ts it did not itself emit).
    build_project_inner(&src, &out, false).expect("rebuild ok");
    assert!(staged.exists(), "the extern .ts survives a rebuild");

    // Its types are enforced: a wrong-typed call is a real tsc error.
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/io\nimport extern/mathx { triple }\nfn main(argv: Array<string>) -> number {\n  io.println(triple(\"nope\"))\n  return 0\n}\n",
    );
    build_project_inner(&src, &out, false).expect("build ok");
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Failed(_) => {}
        TscOutcome::Passed => panic!("a string passed to triple(n: number) should fail tsc"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn task_pool_bounds_concurrency() {
    // F13: task.pool(limit, tasks) runs the thunks with at most `limit` in
    // flight and joins the results in order. The program tracks the peak in-flight
    // count via a store and returns non-zero if the pool exceeded the limit, ran
    // fewer tasks than given, or returned them out of order.
    if !js_toolchain_available() {
        eprintln!("skipping task.pool run: node/tsx not available");
        return;
    }
    let root = unique_tmp("taskpool");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/task
import std/array
import std/store
import std/time

const INFLIGHT = store.create<number>(0)
const MAXSEEN = store.create<number>(0)

async fn work(i: number) -> number {
  mut INFLIGHT.update(fn(n: number) -> number { n + 1 })
  let cur = INFLIGHT.get()
  mut MAXSEEN.update(fn(m: number) -> number { match cur > m { true => cur, false => m, } })
  let _ = await time.sleep(time.Duration.ms(25))
  mut INFLIGHT.update(fn(n: number) -> number { n - 1 })
  return i * 2
}

fn ordered(xs: Array<number>, expected: number) -> bool {
  return match xs {
    [] => true,
    [head, ...rest] => match head == expected {
      true => ordered(rest, expected + 2),
      false => false,
    },
  }
}

async fn main(argv: Array<string>) -> number {
  let thunks = array.map([0, 1, 2, 3, 4, 5], fn(i: number) {
    async fn() -> number { await work(i) }
  })
  let results = await task.pool(2, thunks)
  return match ordered(results, 0) {
    false => 1,
    true => match MAXSEEN.get() <= 2 {
      false => 2,
      true => match array.len(results) == 6 {
        true => 0,
        false => 3,
      },
    },
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "task.pool broke ordering (1), the limit (2), or completeness (3)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping task.pool run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("task.pool program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn async_closure_with_par_all_runs() {
    // F11/F12: an async closure passed to array.map, its results awaited by
    // par.all, type-checks (tsc) and runs the concurrency correctly.
    if !js_toolchain_available() {
        eprintln!("skipping async-closure run: node/tsx not available");
        return;
    }
    let root = unique_tmp("asyncclosure");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/array

async fn work(n: number) -> number {
  return n * 2
}

async fn run(items: Array<number>) -> Array<number> {
  return await par.all(array.map(items, async fn(n: number) -> number {
    await work(n)
  }))
}

fn sum(xs: Array<number>) -> number {
  return match xs {
    [] => 0,
    [head, ...rest] => head + sum(rest),
  }
}

async fn main(argv: Array<string>) -> number {
  let doubled = await run([1, 2, 3, 4])
  return match sum(doubled) == 20 {
    true => 0,
    false => 1,
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "async closure + par.all produced a wrong result");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping async-closure run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("async-closure program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn inline_union_signature_runs_and_type_checks() {
    // F3: an inline `string | number` in a signature and as a type argument
    // type-checks (tsc) and runs, with `is` narrowing over the union.
    if !js_toolchain_available() {
        eprintln!("skipping inline-union run: node/tsx not available");
        return;
    }
    let root = unique_tmp("inlineunion");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

fn seg(p: string | number) -> string {
  return match p {
    is string => p,
    is number => number.to_string(p),
  }
}

fn render(parts: Array<string | number>) -> string {
  return match parts {
    [] => "",
    [head, ...rest] => seg(head) + render(rest),
  }
}

fn main(argv: Array<string>) -> number {
  let ok = seg("a") == "a" && seg(42) == "42" && render(["x", 7]) == "x7"
  return match ok {
    true => 0,
    false => 1,
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "inline-union program produced a wrong value");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping inline-union run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("inline-union program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn value_position_match_with_return_arm_runs() {
    // F5: `let x = match { ... None => return Err(e) }` type-checks (tsc) and runs
    // with function-return semantics; a value-tail block arm assigns the binding.
    if !js_toolchain_available() {
        eprintln!("skipping value-position-match run: node/tsx not available");
        return;
    }
    let root = unique_tmp("valmatch");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/result { Result, Ok, Err }
import std/option { Option, Some, None }

fn require_op(v: Option<string>) -> Result<string, string> {
  let op = match v {
    Some(o) => o,
    None => return Err("missing"),
  }
  return Ok("op=" + op)
}

fn label(n: number) -> string {
  let tag = match n > 0 {
    true => {
      let sign = "pos"
      "sign:" + sign
    },
    false => "nonpos",
  }
  return tag
}

fn main(argv: Array<string>) -> number {
  let ok = match require_op(Some("push")) { Ok(s) => s == "op=push", Err(_) => false, }
    && match require_op(None) { Ok(_) => false, Err(e) => e == "missing", }
    && label(5) == "sign:pos"
    && label(-1) == "nonpos"
  return match ok {
    true => 0,
    false => 1,
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "value-position match with a return arm dispatched wrong");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping value-position-match run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("value-position-match program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn nested_literal_patterns_run_correctly() {
    // F4: a match with nested literal payloads (Ok(true)/Ok(false), Some(0)) runs
    // with the right semantics, including a trailing same-variant catch-all. The
    // program returns a non-zero code if any case is misdispatched.
    if !js_toolchain_available() {
        eprintln!("skipping nested-literal run: node/tsx not available");
        return;
    }
    let root = unique_tmp("nestedlit");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/result { Result, Ok, Err }
import std/option { Option, Some, None }

fn describe(r: Result<bool, string>) -> string {
  return match r {
    Ok(true) => "t",
    Ok(false) => "f",
    Err(e) => "e",
  }
}

fn classify(o: Option<number>) -> string {
  return match o {
    Some(0) => "z",
    Some(_) => "n",
    None => "x",
  }
}

fn main(argv: Array<string>) -> number {
  let ok = describe(Ok(true)) == "t"
    && describe(Ok(false)) == "f"
    && describe(Err("boom")) == "e"
    && classify(Some(0)) == "z"
    && classify(Some(9)) == "n"
    && classify(None) == "x"
  return match ok {
    true => 0,
    false => 1,
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "nested-literal match dispatched a case wrong");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping nested-literal run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("nested-literal program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn record_parse_error_is_an_issue_array() {
    // F2: T.parse returns Result<T, Array<Issue>> (not Result<T, string>), so the
    // Err binds as an issue list whose entries carry a `.message`, matching the
    // documented contract. This is the type the Glyph checker already modeled;
    // the fix aligns the emitted TS, so both agree under tsc.
    if !tsc_available() {
        eprintln!("skipping parse-issues tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("parseissues");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

type User = { id: string, name: string }

pub fn describe(value: unknown) -> string {
  return match User.parse(value) {
    Ok(u) => u.name,
    Err(issues) => match issues {
      [] => "invalid",
      [first, ..._rest] => "bad: " + first.message,
    },
  }
}

fn main(argv: Array<string>) -> number {
  return 0
}
"#,
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("record .parse Err-as-Issue[] failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn descriptor_parse_is_assignable_to_result() {
    // G41: a descriptor's `.parse` used to emit a bare `{tag,value}` union, which
    // is not assignable to `Result<T, E>` (that type intersects the `map`/
    // `map_err` combinators). Returning `T.parse(v)` from a `Result`-returning
    // function was TS2322 and `T.parse(v).map_err(f)` was TS2339, even though
    // Glyph's own checker reports `parse` as a `Result`. All three descriptor
    // kinds (record, refined alias, union) now return the real `Result`.
    if !tsc_available() {
        eprintln!("skipping descriptor-parse-Result tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("parseresult");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

type User = { id: string, name: string }
type Amount = int where value >= 0
type Shape =
  | Circle(int)
  | Square(int)

pub fn parse_user(value: unknown) -> Result<User, Array<Issue>> {
  return User.parse(value)
}

pub fn parse_amount(value: unknown) -> Result<Amount, Array<Issue>> {
  return Amount.parse(value)
}

pub fn parse_shape(value: unknown) -> Result<Shape, Array<Issue>> {
  return Shape.parse(value)
}

pub fn user_or_message(value: unknown) -> Result<User, string> {
  return User.parse(value).map_err(fn(issues: Array<Issue>) -> string { return "bad" })
}

pub fn user_name(value: unknown) -> Result<string, Array<Issue>> {
  let u = User.parse(value)?
  return Ok(u.name)
}

fn main(argv: Array<string>) -> number {
  return 0
}
"#,
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    // The `?` lowering and the descriptors share one aliased `std/result`
    // import; two lines would redeclare `__glyph_err`.
    let emitted = std::fs::read_to_string(out.join("main.ts")).expect("emitted main.ts");
    assert_eq!(
        emitted
            .matches(
                "import { Ok as __glyph_ok, Err as __glyph_err, type Result as __GlyphResult } from \"std/result\";"
            )
            .count(),
        1,
        "{emitted}"
    );
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("descriptor .parse as Result failed tsc:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

#[test]
fn fs_make_dir_and_append_text_round_trip() {
    // F10: make_dir creates the directory (mkdir -p, idempotent) and append_text
    // adds lines without a read-modify-write. The program returns 0 only if two
    // appends produce exactly the expected file content.
    if !js_toolchain_available() {
        eprintln!("skipping fs append/mkdir run: node/tsx not available");
        return;
    }
    let root = unique_tmp("fsappend");
    let dir = root.join("nested").join("data");
    let file = dir.join("log.ndjson");
    let prog = format!(
        "module main\n\
         import std/fs {{ make_dir, append_text, read_text }}\n\
         import std/result {{ Ok, Err }}\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let dir = \"{dir}\"\n\
         \x20 let file = \"{file}\"\n\
         \x20 return match make_dir(dir) {{\n\
         \x20   Err(_) => 2,\n\
         \x20   Ok(_) => match append_text(file, \"a\\n\") {{\n\
         \x20     Err(_) => 3,\n\
         \x20     Ok(_) => match append_text(file, \"b\\n\") {{\n\
         \x20       Err(_) => 4,\n\
         \x20       Ok(_) => match read_text(file) {{\n\
         \x20         Err(_) => 5,\n\
         \x20         Ok(text) => match text == \"a\\nb\\n\" {{\n\
         \x20           true => 0,\n\
         \x20           false => 1,\n\
         \x20         }},\n\
         \x20       }},\n\
         \x20     }},\n\
         \x20   }},\n\
         \x20 }}\n\
         }}\n",
        dir = dir.display(),
        file = file.display(),
    );
    write_file(&root, "fsprog.glyph", &prog);
    let file_glyph = root.join("fsprog.glyph");
    match glyph_cli::run::run_file(&file_glyph, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "make_dir + two append_text should yield exactly \"a\\nb\\n\"");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping fs append/mkdir run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("unexpected type-check failure: {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a main; got NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check");
        }
    }
}

#[test]
fn fs_read_dir_is_dir_and_stat_round_trip() {
    // G46: a directory could not be enumerated at all, so a CLI that takes a path
    // had to be handed every file explicitly. This walks one level: make a dir
    // with a file and a subdir in it, list it, ask which entries are directories,
    // and stat the file for its size.
    //
    // Runs with `check: true` so `FileInfo`'s field names and types are covered by
    // `tsc --strict`, and parses the two numeric fields through a descriptor whose
    // fields are `int`. That parse is the regression guard for `modified`: node
    // reports mtime with sub-millisecond precision, so the raw `mtimeMs` is a
    // float and `int` rejects it, while the docs promise `int` in four files.
    if !js_toolchain_available() {
        eprintln!("skipping fs read_dir run: node/tsx not available");
        return;
    }
    let root = unique_tmp("fsreaddir");
    let dir = root.join("tree");
    let prog = r#"module prog

import std/fs
import std/array
import std/path

type Snap = { size: int, modified: int }

fn main(argv: Array<string>) -> number {
  let dir = "__DIR__"
  let sub = path.join([dir, "sub"])
  let file = path.join([dir, "note.txt"])
  return match fs.make_dir(sub) {
    Err(_) => 2,
    Ok(_) => match fs.write_text(file, "hello") {
      Err(_) => 3,
      Ok(_) => match fs.read_dir(dir) {
        Err(_) => 4,
        Ok(names) => match array.len(names) == 2 && array.contains(names, "sub") && array.contains(names, "note.txt") {
          false => 5,
          true => match fs.is_dir(sub) {
            false => 6,
            true => match fs.is_dir(file) {
              true => 7,
              false => match fs.is_dir(path.join([dir, "missing"])) {
                true => 8,
                false => match fs.stat(file) {
                  Err(_) => 9,
                  Ok(info) => match info.is_file && info.size == 5 && info.is_dir == false && info.modified > 0 {
                    false => 10,
                    true => match Snap.parse({ size: info.size, modified: info.modified }) {
                      Err(_) => 11,
                      Ok(_) => 0,
                    },
                  },
                },
              },
            },
          },
        },
      },
    },
  }
}
"#
    .replace("__DIR__", &dir.display().to_string());
    write_file(&root, "prog.glyph", &prog);
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "a non-zero code names the failing step in the walk; 11 = `modified` \
                 was not an integer, so the documented `int` is a lie"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping fs read_dir run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping fs read_dir run: `tsc` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("read_dir/is_dir/stat must type-check under tsc --strict:\n{msg}")
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
    }
}

#[test]
fn fs_error_kind_is_a_named_closed_taxonomy() {
    // G47: `ErrorKind` used to be `{ tag: string }` with `NotFound` as the whole
    // taxonomy, so the only way to tell "that path is a directory" from "that path
    // is missing" was to compare the raw errno string. Reading a directory now
    // lands on `IsADirectory` by name, and a missing path still lands on
    // `NotFound`. Glyph does not check the match for exhaustiveness yet, so the
    // `else` arm here is load-bearing rather than decorative.
    //
    // Runs with `check: true`, and matches `Other({ code })` as well as the bare
    // kinds: `Other` is the only variant carrying a payload, so it is the only one
    // whose pattern lowering is non-obvious, and it needs `tsc --strict` over the
    // emitted destructure rather than a run alone.
    if !js_toolchain_available() {
        eprintln!("skipping fs error kind run: node/tsx not available");
        return;
    }
    let root = unique_tmp("fserrkind");
    let dir = root.join("adir");
    let prog = r#"module prog

import std/fs
import std/path
import std/string

fn classify(p: string) -> string {
  return match fs.read_text(p) {
    Ok(_) => "ok",
    Err(e) => match e.kind {
      fs.ErrorKind.NotFound => "notfound",
      fs.ErrorKind.IsADirectory => "isdir",
      fs.ErrorKind.PermissionDenied => "perm",
      fs.ErrorKind.Other({ code }) => "other:" + code,
      else => "unnamed",
    },
  }
}

fn main(argv: Array<string>) -> number {
  let dir = "__DIR__"
  let too_long = path.join([dir, string.repeat("x", 300)])
  return match fs.make_dir(dir) {
    Err(_) => 8,
    Ok(_) => match classify(dir) == "isdir" {
      false => 9,
      true => match classify(path.join([dir, "nope.txt"])) == "notfound" {
        false => 10,
        true => match string.starts_with(classify(too_long), "other:")
          && string.len(classify(too_long)) > 6 {
          false => 11,
          true => 0,
        },
      },
    },
  }
}
"#
    .replace("__DIR__", &dir.display().to_string());
    write_file(&root, "prog.glyph", &prog);
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "9 = a directory read did not classify as IsADirectory, 10 = a missing \
                 file did not classify as NotFound, 11 = an unnamed errno did not reach \
                 `Other` with its raw code bound"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping fs error kind run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping fs error kind run: `tsc` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("the `Other({{ code }})` pattern must type-check under tsc --strict:\n{msg}")
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
    }
}

#[test]
fn regex_captures_all_reads_groups_of_every_match() {
    // G51: `find_all` hands back the whole match and drops the groups, so a
    // scanner that wanted the capture text had to be hand-rolled character by
    // character. `captures_all` gives groups 1 onward per match, in order.
    if !js_toolchain_available() {
        eprintln!("skipping regex captures_all run: node/tsx not available");
        return;
    }
    let root = unique_tmp("recapall");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/regex
import std/array

fn main(argv: Array<string>) -> number {
  let rows = regex.captures_all("([a-z]+)=([0-9]+)", "a=1, bb=22")
  let empty = regex.captures_all("([a-z]+)=([0-9]+)", "nothing here")
  return match array.len(empty) == 0 {
    false => 1,
    true => match rows {
      [first, second] => match first {
        [k1, v1] => match second {
          [k2, v2] => match k1 == "a" && v1 == "1" && k2 == "bb" && v2 == "22" {
            true => 0,
            false => 2,
          },
          else => 3,
        },
        else => 4,
      },
      else => 5,
    },
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "captures_all must yield [[\"a\",\"1\"],[\"bb\",\"22\"]]");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping regex captures_all run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn task_pool_settled_keeps_going_past_a_failure() {
    // G53: `pool` is fail-fast, so one throwing task rejects the whole pool and
    // every result it had collected is lost (the other workers keep draining the
    // queue regardless; JS cannot cancel them). `pool_settled` bounds concurrency
    // the same way but reports one outcome per task, so the failure costs exactly
    // one result. Task 1 of 4 throws here and the other three still produce values.
    if !js_toolchain_available() {
        eprintln!("skipping task.pool_settled run: node/tsx not available");
        return;
    }
    let root = unique_tmp("poolsettled");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/task
import std/array
import std/time

async fn boom() -> number {
  let _ = extern_ts("(() => { throw new Error('boom') })()")
  return 0
}

async fn work(i: number) -> number {
  let _ = await time.sleep(time.Duration.ms(5))
  return match i == 1 {
    true => await boom(),
    false => i * 10,
  }
}

async fn main(argv: Array<string>) -> number {
  let thunks = array.map([0, 1, 2, 3], fn(i: number) {
    async fn() -> number { await work(i) }
  })
  let results = await task.pool_settled(2, thunks)
  return match results {
    [a, b, c, d] => match array.len(results) == 4 && a.ok && c.ok && d.ok {
      false => 1,
      true => match b.ok {
        true => 2,
        false => match a.value == 0 && c.value == 20 && d.value == 30 {
          true => 0,
          false => 3,
        },
      },
    },
    else => 4,
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "1 = a sibling of the failing task was abandoned, 2 = the failure was reported as ok, 3 = the surviving values are wrong");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping task.pool_settled run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn string_breadth_helpers_round_trip() {
    // G26/G34/G50: the string basics every app hand-rolled. The stdlib is
    // TypeScript, so it cannot carry `@example`; this run is where the behavior
    // is pinned. Each `let` is a group of assertions and the program returns the
    // number of the first group that failed, so a regression names itself.
    if !js_toolchain_available() {
        eprintln!("skipping string breadth run: node/tsx not available");
        return;
    }
    let root = unique_tmp("strbreadth");
    let prog = "module main\n\
         import std/string { repeat, pad_start, pad_end, slice, index_of, replace_all, trim_start, trim_end }\n\
         import std/option { Some, None }\n\
         \n\
         fn at(s: string, needle: string, from: number) -> number {\n\
         \x20 return match index_of(s, needle, from) {\n\
         \x20\x20\x20 Some(i) => i,\n\
         \x20\x20\x20 None => 0 - 1,\n\
         \x20 }\n\
         }\n\
         \n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let repeated = repeat(\"ab\", 3) == \"ababab\" && repeat(\"ab\", 0) == \"\" && repeat(\"ab\", 0 - 1) == \"\"\n\
         \x20 let padded = pad_start(\"123\", 2, \"0\") == \"123\" && pad_end(\"123\", 2, \"0\") == \"123\" && pad_start(\"7\", 3, \"0\") == \"007\" && pad_end(\"7\", 3, \".\") == \"7..\" && pad_start(\"7\", 3) == \"  7\"\n\
         \x20 let sliced = slice(\"hello world\", 6) == \"world\" && slice(\"hello world\", 0, 5) == \"hello\" && slice(\"hello\", 0 - 3) == \"llo\"\n\
         \x20 let searched = at(\"banana\", \"na\", 0) == 2 && at(\"banana\", \"na\", 3) == 4 && at(\"banana\", \"zz\", 0) == 0 - 1\n\
         \x20 let replaced = replace_all(\"a-b-c-d\", \"-\", \"+\") == \"a+b+c+d\"\n\
         \x20 let trimmed = trim_start(\"  x  \") == \"x  \" && trim_end(\"  x  \") == \"  x\"\n\
         \x20 return match repeated {\n\
         \x20\x20\x20 false => 1,\n\
         \x20\x20\x20 true => match padded {\n\
         \x20\x20\x20\x20\x20 false => 2,\n\
         \x20\x20\x20\x20\x20 true => match sliced {\n\
         \x20\x20\x20\x20\x20\x20\x20 false => 3,\n\
         \x20\x20\x20\x20\x20\x20\x20 true => match searched {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 false => 4,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 true => match replaced {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20 false => 5,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20 true => match trimmed {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20 false => 6,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20 true => 0,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20 },\n\
         \x20 }\n\
         }\n";
    write_file(&root, "strprog.glyph", prog);
    let file_glyph = root.join("strprog.glyph");
    match glyph_cli::run::run_file(&file_glyph, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a non-zero code names the failing assertion group");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping string breadth run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("unexpected type-check failure: {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a main; got NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check");
        }
    }
}

#[test]
fn array_fold_index_of_and_flat_map() {
    // G34/G50: `fold` is the one that touches a pillar — without it every
    // accumulation is a `mut` in a loop, which dilutes `grep mut`. The argument
    // order (collection, seed, callback) is part of what is pinned here.
    if !js_toolchain_available() {
        eprintln!("skipping array fold run: node/tsx not available");
        return;
    }
    let root = unique_tmp("arrfold");
    let prog = "module main\n\
         import std/array { fold, index_of, flat_map, len }\n\
         import std/option { Some, None }\n\
         \n\
         fn at(xs: Array<string>, value: string) -> number {\n\
         \x20 return match index_of(xs, value) {\n\
         \x20\x20\x20 Some(i) => i,\n\
         \x20\x20\x20 None => 0 - 1,\n\
         \x20 }\n\
         }\n\
         \n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let nums: Array<number> = [1, 2, 3, 4]\n\
         \x20 let words: Array<string> = [\"a\", \"b\", \"c\"]\n\
         \x20 let total = fold(nums, 0, fn(acc: number, x: number) -> number { return acc + x })\n\
         \x20 let joined = fold(words, \"\", fn(acc: string, x: string) -> string { return acc + x })\n\
         \x20 let pairs = flat_map(words, fn(w: string) -> Array<string> { return [w, w] })\n\
         \x20 let folded = total == 10 && joined == \"abc\"\n\
         \x20 let searched = at(words, \"b\") == 1 && at(words, \"z\") == 0 - 1\n\
         \x20 let flattened = len(pairs) == 6 && pairs[0] == \"a\" && pairs[1] == \"a\" && pairs[2] == \"b\"\n\
         \x20 return match folded {\n\
         \x20\x20\x20 false => 1,\n\
         \x20\x20\x20 true => match searched {\n\
         \x20\x20\x20\x20\x20 false => 2,\n\
         \x20\x20\x20\x20\x20 true => match flattened {\n\
         \x20\x20\x20\x20\x20\x20\x20 false => 3,\n\
         \x20\x20\x20\x20\x20\x20\x20 true => 0,\n\
         \x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20 },\n\
         \x20 }\n\
         }\n";
    write_file(&root, "arrprog.glyph", prog);
    let file_glyph = root.join("arrprog.glyph");
    match glyph_cli::run::run_file(&file_glyph, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a non-zero code names the failing assertion group");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping array fold run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("unexpected type-check failure: {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a main; got NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check");
        }
    }
}

#[test]
fn array_range_and_range_from_drive_counted_loops() {
    // G30: Glyph has no `..` and no `while`, so every counted loop used to be a
    // hand-rolled `upto(n)` built from `loop`/`break`. `range`/`range_from` are
    // that array; `range_from(start, end)` takes an exclusive end bound, the
    // same reading `slice` gives its second numeric argument, so
    // `range_from(2, 5)` is `[2, 3, 4]`. `for i in array.range(n)` must bind
    // `i` as a `number`,
    // not `Unknown`, or the stdlib version would be a typing regression against
    // the hand-rolled `upto(n) -> Array<int>` it replaces.
    if !js_toolchain_available() {
        eprintln!("skipping array range run: node/tsx not available");
        return;
    }
    let root = unique_tmp("arrrange");
    let prog = "module main\n\
         import std/array\n\
         import std/array { range, range_from, len }\n\
         \n\
         fn sum(xs: Array<number>) -> number {\n\
         \x20 let total = 0\n\
         \x20 for x in xs {\n\
         \x20\x20\x20 mut total = total + x\n\
         \x20 }\n\
         \x20 return total\n\
         }\n\
         \n\
         fn double_each(n: int) -> Array<int> {\n\
         \x20 let out: Array<int> = []\n\
         \x20 for i in array.range(n) {\n\
         \x20\x20\x20 mut out.push(i * 2)\n\
         \x20 }\n\
         \x20 return out\n\
         }\n\
         \n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let empty = len(range(0)) == 0 && len(range(0 - 2)) == 0\n\
         \x20 let three = range(3)\n\
         \x20 let counted = len(three) == 3 && three[0] == 0 && three[1] == 1 && three[2] == 2\n\
         \x20 let from = range_from(2, 5)\n\
         \x20 let offset = len(from) == 3 && from[0] == 2 && from[1] == 3 && from[2] == 4\n\
         \x20 let backwards = len(range_from(3, 3)) == 0 && len(range_from(5, 3)) == 0\n\
         \x20 let doubled = double_each(4)\n\
         \x20 let looped = sum(range(4)) == 6 && len(doubled) == 4 && doubled[3] == 6\n\
         \x20 return match empty {\n\
         \x20\x20\x20 false => 1,\n\
         \x20\x20\x20 true => match counted {\n\
         \x20\x20\x20\x20\x20 false => 2,\n\
         \x20\x20\x20\x20\x20 true => match offset && backwards {\n\
         \x20\x20\x20\x20\x20\x20\x20 false => 3,\n\
         \x20\x20\x20\x20\x20\x20\x20 true => match looped {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 false => 4,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 true => 0,\n\
         \x20\x20\x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20 },\n\
         \x20 }\n\
         }\n";
    write_file(&root, "rangeprog.glyph", prog);
    let file_glyph = root.join("rangeprog.glyph");
    match glyph_cli::run::run_file(&file_glyph, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a non-zero code names the failing assertion group");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping array range run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("unexpected type-check failure: {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a main; got NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check");
        }
    }
}

#[test]
fn start_here_tutorials_broken_program_is_exactly_e0200() {
    // B4 honesty guard: the Start-Here tutorial shows deleting a match arm
    // producing E0200, then fixing it. Keep both halves true so the tutorial
    // can't silently become a lie.
    let root = unique_tmp("starthere");
    let broken = "module main\n\
         type Status = Todo | Doing | Done\n\
         fn label(s: Status) -> string {\n  \
           return match s {\n    \
             Todo => \"not started\",\n    \
             Doing => \"in progress\",\n  \
           }\n\
         }\n\
         fn main(argv: Array<string>) -> number { return 0 }\n";
    write_file(&root.join("broken"), "main.glyph", broken);
    let report = build_project_inner(&root.join("broken"), &root.join("bout"), false).expect("build");
    assert!(report.has_errors(), "the broken program must not compile");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "deleting the arm must give exactly E0200: {:?}",
        report.diagnostics
    );

    // The fixed program (all three arms) compiles clean.
    let fixed = broken.replace(
        "    Doing => \"in progress\",\n  ",
        "    Doing => \"in progress\",\n    Done => \"finished\",\n  ",
    );
    write_file(&root.join("fixed"), "main.glyph", &fixed);
    let ok = build_project_inner(&root.join("fixed"), &root.join("fout"), false).expect("build");
    assert!(!ok.has_errors(), "the fixed program must compile: {:?}", ok.diagnostics);
}

#[test]
fn concurrent_runs_of_one_program_do_not_race_on_the_temp_dir() {
    // C2: many `glyph run`s of the same program share a fingerprint-keyed cache
    // dir. Building into a private staging dir + moving it into place removes the
    // clean-and-write race that surfaced as `DirectoryNotEmpty`. This test drives
    // the build path (via a build failure being impossible only masks it), so it
    // uses run_file with checking off and no JS toolchain requirement: even
    // NoMain/TsxNotFound outcomes exercise the staging/rename path. Any Io error
    // (a lost race) fails the test.
    use std::sync::Arc;
    let root = Arc::new(unique_tmp("race"));
    write_file(
        &root,
        "prog.glyph",
        "module prog\nfn main(argv: Array<string>) -> number { return 0 }\n",
    );
    let file = Arc::new(root.join("prog.glyph"));
    for _round in 0..6 {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let f = Arc::clone(&file);
                std::thread::spawn(move || glyph_cli::run::run_file(&f, &[], false, false))
            })
            .collect();
        for h in handles {
            let outcome = h.join().expect("thread did not panic");
            // The build/staging path must never surface a filesystem race as an
            // Err; a missing JS toolchain (TsxNotFound) is fine.
            assert!(outcome.is_ok(), "concurrent run raced: {outcome:?}");
        }
    }
}

#[test]
fn run_reports_no_main_for_a_library_instead_of_a_type_error() {
    // C1: running a library module (no `fn main`) reports NoMain with the
    // module's exports, rather than letting the generated entrypoint call an
    // undefined `main` and throw a raw Node `TypeError`. This needs no JS
    // toolchain — the check happens before `tsx` is invoked.
    let root = unique_tmp("nomain");
    write_file(
        &root,
        "lib.glyph",
        "module lib\nfn helper(x: number) -> number { return x }\nfn other() -> number { return 1 }\n",
    );
    let file = root.join("lib.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            assert!(exports.contains(&"helper".to_string()), "lists exports: {exports:?}");
            assert!(exports.contains(&"other".to_string()), "lists exports: {exports:?}");
        }
        other => panic!("expected NoMain for a library, got {other:?}"),
    }
}

#[test]
fn run_type_checks_by_default_and_refuses_tsc_broken_code() {
    // G9: `glyph run` type-checks before running. Assigning a stdlib call's
    // result (which Glyph types as `unknown`, so its own checker stays silent)
    // to a mistyped `let` passes Glyph and emits, but `tsc` rejects it — so the
    // run is refused (TypeCheckFailed) instead of running. The mistyped binding
    // is otherwise harmless at run time, so `--no-check` still runs to exit 0.
    if !js_toolchain_available() || !tsc_available() {
        eprintln!("skipping: node/tsx/tsc not all available");
        return;
    }
    let root = unique_tmp("runcheck");
    write_file(
        &root,
        "broken.glyph",
        "module broken\nimport std/string\nimport std/io\nfn main(argv: Array<string>) -> number {\n  let n: number = string.upper(\"hi\")\n  io.println(\"done\")\n  return 0\n}\n",
    );
    let file = root.join("broken.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            assert!(msg.to_lowercase().contains("error"), "tsc output: {msg}");
            // The tsc error is remapped onto the Glyph source: it carries the
            // TS code and no longer points at the generated `.ts`.
            assert!(msg.contains("[TS"), "should carry the remapped tsc code: {msg}");
            assert!(!msg.contains("broken.ts("), "raw .ts location should be gone: {msg}");
        }
        glyph_cli::run::RunOutcome::Ran(code) => {
            panic!("tsc-broken code must not run; got exit {code}");
        }
        other => panic!("expected TypeCheckFailed, got a different outcome: {}", outcome_name(&other)),
    }

    // With checking off, the same program runs (its return value is 0).
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(0) => {}
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping --no-check run assertion: tsx not found");
        }
        other => panic!("--no-check should run the program; got {}", outcome_name(&other)),
    }
}

fn outcome_name(o: &glyph_cli::run::RunOutcome) -> &'static str {
    match o {
        glyph_cli::run::RunOutcome::Ran(_) => "Ran",
        glyph_cli::run::RunOutcome::BuildFailed(_) => "BuildFailed",
        glyph_cli::run::RunOutcome::TypeCheckFailed(_) => "TypeCheckFailed",
        glyph_cli::run::RunOutcome::TsxNotFound => "TsxNotFound",
        glyph_cli::run::RunOutcome::TscMissing => "TscMissing",
        glyph_cli::run::RunOutcome::NoMain { .. } => "NoMain",
    }
}

#[test]
fn fmt_normalizes_a_comment_free_file_in_place() {
    let root = unique_tmp("fmt");
    write_file(
        &root,
        "messy.glyph",
        "module messy\nfn   f(a:number,b:number,c:number)->number{return a+b+c}\n",
    );
    let file = root.join("messy.glyph");
    let report = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert_eq!(report.formatted.len(), 1, "expected one file formatted");

    let after = std::fs::read_to_string(&file).unwrap();
    assert_ne!(after, "module messy\nfn   f(a:number,b:number,c:number)->number{return a+b+c}\n");
    assert!(glyph_parser::parse(&after).is_ok(), "formatted file must parse");

    // Idempotent: a second pass changes nothing.
    let report2 = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert_eq!(report2.formatted.len(), 0, "second pass should be a no-op");
    assert_eq!(report2.unchanged.len(), 1);
}

#[test]
fn fmt_check_reports_without_writing() {
    // F1: `glyph fmt --check` reports a file that would reformat but leaves it
    // untouched, and calls a canonical file clean.
    let root = unique_tmp("fmtcheck");
    let messy = "module messy\nfn   f(a:number)->number{return a}\n";
    write_file(&root, "messy.glyph", messy);
    let file = root.join("messy.glyph");

    // check mode: reports one would-reformat, writes nothing.
    let report = glyph_cli::fmt::format_path(&file, true).expect("fmt ok");
    assert_eq!(report.formatted.len(), 1, "the messy file would reformat");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        messy,
        "--check must not modify the file"
    );

    // Format it for real, then --check reports it clean.
    glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    let report2 = glyph_cli::fmt::format_path(&file, true).expect("fmt ok");
    assert_eq!(report2.formatted.len(), 0, "a canonical file is clean under --check");
    assert_eq!(report2.unchanged.len(), 1);
}

#[test]
fn fmt_preserves_comments() {
    let root = unique_tmp("fmtcomment");
    let original = "module c\n// keep this comment\nfn f() -> number { return 1 }\n";
    write_file(&root, "commented.glyph", original);
    let file = root.join("commented.glyph");
    let report = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert!(report.failed.is_empty(), "should not fail: {:?}", report.failed);
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.contains("// keep this comment"),
        "comment must be preserved:\n{after}"
    );
    assert!(glyph_parser::parse(&after).is_ok(), "formatted file must parse");

    // Idempotent: a second pass changes nothing.
    let report2 = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert_eq!(
        report2.formatted.len(),
        0,
        "second pass should be a no-op:\n{}",
        std::fs::read_to_string(&file).unwrap()
    );
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
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
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
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("a Glyph build failure must precede any tsc check: {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("a broken build should not reach the no-main check: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check, so tsc is never required");
        }
    }
}

#[test]
fn cross_module_record_payload_union_match_binds_whole_object() {
    // Regression (improve-glyph loop batch 3): a `Variant(v)` bind on an
    // *imported* record-payload union emitted `v.value` (TS2339) instead of
    // binding the whole `{tag, ...fields}` object. The cross-module registry
    // now resolves the imported variant's shape.
    let root = unique_tmp("xunion");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "err.glyph",
        "module err\npub type E =\n  | BadLeadByte({ at: number })\n  | Empty\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import err { E, BadLeadByte, Empty }\n\
         import std/string { from }\n\
         pub fn describe(e: E) -> string {\n\
         \x20 return match e {\n\
         \x20\x20\x20 BadLeadByte(v) => from(v.at),\n\
         \x20\x20\x20 Empty => \"empty\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    // The whole object is bound, not `.value`.
    assert!(ts.contains("const v = __") && !ts.contains("const v = __m0.value"), "{ts}");
}

#[test]
fn imported_union_nullary_variants_match_without_false_unreachable() {
    // Regression (improve-glyph loop batch 5): matching an imported union's
    // no-payload variants drew a false E0216 (the imported type lowers to
    // Unknown, so the reachability check read each bare PascalCase arm as a
    // binding catch-all) and then E0300 in the emitter. Both now treat a
    // PascalCase bare ident as a variant reference.
    let root = unique_tmp("nullary");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "net.glyph",
        "module net\npub type ParseError =\n  | WrongOctetCount({ got: number })\n  | EmptyOctet\n  | NotANumber\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import net { ParseError, WrongOctetCount, EmptyOctet, NotANumber }\n\
         import std/string { from }\n\
         pub fn describe(e: ParseError) -> string {\n\
         \x20 return match e {\n\
         \x20\x20\x20 WrongOctetCount(w) => from(w.got),\n\
         \x20\x20\x20 EmptyOctet => \"empty\",\n\
         \x20\x20\x20 NotANumber => \"nan\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    // The nullary variants lower to `case`s on `.tag`, not a `default` binding.
    assert!(ts.contains("case \"EmptyOctet\":"), "{ts}");
    assert!(ts.contains("case \"NotANumber\":"), "{ts}");
}

#[test]
fn non_exhaustive_imported_union_match_is_caught() {
    // The imported-union type-resolution pass: a match on an imported union that
    // omits a variant is now E0200, resolved cross-module by the union's real
    // name. Previously the imported type was Unknown, so exhaustiveness was
    // skipped and the gap leaked past the verifiability pillar at the boundary.
    let root = unique_tmp("impexhaust");
    let src = root.join("src");
    write_file(
        &src,
        "net.glyph",
        "module net\npub type ParseError =\n  | WrongOctetCount({ got: number })\n  | EmptyOctet\n  | NotANumber\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import net { ParseError, WrongOctetCount, EmptyOctet }\n\
         import std/string { from }\n\
         pub fn describe(e: ParseError) -> string {\n\
         \x20 return match e {\n\
         \x20\x20\x20 WrongOctetCount(w) => from(w.got),\n\
         \x20\x20\x20 EmptyOctet => \"empty\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(report.has_errors(), "a missing variant must be caught");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200") && d.contains("NotANumber")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn run_reports_every_build_diagnostic_including_on_a_cache_hit() {
    // `glyph run` computed a full build report and read only `emitted` from it,
    // so a program that ran successfully reported strictly fewer diagnostics
    // than `glyph build` on the same tree, including a warning on the entry file
    // the user named. Both diagnostics must appear now, and they must appear
    // again on the second run: that one is served from the warm run cache, which
    // otherwise has no report to read at all.
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let root = unique_tmp("rundiag");
    write_file(
        &root,
        "solo.glyph",
        "module solo\n\
         import std/io\n\
         import std/record\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 io.println(\"hi\")\n\
         \x20 return 0\n\
         }\n",
    );
    write_file(
        &root,
        "other.glyph",
        "module other\nfn broken() -> number {\n\x20 return \"nope\"\n}\n",
    );

    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_glyph"))
            .arg("run")
            .arg(root.join("solo.glyph"))
            .arg("--no-check")
            .output()
            .expect("spawn glyph run")
    };

    let first = run();
    let out = String::from_utf8_lossy(&first.stdout).to_string();
    let err = String::from_utf8_lossy(&first.stderr).to_string();
    assert!(out.contains("hi"), "the program still runs: stdout {out:?}");
    assert!(
        err.contains("E0204"),
        "the sibling module's type error must be reported: stderr {err}"
    );
    assert!(
        err.contains("E0106"),
        "the entry file's own unused-import warning must be reported: stderr {err}"
    );

    // Second run, unchanged sources: the build is cached, so the diagnostics can
    // only come back from the cache. Reporting them once and then falling silent
    // is worse than never reporting them.
    let second = run();
    let out2 = String::from_utf8_lossy(&second.stdout).to_string();
    let err2 = String::from_utf8_lossy(&second.stderr).to_string();
    assert!(out2.contains("hi"), "the program still runs: stdout {out2:?}");
    assert!(
        err2.contains("E0204") && err2.contains("E0106"),
        "a warm cache must report the identical diagnostics: stderr {err2}"
    );
}

#[test]
fn imported_descriptors_are_called_from_a_record_field_check() {
    // Descriptor resolution used to scan only the emitting module, so every
    // non-generic cross-module composition validated its field by
    // `!== undefined`: `Outer.parse({ i: 42 })` returned Ok with `i` typed by an
    // imported record. The project registry now carries the plain descriptors
    // too, so the field check calls `Inner.is` / `Instant.is` (the D39 predicate
    // included) exactly as a module-local field would.
    let root = unique_tmp("importeddesc");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "types.glyph",
        "module types\n\
         pub type Inner = {\n\x20 n: number,\n}\n\
         pub type Instant = string where value.length > 3\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import types { Inner, Instant }\n\
         pub type Outer = {\n\x20 i: Inner,\n\x20 t: Instant,\n}\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "cross-module descriptor composition must build: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("Inner.is((value as Record<string, unknown>).i)"),
        "imported record field calls its descriptor: {ts}"
    );
    assert!(
        ts.contains("Instant.is((value as Record<string, unknown>).t)"),
        "imported refined field calls its descriptor: {ts}"
    );
    assert!(
        !ts.contains("(value as Record<string, unknown>).i !== undefined"),
        "the presence floor must be gone: {ts}"
    );
    // The check depends on `Inner`/`Instant` being *value* bindings; they are
    // used only in type position here, so a future `import type` optimization
    // would erase them and break the guard at runtime with tsc still clean.
    assert!(
        ts.contains("import { Inner, Instant } from \"./types\";"),
        "descriptor references need a value import: {ts}"
    );
}

#[test]
fn imported_descriptor_field_rejects_bad_data_at_runtime() {
    // The regression that matters: before the fix this program exited 3, because
    // `Outer.parse({ i: 42, ... })` returned Ok. A boundary that accepted
    // unvalidated data now returns Err.
    if !js_toolchain_available() {
        eprintln!("skipping imported-descriptor run: node/tsx not available");
        return;
    }
    let root = unique_tmp("importeddescrun");
    let src = root.join("src");
    write_file(
        &src,
        "types.glyph",
        "module types\n\
         pub type Inner = {\n\x20 n: number,\n}\n\
         pub type Instant = string where value.length > 3\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import types { Inner, Instant }\n\
         import std/result { Ok, Err }\n\
         pub type Outer = {\n\x20 i: Inner,\n\x20 t: Instant,\n}\n\
         fn classify(v: unknown) -> string {\n\
         \x20 return match Outer.parse(v) {\n\
         \x20   Ok(_) => \"ok\",\n\
         \x20   Err(_) => \"err\",\n\
         \x20 }\n\
         }\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let good: unknown = { i: { n: 1 }, t: \"abcd\" }\n\
         \x20 let bad_field: unknown = { i: 42, t: \"abcd\" }\n\
         \x20 let bad_refine: unknown = { i: { n: 1 }, t: \"no\" }\n\
         \x20 return match classify(good) == \"ok\" {\n\
         \x20   true => match classify(bad_field) == \"err\" {\n\
         \x20     true => match classify(bad_refine) == \"err\" {\n\
         \x20       true => 0,\n\
         \x20       false => 4,\n\
         \x20     },\n\
         \x20     false => 3,\n\
         \x20   },\n\
         \x20   false => 2,\n\
         \x20 }\n\
         }\n",
    );

    let file = src.join("app.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "2 = a valid value was rejected, 3 = an imported record field was \
                 not validated, 4 = the imported `where` predicate was dropped"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => eprintln!("skipping: `tsx` not found"),
        glyph_cli::run::RunOutcome::TscMissing => eprintln!("skipping: `tsc` not found"),
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("two-module descriptor program should build: {:?}", r.diagnostics)
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("emitted descriptor checks should type-check under tsc:\n{msg}")
        }
        glyph_cli::run::RunOutcome::NoMain { exports } => {
            panic!("program has a `main`; got NoMain: {exports:?}")
        }
    }
}

#[test]
fn namespaced_imported_descriptor_is_called_from_a_field_check() {
    // The namespaced form (`import types` then a field typed `types.Inner`) is a
    // two-segment path, which the field check did not handle at all. It resolves
    // through the same registry, reached by the namespace binding.
    let root = unique_tmp("nsdesc");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "types.glyph",
        "module types\npub type Inner = {\n\x20 n: number,\n}\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\nimport types\npub type Outer = {\n\x20 i: types.Inner,\n}\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "namespaced descriptor composition must build: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("types.Inner.is((value as Record<string, unknown>).i)"),
        "namespaced field calls the imported descriptor: {ts}"
    );
}

#[test]
fn json_parse_of_an_imported_type_uses_its_schema() {
    // `json.parse<T>` gated on the same resolver, so an imported `T` silently
    // degraded to the casting parse. It now lowers to the validating form.
    let root = unique_tmp("jsonimported");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "types.glyph",
        "module types\npub type Inner = {\n\x20 n: number,\n}\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\n\
         import types { Inner }\n\
         import std/json\n\
         pub fn decode(s: string) -> unknown {\n\x20 return json.parse<Inner>(s)\n}\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "imported json.parse must build: {:?}",
        report.diagnostics
    );
    let ts = std::fs::read_to_string(out.join("app.ts")).unwrap();
    assert!(
        ts.contains("json.parse_with(s, Inner.schema)"),
        "imported json.parse uses the descriptor schema: {ts}"
    );
}

// --- The `@example` / `@doc @run` gate on `glyph build` (D23/D26) -----------
//
// D23 says the compiler runs every `@example` on `glyph build` and a failure
// fails the build. Execution used to sit behind `--test`, and `--json` returned
// before the gate ran at all, so a project whose own example asserted something
// false built green. These pin the default-on behavior on both channels.

/// Spawn the `glyph` binary and collect (exit code, stdout, stderr, child pid).
/// The pid matters: the example runner names its throwaway directory after the
/// process that created it, so a unique pid proves whether one was created.
fn spawn_glyph(args: &[&std::ffi::OsStr]) -> (i32, String, String, u32) {
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn glyph");
    let pid = child.id();
    let out = child.wait_with_output().expect("wait glyph");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        pid,
    )
}

fn failing_example_project(prefix: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = unique_tmp(prefix);
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "calc.glyph",
        "module calc\n\
         @example double(2) == 999\n\
         pub fn double(n: number) -> number { return n * 2 }\n",
    );
    (root, src, out)
}

#[test]
fn failing_example_fails_a_plain_build() {
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let (_root, src, out) = failing_example_project("exdefault");
    let (code, _stdout, stderr, _pid) = spawn_glyph(&[
        "build".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-check".as_ref(),
    ]);
    assert_eq!(code, 1, "a false @example must fail the build: {stderr}");
    assert!(
        stderr.contains("example failed"),
        "the failing example is named: {stderr}"
    );
    assert!(
        stderr.contains("1 of 1 example(s) failed"),
        "the tally is reported: {stderr}"
    );
}

#[test]
fn failing_example_is_visible_under_json() {
    // The agent-facing channel is the one that could not report a failing
    // colocated test: `emit_build_json` diverges, so it ran before the gate.
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let (_root, src, out) = failing_example_project("exjson");
    let (code, stdout, stderr, _pid) = spawn_glyph(&[
        "build".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-check".as_ref(),
        "--json".as_ref(),
    ]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON ({e}): {stdout} / {stderr}"));
    assert_eq!(v["ok"], serde_json::json!(false), "ok must be false: {v}");
    assert_eq!(code, 1, "--json must agree with the human exit code: {v}");
    assert_eq!(v["examples"]["total"], serde_json::json!(1), "{v}");
    assert_eq!(v["examples"]["ran"], serde_json::json!(true), "{v}");
    let failures = v["examples"]["failures"]
        .as_array()
        .unwrap_or_else(|| panic!("examples.failures must be an array: {v}"));
    assert_eq!(failures.len(), 1, "the failure is listed: {v}");
    assert!(
        failures[0].as_str().unwrap_or_default().contains("double(2)"),
        "the failure names the example: {v}"
    );
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "an example failure counts as an error: {v}"
    );
}

#[test]
fn no_test_skips_the_gate_and_says_so() {
    let (_root, src, out) = failing_example_project("exnotest");
    let (code, _stdout, stderr, _pid) = spawn_glyph(&[
        "build".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-check".as_ref(),
        "--no-test".as_ref(),
    ]);
    assert_eq!(code, 0, "--no-test opts out of the gate: {stderr}");
    assert!(
        stderr.contains("1 example(s) skipped (--no-test)"),
        "the skip is on the record: {stderr}"
    );
}

#[test]
fn project_without_examples_pays_nothing() {
    // `run_examples` early-returns before it copies the project, so a project
    // with no examples must not leave a throwaway build behind.
    let root = unique_tmp("exnone");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "plain.glyph",
        "module plain\npub fn id(n: number) -> number { return n }\n",
    );
    let (code, _stdout, stderr, pid) = spawn_glyph(&[
        "build".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-check".as_ref(),
    ]);
    assert_eq!(code, 0, "a clean example-less build stays green: {stderr}");
    assert!(
        !stderr.contains("example(s)"),
        "no example chatter on a project without examples: {stderr}"
    );
    let leftovers: Vec<PathBuf> = std::fs::read_dir(std::env::temp_dir())
        .expect("read temp dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("glyph-examples-{pid}-")))
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "no throwaway project should be copied: {leftovers:?}"
    );
}

#[test]
fn failing_doc_run_fails_a_plain_build() {
    // D26 rides the same path: a ```glyph @run``` block whose assert is false.
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let root = unique_tmp("exdocrun");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "m.glyph",
        "module m\n\
         @doc \"\"\"\n```glyph @run\nassert(double(2) == 5)\n```\n\"\"\"\n\
         pub fn double(n: number) -> number { return n * 2 }\n",
    );
    let (code, _stdout, stderr, _pid) = spawn_glyph(&[
        "build".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-check".as_ref(),
    ]);
    assert_eq!(code, 1, "a false @doc @run must fail the build: {stderr}");
    assert!(
        stderr.contains("doc-run"),
        "the failing doc block is named: {stderr}"
    );
}

#[test]
fn missing_tsx_refuses_to_look_verified() {
    // The gate could not run, so nothing may claim it passed. Same stance as
    // the missing-`tsc` branch: exit 2, no success line, and `--json` agrees.
    let root = unique_tmp("exnotsx");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "calc.glyph",
        "module calc\n\
         @example double(2) == 4\n\
         pub fn double(n: number) -> number { return n * 2 }\n",
    );
    let run = |json: bool| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"));
        cmd.arg("build")
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .arg("--no-check")
            .env("PATH", "/nonexistent-glyph-test-path");
        if json {
            cmd.arg("--json");
        }
        cmd.output().expect("spawn glyph")
    };

    let human = run(false);
    let stderr = String::from_utf8_lossy(&human.stderr).to_string();
    assert_eq!(human.status.code(), Some(2), "human exit: {stderr}");
    assert!(
        stderr.contains("tsx was not found on PATH"),
        "the reason is named: {stderr}"
    );
    assert!(
        !stderr.contains("example(s) passed"),
        "nothing may claim the examples passed: {stderr}"
    );

    let json = run(true);
    let stdout = String::from_utf8_lossy(&json.stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("JSON ({e}): {stdout}"));
    assert_eq!(v["ok"], serde_json::json!(false), "{v}");
    assert_eq!(v["examples"]["ran"], serde_json::json!(false), "{v}");
    assert_eq!(json.status.code(), Some(2), "--json agrees on the code: {v}");
}

// ---------------------------------------------------------------------------
// `glyph check` (G28), the summary-line ordering (G42), and `glyph run`'s
// hyphenated argv passthrough (G36).
// ---------------------------------------------------------------------------

#[test]
fn check_on_a_single_file_reports_its_errors() {
    // The gap: `glyph build one.glyph` refuses a file ("source path is not a
    // directory"), so the only way to type-check one was to run it.
    let root = unique_tmp("check_file_errors");
    write_file(
        &root,
        "broken.glyph",
        "module broken\nfn f() -> number {\n  return \"nope\"\n}\n",
    );
    let path = root.join("broken.glyph");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 1, "a file with a type error fails: {stderr}");
    assert!(
        stderr.contains("broken"),
        "the diagnostic names the file: {stderr}"
    );
    assert!(
        !stderr.contains("not a directory"),
        "a file is a legal target now: {stderr}"
    );
}

#[test]
fn check_on_a_clean_file_exits_zero() {
    let root = unique_tmp("check_file_clean");
    write_file(
        &root,
        "ok.glyph",
        "module ok\npub fn double(n: number) -> number {\n  return n * 2\n}\n",
    );
    let path = root.join("ok.glyph");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 0, "a clean file passes: {stderr}");
    assert!(
        stderr.contains("no diagnostics"),
        "and says so: {stderr}"
    );
}

#[test]
fn check_never_executes_the_program() {
    // This is the gap itself. `glyph run` type-checks a file by running it, so
    // asking "does this compile?" cost a side effect. `check` must answer the
    // same question with the program never starting: no stdout, and the file
    // `main` would have written is absent afterwards.
    let root = unique_tmp("check_no_exec");
    let sentinel = root.join("sentinel.txt");
    write_file(
        &root,
        "prog.glyph",
        &format!(
            "module prog\n\
             import std/io\n\
             import std/fs\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 io.println(\"SENTINEL-STDOUT\")\n\
             \x20 let _ = fs.write({:?}, \"ran\")\n\
             \x20 return 0\n\
             }}\n",
            sentinel.to_string_lossy()
        ),
    );
    let path = root.join("prog.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 0, "the program type-checks: {stderr}");
    assert!(
        stdout.is_empty(),
        "check writes nothing to stdout, so the program did not run: {stdout:?}"
    );
    assert!(
        !stdout.contains("SENTINEL-STDOUT") && !stderr.contains("SENTINEL-STDOUT"),
        "the program's own output must never appear: {stdout:?} {stderr:?}"
    );
    assert!(
        !sentinel.exists(),
        "the program's side effect must never happen"
    );
}

#[test]
fn check_accepts_a_directory_and_writes_nothing_into_it() {
    let root = unique_tmp("check_dir");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\npub fn helper() -> number { return 1 }\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\nimport lib { helper }\nfn main() -> number { return helper() }\n",
    );
    let before: Vec<_> = std::fs::read_dir(&src)
        .expect("read src")
        .map(|e| e.expect("entry").file_name())
        .collect();

    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        src.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 0, "the tree is clean: {stderr}");
    assert!(stderr.contains("2 module(s) checked"), "{stderr}");

    let after: Vec<_> = std::fs::read_dir(&src)
        .expect("read src")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(
        before.len(),
        after.len(),
        "check emits into a temp dir it deletes; the user's tree is untouched"
    );
}

#[test]
fn check_reports_a_sibling_error_when_checking_one_file() {
    // Scope: a file is checked in the context of its directory, exactly as
    // `glyph build` and `glyph run` see that tree. A sibling's error is
    // reported here rather than silently excluded, so the three commands
    // cannot disagree about whether a tree is clean.
    let root = unique_tmp("check_sibling");
    write_file(
        &root,
        "solo.glyph",
        "module solo\nfn main() -> number { return 0 }\n",
    );
    write_file(
        &root,
        "other.glyph",
        "module other\nfn broken() -> number {\n  return \"nope\"\n}\n",
    );
    let path = root.join("solo.glyph");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 1, "the sibling's error fails the check: {stderr}");
    assert!(stderr.contains("E0204"), "{stderr}");
}

#[test]
fn check_json_uses_the_same_keys_as_build_json() {
    let root = unique_tmp("check_json");
    write_file(
        &root,
        "broken.glyph",
        "module broken\nfn f() -> number {\n  return \"nope\"\n}\n",
    );
    let path = root.join("broken.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
        std::ffi::OsStr::new("--json"),
    ]);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("JSON ({e}): {stdout} {stderr}"));
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["ok"], serde_json::json!(false), "{v}");
    assert_eq!(v["errors"], serde_json::json!(1), "{v}");
    assert_eq!(v["warnings"], serde_json::json!(0), "{v}");
    assert_eq!(v["tsc"], serde_json::json!("not-run"), "{v}");
    assert_eq!(v["diagnostics"][0]["code"], serde_json::json!("E0204"), "{v}");
}

#[test]
fn check_missing_target_exits_two() {
    let root = unique_tmp("check_missing");
    let path = root.join("nope.glyph");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("does not exist"), "{stderr}");
}

/// A Glyph-clean program that `tsc` rejects: `std/taint` brands a `Tainted<T>`
/// so it cannot reach a `Trusted<T>` sink, and Glyph's own checker is permissive
/// about opaque imported types. The tsc stage is where this bites, which makes
/// it the fixture for "the Glyph stage was green and the build is still red."
const TSC_RED_GLYPH_CLEAN: &str = "module main\n\
     import std/taint { Tainted, Trusted, taint, expose }\n\
     import std/io { println }\n\
     fn run_query(sql: Trusted<string>) -> void {\n\
     \x20 println(expose(sql))\n\
     }\n\
     fn main() -> void {\n\
     \x20 let user_input: Tainted<string> = taint(\"DROP TABLE users\")\n\
     \x20 run_query(user_input)\n\
     }\n";

#[test]
fn build_prints_no_green_summary_above_its_own_tsc_errors() {
    // G42: the Glyph-stage summary used to print before the tsc stage ran, so a
    // red build opened with "no diagnostics" and then listed its type errors.
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let root = unique_tmp("g42_red");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", TSC_RED_GLYPH_CLEAN);

    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-test"),
    ]);
    assert_eq!(code, 1, "the build is red: {stderr}");
    assert!(
        stderr.contains("tsc reported type errors"),
        "and tsc is why: {stderr}"
    );
    assert!(
        !stderr.contains("no diagnostics"),
        "a red build must not open with a green line: {stderr}"
    );
}

#[test]
fn build_summary_precedes_the_tsc_pass_line_on_a_green_build() {
    // The other half of G42: reordering must not scramble a green build's
    // transcript, which the docs quote verbatim.
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let root = unique_tmp("g42_green");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn main() -> number { return 0 }\n",
    );

    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-test"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let summary = stderr.find("no diagnostics").expect("summary line");
    let passed = stderr.find("tsc --strict passed").expect("tsc line");
    assert!(summary < passed, "summary then tsc: {stderr}");
}

#[test]
fn check_reports_tsc_errors_on_a_glyph_clean_tree() {
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let root = unique_tmp("check_tsc_red");
    write_file(&root, "main.glyph", TSC_RED_GLYPH_CLEAN);
    let path = root.join("main.glyph");
    let (code, _stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("check"), path.as_os_str()]);
    assert_eq!(code, 1, "the tsc stage fails the check: {stderr}");
    assert!(stderr.contains("TS2345"), "remapped onto Glyph source: {stderr}");
    assert!(
        !stderr.contains("no diagnostics"),
        "and no green line above it: {stderr}"
    );
}

#[test]
fn run_passes_hyphenated_arguments_through_to_the_program() {
    // G36: without `allow_hyphen_values` clap rejected `--amount` as an unknown
    // flag, so a negative number could not be passed at all without `--`.
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not found on PATH");
        return;
    }
    let root = unique_tmp("g36_argv");
    write_file(
        &root,
        "echo.glyph",
        "module echo\n\
         import std/io\n\
         import std/string\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 io.println(string.join(argv, \"|\"))\n\
         \x20 return 0\n\
         }\n",
    );
    let path = root.join("echo.glyph");

    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("run"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-check"),
        std::ffi::OsStr::new("--amount"),
        std::ffi::OsStr::new("-12.50"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("--amount|-12.50"),
        "both arguments arrive intact: {stdout:?} {stderr}"
    );

    // A bare negative number, which clap previously read as a short-flag cluster.
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("run"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-check"),
        std::ffi::OsStr::new("-12.50"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("-12.50"), "{stdout:?} {stderr}");
}

#[test]
fn run_still_binds_its_own_flags_after_the_file() {
    // The other side of G36: `glyph run x.glyph --no-check` has always bound
    // `--no-check` to glyph, and it must keep doing so. clap starts the trailing
    // var-arg on *unknown* flags only, so a known one still belongs to glyph;
    // pinned here because it is the collision rule the change leaves in place.
    let root = unique_tmp("g36_known_flag");
    write_file(
        &root,
        "broken.glyph",
        "module broken\nfn main() -> number {\n  return \"nope\"\n}\n",
    );
    let path = root.join("broken.glyph");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("run"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-check"),
    ]);
    // `--no-check` bound to glyph, so the build ran without tsc and the Glyph
    // type error is what stops it — not clap, and not a program argument.
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("E0204"), "{stderr}");
    assert!(
        !stderr.contains("unexpected argument"),
        "clap must still own this flag: {stderr}"
    );
}

#[test]
fn build_on_a_single_file_points_at_check() {
    // The G28 symptom: `glyph build one.glyph` is what everyone types first, and
    // it cannot work. The refusal now names the command that does.
    let root = unique_tmp("build_file_hint");
    write_file(
        &root,
        "one.glyph",
        "module one\npub fn double(n: number) -> number {\n  return n * 2\n}\n",
    );
    let path = root.join("one.glyph");
    let out = root.join("dist");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        path.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("not a directory"), "{stderr}");
    assert!(
        stderr.contains("glyph check") && stderr.contains("one.glyph"),
        "the dead end points at `glyph check <file>`: {stderr}"
    );
}

#[test]
fn build_refusing_a_non_glyph_path_does_not_suggest_check() {
    // `glyph check README.md` would only fail differently, so the hint is
    // limited to the case where it is the answer.
    let root = unique_tmp("build_nonglyph_hint");
    write_file(&root, "README.md", "# not source\n");
    let path = root.join("README.md");
    let out = root.join("dist");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        path.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(!stderr.contains("glyph check"), "{stderr}");
}

#[test]
fn check_rejects_a_file_that_is_not_glyph_source() {
    let root = unique_tmp("check_not_glyph");
    write_file(&root, "notes.md", "# notes\n");
    let path = root.join("notes.md");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        path.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("not a `.glyph` file"),
        "and says why: {stderr}"
    );
}

#[test]
fn no_tsc_skips_the_typescript_stage_on_build_and_run() {
    // One stage, one flag name: `--no-tsc` is the canonical spelling on all
    // three commands. `--no-check` stays accepted on build and run (pinned by
    // `run_still_binds_its_own_flags_after_the_file` and the build tests above),
    // but the greppable name is this one.
    let root = unique_tmp("no_tsc_flag");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn main() -> number { return 0 }\n",
    );
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
        std::ffi::OsStr::new("--no-test"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !stderr.contains("tsc --strict passed"),
        "the tsc stage did not run: {stderr}"
    );

    // The same flag on `run`, and it must bind to glyph rather than to the
    // program's argv.
    let run_root = unique_tmp("no_tsc_flag_run");
    write_file(
        &run_root,
        "broken.glyph",
        "module broken\nfn main() -> number {\n  return \"nope\"\n}\n",
    );
    let broken = run_root.join("broken.glyph");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("run"),
        broken.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("E0204"), "{stderr}");
    assert!(
        !stderr.contains("unexpected argument"),
        "clap owns this flag: {stderr}"
    );
}

/// Spawn the binary with an empty `PATH`, so `tsc` cannot be found whatever the
/// machine has installed. The binary itself is invoked by absolute path.
fn spawn_glyph_without_a_toolchain(args: &[&std::ffi::OsStr]) -> (i32, String, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(args)
        .env("PATH", "")
        .output()
        .expect("spawn glyph");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn build_json_reports_a_missing_tsc_the_way_check_json_does() {
    // Both commands' text paths exit 2 when the TypeScript stage was requested
    // and `tsc` is absent. `build --json` used to report `ok: true` and exit 0
    // on the same machine, so a toolchain-less CI read green.
    let root = unique_tmp("json_no_tsc");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\nfn main() -> number { return 0 }\n",
    );

    let (code, stdout, stderr) = spawn_glyph_without_a_toolchain(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-test"),
        std::ffi::OsStr::new("--json"),
    ]);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("JSON ({e}): {stdout}"));
    assert_eq!(v["tsc"], serde_json::json!("not-found"), "{v}");
    assert_eq!(v["ok"], serde_json::json!(false), "{v} {stderr}");
    assert_eq!(code, 2, "a stage that could not run exits 2: {v}");

    let (code, stdout, stderr) = spawn_glyph_without_a_toolchain(&[
        std::ffi::OsStr::new("check"),
        src.as_os_str(),
        std::ffi::OsStr::new("--json"),
    ]);
    let w: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("JSON ({e}): {stdout}"));
    assert_eq!(w["tsc"], v["tsc"], "one shape: {w} vs {v}");
    assert_eq!(w["ok"], v["ok"], "one shape: {w} vs {v} {stderr}");
    assert_eq!(code, 2, "and one exit code: {w}");
}

#[test]
fn async_fn_type_annotates_a_handler_map() {
    // G45/D40: `async fn(A) -> T` is spellable, so a map of async handlers and a
    // parameter that takes an async callback both get a real annotation instead
    // of an unannotated `let`. The program dispatches through the map, awaits the
    // handler, and returns 0 only if every route answered.
    if !js_toolchain_available() {
        eprintln!("skipping async-fn-type run: node/tsx not available");
        return;
    }
    let root = unique_tmp("asyncfntype");
    write_file(
        &root,
        "prog.glyph",
        r#"module prog

import std/record
import std/option { Some, None }

type Handler = async fn(string) -> string

async fn greet(name: string) -> string {
  return "hello ${name}"
}

async fn shout(name: string) -> string {
  return "HELLO ${name}"
}

// The parameter type could not be written before D40.
async fn dispatch(h: async fn(string) -> string, arg: string) -> string {
  return await h(arg)
}

async fn route(routes: Record<string, Handler>, name: string, arg: string) -> string {
  return match record.get(routes, name) {
    Some(h) => await dispatch(h, arg),
    None => "no route",
  }
}

async fn main(argv: Array<string>) -> number {
  let routes: Record<string, Handler> = { greet: greet, shout: shout }
  let a = await route(routes, "greet", "ada")
  let b = await route(routes, "shout", "ada")
  let c = await route(routes, "nope", "ada")
  return match a == "hello ada" && b == "HELLO ada" && c == "no route" {
    true => 0,
    false => 1,
  }
}
"#,
    );
    let file = root.join("prog.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "async handler map dispatched the wrong answer");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping async-fn-type run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("async handler map failed to build: {:?}", r.diagnostics);
        }
        other => panic!("async handler map did not run: {other:?}"),
    }
}
