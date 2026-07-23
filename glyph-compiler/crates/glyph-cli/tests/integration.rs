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
fn record_descriptor_rejects_extra_keys_unless_open() {
    // C3b: a record's runtime descriptor is strict by default — its `is`/`parse`
    // reject a value with keys the type doesn't declare. `@open` opts out.
    let root = unique_tmp("strict");
    let out = root.join("dist");
    write_file(
        &root.join("src"),
        "main.glyph",
        "module main\n\
         type Point = { x: number, y: number }\n\
         @open\n\
         type Loose = { x: number }\n",
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
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok") {
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
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok") {
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
    match glyph_cli::run::run_file(&file, &[], false, true).expect("run_file ok") {
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
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok") {
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
    let report = glyph_cli::fmt::format_path(&file).expect("fmt ok");
    assert_eq!(report.formatted.len(), 1, "expected one file formatted");

    let after = std::fs::read_to_string(&file).unwrap();
    assert_ne!(after, "module messy\nfn   f(a:number,b:number,c:number)->number{return a+b+c}\n");
    assert!(glyph_parser::parse(&after).is_ok(), "formatted file must parse");

    // Idempotent: a second pass changes nothing.
    let report2 = glyph_cli::fmt::format_path(&file).expect("fmt ok");
    assert_eq!(report2.formatted.len(), 0, "second pass should be a no-op");
    assert_eq!(report2.unchanged.len(), 1);
}

#[test]
fn fmt_preserves_comments() {
    let root = unique_tmp("fmtcomment");
    let original = "module c\n// keep this comment\nfn f() -> number { return 1 }\n";
    write_file(&root, "commented.glyph", original);
    let file = root.join("commented.glyph");
    let report = glyph_cli::fmt::format_path(&file).expect("fmt ok");
    assert!(report.failed.is_empty(), "should not fail: {:?}", report.failed);
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.contains("// keep this comment"),
        "comment must be preserved:\n{after}"
    );
    assert!(glyph_parser::parse(&after).is_ok(), "formatted file must parse");

    // Idempotent: a second pass changes nothing.
    let report2 = glyph_cli::fmt::format_path(&file).expect("fmt ok");
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
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok") {
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
