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

/// Recursively copy `src` into `dst`, creating `dst`. Used by the examples
/// gate, which stages a copy of the tree so the nested multi-module apps can be
/// removed from the single-root pass and built at their own roots instead.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dir");
    for entry in std::fs::read_dir(src).expect("read dir") {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
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

    glyph_cli::gen::openapi(&spec, &out, false, false, &glyph_cli::gen::Renames::new())
        .expect("initial gen");
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
fn build_warns_on_a_module_with_no_pub_no_main_and_no_importer() {
    // A module with nothing `pub`, no `main`, and no Glyph-side importer emits
    // zero TypeScript `export` statements while `glyph build` reports "no
    // diagnostics" (G124). The first tool to notice used to be a *host* `tsc`
    // failing TS2459 on every import of the generated file, in whatever
    // React/Vite project embeds the output. This asserts the compiler catches
    // it itself, naming `pub`, before the build ever reaches that host.
    let root = unique_tmp("no_export_surface");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "lib.glyph",
        "module lib\nfn helper_one(x: number) -> number {\n  return x + 1\n}\nfn helper_two(x: number) -> number {\n  return x * 2\n}\n",
    );
    write_file(
        &src,
        "app.glyph",
        "module app\npub fn main() -> number {\n  return 1\n}\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "the check is advisory and must not fail the build; got: {:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0112")),
        "expected an E0112 warning on the dead module `lib`; got: {:?}",
        report.diagnostics
    );

    // Sibling: `lib` still declares nothing `pub`, but `app` names it in an
    // `import`, which is exactly the Glyph-side-importer case that must NOT
    // warn (a host toolchain is not the only legitimate importer, but a
    // Glyph one is enough to prove the module is reachable).
    let root2 = unique_tmp("no_export_surface_imported");
    let src2 = root2.join("src");
    let out2 = root2.join("dist");
    write_file(
        &src2,
        "lib.glyph",
        "module lib\nfn helper() -> number {\n  return 1\n}\n",
    );
    write_file(
        &src2,
        "app.glyph",
        "module app\nimport lib\npub fn main() -> number {\n  return 1\n}\n",
    );
    let report2 = build_project(&src2, &out2).expect("build_project ok");
    assert!(
        !report2.diagnostics.iter().any(|d| d.contains("E0112")),
        "a module named by a sibling's import must not warn; got: {:?}",
        report2.diagnostics
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
    // A sibling import resolves against the build root, so a multi-module app in
    // its own directory under `examples/apps/` is a project only when that
    // directory *is* the root. Rolling them into one root does not merely fail
    // to link them, it turns off every check that needs a sibling's declaration
    // (G72). So the single-root pass runs over a staged copy with those app
    // directories removed, and each of them is built at its own root below.
    let root = unique_tmp("examples");
    let staged = root.join("tree");
    copy_dir(examples, &staged);
    let mut apps: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(examples.join("apps")).expect("read apps dir") {
        let path = entry.expect("dir entry").path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().expect("app dir name").to_owned();
        std::fs::remove_dir_all(staged.join("apps").join(&name)).expect("unstage app");
        apps.push(path);
    }
    assert!(!apps.is_empty(), "no multi-module apps found under examples/apps");

    for app in &apps {
        let app_out = unique_tmp("exampleapp").join("dist");
        let report = build_project_inner(app, &app_out, false).expect("build app ok");
        assert!(
            !report.has_errors(),
            "{app:?} produced diagnostics: {:?}",
            report.diagnostics
        );
    }

    let out = root.join("dist");

    let report = build_project_inner(&staged, &out, false).expect("build examples ok");
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

import std/http { listen, query, text, Request, Response }
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
  let outcome = await listen("127.0.0.1", 8080, multiply)
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

import std/http { listen, raw, header, text, Request, Response }
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
  let outcome = await listen("127.0.0.1", 8080, verify)
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

import std/http { listen, path, form, json, text, html, redirect, with_header, Request, Response }
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
  let outcome = await listen("127.0.0.1", 8080, route)
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
import {{ listen }} from "std/http";

const port = {port};
const base = "http://127.0.0.1:" + port;


let failures = 0;
function check(ok: boolean, what: string): void {{
  if (!ok) {{
    failures += 1;
    console.error("FAIL: " + what);
  }}
}}

async function drive(): Promise<void> {{
  // Awaited, so the driver starts once the port is actually bound. This used to
  // be a 200-iteration retry loop around the first fetch, which is what a
  // `serve` that never told you it had started forced on every caller.
  const started = await listen("127.0.0.1", port, route);
  if (started.tag === "Err") {{
    console.error("FAIL: could not listen: " + started.value.message);
    process.exit(1);
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

  // An oversized body is refused rather than buffered without limit. Before
  // this, a client posting forever exhausted the process's memory.
  const big = await fetch(base + "/echo", {{
    method: "POST",
    headers: {{ "content-type": "text/plain" }},
    body: "x".repeat(9 * 1024 * 1024),
  }});
  check(big.status === 413, "an oversized body is 413, not unbounded buffering");

  process.exit(failures);
}}

void drive();
"#
    );
    std::fs::write(out.join("__driver.ts"), driver).expect("write driver");

    let run = std::process::Command::new("tsx")
        .arg("--tsconfig")
        .arg(out.join("tsconfig.json"))
        .arg(out.join("__driver.ts"))
        .output()
        .expect("run tsx");
    assert_eq!(
        run.status.code(),
        Some(0),
        "std/http response assertions failed (exit code is the failure count):\n{}",
        String::from_utf8_lossy(&run.stderr)
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
fn union_typed_binding_keeps_its_glyph_type_in_emitted_ts() {
    // A binding whose Glyph type is a union in TypeScript terms (`bool` is
    // `true | false`; a D30 string-literal union is its literal set) must keep
    // that type at every read of it. Glyph does not narrow a binding's type by
    // what was last assigned to it, but TypeScript does, and a `match` over the
    // binding lowers to a `switch` whose other arms are then "not comparable"
    // (TS2678) against the pinned literal. It bit hardest through a callback:
    // a flag set inside a `std/timers` callback and matched after the call is
    // the only way to bridge an event-based API into a value, and it failed
    // with a tsc error naming a type the author never wrote. Equality is the
    // second place it surfaces, as TS2367 ("no overlap"), and it surfaces
    // there whether the other side is a literal or a second binding, so both
    // spellings are here. Requires tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping union-binding tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("unionbinding");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io
import std/timers

type Mode = "fast" | "slow"

fn plain() -> number {
  let done = false
  match done {
    true => { return 1 },
    false => { return 0 },
  }
}

fn annotated() -> number {
  let mode: Mode = "fast"
  match mode {
    "fast" => { return 1 },
    "slow" => { return 2 },
  }
}

fn reassigned(seed: bool) -> number {
  let done = seed
  mut done = false
  match done {
    true => { return 1 },
    false => { return 0 },
  }
}

fn reassigned_literal_union(seed: Mode) -> number {
  let mode = seed
  mut mode = "slow"
  match mode {
    "fast" => { return 1 },
    "slow" => { return 2 },
  }
}

async fn through_a_callback() -> number {
  let done = false
  let value = 0
  timers.after(1, fn() {
    mut done = true
    mut value = 42
  })
  loop {
    match done {
      true => { break },
      false => {},
    }
    await timers.sleep(1)
  }
  return value
}

fn compared() -> bool {
  let done = false
  timers.after(1, fn() {
    mut done = true
  })
  return done == true
}

fn two_bools() -> bool {
  let a = false
  let b = true
  return a == b
}

fn two_bools_differ() -> bool {
  let a = false
  let b = true
  return a != b
}

fn two_modes() -> bool {
  let x: Mode = "fast"
  let y: Mode = "slow"
  return x == y
}

fn two_modes_differ() -> bool {
  let x: Mode = "fast"
  let y: Mode = "slow"
  return x != y
}

async fn main(argv: Array<string>) -> number {
  io.println("${plain()}")
  io.println("${annotated()}")
  io.println("${reassigned(true)}")
  let fast: Mode = "fast"
  io.println("${reassigned_literal_union(fast)}")
  io.println("${await through_a_callback()}")
  io.println("${compared()}")
  io.println("${two_bools()}")
  io.println("${two_bools_differ()}")
  io.println("${two_modes()}")
  io.println("${two_modes_differ()}")
  return 0
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("union-typed binding program failed tsc:\n{msg}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("program has a `main`; got NoMain: {exports:?}");
        }
    }
}

/// Work still pending when `main` returns must be allowed to finish.
///
/// The generated entrypoint used to call `process.exit` the moment `main`
/// returned, which killed the process while its event loop still had live
/// handles. Any long-lived program was therefore impossible to write: a Glyph
/// TCP server bound its port and died in the same tick, so `glyph run` printed
/// nothing, exited 0, and no client could connect. Nothing in the source looked
/// wrong and it type-checked, so the failure was invisible.
///
/// This program schedules a delayed write and returns from `main` immediately.
/// The delayed write is what the assertion is about: if the process is torn
/// down at `return`, the exit code is 0 but the write never lands.
#[test]
fn work_pending_when_main_returns_still_runs() {
    let root = unique_tmp("pending_work");
    let src = root.join("src");
    let marker = root.join("landed.txt");
    let marker_lit = marker.to_string_lossy().replace('\\', "/");
    write_file(
        &src,
        "app.glyph",
        &format!(
            "module app\n\
             import std/fs\n\
             import std/time\n\
             async fn later() {{\n\
             \x20 await time.sleep(time.Duration.ms(50))\n\
             \x20 let _ = fs.write_text(\"{marker_lit}\", \"landed\")\n\
             }}\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 later()\n\
             \x20 return 0\n\
             }}\n"
        ),
    );

    let file = src.join("app.glyph");
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "the program itself succeeds");
            assert!(
                marker.is_file(),
                "a delayed write scheduled before `main` returned must still land; \
                 the process was torn down at `return` instead"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: `tsc` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("fixture should build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("fixture should type-check:\n{msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("program has a `main`; got NoMain: {exports:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&root);
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
fn rng_bool_can_be_called_with_no_argument() {
    // G155: `Rng.bool`'s own doc comment says "true with the given probability
    // (default 0.5)", but the generated type signature made `probability`
    // required, so the documented zero-arg call failed at the tsc stage
    // (TS2554) instead of either working or failing with a Glyph diagnostic.
    // A caller who trusts the doc comment (as any generation-and-simulation
    // code that wants a plain coin flip would) hits a raw TypeScript error
    // pointing at generated code they never wrote.
    if !tsc_available() {
        eprintln!("skipping rng bool tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("rngbool");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\nimport std/io\nimport std/random { seeded }\n\nfn main(argv: Array<string>) -> number {\n  let r = seeded(42)\n  let b = r.bool()\n  io.println(\"coin flip: ${b}\")\n  return 0\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("r.bool() with no argument should type-check:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
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
fn parse_reports_which_rule_the_value_broke() {
    // The four ways a payload can be wrong used to answer with the same string:
    // an absent field, a field failing its `where` predicate, a field of the
    // wrong type, and a posted array all read "field `x` is missing or has the
    // wrong type" (and an array was accepted outright by a record with no
    // required fields). Each now answers differently, in both the message and
    // the machine-readable `code`, and the refinement names its predicate so the
    // rejection greps back to the `type Password = ...` line. The program
    // returns its count of wrong answers as the exit code. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping boundary-issue run: node/tsx not available");
        return;
    }
    let root = unique_tmp("boundary_issue");
    write_file(
        &root,
        "main.glyph",
        r#"module main

import std/result { Ok, Err }
import std/io

pub type Password = string where value.length >= 8

pub type Signup = {
  email: string,
  password: Password,
}

pub type Empty = {
}

fn first_issue(issues: Array<Issue>) -> string {
  let first = issues[0]
  return "${first.code}|${first.message}"
}

fn outcome(v: unknown) -> string {
  return match Signup.parse(v) {
    Ok(s) => "ok",
    Err(issues) => first_issue(issues),
  }
}

fn expect_str(got: string, want: string) -> number {
  return match got == want {
    true => 0,
    false => 1,
  }
}

fn main() -> number {
  let absent = outcome({ email: "user@example.com" })
  let refined = outcome({ email: "user@example.com", password: "short" })
  let wrong = outcome({ email: 42, password: "longenough" })
  let arrayed = outcome([1, 2, 3])

  io.eprintln(absent)
  io.eprintln(refined)
  io.eprintln(wrong)
  io.eprintln(arrayed)

  let f = 0
  mut f = f + expect_str(absent, "missing|field `password` is required")
  mut f = f + expect_str(refined, "refinement|expected Password (string where value.length >= 8)")
  mut f = f + expect_str(wrong, "type|field `email` must be string")
  mut f = f + expect_str(arrayed, "type|expected Signup (an object), got an array")
  mut f = f + match Empty.parse([]) { Ok(e) => 1, Err(x) => 0, }
  mut f = f + match Empty.parse({}) { Ok(e) => 0, Err(x) => 1, }
  return f
}
"#,
    );
    let file = root.join("main.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "boundary parse gave {code} wrong answer(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            eprintln!("skipping: toolchain not found at run time");
        }
        other => panic!("boundary-issue program did not run: {other:?}"),
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
fn infer_output_cast_survives_a_match_in_return_position() {
    // The D28 boundary cast has to reach every `return` the function emits, not
    // just a single `return <value>`. `return match { ... }` lowers to a switch
    // whose arms carry their own `return`, and those bypassed the cast, so the
    // corpus combinator stopped compiling the moment its returned value came
    // from a match. Same body as examples/corpus/infer_output.glyph, one match.
    if !tsc_available() {
        eprintln!("skipping infer_output match-return check: tsc not available");
        return;
    }
    let root = unique_tmp("inferoutput_matchreturn");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "matchret.glyph",
        r#"module matchret

import std/result { Result, Ok, Err }

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

fn object_schema<Shape: Record<string, Schema<unknown>>>(shape: Shape, strict: bool) -> Schema<infer_output<Shape>> {
  return match strict {
    else => { name: "object", parse: fn(input) {
      match input {
        is Record<string, unknown> => {
          let issues: Array<Issue> = []
          let result: Record<string, unknown> = {}
          for key, sub_schema in shape {
            match sub_schema.parse(input[key]) {
              Ok(value) => {
                mut result[key] = value
              },
              Err(sub_issues) => {
                for issue in sub_issues {
                  mut issues.push({ path: [key, ...issue.path], message: issue.message })
                }
              },
            }
          }
          match issues.length {
            0 => Ok(result),
            else => Err(issues),
          }
        },
        else => Err([{ path: [], message: "expected object" }]),
      }
    } },
  }
}

type Point = {
  x: number,
  y: number,
}

const point_schema: Schema<Point> = object_schema({ x: number_schema(), y: number_schema() }, true)
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(
        !report.has_errors(),
        "the combinator is well-formed Glyph: {:?}",
        report.diagnostics
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!(
            "the D28 boundary cast did not reach the returns a `match` lowers to:\n{msg}"
        ),
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

// Named `CodecIssue`, not `Issue`: a module-local type called `Issue` shadows
// the ambient prelude one inside the emitted file, and every descriptor's
// `parse` annotates its error array as `Issue[]`. That shadowing is a known
// edge unrelated to what this test covers.
type CodecIssue = {
  path: Array<string>,
  message: string,
}

type Codec<T> = {
  parse: fn(input: unknown) -> Result<T, Array<CodecIssue>>,
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

/// A dev-loop tool watches a long-running subprocess while it runs, which needs
/// the async `spawn` rather than `spawnSync`: the child's `stdout`/`stderr` are
/// readable streams and the exit code arrives on a `close` event. The bundled
/// shim must type both without `@types/node`, or the callbacks fall back to
/// implicit `any` and `--strict` rejects them (TS7006).
#[test]
fn child_process_spawn_streams_typecheck_with_the_shim() {
    if !tsc_available() {
        eprintln!("skipping spawn-stream check: tsc not available");
        return;
    }
    let root = unique_tmp("spawnstream");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import child_process { spawn }
import std/io

pub async fn main(argv: Array<string>) -> number {
  let child = spawn("echo", ["hello"])
  child.stdout.on("data", fn(chunk) {
    io.println("${chunk}")
  })
  child.on("close", fn(code) {
    io.println("exit ${code}")
  })
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
            panic!("streaming child_process.spawn did not type-check with the bundled shim:\n{msg}")
        }
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

/// The guarantee `spawn` gives must not change with the spelling. `stdio:
/// "inherit"` hands back the *named* `ChildProcess`, whose three pipes are
/// declared nullable exactly as `@types/node` declares them, so a value routed
/// through `fn f(c: ChildProcess)` tells the same story as the value at the call
/// site: the property exists, and reaching it without a null check is the error.
/// The event names are literal types, so a misspelling is caught rather than
/// silently never firing.
#[test]
fn spawned_child_pipes_are_nullable_under_the_named_type() {
    if !tsc_available() {
        eprintln!("skipping spawn-nullability check: tsc not available");
        return;
    }
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};

    let main_glyph = "module main\nimport extern/child { tally }\nfn main(argv: Array<string>) -> number {\n  return tally()\n}\n";

    // The whole surface at once: the base type carries the pipes (guarded
    // access compiles), the default overload's pipes are non-null (unguarded
    // access compiles), and the `stdio` overload's result is still a
    // `ChildProcess`.
    let root = unique_tmp("spawnnull_ok");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "extern/child.ts",
        r#"import { spawn, type ChildProcess } from "child_process";

function tap(child: ChildProcess): number {
  let n = 0;
  child.stdout?.on("data", () => { n = n + 1; });
  child.stderr?.on("data", () => { n = n + 1; });
  child.stdin?.end();
  child.on("close", (code: number | null) => { n = n + (code ?? 0); });
  return n;
}

export function tally(): number {
  const quiet = spawn("echo", ["hi"], { stdio: "inherit" });
  const piped = spawn("echo", ["hi"]);
  piped.stdout.setEncoding("utf8");
  piped.stdout.on("data", () => {});
  return tap(quiet) + tap(piped);
}
"#,
    );
    write_file(&src, "main.glyph", main_glyph);
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diagnostics: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("the named ChildProcess surface did not type-check:\n{msg}"),
        TscOutcome::NotFound => {
            eprintln!("skipping: tsc not found at check time");
            return;
        }
    }

    // Each of these must be rejected. Without the nullable pipes on the base
    // type the first would either pass (pipes non-null everywhere) or fail with
    // "property does not exist", which is the wrong story; without literal
    // event names the second would pass and the handler would never fire.
    for (label, extern_ts) in [
        (
            "unguarded pipe on the named type",
            r#"import { type ChildProcess } from "child_process";
export function tally(): number {
  let n = 0;
  const use = (c: ChildProcess) => { c.stdout.on("data", () => { n = n + 1; }); };
  use(undefined as unknown as ChildProcess);
  return n;
}
"#,
        ),
        (
            "misspelled stream event name",
            r#"import { spawn } from "child_process";
export function tally(): number {
  let n = 0;
  spawn("echo", ["hi"]).stdout.on("datum", () => { n = n + 1; });
  return n;
}
"#,
        ),
        (
            "misspelled process event name",
            r#"import { spawn } from "child_process";
export function tally(): number {
  let n = 0;
  spawn("echo", ["hi"]).on("closed", () => { n = n + 1; });
  return n;
}
"#,
        ),
    ] {
        let root = unique_tmp("spawnnull_bad");
        let src = root.join("src");
        let out = root.join("dist");
        write_file(&src, "extern/child.ts", extern_ts);
        write_file(&src, "main.glyph", main_glyph);
        let report = build_project_inner(&src, &out, false).expect("build ok");
        assert!(!report.has_errors(), "diagnostics: {:?}", report.diagnostics);
        match check_with_tsc(&out).expect("run tsc") {
            TscOutcome::Passed => panic!("{label} was accepted by tsc"),
            TscOutcome::Failed(_) => {}
            TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
        }
    }
}

/// The shim must not export a name the real typings do not. `Signals` is the
/// one that nearly slipped through: `@types/node` keeps it in the global
/// `NodeJS` namespace and exports nothing by that name from `child_process`,
/// and a type declared inside `declare module "child_process"` is exported from
/// it with or without the `export` keyword. Declared there, this program would
/// build green with nothing installed and report `TS2305` the moment a user
/// followed the guide and installed `@types/node`, which is exactly the trap
/// G125 was about.
#[test]
fn child_process_does_not_export_the_signal_names() {
    if !tsc_available() {
        eprintln!("skipping shim-surface check: tsc not available");
        return;
    }
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};

    let root = unique_tmp("spawnsignals");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import child_process { spawn, Signals }

pub fn main(argv: Array<string>) -> number {
  let child = spawn("echo", ["hi"])
  return 0
}
"#,
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diagnostics: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {
            panic!("the shim exports `Signals` from `child_process`; `@types/node` does not")
        }
        TscOutcome::Failed(msg) => assert!(
            msg.contains("Signals"),
            "expected the rejection to name `Signals`, got:\n{msg}"
        ),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }

    // The names it *does* export have to keep working, or the check above would
    // pass for the wrong reason.
    let root = unique_tmp("spawnsignals_ok");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "extern/sig.ts",
        r#"import { spawn } from "child_process";

export function stop(): number {
  const child = spawn("sleep", ["10"]);
  const sig: NodeJS.Signals = "SIGTERM";
  return child.kill(sig) ? 0 : 1;
}
"#,
    );
    write_file(
        &src,
        "main.glyph",
        "module main\nimport extern/sig { stop }\nfn main(argv: Array<string>) -> number {\n  return stop()\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diagnostics: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("`NodeJS.Signals` did not type-check:\n{msg}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("a program with a `main` should run, not report NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check, so tsc is never required");
        }
    }
}

#[test]
fn a_code_set_from_inside_main_survives_a_void_return() {
    // `std/process.set_exit_code` promises the process leaves with the code it
    // recorded, and that the last call wins. The generated entrypoint broke
    // that for every `main` that does not return a number: it ran
    // `process.exitCode = typeof code === "number" ? code : 0` unconditionally
    // after `main` returned, so a void `main` that had recorded a failure
    // reported success to its caller. A batch CLI that rejects its input and
    // calls `set_exit_code(1)` exited 0, with no diagnostic anywhere.
    if !js_toolchain_available() {
        eprintln!("skipping run assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("setexit");
    write_file(
        &root,
        "setexit.glyph",
        "module setexit\n\
         import std/process\n\
         fn main(argv: Array<string>) -> void {\n\
        \x20 process.set_exit_code(4)\n\
         }\n",
    );
    let file = root.join("setexit.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 4, "the code recorded inside `main` is the exit code");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping run assertion: `tsx` not found on PATH");
        }
        other => panic!("expected the program to run: {other:?}"),
    }
}

#[test]
fn a_main_returning_a_number_still_wins_over_an_earlier_recorded_code() {
    // The other half: `set_exit_code` says the last verdict wins, and a
    // numeric `return` is the last thing `main` does. Not assigning the code
    // for a void `main` must not turn into never assigning it.
    if !js_toolchain_available() {
        eprintln!("skipping run assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("setexitret");
    write_file(
        &root,
        "setexitret.glyph",
        "module setexitret\n\
         import std/process\n\
         fn main(argv: Array<string>) -> number {\n\
        \x20 process.set_exit_code(4)\n\
        \x20 return 5\n\
         }\n",
    );
    let file = root.join("setexitret.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 5, "the return value is the later verdict");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping run assertion: `tsx` not found on PATH");
        }
        other => panic!("expected the program to run: {other:?}"),
    }
}

#[test]
fn a_void_main_that_records_nothing_still_exits_zero() {
    // And the default is unchanged: a `main` that returns nothing and records
    // nothing leaves `process.exitCode` unset, which Node reports as 0.
    if !js_toolchain_available() {
        eprintln!("skipping run assertion: node/tsx not available");
        return;
    }
    let root = unique_tmp("voidmain");
    write_file(
        &root,
        "voidmain.glyph",
        "module voidmain\n\
         import std/io\n\
         fn main(argv: Array<string>) -> void {\n\
        \x20 io.println(\"done\")\n\
         }\n",
    );
    let file = root.join("voidmain.glyph");
    match glyph_cli::run::run_file(&file, &[], false, false).expect("run_file ok").outcome {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a program that records nothing exits 0");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping run assertion: `tsx` not found on PATH");
        }
        other => panic!("expected the program to run: {other:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn async_thunks_mapped_and_awaited_run() {
    // F11/F12: concurrency spelled the way D40 names it. `array.map` builds an
    // `Array<async fn() -> T>` and `task.all` awaits them, which is the idiom
    // two example apps already use. This was written against `par.all`, whose
    // `Array<T | Promise<T>>` is the one shape D40 refuses to name, and keeping
    // it was what blocked modeling `array.map` at all (G99).
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
import std/task

async fn work(n: number) -> number {
  return n * 2
}

fn task_for(n: number) -> async fn() -> number {
  return async fn() -> number { return await work(n) }
}

async fn run(items: Array<number>) -> Array<number> {
  return await task.all(array.map(items, task_for))
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
            assert_eq!(code, 0, "async thunks + task.all produced a wrong result");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping async-closure run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("async-closure program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
                "import { Ok as __glyph_ok, Err as __glyph_err, type Result as __GlyphResult } from \"./.glyph-runtime/std/result\";"
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => panic!("has main; got NoMain: {exports:?}"),
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("program has a main; got NoMain: {exports:?}");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            unreachable!("run was --no-check");
        }
    }
}

#[test]
fn a_two_binding_for_over_a_match_result_binds_a_numeric_index() {
    // G37: `record.get` -> `match` -> two-binding `for` took the record
    // (`Object.entries`) lowering, so `i` bound the string `"0"` and the loop
    // printed `01:x` / `11:y` on a build that reported no diagnostics and
    // passed `tsc --strict`. Two independent causes, both needed: `std/record`
    // was unmodeled, so the scrutinee was `Unknown`; and the arm join treated
    // the `None => []` arm's `Array<Unknown>` as disagreeing with the other
    // arm's `Array<string>`. This runs the shape, because the whole point is
    // that the emitted TypeScript type-checks either way.
    if !js_toolchain_available() {
        eprintln!("skipping for-index run: node/tsx not available");
        return;
    }
    let root = unique_tmp("foridx");
    let prog = "module main\n\
         import std/record\n\
         import std/string\n\
         \n\
         fn from_record(t: Record<string, Array<string>>, k: string) -> string {\n\
         \x20 let path = match record.get(t, k) {\n\
         \x20\x20\x20 Some(p) => p,\n\
         \x20\x20\x20 None => [\"z\",],\n\
         \x20 }\n\
         \x20 let out: Array<string> = []\n\
         \x20 for i, hop in path {\n\
         \x20\x20\x20 mut out.push(\"${i + 1}:${hop}\")\n\
         \x20 }\n\
         \x20 return string.join(out, \"|\")\n\
         }\n\
         \n\
         fn from_empty_literal(o: Option<Array<string>>) -> string {\n\
         \x20 let path = match o {\n\
         \x20\x20\x20 Some(p) => p,\n\
         \x20\x20\x20 None => [],\n\
         \x20 }\n\
         \x20 let out: Array<string> = []\n\
         \x20 for i, hop in path {\n\
         \x20\x20\x20 mut out.push(\"${i + 1}:${hop}\")\n\
         \x20 }\n\
         \x20 return string.join(out, \"|\")\n\
         }\n\
         \n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let t: Record<string, Array<string>> = { \"a\": [\"x\", \"y\",], }\n\
         \x20 return match from_record(t, \"a\") == \"1:x|2:y\" {\n\
         \x20\x20\x20 false => 1,\n\
         \x20\x20\x20 true => match from_empty_literal(Some([\"x\", \"y\",])) == \"1:x|2:y\" {\n\
         \x20\x20\x20\x20\x20 false => 2,\n\
         \x20\x20\x20\x20\x20 true => 0,\n\
         \x20\x20\x20 },\n\
         \x20 }\n\
         }\n";
    write_file(&root, "foridx.glyph", prog);
    let file_glyph = root.join("foridx.glyph");
    match glyph_cli::run::run_file(&file_glyph, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "1 = the record.get shape kept the string-key lowering; \
                 2 = the empty-array-literal arm did"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            eprintln!("skipping for-index run: `tsx` not found on PATH");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("unexpected type-check failure: {msg}");
        }
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
    // The fixture has to be one Glyph's own checker passes, or this stops
    // testing the tsc stage. Its previous body was `let n: number =
    // string.upper("hi")`, which 0.1.99 started catching as `E0204` before tsc
    // ever ran (G149), so the outcome became `BuildFailed` and the test failed
    // on the improvement it was measuring. `TSC_RED_GLYPH_CLEAN` is
    // Glyph-clean by construction and red at `tsc`, which is the shape this
    // test needs.
    write_file(&root, "broken.glyph", TSC_RED_GLYPH_CLEAN);
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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
fn namespaced_record_payload_union_match_binds_whole_object() {
    // The same imported union matched through its *namespace* spelling
    // (`err.BadLeadByte(v)`) rather than a named import. The variant name is
    // never bound in the consumer's symbol table under that spelling, so the
    // by-name lookup missed it and the emitter fell back to `v.value`
    // (TS2339). The scrutinee's own imported type now answers the question, so
    // both spellings of the same declaration bind identically.
    let root = unique_tmp("nsunion");
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
         import err\n\
         import std/string { from }\n\
         pub fn describe(e: err.E) -> string {\n\
         \x20 return match e {\n\
         \x20\x20\x20 err.BadLeadByte(v) => from(v.at),\n\
         \x20\x20\x20 err.Empty => \"empty\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    assert!(ts.contains("const v = __m0;"), "{ts}");
}

#[test]
fn namespaced_variant_shape_comes_from_the_scrutinee_not_a_same_named_import() {
    // Two modules declare a variant of the same name with different payload
    // shapes: `a.Hit` carries a record, `b.Hit` a single value. The consumer
    // imports `a`'s `Hit` by name and matches `b`'s through the namespace.
    //
    // The by-name lookup answers "is there any symbol named `Hit` here whose
    // source module registers it as a record payload" without ever consulting
    // what is being matched, so it claimed the record shape for `b.Hit` and
    // bound the whole object where the value belonged (TS2322). The scrutinee's
    // own type is the deciding axis and is now asked first; the by-name lookup
    // is the fallback for a scrutinee whose type is not `Ty::Imported`.
    let root = unique_tmp("nscollide");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "a.glyph",
        "module a\npub type A =\n  | Hit({ x: number })\n  | Miss\n",
    );
    write_file(
        &src,
        "b.glyph",
        "module b\npub type B =\n  | Hit(number)\n  | Gone\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import a { A, Hit, Miss }\n\
         import b\n\
         pub fn from_b(v: b.B) -> number {\n\
         \x20 return match v {\n\
         \x20\x20\x20 b.Hit(n) => n,\n\
         \x20\x20\x20 b.Gone => 0,\n\
         \x20 }\n\
         }\n\
         pub fn from_a(v: A) -> number {\n\
         \x20 return match v {\n\
         \x20\x20\x20 Hit(r) => r.x,\n\
         \x20\x20\x20 Miss => 0,\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    // `b.Hit`'s single value comes from `.value`; `a.Hit`'s record is the
    // whole object. One name, two shapes, each read off its own scrutinee.
    assert!(ts.contains("const n = __m0.value;"), "{ts}");
    assert!(ts.contains("const r = __m1;"), "{ts}");
}

#[test]
fn inferred_let_scrutinee_types_from_the_cross_module_call_it_binds() {
    // `let v = a.make()` with no type annotation: the call is cross-module, so
    // inference has to resolve the callee's return type across the module
    // boundary to type `v`. When that resolution didn't happen, `v` fell back
    // to `Ty::Unknown`, the `variant_payload_is_record` check on the match arm
    // couldn't fire, and the emitter fell back to `const r = __m0.value;` on a
    // record payload, which tsc rejects (TS2339: no `value` on the tagged
    // union member). The annotated spelling (`let v: a.A = a.make()`) and a
    // plain parameter both already worked; only the inferred `let` was broken.
    let root = unique_tmp("inferred-let-scrutinee");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "a.glyph",
        "module a\npub type A =\n  | Hit({ x: number })\n  | Miss\npub fn make() -> A {\n  return Hit({ x: 7, })\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import a\n\
         import std/string { from }\n\
         pub fn describe() -> string {\n\
         \x20 let v = a.make()\n\
         \x20 return match v {\n\
         \x20\x20\x20 a.Hit(r) => from(r.x),\n\
         \x20\x20\x20 a.Miss => \"none\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    // The record payload is the whole object, not `.value`.
    assert!(ts.contains("const r = __m0;") && !ts.contains("const r = __m0.value"), "{ts}");
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
fn nested_imported_nullary_variant_dispatches_on_the_inner_tag() {
    // `Err(EmptyOctet) => .., Err(e) => ..` where the payload union is
    // *imported*: the emitter read the inner PascalCase ident as a payload
    // binding and emitted two `case "Err":` labels, so the first arm swallowed
    // every parse error and the second never ran. tsc accepts a duplicate case
    // label, so nothing anywhere reported it. The payload union's variant list
    // is not readable across the boundary here, so this is the case the name's
    // shape has to answer: a constructor-shaped bare ident in pattern position
    // is a variant reference, never a binding.
    let root = unique_tmp("nested-nullary-imported");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "net.glyph",
        "module net\npub type ParseError =\n  | EmptyOctet\n  | NotANumber({ got: string })\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import net { ParseError, EmptyOctet, NotANumber }\n\
         import std/result { Result, Ok, Err }\n\
         pub fn describe(r: Result<int, ParseError>) -> string {\n\
         \x20 return match r {\n\
         \x20\x20\x20 Err(EmptyOctet) => \"empty\",\n\
         \x20\x20\x20 Err(e) => \"other\",\n\
         \x20\x20\x20 Ok(v) => \"ok\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    assert_eq!(
        ts.matches("case \"Err\"").count(),
        1,
        "one `case \"Err\"` with an inner tag switch, not the duplicate-label miscompile:\n{ts}"
    );
    assert!(ts.contains("case \"EmptyOctet\""), "{ts}");
    assert!(!ts.contains("const EmptyOctet ="), "{ts}");
}

#[test]
fn nested_lowercase_imported_nullary_variant_dispatches_on_the_inner_tag() {
    // G147: the same shape as the test above, but the nullary variant is
    // spelled lowercase (`empty`, not `EmptyOctet`). A lowercase bare ident in
    // pattern position is still a legal variant reference in Glyph (D9 does
    // not require PascalCase), but the emitter's only fallback for an
    // unresolvable imported payload union was the name's *shape*
    // (`is_variant_shaped`), which is uppercase-only. `Err(empty)` read as a
    // fresh binding instead of a variant reference, so it and the following
    // `Err(e) => ..` both lowered to `case "Err":` and the build refused to
    // emit the duplicate label (E0305) rather than silently letting the first
    // arm swallow every error. If this regresses, a real program with a
    // lowercase nullary variant in an imported union (`empty`, `none`,
    // `blank`, any name D9 does not force PascalCase on) stops compiling.
    let root = unique_tmp("nested-nullary-imported-lowercase");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "net.glyph",
        "module net\npub type ParseError =\n  | empty\n  | NotANumber({ got: string })\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import net { ParseError, empty, NotANumber }\n\
         import std/result { Result, Ok, Err }\n\
         pub fn describe(r: Result<int, ParseError>) -> string {\n\
         \x20 return match r {\n\
         \x20\x20\x20 Err(empty) => \"empty\",\n\
         \x20\x20\x20 Err(e) => \"other\",\n\
         \x20\x20\x20 Ok(v) => \"ok\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    assert_eq!(
        ts.matches("case \"Err\"").count(),
        1,
        "one `case \"Err\"` with an inner tag switch, not the duplicate-label miscompile:\n{ts}"
    );
    assert!(ts.contains("case \"empty\""), "{ts}");
    assert!(!ts.contains("const empty ="), "{ts}");
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
    // G74: the imported path renders the missing variants exactly like the
    // module-local one — backticked. One rule, one diagnostic shape.
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("`NotANumber`")),
        "missing variants must be backticked on the imported path: {:?}",
        report.diagnostics
    );
}

/// G143: the imported-union coverage check only ever counted the outer
/// union's variant tags, with no equivalent of `check_patterns_exhaustive`'s
/// recursion into a constructor arm's payload. `B(X)` over an imported
/// `type G = A | B(Inner)`, where `Inner = X | Y` is a sibling declaration in
/// the same imported module, marked `B` fully covered without ever looking
/// at whether the nested `Inner` pattern covered `X` alone. The module-local
/// spelling of the identical program was already caught by
/// `a_generic_union_whose_payload_is_a_union_recurses_into_the_payload`'s
/// non-generic twin; only the cross-module path had the hole.
///
/// If this regresses, a program built clean and passed `tsc --strict` with
/// `Y` completely unhandled, and `f(B(Y))` threw `Error: non-exhaustive
/// match` at run time instead of failing at compile time.
#[test]
fn an_alias_to_a_union_is_not_accused_of_having_no_variants() {
    // The G146 check asks whether a payload type is provably variant-free
    // before flagging a constructor-shaped sub-pattern under it. Its first form
    // asked the weaker question "is this declaration body syntactically a
    // union", and an alias to a union is a path rather than a union. So
    // `type MaybeAge = Option<int>` was ruled variant-free and `Ok(Some(n))`
    // over it drew two E0220s on a program 0.1.95 compiled and emitted
    // correctly. Rejecting working code is worse than the missing diagnostic
    // the check exists to supply, so an accusation needs certainty here and
    // silence is the safe answer.
    let root = unique_tmp("aliasunionpayload");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/result { Result, Ok, Err }\n\
         import std/option { Option, Some, None }\n\
         pub type MaybeAge = Option<int>\n\
         pub fn f(r: Result<MaybeAge, string>) -> string {\n\
         \x20 return match r {\n\
         \x20\x20\x20 Ok(Some(n)) => \"some\",\n\
         \x20\x20\x20 Ok(None) => \"none\",\n\
         \x20\x20\x20 Err(e) => \"err\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.diagnostics.iter().any(|d| format!("{d:?}").contains("E0220")),
        "an alias to a union must not be accused of having no variants: {:?}",
        report.diagnostics
    );
    assert!(
        !report.has_errors(),
        "this program compiles on 0.1.95 and must keep compiling: {:?}",
        report.diagnostics
    );
}

#[test]
fn a_local_unions_imported_payload_is_exhaustiveness_checked() {
    // The mirror of the imported-outer case, and it was still a silent
    // miscompile after that one was fixed: a union declared *here* whose
    // variant payload is a union declared *elsewhere*. `named_union_variants`
    // resolves only `Ty::Named`, so there was no variant list to require and
    // the inner match went unchecked. It built clean, passed `tsc --strict`,
    // and threw `non-exhaustive match` at run time.
    //
    // Which side of the boundary is the imported half must not decide whether
    // the compiler checks the match. This is the fifth time this project has
    // fixed one site of this shape and found a sibling still broken, which is
    // why the sibling gets its own test rather than a note.
    let root = unique_tmp("localunionimportedpayload");
    let src = root.join("src");
    write_file(
        &src,
        "inner.glyph",
        "module inner\n\
         pub type Inner =\n\
         \x20 | X\n\
         \x20 | Y\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import inner { Inner, X, Y }\n\
         type Outer =\n\
         \x20 | A\n\
         \x20 | B(Inner)\n\
         pub fn label(o: Outer) -> string {\n\
         \x20 return match o {\n\
         \x20\x20\x20 A => \"a\",\n\
         \x20\x20\x20 B(X) => \"bx\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.has_errors(),
        "an omitted variant of a local union's imported payload must be caught: {:?}",
        report.diagnostics
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| format!("{d:?}").contains("E0200") && format!("{d:?}").contains('Y')),
        "expected E0200 naming the missing inner variant: {:?}",
        report.diagnostics
    );
}

#[test]
fn nested_payload_of_an_imported_union_is_exhaustiveness_checked() {
    let root = unique_tmp("impnestedexhaust");
    let src = root.join("src");
    write_file(
        &src,
        "tree.glyph",
        "module tree\n\
         pub type Inner =\n\
         \x20 | X\n\
         \x20 | Y\n\
         pub type G =\n\
         \x20 | A\n\
         \x20 | B(Inner)\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io { println }\n\
         import tree { X, Y, G, A, B }\n\
         fn f(g: G) -> string {\n\
         \x20 return match g {\n\
         \x20\x20\x20 A => \"a\",\n\
         \x20\x20\x20 B(X) => \"x\",\n\
         \x20 }\n\
         }\n\
         fn main() {\n\
         \x20 println(f(B(Y)))\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.has_errors(),
        "a missing nested variant of an imported union's payload must be caught: {:?}",
        report.diagnostics
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Inner") && d.contains("`Y`")),
        "the missing inner variant `Y` must be reported by name through the \
         imported outer union `G`: {:?}",
        report.diagnostics
    );

    // The control: the identical two declarations moved into one module
    // build clean once `Y` is handled — this is not a false positive on the
    // valid spelling.
    let ok_root = unique_tmp("impnestedexhaustok");
    let ok_src = ok_root.join("src");
    write_file(
        &ok_src,
        "tree.glyph",
        "module tree\n\
         pub type Inner =\n\
         \x20 | X\n\
         \x20 | Y\n\
         pub type G =\n\
         \x20 | A\n\
         \x20 | B(Inner)\n",
    );
    write_file(
        &ok_src,
        "main.glyph",
        "module main\n\
         import tree { X, Y, G, A, B }\n\
         fn f(g: G) -> string {\n\
         \x20 return match g {\n\
         \x20\x20\x20 A => \"a\",\n\
         \x20\x20\x20 B(X) => \"x\",\n\
         \x20\x20\x20 B(Y) => \"y\",\n\
         \x20 }\n\
         }\n",
    );
    let ok_report = build_project_inner(&ok_src, &ok_root.join("dist"), false).expect("build");
    assert!(
        !ok_report.has_errors(),
        "covering every nested variant must build clean: {:?}",
        ok_report.diagnostics
    );
}

/// A *generic* tagged union in a sibling module, for the tests below. The
/// non-generic spelling of the same declaration is already covered by
/// `non_exhaustive_imported_union_match_is_caught`; the only difference here is
/// the type parameter.
const TREE_MODULE: &str = "module tree\n\
     pub type Tree<K> =\n\
     \x20 | Leaf\n\
     \x20 | Node({ left: Tree<K>, key: K, right: Tree<K> })\n";

#[test]
fn exhaustive_match_on_imported_generic_union_builds_clean() {
    // The other half of `an_imported_generic_union_is_exhaustiveness_checked`:
    // covering every variant of the generic imported union has to stay clean,
    // so the widened gate is not paid for with a false E0200 on every correct
    // match.
    let root = unique_tmp("genimpexhaustok");
    let src = root.join("src");
    write_file(&src, "tree.glyph", TREE_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import tree { Tree, Leaf, Node }\n\
         pub fn label(t: Tree<string>) -> string {\n\
         \x20 return match t {\n\
         \x20\x20\x20 Leaf => \"leaf\",\n\
         \x20\x20\x20 Node(n) => n.key,\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.has_errors(),
        "an exhaustive match on a generic imported union must build clean: {:?}",
        report.diagnostics
    );
}

#[test]
fn namespace_qualified_match_on_imported_generic_union_is_exhaustiveness_checked() {
    // The namespace spelling of the same scrutinee (`tree.Tree<string>`) also
    // lowers through `TypeExpr::Generic`, so it reaches the gate as an
    // application too. D9 does not depend on the import spelling, and it does
    // not depend on the union's arity either.
    let root = unique_tmp("genimpexhaustns");
    let src = root.join("src");
    write_file(&src, "tree.glyph", TREE_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import tree\n\
         pub fn label(t: tree.Tree<string>) -> string {\n\
         \x20 return match t {\n\
         \x20\x20\x20 tree.Node(n) => n.key,\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("`Leaf`")),
        "diags: {:?}",
        report.diagnostics
    );
}

/// A three-variant union in a sibling module, for the namespace-import
/// exhaustiveness tests below. Written once so every spelling is checked
/// against the same declaration.
const COND_MODULE: &str = "module model\n\
     pub type Cond =\n\
     \x20 | Yes({ k: string })\n\
     \x20 | No({ k: string })\n\
     \x20 | Maybe({ k: string })\n";

#[test]
fn namespace_qualified_match_on_imported_union_is_exhaustiveness_checked() {
    // `import model` + `model.Yes(_)` arms got no exhaustiveness check at all:
    // the variant name is not a symbol under a namespace import, so the
    // imported-union lookup missed and the match was silently accepted. The
    // build reported no diagnostics and `tsc --strict` passed, then the program
    // threw `non-exhaustive match` at runtime. The same match with a named
    // import was E0200. D9 does not depend on the import spelling.
    let root = unique_tmp("nsexhaust");
    let src = root.join("src");
    write_file(&src, "model.glyph", COND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import model\n\
         pub fn name(c: model.Cond) -> string {\n\
         \x20 return match c {\n\
         \x20\x20\x20 model.Yes(_) => \"yes\",\n\
         \x20\x20\x20 model.No(_) => \"no\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Maybe")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn aliased_namespace_match_on_imported_union_is_exhaustiveness_checked() {
    // `import model as m` interns an `ImportAlias` rather than an
    // `ImportNamespace`; both resolve through the import's own path, so the
    // aliased spelling is held to the same bar.
    let root = unique_tmp("aliasexhaust");
    let src = root.join("src");
    write_file(&src, "model.glyph", COND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import model as m\n\
         pub fn name(c: m.Cond) -> string {\n\
         \x20 return match c {\n\
         \x20\x20\x20 m.Yes(_) => \"yes\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("No") && d.contains("Maybe")),
        "diags: {:?}",
        report.diagnostics
    );
}

/// A string-literal union in a sibling module, for the D30 cross-module tests
/// below. Written once so every import spelling is checked against the same
/// declaration.
const KIND_MODULE: &str = "module catalog\npub type Kind = \"a\" | \"b\"\n";

#[test]
fn imported_string_literal_union_match_is_exhaustive_without_else() {
    // D30 promises a match over a string-literal union is exhaustive without an
    // `else`. Importing the type used to lower it to `Ty::Unknown`, so the same
    // match drew E0218 with help text telling the author to add the `else` that
    // destroys the guarantee. The named-import spelling.
    let root = unique_tmp("d30named");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Kind }\n\
         pub fn label(k: Kind) -> string {\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"a\" => \"first\",\n\
         \x20\x20\x20 \"b\" => \"second\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.has_errors(),
        "an exhaustive match on an imported string-literal union must build \
         with no `else`: {:?}",
        report.diagnostics
    );
}

#[test]
fn imported_string_literal_union_missing_literal_is_e0200() {
    // The other half: omitting a literal is a *missing value* error (E0200), not
    // a "needs a catch-all" error (E0218). E0218 would be the compiler telling
    // the author to delete the guarantee.
    let root = unique_tmp("d30missing");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Kind }\n\
         pub fn label(k: Kind) -> string {\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"a\" => \"first\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains('b')),
        "a missing literal must be E0200: {:?}",
        report.diagnostics
    );
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0218")),
        "E0218 would coach the author into adding the `else` that destroys \
         D30's guarantee: {:?}",
        report.diagnostics
    );
}

#[test]
fn namespace_qualified_imported_string_literal_union_is_exhaustiveness_checked() {
    // `import catalog` + `catalog.Kind` is the spelling csvql actually used.
    // D30 must not depend on which legal import spelling brought the type in.
    let root = unique_tmp("d30ns");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog\n\
         pub fn label(k: catalog.Kind) -> string {\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"a\" => \"first\",\n\
         \x20\x20\x20 \"b\" => \"second\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.has_errors(),
        "namespace-qualified spelling must be exhaustive without an `else`: {:?}",
        report.diagnostics
    );

    let root = unique_tmp("d30nsmissing");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog\n\
         pub fn label(k: catalog.Kind) -> string {\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"a\" => \"first\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "a missing literal must be E0200 under the namespace spelling: {:?}",
        report.diagnostics
    );
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0218")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn aliased_namespace_imported_string_literal_union_is_exhaustiveness_checked() {
    // `import catalog as c` interns an `ImportAlias`; both namespace forms
    // resolve through the import's own path, so they are held to the same bar.
    let root = unique_tmp("d30alias");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog as c\n\
         pub fn label(k: c.Kind) -> string {\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"a\" => \"first\",\n\
         \x20\x20\x20 \"b\" => \"second\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.has_errors(),
        "aliased spelling must be exhaustive without an `else`: {:?}",
        report.diagnostics
    );

    let root = unique_tmp("d30aliasmissing");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog as c\n\
         pub fn label(k: c.Kind) -> string {\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"b\" => \"second\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "a missing literal must be E0200 under the alias spelling: {:?}",
        report.diagnostics
    );
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0218")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn imported_string_literal_union_in_let_annotation_is_exhaustiveness_checked() {
    // The Assigner's own lowerer (not just the `decl_ty` query) has to reach
    // across, or a `let` annotation naming the imported type stays Unknown.
    let root = unique_tmp("d30let");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", KIND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Kind }\n\
         pub fn label() -> string {\n\
         \x20 let k: Kind = \"a\"\n\
         \x20 return match k {\n\
         \x20\x20\x20 \"a\" => \"first\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "a `let`-annotated imported union must be checked: {:?}",
        report.diagnostics
    );
}

#[test]
fn namespace_qualified_match_on_prelude_option_is_exhaustiveness_checked() {
    // `option.Option<string>` used to lower to `Ty::Unknown` (the two-segment
    // stdlib table knew only the three `fs.*` types), so a match over it fell
    // into the imported-union path and was never checked. The most-used union
    // in the language lost D9 to a one-token change in how it was imported.
    let root = unique_tmp("nsoption");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/option\n\
         pub fn unwrap(o: option.Option<string>) -> string {\n\
         \x20 return match o {\n\
         \x20\x20\x20 option.Some(s) => s,\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("None")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn namespace_qualified_match_on_prelude_result_is_exhaustiveness_checked() {
    let root = unique_tmp("nsresult");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/result\n\
         pub fn ok_or(r: result.Result<string, string>) -> string {\n\
         \x20 return match r {\n\
         \x20\x20\x20 result.Ok(v) => v,\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Err")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn namespace_qualified_misspelled_variant_is_a_glyph_diagnostic() {
    // A misspelled qualified head was inserted into the covered set unexamined,
    // so it reached `tsc` and came back as a raw TS2678. E0220 belongs here,
    // pointing at the arm, with the nearest-variant hint.
    let root = unique_tmp("nstypo");
    let src = root.join("src");
    write_file(&src, "model.glyph", COND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import model\n\
         pub fn name(c: model.Cond) -> string {\n\
         \x20 return match c {\n\
         \x20\x20\x20 model.Yes(_) => \"yes\",\n\
         \x20\x20\x20 model.Nooo(_) => \"typo\",\n\
         \x20\x20\x20 model.Maybe(_) => \"maybe\",\n\
         \x20\x20\x20 else => \"other\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0220") && d.contains("Nooo")),
        "diags: {:?}",
        report.diagnostics
    );
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("TS2678")),
        "the typo must not leak to tsc: {:?}",
        report.diagnostics
    );
}

#[test]
fn namespace_qualified_match_with_else_arm_is_accepted() {
    // Guard against over-firing: an `else` catch-all forfeits D9 by choice and
    // is legal, so the new check must not report a missing variant.
    let root = unique_tmp("nselse");
    let src = root.join("src");
    write_file(&src, "model.glyph", COND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import model\n\
         pub fn name(c: model.Cond) -> string {\n\
         \x20 return match c {\n\
         \x20\x20\x20 model.Yes(_) => \"yes\",\n\
         \x20\x20\x20 else => \"other\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
}

#[test]
fn namespace_qualified_match_covering_every_variant_is_clean() {
    // The other over-firing guard: a fully covered namespace-form match, plus a
    // qualified pattern whose head is a stdlib namespace that owns no project
    // union (`fs.ErrorKind.NotFound`), must both stay silent.
    let root = unique_tmp("nsfull");
    let src = root.join("src");
    write_file(&src, "model.glyph", COND_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import model\n\
         pub fn name(c: model.Cond) -> string {\n\
         \x20 return match c {\n\
         \x20\x20\x20 model.Yes(_) => \"yes\",\n\
         \x20\x20\x20 model.No(_) => \"no\",\n\
         \x20\x20\x20 model.Maybe(_) => \"maybe\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(root.join("dist").join("main.ts")).unwrap();
    assert!(ts.contains("case \"Maybe\":"), "{ts}");
}

#[test]
fn question_operator_accepts_a_namespace_qualified_result_return() {
    // Lowering `result.Result<T, E>` to the prelude type made it decidable, and
    // the `?` rule only recognized the prelude and `ImportNamed` spellings, so
    // every `?` in a function returning `result.Result<_, _>` was rejected as
    // sitting outside a `Result` function.
    let root = unique_tmp("nsquestion");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/result\n\
         fn inner(x: int) -> result.Result<int, string> {\n\
         \x20 return match x >= 0 {\n\
         \x20\x20\x20 true => result.Ok(x),\n\
         \x20\x20\x20 false => result.Err(\"neg\"),\n\
         \x20 }\n\
         }\n\
         pub fn outer(x: int) -> result.Result<int, string> {\n\
         \x20 let v = inner(x)?\n\
         \x20 return result.Ok(v + 1)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
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
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
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

/// Run `glyph` under a wall-clock budget, and kill it if it outlives one.
///
/// `None` means the process was still alive at the deadline. Every other
/// helper here waits forever, which is the one thing a test about a program
/// that never exits cannot do: without a budget the hang becomes the harness's
/// hang and the suite stops instead of failing.
fn spawn_glyph_bounded(
    args: &[&std::ffi::OsStr],
    budget: std::time::Duration,
) -> Option<(i32, String, String)> {
    use std::io::Read;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"));
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // `glyph run` compiles and then spawns node, which inherits these pipes and
    // is the process that actually hangs. Killing only `glyph` leaves node
    // holding the write end and running, so the timeout path has to be able to
    // reach the whole tree; its own process group is what makes that possible.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().expect("spawn glyph");
    // Drained on their own threads. A child that fills a pipe buffer blocks on
    // the write, which would look exactly like the hang under test.
    let mut out_pipe = child.stdout.take().expect("stdout is piped");
    let mut err_pipe = child.stderr.take().expect("stderr is piped");
    let out_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out_pipe.read_to_string(&mut text);
        text
    });
    let err_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = err_pipe.read_to_string(&mut text);
        text
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait().expect("wait on glyph") {
            Some(status) => {
                let stdout = out_reader.join().unwrap_or_default();
                let stderr = err_reader.join().unwrap_or_default();
                return Some((status.code().unwrap_or(-1), stdout, stderr));
            }
            None if started.elapsed() >= budget => {
                kill_the_whole_tree(&mut child);
                // The readers are left to finish on their own. Joining them here
                // is what turned "the program hung" into "the test run hung" the
                // first time this was written.
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// Kill a `glyph` process and everything it started.
#[cfg(unix)]
fn kill_the_whole_tree(child: &mut std::process::Child) {
    // A negative pid names the process group, so the node the CLI spawned dies
    // with it rather than outliving the test suite.
    let _ = std::process::Command::new("kill")
        .arg("-9")
        .arg(format!("-{}", child.id()))
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn kill_the_whole_tree(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
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
             \x20 let _ = fs.write_text({:?}, \"ran\")\n\
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

#[test]
fn a_variant_named_after_a_js_global_keeps_its_name() {
    // G63. `emit_variant_constructor` writes `export function Error(...)`
    // straight from the Glyph name, so every `new Error(...)` the emitter wrote
    // below it called the variant instead. That was first a `tsc` error in the
    // wrong place, then an E0110 that took the name away from the author.
    // A spreadsheet cell really is `Number | Text | Empty | Error`, so the name
    // stays and the module captures the global the compiler needs instead.
    let root = unique_tmp("shadowglobal");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type Value =\n\
         \x20 | Num(number)\n\
         \x20 | Error(string)\n\
         \n\
         fn describe(v: Value) -> string {\n\
         \x20 return match v {\n\
         \x20   Num(n) => number.to_string(n),\n\
         \x20   Error(e) => e,\n\
         \x20 }\n\
         }\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 print(describe(Num(1)))\n\
         \x20 return 0\n\
         }\n",
    );

    let out = root.join("out");
    let report = build_project_inner(&src, &out, false).expect("build ran");
    assert!(!report.has_errors(), "compiles now: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();

    // The author's name survives verbatim, which is the half that matters for
    // grep: `grep "Error"` finds the variant, not a mangled stand-in.
    assert!(
        ts.contains("function Error("),
        "the variant keeps its name: {ts}"
    );
    // The compiler's own reference goes through the captured global instead.
    assert!(
        ts.contains("const __glyph_Error = globalThis.Error;"),
        "the module captures the real Error: {ts}"
    );
    assert!(
        ts.contains("throw new __glyph_Error("),
        "the emitter's throw uses the capture: {ts}"
    );
    assert!(
        !ts.contains("throw new Error("),
        "and never the shadowed name: {ts}"
    );
    // And the program actually emits, which the old behaviour never let it do.
    assert!(!report.emitted.is_empty(), "emitted nothing: {:?}", report.emitted);
}

#[test]
fn a_bare_primitive_union_is_rejected_with_the_misparse_explained() {
    // `type Key = string | number` built clean, passed `tsc --strict`, and
    // emitted `export const string` / `export const number` that shadowed the
    // prelude. It is a D8 tagged union with two badly named variants.
    let root = unique_tmp("primunion");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type Key = string | number\n\
         fn main(argv: Array<string>) -> number { return 0 }\n",
    );

    let report = build_project_inner(&src, &root.join("out"), false).expect("build ran");
    assert!(report.has_errors(), "diags: {:?}", report.diagnostics);
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.contains("E0111"))
        .unwrap_or_else(|| panic!("no E0111; diagnostics were: {:?}", report.diagnostics));
    assert!(
        diag.contains("extern_ts"),
        "the help must name the escape hatch: {diag}"
    );
    assert!(
        diag.contains("tagged union"),
        "the help must explain what it actually parsed as: {diag}"
    );
    assert!(report.emitted.is_empty(), "emitted: {:?}", report.emitted);
}

// ---------------------------------------------------------------------------
// G75: an imported record keeps its identity across a module boundary
// ---------------------------------------------------------------------------

/// A record type in a sibling module, for the cross-module record tests below.
/// Written once so every import spelling is checked against the same
/// declaration.
const SHEET_MODULE: &str = "module catalog\npub type Sheet = { rows: Array<Array<string>> }\n";

#[test]
fn named_imported_record_field_loops_numerically() {
    // `for i, x` over an `Array` binds a number. An imported record's field used
    // to type as `Ty::Unknown`, so the emitter could not tell an array from a
    // record and fell back to `Object.entries`, binding the string `"0"`. The
    // build stayed green, which is what made it a silent miscompile.
    let root = unique_tmp("g75named");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", SHEET_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         import catalog { Sheet }\n\
         pub fn show(s: Sheet) -> void {\n\
         \x20 for i, r in s.rows {\n\
         \x20\x20\x20 io.println(\"row\")\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(root.join("dist").join("main.ts")).unwrap();
    assert!(ts.contains("s.rows.entries()"), "array loop expected: {ts}");
    assert!(!ts.contains("Object.entries"), "record loop emitted: {ts}");
}

#[test]
fn namespace_qualified_imported_record_field_loops_numerically() {
    // `import catalog` + `catalog.Sheet`. The guarantee must not depend on
    // which legal spelling brought the type into scope.
    let root = unique_tmp("g75ns");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", SHEET_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         import catalog\n\
         pub fn show(s: catalog.Sheet) -> void {\n\
         \x20 for i, r in s.rows {\n\
         \x20\x20\x20 io.println(\"row\")\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(root.join("dist").join("main.ts")).unwrap();
    assert!(ts.contains("s.rows.entries()"), "array loop expected: {ts}");
    assert!(!ts.contains("Object.entries"), "record loop emitted: {ts}");
}

#[test]
fn aliased_imported_record_field_loops_numerically() {
    // `import catalog as c` + `c.Sheet`, the third spelling.
    let root = unique_tmp("g75alias");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", SHEET_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         import catalog as c\n\
         pub fn show(s: c.Sheet) -> void {\n\
         \x20 for i, r in s.rows {\n\
         \x20\x20\x20 io.println(\"row\")\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(root.join("dist").join("main.ts")).unwrap();
    assert!(ts.contains("s.rows.entries()"), "array loop expected: {ts}");
    assert!(!ts.contains("Object.entries"), "record loop emitted: {ts}");
}

#[test]
fn unknown_field_on_an_imported_record_is_e0210_naming_the_type() {
    // Field checking used to be entirely off across a module boundary: a typo
    // drew nothing from Glyph. The diagnostic has to name `Sheet`, not
    // `record`: naming the type is why an imported type carries its own
    // identity rather than being flattened to a structural shape.
    let root = unique_tmp("g75field");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", SHEET_MODULE);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         import catalog { Sheet }\n\
         pub fn show(s: Sheet) -> void {\n\
         \x20 io.println(s.rowz)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.contains("E0210"))
        .unwrap_or_else(|| panic!("no E0210; diagnostics were: {:?}", report.diagnostics));
    assert!(diag.contains("Sheet"), "must name the type: {diag}");
    assert!(diag.contains("rowz"), "must name the field: {diag}");
}

#[test]
fn unknown_field_via_inferred_let_on_cross_module_call_is_e0210_naming_the_type() {
    // G133: the checker had no cross-module *function* signature at all,
    // only cross-module type/union resolution. A call into another module
    // (`make()`) typed as `Unknown`, so an inferred `let s = make()` bound
    // `s` at `Unknown` too, and a later field typo on `s` fell straight
    // through Glyph's own field-existence check. The mistake surfaced only
    // once the emitted TS reached `tsc`, as a degraded `[TS2339]` pinned to
    // the whole `return` statement rather than `[E0210]` naming the type
    // and field the way an annotated binding already does (see
    // `unknown_field_on_an_imported_record_is_e0210_naming_the_type` above).
    // A cross-module `fn` now has a lowered signature the same way a
    // cross-module `type` already does, so the inferred `let` sees a
    // decidable `Sheet` and the typo is caught at the Glyph layer.
    let root = unique_tmp("g133field");
    let src = root.join("src");
    write_file(
        &src,
        "a.glyph",
        "module a\npub type Sheet = { rows: number, }\npub fn make() -> Sheet {\n  return { rows: 3, }\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import a { make }\n\
         pub fn go() -> number {\n\
         \x20 let s = make()\n\
         \x20 return s.rowz\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.contains("E0210"))
        .unwrap_or_else(|| panic!("no E0210; diagnostics were: {:?}", report.diagnostics));
    assert!(diag.contains("Sheet"), "must name the type: {diag}");
    assert!(diag.contains("rowz"), "must name the field: {diag}");
}

#[test]
fn nested_sibling_type_resolves_one_level_down() {
    // A sibling type named inside another sibling type is itself an imported
    // type, resolved when a field set is asked for. Nothing is expanded at
    // lowering, so this costs no cycle guard.
    let root = unique_tmp("g75nested");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\n\
         pub type Sheet = { rows: Array<Array<string>> }\n\
         pub type Book = { sheet: Sheet }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         import catalog { Book }\n\
         pub fn show(b: Book) -> void {\n\
         \x20 for i, r in b.sheet.rows {\n\
         \x20\x20\x20 io.println(\"row\")\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(root.join("dist").join("main.ts")).unwrap();
    assert!(
        ts.contains("b.sheet.rows.entries()"),
        "array loop expected through a nested sibling type: {ts}"
    );

    // And the field check reaches the same level down.
    let root = unique_tmp("g75nestedfield");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\n\
         pub type Sheet = { rows: Array<Array<string>> }\n\
         pub type Book = { sheet: Sheet }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         import catalog { Book }\n\
         pub fn show(b: Book) -> void {\n\
         \x20 io.println(b.sheet.rowz)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.contains("E0210"))
        .unwrap_or_else(|| panic!("no E0210; diagnostics were: {:?}", report.diagnostics));
    assert!(diag.contains("Sheet"), "must name the nested type: {diag}");
}

#[test]
fn self_referential_sibling_type_terminates() {
    // `type Node = { next: Option<Node> }` imported and used. Lowering emits an
    // imported type unconditionally and never consults the cross-module query,
    // so nothing expands and this terminates by construction. The test exists
    // to keep it that way: a representation that expanded eagerly would hang
    // here instead of failing.
    let root = unique_tmp("g75cycle");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\npub type Node = { label: string, next: Option<Node> }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Node }\n\
         pub fn label_of(n: Node) -> string {\n\
         \x20 return n.label\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
}

#[test]
fn generic_sibling_record_substitutes_its_type_argument() {
    // `type Box<T> = { value: T }` used as `Box<string>` across the boundary
    // gets the same argument substitution a local generic record gets.
    let root = unique_tmp("g75generic");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", "module catalog\npub type Box<T> = { value: T }\n");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Box }\n\
         pub fn unwrap(b: Box<string>) -> string {\n\
         \x20 return b.value\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    // The field set is real, so a typo on the generic record is caught too.
    let root = unique_tmp("g75genericfield");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", "module catalog\npub type Box<T> = { value: T }\n");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Box }\n\
         pub fn unwrap(b: Box<string>) -> string {\n\
         \x20 return b.valu\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0210") && d.contains("Box")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn match_on_an_imported_records_literal_union_field_is_exhaustive_without_else() {
    // D30 through an imported record *field*. The field's type is lowered on
    // the source side, so it arrives as an imported type and needs the same
    // resolution the direct spelling already got.
    let root = unique_tmp("g75fieldunion");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\n\
         pub type Kind = \"csv\" | \"tsv\"\n\
         pub type Sheet = { kind: Kind }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Sheet }\n\
         pub fn label(s: Sheet) -> string {\n\
         \x20 return match s.kind {\n\
         \x20\x20\x20 \"csv\" => \"comma\",\n\
         \x20\x20\x20 \"tsv\" => \"tab\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    // And a missing literal is E0200, not the E0218 that coaches an `else`.
    let root = unique_tmp("g75fieldunionmissing");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\n\
         pub type Kind = \"csv\" | \"tsv\"\n\
         pub type Sheet = { kind: Kind }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Sheet }\n\
         pub fn label(s: Sheet) -> string {\n\
         \x20 return match s.kind {\n\
         \x20\x20\x20 \"csv\" => \"comma\",\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "diags: {:?}",
        report.diagnostics
    );
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0218")),
        "diags: {:?}",
        report.diagnostics
    );
}

#[test]
fn a_stdlib_type_used_as_an_annotation_draws_no_new_diagnostics() {
    // A name whose module is not a project sibling still has an identity, but
    // nothing can resolve it, so member access falls through to the stdlib
    // tables exactly as before. The negative that pins "no new errors".
    //
    // `http.Response` on purpose, not `fs.FsError`: the latter is in
    // `stdlib_modeled_type`, so `stdlib_path_ty` answers first and the
    // `qualified_imported_ty` fall-through this test exists to pin is never
    // entered.
    let root = unique_tmp("g75stdlib");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/http\n\
         pub fn code(r: http.Response) -> number {\n\
         \x20 return r.status\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
}

#[test]
fn a_question_operator_under_an_imported_result_alias_is_not_flagged() {
    // `pub type Res = Result<string, string>` in a sibling: this module cannot
    // see through the alias, so it cannot judge whether the return type is a
    // `Result`. Giving an imported type an identity must not turn "cannot judge"
    // into "decidably not a Result" — that would reject every `?` in a function
    // declared with that return type.
    let root = unique_tmp("g75resalias");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\npub type Res = Result<string, string>\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/result { Result, Ok, Err }\n\
         import catalog { Res }\n\
         fn g() -> Res { return Ok(\"x\") }\n\
         pub fn f() -> Res {\n\
         \x20 let v = g()?\n\
         \x20 return Ok(v)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
}

#[test]
fn a_reserved_or_shadowing_declaration_is_reported_exactly_once() {
    // One declaration, one diagnostic. `is_reserved_ts_word` is consulted from
    // both the collect pass (top-level names) and the resolve pass (local
    // bindings); the two check disjoint name sets and the collect-failure guard
    // in `build_project_inner` drops resolve's output, so neither code can
    // double-report. Count, not `any`: a regression here shows up as 2.
    let root = unique_tmp("once");
    let src = root.join("src");
    write_file(
        &src,
        "shadow.glyph",
        // `Array` is the one that stays reserved: it is a Glyph prelude type, so a
        // local declaration redefines how the module spells an array.
        "module shadow\npub type Array = { items: number }\n",
    );
    write_file(&src, "reserved.glyph", "module reserved\nfn switch() {}\n");

    let report =
        build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
    let count = |code: &str| {
        report
            .structured
            .iter()
            .filter(|d| d.code == code)
            .count()
    };
    assert_eq!(count("E0110"), 1, "diags: {:?}", report.structured);
    assert_eq!(count("E0109"), 1, "diags: {:?}", report.structured);
}

#[test]
fn a_module_local_issue_type_is_rejected_at_the_declaration() {
    // The emitted descriptor writes `Issue[]` whether or not the author did, so
    // a module-local `type Issue` wins over the prelude one and every
    // descriptor in the module breaks with a `tsc` error about generated code.
    // It is the same failure E0110 already names for JavaScript globals.
    let root = unique_tmp("issueshadow");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\ntype Issue = { path: Array<string>, message: string }\npub type User = { id: string }\n",
    );

    let report =
        build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
    let shadow: Vec<_> = report
        .structured
        .iter()
        .filter(|d| d.code == "E0110")
        .collect();
    assert_eq!(shadow.len(), 1, "diags: {:?}", report.structured);
    assert!(
        shadow[0].message.contains("Issue"),
        "message should name the type: {:?}",
        shadow[0]
    );
}

#[test]
fn an_unresolvable_local_import_names_the_module_and_where_it_lives() {
    // A local import resolves from the build root (D15). Build an enclosing
    // directory and a nested app's `import model` resolves to nothing; before
    // this diagnostic the type degraded and the user got a non-exhaustive-match
    // error on a match that was exhaustive.
    let root = unique_tmp("localimport");
    let src = root.join("src");
    write_file(
        &src,
        "app/main.glyph",
        "module app/main\nimport model { Id }\npub fn use_it(i: Id) -> Id {\n\x20 return i\n}\n",
    );
    write_file(
        &src,
        "app/nested/model.glyph",
        "module app/nested/model\npub type Id = string\n",
    );

    let report =
        build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
    let unresolved: Vec<_> = report
        .structured
        .iter()
        .filter(|d| d.code == "E0104")
        .collect();
    assert_eq!(unresolved.len(), 1, "diags: {:?}", report.structured);
    let msg = &unresolved[0].message;
    assert!(msg.contains("`model`"), "{msg}");
    assert!(msg.contains("app/nested/model.glyph"), "{msg}");
}

#[test]
fn a_declared_npm_module_is_not_reported_when_a_local_file_shares_its_name() {
    // The false positive this check was rebuilt around: `tinylog` is declared in
    // `<root>/.types/`, exactly as E0104's own help text tells the user to
    // declare an npm package, and an unrelated `vendor/tinylog.glyph` happens to
    // share the basename. The import is correct and must not be reported.
    let root = unique_tmp("declaredpkg");
    let src = root.join("src");
    write_file(
        &src,
        ".types/tinylog.d.ts",
        "declare module \"tinylog\" { export function log(msg: string): void; }\n",
    );
    write_file(
        &src,
        "vendor/tinylog.glyph",
        "module vendor/tinylog\npub fn helper() -> number {\n\x20 return 1\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\nimport tinylog { log }\npub fn go() -> void {\n\x20 log(\"hi\")\n}\n",
    );

    let report =
        build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
    let unresolved: Vec<_> = report
        .structured
        .iter()
        .filter(|d| d.code == "E0104")
        .collect();
    assert!(unresolved.is_empty(), "diags: {:?}", report.structured);
}

#[test]
fn an_import_matching_nothing_is_reported_only_when_the_build_can_see_node_modules() {
    // With no `node_modules` in the project there is no way to tell a misspelled
    // local import from a dependency that has not been installed yet, so the
    // check says nothing. Install one and the same import is E0104, with the
    // help line that tells the user to install it or declare it.
    let root = unique_tmp("npmview");
    let src = root.join("src");
    write_file(
        &src,
        "model.glyph",
        "module model\npub type Id = string\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\nimport modle { Id }\npub fn use_it(i: Id) -> Id {\n\x20 return i\n}\n",
    );

    let quiet = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(
        !quiet.structured.iter().any(|d| d.code == "E0104"),
        "diags: {:?}",
        quiet.structured
    );

    // A `node_modules` at the project root gives the build a view of what is
    // installed, which is what makes "nothing answers to this name" provable.
    std::fs::create_dir_all(src.join("node_modules/left-pad")).expect("mkdir node_modules");
    let report = build_project_inner(&src, &root.join("dist2"), false).expect("build ok");
    let unresolved: Vec<_> = report
        .structured
        .iter()
        .filter(|d| d.code == "E0104")
        .collect();
    assert_eq!(unresolved.len(), 1, "diags: {:?}", report.structured);
    let msg = &unresolved[0].message;
    assert!(msg.contains("`modle`"), "{msg}");
    assert!(!msg.contains("There is a"), "{msg}");

    // An installed package with the same shape is not reported.
    std::fs::create_dir_all(src.join("node_modules/tinylog")).expect("mkdir pkg");
    write_file(
        &src,
        "uses_pkg.glyph",
        "module uses_pkg\nimport tinylog { log }\npub fn go() -> void {\n\x20 log(\"hi\")\n}\n",
    );
    let installed = build_project_inner(&src, &root.join("dist3"), false).expect("build ok");
    assert!(
        !installed
            .structured
            .iter()
            .any(|d| d.code == "E0104" && d.message.contains("tinylog")),
        "diags: {:?}",
        installed.structured
    );
}

#[test]
fn run_accepts_a_directory_and_runs_its_main_glyph() {
    // `glyph build <dir>` and `glyph run <dir>` are the two commands a
    // multi-module app needs; they are spelled the same way.
    let root = unique_tmp("rundir");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\npub fn code() -> number {\n\x20 return 7\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\nimport lib { code }\nfn main() -> number {\n\x20 return code()\n}\n",
    );

    match glyph_cli::run::run_file(&src, &[], false, false)
        .expect("run_file accepts a directory")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => assert_eq!(code, 7),
        glyph_cli::run::RunOutcome::TsxNotFound => eprintln!("skipping: `tsx` not found"),
        glyph_cli::run::RunOutcome::TscMissing => eprintln!("skipping: `tsc` not found"),
        other => panic!("expected the directory's main.glyph to run; got {other:?}"),
    }
}

#[test]
fn run_on_a_directory_without_a_main_glyph_says_so() {
    let root = unique_tmp("rundirnomain");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\npub fn code() -> number {\n\x20 return 7\n}\n",
    );

    let err = glyph_cli::run::run_file(&src, &[], false, false)
        .expect_err("a directory with no main.glyph is an error");
    let msg = err.to_string();
    assert!(msg.contains("main.glyph"), "{msg}");
    assert!(msg.contains(&src.display().to_string()), "{msg}");
}

// ---------------------------------------------------------------------------
// D41: a `package.json` with a `"glyph"` key is a module-resolution root
// ---------------------------------------------------------------------------

/// Write a two-app tree under `root/apps/`. Each app has a `lib` module and a
/// `main` that imports it by bare name, which only resolves when the app itself
/// is the resolution root.
fn two_app_tree(root: &Path, with_markers: bool) {
    for app in ["alpha", "beta"] {
        let dir = root.join("apps").join(app);
        std::fs::create_dir_all(&dir).expect("create app dir");
        if with_markers {
            std::fs::write(
                dir.join("package.json"),
                format!("{{ \"name\": \"{app}\", \"private\": true, \"glyph\": {{}} }}\n"),
            )
            .expect("write marker");
        }
        std::fs::write(
            dir.join("lib.glyph"),
            "module lib\n\npub fn greet(name: string) -> string {\n  return name\n}\n",
        )
        .expect("write lib");
        std::fs::write(
            dir.join("main.glyph"),
            "module main\n\nimport lib { greet }\n\npub fn run() -> string {\n  return greet(\"x\")\n}\n",
        )
        .expect("write main");
    }
}

#[test]
fn a_tree_of_marked_projects_builds_in_one_invocation() {
    let root = unique_tmp("tree_markers");
    let out = unique_tmp("tree_markers_out");
    two_app_tree(&root, true);

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(
        !tree.has_errors(),
        "a marked tree should build clean: {:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );
    // Each app's output lands under `<out>/apps/<name>/`.
    for app in ["alpha", "beta"] {
        assert!(
            out.join("apps").join(app).join("main.ts").is_file(),
            "expected <out>/apps/{app}/main.ts"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn the_same_tree_without_markers_still_fails_to_resolve() {
    // The pre-D41 behaviour is preserved exactly when no marker exists: one
    // root, and `import lib` from `apps/alpha/main.glyph` names nothing.
    let root = unique_tmp("tree_nomarkers");
    let out = unique_tmp("tree_nomarkers_out");
    two_app_tree(&root, false);

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(tree.has_errors(), "an unmarked tree must still report E0104");
    assert!(
        tree.diagnostics().any(|d| d.contains("E0104")),
        "{:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );
    assert_eq!(tree.projects.len(), 1, "no marker means exactly one project");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

/// A nested project's `.types/` ambient declarations must survive a tree build.
///
/// `include`'s `**/*.ts` reaches into a nested project's emitted output, but
/// `.types/**/*.d.ts` only ever covered the outer project's own directory, so
/// the outer `tsc` run type-checked the inner project's files without the
/// declarations they depend on. The inner project's own run passed; the tree
/// build failed. The app that found this declares `net` in
/// `examples/apps/chat/.types/`, and every `socket` parameter came back as an
/// implicit `any` when the tree was built as one.
#[test]
fn a_nested_projects_ambient_types_survive_a_tree_build() {
    let root = unique_tmp("nested_types");
    let out = unique_tmp("nested_types_out");
    std::fs::write(
        root.join("outer.glyph"),
        "module outer\n\npub fn v() -> int {\n  return 1\n}\n",
    )
    .expect("write outer");

    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    write_file(
        &inner,
        "package.json",
        "{ \"name\": \"inner\", \"private\": true, \"glyph\": {} }\n",
    );
    // Declared only for the inner project. Nothing outside it can see this.
    write_file(
        &inner,
        ".types/widget.d.ts",
        "declare module \"widget\" {\n  export function spin(turns: number): string;\n}\n",
    );
    write_file(
        &inner,
        "main.glyph",
        "module main\n\nimport widget { spin }\n\npub fn go() -> string {\n  return spin(2)\n}\n",
    );

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(
        !tree.has_errors(),
        "a nested project's own .types/ must be honoured in a tree build: {:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );

    // `build_tree` does not type-check; the whole subject of this fix is what
    // `tsc` sees, so the assertion has to go through the checker that produced
    // the errors in the first place. Asserting on the tsconfig text instead
    // would only pin the current implementation of the fix.
    match glyph_cli::runtime::check_tree_with_tsc(&tree, &out).expect("run tsc") {
        glyph_cli::runtime::TscOutcome::Passed => {}
        glyph_cli::runtime::TscOutcome::NotFound => {
            eprintln!("skipping: `tsc` not found on PATH");
        }
        glyph_cli::runtime::TscOutcome::Failed(msg) => {
            panic!("the nested project's ambient types must reach its own tsc run:\n{msg}");
        }
    }

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

/// The same thing for the layout `glyph init` scaffolds: a project whose
/// sources live in `src/`.
///
/// A project's output directory is derived from its *package* directory, while
/// its sources may sit a level below it. Deriving the exclude list from the
/// source directories instead produced `apps/inner/src`, which matches nothing
/// in the out tree, so the outer run kept swallowing the nested project. The
/// flat `{"glyph": {}}` layout that every app under `examples/` uses hid it,
/// because there the two paths coincide.
#[test]
fn a_nested_project_with_a_src_layout_is_excluded_by_its_output_path() {
    let root = unique_tmp("nested_src_layout");
    let out = unique_tmp("nested_src_layout_out");
    std::fs::write(
        root.join("outer.glyph"),
        "module outer\n\npub fn v() -> int {\n  return 1\n}\n",
    )
    .expect("write outer");

    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    write_file(
        &inner,
        "package.json",
        "{ \"name\": \"inner\", \"private\": true, \"glyph\": { \"src\": \"src\" } }\n",
    );
    write_file(
        &inner,
        "src/.types/widget.d.ts",
        "declare module \"widget\" {\n  export function spin(turns: number): string;\n}\n",
    );
    write_file(
        &inner,
        "src/main.glyph",
        "module main\n\nimport widget { spin }\n\npub fn go() -> string {\n  return spin(2)\n}\n",
    );

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(
        !tree.has_errors(),
        "a src/-layout nested project must build clean: {:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );

    // The nested project's output lands at `<out>/inner`, so that is what the
    // outer config must disclaim — not `<out>/inner/src`, which does not exist.
    let outer_cfg =
        std::fs::read_to_string(out.join("tsconfig.json")).expect("read outer tsconfig");
    assert!(
        outer_cfg.contains("\"inner\""),
        "exclude must name the output path, got: {outer_cfg}"
    );
    assert!(
        !outer_cfg.contains("inner/src"),
        "exclude must not use the source path, got: {outer_cfg}"
    );
    assert!(
        out.join("inner").join("main.ts").is_file(),
        "sanity: nested output lands at <out>/inner/"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

/// Excluding a nested project from the outer `tsc` run must not stop it being
/// checked at all.
///
/// The error here is one **only `tsc`** can catch: a call into an ambient
/// declaration, which Glyph's own checker does not type. A Glyph-level error
/// would prove nothing, since it is reported with or without the exclusion, and
/// with or without `tsc` running at all.
#[test]
fn a_nested_projects_tsc_only_error_still_fails_the_tree_build() {
    let root = unique_tmp("nested_types_bad");
    let out = unique_tmp("nested_types_bad_out");
    std::fs::write(
        root.join("outer.glyph"),
        "module outer\n\npub fn v() -> int {\n  return 1\n}\n",
    )
    .expect("write outer");

    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    write_file(
        &inner,
        "package.json",
        "{ \"name\": \"inner\", \"private\": true, \"glyph\": {} }\n",
    );
    write_file(
        &inner,
        ".types/widget.d.ts",
        "declare module \"widget\" {\n  export function spin(turns: number): string;\n}\n",
    );
    // `spin` wants a number. Glyph does not type external modules, so nothing
    // short of the nested project's own `tsc` run rejects this.
    write_file(
        &inner,
        "main.glyph",
        "module main\n\nimport widget { spin }\n\npub fn go() -> string {\n  return spin(\"two\")\n}\n",
    );

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(
        !tree.has_errors(),
        "sanity: Glyph's own checker does not catch this, tsc must: {:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );

    match glyph_cli::runtime::check_tree_with_tsc(&tree, &out).expect("run tsc") {
        glyph_cli::runtime::TscOutcome::Failed(msg) => {
            assert!(
                msg.contains("string") || msg.contains("number"),
                "expected an argument-type error, got:\n{msg}"
            );
        }
        glyph_cli::runtime::TscOutcome::NotFound => {
            eprintln!("skipping: `tsc` not found on PATH");
        }
        glyph_cli::runtime::TscOutcome::Passed => {
            panic!(
                "excluding a nested project from the outer run must not stop it \
                 being checked by its own run"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn a_nested_project_cannot_import_from_the_enclosing_one() {
    let root = unique_tmp("nested_imports_up");
    let out = unique_tmp("nested_imports_up_out");
    std::fs::write(
        root.join("shared.glyph"),
        "module shared\n\npub fn v() -> int {\n  return 1\n}\n",
    )
    .expect("write shared");
    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    std::fs::write(
        inner.join("package.json"),
        "{ \"name\": \"inner\", \"private\": true, \"glyph\": {} }\n",
    )
    .expect("write marker");
    std::fs::write(
        inner.join("main.glyph"),
        "module main\n\nimport shared { v }\n\npub fn run() -> int {\n  return v()\n}\n",
    )
    .expect("write inner main");

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    let diags: Vec<&str> = tree.diagnostics().collect();
    assert!(
        diags.iter().any(|d| d.contains("E0104")),
        "a nested project importing an enclosing module must be E0104: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("a project's imports resolve within its own root only")),
        "the message must name the D41 rule: {diags:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn an_enclosing_project_cannot_import_from_a_nested_one() {
    let root = unique_tmp("nested_imports_down");
    let out = unique_tmp("nested_imports_down_out");
    std::fs::write(
        root.join("main.glyph"),
        "module main\n\nimport inner/helper { v }\n\npub fn run() -> int {\n  return v()\n}\n",
    )
    .expect("write outer main");
    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    std::fs::write(
        inner.join("package.json"),
        "{ \"name\": \"inner\", \"private\": true, \"glyph\": {} }\n",
    )
    .expect("write marker");
    std::fs::write(
        inner.join("helper.glyph"),
        "module helper\n\npub fn v() -> int {\n  return 1\n}\n",
    )
    .expect("write helper");

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    let diags: Vec<&str> = tree.diagnostics().collect();
    assert!(
        diags.iter().any(|d| d.contains("E0104")),
        "a nested project's module is not part of the enclosing compilation: {diags:?}"
    );
    // The other project is named relative to the build target, not as an
    // absolute machine path, and the message says where the file actually is.
    assert!(
        diags
            .iter()
            .any(|d| d.contains("in the project rooted at `inner`")),
        "the message names the other project relative to the target: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.contains(&format!(
            "rooted at `{}",
            root.display()
        ))),
        "the project is not named by an absolute path: {diags:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn a_marker_at_the_build_target_roots_at_its_src() {
    let root = unique_tmp("marker_at_target");
    let out = unique_tmp("marker_at_target_out");
    std::fs::write(
        root.join("package.json"),
        "{ \"name\": \"app\", \"private\": true, \"glyph\": { \"src\": \"src\" } }\n",
    )
    .expect("write marker");
    let src = root.join("src");
    std::fs::create_dir_all(src.join("deep")).expect("create src");
    std::fs::write(
        src.join("deep").join("lib.glyph"),
        "module lib\n\npub fn v() -> int {\n  return 1\n}\n",
    )
    .expect("write lib");
    std::fs::write(
        src.join("main.glyph"),
        "module main\n\nimport deep/lib { v }\n\npub fn run() -> int {\n  return v()\n}\n",
    )
    .expect("write main");
    // A stray `.glyph` outside `src/` is not a module of this project.
    std::fs::create_dir_all(root.join("scripts")).expect("create scripts");
    std::fs::write(root.join("scripts").join("tool.glyph"), "module tool\n")
        .expect("write tool");

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(
        !tree.has_errors(),
        "{:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );
    let modules: Vec<String> = tree
        .projects
        .iter()
        .flat_map(|p| p.report.modules.iter().cloned())
        .collect();
    assert_eq!(modules, vec!["deep/lib".to_string(), "main".to_string()]);
    // Output is flat under `--out` for a single project, exactly as before D41.
    assert!(out.join("main.ts").is_file());
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn discovery_never_looks_above_the_build_target() {
    // `glyph build app/src` means `app/src`, whether or not `app` is marked.
    let root = unique_tmp("no_climb");
    std::fs::write(
        root.join("package.json"),
        "{ \"name\": \"app\", \"private\": true, \"glyph\": { \"src\": \"src\" } }\n",
    )
    .expect("write marker");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("main.glyph"), "module main\n").expect("write main");

    let found = glyph_cli::build::discover_projects(&src).expect("discover");
    assert_eq!(found.projects.len(), 1);
    assert_eq!(found.projects[0].src, src);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_package_json_without_the_glyph_key_is_not_a_root() {
    let root = unique_tmp("plain_npm_pkg");
    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    std::fs::write(inner.join("package.json"), "{ \"name\": \"plain\" }\n")
        .expect("write plain manifest");
    std::fs::write(inner.join("lib.glyph"), "module lib\n").expect("write lib");

    let found = glyph_cli::build::discover_projects(&root).expect("discover");
    assert_eq!(
        found.projects.len(),
        1,
        "a plain npm package is not a project root"
    );
    assert!(found.notices.is_empty(), "{:?}", found.notices);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unparseable_package_json_is_not_a_root() {
    let root = unique_tmp("bad_manifest");
    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).expect("create inner");
    std::fs::write(inner.join("package.json"), "{ not json").expect("write bad manifest");
    std::fs::write(inner.join("lib.glyph"), "module lib\n").expect("write lib");

    let found = glyph_cli::build::discover_projects(&root).expect("discover");
    assert_eq!(
        found.projects.len(),
        1,
        "a malformed manifest must not fail the tree"
    );
    // P8: it must not fail the build, and it must not be silent either.
    assert_eq!(found.notices.len(), 1, "{:?}", found.notices);
    assert!(
        found.notices[0].contains("package.json") && found.notices[0].contains("inner"),
        "the notice names the manifest and its directory: {:?}",
        found.notices
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A one-app tree whose entry module lives a directory below the resolution
/// root and imports a sibling by bare name: `app/src/queries/report.glyph`
/// importing `catalog`. That import resolves only when `app` is found as the
/// project root, which is what makes it the fixture for path spelling.
fn nested_entry_app(root: &Path) {
    write_file(
        root,
        "app/package.json",
        "{ \"name\": \"app\", \"private\": true, \"glyph\": { \"src\": \"src\" } }\n",
    );
    write_file(
        root,
        "app/src/catalog.glyph",
        "module catalog\n\npub fn all() -> number {\n  return 1\n}\n",
    );
    write_file(
        root,
        "app/src/queries/report.glyph",
        "module report\n\nimport catalog { all }\n\npub fn run() -> number {\n  return all()\n}\n",
    );
}

#[test]
fn a_relative_path_finds_the_same_project_as_an_absolute_one() {
    // A file has one meaning regardless of how its path was spelled (D41).
    // Comparing a user-typed relative path against a canonicalized project root
    // used to fail every relative invocation, and fail it silently: the import
    // resolved to nothing, and an import that names nothing outside a known
    // project draws no diagnostic at all.
    let root = unique_tmp("relative_path_project");
    nested_entry_app(&root);

    let check = |arg: &Path| {
        std::process::Command::new(env!("CARGO_BIN_EXE_glyph"))
            .arg("check")
            .arg(arg)
            .arg("--no-tsc")
            .current_dir(&root)
            .output()
            .expect("spawn glyph check")
    };

    let relative = check(Path::new("app/src/queries/report.glyph"));
    let rel_err = String::from_utf8_lossy(&relative.stderr).to_string();
    let absolute = check(&root.join("app/src/queries/report.glyph"));
    let abs_err = String::from_utf8_lossy(&absolute.stderr).to_string();

    assert!(
        !rel_err.contains("E0104"),
        "a relative path must resolve `catalog` too: {rel_err}"
    );
    assert!(
        rel_err.contains("2 module(s) checked"),
        "the relative invocation must compile the whole project: {rel_err}"
    );
    assert!(
        abs_err.contains("2 module(s) checked"),
        "the absolute invocation compiles the whole project: {abs_err}"
    );
    assert_eq!(relative.status.code(), absolute.status.code());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_on_an_unmarked_directory_still_runs_its_own_main() {
    // No marker means the directory itself is the root, exactly as before D41.
    // Applying the `src/` convention to an unmarked directory would make
    // `glyph run <dir>` and `glyph build <dir>` disagree about the root.
    let root = unique_tmp("run_unmarked_dir");
    write_file(
        &root,
        "main.glyph",
        "module main\n\npub fn helper() -> number {\n  return 1\n}\n",
    );
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\nfn main(argv: Array<string>) -> number {\n  return 0\n}\n",
    );

    let result = glyph_cli::run_file(&root, &[], false, false).expect("run");
    match result.outcome {
        glyph_cli::RunOutcome::NoMain { module, .. } => {
            assert_eq!(module, root.join("main.glyph"));
        }
        other => panic!("expected the directory's own main.glyph, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_on_a_marked_directory_runs_the_main_at_its_resolution_root() {
    // With a marker, `glyph run <dir>` runs the `main.glyph` at the project's
    // resolution root, which is what `glyph build <dir>` compiles.
    let root = unique_tmp("run_marked_dir");
    write_file(
        &root,
        "package.json",
        "{ \"name\": \"app\", \"private\": true, \"glyph\": {} }\n",
    );
    write_file(
        &root,
        "main.glyph",
        "module main\n\nfn main(argv: Array<string>) -> number {\n  return 0\n}\n",
    );
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\npub fn helper() -> number {\n  return 1\n}\n",
    );

    let result = glyph_cli::run_file(&root, &[], false, false).expect("run");
    match result.outcome {
        glyph_cli::RunOutcome::NoMain { module, .. } => {
            // The project root is canonicalized, so the module is named by its
            // canonical path (on macOS, `/private/var/...` for a temp dir).
            assert_eq!(
                module,
                root.join("src")
                    .join("main.glyph")
                    .canonicalize()
                    .expect("canonicalize")
            );
        }
        other => panic!("expected the project's src/main.glyph, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_on_a_nested_file_resolves_its_project_siblings() {
    // `glyph run app/src/queries/report.glyph` climbs to the marker at `app`, so
    // `import catalog` resolves the same way `glyph build app` resolves it.
    let root = unique_tmp("run_nested_file");
    nested_entry_app(&root);
    let entry = root.join("app/src/queries/report.glyph");

    let result = glyph_cli::run_file(&entry, &[], false, false).expect("run");
    assert!(
        !result.diagnostics.iter().any(|d| d.contains("E0104")),
        "the sibling import must resolve: {:?}",
        result.diagnostics
    );
    // The entry is a library module, so nothing runs; that is reported instead
    // of the import being quietly lost.
    assert!(
        matches!(result.outcome, glyph_cli::RunOutcome::NoMain { .. }),
        "expected NoMain, got {:?}",
        result.outcome
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn read_line_returns_a_line_before_stdin_closes() {
    // The bug this pins: `read_line` used to slurp fd 0 to EOF, so an echo loop
    // printed nothing until the writer went away and no interactive program was
    // writable. Piping a complete file does not catch that (the whole 0.1.x line
    // shipped with the defect because the harness only ever did that), so this is
    // a timing test: write ONE line, leave stdin OPEN, and require the echo back.
    if !js_toolchain_available() {
        eprintln!("skipping interactive read_line run: node/tsx not available");
        return;
    }
    let root = unique_tmp("readline_interactive");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io\n\
         fn main() -> number {\n\
         \x20 loop {\n\
         \x20   match io.read_line() {\n\
         \x20     None => { break },\n\
         \x20     Some(line) => { io.println(\"echo:${line}\") },\n\
         \x20   }\n\
         \x20 }\n\
         \x20 return 0\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "echo program should build: {:?}",
        report.diagnostics
    );
    let entry = out.join("__glyph_run.ts");
    std::fs::write(
        &entry,
        "import \"./.glyph-runtime/glyph-bootstrap.ts\";\n\
         import { main } from \"./main.ts\";\n\
         (async () => { process.exit(await main()); })();\n",
    )
    .expect("write entry");

    let mut child = std::process::Command::new("tsx")
        .arg("--tsconfig")
        .arg(out.join("tsconfig.json"))
        .arg(&entry)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn tsx");

    // Stream stdout back over a channel so the assertion can time out instead of
    // blocking forever when the fix regresses.
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("piped stdin");
        stdin.write_all(b"hello\n").expect("write a line");
        stdin.flush().expect("flush");
    }
    // stdin is deliberately still open here. Node/tsx startup dominates, so the
    // window is generous; what it must not require is EOF.
    let first = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("read_line must return a line while stdin is still open");
    assert_eq!(first, "echo:hello", "the echoed line");

    // A second line on the same open stream, proving the buffer keeps working
    // rather than having simply handed back one slurped chunk.
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("piped stdin");
        stdin.write_all(b"world\r\n").expect("write a CRLF line");
        stdin.flush().expect("flush");
    }
    let second = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the second line must come back too");
    assert_eq!(
        second, "echo:world",
        "CRLF input yields the same line as LF (the \\r is stripped)"
    );

    // Now close stdin: the loop sees `None` and the program exits 0.
    drop(child.stdin.take());
    let status = child.wait().expect("wait for the echo program");
    assert_eq!(status.code(), Some(0), "program exits cleanly at EOF");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn imported_literal_union_field_keeps_its_membership_check() {
    // G88 cause 2. A field typed by a local `"text" | "int"` union gets a
    // membership check; the same union imported from a sibling module resolved
    // to no descriptor and fell to `!== undefined`, which accepts any string.
    // That is D30's guarantee evaporating at a module boundary, the same hole
    // G76 closed for `match` exhaustiveness.
    //
    // This goes through the whole project build rather than the emitter alone,
    // because the fix has two halves: the cross-module alias map is collected
    // here and consumed there.
    let root = unique_tmp("impunion");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(&src, "cat.glyph", "module cat\npub type ColType = \"text\" | \"int\"\n");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import cat { ColType }\n\
         pub type Local = \"a\" | \"b\"\n\
         pub type Row = { local_kind: Local, imported_kind: ColType }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "clean: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();

    assert!(
        ts.contains(r#".imported_kind === "text""#) && ts.contains(r#".imported_kind === "int""#),
        "imported literal union checks membership: {ts}"
    );
    assert!(
        !ts.contains("!((value as Record<string, unknown>).imported_kind !== undefined)"),
        "and emits no branch that can never fire: {ts}"
    );
    // The local one was always right; this guards against fixing one by
    // breaking the other.
    assert!(
        ts.contains(r#".local_kind === "a""#),
        "local literal union still checks membership: {ts}"
    );
}

#[test]
fn json_parse_of_a_type_reports_the_same_field_paths_as_its_parse() {
    // G68. `json.parse<T>(text)` is rewritten to `json.parse_with(text,
    // T.schema)`, and the schema was built from the descriptor's boolean guard
    // alone, so every field-level failure collapsed to one issue reading
    // `expected T`. The two-step form (`json.parse<unknown>` then `T.parse`)
    // named the field and its path for the same fixture, and the one-step form
    // is the one the guide teaches.
    let root = unique_tmp("jsonpaths");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/json\n\
         pub type Cfg = { host: string, port: number }\n\
         pub fn load(text: string) -> Result<Cfg, Array<Issue>> {\n\
        \x20 return json.parse<Cfg>(text)\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "clean: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();

    // The schema carries the descriptor's own parse, not just its guard.
    assert!(
        ts.contains("(v: unknown): __GlyphResult<Cfg, Issue[]> => Cfg.parse(v)"),
        "schema threads the deep parser: {ts}"
    );
    // And the rewrite still routes through the schema (G3's validation).
    assert!(
        ts.contains("json.parse_with(text, Cfg.schema)"),
        "still the validating form: {ts}"
    );
}

#[test]
fn never_is_spellable_and_behaves_as_a_bottom_type() {
    // G89/D43. `std/process.exit` was typed `-> never`, so the concept existed
    // and only user code could not name it. A `serve` that is driven by socket
    // events from then on had to say so in a doc comment, keep a `return` that
    // is never reached, and carry a dead `match` arm to stay exhaustive.
    let root = unique_tmp("never");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/array\n\
         import std/process\n\
         pub fn serve(port: int) -> never {\n\
        \x20 loop {\n\
        \x20 }\n\
         }\n\
         pub fn main(argv: Array<string>) -> number {\n\
        \x20 return match array.len(argv) {\n\
        \x20   0 => serve(8080),\n\
        \x20   else => process.exit(2),\n\
        \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "clean: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();

    // It emits verbatim: `never` means the same thing on both sides.
    assert!(
        ts.contains("export function serve(port: number): never {"),
        "never emits verbatim: {ts}"
    );
    // `main` returns `number` while both arms are `never`, so the arm join
    // takes the other side and no unreachable `return 0` is owed.
    assert!(
        ts.contains("export function main(argv: Array<string>): number {"),
        "a never-typed arm joins with the declared return: {ts}"
    );
}

#[test]
fn a_never_function_may_not_return_a_value() {
    // The other half of a bottom type: nothing is assignable to it.
    let root = unique_tmp("neverbad");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\npub fn bad() -> never {\n  return 1\n}\n",
    );
    let report =
        build_project_inner(&src, &root.join("out"), false).expect("build runs");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0204")),
        "returning a value where `never` is declared is a type error: {:?}",
        report.diagnostics
    );
}

#[test]
fn parse_is_refused_on_a_record_with_an_unverifiable_field() {
    // G88 cause 3 / E0304. Descriptors are emitted for every record. For a
    // field whose type the emitter cannot see into, the generated check was
    // `field !== undefined` under the message ``field `sock` must be Socket``,
    // so `parse` returned `Ok` for a value it had not validated. A boolean that
    // is always true is useless; one that is always true under a message naming
    // a type it never checked is worse, because `parse` is what a boundary is
    // told to trust.
    let root = unique_tmp("unverifiable");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type Opaque = extern_ts(\"{ handle: number }\")\n\
         pub type Conn = { id: number, sock: Opaque }\n\
         pub fn make(n: number, s: Opaque) -> Conn {\n\
        \x20 return { id: n, sock: s }\n\
         }\n\
         pub fn take(v: unknown) -> Result<Conn, Array<Issue>> {\n\
        \x20 return Conn.parse(v)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
    let all = report.diagnostics.join("\n");
    assert!(all.contains("E0304"), "refuses the parse: {all}");
    assert!(
        all.contains("sock") && all.contains("Opaque"),
        "names the field and its type: {all}"
    );
}

#[test]
fn an_unverifiable_field_is_only_refused_where_it_is_trusted() {
    // Holding a socket in a record is ordinary and stays legal; the error is at
    // the boundary, not the declaration. Without this the change would ban a
    // shape every event-driven program uses.
    let root = unique_tmp("holdok");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type Opaque = extern_ts(\"{ handle: number }\")\n\
         pub type Conn = { id: number, sock: Opaque }\n\
         pub fn make(n: number, s: Opaque) -> Conn {\n\
        \x20 return { id: n, sock: s }\n\
         }\n\
         pub fn id_of(c: Conn) -> number {\n\
        \x20 return c.id\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build ok");
    assert!(!report.has_errors(), "declaring one is fine: {:?}", report.diagnostics);
}

#[test]
fn an_unknown_field_is_not_an_unverifiable_one() {
    // `unknown` claims nothing, so every value satisfies it and presence is the
    // whole check. Treating it as unverifiable would refuse the ordinary
    // "give me this key, whatever it holds" boundary read, and the descriptor
    // no longer emits a type branch that could never fire.
    let root = unique_tmp("unknownok");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         @open\n\
         pub type WithPayload = { payload: unknown }\n\
         pub fn take(v: unknown) -> Result<WithPayload, Array<Issue>> {\n\
        \x20 return WithPayload.parse(v)\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "unknown parses: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();
    assert!(
        !ts.contains("!((value as Record<string, unknown>).payload !== undefined)"),
        "no branch that can never fire: {ts}"
    );
}

#[test]
fn a_module_that_shadows_a_global_still_gets_working_descriptors() {
    // G63, the harder half. `Error` alone only proves the `?`/match throw path.
    // A record descriptor reaches for `Object.keys` and `Array.isArray`, so a
    // module that shadows those has to keep working too.
    let root = unique_tmp("shadowdesc");
    let out = root.join("dist");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         pub type Object = { id: number, tags: Array<string> }\n\
         pub type Number = { n: number }\n\
         pub fn take(v: unknown) -> Result<Object, Array<Issue>> {\n\
        \x20 return Object.parse(v)\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "clean: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).unwrap();

    // The author's names survive.
    assert!(ts.contains("export type Object = "), "kept `Object`: {ts}");
    assert!(ts.contains("export type Number = "), "kept `Number`: {ts}");
    // The descriptor's own machinery goes through the captures.
    assert!(
        ts.contains("const __glyph_Object = globalThis.Object;"),
        "captured Object: {ts}"
    );
    assert!(
        ts.contains("__glyph_Object.keys("),
        "the descriptor's key check uses the capture: {ts}"
    );
    assert!(
        !ts.contains(" Object.keys("),
        "and never the shadowed name: {ts}"
    );
    // Only what is actually shadowed is captured. This module cannot declare
    // `Array` (it stays reserved as a Glyph type name), so `Array.isArray`
    // emits plain, and a module that shadows nothing emits exactly what it
    // always did.
    assert!(
        ts.contains("Array.isArray(") && !ts.contains("__glyph_Array"),
        "an unshadowed global is left alone: {ts}"
    );
}

#[test]
fn a_match_on_a_trailing_optional_stdlib_call_is_checked() {
    // G39 phase 2, and the case the entry was really about. `string.index_of`
    // returns `Option<number>`, but it takes a trailing optional argument and
    // the arity check compared one number against one number, so modeling it
    // would have reported a false error on every two-argument call. Unmodeled,
    // its result was `Unknown`, D9 exhaustiveness never ran, and a `match` with
    // no `None` arm built clean and threw `non-exhaustive match` at run time.
    let root = unique_tmp("optarity");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/string\n\
         pub fn at(s: string) -> number {\n\
        \x20 return match string.index_of(s, \"x\") {\n\
        \x20   Some(i) => i,\n\
        \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "the missing `None` arm is a compile error now: {:?}",
        report.diagnostics
    );
}

#[test]
fn omitting_a_trailing_optional_argument_is_still_legal() {
    // The other half. Modeling these was blocked because an exact arity check
    // rejects the two-argument form, so the check now reads a minimum and a
    // maximum: both spellings compile, and a third argument too many does not.
    let root = unique_tmp("optok");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/array\n\
         import std/string\n\
         pub fn a(s: string) -> string {\n\
        \x20 return string.slice(s, 1)\n\
         }\n\
         pub fn b(s: string) -> string {\n\
        \x20 return string.slice(s, 1, 3)\n\
         }\n\
         pub fn c(xs: Array<number>) -> Array<number> {\n\
        \x20 return array.slice(xs, 1)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build ok");
    assert!(!report.has_errors(), "both spellings compile: {:?}", report.diagnostics);
}

#[test]
fn a_misspelled_stdlib_member_is_a_glyph_error() {
    // G27. `import std/string { repeeat }` was already E0105, because named
    // imports are checked against the resolver seed. `string.repeeat(...)` was
    // a TS2339 from the back end, so one typo had two experiences depending on
    // which spelling of the import you had used. A member read out of a
    // namespace import is now recorded during resolution and held to the same
    // export list, and the resolution is what keeps a local binding that shares
    // a namespace's name out of it.
    let root = unique_tmp("nsmember");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/string\n\
         pub fn go(s: string) -> string {\n\
        \x20 return string.repeeat(s, 2)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
    let all = report.diagnostics.join("\n");
    assert!(all.contains("E0105"), "a Glyph code, not a TS one: {all}");
    assert!(
        all.contains("repeeat") && all.contains("std/string"),
        "names the member and the module: {all}"
    );
    assert!(!all.contains("TS2339"), "and never reaches tsc: {all}");
}

#[test]
fn a_real_stdlib_member_through_a_namespace_still_compiles() {
    // The guard on the guard: every `std/*` namespace call in the examples tree
    // goes through this path, so a seed list that is missing a real export
    // would turn a working program into an error. It was missing one when this
    // landed, in a test fixture that called `fs.write` where the function is
    // `fs.write_text`, which nothing had caught because `--no-tsc` was set.
    let root = unique_tmp("nsmemberok");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/array\n\
         import std/string\n\
         pub fn go(s: string) -> number {\n\
        \x20 return array.len(string.split(s, \",\"))\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build ok");
    assert!(!report.has_errors(), "real members compile: {:?}", report.diagnostics);
}

// --- std/bytes (G102) -------------------------------------------------------
//
// Two apps in one dogfood round stopped on the same sentence: Glyph had no
// bytes. A PNG reader could not read a file whose first byte is 0x89, and an
// RFC 6238 authenticator could not form either argument to an HMAC. These pin
// both programs as writable, and pin the codecs against their published
// vectors rather than against themselves.

/// The RFC 4648 §10 vectors, and the malformed inputs node's `Buffer` accepts
/// silently. `Buffer.from("zz", "hex")` returns an empty buffer and reports
/// success, which is the failure mode this module exists to not have.
#[test]
fn bytes_codecs_match_the_published_vectors_and_refuse_malformed_input() {
    if !js_toolchain_available() {
        eprintln!("skipping bytes codec vectors: node/tsx not available");
        return;
    }
    let root = unique_tmp("bytesvec");
    let src = root.join("src");
    write_file(
        &src,
        "codec.glyph",
        r#"module codec

import std/bytes
import std/result { Result, Ok, Err }

// RFC 4648 section 10.
@example b64("") == ""
@example b64("f") == "Zg=="
@example b64("fo") == "Zm8="
@example b64("foo") == "Zm9v"
@example b64("foob") == "Zm9vYg=="
@example b64("fooba") == "Zm9vYmE="
@example b64("foobar") == "Zm9vYmFy"
pub fn b64(text: string) -> string {
  return bytes.to_base64(bytes.from_text(text))
}

@example hex("foobar") == "666f6f626172"
pub fn hex(text: string) -> string {
  return bytes.to_hex(bytes.from_text(text))
}

// RFC 4648 section 10, base32.
@example b32("") == ""
@example b32("f") == "MY======"
@example b32("fo") == "MZXQ===="
@example b32("foo") == "MZXW6==="
@example b32("foob") == "MZXW6YQ="
@example b32("fooba") == "MZXW6YTB"
@example b32("foobar") == "MZXW6YTBOI======"
pub fn b32(text: string) -> string {
  return bytes.to_base32(bytes.from_text(text))
}

// Case carries no information in base32, and a real `otpauth://` secret is
// usually written with no padding at all.
@example decode_b32("MZXW6YTBOI======") == Ok("foobar")
@example decode_b32("mzxw6ytboi") == Ok("foobar")
@example decode_b32("MZXW6YTB1") == Err(8)
@example decode_b32("MZXW6YTBOIB") == Err(10)
pub fn decode_b32(encoded: string) -> Result<string, number> {
  return match bytes.from_base32(encoded) {
    Ok(b) => match bytes.to_text(b) {
      Ok(t) => Ok(t),
      Err(e) => Err(e.index),
    },
    Err(e) => Err(e.index),
  }
}

// Every decode round-trips, and every malformed input is an `Err` naming where.
@example decode_hex("666f6f") == Ok("foo")
@example decode_hex("zz") == Err(0)
@example decode_hex("abc") == Err(3)
pub fn decode_hex(encoded: string) -> Result<string, number> {
  return match bytes.from_hex(encoded) {
    Ok(b) => match bytes.to_text(b) {
      Ok(t) => Ok(t),
      Err(e) => Err(e.index),
    },
    Err(e) => Err(e.index),
  }
}

// A base64url string under the standard alphabet, and a final character with
// bits set past the end of the data. Both decode to quietly wrong bytes through
// `Buffer`.
@example decode_b64("Zm9v") == Ok("foo")
@example decode_b64("a-b_") == Err(1)
@example decode_b64("Zh==") == Err(1)
@example decode_b64("Z") == Err(0)
pub fn decode_b64(encoded: string) -> Result<string, number> {
  return match bytes.from_base64(encoded) {
    Ok(b) => match bytes.to_text(b) {
      Ok(t) => Ok(t),
      Err(e) => Err(e.index),
    },
    Err(e) => Err(e.index),
  }
}

// 256 is not a byte. A silent `& 0xff` would make it 0.
@example rejects_at([1, 300, 3,]) == 1
@example rejects_at([1, 2, 3,]) == 0 - 1
pub fn rejects_at(xs: Array<int>) -> number {
  return match bytes.from_array(xs) {
    Ok(_) => 0 - 1,
    Err(e) => e.index,
  }
}
"#,
    );
    let report = glyph_cli::examples::run_examples(&src).expect("run_examples ok");
    assert!(report.ran, "examples should have run");
    assert!(
        report.build_failed.is_none(),
        "the codec module should compile: {:?}",
        report.build_failed
    );
    assert!(
        report.failures.is_empty(),
        "every published vector should hold: {:?}",
        report.failures
    );
    assert_eq!(report.total, 28, "every @example above ran");
}

/// The two programs the round could not write. The TOTP is checked against the
/// RFC 6238 test vector (the ASCII secret `12345678901234567890` at T=59 is
/// `94287082`), which is the whole point: an HMAC over bytes routed through a
/// string computes a different answer, and only a published vector catches it.
#[test]
fn bytes_carry_a_binary_file_and_an_rfc_6238_hmac() {
    if !js_toolchain_available() {
        eprintln!("skipping bytes end-to-end: node/tsx not available");
        return;
    }
    let root = unique_tmp("bytesapp");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/array
import std/bytes
import std/crypto
import std/fs
import std/io
import std/math
import std/option { Some, None }
import std/result { Result, Ok, Err }

fn at(b: bytes.Bytes, i: int) -> int {
  return match bytes.get(b, i) {
    Some(v) => v,
    None => 0 - 1,
  }
}

// The 8-byte big-endian counter. Shifts stop working past 32 bits, so divide.
fn counter_bytes(counter: number) -> Array<int> {
  return array.map(array.range(8), fn(i: int) -> int {
    return math.floor(counter / math.pow(256, 7 - i)) % 256
  })
}

fn totp(key: bytes.Bytes, counter: number) -> Result<string, bytes.BytesError> {
  let msg = bytes.from_array(counter_bytes(counter))?
  let digest = crypto.hmac_sha1_bytes(key, msg)
  let offset = at(digest, bytes.len(digest) - 1) % 16
  let code = (((at(digest, offset) % 128) * 16777216)
    + (at(digest, offset + 1) * 65536)
    + (at(digest, offset + 2) * 256)
    + at(digest, offset + 3)) % 100000000
  return Ok(number.to_string(code))
}

fn run(path: string) -> Result<void, string> {
  let key = bytes.from_text("12345678901234567890")
  let code = totp(key, 1).map_err(fn(e: bytes.BytesError) -> string { e.message })?
  io.println("totp=${code}")

  // The PNG signature: the first byte is 0x89, which is not valid UTF-8 alone.
  let sig = bytes.from_array([137, 80, 78, 71, 13, 10, 26, 10,])
    .map_err(fn(e: bytes.BytesError) -> string { e.message })?
  let file = bytes.join([sig, bytes.from_text("IHDR"),])
  fs.write_bytes(path, file).map_err(fn(e: fs.FsError) -> string { e.message })?
  let back = fs.read_bytes(path).map_err(fn(e: fs.FsError) -> string { e.message })?
  io.println("same=${bytes.equals(back, file)}")
  io.println("magic=${bytes.to_hex(bytes.slice(back, 0, 4))}")
  io.println("signed=${bytes.starts_with(back, sig)}")
  let as_text = match bytes.to_text(back) {
    Ok(_) => "decoded",
    Err(e) => "refused at ${number.to_string(e.index)}",
  }
  io.println("text=${as_text}")

  // A key that is not valid UTF-8, which is what a real one looks like. The
  // RFC 6238 vector cannot catch a key routed through a string, because its
  // secret is ASCII and survives the trip; this one does not.
  let raw = bytes.from_array([255, 0, 195, 40, 128, 65,])
    .map_err(fn(e: bytes.BytesError) -> string { e.message })?
  let msg = bytes.from_array([0, 0, 0, 0, 0, 0, 0, 1,])
    .map_err(fn(e: bytes.BytesError) -> string { e.message })?
  io.println("mac=${bytes.to_hex(crypto.hmac_sha1_bytes(raw, msg))}")
  io.println("dig=${bytes.to_hex(crypto.sha1_bytes(raw))}")
  io.println("safe=${crypto.timing_safe_equal(raw, bytes.slice(raw, 0, 6))}")
  io.println("nolen=${crypto.timing_safe_equal(raw, msg)}")
  return Ok(void)
}

pub fn main(argv: Array<string>) -> number {
  // The path comes in on argv so the test's temp root owns it; a relative name
  // would drop the file wherever `glyph run` happened to be invoked from.
  let path = match array.get(argv, 0) {
    Some(p) => p,
    None => "probe.png",
  }
  return match run(path) {
    Ok(_) => 0,
    Err(e) => {
      io.eprintln(e)
      1
    },
  }
}
"#,
    );
    let entry = src.join("main.glyph");
    let png = root.join("g102-probe.png");
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("run"),
        entry.as_os_str(),
        png.as_os_str(),
    ]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    // The RFC 6238 vector. A key or a message routed through a string would
    // give some other eight digits, and nothing else here would notice.
    assert!(stdout.contains("totp=94287082"), "RFC 6238 T=59: {stdout}");
    assert!(stdout.contains("same=true"), "a binary file round-trips: {stdout}");
    assert!(stdout.contains("magic=89504e47"), "the PNG magic survives: {stdout}");
    assert!(stdout.contains("signed=true"), "starts_with finds it: {stdout}");
    assert!(
        stdout.contains("text=refused at 0"),
        "0x89 is not text, and to_text says where: {stdout}"
    );
    // Computed by node's own `crypto` over the same octets. Routing the key
    // through a string gives 4ab779f0..., so this is the assertion that pins
    // "the bytes reached the primitive as bytes".
    assert!(
        stdout.contains("mac=c543ef42e8b49063b658bdb6f93799e0e42b92b4"),
        "an HMAC over a key that is not valid UTF-8: {stdout}"
    );
    assert!(
        stdout.contains("dig=9452f2de485ecf156ae9b4da5072478302060aa4"),
        "a digest over the same octets: {stdout}"
    );
    assert!(stdout.contains("safe=true"), "equal secrets compare equal: {stdout}");
    assert!(
        stdout.contains("nolen=false"),
        "different lengths answer false instead of throwing: {stdout}"
    );
}

/// `std/net` over a real loopback socket: a server, a client, a bind failure
/// reported as a value, and a UTF-8 character split across two packets.
///
/// That last one is the reason `on_text` exists rather than a `setEncoding`
/// pass-through. TCP has no message boundaries, so a two-octet character can
/// arrive in two reads; decoding each read on its own turns it into two
/// replacement characters, and the bug only shows under load or with non-ASCII
/// input. The test sends `0xC3` at the end of one write and `0xA9` at the start
/// of the next, and asserts the server saw `é` rather than U+FFFD.
#[test]
fn net_carries_a_split_character_and_reports_a_bind_failure() {
    if !js_toolchain_available() {
        eprintln!("skipping std/net end-to-end: node/tsx not available");
        return;
    }
    let root = unique_tmp("netsock");
    let src = root.join("src");
    // A port unlikely to collide with anything else on the machine.
    let port = 45000 + (std::process::id() % 2000);
    write_file(
        &src,
        "main.glyph",
        &format!(
            r#"module main

import std/bytes
import std/io
import std/net
import std/net {{ Socket }}
import std/option {{ Some, None }}
import std/process
import std/result {{ Ok, Err }}
import std/store
import std/timers

const PORT: int = {port}

pub async fn main() -> void {{
  let first = await net.listen("127.0.0.1", PORT, fn(sock: Socket) {{
    net.on_error(sock, fn(m: string) {{ io.eprintln("sock: ${{m}}") }})
    net.on_text(sock, fn(text: string) {{
      io.println("got=${{text}}")
      net.send(sock, "echo:${{text}}")
    }})
  }})
  // A second listener on the same port cannot bind, and says so as a value
  // rather than throwing at a handler.
  match first {{
    Err(e) => io.eprintln("first=${{e.message}}"),
    Ok(server) => {{
      match await net.listen("127.0.0.1", PORT, fn(_: Socket) {{}}) {{
        Ok(_) => io.println("second=bound (wrong)"),
        Err(e) => io.println("second=refused:${{e.kind}}"),
      }}
      drive()
      net.on_stop(server, fn() {{ io.println("first=closed") }})
    }},
  }}
}}

fn drive() -> void {{
  let c = net.connect("127.0.0.1", PORT)
  let seen = store.create<int>(0)
  net.on_error(c, fn(m: string) {{ io.eprintln("client: ${{m}}") }})
  net.on_text(c, fn(text: string) {{
    io.println("client=${{text}}")
    seen.update(fn(n: int) {{ n + 1 }})
    match seen.get() >= 2 {{
      true => {{
        net.close(c)
        process.exit(0)
      }},
      false => {{}},
    }}
  }})
  net.on_connect(c, fn() {{
    // "hi " then a lone 0xC3, which is the first half of "é".
    net.send_bytes(c, octets([104, 105, 32, 195,]))
    timers.after(60, fn() {{ net.send_bytes(c, octets([169, 33,])) }})
  }})
}}

fn octets(xs: Array<int>) -> bytes.Bytes {{
  return match bytes.from_array(xs) {{
    Ok(b) => b,
    Err(_) => bytes.empty,
  }}
}}
"#
        ),
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("second=refused:in_use"),
        "a port already in use is a structured Err, not a throw or a string to scrape: {stdout}"
    );
    // The lone 0xC3 is held back rather than decoded to U+FFFD, so the first
    // read is "hi " and the character appears whole in the second.
    assert!(stdout.contains("got=hi "), "first read stops before the partial character: {stdout}");
    assert!(
        stdout.contains("got=\u{e9}!"),
        "the split character arrives whole, not as U+FFFD: {stdout}"
    );
    assert!(
        !stdout.contains('\u{fffd}'),
        "nothing was replaced by U+FFFD: {stdout}"
    );
    assert!(stdout.contains("client=echo:hi "), "the echo came back: {stdout}");
}

/// `std/url` against the cases a hand-rolled parser gets wrong.
///
/// The confusable authority is the one that matters: `https://evil.com@example.com/`
/// has host `example.com`, and splitting on `/` or looking for the first `.`
/// answers `evil.com`. Pure, so it runs as `@example` with no network.
#[test]
fn url_parses_the_cases_a_hand_rolled_parser_gets_wrong() {
    if !js_toolchain_available() {
        eprintln!("skipping std/url examples: node/tsx not available");
        return;
    }
    let root = unique_tmp("stdurl");
    let src = root.join("src");
    write_file(
        &src,
        "u.glyph",
        r#"module u

import std/option { Option, Some, None }
import std/result { Result, Ok, Err }
import std/url

// The userinfo belongs to the authority, not the host.
@example host_of("https://evil.com@example.com/a") == Ok("example.com")
@example host_of("https://example.com:8443/a") == Ok("example.com")
@example host_of("not a url") == Err("not a URL: \"not a url\"")
pub fn host_of(text: string) -> Result<string, string> {
  return match url.parse(text) {
    Ok(u) => Ok(u.host),
    Err(e) => Err(e),
  }
}

// `port` is None when the scheme's default applies, so a round trip does not
// invent one.
@example port_of("https://example.com/a") == 0 - 1
@example port_of("https://example.com:8443/a") == 8443
pub fn port_of(text: string) -> int {
  return match url.parse(text) {
    Ok(u) => match u.port {
      Some(p) => p,
      None => 0 - 1,
    },
    Err(_) => 0 - 2,
  }
}

@example round_trip("https://example.com/a/b?x=1&x=2#top") == "https://example.com/a/b?x=1&x=2#top"
@example round_trip("https://example.com/") == "https://example.com/"
pub fn round_trip(text: string) -> string {
  return match url.parse(text) {
    Ok(u) => url.format(u),
    Err(e) => e,
  }
}

// Relative resolution, which string concatenation gets wrong.
@example resolve("https://x.test/a/b/c", "../d") == "https://x.test/a/d"
@example resolve("https://x.test/a/b", "/root") == "https://x.test/root"
@example resolve("https://x.test/a", "https://y.test/z") == "https://y.test/z"
pub fn resolve(base: string, rel: string) -> string {
  return match url.join(base, rel) {
    Ok(u) => url.format(u),
    Err(e) => e,
  }
}

// A repeated key keeps both values; a map would drop one.
@example both_values("x=1&x=2") == "1,2"
@example both_values("a=1&b=2") == ""
pub fn both_values(query: string) -> string {
  let out = ""
  for p in url.query_params(query) {
    match p.key == "x" {
      true => { mut out = match out == "" { true => p.value, false => "${out},${p.value}" } },
      false => {},
    }
  }
  return out
}

@example encoded("a b&c=d/e") == "a%20b%26c%3Dd%2Fe"
pub fn encoded(text: string) -> string {
  return url.encode_component(text)
}

// A malformed escape is refused rather than guessed at.
@example decoded("%41") == Ok("A")
@example decoded("%zz") == Err("malformed percent-encoding in \"%zz\"")
pub fn decoded(text: string) -> Result<string, string> {
  return url.decode_component(text)
}
"#,
    );
    let report = glyph_cli::examples::run_examples(&src).expect("run_examples ok");
    assert!(report.ran, "examples should have run");
    assert!(
        report.build_failed.is_none(),
        "the url module should compile: {:?}",
        report.build_failed
    );
    assert!(report.failures.is_empty(), "every case should hold: {:?}", report.failures);
    assert_eq!(report.total, 15, "every @example above ran");
}

/// `std/dns` and `std/tls` failure paths, without depending on the internet.
///
/// A name lookup that cannot succeed and a TLS connection to a closed local
/// port both have to arrive as `Err` rather than as a throw, which is the whole
/// reason these wrap node. `localhost` is the only name resolved, and it comes
/// from the hosts file rather than the network.
#[test]
fn dns_and_tls_failures_are_values() {
    if !js_toolchain_available() {
        eprintln!("skipping std/dns + std/tls: node/tsx not available");
        return;
    }
    let root = unique_tmp("dnstls");
    let src = root.join("src");
    let closed = 45000 + (std::process::id() % 2000) + 3;
    write_file(
        &src,
        "main.glyph",
        &format!(
            r#"module main

import std/dns
import std/io
import std/result {{ Ok, Err }}
import std/tls

pub async fn main() -> void {{
  match await dns.lookup("localhost") {{
    Ok(_) => io.println("localhost=resolved"),
    Err(e) => io.println("localhost=failed ${{e}}"),
  }}
  // `.invalid` is reserved by RFC 2606 and can never resolve.
  match await dns.ipv4("nothing-here.invalid") {{
    Ok(_) => io.println("invalid=resolved (wrong)"),
    Err(_) => io.println("invalid=refused"),
  }}
  match await tls.connect("127.0.0.1", {closed}, 5000) {{
    Ok(_) => io.println("tls=connected (wrong)"),
    Err(_) => io.println("tls=refused"),
  }}
}}
"#
        ),
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(stdout.contains("localhost=resolved"), "the hosts file resolves: {stdout}");
    assert!(
        stdout.contains("invalid=refused"),
        "an unresolvable name is a value, not a throw: {stdout}"
    );
    assert!(
        stdout.contains("tls=refused"),
        "a refused connection is a value, not a throw: {stdout}"
    );
}

/// A TLS dial against a peer that never answers is bounded, and the process
/// still exits.
///
/// The peer here completes the TCP handshake and then says nothing, which is
/// what a wedged endpoint or a firewall that swallows the TLS records looks
/// like from the client. Before `connect` carried a deadline, that dial never
/// settled: no `Ok`, no `Err`, no handle to abort, and the pending socket kept
/// node's event loop alive forever, so the program printed its last line and
/// then had to be killed. Both halves are asserted, because a `connect` that
/// answers `Err` and leaves the socket attached would still hang the process.
///
/// The same run covers both ends of the deadline itself. `setTimeout` clamps a
/// delay past 2^31-1 to one millisecond rather than refusing it, so an
/// unchecked `connect` turns a 35-day bound into a 1ms failure that reports the
/// 35 days as elapsed, and neither usage error may wear the `host: ` prefix the
/// real network failures use.
#[test]
fn a_tls_dial_against_a_silent_peer_is_bounded() {
    if !js_toolchain_available() {
        eprintln!("skipping std/tls deadline: node/tsx not available");
        return;
    }
    let root = unique_tmp("tlsdeadline");
    let src = root.join("src");
    let port = 47000 + (std::process::id() % 2000);
    write_file(
        &src,
        "main.glyph",
        &format!(
            r#"module main

import std/io
import std/net
import std/net {{ Socket }}
import std/result {{ Ok, Err }}
import std/tls

const PORT: int = {port}

pub async fn main() -> void {{
  // Accepts the connection and then says nothing: the TLS handshake can never
  // finish, and nothing will ever close this end.
  match await net.listen("127.0.0.1", PORT, fn(peer: Socket) {{}}) {{
    Ok(server) => {{
      match await tls.connect("127.0.0.1", PORT, 500) {{
        Ok(_) => io.println("tls=handshook (wrong)"),
        Err(_) => io.println("tls=deadline"),
      }}
      // A deadline node's timer cannot hold. `setTimeout` clamps anything past
      // 2^31-1 to one millisecond instead of refusing it, so without a check
      // this dials, fails in 1ms, and blames a deadline that never elapsed.
      match await tls.connect("127.0.0.1", PORT, 3000000000) {{
        Ok(_) => io.println("huge=handshook (wrong)"),
        Err(e) => io.println("huge=${{e}}"),
      }}
      match await tls.connect("127.0.0.1", PORT, 0) {{
        Ok(_) => io.println("zero=handshook (wrong)"),
        Err(e) => io.println("zero=${{e}}"),
      }}
      net.stop(server)
      io.println("main is returning now")
    }},
    Err(e) => io.println("listen=failed ${{e.message}}"),
  }}
}}
"#
        ),
    );
    let entry = src.join("main.glyph");
    // The dial's own bound is 500ms and compiling the program is a few seconds,
    // so this is generous. It is not more generous than that on purpose: the
    // budget is the cost of *reporting* a regression, and a suite that takes a
    // minute and a half to tell you the loop is pinned again is a suite you
    // stop running.
    let finished = spawn_glyph_bounded(
        &[std::ffi::OsStr::new("run"), entry.as_os_str()],
        std::time::Duration::from_secs(30),
    );
    let (code, stdout, stderr) = match finished {
        Some(done) => done,
        None => panic!(
            "the program never exited: a dial that cannot complete left the event loop pinned"
        ),
    };
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("tls=deadline"),
        "a handshake that cannot finish is an `Err`, not a wait: {stdout}"
    );
    assert!(
        stdout.contains("main is returning now"),
        "the program reached its end: {stdout}"
    );
    // A deadline larger than node can hold is refused, not silently clamped to
    // 1ms and then reported as if the number asked for had elapsed.
    assert!(
        stdout.contains("huge=a TLS dial deadline must be at most 2147483647ms, got 3000000000"),
        "an unholdable deadline is a usage error naming the limit: {stdout}"
    );
    assert!(
        !stdout.contains("no TLS handshake within 3000000000ms"),
        "a 1ms failure must not claim the deadline it was asked for elapsed: {stdout}"
    );
    // Neither usage error wears the `host: ` prefix the network failures use.
    // A caller logging the string files a programming error as an endpoint
    // being down, which for an uptime monitor is the wrong answer twice.
    assert!(
        stdout.contains("zero=a TLS dial needs a deadline greater than 0ms, got 0"),
        "a zero deadline is a usage error: {stdout}"
    );
    assert!(
        !stdout.contains("zero=127.0.0.1:") && !stdout.contains("huge=127.0.0.1:"),
        "a usage error must not impersonate a connection failure: {stdout}"
    );
}

/// `std/websocket` end to end: a Glyph server and Node's own WHATWG client
/// talking over a real socket.
///
/// The client is the host's, not ours, which is what makes this worth running:
/// it validates the `Sec-WebSocket-Accept` handshake against an implementation
/// that did not come from this repo, and it refuses the connection outright if
/// the digest is wrong.
///
/// Three payload sizes on purpose. A WebSocket frame encodes its length in
/// 7 bits, or 16, or 64, depending on size, and the boundaries are 126 and
/// 65536; a codec that gets the extended forms wrong still passes every test
/// that only sends short messages.
#[test]
fn websocket_server_and_client_exchange_text_and_binary() {
    if !js_toolchain_available() {
        eprintln!("skipping std/websocket end-to-end: node/tsx not available");
        return;
    }
    let root = unique_tmp("wsboth");
    let src = root.join("src");
    let port = 46000 + (std::process::id() % 2000);
    write_file(
        &src,
        "main.glyph",
        &format!(
            r#"module main

import std/array
import std/bytes
import std/io
import std/result {{ Ok, Err }}
import std/store
import std/websocket
import std/websocket {{ Server, Socket }}

const PORT: int = {port}

pub async fn main() -> void {{
  match await websocket.listen("127.0.0.1", PORT, fn(peer: Socket) {{
    websocket.on_message(peer, fn(text: string) {{
      io.println("server_text=${{text}}")
      websocket.send(peer, "echo:${{text}}")
    }})
    websocket.on_binary(peer, fn(b: bytes.Bytes) {{
      // Report the length and a fingerprint rather than the whole payload, so
      // a 70 KB frame does not become 140 KB of test output.
      let n = number.to_string(bytes.len(b))
      let head = bytes.to_hex(bytes.slice(b, 0, 4))
      io.println("server_bin=${{n}}:${{head}}")
      websocket.send_bytes(peer, b)
    }})
  }}) {{
    Err(why) => io.eprintln("cannot bind: ${{why}}"),
    Ok(server) => {{
      io.println("bound")
      drive(server)
    }},
  }}
}}

// 7-bit, 16-bit and 64-bit length forms.
fn sizes() -> Array<int> {{
  return [8, 200, 70000,]
}}

fn payload(n: int) -> bytes.Bytes {{
  return match bytes.from_array(array.map(array.range(n), fn(i: int) -> int {{
    return (i * 7 + 3) % 256
  }})) {{
    Ok(b) => b,
    Err(_) => bytes.empty,
  }}
}}

fn drive(server: Server) -> void {{
  let sent = store.create<int>(0)
  let c = websocket.connect("ws://127.0.0.1:${{number.to_string(PORT)}}")
  websocket.on_open(c, fn() {{ websocket.send(c, "hello") }})
  websocket.on_message(c, fn(text: string) {{
    io.println("client_text=${{text}}")
    websocket.send_bytes(c, payload(8))
  }})
  websocket.on_binary(c, fn(b: bytes.Bytes) {{
    let n = number.to_string(bytes.len(b))
    let head = bytes.to_hex(bytes.slice(b, 0, 4))
    io.println("client_bin=${{n}}:${{head}}")
    sent.update(fn(k: int) {{ k + 1 }})
    let next = sent.get()
    match next < 3 {{
      true => websocket.send_bytes(c, payload(index_size(next))),
      false => {{
        websocket.close(c)
        websocket.stop(server)
      }},
    }}
  }})
  websocket.on_close(c, fn(code: int, reason: string) {{
    io.println("client_close=${{number.to_string(code)}}")
  }})
}}

fn index_size(i: int) -> int {{
  return match array.get(sizes(), i) {{
    Some(v) => v,
    None => 8,
  }}
}}
"#
        ),
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(stdout.contains("bound"), "the server bound: {stdout}");
    // The handshake: Node's client refuses outright if Sec-WebSocket-Accept is
    // wrong, so any traffic at all proves the digest.
    assert!(stdout.contains("server_text=hello"), "text reached the server: {stdout}");
    assert!(stdout.contains("client_text=echo:hello"), "and came back: {stdout}");
    // Each length encoding, in both directions, with the payload unchanged.
    for n in ["8", "200", "70000"] {
        assert!(
            stdout.contains(&format!("server_bin={n}:030a1118")),
            "a {n}-byte frame reached the server intact: {stdout}"
        );
        assert!(
            stdout.contains(&format!("client_bin={n}:030a1118")),
            "and came back intact: {stdout}"
        );
    }
    // 1000 is a deliberate close. 1005 would mean the close frame carried no
    // status, which is what an earlier version of this server sent.
    assert!(stdout.contains("client_close=1000"), "a clean close says so: {stdout}");
}

/// A record payload's field matched against a nested pattern, two levels deep:
/// the Okasaki red-black `balance` rotation, which is the shape D8 forces on
/// any multi-field variant payload. The arm names a variant tag in one field
/// (`color: Black`) and a whole nested constructor in another (`left: Node({
/// color: Red, ... })`), and the arms that do not match must fall through to
/// the next one rather than being swallowed by the outer `Node` tag.
#[test]
fn a_nested_pattern_in_an_object_pattern_field_matches_and_falls_through() {
    let root = unique_tmp("nestedfield");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io { println }

type Color =
  | Red
  | Black

type Tree =
  | Leaf
  | Node({ color: Color, left: Tree, value: number, right: Tree })

fn balance(t: Tree) -> Tree {
  return match t {
    Node({ color: Black, left: Node({ color: Red, left: a, value: x, right: b }), value: y, right: c }) =>
      Node({ color: Red, left: Node({ color: Black, left: a, value: x, right: b }), value: y, right: c }),
    other => other,
  }
}

fn label(t: Tree) -> string {
  return match t {
    Leaf => "leaf",
    Node({ color: Red, value: v, left: l, right: r }) => "red",
    else => "black",
  }
}

fn main() {
  let inner = Node({ color: Red, left: Leaf, value: 1, right: Leaf })
  let outer = Node({ color: Black, left: inner, value: 2, right: Leaf })
  println("balanced=" + label(balance(outer)))
  let plain = Node({ color: Black, left: Leaf, value: 3, right: Leaf })
  println("untouched=" + label(balance(plain)))
}
"#,
    );
    let dist = root.join("dist");
    let report = build_project_inner(&src, &dist, false).expect("build ok");
    assert!(
        !report.has_errors(),
        "a nested object-pattern field should compile: {:?}",
        report.diagnostics
    );
    // The build-level assertion above passes whatever the arm lowers to, so it
    // proves nothing on its own: read the emitted TypeScript and require the
    // exclusive chain. A `switch` on the outer tag would reach the field tests
    // only after entering the `Node` case, and could not leave it again.
    let ts = std::fs::read_to_string(dist.join("main.ts")).expect("read emitted main.ts");
    assert!(
        ts.contains(r#".tag === "Node" && "#) && ts.contains(r#".left.color.tag === "Red""#),
        "the arm lowers to a conjunction of field tests, two levels deep: {ts}"
    );
    assert!(
        ts.contains("} else if ("),
        "a second arm sharing the outer tag has to be reachable: {ts}"
    );

    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    // The rotation fired: a black node whose left child is red recolors to red.
    assert!(stdout.contains("balanced=red"), "the rotation arm matched: {stdout}");
    // And the arm that does not match falls through to `other` instead of being
    // swallowed by the outer `Node` tag.
    assert!(stdout.contains("untouched=black"), "the non-matching value fell through: {stdout}");
}

/// A field pattern that can fail does not cover its variant. Without a
/// catch-all the match is non-exhaustive and must say so, rather than being
/// accepted because a `Node({ ... })` arm exists at all.
#[test]
fn a_refutable_object_pattern_field_does_not_cover_its_variant() {
    let root = unique_tmp("nestedexh");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

type Color =
  | Red
  | Black

type Tree =
  | Leaf
  | Node({ color: Color, value: number })

fn label(t: Tree) -> string {
  return match t {
    Leaf => "leaf",
    Node({ color: Red, value: v }) => "red",
  }
}

fn main() {
  let _ = label(Leaf)
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    let text = format!("{:?}", report.diagnostics);
    assert!(
        text.contains("E0200") && text.contains("`Node`"),
        "a refutable field pattern must not count as covering `Node`: {text}"
    );
}

/// The same nested field pattern, over a union declared in *another* module.
/// The scrutinee is fully typed (`Ty::Imported`), and the only question the
/// emitter has to answer — is `Node`'s payload spread flat or stored under
/// `value` — is one the project-wide record-variant registry already answers
/// across modules. A multi-module project is the normal layout, so an arm that
/// works only when the union is declared in the matching file is a feature that
/// is not there.
#[test]
fn a_nested_field_pattern_works_on_an_imported_union() {
    let root = unique_tmp("nestedimport");
    let src = root.join("src");
    write_file(
        &src,
        "shapes.glyph",
        r#"module shapes

pub type Color =
  | Red
  | Black

pub type Tree =
  | Leaf
  | Node({ color: Color, value: number })
"#,
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io { println }
import shapes { Tree, Node, Leaf, Red, Black }

fn label(t: Tree) -> string {
  return match t {
    Node({ color: Black, value: v }) => "black",
    Node({ color: Red, value: v }) => "red",
    else => "leaf",
  }
}

fn main() {
  println("a=" + label(Node({ color: Black, value: 1, })))
  println("b=" + label(Node({ color: Red, value: 2, })))
  println("c=" + label(Leaf))
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(
        !report.has_errors(),
        "an imported union's payload field should be matchable: {:?}",
        report.diagnostics
    );

    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(stdout.contains("a=black"), "the Black arm matched: {stdout}");
    assert!(stdout.contains("b=red"), "the Red arm matched: {stdout}");
    assert!(stdout.contains("c=leaf"), "the else arm took the rest: {stdout}");
}

/// The namespace-import twin of `a_nested_field_pattern_works_on_an_imported_union`:
/// same declaration, same nested constructor pattern, only the import spelling
/// and the match arms are written as `tree.Node(...)` instead of pulling
/// `Node`/`Leaf` into scope by name. D9/G75 both say the import spelling must
/// not change what a program means, but this one changed whether it *builds*
/// at all: matched through `import tree { Tree, Leaf, Node }` the payload's
/// storage is decided from the resolved-symbol lookup and the arm emits fine;
/// matched through `import tree` + `tree.Node(...)` the same lookup misses
/// because the constructor name carries a namespace prefix, so the emitter
/// falls through to "cannot be decided here" on a pattern that is otherwise
/// identical. If this regresses, a nested field pattern over a payload-carrying
/// variant reached through a namespace import goes back to E0300 while the
/// named-import spelling of the exact same code keeps compiling, which is
/// exactly the two-answers-for-one-declaration bug G75 was written to close.
#[test]
fn a_nested_field_pattern_works_on_an_imported_union_through_its_namespace() {
    let root = unique_tmp("nestedimportns");
    let src = root.join("src");
    write_file(
        &src,
        "tree.glyph",
        r#"module tree

pub type Tree =
  | Leaf
  | Node({ left: Tree, key: string, right: Tree })
"#,
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io { println }
import tree

fn shape(t: tree.Tree) -> string {
  return match t {
    tree.Node({ left: tree.Node({ key: lk, left: ll, right: lr }), key: k, right: r }) => "deep:" + lk + "/" + k,
    tree.Node({ left: tree.Leaf, key: k, right: r }) => "shallow:" + k,
    else => "leaf",
  }
}

fn main() {
  let deep = tree.Node({
    left: tree.Node({ left: tree.Leaf, key: "inner", right: tree.Leaf, }),
    key: "outer",
    right: tree.Leaf,
  })
  let shallow = tree.Node({ left: tree.Leaf, key: "solo", right: tree.Leaf, })
  println("a=" + shape(deep))
  println("b=" + shape(shallow))
  println("c=" + shape(tree.Leaf))
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(
        !report.has_errors(),
        "a nested constructor pattern on an imported union's payload-carrying \
         variant must build the same way through the namespace spelling as it \
         does through the named-import spelling: {:?}",
        report.diagnostics
    );

    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(stdout.contains("a=deep:inner/outer"), "the nested payload-carrying arm matched: {stdout}");
    assert!(stdout.contains("b=shallow:solo"), "the nested nullary-variant arm matched: {stdout}");
    assert!(stdout.contains("c=leaf"), "the else arm took the rest: {stdout}");
}

/// The same imported-union arm, but over a *generic self-referential* union
/// instantiated at a concrete record: `Tree<T> = Leaf(T) | Dir({ value: T,
/// children: Array<Tree<T>> })`, declared in one module and matched in
/// another at `Tree<Payload>`. Three things have to line up at once here that
/// the non-generic case never asks for. The emitter has to read the arm's
/// payload shape back from the matched type rather than from the arm's
/// syntax, that type has to survive the module boundary, and it has to survive
/// substitution of `T` on the way. The last one is where it broke: the arm's
/// type is `Ty::App` over `Ty::Imported`, and `payload_shape` has to unwrap the
/// application before any of its four lookups can decide anything. That holds
/// for any imported generic union under a nested pattern, recursive or not, and
/// a two-variant non-recursive control fails identically without the unwrap.
/// The recursion is here because the finding had it, and because the recursive
/// `for` over `children` exercises the arm at depth.
///
/// The run half is the part worth keeping. A tag test that resolved to the
/// wrong variant still type-checks, so the assertion is on the nesting the
/// recursion prints, not on the build being green.
#[test]
fn a_generic_self_referential_imported_union_matches_at_a_concrete_type() {
    let root = unique_tmp("genrecursive");
    let src = root.join("src");
    write_file(
        &src,
        "tree.glyph",
        r#"module tree

pub type Tree<T> =
  | Leaf(T)
  | Dir({ value: T, children: Array<Tree<T>> })
"#,
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io { println }
import tree { Tree, Leaf, Dir }

type Payload = { name: string }

fn render(t: Tree<Payload>) -> string {
  return match t {
    Leaf({ name }) => name,
    Dir({ value: { name }, children }) => {
      let out = name + "["
      for c in children {
        mut out = out + render(c)
      }
      return out + "]"
    },
  }
}

fn main() {
  let a: Tree<Payload> = Leaf({ name: "a", })
  let b: Tree<Payload> = Leaf({ name: "b", })
  let inner: Tree<Payload> = Dir({ value: { name: "i", }, children: [a, b,], })
  let root: Tree<Payload> = Dir({ value: { name: "r", }, children: [inner,], })
  println("tree=" + render(root))
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(
        !report.has_errors(),
        "a generic self-referential imported union should be matchable at a \
         concrete instantiation: {:?}",
        report.diagnostics
    );

    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("tree=r[i[ab]]"),
        "each arm should take the variant it names, all the way down the \
         recursion: {stdout}"
    );
}

/// The same nested field pattern as the red-black rotation above, over a union
/// this module declares that is *generic and self-referential*: `Tree<K>` whose
/// `Node` payload holds two `Tree<K>` children. Nothing about the arm changes;
/// only the union's arity does.
///
/// That was the whole trigger. A constructor pattern's sub-patterns take their
/// types from the variant's payload, and that lookup wanted a bare `Ty::Named`.
/// A scrutinee of `Tree<K>` is a `Ty::App` over the union, so no payload type
/// was recorded, nothing under the payload got a type, and the emitter had
/// nothing left to decide flat-versus-boxed from. The arm was refused as
/// undecidable even though the declaration was in the same file.
///
/// Both spellings of the scrutinee are here because they reach the lookup with
/// different arguments: `balance<K>` matches at an open `Ty::Param`, `depth`
/// matches at the concrete `Tree<string>` the substitution has to survive. The
/// run half is what proves the arm resolved to the variant it names, since a
/// tag test pointing at the wrong variant still type-checks.
#[test]
fn a_nested_field_pattern_works_on_a_generic_self_referential_local_union() {
    let root = unique_tmp("genlocalnested");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io { println }

type Color =
  | Red
  | Black

type Tree<K> =
  | Leaf
  | Node({ color: Color, left: Tree<K>, key: K, right: Tree<K> })

fn balance<K>(t: Tree<K>) -> Tree<K> {
  return match t {
    Node({ color: Black, left: Node({ color: Red, left: a, key: x, right: b }), key: y, right: c }) =>
      Node({ color: Red, left: Node({ color: Black, left: a, key: x, right: b }), key: y, right: c }),
    other => other,
  }
}

fn label<K>(t: Tree<K>) -> string {
  return match t {
    Leaf => "leaf",
    Node({ color: Red, key: v, left: l, right: r }) => "red",
    else => "black",
  }
}

fn depth(t: Tree<string>) -> number {
  return match t {
    Node({ color: c1, left: Node({ color: c2, left: a, key: x, right: b }), key: y, right: r }) => 2,
    Node({ color: c, left: l, key: k, right: r }) => 1,
    Leaf => 0,
  }
}

fn main() {
  let inner: Tree<string> = Node({ color: Red, left: Leaf, key: "i", right: Leaf })
  let outer: Tree<string> = Node({ color: Black, left: inner, key: "o", right: Leaf })
  println("balanced=" + label(balance(outer)))
  let plain: Tree<string> = Node({ color: Black, left: Leaf, key: "p", right: Leaf })
  println("untouched=" + label(balance(plain)))
  println("deep=" + depth(outer))
  println("shallow=" + depth(plain))
}
"#,
    );
    let dist = root.join("dist");
    let report = build_project_inner(&src, &dist, false).expect("build ok");
    assert!(
        !report.has_errors(),
        "a nested field pattern over a generic self-referential local union \
         should compile: {:?}",
        report.diagnostics
    );
    // The green build alone proves nothing about the lowering, so read the
    // emitted TypeScript: the record payload is spread flat into the tag
    // object, so the inner test has to reach through `.left` and not through
    // `.left.value`.
    let ts = std::fs::read_to_string(dist.join("main.ts")).expect("read emitted main.ts");
    assert!(
        ts.contains(r#".left.color.tag === "Red""#),
        "the nested field test reads the flat payload two levels deep: {ts}"
    );

    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(stdout.contains("balanced=red"), "the rotation arm matched: {stdout}");
    assert!(stdout.contains("untouched=black"), "the non-matching value fell through: {stdout}");
    assert!(stdout.contains("deep=2"), "the nested arm won at a concrete type: {stdout}");
    assert!(stdout.contains("shallow=1"), "the flat arm took the rest: {stdout}");
}

/// Exhaustiveness on a union does not depend on whether the union is generic.
///
/// `required_variants` is what decides whether a match is missing an arm, and
/// it reaches a module-local union through `named_union_variants`. That lookup
/// wanted a bare `Ty::Named`, so a scrutinee of `Tree<K>` (a `Ty::App` over the
/// union) produced no variant set at all and the whole check quietly declined
/// to run. Delete `<K>` from the same program and it is E0200; keep it and the
/// program builds, passes `tsc --strict`, and throws `non-exhaustive match` at
/// runtime instead.
///
/// Both spellings of the scrutinee are asserted for the same reason the nested
/// pattern test carries both: `genmissing<K>` reaches the lookup at an open
/// `Ty::Param`, `concrete` at `Tree<string>`.
#[test]
fn a_generic_local_union_is_exhaustiveness_checked_like_its_non_generic_twin() {
    let root = unique_tmp("genexhaust");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

type Color =
  | Red
  | Black

type Tree<K> =
  | Leaf
  | Node({ color: Color, left: Tree<K>, key: K, right: Tree<K> })

fn genmissing<K>(t: Tree<K>) -> string {
  return match t {
    Node({ color: c, left: l, key: k, right: r }) => "node",
  }
}

fn concrete(t: Tree<string>) -> string {
  return match t {
    Node({ color: c, left: l, key: k, right: r }) => "node",
  }
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let missing: Vec<&String> = report
        .diagnostics
        .iter()
        .filter(|d| d.contains("E0200") && d.contains("Tree") && d.contains("Leaf"))
        .collect();
    assert_eq!(
        missing.len(),
        2,
        "both the open-param and the concrete spelling must report the missing \
         `Leaf` arm: {:?}",
        report.diagnostics
    );
}

/// The same hole one level down: a generic union whose payload is *itself* a
/// union. Exhaustiveness recurses into a constructor pattern's payload, but the
/// recursion is only reached once the outer scrutinee produced a variant set,
/// so a `Ty::App` outer type skipped the inner check too.
///
/// This one is worth its own test because the arm it lets through is a
/// miscompile, not just an unchecked value: `B(X)` over an unchecked union
/// lowers `X` to `const X = __m0.value`, a binding shadowing the variant, so
/// `f(B(Y))` returned the `X` arm's answer. The non-generic spelling never
/// reached the emitter because E0200 stopped it; the generic one has to be
/// stopped by the same error.
#[test]
fn a_generic_union_whose_payload_is_a_union_recurses_into_the_payload() {
    let root = unique_tmp("genexhaustinner");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

type Inner =
  | X
  | Y

type G<K> =
  | A
  | B(Inner)

fn f<K>(g: G<K>) -> string {
  return match g {
    A => "a",
    B(X) => "x",
  }
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Inner") && d.contains('Y')),
        "the missing inner variant `Y` must be reported through the generic \
         outer union: {:?}",
        report.diagnostics
    );
}

/// The case the three tests above do not reach: they all declare their union in
/// the module that matches it, which is the whole scope of the
/// `resolve_named_union` unwrap. An imported scrutinee is diverted earlier, in
/// `check_match_exhaustiveness`, and an imported *generic* scrutinee arrives
/// there as `Ty::App { base: Ty::Imported, .. }`. That gate used to test the
/// application itself for a bare `Ty::Imported`, so it never matched and
/// `check_imported_union_coverage` never ran: the build was clean, `tsc
/// --strict` passed, and the program threw `non-exhaustive match` at run time.
/// It now tests `union_base(scrutinee_ty)`, so the arity is invisible to the
/// gate (G148).
///
/// This test previously pinned that hole open (G142's second half) because the
/// claim had been written down as closed once already, on a fix that only
/// covered module-local unions. It is inverted rather than deleted: the same
/// program, the same two spellings, asserting the answer the language promises.
///
/// The control below is the point. The identical program with `<K>` deleted
/// from both files was `E0200` throughout, so this was one keystroke away from
/// a checked program the whole time.
#[test]
fn an_imported_generic_union_is_exhaustiveness_checked() {
    const TREE: &str = "module tree\n\
         \n\
         pub type Tree<K> =\n\
         \x20 | Leaf\n\
         \x20 | Node({ key: K })\n";
    const PLAIN: &str = "module tree\n\
         \n\
         pub type Tree =\n\
         \x20 | Leaf\n\
         \x20 | Node({ key: string })\n";
    const MAIN: &str = "module main\n\
         \n\
         import std/io { println }\n\
         import tree { Tree, Leaf, Node }\n\
         \n\
         fn label(t: Tree<string>) -> string {\n\
         \x20 return match t {\n\
         \x20\x20\x20 Node({ key: k }) => k,\n\
         \x20 }\n\
         }\n\
         \n\
         fn main() {\n\
         \x20 println(label(Leaf))\n\
         }\n";

    // The generic spelling: the omitted `Leaf` arm is E0200, named, at compile
    // time.
    let root = unique_tmp("impgenexhaust");
    let src = root.join("src");
    write_file(&src, "tree.glyph", TREE);
    write_file(&src, "main.glyph", MAIN);
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.has_errors(),
        "an imported generic union must be exhaustiveness-checked: {:?}",
        report.diagnostics
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Tree") && d.contains("`Leaf`")),
        "the missing variant must be named, exactly as the non-generic \
         spelling names it: {:?}",
        report.diagnostics
    );

    // The concrete instantiation above is not the only spelling that used to
    // fall through the old gate: an open type parameter on the consuming
    // function (`fn label<K>(t: Tree<K>)`) reaches `check_match_exhaustiveness`
    // as the exact same `Ty::App { base: Ty::Imported, .. }` shape, with `K`
    // never resolved to a concrete argument at all. G141/G142 held this line
    // for module-local unions at both an open parameter and a concrete
    // instantiation; the imported side should hold it too.
    const MAIN_OPEN_PARAM: &str = "module main\n\
         \n\
         import std/io { println }\n\
         import tree { Tree, Leaf, Node }\n\
         \n\
         fn label<K>(t: Tree<K>) -> string {\n\
         \x20 return match t {\n\
         \x20\x20\x20 Node({ key: k }) => \"node\",\n\
         \x20 }\n\
         }\n\
         \n\
         fn main() {\n\
         \x20 println(label(Leaf))\n\
         }\n";
    let open_root = unique_tmp("impgenexhaustopen");
    let open_src = open_root.join("src");
    write_file(&open_src, "tree.glyph", TREE);
    write_file(&open_src, "main.glyph", MAIN_OPEN_PARAM);
    let open_report =
        build_project_inner(&open_src, &open_root.join("dist"), false).expect("build");
    assert!(
        open_report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Tree") && d.contains("`Leaf`")),
        "an imported generic union must be exhaustiveness-checked at an open \
         type parameter, not only at a concrete instantiation: {:?}",
        open_report.diagnostics
    );

    // The diagnostic is not the whole finding; the throw it replaces is. Adding
    // the arm the checker asked for has to produce a program that builds and
    // runs, so this pins observable behaviour rather than the current spelling
    // of a check.
    let fixed_root = unique_tmp("impgenexhaustfixed");
    let fixed_src = fixed_root.join("src");
    write_file(&fixed_src, "tree.glyph", TREE);
    write_file(
        &fixed_src,
        "main.glyph",
        &MAIN.replace(
            "   Node({ key: k }) => k,\n",
            "   Leaf => \"leaf\",\n    Node({ key: k }) => k,\n",
        ),
    );
    let fixed =
        build_project_inner(&fixed_src, &fixed_root.join("dist"), false).expect("build");
    assert!(
        !fixed.has_errors(),
        "covering every variant must build clean: {:?}",
        fixed.diagnostics
    );
    if js_toolchain_available() {
        let entry = fixed_src.join("main.glyph");
        let (code, stdout, stderr, _) =
            spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
        assert_eq!(code, 0, "the covered match must run: {stdout} {stderr}");
        assert!(stdout.contains("leaf"), "stdout: {stdout}");
    } else {
        eprintln!("skipping imported-generic-exhaustiveness run: node/tsx not available");
    }

    // The control: the same two files with the arity removed. One keystroke of
    // difference, and this one is caught.
    let plain_root = unique_tmp("impplainexhaust");
    let plain_src = plain_root.join("src");
    write_file(&plain_src, "tree.glyph", PLAIN);
    write_file(&plain_src, "main.glyph", &MAIN.replace("Tree<string>", "Tree"));
    let plain = build_project_inner(&plain_src, &plain_root.join("dist"), false).expect("build");
    assert!(
        plain
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Tree") && d.contains("Leaf")),
        "the non-generic spelling of the identical imported program must still \
         be caught; the two spellings agree, which is the whole point of \
         G148: {:?}",
        plain.diagnostics
    );
}

/// A match whose arms can all fail, over a scrutinee with no variant set to
/// reason about. There is no tag to count, but there is nothing to guarantee
/// the match produces a value either, and the emitted chain throws. The
/// compiler has to say so: a construct that compiles clean and throws is the
/// one thing the first pillar rules out.
#[test]
fn a_record_match_whose_arms_can_all_fail_is_non_exhaustive() {
    let root = unique_tmp("recordexh");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

type Point = { x: number, y: number, }

fn f(p: Point) -> string {
  return match p {
    { x: 0, y: y, } => "origin-ish",
  }
}

fn main() {
  let _ = f({ x: 3, y: 4, })
}
"#,
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    let text = format!("{:?}", report.diagnostics);
    assert!(
        text.contains("E0226"),
        "a match with only refutable arms and no catch-all must be reported: {text}"
    );
}
