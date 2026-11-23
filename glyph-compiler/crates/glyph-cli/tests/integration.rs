//! Integration tests for `glyph build`.
//!
//! Each test writes a small multi-file fixture to a unique temp
//! directory, calls `build_project` directly (no subprocess), and asserts
//! on the `BuildReport`. Cleanup is best-effort — `std::env::temp_dir()`
//! is the OS temp dir, periodically cleaned by the system.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_cli::test_support::{js_toolchain_available, tool_missing, tsc_available};
use glyph_cli::{build::build_project_inner, build_project};

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
    assert!(
        map.contains("\"sources\":[\"main.glyph\"]"),
        "names the source: {map}"
    );
    assert!(map.contains("fn f"), "embeds sourcesContent: {map}");
    // Non-empty mappings (not the empty-string field).
    assert!(
        !map.contains("\"mappings\":\"\""),
        "non-empty mappings: {map}"
    );
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
    assert!(
        d.range.start.line >= 1 && d.range.start.col >= 1,
        "1-based pos"
    );
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
    assert!(
        point_is.contains("Object.keys"),
        "strict record checks its keys: {point_is}"
    );
    let loose = ts.split("export const Loose").nth(1).unwrap_or("");
    let loose_is = loose.split("parse(").next().unwrap_or("");
    assert!(
        !loose_is.contains("Object.keys"),
        "@open record does not check keys: {loose_is}"
    );
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
    assert!(
        ts.contains("redact(value: User)"),
        "emits a redact method: {ts}"
    );
    assert!(
        ts.contains("\"ssn\": \"[REDACTED]\""),
        "masks the field: {ts}"
    );
    // The other members are untouched — the redact method is additive.
    assert!(
        ts.contains("is(value: unknown): value is User"),
        "is intact: {ts}"
    );
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
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0219")),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn rebuild_prunes_a_renamed_modules_stale_ts_and_map() {
    // Build a module, then rename it and rebuild into the same out dir. The old
    // `.ts` and its `.ts.map` sidecar must be gone (no orphan tsc picks up),
    // while an unrelated file the user placed in the out dir is preserved.
    let root = unique_tmp("prune");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "old.glyph",
        "module old\nfn f() -> number { return 1 }\n",
    );
    build_project_inner(&src, &out, false).expect("first build");
    assert!(out.join("old.ts").exists(), "old.ts emitted");
    assert!(out.join("old.ts.map").exists(), "old.ts.map emitted");

    // A file the user dropped in the out dir must survive the prune.
    std::fs::write(out.join("keep.me"), "hand-written").unwrap();

    // Rename the module and rebuild.
    std::fs::remove_file(src.join("old.glyph")).unwrap();
    write_file(
        &src,
        "new.glyph",
        "module new\nfn f() -> number { return 2 }\n",
    );
    build_project_inner(&src, &out, false).expect("second build");

    assert!(out.join("new.ts").exists(), "new.ts emitted");
    assert!(!out.join("old.ts").exists(), "stale old.ts pruned");
    assert!(!out.join("old.ts.map").exists(), "stale old.ts.map pruned");
    assert!(
        out.join("keep.me").exists(),
        "unrelated user file preserved"
    );
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
    assert!(
        first.contains("Regenerate with"),
        "provenance header: {first}"
    );
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
    assert!(
        second.contains("title"),
        "the new field flowed in: {second}"
    );

    // Idempotent: a second regen with no spec change leaves the file identical.
    glyph_cli::regen::regen(&root).expect("regen again");
    assert_eq!(
        second,
        std::fs::read_to_string(&gen_file).unwrap(),
        "idempotent"
    );
}

#[test]
fn regen_reports_when_nothing_is_generated() {
    let root = unique_tmp("regen_empty");
    write_file(
        &root,
        "hand.glyph",
        "module m\nfn f() -> number { return 1 }\n",
    );
    let err = glyph_cli::regen::regen(&root).expect_err("no generated files");
    assert!(matches!(
        err,
        glyph_cli::regen::RegenError::NoGenerated { .. }
    ));
}

#[test]
fn a_type_name_with_braces_on_its_line_is_e0013_and_nothing_else() {
    // G217 and G351, end to end. `Order { ... }` used to parse as `return
    // Order` followed by an object literal: before E0228 the only thing the
    // compiler said was that the literal was unreachable, and it exited 0.
    // From 0.1.142 to 0.1.145 it said E0228 and then E0108 on the literal,
    // both true of that parse (record 104). The parse was the defect, and
    // since 0.1.146 a `{` on the line where a statement ended is E0013. A
    // parse error stops the file before the checker and the lints, so the
    // list is that one error, carrying E0228's guidance.
    let root = unique_tmp("type_name_as_value");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type Order = {\n  \
           id: string,\n  \
           total: int,\n\
         }\n\
         fn make() -> Order {\n  \
           return Order { id: \"a\", total: 1 }\n\
         }\n\
         fn main(argv: Array<string>) -> string { return make().id }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build ran");
    assert!(report.has_errors(), "{:?}", report.diagnostics);
    assert!(
        report.diagnostics.iter().any(|d| d.contains("[E0013]")
            && d.contains("`return Order` ends")
            && d.contains("no `TypeName { ... }` construction form")),
        "{:?}",
        report.diagnostics
    );
    let codes: Vec<(&str, &str)> = report
        .structured
        .iter()
        .map(|d| (d.code.as_str(), d.severity.as_str()))
        .collect();
    assert_eq!(codes, [("E0013", "error")], "{:?}", report.diagnostics);
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
    assert!(
        !report.has_errors(),
        "lints are warnings: {:?}",
        report.diagnostics
    );
    let codes: Vec<&str> = ["E0106", "E0107", "E0108"]
        .into_iter()
        .filter(|c| report.diagnostics.iter().any(|d| d.contains(c)))
        .collect();
    assert_eq!(
        codes,
        vec!["E0106", "E0107", "E0108"],
        "{:?}",
        report.diagnostics
    );
    assert_eq!(
        report.emitted,
        vec!["main.ts".to_string()],
        "warnings still emit"
    );
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
    // runtime store. The binding stays `const`; the `update` is a `mut`
    // statement (D5, G423 step 2).
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
           mut total.update(fn(n: number) -> number { return n + 1 })\n\
         }\n\
         fn value() -> number {\n  return total.get()\n}\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    assert!(ts.contains("create(0)"), "lowers create: {ts}");
    assert!(ts.contains("total.update("), "lowers update: {ts}");
    assert!(ts.contains("total.get()"), "lowers get: {ts}");
    // The store binding stays a plain const, never reassigned.
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
         component Panel(props: { show: bool }) -> Component {\n\
         \x20 return <div>\n\
         \x20   <if cond={props.show}>\n\
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
    assert!(
        !out.join("main.ts").exists(),
        "no .ts for a rejected module"
    );
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
    assert!(
        matches!(err, glyph_cli::BuildError::NoSources(_)),
        "got: {err:?}"
    );
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
    assert_eq!(
        report.diagnostics.len(),
        1,
        "diagnostics: {:?}",
        report.diagnostics
    );
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
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("Feed") && d.contains("Failed")),
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
    // is no longer read as a catch-all, the genuinely missing `Loading` still
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
             Loaded => 2,\n    \
             Failed => 3,\n    \
             Loadign => 1,\n  \
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
fn cross_module_union_typo_draws_e0220_like_the_module_local_one() {
    // G169. This test used to pin the opposite: E0220 was module-local, and a
    // cross-module typo drew nothing, because `check_imported_union_coverage`
    // counted any bare PascalCase head as covering a variant with no membership
    // check. It said the day that changed it should change here deliberately
    // rather than by surprise, and this is that change.
    //
    // `Loadign` over an imported `Feed` is the same mistake as `Loadign` over a
    // local one, so it gets the same diagnostic with the same suggestion. The
    // module-local twin is `build_flags_unknown_variant_pattern_with_suggestion`
    // and the two programs differ only in which module declares the union.
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
         import lib { Feed, Loaded, Failed }\n\
         fn show(f: Feed) -> number {\n  \
           return match f {\n    \
             Loaded => 2,\n    \
             Failed => 3,\n    \
             Loadign => 1,\n  \
           }\n\
         }\n",
    );

    let report = build_project(&src, &out).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0220") && d.contains("did you mean `Loading`?")),
        "a cross-module typo draws E0220 with the same suggestion as a local one: {:?}",
        report.diagnostics
    );
    // The typo arm covers nothing, so the variant it was meant to be is still
    // missing and E0200 surfaces beside it, exactly as on the local side.
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("Loading")),
        "the unnamed variant must still surface: {:?}",
        report.diagnostics
    );
}

/// G39. A module-level `const` carrying a type annotation is that type, so a
/// field typo on one is E0210 naming the type and the field, rather than a raw
/// `tsc` TS2339 pinned to the whole statement.
///
/// This runs through `build_project_inner` rather than the typechecker's own
/// harness on purpose. The lowering fix alone was inert here: `decl_ty` is a
/// salsa query whose resolver slice is built by `collect_signature_spans`, and
/// that slice held a callable's param and return types only, so a `const`'s
/// annotation resolved to nothing and lowered back to `Unknown` no matter what
/// the lowerer did with it. A unit test on the local resolver passed while the
/// compiler stayed silent.
#[test]
fn an_annotated_module_const_is_field_checked() {
    let root = unique_tmp("constann");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         type Sheet = { rows: number, cols: number }\n\
         const ORIGIN: Sheet = { rows: 0, cols: 0 }\n\
         fn f() -> number {\n  \
           return ORIGIN.rowz\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0210") && d.contains("rowz") && d.contains("Sheet")),
        "expected E0210 naming the type and the field; got: {:?}",
        report.diagnostics
    );
}

/// An annotated module `const` has its initializer judged against the
/// annotation, under the same code the annotated `let` uses. Run through
/// `build_project_inner` for the reason `an_annotated_module_const_is_field_checked`
/// is: the typechecker's own harness lowers through a different resolver than
/// the compiler does, and the lowering half of this work once passed there
/// while the shipped compiler stayed silent.
#[test]
fn an_annotated_module_const_initializer_is_checked() {
    let root = unique_tmp("constinit");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         const X: number = \"hi\"\n\
         fn f() -> number {\n  \
           return X\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0204") && d.contains("number") && d.contains("string")),
        "expected E0204 naming both types; got: {:?}",
        report.diagnostics
    );
}

/// The other half of the same rule, and the reason it is safe: a `const` with
/// no annotation claims nothing. Inferring a type from the initializer is a
/// separate decision, and reporting against a guess would reject working code.
#[test]
fn an_unannotated_module_const_claims_nothing() {
    let root = unique_tmp("constnoann");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         const ORIGIN = { rows: 0, cols: 0 }\n\
         fn f() -> number {\n  \
           return ORIGIN.rows\n\
         }\n",
    );

    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0210")),
        "no annotation, no claim: {:?}",
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
        report.diagnostics.iter().any(|d| d.contains("typecheck")
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
    write_file(
        &src,
        "node_modules/pkg/decoy.glyph",
        "module decoy\nfn main(\n",
    );

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
    assert!(examples.is_dir(), "examples dir not found at {examples:?}");
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
    assert!(
        !apps.is_empty(),
        "no multi-module apps found under examples/apps"
    );

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
    // Every emitted module unconditionally imports the bootstrap, and the
    // bootstrap itself unconditionally imports `std/result` for its `Result`
    // wrapping — so both are written no matter what the program imports.
    for rel in [
        ".glyph-runtime/std/result.ts",
        ".glyph-runtime/glyph-bootstrap.ts",
        ".glyph-runtime/glyph-prelude.d.ts",
        ".glyph-runtime/glyph-stdlib.d.ts",
    ] {
        assert!(
            out.join(rel).is_file(),
            "missing bundled runtime file {rel}"
        );
    }
    // This program imports no stdlib module and uses no descriptor or `Option`,
    // so `option.ts` and `schema.ts` are unreached (G115): writing them anyway
    // is exactly the unfiltered-bundle behavior a browser deploy target cannot
    // tolerate (see `build_prunes_unreached_std_modules_from_a_browser_hostile_program`).
    for rel in [
        ".glyph-runtime/std/option.ts",
        ".glyph-runtime/std/schema.ts",
    ] {
        assert!(
            !out.join(rel).is_file(),
            "{rel} written despite the program never reaching it"
        );
    }
}

/// A program is not required to declare a deploy target for pruning to matter:
/// even a plain build must not hand every consumer all 36 std modules, seven of
/// which (`dns`, `fs`, `http`, `net`, `process`, `sqlite`, `tls`) are Node-only
/// and break a browser bundler that tries to include them (G115). A program
/// that imports only `std/array` (which itself only reaches `std/option`) must
/// come out of the build with exactly its reachable set of std modules on disk,
/// bootstrap/result always included, and every unreached module — most
/// pointedly the seven host-only ones — absent.
#[test]
fn build_prunes_unreached_std_modules_from_a_browser_hostile_program() {
    let root = unique_tmp("prune_std");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/array

fn main() {
  let xs = [1, 2, 3]
  let _doubled = array.map(xs, fn(x) -> int {
    return x * 2
  })
  print("done")
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    // Reached: what the program imports (`array`), what that module itself
    // imports (`option`, for its `Option`-returning helpers), and the two
    // files every build always writes.
    for rel in [
        ".glyph-runtime/std/array.ts",
        ".glyph-runtime/std/option.ts",
        ".glyph-runtime/std/result.ts",
        ".glyph-runtime/glyph-bootstrap.ts",
    ] {
        assert!(
            out.join(rel).is_file(),
            "missing reachable runtime file {rel}"
        );
    }

    // Unreached, including all seven browser-hostile Node modules: a program
    // that never imports them, and whose transitive std dependencies never
    // reach them either, must not have them materialize into the output at all.
    for rel in [
        ".glyph-runtime/std/dns.ts",
        ".glyph-runtime/std/fs.ts",
        ".glyph-runtime/std/http.ts",
        ".glyph-runtime/std/net.ts",
        ".glyph-runtime/std/process.ts",
        ".glyph-runtime/std/sqlite.ts",
        ".glyph-runtime/std/tls.ts",
        ".glyph-runtime/std/schema.ts",
        ".glyph-runtime/std/json.ts",
    ] {
        assert!(
            !out.join(rel).is_file(),
            "{rel} written despite the program never reaching it"
        );
    }
}

/// The prune pass must see what a hand-written `extern/*.ts` shim imports.
/// `<src>/extern/**` is staged into `<out>/extern/` and type-checked by the
/// generated tsconfig, which deliberately maps `"std/*"` onto the bundled
/// runtime so a shim can reach the stdlib (D29). Reachability derived only from
/// the emitted Glyph modules prunes a std module only a shim pulls in, and the
/// build then fails with TS2307 on a program that used to compile. Both
/// spellings a shim can legally use count: the bare `std/<name>` the tsconfig
/// maps, and the relative `../.glyph-runtime/std/<name>`.
#[test]
fn rebuilding_after_deleting_an_extern_shim_does_not_leave_it_behind() {
    // Staging into `<out>/extern/` only ever added files. That was harmless
    // while every std module was written unconditionally: a shim left behind
    // after its source was deleted still resolved. Pruning made it fatal. The
    // stale copy is still inside the tsconfig's `include`, so it is still
    // type-checked, and the std module only it imported has just been pruned.
    // The build then failed on a path with no source, kept failing on every
    // rebuild, and only `rm -rf <out>` cleared it. Under `--watch` the session
    // stayed red for the rest of its life.
    //
    // This is incremental-only, which is why the whole suite stayed green: every
    // other extern test builds once into a fresh out dir.
    let root = unique_tmp("extern_stale");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "extern/clock.ts",
        "import * as time from \"std/time\";\n\nexport function stamp(): string {\n  return String(time.now());\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\nimport extern/clock { stamp }\n\nfn main() {\n  print(stamp())\n}\n",
    );

    let first = build_project_inner(&src, &out, false).expect("first build ok");
    assert!(
        !first.has_errors(),
        "first build diags: {:?}",
        first.diagnostics
    );
    assert!(
        out.join("extern/clock.ts").is_file(),
        "the shim should be staged"
    );

    // Delete the shim and the import that reached it, then rebuild into the
    // SAME out dir. This is the ordinary edit-and-rebuild path.
    std::fs::remove_file(src.join("extern/clock.ts")).expect("remove shim");
    write_file(
        &src,
        "main.glyph",
        "module main\n\nfn main() {\n  print(\"hi\")\n}\n",
    );

    let second = build_project_inner(&src, &out, false).expect("second build ok");
    assert!(
        !second.has_errors(),
        "a build after deleting an extern shim must not fail: {:?}",
        second.diagnostics
    );
    assert!(
        !out.join("extern/clock.ts").exists(),
        "the staged copy of a deleted shim must not survive into the next build"
    );
}

#[test]
fn build_keeps_std_modules_reached_only_by_an_extern_shim() {
    let root = unique_tmp("prune_extern");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "extern/clock.ts",
        "import * as time from \"std/time\";\n\nexport function stamp(): string {\n  return String(time.now());\n}\n",
    );
    write_file(
        &src,
        "extern/measure.ts",
        "import * as math from \"../.glyph-runtime/std/math\";\n\nexport function half(n: number): string {\n  return String(math.floor(n / 2));\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main

import extern/clock { stamp }
import extern/measure { half }

fn main() {
  print(stamp())
  print(half(10))
}
"#,
    );

    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);

    for rel in [
        // Reached only through the bare `std/time` spelling.
        ".glyph-runtime/std/time.ts",
        // Reached only through the relative `../.glyph-runtime/std/math` spelling.
        ".glyph-runtime/std/math.ts",
        // `time.ts` imports it, so the closure has to run over extern hits too.
        ".glyph-runtime/std/option.ts",
        ".glyph-runtime/std/result.ts",
    ] {
        assert!(
            out.join(rel).is_file(),
            "{rel} pruned even though an extern shim reaches it"
        );
    }

    // Pruning still prunes: nothing here is reachable from the program or its
    // shims, and a fix that simply stopped pruning would pass the block above.
    for rel in [
        ".glyph-runtime/std/dns.ts",
        ".glyph-runtime/std/http.ts",
        ".glyph-runtime/std/sqlite.ts",
        ".glyph-runtime/std/tls.ts",
        ".glyph-runtime/std/schema.ts",
    ] {
        assert!(
            !out.join(rel).is_file(),
            "{rel} written despite nothing reaching it"
        );
    }

    if !tsc_available() {
        eprintln!("skipping extern-prune tsc check: tsc not available");
        return;
    }
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("a shim reaching std failed tsc:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
fn http_response_text_accessor_returns_the_exact_bytes_the_server_sent() {
    // G118: `Response.body` is `unknown` and already best-effort JSON-parsed by
    // the client before a Glyph program ever sees it. A caller who wanted the
    // body as text had no accessor and reached for `string.from(response.body)`,
    // which compiles clean, passes `tsc --strict`, and prints the literal
    // `[object Object]` for a JSON endpoint with no diagnostic anywhere in the
    // pipeline. `http.to_text` reads `Response.raw` instead, so both endpoints
    // hand back exactly what was sent over the socket. Needs node/tsx.
    if !js_toolchain_available() {
        eprintln!("skipping http response-text accessor run: node/tsx not available");
        return;
    }
    let root = unique_tmp("httptextaccessor");
    let src = root.join("src");
    let port = free_port();
    write_file(
        &src,
        "main.glyph",
        &format!(
            r#"module main

import std/http
import std/io
import std/net
import std/result {{ Result, Ok, Err }}

pub async fn main(argv: Array<string>) -> number {{
  let started = await http.listen("127.0.0.1", {port}, handler)
  return match started {{
    Err(e) => {{
      io.eprintln("listen failed: ${{e.message}}")
      1
    }},
    Ok(srv) => {{
      let plain = await http.get("http://127.0.0.1:{port}/plain")
      match plain {{
        Err(e) => io.eprintln("plain get failed: ${{e.message}}"),
        Ok(response) => match http.to_text(response) {{
          Ok(t) => io.println("plain=Ok(${{t}})"),
          Err(e) => io.println("plain=Err(${{e}})"),
        }},
      }}
      let api = await http.get("http://127.0.0.1:{port}/api")
      match api {{
        Err(e) => io.eprintln("api get failed: ${{e.message}}"),
        Ok(response) => match http.to_text(response) {{
          Ok(t) => io.println("api=Ok(${{t}})"),
          Err(e) => io.println("api=Err(${{e}})"),
        }},
      }}
      net.stop(srv)
      0
    }},
  }}
}}

fn handler(req: http.Request) -> Result<http.Response, string> {{
  return match http.path(req) {{
    "/plain" => Ok(http.text(200, "plain body")),
    "/api" => Ok(http.json(200, {{ hello: "world" }})),
    else => Ok(http.text(404, "not found")),
  }}
}}
"#
        ),
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("plain=Ok(plain body)"),
        "a text/plain response must round-trip exactly: {stdout}"
    );
    // `to_text` hands back the exact bytes the server sent, so a JSON body
    // returns its JSON text rather than an error: the server did send text.
    // What it must never do is what `string.from(response.body)` did, which is
    // render the parsed object as `[object Object]` and report nothing.
    assert!(
        stdout.contains("api=Ok({"),
        "a JSON body comes back as its exact text: {stdout}"
    );
    assert!(
        !stdout.contains("[object Object]"),
        "the documented workaround's failure mode must be gone: {stdout}"
    );
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
  let rest = tail(words)
  io.println(rest[0])
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "imported descriptor should accept the good value and reject the bad one \
                 (2 = good wrongly rejected, 3 = bad wrongly accepted)"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsc");
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
/// down at `return`, the exit code is 0 but the write never lands. It is
/// started with `task.detach`, the one spelling of "start this and do not
/// wait" since a bare un-awaited call became E0238 (D51).
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
             import std/task\n\
             import std/time\n\
             async fn later() {{\n\
             \x20 await time.sleep(time.Duration.ms(50))\n\
             \x20 let _ = fs.write_text(\"{marker_lit}\", \"landed\")\n\
             }}\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 task.detach(async fn() -> void {{ await later() }})\n\
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
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsc");
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
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "cross-module `is` should accept the good value and reject the bad \
                 element (2 = good wrongly rejected, 3 = bad wrongly accepted)"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => tool_missing("tsx"),
        glyph_cli::run::RunOutcome::TscMissing => tool_missing("tsc"),
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
        ts.contains("Box.__is("),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
    write_file(
        &root,
        "package.json",
        "{\n  \"name\": \"proj\",\n  \"version\": \"1.0.0\"\n}\n",
    );
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
        TscOutcome::Failed(msg) => {
            panic!("@types companion did not resolve over the typeless js:\n{msg}")
        }
        TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "sanitized taint program: {:?}",
        report.diagnostics
    );
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("sanitized path should pass tsc:\n{msg}"),
        TscOutcome::NotFound => {
            tool_missing("tsc");
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "where-refinement had {code} wrong boundary outcome(s)"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsx or tsc");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "boundary parse gave {code} wrong answer(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsx or tsc");
        }
        other => panic!("boundary-issue program did not run: {other:?}"),
    }
}

/// G369 / D50: the program behind [`checks_refuse_a_value_past_the_depth_limit`].
/// Each `edge` line is one path or shape: the deepest value the limit admits
/// passes, and the first value past it and one twenty times deeper are
/// refused with the same answer. `main` returns the count of wrong answers.
const DEPTH_LIMIT_MAIN: &str = r#"module main

import std/io
import std/json
import std/string
import nodes { SNode, named }
import nodes

type Tree = { n: int, kids: Array<Tree>, }
type Shape = | Leaf(int) | Wrap(Shape)
type List = { v: int, next: Option<List>, }
type NList = { v: int, next: Nullable<NList>, }
type Gen<T> = { v: T, kids: Array<Gen<T>>, }
type M = { m: Record<string, M>, }
type K = "k"
type MK = { m: Record<K, Array<MK>>, }
type PosInt = int where value > 0
type Pos = { n: PosInt, kids: Array<Pos>, }
type RKids = Array<RT> where value.length < 3
type RT = { kids: RKids, }
type IR = { inner: { k: Array<IR>, }, }
type ViaNs = { t: nodes.NNode, }

fn rep(open: string, leaf: string, close: string, n: int) -> string {
  return string.repeat(open, n) + leaf + string.repeat(close, n)
}

// Each generator nests `n` levels around a leaf; the comment is the JSON
// depth of the deepest object or array it writes.
fn tree(n: int) -> string { return rep("{\"n\":1,\"kids\":[", "{\"n\":1,\"kids\":[]}", "]}", n) }          // 2n + 2
fn shape(n: int) -> string { return rep("{\"tag\":\"Wrap\",\"value\":", "{\"tag\":\"Leaf\",\"value\":1}", "}", n) } // n + 1
fn list(n: int) -> string { return rep("{\"v\":1,\"next\":{\"tag\":\"Some\",\"value\":", "{\"v\":1,\"next\":{\"tag\":\"None\"}}", "}}", n) } // 2n + 2
fn nlist(n: int) -> string { return rep("{\"v\":1,\"next\":", "{\"v\":1,\"next\":null}", "}", n) }    // n + 1
fn gen(n: int) -> string { return rep("{\"v\":1,\"kids\":[", "{\"v\":1,\"kids\":[]}", "]}", n) }       // 2n + 2
fn ms(n: int) -> string { return rep("{\"m\":{\"k\":", "{\"m\":{}}", "}}", n) }                         // 2n + 2
fn mk(n: int) -> string { return rep("{\"m\":{\"k\":[", "{\"m\":{\"k\":[]}}", "]}}", n) }               // 3n + 3
fn rt(n: int) -> string { return rep("{\"kids\":[", "{\"kids\":[]}", "]}", n) }                         // 2n + 2
fn ir(n: int) -> string { return rep("{\"inner\":{\"k\":[", "{\"inner\":{\"k\":[]}}", "]}}", n) }      // 3n + 3

fn raw(t: string) -> unknown {
  return match json.parse(t) {
    Ok(x) => x,
    Err(_) => "unparsed",
  }
}

fn show<T>(r: Result<T, Array<Issue>>) -> string {
  return match r {
    Ok(_) => "ok",
    Err(issues) => json.stringify(issues),
  }
}

fn bit(b: bool) -> string {
  return match b {
    true => "ok",
    false => "no",
  }
}

fn ist(v: unknown) -> bool { return match v { is Tree => true, _ => false, } }
fn isa(v: unknown) -> bool { return match v { is Array<Tree> => true, _ => false, } }
fn isr(v: unknown) -> bool { return match v { is Record<string, Tree> => true, _ => false, } }
fn isg(v: unknown) -> bool { return match v { is Gen<int> => true, _ => false, } }
fn iss(v: unknown) -> bool { return match v { is Shape => true, _ => false, } }
fn issib(v: unknown) -> bool { return match v { is SNode => true, _ => false, } }

fn depth_at(path: string) -> string {
  return "[{\"path\":" + path + ",\"message\":\"nested more than 1000 objects and arrays deep\",\"code\":\"depth\"}]"
}

// One wrong answer per call that does not see `want`.
fn expect(name: string, got: string, want: string) -> number {
  return match got == want {
    true => 0,
    false => {
      io.eprintln(name + ": got " + got + ", want " + want)
      return 1
    },
  }
}

// The deepest value the limit admits passes; the first deeper one, and one
// twenty times deeper, are refused with the same answer.
fn edge(name: string, at: string, past: string, far: string, refused: string) -> number {
  return expect(name + " at the limit", at, "ok") + expect(name + " past it", past, refused) + expect(name + " far past it", far, refused)
}

fn cyclic() -> unknown {
  let kids: Array<unknown> = []
  let t: unknown = { n: 1, kids: kids, }
  mut kids.push(t)
  return t
}

fn main() -> number {
  let no = "no"
  let f = 0
  mut f = f + edge("is Tree", bit(ist(raw(tree(499)))), bit(ist(raw(tree(500)))), bit(ist(raw(tree(10000)))), no)
  mut f = f + edge("is Array<Tree>", bit(isa(raw("[" + tree(498) + "]"))), bit(isa(raw("[" + tree(499) + "]"))), bit(isa(raw("[" + tree(10000) + "]"))), no)
  mut f = f + edge("is Record<string, Tree>", bit(isr(raw("{\"a\":" + tree(498) + "}"))), bit(isr(raw("{\"a\":" + tree(499) + "}"))), bit(isr(raw("{\"a\":" + tree(10000) + "}"))), no)
  mut f = f + edge("is Gen<int>", bit(isg(raw(gen(499)))), bit(isg(raw(gen(500)))), bit(isg(raw(gen(10000)))), no)
  mut f = f + edge("is Shape", bit(iss(raw(shape(999)))), bit(iss(raw(shape(1000)))), bit(iss(raw(shape(20000)))), no)
  mut f = f + edge("is SNode", bit(issib(raw(tree(499)))), bit(issib(raw(tree(500)))), bit(issib(raw(tree(10000)))), no)
  mut f = f + edge("Tree.is", bit(Tree.is(raw(tree(499)))), bit(Tree.is(raw(tree(500)))), bit(Tree.is(raw(tree(10000)))), no)
  mut f = f + edge("Shape.is", bit(Shape.is(raw(shape(999)))), bit(Shape.is(raw(shape(1000)))), bit(Shape.is(raw(shape(20000)))), no)

  let kids = depth_at("[\"kids\"]")
  let root = depth_at("[]")
  mut f = f + edge("Tree.parse", show(Tree.parse(raw(tree(499)))), show(Tree.parse(raw(tree(500)))), show(Tree.parse(raw(tree(10000)))), kids)
  mut f = f + edge("Tree.schema.parse", show(Tree.schema.parse(raw(tree(499)))), show(Tree.schema.parse(raw(tree(500)))), show(Tree.schema.parse(raw(tree(10000)))), kids)
  mut f = f + edge("Tree.schema.array().parse", show(Tree.schema.array().parse(raw("[" + tree(498) + "]"))), show(Tree.schema.array().parse(raw("[" + tree(499) + "]"))), show(Tree.schema.array().parse(raw("[" + tree(10000) + "]"))), depth_at("[0,\"kids\"]"))
  mut f = f + edge("json.parse<Tree>", show(json.parse<Tree>(tree(499))), show(json.parse<Tree>(tree(500))), show(json.parse<Tree>(tree(10000))), kids)
  mut f = f + edge("json.parse_with", show(json.parse_with(tree(499), Tree.schema)), show(json.parse_with(tree(500), Tree.schema)), show(json.parse_with(tree(10000), Tree.schema)), kids)
  mut f = f + edge("parse<SNode>", named(tree(499)), named(tree(500)), named(tree(10000)), "1|depth|1")
  mut f = f + edge("json.parse<Array<Tree>>", show(json.parse<Array<Tree>>("[" + tree(498) + "]")), show(json.parse<Array<Tree>>("[" + tree(499) + "]")), show(json.parse<Array<Tree>>("[" + tree(10000) + "]")), depth_at("[0,\"kids\"]"))
  mut f = f + edge("json.parse<Record<string, Tree>>", show(json.parse<Record<string, Tree>>("{\"a\":" + tree(498) + "}")), show(json.parse<Record<string, Tree>>("{\"a\":" + tree(499) + "}")), show(json.parse<Record<string, Tree>>("{\"a\":" + tree(10000) + "}")), root)
  mut f = f + edge("json.parse<Option<Tree>>", show(json.parse<Option<Tree>>("{\"tag\":\"Some\",\"value\":" + tree(498) + "}")), show(json.parse<Option<Tree>>("{\"tag\":\"Some\",\"value\":" + tree(499) + "}")), show(json.parse<Option<Tree>>("{\"tag\":\"Some\",\"value\":" + tree(10000) + "}")), root)
  mut f = f + edge("Shape.parse", show(Shape.parse(raw(shape(999)))), show(Shape.parse(raw(shape(1000)))), show(Shape.parse(raw(shape(20000)))), root)
  mut f = f + edge("json.parse<Shape>", show(json.parse<Shape>(shape(999))), show(json.parse<Shape>(shape(1000))), show(json.parse<Shape>(shape(20000))), root)
  mut f = f + edge("json.parse<List>", show(json.parse<List>(list(499))), show(json.parse<List>(list(500))), show(json.parse<List>(list(10000))), depth_at("[\"next\"]"))
  mut f = f + edge("json.parse<NList>", show(json.parse<NList>(nlist(999))), show(json.parse<NList>(nlist(1000))), show(json.parse<NList>(nlist(20000))), depth_at("[\"next\"]"))
  mut f = f + edge("json.parse<Gen<int>>", show(json.parse<Gen<int>>(gen(499))), show(json.parse<Gen<int>>(gen(500))), show(json.parse<Gen<int>>(gen(10000))), root)
  mut f = f + edge("Gen.parse<int>", show(Gen.parse<int>(raw(gen(499)))), show(Gen.parse<int>(raw(gen(500)))), show(Gen.parse<int>(raw(gen(10000)))), kids)
  mut f = f + edge("json.parse<M>", show(json.parse<M>(ms(499))), show(json.parse<M>(ms(500))), show(json.parse<M>(ms(10000))), depth_at("[\"m\"]"))
  mut f = f + edge("json.parse<MK>", show(json.parse<MK>(mk(332))), show(json.parse<MK>(mk(333))), show(json.parse<MK>(mk(7000))), depth_at("[\"m\"]"))
  mut f = f + edge("json.parse<Pos>", show(json.parse<Pos>(tree(499))), show(json.parse<Pos>(tree(500))), show(json.parse<Pos>(tree(10000))), kids)
  mut f = f + edge("json.parse<RT>", show(json.parse<RT>(rt(499))), show(json.parse<RT>(rt(500))), show(json.parse<RT>(rt(10000))), kids)
  mut f = f + edge("json.parse<IR>", show(json.parse<IR>(ir(332))), show(json.parse<IR>(ir(333))), show(json.parse<IR>(ir(7000))), depth_at("[\"inner\"]"))
  mut f = f + edge("json.parse<SNode>", show(json.parse<SNode>(tree(499))), show(json.parse<SNode>(tree(500))), show(json.parse<SNode>(tree(10000))), kids)
  mut f = f + edge("json.parse<ViaNs>", show(json.parse<ViaNs>("{\"t\":" + tree(498) + "}")), show(json.parse<ViaNs>("{\"t\":" + tree(499) + "}")), show(json.parse<ViaNs>("{\"t\":" + tree(10000) + "}")), depth_at("[\"t\",\"kids\"]"))

  // A cyclic value is infinitely deep: refused the same way, never a throw.
  let c = cyclic()
  mut f = f + expect("is Tree on a cycle", bit(ist(c)), no)
  mut f = f + expect("Tree.is on a cycle", bit(Tree.is(c)), no)
  mut f = f + expect("Tree.parse on a cycle", show(Tree.parse(c)), kids)
  return f
}
"#;

const DEPTH_LIMIT_NODES: &str = r#"module nodes

import std/json { parse }

pub type SNode = {
  n: int,
  kids: Array<SNode>,
}

pub type NNode = {
  n: int,
  kids: Array<NNode>,
}

pub fn named(text: string) -> string {
  return match parse<SNode>(text) {
    Ok(_) => "ok",
    Err(issues) => json_text(issues),
  }
}

fn json_text(issues: Array<Issue>) -> string {
  let first = issues[0]
  return "${issues.length}|${first.code}|${first.path.length}"
}
"#;

#[test]
fn checks_refuse_a_value_past_the_depth_limit() {
    // G369 / D50. A check refuses a value nested more than `MAX_DEPTH` (1000)
    // objects and arrays deep: `is` answers false, a `parse` answers one
    // `code: "depth"` issue at the path a type issue would carry. Before, every
    // check recursed once per level and threw `RangeError` a few thousand
    // levels down. Every check path and descriptor kind is driven exactly at
    // the limit and past it, under `glyph run` (tsx), under tsx and plain node
    // at half node's default stack, so the headroom is gated rather than
    // assumed, and under plain node. Needs node, tsx and tsc.
    if !js_toolchain_available() || !tsc_available() {
        eprintln!("skipping depth-limit run: node/tsx/tsc not available");
        return;
    }
    let root = unique_tmp("depth_limit");
    write_file(&root, "package.json", "{\"name\":\"depth\",\"version\":\"0.0.0\",\"type\":\"module\",\"glyph\":{}}\n");
    write_file(&root, "src/main.glyph", DEPTH_LIMIT_MAIN);
    write_file(&root, "src/nodes.glyph", DEPTH_LIMIT_NODES);

    match glyph_cli::run::run_file(&root.join("src/main.glyph"), &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "glyph run: {code} wrong answer(s) at the depth limit");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsx or tsc");
            return;
        }
        other => panic!("depth-limit program did not run: {other:?}"),
    }

    let out = root.join("out");
    let report = build_project(&root.join("src"), &out).expect("build_project ok");
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    write_file(
        &out,
        "__run.ts",
        "import \"./.glyph-runtime/glyph-bootstrap.ts\";\nimport { main } from \"./main.ts\";\nprocess.exitCode = main();\n",
    );
    let tsx = std::process::Command::new("tsx")
        .arg("--stack-size=492")
        .arg("--tsconfig")
        .arg(out.join("tsconfig.json"))
        .arg(out.join("__run.ts"))
        .output()
        .expect("run tsx");
    assert_eq!(
        tsx.status.code(),
        Some(0),
        "tsx --stack-size=492: the exit code is the wrong-answer count:\n{}",
        String::from_utf8_lossy(&tsx.stderr)
    );

    // Plain node runs the CommonJS that `tsc` emits from the same TypeScript.
    std::fs::remove_file(out.join("__run.ts")).expect("remove tsx entry");
    let js = root.join("js");
    let tsc = std::process::Command::new("tsc")
        .current_dir(&out)
        .args(["-p", "tsconfig.json", "--noEmit", "false", "--outDir"])
        .arg(&js)
        .args(["--module", "commonjs", "--moduleResolution", "node10", "--ignoreDeprecations", "6.0"])
        .output()
        .expect("run tsc");
    assert!(tsc.status.success(), "tsc emit: {}", String::from_utf8_lossy(&tsc.stdout));
    write_file(&js, "package.json", "{\"type\":\"commonjs\"}\n");
    write_file(
        &js,
        "__run.js",
        "require(\"./.glyph-runtime/glyph-bootstrap\");\nprocess.exitCode = require(\"./main\").main();\n",
    );
    for stack in [None, Some("--stack-size=492")] {
        let mut node = std::process::Command::new("node");
        node.args(stack).arg(js.join("__run.js"));
        let run = node.output().expect("run node");
        assert_eq!(
            run.status.code(),
            Some(0),
            "node {stack:?}: the exit code is the wrong-answer count:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}

#[test]
fn the_depth_sentinel_cannot_be_reached_from_glyph() {
    // D50, record 122. The helpers that throw and catch the depth sentinel
    // live in the internal runtime module, which the resolver does not seed.
    // When `std/schema` exported `too_deep`, a refined field's predicate could
    // call it, and `json.parse<Doc>` threw the private sentinel out of a
    // public entry (the emitter does not wrap a part it proves can never go
    // too deep). Both import spellings are refused at resolve; `MAX_DEPTH`,
    // a number nothing can forge with, stays importable.
    for (name, main) in [
        (
            "named",
            "module main\nimport std/schema { too_deep }\npub fn f() -> number { return too_deep() }\n",
        ),
        (
            "namespaced",
            "module main\nimport std/json\nimport std/schema\n\
             type Name = string where value.length > limit()\n\
             type Doc = { name: Name, }\n\
             fn limit() -> number { return schema.too_deep() }\n\
             pub fn f() -> bool { return match json.parse<Doc>(\"{}\") { Ok(_) => true, Err(_) => false, } }\n",
        ),
    ] {
        let root = unique_tmp(&format!("sentinel_{name}"));
        let src = root.join("src");
        write_file(&src, "main.glyph", main);
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.contains("E0105") && d.contains("too_deep")),
            "{name}: `too_deep` must not resolve: {:?}",
            report.diagnostics
        );
    }
    let root = unique_tmp("sentinel_limit");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/schema { MAX_DEPTH }\npub fn f() -> number { return MAX_DEPTH }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0105")),
        "`MAX_DEPTH` is the public contract: {:?}",
        report.diagnostics
    );
}

#[test]
fn depth_helpers_resolve_in_a_host_project_without_the_generated_tsconfig() {
    // G122's case for the internal runtime module (D50): emitted code imports
    // the depth helpers by a relative specifier, so a host project that
    // compiles the generated files under its own strict tsconfig, with no
    // `paths`, resolves them, and the module carries its own reference to the
    // prelude for `Issue`.
    if !tsc_available() {
        eprintln!("skipping host-project depth run: tsc not available");
        return;
    }
    let root = unique_tmp("depth_host");
    let src = root.join("src");
    let out = root.join("out");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/json\n\
         pub type Name = string where value.length > 0\n\
         pub type Tree = { n: Name, kids: Array<Tree>, }\n\
         pub fn read(text: string) -> bool { return match json.parse<Tree>(text) { Ok(_) => true, Err(_) => false, } }\n\
         pub fn shape(v: unknown) -> bool { return match v { is Array<Tree> => true, _ => false, } }\n",
    );
    let report = build_project(&src, &out).expect("build_project ok");
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    let emitted = std::fs::read_to_string(out.join("main.ts")).expect("main.ts");
    assert!(emitted.contains("from \"./.glyph-runtime/internal/depth\";"), "{emitted}");

    let host = root.join("host");
    let gen = host.join("gen");
    std::fs::create_dir_all(&gen).expect("mkdir gen");
    std::fs::copy(out.join("main.ts"), gen.join("main.ts")).expect("copy main.ts");
    copy_dir(&out.join(".glyph-runtime"), &gen.join(".glyph-runtime"));
    write_file(
        &host,
        "tsconfig.json",
        "{\"compilerOptions\":{\"strict\":true,\"noEmit\":true,\"target\":\"es2022\",\"module\":\"esnext\",\
         \"moduleResolution\":\"bundler\",\"lib\":[\"es2022\",\"dom\"],\"skipLibCheck\":true,\"types\":[]},\
         \"include\":[\"gen/*.ts\"]}\n",
    );
    let tsc = std::process::Command::new("tsc")
        .current_dir(&host)
        .args(["-p", "tsconfig.json"])
        .output()
        .expect("run tsc");
    assert!(
        tsc.status.success(),
        "host tsc --strict: {}",
        String::from_utf8_lossy(&tsc.stdout)
    );
}

#[test]
fn a_match_on_issue_code_needs_a_catch_all_before_and_after_depth() {
    // D50 added `"depth"` to `Issue.code` in the TypeScript prelude. Glyph's
    // checker types `code` as `string`, so a match listing the codes was
    // already E0218 without a catch-all and adding a fifth code refuses no
    // program that compiled. Pinned so a change to how Glyph types `code`
    // is a decision, not a side effect.
    let root = unique_tmp("issue_code_match");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main
         pub fn label(i: Issue) -> string {
         \x20 return match i.code {
         \x20\x20\x20 \"missing\" => \"m\",
         \x20\x20\x20 \"type\" => \"t\",
         \x20\x20\x20 \"refinement\" => \"r\",
         \x20\x20\x20 \"unexpected\" => \"u\",
         \x20\x20\x20 \"depth\" => \"d\",
         \x20 }
         }
",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0218")),
        "a match on `Issue.code` with no catch-all: {:?}",
        report.diagnostics
    );
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "bigint produced {code} wrong result(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsx or tsc");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "parse_iso had {code} wrong outcome(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsx or tsc");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "std/decimal produced {code} wrong result(s)");
        }
        glyph_cli::run::RunOutcome::TsxNotFound | glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsx or tsc");
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
                msg.contains("age")
                    || msg.contains('y')
                    || msg.to_lowercase().contains("assignable"),
                "expected a shape/type mismatch, got:\n{msg}"
            );
        }
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::Failed(msg) => {
            panic!("the D28 boundary cast did not reach the returns a `match` lowers to:\n{msg}")
        }
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::Failed(msg) => {
            panic!("infer_output failed to unwrap a validator NOT named `Schema` (`Codec`):\n{msg}")
        }
        TscOutcome::NotFound => tool_missing("tsc"),
    }
}

/// A keyword may be a type path segment after a dot, as it may be a member
/// after a dot: `kinds.type` is a type as `kinds.make()` is a value. The
/// emitted TypeScript keeps the qualified name, and tsc accepts a reserved word
/// on the right of a dot in one (a qualified name's right side is an
/// IdentifierName). This pins that against a real `.d.ts` whose export is named
/// `type`, the spelling a Glyph import item cannot take. A wrong value is
/// rejected through the same name, so the type was loaded and not `any`.
#[test]
fn a_keyword_type_path_segment_type_checks_through_tsc() {
    if !tsc_available() {
        eprintln!("skipping keyword type path segment check: tsc not available");
        return;
    }
    let root = unique_tmp("kw_type_segment");
    let src = root.join("src");
    write_file(
        &src,
        "node_modules/kinds/package.json",
        r#"{ "name": "kinds", "version": "1.0.0", "types": "index.d.ts" }"#,
    );
    write_file(
        &src,
        "node_modules/kinds/index.d.ts",
        "export type type = \"a\" | \"b\";\nexport declare function make(): type;\nexport declare function take(k: type): string;\n",
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main

import kinds

fn pick() -> kinds.type {
  return kinds.make()
}

fn main(argv: Array<string>) -> number {
  print(kinds.take(pick()))
  return 0
}
"#,
    );

    let out = root.join("dist-good");
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "`kinds.type` is a type: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).expect("emitted main.ts");
    assert!(ts.contains("kinds.type"), "the qualified name is emitted as written:\n{ts}");

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("tsc refused `kinds.type` in type position:\n{msg}"),
        TscOutcome::NotFound => panic!("tsc vanished mid-test"),
    }

    // A value outside the union, returned as `kinds.type`: tsc rejects it
    // naming the declared type, so the qualified name reached the `.d.ts`'s
    // `type` and did not fall back to `any`.
    write_file(
        &src,
        "main.glyph",
        r#"module main

import kinds

fn pick() -> kinds.type {
  return "c"
}

fn main(argv: Array<string>) -> number {
  print(kinds.take(pick()))
  return 0
}
"#,
    );
    let out = root.join("dist-bad");
    let report = build_project_inner(&src, &out, false).expect("build runs");
    assert!(!report.has_errors(), "the checker leaves the npm type to tsc: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Failed(msg) => assert!(
            msg.contains("is not assignable to type 'type'"),
            "tsc names the declared type:\n{msg}"
        ),
        other => panic!("`\"c\"` as `kinds.type` must be refused by tsc, got {other:?}"),
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
    assert!(
        !report.has_errors(),
        "diagnostics: {:?}",
        report.diagnostics
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => {
            panic!("bare node builtins did not type-check with the bundled shim:\n{msg}")
        }
        TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "diagnostics: {:?}",
        report.diagnostics
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => {
            panic!("Buffer byte boundary did not type-check with the bundled shim:\n{msg}")
        }
        TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "diagnostics: {:?}",
        report.diagnostics
    );

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => {
            panic!("streaming child_process.spawn did not type-check with the bundled shim:\n{msg}")
        }
        TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "diagnostics: {:?}",
        report.diagnostics
    );
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => {
            panic!("the named ChildProcess surface did not type-check:\n{msg}")
        }
        TscOutcome::NotFound => {
            tool_missing("tsc");
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
        assert!(
            !report.has_errors(),
            "diagnostics: {:?}",
            report.diagnostics
        );
        match check_with_tsc(&out).expect("run tsc") {
            TscOutcome::Passed => panic!("{label} was accepted by tsc"),
            TscOutcome::Failed(_) => {}
            TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "diagnostics: {:?}",
        report.diagnostics
    );
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {
            panic!("the shim exports `Signals` from `child_process`; `@types/node` does not")
        }
        TscOutcome::Failed(msg) => assert!(
            msg.contains("Signals"),
            "expected the rejection to name `Signals`, got:\n{msg}"
        ),
        TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "diagnostics: {:?}",
        report.diagnostics
    );
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("`NodeJS.Signals` did not type-check:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
    }
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
    assert!(
        report.build_failed.is_none(),
        "augmented build should compile"
    );
    assert_eq!(report.total, 3, "three @example lines");
    assert_eq!(
        report.failures.len(),
        1,
        "exactly the `add(1,1) == 3` example fails: {:?}",
        report.failures
    );
    assert!(
        report.failures[0].detail.contains("add(1, 1)"),
        "failure should name the bad example: {:?}",
        report.failures
    );
    assert_eq!(
        report.failures[0].entity.as_deref(),
        Some("calc::add"),
        "the failure carries the declaration it is about: {:?}",
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
    assert!(
        report.build_failed.is_none(),
        "should compile: {:?}",
        report.build_failed
    );
    assert_eq!(report.total, 2, "two property @examples");
    assert_eq!(
        report.failures.len(),
        1,
        "the `n > 0` property should fail (ints() yields 0 and negatives): {:?}",
        report.failures
    );
}

/// A project with a `package.json` "glyph" key and one installed npm package,
/// `tinypad`, built by hand in `node_modules` so no network is needed. `main`
/// imports it and carries `@example double(21) == <rhs>`; `index_js` is the
/// package's runtime body. Returns the project root.
fn npm_example_project(prefix: &str, rhs: &str, index_js: &str) -> PathBuf {
    let root = unique_tmp(prefix);
    write_file(
        &root,
        "package.json",
        "{ \"name\": \"g280\", \"version\": \"0.1.0\", \"private\": true, \
         \"glyph\": { \"src\": \"src\" }, \"dependencies\": { \"tinypad\": \"1.0.0\" } }\n",
    );
    write_file(
        &root,
        "node_modules/tinypad/package.json",
        "{ \"name\": \"tinypad\", \"version\": \"1.0.0\", \"main\": \"index.js\", \
         \"types\": \"index.d.ts\" }\n",
    );
    write_file(&root, "node_modules/tinypad/index.js", index_js);
    write_file(
        &root,
        "node_modules/tinypad/index.d.ts",
        "export declare function pad(s: string, n: number): string;\n",
    );
    write_file(
        &root,
        "src/main.glyph",
        &format!(
            "module main\n\
             \n\
             import tinypad {{ pad }}\n\
             \n\
             @example double(21) == {rhs}\n\
             pub fn double(n: number) -> number {{\n\
             \x20 return n * 2\n\
             }}\n\
             \n\
             @example padded(\"a\") == \"   a\"\n\
             pub fn padded(s: string) -> string {{\n\
             \x20 return pad(s, 4)\n\
             }}\n"
        ),
    );
    root
}

const TINYPAD_JS: &str = "exports.pad = function (s, n) { return s.padStart(n, \" \"); };\n";

/// Run the real binary on the project at `root` and return (exit code, stdout,
/// stderr). `{src}` and `{out}` in `args` become `root/src` and `root/out`.
/// Bounded by the suite's deadline through `spawn_glyph`.
fn glyph_in(root: &Path, args: &[&str]) -> (i32, String, String) {
    let src = root.join("src");
    let out = root.join("out");
    let owned: Vec<std::ffi::OsString> = args
        .iter()
        .map(|a| match *a {
            "{src}" => src.clone().into_os_string(),
            "{out}" => out.clone().into_os_string(),
            other => other.into(),
        })
        .collect();
    let refs: Vec<&std::ffi::OsStr> = owned.iter().map(|a| a.as_os_str()).collect();
    let (code, stdout, stderr, _) = spawn_glyph(&refs);
    (code, stdout, stderr)
}

#[test]
fn a_failing_example_fails_in_a_module_that_imports_an_npm_package() {
    // G280: the gate builds a copy of the project in the temp directory, where
    // no `node_modules` is reachable. The harness died loading `tinypad`,
    // printed no `FAIL` line, and the runner read the silence as "1 example(s)
    // passed", exit 0, on an example that is plainly false.
    if !js_toolchain_available() {
        eprintln!("skipping npm-import example gate: node/tsx not available");
        return;
    }
    let root = npm_example_project("g280_fail", "43", TINYPAD_JS);

    let (code, _, stderr) = glyph_in(&root, &["build", "{src}", "--out", "{out}", "--no-tsc"]);
    assert_eq!(code, 1, "a false example fails the build:\n{stderr}");
    assert!(
        stderr.contains("example failed: main::double example #1: (double(21)) != (43)"),
        "the failure names the example:\n{stderr}"
    );
    assert!(
        stderr.contains("1 of 2 example(s) failed"),
        "the example that calls into the package ran and passed:\n{stderr}"
    );

    let (code, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--json", "{src}"]);
    assert_eq!(code, 1, "--json exits 1 on the same source:\n{stdout}\n{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("check --json prints JSON");
    assert_eq!(v["ok"], false, "{stdout}");
    let ex = &v["examples"];
    assert_eq!(ex["ran"], true, "{stdout}");
    assert_eq!(ex["total"], 2, "{stdout}");
    let failures = ex["failures"].as_array().expect("failures array");
    assert_eq!(failures.len(), 1, "{stdout}");
    assert_eq!(failures[0]["code"], "E0400", "{stdout}");
    assert_eq!(failures[0]["entity"], "main::double", "{stdout}");
}

#[test]
fn a_passing_example_passes_in_a_module_that_imports_an_npm_package() {
    // The other half of G280: the copy resolves the project's packages, so an
    // example that calls through `tinypad` runs and is checked, rather than
    // only being reported loudly as not run.
    if !js_toolchain_available() {
        eprintln!("skipping npm-import example gate: node/tsx not available");
        return;
    }
    let root = npm_example_project("g280_pass", "42", TINYPAD_JS);
    let (code, _, stderr) = glyph_in(&root, &["build", "{src}", "--out", "{out}", "--no-tsc"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("2 example(s) passed"), "{stderr}");
}

#[test]
fn an_example_harness_that_stops_early_is_never_a_pass() {
    // Whatever stops the harness before its tally (here, a package that
    // throws on load), the examples it never reached are reported as not run
    // and fail the gate. Silence from the harness is not a verdict.
    if !js_toolchain_available() {
        eprintln!("skipping example-harness crash gate: node/tsx not available");
        return;
    }
    let root = npm_example_project(
        "g280_crash",
        "42",
        "throw new Error(\"tinypad refuses to load\");\n",
    );

    let (code, _, stderr) = glyph_in(&root, &["build", "{src}", "--out", "{out}", "--no-tsc"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("2 example(s) not run"), "{stderr}");
    assert!(stderr.contains("tinypad refuses to load"), "the cause is shown:\n{stderr}");
    assert!(!stderr.contains("passed"), "nothing is reported as passed:\n{stderr}");

    let (code, _, stderr) = glyph_in(&root, &["check", "--no-tsc", "{src}"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("2 example(s) not run"), "{stderr}");

    let (code, stdout, _) = glyph_in(&root, &["check", "--no-tsc", "--json", "{src}"]);
    assert_eq!(code, 1, "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("check --json prints JSON");
    assert_eq!(v["ok"], false, "{stdout}");
    assert_eq!(v["examples"]["ran"], false, "{stdout}");
    let failures = v["examples"]["failures"].as_array().expect("failures array");
    assert_eq!(failures.len(), 1, "{stdout}");
    assert_eq!(failures[0]["code"], "E0404", "{stdout}");
    assert_eq!(failures[0]["kind"], "gate", "{stdout}");

    let report = glyph_cli::examples::run_examples(&root.join("src")).expect("run_examples ok");
    assert!(!report.ok(), "the library report is not ok either");
    assert!(report.incomplete.is_some(), "{report:?}");
}

/// `npm_example_project`'s package and manifest, with an entry point that
/// prints a value computed through `tinypad`, so a run that got past the gate
/// shows on stdout. `tests` is placed above `padded`: an `@example` or a
/// `@doc` carrying a `@run` block.
fn npm_run_project(prefix: &str, tests: &str, index_js: &str) -> PathBuf {
    let root = npm_example_project(prefix, "42", index_js);
    write_file(
        &root,
        "src/main.glyph",
        &format!(
            "module main\n\
             \n\
             import std/io\n\
             import tinypad {{ pad }}\n\
             \n\
             {tests}\n\
             pub fn padded(s: string) -> string {{\n\
             \x20 return pad(s, 4)\n\
             }}\n\
             \n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 io.println(\"ran:\" + padded(\"z\"))\n\
             \x20 return 0\n\
             }}\n"
        ),
    );
    root
}

/// A `@doc` whose one ` ```glyph @run ``` ` block asserts `padded("a")` is
/// `want`.
fn doc_run_asserting(want: &str) -> String {
    format!("@doc \"\"\"\n```glyph @run\nassert(padded(\"a\") == \"{want}\")\n```\n\"\"\"")
}

#[test]
fn glyph_run_stops_on_a_failing_example_in_a_module_that_imports_an_npm_package() {
    // G280 covered `build` and `check` with tests of their own; `run`'s
    // pre-run gate rode the same fix with nothing pinning it. The example is
    // false, so the program must not start and the run must fail.
    if !js_toolchain_available() {
        eprintln!("skipping npm-import run gate: node/tsx not available");
        return;
    }
    let root = npm_run_project("g280_run_fail", "@example padded(\"a\") == \"a\"", TINYPAD_JS);
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 1, "a false example stops `glyph run`:\n{stdout}\n{stderr}");
    assert!(
        stderr.contains("glyph run: example failed: main::padded example #1"),
        "the failure names the example:\n{stderr}"
    );
    assert!(stderr.contains("1 of 1 example(s) failed"), "{stderr}");
    assert!(!stdout.contains("ran:"), "the program never started:\n{stdout}");

    // Control: the same project with a true example runs the program, so the
    // failure above is the example's and not the project's.
    let root = npm_run_project("g280_run_pass", "@example padded(\"a\") == \"   a\"", TINYPAD_JS);
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stderr.contains("1 example(s) passed"), "{stderr}");
    assert!(stdout.contains("ran:   z"), "the program ran after the gate:\n{stdout}");
}

#[test]
fn glyph_run_stops_when_the_example_harness_cannot_load_an_npm_package() {
    // The other G280 failure on `run`: a package that throws on load kills
    // the harness before its tally. That is "not run", never a pass, and the
    // program must not start on the strength of it.
    if !js_toolchain_available() {
        eprintln!("skipping npm-import run gate: node/tsx not available");
        return;
    }
    let root = npm_run_project(
        "g280_run_crash",
        "@example padded(\"a\") == \"   a\"",
        "throw new Error(\"tinypad refuses to load\");\n",
    );
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 1, "{stdout}\n{stderr}");
    assert!(stderr.contains("glyph run: 1 example(s) not run"), "{stderr}");
    assert!(stderr.contains("tinypad refuses to load"), "the cause is shown:\n{stderr}");
    assert!(!stderr.contains("passed"), "nothing is reported as passed:\n{stderr}");
    assert!(!stdout.contains("ran:"), "the program never started:\n{stdout}");
}

#[test]
fn a_failing_doc_run_block_fails_in_a_module_that_imports_an_npm_package() {
    // D26's `@doc @run` blocks go through the same harness as `@example`, so
    // G280 hid a false one the same way. Driven through `run`, whose gate had
    // no npm test, and `build`, so both channels are pinned for `@run`.
    if !js_toolchain_available() {
        eprintln!("skipping npm-import doc-run gate: node/tsx not available");
        return;
    }
    let root = npm_run_project("g280_docrun_fail", &doc_run_asserting("a"), TINYPAD_JS);

    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 1, "a false @doc @run stops `glyph run`:\n{stdout}\n{stderr}");
    assert!(
        stderr.contains("glyph run: example failed: main::padded doc-run #1"),
        "the failing block is named:\n{stderr}"
    );
    assert!(!stdout.contains("ran:"), "the program never started:\n{stdout}");

    let (code, _, stderr) = glyph_in(&root, &["build", "{src}", "--out", "{out}", "--no-tsc"]);
    assert_eq!(code, 1, "a false @doc @run fails the build:\n{stderr}");
    assert!(stderr.contains("main::padded doc-run #1"), "{stderr}");

    let (code, stdout, _) = glyph_in(&root, &["check", "--no-tsc", "--json", "{src}"]);
    assert_eq!(code, 1, "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("check --json prints JSON");
    let failures = v["examples"]["failures"].as_array().expect("failures array");
    assert_eq!(failures.len(), 1, "{stdout}");
    assert_eq!(failures[0]["code"], "E0401", "{stdout}");
    assert_eq!(failures[0]["entity"], "main::padded", "{stdout}");

    // Control: a true block passes on both, and `run` goes on to the program.
    let root = npm_run_project("g280_docrun_pass", &doc_run_asserting("   a"), TINYPAD_JS);
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stderr.contains("1 example(s) passed"), "{stderr}");
    assert!(stdout.contains("ran:   z"), "{stdout}");
    let (code, _, stderr) = glyph_in(&root, &["build", "{src}", "--out", "{out}", "--no-tsc"]);
    assert_eq!(code, 0, "{stderr}");
}

/// `npm_example_project`'s package and manifest, with a module that carries
/// one benchmark calling through `tinypad` instead of examples.
fn npm_bench_project(prefix: &str, index_js: &str) -> PathBuf {
    let root = npm_example_project(prefix, "42", index_js);
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\
         \n\
         import tinypad { pad }\n\
         \n\
         pub fn bench_pad() -> string {\n\
         \x20 return pad(\"a\", 4)\n\
         }\n",
    );
    root
}

#[test]
fn a_bench_whose_module_throws_on_load_fails() {
    // G287: `glyph bench` read only the harness's `BENCH` lines and never its
    // exit status, so a module that threw while loading printed
    // `0 benchmark(s)` and exited 0. A harness that does not account for every
    // benchmark it was given is a failed run.
    if !js_toolchain_available() {
        return;
    }
    let root = npm_bench_project(
        "g287_crash",
        "throw new Error(\"tinypad refuses to load\");\n",
    );
    let (code, stdout, stderr) = glyph_in(&root, &["bench", "{src}"]);
    assert_eq!(code, 1, "{stdout}\n{stderr}");
    assert!(stderr.contains("benchmarks not run"), "{stderr}");
    assert!(
        stderr.contains("none of the 1 benchmark(s) ran to a verdict"),
        "{stderr}"
    );
    assert!(stderr.contains("tinypad refuses to load"), "the cause is shown:\n{stderr}");
    assert!(!stderr.contains("0 benchmark(s)."), "never a count of zero:\n{stderr}");
}

#[test]
fn a_bench_that_throws_when_called_fails_by_name() {
    if !js_toolchain_available() {
        return;
    }
    let root = npm_bench_project(
        "g287_throw",
        "exports.pad = function () { throw new Error(\"pad is broken\"); };\n",
    );
    let (code, stdout, stderr) = glyph_in(&root, &["bench", "{src}"]);
    assert_eq!(code, 1, "{stdout}\n{stderr}");
    assert!(stderr.contains("main.bench_pad threw"), "{stderr}");
    assert!(stderr.contains("pad is broken"), "{stderr}");
    assert!(stderr.contains("1 of 1 benchmark(s) threw."), "{stderr}");
}

#[test]
fn a_bench_that_runs_reports_its_benchmarks() {
    if !js_toolchain_available() {
        return;
    }
    let root = npm_bench_project("g287_ok", TINYPAD_JS);
    let (code, stdout, stderr) = glyph_in(&root, &["bench", "{src}"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("main.bench_pad:"), "{stdout}");
    assert!(stdout.contains("ns/op"), "{stdout}");
    assert!(stderr.contains("glyph bench: 1 benchmark(s)."), "{stderr}");
}

/// A scaffold-shaped project (`package.json` with a `"glyph"` key, the entry
/// at `src/main.glyph`) whose `main` prints its argv joined with `|`, plus a
/// `good.json` beside the manifest. Returns the project root.
fn argv_echo_project(prefix: &str) -> PathBuf {
    let root = unique_tmp(prefix);
    write_file(
        &root,
        "package.json",
        "{ \"name\": \"g282\", \"version\": \"0.1.0\", \"private\": true, \
         \"scripts\": { \"start\": \"glyph run --\" }, \
         \"glyph\": { \"src\": \"src\" } }\n",
    );
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\
         import std/io\n\
         import std/string\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 io.println(\"argv=\" + string.join(argv, \"|\"))\n\
         \x20 return 0\n\
         }\n",
    );
    write_file(&root, "good.json", "{}\n");
    root
}

/// Run the real binary with `root` as its working directory.
fn glyph_at(root: &Path, args: &[&str]) -> (i32, String, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn glyph");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn run_with_a_separator_runs_the_current_project_with_those_args() {
    // G282: clap filled `PATH` from the first value after `--`, so
    // `glyph run -- good.json` tried to run `good.json` and stopped at "not a
    // `.glyph` file". Everything after `--` is the program's.
    if !(js_toolchain_available() && tsc_available()) {
        return;
    }
    let root = argv_echo_project("g282_sep");
    let (code, stdout, stderr) = glyph_at(&root, &["run", "--", "good.json", "--verbose"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("argv=good.json|--verbose"), "{stdout}\n{stderr}");

    // A glyph flag before the separator still binds to glyph, and one after it
    // is the program's.
    let (code, stdout, stderr) =
        glyph_at(&root, &["run", "--no-tsc", "--", "good.json", "--no-tsc"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("argv=good.json|--no-tsc"), "{stdout}\n{stderr}");

    // A path before the separator is still the path.
    let (code, stdout, stderr) = glyph_at(&root, &["run", "--no-tsc", "src", "--", "good.json"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("argv=good.json"), "{stdout}\n{stderr}");

    // Nothing after the separator is an empty argv.
    let (code, stdout, stderr) = glyph_at(&root, &["run", "--no-tsc", "--"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("argv=\n") || stdout.trim_end().ends_with("argv="), "{stdout}");
}

#[test]
fn run_on_a_non_glyph_path_without_a_separator_stays_an_error() {
    // The other side of G282: without `--`, a value in `PATH` position is a
    // path. Reading `good.json` as an argument would turn a mistyped file name
    // into a run of something else, so it is refused, and the refusal says how
    // to pass it to the program instead. No toolchain is needed: the refusal
    // comes before anything is built.
    let root = argv_echo_project("g282_nosep");
    let (code, stdout, stderr) = glyph_at(&root, &["run", "--no-test", "good.json"]);
    assert_eq!(code, 2, "{stdout}\n{stderr}");
    assert!(stderr.contains("not a `.glyph` file: good.json"), "{stderr}");
    assert!(stderr.contains("`glyph run -- good.json`"), "the refusal names the fix:\n{stderr}");
    assert!(!stdout.contains("argv="), "nothing ran:\n{stdout}");

    let (code, _stdout, stderr) = glyph_at(&root, &["run", "--no-test", "missing.json"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("does not exist: missing.json"), "{stderr}");
    assert!(stderr.contains("`glyph run -- missing.json`"), "{stderr}");

    // A missing `.glyph` file is a typo'd program, and gets no such hint.
    let (code, _stdout, stderr) = glyph_at(&root, &["run", "--no-test", "mian.glyph"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(!stderr.contains("glyph run --"), "{stderr}");
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
    assert!(
        report.build_failed.is_none(),
        "augmented build should compile"
    );
    assert_eq!(report.total, 1, "one @run block");
    assert_eq!(
        report.failures.len(),
        1,
        "the block's second assert fails: {:?}",
        report.failures
    );
    assert!(
        report.failures[0].message.contains("doc-run"),
        "{:?}",
        report.failures
    );
    assert_eq!(
        report.failures[0].kind, "doc-run",
        "the kind is structure, not a substring of the message: {:?}",
        report.failures
    );
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 7, "main's return value should be the exit code");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 4, "the code recorded inside `main` is the exit code");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 5, "the return value is the later verdict");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a program that records nothing exits 0");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    assert!(
        !report.has_errors(),
        "extern server is well-formed: {:?}",
        report.diagnostics
    );
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("extern http server failed tsc against the shim:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
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
    assert!(
        !report.has_errors(),
        "extern import is well-formed: {:?}",
        report.diagnostics
    );
    let staged = out.join("extern").join("mathx.ts");
    assert!(staged.exists(), "the extern .ts is staged into the output");

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("extern program failed tsc:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "task.pool broke ordering (1), the limit (2), or completeness (3)"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("task.pool program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "async thunks + task.all produced a wrong result");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("async-closure program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "inline-union program produced a wrong value");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("inline-union program failed to build: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "value-position match with a return arm dispatched wrong"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!(
                "value-position-match program failed to build: {:?}",
                r.diagnostics
            );
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "nested-literal match dispatched a case wrong");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!(
                "nested-literal program failed to build: {:?}",
                r.diagnostics
            );
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
        TscOutcome::NotFound => tool_missing("tsc"),
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
    match glyph_cli::run::run_file(&file_glyph, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "make_dir + two append_text should yield exactly \"a\\nb\\n\""
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "a non-zero code names the failing step in the walk; 11 = `modified` \
                 was not an integer, so the documented `int` is a lie"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsc");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("read_dir/is_dir/stat must type-check under tsc --strict:\n{msg}")
        }
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "9 = a directory read did not classify as IsADirectory, 10 = a missing \
                 file did not classify as NotFound, 11 = an unnamed errno did not reach \
                 `Other` with its raw code bound"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::TscMissing => {
            tool_missing("tsc");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            panic!("the `Other({{ code }})` pattern must type-check under tsc --strict:\n{msg}")
        }
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "captures_all must yield [[\"a\",\"1\"],[\"bb\",\"22\"]]"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "1 = a sibling of the failing task was abandoned, 2 = the failure was reported as ok, 3 = the surviving values are wrong");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!("unexpected build failure: {:?}", r.diagnostics);
        }
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => panic!("type-check failed: {msg}"),
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            panic!("has main; got NoMain: {exports:?}")
        }
        glyph_cli::run::RunOutcome::TscMissing => unreachable!("run was --no-check"),
    }
}

#[test]
fn task_detach_reports_a_rejection_instead_of_losing_it() {
    // D51: `task.detach` is the one sanctioned way to start work without
    // waiting, because a call to an async function nothing awaits is E0238.
    // Its promise is that a failure is reported, never swallowed: the work
    // here throws after its first `await`, the caller has already moved on and
    // returned, and the rejection has to arrive on stderr as a failed detached
    // task. The order of the two stdout lines is the other half: `detach` does
    // not wait.
    if !js_toolchain_available() {
        eprintln!("skipping task.detach run: node/tsx not available");
        return;
    }
    let root = unique_tmp("taskdetach");
    write_file(
        &root,
        "main.glyph",
        r#"module main

import std/io
import std/task
import std/time

async fn boom() -> number {
  let _ = extern_ts("(() => { throw new Error('detached boom') })()")
  return 0
}

async fn work() -> void {
  await time.sleep(time.Duration.ms(5))
  io.println("work started")
  let _n = await boom()
}

fn kick() -> void {
  task.detach(async fn() -> void { await work() })
  io.println("kicked")
}

fn main(argv: Array<string>) -> number {
  kick()
  return 0
}
"#,
    );
    let entry = root.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "a reported detached failure does not crash the program: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().filter(|l| !l.starts_with("glyph ")).collect::<Vec<_>>(),
        vec!["kicked", "work started"],
        "detach returns before the work runs: {stdout}"
    );
    assert!(
        stderr.contains("std/task: a detached task failed: Error: detached boom"),
        "the rejection is reported on stderr: {stderr}"
    );
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
    match glyph_cli::run::run_file(&file_glyph, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a non-zero code names the failing assertion group");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    match glyph_cli::run::run_file(&file_glyph, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "a non-zero code names the failing assertion group");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
fn array_reductions_return_option_on_empty_and_first_on_ties() {
    // G100: argmax over an array is what every search, ranking and scheduler
    // does, and until these five landed it was a four-line `match acc` fold at
    // every site. One outside app wrote it five times in one file. Every one of
    // those folds type-checks whether or not the seed and the comparison
    // direction are right, so the compiler could not catch a mistake in them.
    //
    // Two things are pinned here that a caller cannot re-derive from the
    // signature: the empty array is `None` for the four that pick a value (not
    // 0, not -Infinity) while `sum([])` is 0, and a tie goes to the *first*
    // element that achieves the extreme. Both moves are `max_by(moves, score)`
    // over two 7s and `min_by` over two 2s, so a comparison that flipped to
    // `>=`/`<=` fails this test rather than shipping.
    if !js_toolchain_available() {
        eprintln!("skipping array reductions run: node/tsx not available");
        return;
    }
    let root = unique_tmp("arrreduce");
    let prog = "module main\n\
         import std/array { max, min, sum, max_by, min_by }\n\
         import std/option { Some, None }\n\
         \n\
         type Move = { name: string, score: number }\n\
         \n\
         fn or_else(o: Option<number>, fallback: number) -> number {\n\
         \x20 return match o {\n\
         \x20\x20\x20 Some(n) => n,\n\
         \x20\x20\x20 None => fallback,\n\
         \x20 }\n\
         }\n\
         \n\
         fn name_or(o: Option<Move>, fallback: string) -> string {\n\
         \x20 return match o {\n\
         \x20\x20\x20 Some(m) => m.name,\n\
         \x20\x20\x20 None => fallback,\n\
         \x20 }\n\
         }\n\
         \n\
         fn score(m: Move) -> number {\n\
         \x20 return m.score\n\
         }\n\
         \n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let nums: Array<number> = [3, 9, 4, 9, 1]\n\
         \x20 let none: Array<number> = []\n\
         \x20 // Two 7s and two 2s, so both extremes are ties.\n\
         \x20 let moves: Array<Move> = [\n\
         \x20\x20\x20 { name: \"a\", score: 2 },\n\
         \x20\x20\x20 { name: \"b\", score: 7 },\n\
         \x20\x20\x20 { name: \"c\", score: 7 },\n\
         \x20\x20\x20 { name: \"d\", score: 2 },\n\
         \x20 ]\n\
         \x20 let no_moves: Array<Move> = []\n\
         \x20 let scalars = or_else(max(nums), 0 - 1) == 9\n\
         \x20\x20\x20 && or_else(min(nums), 0 - 1) == 1\n\
         \x20\x20\x20 && sum(nums) == 26\n\
         \x20 let empties = or_else(max(none), 0 - 1) == 0 - 1\n\
         \x20\x20\x20 && or_else(min(none), 0 - 1) == 0 - 1\n\
         \x20\x20\x20 && sum(none) == 0\n\
         \x20\x20\x20 && name_or(max_by(no_moves, score), \"none\") == \"none\"\n\
         \x20\x20\x20 && name_or(min_by(no_moves, score), \"none\") == \"none\"\n\
         \x20 let ties = name_or(max_by(moves, score), \"none\") == \"b\"\n\
         \x20\x20\x20 && name_or(min_by(moves, score), \"none\") == \"a\"\n\
         \x20 let single = or_else(max([5]), 0) == 5 && or_else(min([5]), 0) == 5\n\
         \x20 return match scalars {\n\
         \x20\x20\x20 false => 1,\n\
         \x20\x20\x20 true => match empties {\n\
         \x20\x20\x20\x20\x20 false => 2,\n\
         \x20\x20\x20\x20\x20 true => match ties {\n\
         \x20\x20\x20\x20\x20\x20\x20 false => 3,\n\
         \x20\x20\x20\x20\x20\x20\x20 true => match single {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 false => 4,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20 true => 0,\n\
         \x20\x20\x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20\x20\x20 },\n\
         \x20\x20\x20 },\n\
         \x20 }\n\
         }\n";
    write_file(&root, "reduce.glyph", prog);
    let file_glyph = root.join("reduce.glyph");
    match glyph_cli::run::run_file(&file_glyph, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "1 = max/min/sum over a non-empty array; 2 = the empty cases \
                 (None, None, 0); 3 = a tie went to the later element; \
                 4 = a one-element array"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
            tool_missing("tsx");
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
            tool_missing("tsx");
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

/// G101. `fold` cannot stop early, so alpha-beta pruning, which is a fold that
/// stops when the window closes, was four mutually recursive functions
/// threading an index. `fold_while` is that fold; the stop test runs before
/// each element is consumed, so a closed window skips the rest of the
/// children. The node count is what proves pruning happened: the standard
/// three-child tree has six leaves and a correct alpha-beta visits four.
#[test]
fn array_fold_while_prunes_an_alpha_beta_search() {
    if !js_toolchain_available() {
        eprintln!("skipping fold_while run: node/tsx not available");
        return;
    }
    let root = unique_tmp("foldwhile");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/array
import std/io

const INF: int = 1000000000

type Window = { alpha: int, beta: int, value: int, nodes: int }

fn lower(a: int, b: int) -> int {
  return match a < b {
    true => a,
    false => b,
  }
}

fn higher(a: int, b: int) -> int {
  return match a > b {
    true => a,
    false => b,
  }
}

// The window has closed: nothing further down this branch can change the
// answer, so the fold stops before touching the next child.
fn closed(w: Window) -> bool {
  return w.alpha >= w.beta
}

fn min_node(leaves: Array<int>, alpha: int, beta: int, nodes: int) -> Window {
  let start: Window = { alpha: alpha, beta: beta, value: INF, nodes: nodes }
  return array.fold_while(leaves, start, fn(w: Window, leaf: int) -> Window {
    let v = lower(w.value, leaf)
    return { alpha: w.alpha, beta: lower(w.beta, v), value: v, nodes: w.nodes + 1 }
  }, closed)
}

fn max_root(children: Array<Array<int>>) -> Window {
  let start: Window = { alpha: 0 - INF, beta: INF, value: 0 - INF, nodes: 0 }
  return array.fold_while(children, start, fn(w: Window, leaves: Array<int>) -> Window {
    let child = min_node(leaves, w.alpha, w.beta, w.nodes)
    let v = higher(w.value, child.value)
    return { alpha: higher(w.alpha, v), beta: w.beta, value: v, nodes: child.nodes }
  }, closed)
}

fn count_leaves(children: Array<Array<int>>) -> int {
  return array.fold(children, 0, fn(n: int, leaves: Array<int>) -> int {
    return n + array.len(leaves)
  })
}

pub fn main() -> void {
  let tree: Array<Array<int>> = [[3, 5], [2, 9], [0, 1]]
  let result = max_root(tree)
  io.println("value=${number.to_string(result.value)} nodes=${number.to_string(result.nodes)} of ${number.to_string(count_leaves(tree))}")
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr) = spawn_glyph_bounded(
        &[std::ffi::OsStr::new("run"), entry.as_os_str()],
        std::time::Duration::from_secs(60),
    )
    .expect("the program exits");
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    // Minimax over this tree is 3 either way; the count is the evidence. A
    // `fold_while` that consumed every element would print `nodes=6`.
    assert!(
        stdout.contains("value=3 nodes=4 of 6"),
        "alpha-beta must reach the minimax answer having visited four of six leaves: {stdout}"
    );
}

/// G101, the error-carrying half. `try_fold` stops at the first `Err` and
/// returns it; the elements after it are never visited, and a full run returns
/// `Ok` of the final accumulator.
#[test]
fn array_try_fold_stops_at_the_first_err() {
    if !js_toolchain_available() {
        eprintln!("skipping try_fold run: node/tsx not available");
        return;
    }
    let root = unique_tmp("tryfold");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/array
import std/io

type Tally = { sum: int, seen: int }

fn add(acc: Tally, x: int) -> Result<Tally, string> {
  io.println("visit ${number.to_string(x)}")
  return match x < 0 {
    true => Err("negative ${number.to_string(x)} after ${number.to_string(acc.seen)} elements"),
    false => Ok({ sum: acc.sum + x, seen: acc.seen + 1 }),
  }
}

pub fn main() -> void {
  let start: Tally = { sum: 0, seen: 0 }
  match array.try_fold([1, 2, 0 - 1, 4], start, add) {
    Ok(t) => io.println("ok sum=${number.to_string(t.sum)}"),
    Err(e) => io.println("err ${e}"),
  }
  match array.try_fold([1, 2, 3], start, add) {
    Ok(t) => io.println("ok sum=${number.to_string(t.sum)} seen=${number.to_string(t.seen)}"),
    Err(e) => io.println("err ${e}"),
  }
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr) = spawn_glyph_bounded(
        &[std::ffi::OsStr::new("run"), entry.as_os_str()],
        std::time::Duration::from_secs(60),
    )
    .expect("the program exits");
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("err negative -1 after 2 elements"),
        "the first Err is the result: {stdout}"
    );
    assert!(
        !stdout.contains("visit 4"),
        "the element after the failure is never visited: {stdout}"
    );
    assert!(
        stdout.contains("ok sum=6 seen=3"),
        "a run with no Err is Ok of the final accumulator: {stdout}"
    );
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
    let report =
        build_project_inner(&root.join("broken"), &root.join("bout"), false).expect("build");
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
    assert!(
        !ok.has_errors(),
        "the fixed program must compile: {:?}",
        ok.diagnostics
    );
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::NoMain { exports, .. } => {
            assert!(
                exports.contains(&"helper".to_string()),
                "lists exports: {exports:?}"
            );
            assert!(
                exports.contains(&"other".to_string()),
                "lists exports: {exports:?}"
            );
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
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::TypeCheckFailed(msg) => {
            assert!(msg.to_lowercase().contains("error"), "tsc output: {msg}");
            // The tsc error is remapped onto the Glyph source: it carries the
            // TS code and no longer points at the generated `.ts`.
            assert!(
                msg.contains("[TS"),
                "should carry the remapped tsc code: {msg}"
            );
            assert!(
                !msg.contains("broken.ts("),
                "raw .ts location should be gone: {msg}"
            );
        }
        glyph_cli::run::RunOutcome::Ran(code) => {
            panic!("tsc-broken code must not run; got exit {code}");
        }
        other => panic!(
            "expected TypeCheckFailed, got a different outcome: {}",
            outcome_name(&other)
        ),
    }

    // With checking off, the same program runs (its return value is 0).
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(0) => {}
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
        }
        other => panic!(
            "--no-check should run the program; got {}",
            outcome_name(&other)
        ),
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
    assert_ne!(
        after,
        "module messy\nfn   f(a:number,b:number,c:number)->number{return a+b+c}\n"
    );
    assert!(
        glyph_parser::parse(&after).is_ok(),
        "formatted file must parse"
    );

    // Idempotent: a second pass changes nothing.
    let report2 = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert_eq!(report2.formatted.len(), 0, "second pass should be a no-op");
    assert_eq!(report2.unchanged.len(), 1);
}

/// G241: `glyph fmt` must never report a failure on a file it just wrote.
///
/// A run of unary minuses parsed, and the formatter printed each operand
/// parenthesized. The parser charged a depth level for the operand slot and
/// another for the parentheses around it, so the formatted file counted twice
/// as deep as the one that went in: the second pass reported `[E0011] this
/// parenthesized expression nests deeper than the parser's limit of 64 levels`
/// and `glyph fmt: 0 formatted, 0 already formatted, 1 failed`. A parenthesized
/// operand counts once now, so both spellings are the same depth.
#[test]
fn fmt_output_of_a_deep_unary_run_still_parses() {
    let root = unique_tmp("fmtdeep");
    let minuses = "-".repeat(glyph_parser::MAX_NESTING_DEPTH as usize);
    write_file(&root, "deep.glyph", &format!("module deep

const n = {minuses}1
"));
    let file = root.join("deep.glyph");

    let first = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert!(first.failed.is_empty(), "the input parses: {:?}", first.failed);
    assert_eq!(first.formatted.len(), 1);

    let rewritten = std::fs::read_to_string(&file).unwrap();
    assert!(rewritten.contains("-(-("), "operands are parenthesized:\n{rewritten}");

    let second = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert!(
        second.failed.is_empty(),
        "glyph fmt refused a file it wrote itself: {:?}",
        second.failed
    );
    assert_eq!(second.unchanged.len(), 1, "and the rewrite is a fixed point");
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
    assert_eq!(
        report2.formatted.len(),
        0,
        "a canonical file is clean under --check"
    );
    assert_eq!(report2.unchanged.len(), 1);
}

#[test]
fn fmt_preserves_comments() {
    let root = unique_tmp("fmtcomment");
    let original = "module c\n// keep this comment\nfn f() -> number { return 1 }\n";
    write_file(&root, "commented.glyph", original);
    let file = root.join("commented.glyph");
    let report = glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    assert!(
        report.failed.is_empty(),
        "should not fail: {:?}",
        report.failed
    );
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.contains("// keep this comment"),
        "comment must be preserved:\n{after}"
    );
    assert!(
        glyph_parser::parse(&after).is_ok(),
        "formatted file must parse"
    );

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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
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
    assert!(
        ts.contains("const v = __") && !ts.contains("const v = __m0.value"),
        "{ts}"
    );
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
    assert!(
        ts.contains("const r = __m0;") && !ts.contains("const r = __m0.value"),
        "{ts}"
    );
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
fn imported_lowercase_payload_variant_dispatches_at_run_time() {
    // G147, the entry's own program, run rather than read: `Err(empty)` and
    // `Err(NotANumber(e))` through an imported `ParseError` must each reach
    // their own arm under node. This shape has built since 0.1.96; it is
    // pinned here as the run-time half of that fix, so a regression in the
    // registry shows up as the wrong line printed and not only as a changed
    // `.ts`.
    let root = unique_tmp("g147-prelude-run");
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
         import std/io { println }\n\
         import net { ParseError, empty, NotANumber }\n\
         import std/result { Result, Ok, Err }\n\
         pub fn describe(r: Result<int, ParseError>) -> string {\n\
         \x20 return match r {\n\
         \x20\x20\x20 Err(empty) => \"arm:empty\",\n\
         \x20\x20\x20 Err(NotANumber(e)) => \"arm:nan:\" + e.got,\n\
         \x20\x20\x20 Ok(v) => \"arm:ok\",\n\
         \x20 }\n\
         }\n\
         fn main() {\n\
         \x20 println(describe(Err(empty)))\n\
         \x20 println(describe(Err(NotANumber({ got: \"x\", }))))\n\
         \x20 println(describe(Ok(1)))\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["arm:empty", "arm:nan:x", "arm:ok"],
        "each value reaches its own arm: {stdout}"
    );
}

#[test]
fn imported_outer_union_lowercase_payload_variant_dispatches_at_run_time() {
    // G147, the half 0.1.96 left open: the *outer* union is the imported one.
    // `B(alpha)` and `B(beta)` over `tree.G` reached the emitter with no
    // payload type to read, fell to the shape rule, and the pair stopped the
    // build at E0305 on a valid program. The variant registry now carries the
    // declaration each variant's payload names, so the arms dispatch, and the
    // program is run so the dispatched arm is the evidence rather than the
    // emitted text.
    let root = unique_tmp("g147-outer-run");
    let src = root.join("src");
    write_file(
        &src,
        "tree.glyph",
        "module tree\npub type Inner =\n  | alpha\n  | beta\npub type G =\n  | A\n  | B(Inner)\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io { println }\n\
         import tree { G, A, B, alpha, beta }\n\
         pub fn label(g: G) -> string {\n\
         \x20 return match g {\n\
         \x20\x20\x20 A => \"arm:a\",\n\
         \x20\x20\x20 B(alpha) => \"arm:b-alpha\",\n\
         \x20\x20\x20 B(beta) => \"arm:b-beta\",\n\
         \x20 }\n\
         }\n\
         fn main() {\n\
         \x20 println(label(A))\n\
         \x20 println(label(B(alpha)))\n\
         \x20 println(label(B(beta)))\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(
        !report.has_errors(),
        "a lowercase variant of an imported outer union's payload must dispatch, not stop: {:?}",
        report.diagnostics
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["arm:a", "arm:b-alpha", "arm:b-beta"],
        "each value reaches its own arm: {stdout}"
    );
}

#[test]
fn g147_shapes_dispatch_across_a_module_boundary_at_run_time() {
    // The three G147 shapes in one program, run under node: an imported outer
    // union with a payload variant (`B(Inner)` on `shapes.G`), an imported
    // payload union under a local outer (`Wrap(Inner)` on `main.Outer`), and
    // a lowercase nullary imported variant (`alpha`) in both positions. The
    // record-payload bind is pinned in the named-import spelling and the
    // namespace spelling, since the namespace one binds no variant symbol in
    // the consumer and so has no by-name fallback to hide behind.
    //
    // The emitter reads every one of these facts about `shapes` through the
    // checker's `DeclTyResolver` rather than a registry of its own, so this
    // is the test that has to keep passing while that registry is removed:
    // a shape the trait cannot answer prints the wrong line here.
    let root = unique_tmp("g147-shapes-run");
    let src = root.join("src");
    write_file(
        &src,
        "shapes.glyph",
        "module shapes\n\
         pub type Inner =\n\
         \x20 | alpha\n\
         \x20 | beta\n\
         pub type G =\n\
         \x20 | A\n\
         \x20 | B(Inner)\n\
         \x20 | C({ label: string })\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io { println }\n\
         import shapes { G, A, B, C, Inner, alpha, beta }\n\
         import shapes as sh\n\
         type Outer =\n\
         \x20 | Leaf\n\
         \x20 | Wrap(Inner)\n\
         pub fn label(g: G) -> string {\n\
         \x20 return match g {\n\
         \x20\x20\x20 A => \"arm:a\",\n\
         \x20\x20\x20 B(alpha) => \"arm:b-alpha\",\n\
         \x20\x20\x20 B(beta) => \"arm:b-beta\",\n\
         \x20\x20\x20 C(c) => \"arm:c:\" + c.label,\n\
         \x20 }\n\
         }\n\
         pub fn label_ns(g: sh.G) -> string {\n\
         \x20 return match g {\n\
         \x20\x20\x20 sh.A => \"ns:a\",\n\
         \x20\x20\x20 sh.B(sh.alpha) => \"ns:b-alpha\",\n\
         \x20\x20\x20 sh.B(sh.beta) => \"ns:b-beta\",\n\
         \x20\x20\x20 sh.C(c) => \"ns:c:\" + c.label,\n\
         \x20 }\n\
         }\n\
         pub fn outer(o: Outer) -> string {\n\
         \x20 return match o {\n\
         \x20\x20\x20 Leaf => \"arm:leaf\",\n\
         \x20\x20\x20 Wrap(alpha) => \"arm:wrap-alpha\",\n\
         \x20\x20\x20 Wrap(beta) => \"arm:wrap-beta\",\n\
         \x20 }\n\
         }\n\
         fn main() {\n\
         \x20 println(label(A))\n\
         \x20 println(label(B(alpha)))\n\
         \x20 println(label(B(beta)))\n\
         \x20 println(label(C({ label: \"x\", })))\n\
         \x20 println(label_ns(sh.A))\n\
         \x20 println(label_ns(sh.B(sh.alpha)))\n\
         \x20 println(label_ns(sh.B(sh.beta)))\n\
         \x20 println(label_ns(sh.C({ label: \"y\", })))\n\
         \x20 println(outer(Leaf))\n\
         \x20 println(outer(Wrap(alpha)))\n\
         \x20 println(outer(Wrap(beta)))\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build ok");
    assert!(
        !report.has_errors(),
        "every G147 shape must build across the boundary: {:?}",
        report.diagnostics
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            "arm:a",
            "arm:b-alpha",
            "arm:b-beta",
            "arm:c:x",
            "ns:a",
            "ns:b-alpha",
            "ns:b-beta",
            "ns:c:y",
            "arm:leaf",
            "arm:wrap-alpha",
            "arm:wrap-beta",
        ],
        "each value reaches its own arm: {stdout}"
    );
}

#[test]
fn g213_a_user_type_named_option_is_validated_by_its_own_descriptor() {
    // G213 under node. `type Option<T> = { label: T }` is the program's own
    // record, and `Poll.parse` on `{ pick: { tag: "None" } }` has to reject
    // it: the value has no `label`. Under 0.1.118 the emitter decided the
    // field's runtime check from the spelling `Option` and gave it the
    // prelude's `{ tag: "None" } | { tag: "Some", value }` shape, so the
    // parse returned `Ok` for a value that is not a `Poll`.
    let root = unique_tmp("g213-user-option");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import std/io { println }\n\
         type Option<T> = { label: T }\n\
         type Poll = { pick: Option<string> }\n\
         fn main() {\n\
         \x20 let raw: unknown = { pick: { tag: \"None\", }, }\n\
         \x20 match Poll.parse(raw) {\n\
         \x20\x20\x20 Ok(p) => println(\"accepted a None as a Poll\"),\n\
         \x20\x20\x20 Err(e) => println(\"rejected\"),\n\
         \x20 }\n\
         \x20 let good: unknown = { pick: { label: \"yes\", }, }\n\
         \x20 match Poll.parse(good) {\n\
         \x20\x20\x20 Ok(p) => println(\"accepted:\" + p.pick.label),\n\
         \x20\x20\x20 Err(e) => println(\"rejected a Poll\"),\n\
         \x20 }\n\
         }\n",
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["rejected", "accepted:yes"],
        "the user's Option is checked by its own descriptor: {stdout}"
    );
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
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0200") && d.contains("NotANumber")),
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

/// G239: a project union named like a prelude type, imported, lost its
/// exhaustiveness check entirely.
///
/// `import orders { Result }` bound the name to the prelude container because
/// the lowerer read the name before the import path, so a bare `Result`
/// annotation lowered to the prelude `Result` rather than to `orders::Result`.
/// A bare prelude `Result` is not an application, so no variant list was
/// required and the match was never checked: the program compiled clean and
/// the emitted TypeScript threw `non-exhaustive match` at run time on `Lost`.
///
/// The three-way reproduction the review isolated is asserted in full, because
/// which of the three is silent is the whole finding: rename the type and the
/// check fires, declare it locally and the check fires, import it under a
/// prelude name and it did not. `Option` and `Nullable` collided the same way.
#[test]
fn an_imported_union_named_like_a_prelude_type_is_exhaustiveness_checked() {
    let orders = "module orders\npub type Result =\n  | Won\n  | Lost\n";
    let outcomes = "module outcomes\npub type Outcome =\n  | Won\n  | Lost\n";
    let containers =
        "module containers\npub type Option =\n  | Yes\n  | No\npub type Nullable =\n  | A\n  | B\n";
    let cases: [(&str, &str, &str, &str); 5] = [
        // (case, the declaring module's source, main's source, the variant
        // the diagnostic has to name)
        (
            "imported-result",
            orders,
            "module main\nimport orders { Result, Won }\n\
             pub fn f(r: Result) -> string {\n  return match r {\n    Won => \"won\",\n  }\n}\n",
            "Lost",
        ),
        (
            "imported-outcome",
            outcomes,
            "module main\nimport outcomes { Outcome, Won }\n\
             pub fn f(r: Outcome) -> string {\n  return match r {\n    Won => \"won\",\n  }\n}\n",
            "Lost",
        ),
        (
            "imported-option",
            containers,
            "module main\nimport containers { Option, Yes }\n\
             pub fn f(o: Option) -> string {\n  return match o {\n    Yes => \"yes\",\n  }\n}\n",
            "No",
        ),
        (
            "imported-nullable",
            containers,
            "module main\nimport containers { Nullable, A }\n\
             pub fn f(n: Nullable) -> string {\n  return match n {\n    A => \"a\",\n  }\n}\n",
            "B",
        ),
        (
            // The local declaration, which was always checked. Here so the
            // three spellings are one assertion rather than one plus a memory.
            "local-result",
            orders,
            "module main\npub type Result =\n  | Won\n  | Lost\n\
             pub fn f(r: Result) -> string {\n  return match r {\n    Won => \"won\",\n  }\n}\n",
            "Lost",
        ),
    ];
    for (case, declaring, main, missing) in cases {
        let root = unique_tmp("g239");
        let src = root.join("src");
        let module = declaring
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("module "))
            .expect("a module line");
        write_file(&src, &format!("{module}.glyph"), declaring);
        write_file(&src, "main.glyph", main);
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
        assert!(report.has_errors(), "{case}: {:?}", report.diagnostics);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.contains("E0200") && d.contains(missing)),
            "{case}: {:?}",
            report.diagnostics
        );
    }
}

/// The other half of G239: `import std/result { Result }` is still the prelude
/// container, because the prelude is a curated re-export of the stdlib and the
/// two are one declaration (Q3, G231). Only a non-`std/` import shadows.
#[test]
fn an_import_of_the_stdlib_result_is_still_the_prelude_container() {
    let root = unique_tmp("g239std");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/result { Result, Ok, Err }\n\
         pub fn f(r: Result<number, string>) -> string {\n\
         \x20 return match r {\n    Ok(n) => \"ok\",\n  }\n}\n",
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
        !report
            .diagnostics
            .iter()
            .any(|d| format!("{d:?}").contains("E0220")),
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
fn an_imported_string_literal_union_is_named_by_its_declaration() {
    // G233. The type keeps the name its author gave it across the module
    // boundary, which is G75's rule, and the one lowering that broke it was
    // this one: an imported `type Kind = "a" | "b"` lowered straight to its
    // literal set, so the diagnostic said ``expected `"a" | "b"` `` while an
    // imported alias of the same declaration said ``expected `Alias` `` and a
    // local declaration said ``expected `Kind` ``. One declaration, three
    // spellings, and a reader with nothing to open.
    let root = unique_tmp("g233name");
    let src = root.join("src");
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\npub type Kind = \"a\" | \"b\"\npub type Alias = Kind\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import catalog { Kind, Alias }\n\
         import catalog as c\n\
         pub fn main() -> void {\n\
         \x20 let a: Kind = \"z\"\n\
         \x20 let b: Alias = \"z\"\n\
         \x20 let d: c.Kind = \"z\"\n\
         \x20 print(a)\n\
         \x20 print(b)\n\
         \x20 print(d)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let named: Vec<&String> = report
        .diagnostics
        .iter()
        .filter(|d| d.contains("E0204"))
        .collect();
    assert_eq!(named.len(), 3, "three mismatches: {:?}", report.diagnostics);
    assert!(
        named.iter().filter(|d| d.contains("expected `Kind`")).count() == 2,
        "the named and the namespace spelling both name `Kind`: {named:?}"
    );
    assert!(
        named.iter().any(|d| d.contains("expected `Alias`")),
        "the alias names itself, exactly as a local alias does: {named:?}"
    );
    assert!(
        !named.iter().any(|d| d.contains("\"a\" | \"b\"")),
        "no spelling prints the literal set where a name exists: {named:?}"
    );
}

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

/// The G291 program: a three-arm literal match over `catalog.ColType`.
const UNSEEN_COLTYPE_MAIN: &str = "module main\n\
     import catalog { ColType }\n\
     pub fn width(t: ColType) -> int {\n\
     \x20 return match t {\n\
     \x20\x20\x20 \"text\" => 10,\n\
     \x20\x20\x20 \"int\" => 4,\n\
     \x20\x20\x20 \"real\" => 8,\n\
     \x20 }\n\
     }\n";

#[test]
fn match_over_a_type_whose_declaration_is_unseen_is_e0232_not_e0218() {
    // G291. Through 0.1.129, with `catalog` absent from the build or an npm
    // package whose types were never materialized, this match drew E0218 and
    // the help "Add an `else` arm": a claim that the type is an open string,
    // made about a declaration nobody had read, whose advice turns the check
    // off. Both shapes are E0232 now, naming the type and its module.
    let absent = unique_tmp("g291absent");
    write_file(&absent, "main.glyph", UNSEEN_COLTYPE_MAIN);

    let npm = unique_tmp("g291npm");
    write_file(&npm, "package.json", "{\"name\":\"app\",\"glyph\":{}}\n");
    write_file(
        &npm,
        "node_modules/catalog/package.json",
        "{\"name\":\"catalog\",\"version\":\"1.0.0\",\"types\":\"index.d.ts\"}\n",
    );
    write_file(
        &npm,
        "node_modules/catalog/index.d.ts",
        "export type ColType = \"text\" | \"int\" | \"real\";\n",
    );
    write_file(&npm, "main.glyph", UNSEEN_COLTYPE_MAIN);

    for root in [&absent, &npm] {
        let (code, stdout, stderr) = glyph_at(root, &["check", "--no-tsc", "main.glyph"]);
        let out = format!("{stdout}\n{stderr}");
        assert_eq!(code, 1, "an undecided match fails the build: {out}");
        assert!(out.contains("[E0232]"), "{out}");
        assert!(!out.contains("E0218"), "no false claim that the type is open: {out}");
        assert!(
            out.contains("`ColType` is declared in `catalog`"),
            "the message names the type and the module: {out}"
        );
        assert!(out.contains("glyph gen dts catalog"), "the help names the remedy: {out}");
    }
}

#[test]
fn unseen_link_in_a_chain_of_second_names_is_the_one_named() {
    // `catalog.glyph` is in the build and declares `ColType = Kind`, where
    // `Kind` comes from an npm package whose types were never materialized.
    // The declaration that cannot be read is `kinds`'s, so that is the one
    // E0232 names, and the match is not E0218.
    let root = unique_tmp("g291chain");
    write_file(&root, "package.json", "{\"name\":\"app\",\"glyph\":{}}\n");
    write_file(
        &root,
        "node_modules/kinds/package.json",
        "{\"name\":\"kinds\",\"version\":\"1.0.0\",\"types\":\"index.d.ts\"}\n",
    );
    write_file(
        &root,
        "node_modules/kinds/index.d.ts",
        "export type Kind = \"text\" | \"int\" | \"real\";\n",
    );
    write_file(&root, "catalog.glyph", "module catalog\nimport kinds { Kind }\npub type ColType = Kind\n");
    write_file(&root, "main.glyph", UNSEEN_COLTYPE_MAIN);
    let (code, stdout, stderr) = glyph_at(&root, &["check", "--no-tsc", "main.glyph"]);
    let out = format!("{stdout}\n{stderr}");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[E0232]"), "{out}");
    assert!(out.contains("`Kind` is declared in `kinds`"), "{out}");
    assert!(!out.contains("E0218"), "{out}");
}

#[test]
fn misspelled_import_of_a_visible_module_draws_one_error_not_two() {
    // The module is in the build and has no `ColTyp`: E0105 at the import says
    // so. Through 0.1.129 the match added an E0218 on top; it adds nothing now,
    // and certainly not an E0232 claiming the declaration is out of sight.
    let root = unique_tmp("g291typo");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", "module catalog\npub type ColType = \"text\" | \"int\" | \"real\"\n");
    write_file(&src, "main.glyph", &UNSEEN_COLTYPE_MAIN.replace("ColType", "ColTyp"));
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let codes: Vec<&str> = report
        .diagnostics
        .iter()
        .filter_map(|d| d.split(['[', ']']).nth(1))
        .collect();
    assert_eq!(codes, ["E0105"], "{:?}", report.diagnostics);
}

#[test]
fn match_over_a_stdlib_literal_union_is_checked_without_an_else() {
    // G294. `http.RedirectPolicy` is declared in Glyph's own standard library,
    // and through 0.1.130 a match over it was E0232 with every value covered
    // or not: the checker could not read the declaration, so an `else` was
    // the only way to compile it. Covering all three is clean now, and
    // dropping one is E0200 naming it.
    const MAIN: &str = "module main\nimport std/http\n\n\
         pub fn describe(p: http.RedirectPolicy) -> string {\n\
         \x20 return match p {\n\
         \x20\x20\x20 \"follow\" => \"f\",\n\
         \x20\x20\x20 \"manual\" => \"m\",\n\
         \x20\x20\x20 \"error\" => \"e\",\n\
         \x20 }\n\
         }\n";
    let full = unique_tmp("g294full");
    write_file(&full, "main.glyph", MAIN);
    let (code, stdout, stderr) = glyph_at(&full, &["check", "--no-tsc", "main.glyph"]);
    let out = format!("{stdout}\n{stderr}");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("no diagnostics"), "{out}");

    let partial = unique_tmp("g294partial");
    write_file(&partial, "main.glyph", &MAIN.replace("    \"manual\" => \"m\",\n", ""));
    let (code, stdout, stderr) = glyph_at(&partial, &["check", "--no-tsc", "main.glyph"]);
    let out = format!("{stdout}\n{stderr}");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[E0200]"), "{out}");
    assert!(out.contains("missing variants \"manual\""), "{out}");
    assert!(!out.contains("E0232"), "the declaration is in sight now: {out}");
}

#[test]
fn a_literal_arm_outside_a_union_is_refused_under_every_spelling() {
    // G305. An arm for a literal the union does not hold can never run, and
    // `tsc` says so (TS2678); through 0.1.131 Glyph's checker passed it. The
    // stdlib's own union, and a sibling's union under a named import, a
    // namespace import and an aliased one, are each refused here with E0220.
    let stdlib = unique_tmp("g305std");
    write_file(
        &stdlib,
        "main.glyph",
        "module main\nimport std/http\n\n\
         pub fn describe(p: http.RedirectPolicy) -> string {\n\
         \x20 return match p {\n\
         \x20\x20\x20 \"manual\" => \"m\",\n\
         \x20\x20\x20 \"error\" => \"e\",\n\
         \x20\x20\x20 \"folow\" => \"x\",\n\
         \x20 }\n\
         }\n",
    );
    let (code, stdout, stderr) = glyph_at(&stdlib, &["check", "--no-tsc", "main.glyph"]);
    let out = format!("{stdout}\n{stderr}");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[E0220]"), "{out}");
    // No arm names `"follow"`, so it is the suggestion. A value another arm
    // already names is never offered (G305).
    assert!(out.contains("did you mean `\"follow\"`?"), "{out}");

    for (tag, import, ty) in [
        ("named", "import modes { Mode }", "Mode"),
        ("namespace", "import modes", "modes.Mode"),
        ("alias", "import modes as md", "md.Mode"),
    ] {
        let root = unique_tmp(&format!("g305{tag}"));
        write_file(&root, "package.json", "{\"name\":\"x\",\"private\":true,\"glyph\":{}}\n");
        write_file(&root, "modes.glyph", "module modes\n\npub type Mode = \"manual\" | \"auto\"\n");
        write_file(
            &root,
            "main.glyph",
            &format!(
                "module main\n{import}\n\n\
                 pub fn label(m: {ty}) -> string {{\n\
                 \x20 return match m {{\n\
                 \x20\x20\x20 \"manual\" => \"M\",\n\
                 \x20\x20\x20 \"auto\" => \"A\",\n\
                 \x20\x20\x20 \"manuel\" => \"X\",\n\
                 \x20 }}\n\
                 }}\n"
            ),
        );
        let (code, stdout, stderr) = glyph_at(&root, &["check", "--no-tsc", "main.glyph"]);
        let out = format!("{stdout}\n{stderr}");
        assert_eq!(code, 1, "{tag}: {out}");
        assert!(out.contains("[E0220]"), "{tag}: {out}");
        assert!(out.contains("`\"manuel\"` is not a value of `Mode`"), "{tag}: {out}");
    }
}

#[test]
fn match_over_a_visible_imported_type_is_still_checked_against_it() {
    // The other side of G291: with `catalog.glyph` in the build the same
    // program is clean, and dropping a literal is E0200, never E0232.
    let root = unique_tmp("g291visible");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", "module catalog\npub type ColType = \"text\" | \"int\" | \"real\"\n");
    write_file(&src, "main.glyph", UNSEEN_COLTYPE_MAIN);
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(!report.has_errors(), "{:?}", report.diagnostics);

    let root = unique_tmp("g291visiblemissing");
    let src = root.join("src");
    write_file(&src, "catalog.glyph", "module catalog\npub type ColType = \"text\" | \"int\" | \"real\"\n");
    write_file(
        &src,
        "main.glyph",
        &UNSEEN_COLTYPE_MAIN.replace("    \"real\" => 8,\n", ""),
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("E0200")),
        "{:?}",
        report.diagnostics
    );
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("E0232") || d.contains("E0218")),
        "{:?}",
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

    let entry = root.join("solo.glyph");
    let run = || {
        let (_code, stdout, stderr, _) = spawn_glyph(&[
            std::ffi::OsStr::new("run"),
            entry.as_os_str(),
            std::ffi::OsStr::new("--no-check"),
        ]);
        (stdout, stderr)
    };

    let (out, err) = run();
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
    let (out2, err2) = run();
    assert!(
        out2.contains("hi"),
        "the program still runs: stdout {out2:?}"
    );
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
        ts.contains("Inner.__is((value as Record<string, unknown>).i, __d + 1)"),
        "imported record field calls its descriptor: {ts}"
    );
    assert!(
        ts.contains("Instant.__is((value as Record<string, unknown>).t, __d + 1)"),
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
    match glyph_cli::run::run_file(&file, &[], false, true)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(
                code, 0,
                "2 = a valid value was rejected, 3 = an imported record field was \
                 not validated, 4 = the imported `where` predicate was dropped"
            );
        }
        glyph_cli::run::RunOutcome::TsxNotFound => tool_missing("tsx"),
        glyph_cli::run::RunOutcome::TscMissing => tool_missing("tsc"),
        glyph_cli::run::RunOutcome::BuildFailed(r) => {
            panic!(
                "two-module descriptor program should build: {:?}",
                r.diagnostics
            )
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
        ts.contains("types.Inner.__is((value as Record<string, unknown>).i, __d + 1)"),
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
///
/// Every invocation is bounded by `GLYPH_DEADLINE`. A `glyph run` whose program
/// never exited used to become the suite's own hang: G166 sat for eleven hours
/// in three worktrees at once, and each `tsx` tree it started was still alive
/// at PPID 1 after the test binary had been killed by hand. Past the deadline
/// the whole process tree is killed and the test fails with its own name, the
/// elapsed time and everything the child had written.
fn spawn_glyph(args: &[&std::ffi::OsStr]) -> (i32, String, String, u32) {
    let run = run_glyph_within(args, GLYPH_DEADLINE);
    match run.code {
        Some(code) => (code, run.stdout, run.stderr, run.pid),
        None => panic!("{}", run.deadline_report(args)),
    }
}

/// The wall-clock budget for one `glyph` invocation a test spawns. The slowest
/// honest run here is a cold `glyph run` that compiles, type-checks with `tsc`
/// and starts `tsx`, under ten seconds alone and well under a minute with the
/// suite running around it. Two minutes is the price of reporting a hang, and
/// nothing that finishes needs it.
const GLYPH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

/// Run `glyph` under a caller-chosen wall-clock budget, and kill it if it
/// outlives one. `None` means the process was still alive at the deadline.
///
/// `spawn_glyph` has the same bound with a fixed budget and a panic; this form
/// is for a test whose subject is the hang itself, which wants to say in its
/// own words what a program that never exits means.
fn spawn_glyph_bounded(
    args: &[&std::ffi::OsStr],
    budget: std::time::Duration,
) -> Option<(i32, String, String)> {
    let run = run_glyph_within(args, budget);
    run.code.map(|code| (code, run.stdout, run.stderr))
}

/// What one bounded `glyph` invocation produced. `code` is `None` when the
/// deadline killed it; `stdout` and `stderr` then hold whatever had been
/// written by the time the tree died, which is the evidence a hang leaves.
struct GlyphRun {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    pid: u32,
    elapsed: std::time::Duration,
}

impl GlyphRun {
    /// The failure a test reports when its child outlived the deadline: which
    /// test, which command, how long, and the output captured so far.
    fn deadline_report(&self, args: &[&std::ffi::OsStr]) -> String {
        let test = std::thread::current()
            .name()
            .unwrap_or("<unnamed test>")
            .to_string();
        let args = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "{test}: `glyph {args}` (pid {}) was still running after {:.1?}; \
             it was killed with its whole process tree.\n\
             --- stdout so far ---\n{}\n--- stderr so far ---\n{}",
            self.pid, self.elapsed, self.stdout, self.stderr
        )
    }
}

/// Spawn `glyph`, drain both pipes, and either see it exit within `budget` or
/// kill it and everything it started.
///
/// `glyph run` compiles and then spawns `tsx`, which starts an `esbuild`
/// service; the program that hangs is two processes down from the one the test
/// spawned, and it inherits the pipes. Killing only `glyph` would leave `tsx`
/// holding the write end and running, so the child is given its own process
/// group and the timeout path kills the group.
fn run_glyph_within(args: &[&std::ffi::OsStr], budget: std::time::Duration) -> GlyphRun {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"));
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().expect("spawn glyph");
    let pid = child.id();
    let out_rx = drain_pipe(child.stdout.take().expect("stdout is piped"));
    let err_rx = drain_pipe(child.stderr.take().expect("stderr is piped"));
    let started = std::time::Instant::now();
    let code = loop {
        match child.try_wait().expect("wait on glyph") {
            Some(status) => break Some(status.code().unwrap_or(-1)),
            None if started.elapsed() >= budget => {
                kill_the_whole_tree(&mut child);
                break None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };
    let elapsed = started.elapsed();
    // With every process in the group gone both pipes are closed and the
    // readers finish at once. The grace is for a writer outside the group; it
    // gets a moment, and then the test proceeds on what was captured rather
    // than waiting on it. Joining the reader threads instead is what turned
    // "the program hung" into "the test run hung" the first time this was
    // written.
    let grace = std::time::Duration::from_secs(if code.is_some() { 10 } else { 2 });
    GlyphRun {
        code,
        stdout: collect_pipe(out_rx, grace),
        stderr: collect_pipe(err_rx, grace),
        pid,
        elapsed,
    }
}

/// Read a child's pipe on its own thread, handing each chunk over a channel.
/// A child that fills a pipe buffer blocks on the write, which would look
/// exactly like the hang under test; chunks rather than one final string so a
/// killed child's partial output survives.
fn drain_pipe<R: std::io::Read + Send + 'static>(
    mut pipe: R,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

/// Everything a `drain_pipe` reader has produced, waiting at most `grace` for
/// it to reach end of stream.
fn collect_pipe(rx: std::sync::mpsc::Receiver<Vec<u8>>, grace: std::time::Duration) -> String {
    let deadline = std::time::Instant::now() + grace;
    let mut bytes = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(chunk) => bytes.extend(chunk),
            // Disconnected means the reader hit end of stream; a timeout means
            // something still holds the pipe. Either way, what is here is what
            // the test gets.
            Err(_) => break,
        }
    }
    while let Ok(chunk) = rx.try_recv() {
        bytes.extend(chunk);
    }
    String::from_utf8_lossy(&bytes).into_owned()
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
        failures[0]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("double(2)"),
        "the failure names the example: {v}"
    );
    assert_eq!(
        failures[0]["entity"],
        serde_json::json!("calc::double"),
        "the failure carries the declaration it is about: {v}"
    );
    assert_eq!(
        failures[0]["code"],
        serde_json::json!("E0400"),
        "the failure carries a stable code: {v}"
    );
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "an example failure counts as an error: {v}"
    );
}

/// G183: `glyph check --json` reported a clean tree on a program plain `glyph
/// check` failed. `emit_check_json` diverged (it exits the process) before the
/// `@example` gate ran, so the agent-facing channel printed `{"ok": true,
/// "errors": 0}` and exited 0 on the same source, with the same flags, that the
/// human path exited 1 on. The two paths are one command; they answer the same.
///
/// The failure also has to arrive as structure, not prose: a code, the
/// `module::name` declaration it belongs to, and the detail. An agent that has
/// to regex "example failed: main::add example #1" back apart is being handed
/// the identity the runner already had and threw away.
#[test]
fn failing_example_is_visible_under_check_json() {
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let root = unique_tmp("checkexjson");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "@example add(2, 2) == 5\n\
         pub fn add(a: number, b: number) -> number { return a + b }\n",
    );

    // The human path fails, and this test is only meaningful while it does.
    let (text_code, _text_out, text_err, _pid) = spawn_glyph(&[
        "check".as_ref(),
        src.as_os_str(),
        "--no-tsc".as_ref(),
    ]);
    assert_eq!(
        text_code, 1,
        "the text path must fail on a false @example: {text_err}"
    );

    let (code, stdout, stderr, _pid) = spawn_glyph(&[
        "check".as_ref(),
        src.as_os_str(),
        "--no-tsc".as_ref(),
        "--json".as_ref(),
    ]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON ({e}): {stdout} / {stderr}"));
    assert_eq!(v["ok"], serde_json::json!(false), "ok must be false: {v}");
    assert_eq!(
        code, text_code,
        "--json must exit the same as the text path: {v}"
    );
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "an example failure counts as an error: {v}"
    );
    let failures = v["examples"]["failures"]
        .as_array()
        .unwrap_or_else(|| panic!("examples.failures must be an array: {v}"));
    assert_eq!(failures.len(), 1, "the one failure is listed: {v}");
    assert_eq!(
        failures[0]["entity"],
        serde_json::json!("main::add"),
        "the failure carries the declaration it is about: {v}"
    );
    assert_eq!(
        failures[0]["code"],
        serde_json::json!("E0400"),
        "the failure carries a stable code: {v}"
    );
    assert!(
        failures[0]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("add(2, 2)"),
        "the failure carries the detail: {v}"
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
    assert_eq!(
        json.status.code(),
        Some(2),
        "--json agrees on the code: {v}"
    );
}

// --- `glyph build --watch` (G123) -------------------------------------------
//
// Without a watch mode, an agent driving a fast edit-compile loop has to pay
// full process startup on every change, because `glyph build` has always been
// one-shot: it builds once and exits. This pins both halves of the guarantee
// a watch mode has to provide: the process must still be alive after its
// first build completes (a plain `glyph build` would already have exited by
// then), and editing a watched source file while it runs must produce a
// second build without the process being restarted.

/// Read one of a spawned child's pipes on a background thread into a shared
/// buffer while the child stays alive. `spawn_glyph_bounded` (used elsewhere in
/// this file) deliberately leaves its reader threads unjoined on timeout, which
/// is right for a hang test but wrong here: a `--watch` test needs to observe
/// rebuild banners *while the process keeps running*, not just its final
/// output after it exits or is killed.
///
/// Generic over the pipe because `--watch --json` writes its per-build objects
/// to stdout while the banners stay on stderr, and both have to be watched at
/// once.
fn stream_pipe_into<R: std::io::Read + Send + 'static>(
    mut pipe: R,
) -> std::sync::Arc<std::sync::Mutex<String>> {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let writer = std::sync::Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&chunk[..n]).into_owned();
                    writer.lock().unwrap().push_str(&text);
                }
            }
        }
    });
    buf
}

#[test]
fn build_watch_rebuilds_on_source_change_without_exiting() {
    let root = unique_tmp("buildwatch");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\npub fn value() -> number {\n  return 1\n}\n",
    );

    // Through `spawn_build_watch` rather than an inline spawn, so the child is a
    // `WatchChild` and dies on unwind. This test used to kill it with an inline
    // `kill -9` on the success path only.
    let mut child = spawn_build_watch(&src, &out, &[]);
    // Drop stdout eagerly so the child never blocks writing to a full pipe
    // buffer nobody is draining; only stderr carries the banners we assert on.
    drop(child.take_stdout());
    let captured = stream_pipe_into(child.take_stderr().expect("stderr is piped"));

    let banner_count = |text: &str| text.matches("TypeScript file(s) emitted").count();

    // Wait for the first build to land, while continuously checking the
    // process is still alive: a build that exits early would otherwise just
    // look like a slow build to the polling loop below.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if banner_count(&captured.lock().unwrap()) >= 1 {
            break;
        }
        assert!(
            child.try_wait().expect("poll child").is_none(),
            "glyph build --watch exited before completing its first build: {}",
            captured.lock().unwrap()
        );
        assert!(
            std::time::Instant::now() < deadline,
            "watch mode never completed an initial build: {}",
            captured.lock().unwrap()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // The defining behavior: a one-shot `glyph build` would have exited with
    // its first build. Watch mode must still be running.
    assert!(
        child
            .try_wait()
            .expect("poll child after first build")
            .is_none(),
        "glyph build --watch exited instead of staying alive after its first build"
    );

    // Edit the watched source and expect a second build banner, with no
    // restart: the pid must not change underneath us.
    let pid_before = child.id();
    write_file(
        &src,
        "main.glyph",
        "module main\npub fn value() -> number {\n  return 2\n}\n",
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if banner_count(&captured.lock().unwrap()) >= 2 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "editing a watched source file never triggered a rebuild: {}",
            captured.lock().unwrap()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        child.id(),
        pid_before,
        "the rebuild must happen in the same long-running process, not a restart"
    );

    // Watch mode has no natural end; only a signal stops it. `WatchChild` kills
    // on drop, so this holds whether the test passes or unwinds above.
    kill_watch(child);
}

// --- `--watch` on the layouts and timings the dev loop actually hits --------
//
// Three regressions in the first cut of watch mode, each of which left the
// watcher alive and apparently healthy while it silently stopped doing its
// job. That shape is what makes them worth pinning: a watcher that crashes
// gets noticed in a second, and a watcher that quietly serves stale output
// costs an afternoon.

/// Spawn `glyph build <src> --out <out> --no-tsc --no-test --watch` with both
/// pipes captured, plus any extra flags. Watch mode has no natural end, so
/// every caller owns killing the child; `kill_watch` does that.
fn spawn_build_watch(src: &Path, out: &Path, extra: &[&str]) -> WatchChild {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"));
    cmd.arg("build")
        .arg(src)
        .arg("--out")
        .arg(out)
        .arg("--no-tsc")
        .arg("--no-test")
        .arg("--watch");
    for flag in extra {
        cmd.arg(flag);
    }
    WatchChild(Some(
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn glyph build --watch"),
    ))
}

/// Stop a watch child. Only a signal ends watch mode, so a test that just
/// returned would leave it running past the end of the run.
///
/// Kept as a function so a test can end the child early and then assert on what
/// it wrote. It is no longer the only thing standing between a failure and a
/// leaked process: see `WatchChild`.
fn kill_watch(child: WatchChild) {
    drop(child);
}

/// A `glyph build --watch` child that dies when it goes out of scope, including
/// when a test unwinds out of an assertion.
///
/// `std::process::Child` does not kill on drop, and every watch test used to
/// call `kill_watch` on its success path only. Any earlier assertion left a
/// watcher polling forever, outliving `cargo test` itself: measured at about
/// 13% of a core re-hashing 1501 source files every 300ms, for the life of the
/// machine. A failing test that also wedges the developer's laptop is a bad
/// trade for one line of cleanup.
struct WatchChild(Option<std::process::Child>);

impl WatchChild {
    fn id(&self) -> u32 {
        self.0.as_ref().expect("child still owned").id()
    }
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.0.as_mut().expect("child still owned").try_wait()
    }
    fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.0.as_mut().expect("child still owned").stdout.take()
    }
    fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.0.as_mut().expect("child still owned").stderr.take()
    }
}

impl Drop for WatchChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // SIGKILL: watch mode has no other exit, and a test that is already
            // unwinding must not block on a graceful shutdown.
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(child.id().to_string())
                .status();
            let _ = child.wait();
        }
    }
}

/// Poll `cond` every 25ms for up to `secs`, reporting whether it ever held.
fn wait_until(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if cond() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// How many builds a stderr transcript has reported finishing.
fn watch_build_count(text: &str) -> usize {
    text.matches("TypeScript file(s) emitted").count()
}

#[test]
fn build_watch_rebuilds_when_a_nested_project_changes() {
    // The watch fingerprint walked the build target with the same walker the
    // build uses, and that walker stops at a nested project's boundary
    // (D41) -- correctly, because a nested project's files are not part of
    // the enclosing project's compilation. A tree whose projects are *all*
    // nested (`src/alpha`, `src/beta`, each carrying its own marker, nothing
    // compilable at `src` itself) therefore had a root fingerprint covering
    // zero files, which cannot change, so no edit anywhere in the tree ever
    // triggered a second build. That is the layout the hybrid app uses, so
    // watch mode was broken on exactly the dev loop it was written for.
    let root = unique_tmp("buildwatchnested");
    let src = root.join("src");
    let out = root.join("dist");
    let source = |n: u32| format!("module main\npub fn value() -> number {{\n  return {n}\n}}\n");
    write_file(&src, "alpha/package.json", "{ \"glyph\": {} }\n");
    write_file(&src, "alpha/main.glyph", &source(1));
    write_file(&src, "beta/package.json", "{ \"glyph\": {} }\n");
    write_file(&src, "beta/main.glyph", &source(1));

    let mut child = spawn_build_watch(&src, &out, &[]);
    drop(child.take_stdout());
    let captured = stream_pipe_into(child.take_stderr().expect("stderr is piped"));

    assert!(
        wait_until(30, || watch_build_count(&captured.lock().unwrap()) >= 1),
        "watch mode never completed an initial build: {}",
        captured.lock().unwrap()
    );
    // Guard the premise: this test only means something on a tree that really
    // discovered more than one project.
    assert!(
        captured.lock().unwrap().contains("2 project(s)"),
        "the fixture must build as two nested projects: {}",
        captured.lock().unwrap()
    );

    let pid_before = child.id();
    write_file(&src, "alpha/main.glyph", &source(42));

    // The banner is printed once the whole build has finished, so it is the
    // later of the two signals; waiting on the emitted file first and then
    // asserting on the banner would be a race against the build's own output.
    assert!(
        wait_until(30, || watch_build_count(&captured.lock().unwrap()) >= 2),
        "editing a nested project's source never triggered a rebuild: {}",
        captured.lock().unwrap()
    );
    let emitted = out.join("alpha").join("main.ts");
    let landed = wait_until(30, || {
        std::fs::read_to_string(&emitted).is_ok_and(|ts| ts.contains("return 42"))
    });
    assert!(
        landed,
        "the rebuild did not carry the edit into the output: {}\n{}",
        captured.lock().unwrap(),
        std::fs::read_to_string(&emitted).unwrap_or_default()
    );
    assert_eq!(
        child.id(),
        pid_before,
        "the rebuild must happen in the same long-running process, not a restart"
    );
    kill_watch(child);
}

#[test]
fn build_watch_rebuilds_when_an_edit_lands_during_a_build() {
    // The baseline fingerprint was sampled *after* the build returned, so a
    // save that landed while the build was running got folded into the
    // baseline it would later be compared against. The build that had already
    // read the old text was the last one to run, and the edit was never built
    // -- not late, never. The watcher then sat there reporting nothing to do.
    //
    // The fixture makes "during a build" observable rather than guessed at.
    // A build reads every source in the tree before it emits the first `.ts`,
    // so the appearance of the first emitted file proves the sources are read
    // and the output for this edit is already decided; and with 1500 modules
    // still to emit after it, there is well over a second of build left to
    // write into. The test asserts no build has finished both immediately
    // before and immediately after the write, so an edit that lost the race
    // fails the test instead of passing it vacuously.
    const FILLER: usize = 1500;
    let root = unique_tmp("buildwatchmidbuild");
    let src = root.join("src");
    let out = root.join("dist");
    for i in 0..FILLER {
        write_file(
            &src,
            &format!("filler{i:04}.glyph"),
            &format!("module filler{i:04}\npub fn v() -> number {{\n  return {i}\n}}\n"),
        );
    }
    // Sorted last of all the module names, so it is emitted last too.
    write_file(
        &src,
        "main.glyph",
        "module main\npub fn value() -> number {\n  return 1\n}\n",
    );

    let mut child = spawn_build_watch(&src, &out, &[]);
    drop(child.take_stdout());
    let captured = stream_pipe_into(child.take_stderr().expect("stderr is piped"));

    let first_emitted = out.join("filler0000.ts");
    assert!(
        wait_until(60, || first_emitted.exists()),
        "the first build never emitted anything: {}",
        captured.lock().unwrap()
    );
    assert_eq!(
        watch_build_count(&captured.lock().unwrap()),
        0,
        "the first build finished before the edit could be written, so this \
         test would not be exercising a mid-build edit at all"
    );
    let pid_before = child.id();
    write_file(
        &src,
        "main.glyph",
        "module main\npub fn value() -> number {\n  return 2\n}\n",
    );
    assert_eq!(
        watch_build_count(&captured.lock().unwrap()),
        0,
        "the first build finished while the edit was being written, so this \
         test would not be exercising a mid-build edit at all"
    );

    let emitted = out.join("main.ts");
    let landed = wait_until(60, || {
        std::fs::read_to_string(&emitted).is_ok_and(|ts| ts.contains("return 2"))
    });
    assert!(
        landed,
        "an edit that landed while a build was running was never built: {}\n{}",
        captured.lock().unwrap(),
        std::fs::read_to_string(&emitted).unwrap_or_default()
    );
    assert_eq!(
        child.id(),
        pid_before,
        "the rebuild must happen in the same long-running process, not a restart"
    );
    kill_watch(child);
}

#[test]
fn build_watch_with_json_stays_alive_and_reports_every_build() {
    // `--json` printed its object and exited the process, which under
    // `--watch` meant the watcher was gone after one build while its output
    // looked like a perfectly ordinary successful build. An agent driving the
    // loop through the JSON channel got one report and then silence it had no
    // way to tell from a quiet tree.
    let root = unique_tmp("buildwatchjson");
    let src = root.join("src");
    let out = root.join("dist");
    let source = |n: u32| format!("module main\npub fn value() -> number {{\n  return {n}\n}}\n");
    write_file(&src, "main.glyph", &source(1));

    let mut child = spawn_build_watch(&src, &out, &["--json"]);
    let stdout = stream_pipe_into(child.take_stdout().expect("stdout is piped"));
    let stderr = stream_pipe_into(child.take_stderr().expect("stderr is piped"));
    // Each build's object is pretty-printed, so a line that is exactly `}`
    // closes one of them.
    let objects = |text: &str| text.lines().filter(|line| *line == "}").count();

    assert!(
        wait_until(30, || objects(&stdout.lock().unwrap()) >= 1),
        "watch mode with --json never emitted a build object: {}\n{}",
        stdout.lock().unwrap(),
        stderr.lock().unwrap()
    );
    assert!(
        child.try_wait().expect("poll child").is_none(),
        "--watch --json exited after its first build instead of watching: {}",
        stdout.lock().unwrap()
    );

    let pid_before = child.id();
    write_file(&src, "main.glyph", &source(7));
    assert!(
        wait_until(30, || objects(&stdout.lock().unwrap()) >= 2),
        "a rebuild under --json produced no second object: {}\n{}",
        stdout.lock().unwrap(),
        stderr.lock().unwrap()
    );
    assert_eq!(
        child.id(),
        pid_before,
        "the rebuild must happen in the same long-running process, not a restart"
    );

    // Every object on the stream is a whole, parseable report, not a fragment
    // of one long document.
    let text = stdout.lock().unwrap().clone();
    let mut current = String::new();
    let mut parsed = Vec::new();
    for line in text.lines() {
        current.push_str(line);
        current.push('\n');
        if line == "}" {
            let value: serde_json::Value = serde_json::from_str(&current)
                .unwrap_or_else(|e| panic!("a build object did not parse ({e}): {current}"));
            parsed.push(value);
            current.clear();
        }
    }
    assert!(parsed.len() >= 2, "expected one object per build: {text}");
    for value in &parsed {
        assert_eq!(value["ok"], serde_json::json!(true), "{value}");
    }
    assert!(
        std::fs::read_to_string(out.join("main.ts"))
            .expect("read emitted main.ts")
            .contains("return 7"),
        "the rebuild reported by --json must be the edited source"
    );
    kill_watch(child);
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
    assert!(stderr.contains("no diagnostics"), "and says so: {stderr}");
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
    // `pub`, so the reachability lint (E0112), which runs past a type error
    // since G313, has nothing to say and the warning count is zero.
    write_file(
        &root,
        "broken.glyph",
        "module broken\npub fn f() -> number {\n  return \"nope\"\n}\n",
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
    assert_eq!(
        v["diagnostics"][0]["code"],
        serde_json::json!("E0204"),
        "{v}"
    );
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

/// A Glyph-clean program that `tsc` rejects: a value typed through `extern_ts`,
/// which Glyph leaves opaque by design (D29), passed where a `string` is
/// declared. The tsc stage is where this bites (TS2345), which makes it the
/// fixture for "the Glyph stage was green and the build is still red."
///
/// It used to pass a `taint.Tainted<string>` where a `Trusted<string>` is
/// declared. Since G387 (record 132) the taint wrappers are modeled, and Glyph's
/// own checker refuses that pairing with E0211 before `tsc` runs.
const TSC_RED_GLYPH_CLEAN: &str = "module main\n\
     import std/io { println }\n\
     fn run_query(sql: string) -> void {\n\
     \x20 println(sql)\n\
     }\n\
     fn main() -> void {\n\
     \x20 let user_input: Raw = extern_ts(\"42\")\n\
     \x20 run_query(user_input)\n\
     }\n\
     type Raw = extern_ts(\"number\")\n";

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
    assert!(
        stderr.contains("TS2345"),
        "remapped onto Glyph source: {stderr}"
    );
    assert!(
        !stderr.contains("no diagnostics"),
        "and no green line above it: {stderr}"
    );
}

/// G211. The ledger program: a `let` on line 7 and the failing call on
/// line 8, the last statement of the body.
///
/// The misspelled member has to be one only `tsc` sees, so the receiver is
/// typed through `extern_ts`, which Glyph leaves opaque by design (D29). On a
/// plain `string` the same typo is Glyph's own E0210 since G288, and the build
/// would stop before `tsc` ran. The declaration sits after the function so the
/// line numbers the tests assert are the ledger program's.
const G211_TAIL_CALL: &str = "module main\n\
     \n\
     import std/io\n\
     import std/string\n\
     \n\
     pub fn main() -> void {\n\
     \x20 let t: Raw = string.trim(\"  a  \")\n\
     \x20 io.println(t.toUpperCasee())\n\
     }\n\
     \n\
     type Raw = extern_ts(\"string\")\n";

/// G211, the second shape: the failing expression is the tail of a block
/// arm inside a `match` that initialises a `let`, on line 11, after a `let`
/// on line 10 inside the same arm.
const G211_ARM_TAIL: &str = "module main\n\
     \n\
     import std/io\n\
     import std/string\n\
     \n\
     pub fn main() -> void {\n\
     \x20 let n = 2\n\
     \x20 let s = match n {\n\
     \x20   2 => {\n\
     \x20     let t: Raw = string.trim(\"  a  \")\n\
     \x20     t.toUpperCasee()\n\
     \x20   },\n\
     \x20   else => \"b\",\n\
     \x20 }\n\
     \x20 io.println(s)\n\
     }\n\
     \n\
     type Raw = extern_ts(\"string\")\n";

/// The source-map half of G211, without `tsc`: build the program, find the
/// emitted line that carries the bad call, and remap a `tsc` diagnostic at
/// that position. The remap must land on the Glyph line of the call, not on
/// the statement before it. Runs wherever the suite runs, so the two
/// `tsc`-driven tests below are not the only thing standing on the fix.
fn remapped_position_of(program: &str, prefix: &str) -> String {
    let root = unique_tmp(prefix);
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", program);
    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "the program is Glyph-clean; tsc is what rejects it: {:?}",
        report.diagnostics
    );
    let m = report
        .module_maps
        .iter()
        .find(|m| m.ts_rel == "main.ts")
        .expect("a map for main.ts");
    let (line_no, line) = m
        .ts_source
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("toUpperCasee"))
        .expect("the bad call is in the emitted TypeScript");
    let col = line.find("toUpperCasee").unwrap() + 1;
    let raw = format!(
        "dist/main.ts({},{col}): error TS2551: Property 'toUpperCasee' does not exist on type 'string'.\n",
        line_no + 1
    );
    glyph_cli::tscmap::remap_tsc_output(&raw, &report.module_maps, false)
}

#[test]
fn a_tsc_error_in_the_last_statement_maps_onto_that_statement() {
    let out = remapped_position_of(G211_TAIL_CALL, "g211_map_tail");
    // Column 14 is `t.toUpperCasee` inside the `io.println(...)` call: since
    // G285 the map goes to the expression `tsc` pointed into, not to the
    // start of the statement.
    assert!(
        out.contains("main:8:14"),
        "the call is on line 8, not the `let` on line 7: {out}"
    );
    assert!(!out.contains("main:7:"), "one statement early: {out}");

    let out = remapped_position_of(G211_ARM_TAIL, "g211_map_arm");
    assert!(
        out.contains("main:11:7"),
        "the arm's tail expression is on line 11, not its `let` on line 10: {out}"
    );
    assert!(!out.contains("main:10:"), "one statement early: {out}");
}

/// G211 against the real `tsc`: `glyph build` reports the ledger program's
/// error at the call on line 8. It was reported at the `let` on line 7, which
/// is where a reader following the caret, or an agent repairing from the
/// mapped position, would have edited.
#[test]
fn build_reports_a_tsc_error_at_the_statement_that_failed() {
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let root = unique_tmp("g211_tsc_tail");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", G211_TAIL_CALL);
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-test"),
    ]);
    assert_eq!(code, 1, "tsc rejects the call: {stderr}");
    assert!(stderr.contains("TS2551"), "remapped tsc error: {stderr}");
    assert!(
        stderr.contains("main:8:14"),
        "reported at the member access on line 8: {stderr}"
    );
    assert!(!stderr.contains("main:7:"), "not at the `let` before it: {stderr}");
}

/// Build `program`, find `needle` in the emitted `main.ts`, and remap a `tsc`
/// diagnostic at `skip` characters into it the way `glyph build` would. G285's
/// harness: it needs no `tsc`, so it runs wherever the suite runs.
fn remap_tsc_at(program: &str, prefix: &str, needle: &str, skip: usize, tail: &str) -> String {
    let root = unique_tmp(prefix);
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", program);
    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(
        !report.has_errors(),
        "the program is Glyph-clean; tsc is what rejects it: {:?}",
        report.diagnostics
    );
    let m = report
        .module_maps
        .iter()
        .find(|m| m.ts_rel == "main.ts")
        .expect("a map for main.ts");
    let (line_no, line) = m
        .ts_source
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` is in the emitted TypeScript:\n{}", m.ts_source));
    let col = line[..line.find(needle).unwrap()].chars().count() + 1 + skip;
    let raw = format!("dist/main.ts({},{col}): error {tail}\n", line_no + 1);
    glyph_cli::tscmap::remap_tsc_output(&raw, &report.module_maps, false)
}

/// The caret line ariadne draws under a one-line span: the run of `─` it
/// renders, which is as wide as the span.
fn underline_width(rendered: &str) -> usize {
    rendered
        .lines()
        .find(|l| l.contains('┬'))
        .map(|l| l.chars().filter(|c| *c == '─' || *c == '┬').count())
        .unwrap_or(0)
}

/// G285. A wrong member inside one `on_change` handler underlined the whole
/// eight-line `return <div>...</div>`: the source map stopped at top-level
/// statements, and the handler is a lambda rendered by a sub-emitter. The map
/// now carries every expression, inside lambdas too, so the error lands on
/// `e.target.valu` and on nothing else.
#[test]
fn a_tsc_error_in_a_jsx_handler_maps_onto_the_member_access() {
    let program = "module main\n\
         \n\
         pub component Form(props: { name: string }) {\n\
         \x20 return <div>\n\
         \x20   <p>{props.name}</p>\n\
         \x20   <input\n\
         \x20     value={props.name}\n\
         \x20     on_change={fn(e) { e.target.valu }}\n\
         \x20   />\n\
         \x20   <span>done</span>\n\
         \x20 </div>\n\
         }\n";
    let out = remap_tsc_at(
        program,
        "g285_jsx",
        ".valu",
        1,
        "TS2339: Property 'valu' does not exist on type 'EventTarget'.",
    );
    // `tsc` reports a missing property at the property name, `valu`. The
    // innermost expression around it is `e.target.valu`, which starts at line
    // 8, column 26.
    assert!(out.contains("main:8:26"), "on the member access: {out}");
    assert!(!out.contains("main:4:"), "not the whole `return`: {out}");
    assert_eq!(underline_width(&out), "e.target.valu".len(), "{out}");
    assert!(out.contains(glyph_cli::render::TSC_HELP_EXACT), "{out}");
    assert!(!out.contains("generated `.ts`"), "{out}");
}

/// G285, the argument shape: a wrong argument inside a multi-line call in a
/// `let` underlined the whole `let`. It lands on the argument now.
const G285_ARGUMENT: &str = "module main\n\
     \n\
     import path { join }\n\
     import std/io\n\
     \n\
     pub fn main(argv: Array<string>) -> number {\n\
     \x20 let full = join(\n\
     \x20   \"a\",\n\
     \x20   42,\n\
     \x20   \"c\",\n\
     \x20 )\n\
     \x20 io.println(full)\n\
     \x20 return 0\n\
     }\n";

#[test]
fn a_wrong_argument_maps_onto_the_argument() {
    let out = remap_tsc_at(
        G285_ARGUMENT,
        "g285_arg",
        "42",
        0,
        "TS2345: Argument of type 'number' is not assignable to parameter of type 'string'.",
    );
    assert!(out.contains("main:9:5"), "on `42`: {out}");
    assert!(!out.contains("main:7:"), "not the whole `let`: {out}");
    assert_eq!(underline_width(&out), 2, "{out}");
}

/// The same program through the real `tsc`, so the column `tsc` reports is
/// the one the map is tested against rather than one the test chose.
#[test]
fn build_reports_a_wrong_argument_at_the_argument() {
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let root = unique_tmp("g285_tsc_arg");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", G285_ARGUMENT);
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-test"),
    ]);
    assert_eq!(code, 1, "tsc rejects the argument: {stderr}");
    assert!(stderr.contains("TS2345"), "remapped tsc error: {stderr}");
    assert!(stderr.contains("main:9:5"), "reported at `42`: {stderr}");
    assert!(!stderr.contains("main:7:"), "not at the `let`: {stderr}");
}

/// G211, the second shape under `tsc`: the tail expression of a block arm
/// inside a `match`. Statements inside an arm block go through the same tail
/// path as a function body's last statement, so the fix has to hold there too.
#[test]
fn build_reports_a_tsc_error_at_the_tail_of_a_match_arm() {
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let root = unique_tmp("g211_tsc_arm");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", G211_ARM_TAIL);
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        src.as_os_str(),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        std::ffi::OsStr::new("--no-test"),
    ]);
    assert_eq!(code, 1, "tsc rejects the call: {stderr}");
    assert!(stderr.contains("TS2551"), "remapped tsc error: {stderr}");
    assert!(
        stderr.contains("main:11:7"),
        "reported at the arm's tail on line 11: {stderr}"
    );
    assert!(!stderr.contains("main:10:"), "not at the `let` before it: {stderr}");
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
    match glyph_cli::run::run_file(&file, &[], false, false)
        .expect("run_file ok")
        .outcome
    {
        glyph_cli::run::RunOutcome::Ran(code) => {
            assert_eq!(code, 0, "async handler map dispatched the wrong answer");
        }
        glyph_cli::run::RunOutcome::TsxNotFound => {
            tool_missing("tsx");
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
    assert!(
        !report.has_errors(),
        "compiles now: {:?}",
        report.diagnostics
    );
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
    assert!(
        !report.emitted.is_empty(),
        "emitted nothing: {:?}",
        report.emitted
    );
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
fn explicit_type_arguments_bind_a_sibling_generic_under_both_import_spellings() {
    // G292 across the module boundary. A written type argument binds the
    // parameter it is written for, so `id<number>("x")` is E0211 on the
    // argument and E0204 on the `return`, whether the sibling's function is
    // imported by name or reached through its namespace. The order of the
    // parameters is read off the sibling's declaration, because the
    // signature carries each one by name only.
    let root = unique_tmp("g292sibling");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\npub fn id<T>(x: T) -> T {\n  return x\n}\npub fn second<A, B>(a: A, b: B) -> B {\n  return b\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import lib { id }\n\
         pub fn named() -> string {\n\
         \x20 return id<number>(\"x\")\n\
         }\n",
    );
    write_file(
        &src,
        "other.glyph",
        "module other\n\
         import lib\n\
         pub fn spaced() -> number {\n\
         \x20 return lib.second<string, number>(1, \"b\")\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let count = |code: &str, file: &str| {
        report
            .diagnostics
            .iter()
            .filter(|d| d.contains(code) && d.contains(&format!("[ {file}:")))
            .count()
    };
    assert_eq!(count("E0211", "main"), 1, "{:?}", report.diagnostics);
    assert_eq!(count("E0204", "main"), 1, "{:?}", report.diagnostics);
    // `lib.second<string, number>(1, "b")`: both arguments disagree with the
    // written types, and the result is the written `number`, which the
    // declared return accepts.
    assert_eq!(count("E0211", "other"), 2, "{:?}", report.diagnostics);
    assert_eq!(count("E0204", "other"), 0, "{:?}", report.diagnostics);
}

#[test]
fn a_wrong_number_of_type_arguments_on_a_sibling_is_e0233_under_every_spelling() {
    // E0233 across the module boundary. A sibling's `fn` and `type` have a
    // parameter list the checker reads, so a wrong count of written type
    // arguments is refused whether the name is imported by name or reached
    // through the namespace, on a call and on a type alike.
    let root = unique_tmp("e0233sibling");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\n\
         pub fn id<T>(x: T) -> T {\n  return x\n}\n\
         pub fn plain(x: number) -> number {\n  return x\n}\n\
         pub type Box<T> = { value: T, }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import lib { id, plain, Box }\n\
         pub fn named(b: Box<number, string>) -> number {\n\
         \x20 let a = id<number, bool>(1)\n\
         \x20 return plain<number>(1)\n\
         }\n",
    );
    write_file(
        &src,
        "other.glyph",
        "module other\n\
         import lib\n\
         pub fn spaced(b: lib.Box<number, string>) -> number {\n\
         \x20 let a = lib.id<number, bool>(1)\n\
         \x20 return lib.plain<number>(1)\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let found = |file: &str| -> Vec<String> {
        report
            .diagnostics
            .iter()
            .filter(|d| d.contains("E0233") && d.contains(&format!("[ {file}:")))
            .cloned()
            .collect()
    };
    let named = found("main");
    assert_eq!(named.len(), 3, "{:?}", report.diagnostics);
    assert!(named.iter().any(|d| d.contains("`Box` takes 1, found 2")), "{named:?}");
    assert!(named.iter().any(|d| d.contains("`id` takes 1, found 2")), "{named:?}");
    assert!(named.iter().any(|d| d.contains("`plain` takes 0, found 1")), "{named:?}");
    let spaced = found("other");
    assert_eq!(spaced.len(), 3, "{:?}", report.diagnostics);
    assert!(spaced.iter().any(|d| d.contains("`lib.Box` takes 1, found 2")), "{spaced:?}");
    assert!(spaced.iter().any(|d| d.contains("`lib.id` takes 1, found 2")), "{spaced:?}");
    assert!(spaced.iter().any(|d| d.contains("`lib.plain` takes 0, found 1")), "{spaced:?}");
}

#[test]
fn type_arguments_through_a_function_value_hold_for_a_sibling_under_both_spellings() {
    // A sibling's function bound to a local keeps its type parameters, so a
    // call through the local reads its written type arguments the way a call
    // of the declaration does: the right count binds (E0211, E0204), a wrong
    // count is E0233, and any on a non-generic function is E0233. If this
    // regresses, `let f = id` turns `f<number>("x")` into a call `--no-tsc`
    // passes, and whether the guarantee holds depends on how the function
    // was reached.
    let root = unique_tmp("fnvaluetargs");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\n\
         pub fn id<T>(x: T) -> T {\n  return x\n}\n\
         pub fn plain(x: number) -> number {\n  return x\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import lib { id, plain }\n\
         pub fn named() -> string {\n\
         \x20 let f = id\n\
         \x20 let g = plain\n\
         \x20 let n: number = f<number, bool>(1)\n\
         \x20 let m: number = g<number>(1)\n\
         \x20 return f<number>(\"x\")\n\
         }\n",
    );
    write_file(
        &src,
        "other.glyph",
        "module other\n\
         import lib\n\
         pub fn spaced() -> string {\n\
         \x20 let f = lib.id\n\
         \x20 let g = lib.plain\n\
         \x20 let n: number = f<number, bool>(1)\n\
         \x20 let m: number = g<number>(1)\n\
         \x20 return f<number>(\"x\")\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    for file in ["main", "other"] {
        let found = |code: &str| -> Vec<String> {
            report
                .diagnostics
                .iter()
                .filter(|d| d.contains(code) && d.contains(&format!("[ {file}:")))
                .cloned()
                .collect()
        };
        let counts = found("E0233");
        assert_eq!(counts.len(), 2, "{file}: {:?}", report.diagnostics);
        assert!(counts.iter().any(|d| d.contains("`f` takes 1, found 2")), "{counts:?}");
        assert!(counts.iter().any(|d| d.contains("`g` takes 0, found 1")), "{counts:?}");
        assert_eq!(found("E0211").len(), 1, "{file}: {:?}", report.diagnostics);
        assert_eq!(found("E0204").len(), 1, "{file}: {:?}", report.diagnostics);
    }
}

#[test]
fn a_sibling_generic_written_with_no_type_arguments_is_e0233_under_every_spelling() {
    // G292, the bare count across the module boundary. A sibling's generic
    // `type` written with no arguments takes 0 of its 1, by name and through
    // the namespace alike. The sibling's own non-generic `Result` takes 0, so
    // naming it bare is not counted against the prelude's 2.
    let root = unique_tmp("e0233bare");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\n\
         pub type Box<T> = { value: T, }\n\
         pub type Result = | Won | Lost\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import lib { Box, Result }\n\
         pub fn named(b: Box, r: Result) -> number {\n\
         \x20 return 0\n\
         }\n",
    );
    write_file(
        &src,
        "other.glyph",
        "module other\n\
         import lib\n\
         pub fn spaced(b: lib.Box, r: lib.Result) -> number {\n\
         \x20 return 0\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let found = |file: &str| -> Vec<String> {
        report
            .diagnostics
            .iter()
            .filter(|d| d.contains("E0233") && d.contains(&format!("[ {file}:")))
            .cloned()
            .collect()
    };
    let named = found("main");
    assert_eq!(named.len(), 1, "{:?}", report.diagnostics);
    assert!(named[0].contains("`Box` takes 1, found 0"), "{named:?}");
    let spaced = found("other");
    assert_eq!(spaced.len(), 1, "{:?}", report.diagnostics);
    assert!(spaced[0].contains("`lib.Box` takes 1, found 0"), "{spaced:?}");
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
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\npub type Box<T> = { value: T }\n",
    );
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
    write_file(
        &src,
        "catalog.glyph",
        "module catalog\npub type Box<T> = { value: T }\n",
    );
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
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("E0210") && d.contains("Box")),
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

    let report = build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
    let count = |code: &str| report.structured.iter().filter(|d| d.code == code).count();
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

    let report = build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
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

    let report = build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
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

    let report = build_project_inner(&src, &root.join("dist"), false).expect("build_project ok");
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
    write_file(&src, "model.glyph", "module model\npub type Id = string\n");
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
        glyph_cli::run::RunOutcome::TsxNotFound => tool_missing("tsx"),
        glyph_cli::run::RunOutcome::TscMissing => tool_missing("tsc"),
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
    assert!(
        tree.has_errors(),
        "an unmarked tree must still report E0104"
    );
    assert!(
        tree.diagnostics().any(|d| d.contains("E0104")),
        "{:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );
    assert_eq!(
        tree.projects.len(),
        1,
        "no marker means exactly one project"
    );
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
            tool_missing("tsc");
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
            tool_missing("tsc");
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

/// The G107 layout, built once: a root project with a working `main`, and a
/// nested `beta` project whose `main` has one unresolvable import.
///
/// This is the arrangement that actually collides. Both projects emit a module
/// at `main.ts` (the conventional entry name), and the *root* project's output
/// sits at the top of the out tree, so its emitted path is a suffix of the
/// nested project's. Two sibling subdirectories do not reproduce it: there the
/// root contributes no sources, every remaining project carries a directory of
/// its own, and no path can shadow another.
fn build_root_and_nested_beta(tag: &str) -> (PathBuf, PathBuf, glyph_cli::build::TreeReport) {
    let root = unique_tmp(tag);
    let out = unique_tmp(&format!("{tag}_out"));

    write_file(
        &root,
        "package.json",
        "{ \"name\": \"root\", \"private\": true, \"glyph\": {} }\n",
    );
    write_file(
        &root,
        "main.glyph",
        "module main\n\nimport std/io { println }\n\npub fn run() -> number {\n  println(\"root\")\n  return 1\n}\n",
    );

    let beta = root.join("beta");
    std::fs::create_dir_all(&beta).expect("create beta");
    // `totally-not-installed` is declared and never installed, which is how a
    // tsc-only import error is spelled: Glyph reports an import no manifest
    // declares (E0104), so a name that IS declared stays its business alone and
    // reaches `tsc` as TS2307.
    write_file(
        &beta,
        "package.json",
        "{ \"name\": \"beta\", \"private\": true, \"glyph\": {}, \
         \"dependencies\": { \"totally-not-installed\": \"^1.0.0\" } }\n",
    );
    write_file(
        &beta,
        "main.glyph",
        "module main\n\nimport totally-not-installed { thing }\n\npub fn run() -> number {\n  return 1\n}\n",
    );

    let tree = glyph_cli::build::build_tree(&root, &out, false).expect("build tree");
    assert!(
        !tree.has_errors(),
        "sanity: the bad import is a tsc-only error (an unresolvable npm \
         package, not a Glyph resolve error): {:?}",
        tree.diagnostics().collect::<Vec<_>>()
    );
    assert_eq!(tree.projects.len(), 2, "sanity: root and beta both build");
    assert!(
        out.join("main.ts").is_file() && out.join("beta").join("main.ts").is_file(),
        "sanity: the root project emits at the top of the out tree, the nested \
         one below it"
    );
    (root, out, tree)
}

/// G107: a multi-project tree's `tsc` error must quote *its own* project's
/// source, not a same-named module from another project in the tree.
///
/// The root project builds clean; `beta` has one broken import. If the remap
/// picks the wrong project's `ModuleMap` for `main.ts`, the rendered diagnostic
/// quotes the root's working import line instead of `beta`'s broken one, and a
/// developer staring at correct code for an error that is not there has no way
/// to find the real bug. The root's import line must never appear in the
/// remapped output, and `beta`'s own broken import line must.
///
/// No `tsc` needed: the collision is entirely in how the flattened module maps
/// are keyed and matched, so the error line is written out here exactly as
/// `tsc` prints one. A test that needs a toolchain is a test that passes on a
/// machine where it never ran.
#[test]
fn a_tsc_error_is_quoted_against_its_own_projects_source() {
    let (root, out, tree) = build_root_and_nested_beta("multiproject_tscmap");

    // Verbatim `tsc` shape: `path(line,col): error TSxxxx: message`, naming the
    // nested project's emitted file. Line 3 is the import line in the emitted
    // `.ts` as well as in the source.
    let beta_ts = out.join("beta").join("main.ts");
    let raw = format!(
        "{}(3,23): error TS2307: Cannot find module 'totally-not-installed' or \
         its corresponding type declarations.\n",
        beta_ts.display()
    );

    let flat = tree.flatten();

    // Half one of the fix, checked on its own: every project's map is keyed by
    // where its output landed, the root project's included. A key that is a
    // path suffix of another key *is* the collision, whatever the matcher then
    // makes of it, so assert the shape and not just the outcome.
    let keys: Vec<&str> = flat.module_maps.iter().map(|m| m.ts_rel.as_str()).collect();
    let out_dir = out
        .file_name()
        .and_then(|n| n.to_str())
        .expect("out dir name");
    assert!(
        keys.iter().all(|k| k.starts_with(&format!("{out_dir}/"))),
        "every module map must name the out path its file landed at: {keys:?}"
    );
    for a in &keys {
        for b in &keys {
            assert!(
                a == b || !a.ends_with(&format!("/{b}")),
                "`{b}` is a path suffix of `{a}`, so it can shadow it: {keys:?}"
            );
        }
    }

    // Half two, and the behaviour the whole thing exists for.
    let rendered = glyph_cli::tscmap::remap_tsc_output(&raw, &flat.module_maps, false);
    assert!(
        rendered.contains("import totally-not-installed { thing }"),
        "the remapped diagnostic must quote beta's own broken import line:\n{rendered}"
    );
    assert!(
        !rendered.contains("import std/io { println }"),
        "the error is beta's, not the root project's: the root's own working \
         import line must never be quoted as the failing source:\n{rendered}"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&out);
}

/// The same G107 collision, end to end through a real `tsc` run, so the
/// hand-written error line above cannot drift from the shape `tsc` emits.
///
/// Skipped when `tsc` is absent, and it says so: the assertions below are the
/// point of the test, and a green result that skipped them means nothing.
#[test]
fn a_real_tsc_error_is_quoted_against_its_own_projects_source() {
    if !tsc_available() {
        eprintln!("skipping G107 end-to-end check: tsc not available");
        return;
    }
    let (root, out, tree) = build_root_and_nested_beta("multiproject_tscmap_e2e");

    let raw = match glyph_cli::runtime::check_tree_with_tsc(&tree, &out).expect("run tsc") {
        glyph_cli::runtime::TscOutcome::Failed(msg) => msg,
        glyph_cli::runtime::TscOutcome::NotFound => {
            panic!("tsc answered --version a moment ago; it must not vanish mid-test")
        }
        glyph_cli::runtime::TscOutcome::Passed => {
            panic!("sanity: beta's unresolvable import must fail tsc");
        }
    };
    assert!(
        raw.contains("TS2307"),
        "sanity: the failure must be the unresolvable import:\n{raw}"
    );

    let flat = tree.flatten();
    let rendered = glyph_cli::tscmap::remap_tsc_output(&raw, &flat.module_maps, false);

    assert!(
        rendered.contains("import totally-not-installed { thing }"),
        "the remapped diagnostic must quote beta's own broken import line:\n{rendered}"
    );
    assert!(
        !rendered.contains("import std/io { println }"),
        "the error is beta's, not the root project's: the root's own working \
         import line must never be quoted as the failing source:\n{rendered}"
    );

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
        !diags
            .iter()
            .any(|d| d.contains(&format!("rooted at `{}", root.display()))),
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
    std::fs::write(root.join("scripts").join("tool.glyph"), "module tool\n").expect("write tool");

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
    write_file(
        &src,
        "cat.glyph",
        "module cat\npub type ColType = \"text\" | \"int\"\n",
    );
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
        ts.contains("(v: unknown, d: number): __GlyphResult<Cfg, Issue[]> => Cfg.__parse(v, d)"),
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
    // G89/D47. `std/process.exit` was typed `-> never`, so the concept existed
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
    let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
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
fn parse_is_refused_on_an_imported_record_with_an_unverifiable_field() {
    // G210. E0304 fired only in the module that declared the record: the
    // emitter looked the record up in the module it was emitting, an import
    // was not there, and `Doc.parse` in any other module built clean and
    // returned `Ok` on a value whose `lexer` it never checked. The verdict is
    // now the declaring module's, read by every importer, so each spelling of
    // the import draws the same diagnostic, naming the record by its own name.
    const W: &str = "module w\n\
         pub type Lexer = extern_ts(\"import('marky').Lexer\")\n\
         @open\n\
         pub type Doc = { lexer: Lexer, title: string, }\n\
         pub type Bag = { items: Array<unknown>, title: string, }\n\
         pub type Fine = { title: string, }\n";
    let cases = [
        ("named", "import w { Doc }", "Doc.parse(v)", "`Doc`: field `lexer` has type `Lexer`"),
        ("namespace", "import w", "w.Doc.parse(v)", "`Doc`: field `lexer` has type `Lexer`"),
        ("aliased", "import w as ww", "ww.Doc.parse(v)", "`Doc`: field `lexer` has type `Lexer`"),
    ];
    for (dir, import, call, expected) in cases {
        let root = unique_tmp(&format!("g210_{dir}"));
        let src = root.join("src");
        write_file(&src, "w.glyph", W);
        write_file(
            &src,
            "u.glyph",
            &format!(
                "module u\n{import}\n\
                 pub fn check(v: unknown) -> bool {{\n\
                 \x20 return match {call} {{ Ok(_) => true, Err(_) => false, }}\n\
                 }}\n"
            ),
        );
        let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
        let refused: Vec<_> = report.structured.iter().filter(|d| d.code == "E0304").collect();
        assert_eq!(refused.len(), 1, "`{call}` is refused once: {:?}", report.diagnostics);
        assert_eq!(refused[0].file, "u.glyph", "the refusal is at the use: {:?}", report.diagnostics);
        let all = report.diagnostics.join("\n");
        assert!(all.contains(expected), "`{call}` names the record and its field: {all}");
    }

    // Positive control: a record every field of which has a check is still
    // accepted through the import. `Bag` is one since 0.1.133: its
    // `Array<unknown>` field is checked by being an array, where it used to
    // be refused as unverifiable.
    let root = unique_tmp("g210_fine");
    let src = root.join("src");
    write_file(&src, "w.glyph", W);
    write_file(
        &src,
        "u.glyph",
        "module u\nimport w { Fine, Bag }\nimport w as ww\n\
         pub fn a(v: unknown) -> bool {\n\
         \x20 return match Fine.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n\
         pub fn b(v: unknown) -> bool {\n\
         \x20 return match ww.Fine.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n\
         pub fn c(v: unknown) -> bool {\n\
         \x20 return match Bag.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n\
         pub fn d(v: unknown) -> bool {\n\
         \x20 return match ww.Bag.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
    assert!(!report.has_errors(), "a verifiable imported record parses: {:?}", report.diagnostics);
}

#[test]
fn a_second_name_for_an_imported_record_parses_through_tsc() {
    // G296. `pub type Page = Doc`, `Doc` imported, emitted a bare `type`
    // alias and no value, so `Page.parse(v)` passed Glyph's checker and was
    // TS2693 from `tsc`. D46 makes `Page` a second name for `a`'s
    // declaration; the emitter now binds `const Page = Doc;`. A third module
    // reaches it by each import spelling, and a field typed by it is checked
    // through it rather than refused as having no runtime check. Requires
    // tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping G296 tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("g296");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "a.glyph", "module a\n\npub type Doc = {\n  title: string,\n}\n\npub type D2 = Doc\n");
    write_file(
        &src,
        "b.glyph",
        "module b\n\nimport a { Doc }\n\npub type Page = Doc\n\n\
         pub fn read(v: unknown) -> string {\n\
         \x20 return match Page.parse(v) {\n\
         \x20\x20\x20 Ok(p) => p.title,\n\
         \x20\x20\x20 Err(_) => \"bad\",\n\
         \x20 }\n\
         }\n",
    );
    write_file(
        &src,
        "c.glyph",
        "module c\n\nimport a { D2 }\nimport b { Page }\nimport b as bb\n\n\
         pub type Holder = {\n  page: Page,\n  other: bb.Page,\n  d: D2,\n}\n\n\
         pub fn named(v: unknown) -> bool {\n\
         \x20 return match Page.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n\n\
         pub fn spaced(v: unknown) -> bool {\n\
         \x20 return match bb.Page.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n\n\
         pub fn held(v: unknown) -> bool {\n\
         \x20 return match Holder.parse(v) { Ok(_) => true, Err(_) => false, }\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build runs");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let b = std::fs::read_to_string(out.join("b.ts")).expect("read b.ts");
    assert!(b.contains("export const Page = Doc;"), "the second name is bound: {b}");

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("a second name for an imported record failed tsc:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
    }
}

/// The sibling the G302 tests alias: a record and a union.
const G302_A: &str = "module a\n\npub type Doc = {\n  title: string,\n}\n\n\
     pub type Shape =\n  | Circle(number)\n  | Square(number)\n  | Point\n";

/// A module that names `a`'s record and union under second names, by the
/// import spelling `import`, with `q` the qualifier it writes (`a.`, `aa.`, or
/// nothing for the named import), and uses each second name once correctly
/// and once wrongly: a field typo and a match that misses a variant.
fn g302_b(import: &str, q: &str) -> String {
    format!(
        "module b\n\n{import}\n\n\
         pub type Page = {q}Doc\n\n\
         pub type L = {q}Shape\n\n\
         pub fn conv(d: {q}Doc) -> string {{\n  let p: Page = d\n  let back: {q}Doc = p\n  return back.title\n}}\n\n\
         pub fn typo(p: Page) -> string {{\n  return p.titel\n}}\n\n\
         pub fn area(s: L) -> number {{\n  return match s {{\n    {q}Circle(r) => r,\n    {q}Square(x) => x,\n  }}\n}}\n"
    )
}

#[test]
fn a_qualified_second_name_is_the_declaration_by_every_import_spelling() {
    // G302. D46 follows a name however it is spelled. `pub type Page = a.Doc`
    // under `import a` was not followed the way `pub type Page = Doc` under
    // `import a { Doc }` is: `Page` and `a.Doc` were E0204 against each
    // other, `p.titel` passed Glyph's checker, and a match over `a.Shape`
    // through a second name was held to no variant set. All three spellings
    // now give the named import's answers.
    for (import, q) in [
        ("import a { Doc, Shape, Circle, Square }", ""),
        ("import a", "a."),
        ("import a as aa", "aa."),
    ] {
        // The message names the second name the site wrote and the
        // declaration it reaches, spelled the way this module spells it.
        let message = format!(
            "type `Page` (a second name for `{q}Doc`) has no field `titel`; did you mean `title`?"
        );
        let root = unique_tmp("g302");
        let src = root.join("src");
        write_file(&src, "a.glyph", G302_A);
        write_file(&src, "b.glyph", &g302_b(import, q));
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
        let codes = |code: &str| report.diagnostics.iter().filter(|d| d.contains(code)).count();
        assert_eq!(codes("[E0204]"), 0, "`{import}`: a second name passes both ways: {:?}", report.diagnostics);
        assert!(
            report.diagnostics.iter().any(|d| d.contains("[E0210]") && d.contains(&message)),
            "`{import}`: the field typo is E0210 naming both names: {:?}",
            report.diagnostics
        );
        assert!(
            report.diagnostics.iter().any(|d| d.contains("[E0200]") && d.contains("Point")),
            "`{import}`: the match is held to the union's variants: {:?}",
            report.diagnostics
        );
        assert_eq!(report.diagnostics.len(), 2, "`{import}`: {:?}", report.diagnostics);
    }
}

#[test]
fn a_second_name_imported_from_a_sibling_names_its_target_in_the_sites_spelling() {
    // G302's message when the second name is the sibling's own: `a` declares
    // `pub type Page = Doc` and the site imports `Page`. The target is spelled
    // as this module would spell it, `Doc` when it imports `Doc` by name and
    // `a.Doc` when it reaches `a` through the namespace. A module that has no
    // name for `Doc` at all gets the namespace spelling of `Doc`'s module.
    for (import, ty, target) in [
        ("import a { Page, Doc }", "Page", "Doc"),
        ("import a", "a.Page", "a.Doc"),
        ("import a { Page }", "Page", "a.Doc"),
    ] {
        let root = unique_tmp("g302msg");
        let src = root.join("src");
        write_file(
            &src,
            "a.glyph",
            "module a\n\npub type Doc = {\n  title: string,\n}\n\npub type Page = Doc\n",
        );
        write_file(
            &src,
            "b.glyph",
            &format!(
                "module b\n\n{import}\n\npub fn typo(p: {ty}) -> string {{\n  return p.titel\n}}\n"
            ),
        );
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
        let message = format!(
            "type `Page` (a second name for `{target}`) has no field `titel`; did you mean `title`?"
        );
        assert!(
            report.diagnostics.iter().any(|d| d.contains("[E0210]") && d.contains(&message)),
            "`{import}`: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn a_qualified_second_name_parses_through_tsc() {
    // G302, the emitter's half. `pub type Page = a.Doc` binds
    // `export const Page = a.Doc;`, so `Page.parse(v)` reaches the sibling's
    // descriptor. It emitted a bare `type` alias and `Page.parse` was TS2693.
    // Requires tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping G302 tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("g302tsc");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "a.glyph", G302_A);
    for (module, import, q) in [("b", "import a", "a"), ("c", "import a as aa", "aa")] {
        write_file(
            &src,
            &format!("{module}.glyph"),
            &format!(
                "module {module}\n\n{import}\n\npub type Page = {q}.Doc\n\n\
                 pub type Holder = {{\n  page: Page,\n}}\n\n\
                 pub fn read(v: unknown) -> string {{\n\
                 \x20 return match Page.parse(v) {{\n\
                 \x20\x20\x20 Ok(p) => p.title,\n\
                 \x20\x20\x20 Err(_) => \"bad\",\n\
                 \x20 }}\n\
                 }}\n\n\
                 pub fn held(v: unknown) -> bool {{\n\
                 \x20 return match Holder.parse(v) {{ Ok(_) => true, Err(_) => false, }}\n\
                 }}\n"
            ),
        );
    }
    let report = build_project_inner(&src, &out, false).expect("build runs");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    for (module, q) in [("b", "a"), ("c", "aa")] {
        let ts = std::fs::read_to_string(out.join(format!("{module}.ts"))).expect("read ts");
        assert!(ts.contains(&format!("export const Page = {q}.Doc;")), "bound: {ts}");
    }

    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("a qualified second name failed tsc:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
    }
}

#[test]
fn where_over_an_imported_record_is_e0300_by_every_spelling() {
    // D39 through a chain (G207) across a module boundary. A `where` over a
    // record is E0300 when the record is declared beside it; declared in a
    // sibling and reached by any import spelling, directly or through a
    // second name, it built clean and emitted a descriptor that ran the
    // predicate on a record.
    for (import, q) in [("import a { Doc }", ""), ("import a", "a."), ("import a as aa", "aa.")] {
        for decls in [
            format!("pub type Titled = {q}Doc where value.title != \"\"\n"),
            format!("pub type Page = {q}Doc\n\npub type Titled = Page where value.title != \"\"\n"),
        ] {
            let root = unique_tmp("g302where");
            let src = root.join("src");
            write_file(&src, "a.glyph", G302_A);
            write_file(&src, "b.glyph", &format!("module b\n\n{import}\n\n{decls}"));
            let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
            assert!(
                report.diagnostics.iter().any(|d| d.contains("[E0300]") && d.contains("`Titled` refines") && d.contains(", a record,")),
                "`{decls}` under `{import}` is E0300: {:?}",
                report.diagnostics
            );
        }
    }
}

#[test]
fn the_unverifiable_parse_catch_holds_with_the_record_in_its_own_module() {
    // `catches/unverifiable-parse/` declares `Conn` beside the `Conn.parse` it
    // refuses. Moving the record into its own module, the way a real project
    // lays it out, must not move the refusal away.
    let catch = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../catches/unverifiable-parse/glyph.glyph");
    let original = std::fs::read_to_string(&catch).expect("read the catch");
    assert!(original.contains("pub type Conn = {"), "the catch still declares Conn: {original}");
    let root = unique_tmp("g210_catch");
    let src = root.join("src");
    write_file(
        &src,
        "conn.glyph",
        "module conn\n\nimport net { Socket }\n\npub type Conn = {\n  id: number,\n  sock: Socket,\n}\n",
    );
    write_file(
        &src,
        "m.glyph",
        "module m\n\nimport conn { Conn }\n\n\
         pub fn take(v: unknown) -> Result<Conn, Array<Issue>> {\n  return Conn.parse(v)\n}\n",
    );
    let report = build_project_inner(&src, &root.join("out"), false).expect("build runs");
    let all = report.diagnostics.join("\n");
    assert!(
        all.contains("E0304") && all.contains("`Conn`: field `sock` has type `Socket`"),
        "the split catch is still refused: {all}"
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
    assert!(
        !report.has_errors(),
        "declaring one is fine: {:?}",
        report.diagnostics
    );
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
    assert!(
        !report.has_errors(),
        "unknown parses: {:?}",
        report.diagnostics
    );
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
    assert!(
        !report.has_errors(),
        "both spellings compile: {:?}",
        report.diagnostics
    );
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
    assert!(
        !report.has_errors(),
        "real members compile: {:?}",
        report.diagnostics
    );
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
    assert!(
        stdout.contains("same=true"),
        "a binary file round-trips: {stdout}"
    );
    assert!(
        stdout.contains("magic=89504e47"),
        "the PNG magic survives: {stdout}"
    );
    assert!(
        stdout.contains("signed=true"),
        "starts_with finds it: {stdout}"
    );
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
    assert!(
        stdout.contains("safe=true"),
        "equal secrets compare equal: {stdout}"
    );
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
///
/// The program's own exit condition has to respect the same fact. It used to
/// count reads and exit on the second one, with a 60 ms timer between the two
/// halves; under load the two halves reached the server in one read, or the
/// two echoes reached the client in one, and a program waiting for a second
/// read that could never come sat on an idle socket for eleven hours (G166).
/// The client now sends the second half only after the first echo is back,
/// and exits when what it has heard ends in the second echo, so neither the
/// server's reads nor the client's depend on who else wants the CPU.
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
import std/string

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
  // Everything heard back so far. The exit condition is about content, not
  // about how many reads carried it: TCP has no message boundaries.
  let heard = store.create<string>("")
  let sent_rest = store.create<bool>(false)
  net.on_error(c, fn(m: string) {{ io.eprintln("client: ${{m}}") }})
  net.on_text(c, fn(text: string) {{
    io.println("client=${{text}}")
    mut heard.update(fn(h: string) {{ "${{h}}${{text}}" }})
    match string.ends_with(heard.get(), "echo:é!") {{
      true => {{
        net.close(c)
        process.exit(0)
      }},
      false => {{}},
    }}
    // The second half goes only once the first echo is back, so the server
    // has already read "hi " and is holding the lone 0xC3 when 0xA9 arrives.
    match string.ends_with(heard.get(), "echo:hi ") && !sent_rest.get() {{
      true => {{
        mut sent_rest.set(true)
        net.send_bytes(c, octets([169, 33,]))
      }},
      false => {{}},
    }}
  }})
  net.on_connect(c, fn() {{
    // "hi " then a lone 0xC3, which is the first half of "é".
    net.send_bytes(c, octets([104, 105, 32, 195,]))
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
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("second=refused:in_use"),
        "a port already in use is a structured Err, not a throw or a string to scrape: {stdout}"
    );
    // The lone 0xC3 is held back rather than decoded to U+FFFD, so the first
    // read is "hi " and the character appears whole in the second.
    assert!(
        stdout.contains("got=hi "),
        "first read stops before the partial character: {stdout}"
    );
    assert!(
        stdout.contains("got=\u{e9}!"),
        "the split character arrives whole, not as U+FFFD: {stdout}"
    );
    assert!(
        !stdout.contains('\u{fffd}'),
        "nothing was replaced by U+FFFD: {stdout}"
    );
    assert!(
        stdout.contains("client=echo:hi "),
        "the echo came back: {stdout}"
    );
    assert!(
        stdout.contains("client=echo:\u{e9}!"),
        "the second echo came back whole: {stdout}"
    );
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
    assert!(
        report.failures.is_empty(),
        "every case should hold: {:?}",
        report.failures
    );
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
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("localhost=resolved"),
        "the hosts file resolves: {stdout}"
    );
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

/// G128. `std/http` bounded nothing by default: `get` and the other verbs
/// waited forever on a peer that accepted the connection and said nothing,
/// which is the shape G127 argued against one file over. Every request now
/// carries a 30 000 ms deadline unless a `Fetch` sent through `send` says
/// `timeout_ms: 0`, which keeps meaning no timeout, so the opt-out is one
/// greppable literal at the call site. A deadline past what `setTimeout` can
/// hold is refused with an `Err` naming the limit rather than clamped to one
/// millisecond and reported as if it had elapsed.
///
/// Waiting out the default is not something a test suite does, so the default
/// is read off `fetch_of` and the mechanism is exercised at 200 ms.
#[test]
fn an_http_request_against_a_silent_peer_is_bounded() {
    if !js_toolchain_available() {
        eprintln!("skipping std/http deadline: node/tsx not available");
        return;
    }
    let root = unique_tmp("httpdeadline");
    let src = root.join("src");
    let silent = 50000 + (std::process::id() % 1000);
    let quick = silent + 1000;
    write_file(
        &src,
        "main.glyph",
        &format!(
            r#"module main

import std/http
import std/io
import std/net
import std/net {{ Socket }}
import std/result {{ Ok, Err }}

const SILENT: int = {silent}
const QUICK: int = {quick}

fn hello(req: http.Request) -> Result<http.Response, string> {{
  return Ok(http.text(200, "hi"))
}}

pub async fn main() -> void {{
  let silent_url = "http://127.0.0.1:${{number.to_string(SILENT)}}/"
  let quick_url = "http://127.0.0.1:${{number.to_string(QUICK)}}/"
  let fresh = http.fetch_of(silent_url, "GET")
  io.println("default=${{number.to_string(fresh.timeout_ms)}}")
  // Accepts the connection and then says nothing, so no response ever comes.
  match await net.listen("127.0.0.1", SILENT, fn(peer: Socket) {{}}) {{
    Ok(server) => {{
      let bounded = http.fetch_of(silent_url, "GET")
      mut bounded.timeout_ms = 200
      match await http.send(bounded) {{
        Ok(_) => io.println("bounded=answered (wrong)"),
        Err(e) => io.println("bounded=${{e.kind}}"),
      }}
      let huge = http.fetch_of(silent_url, "GET")
      mut huge.timeout_ms = 3000000000
      match await http.send(huge) {{
        Ok(_) => io.println("huge=answered (wrong)"),
        Err(e) => io.println("huge=${{e.kind}} ${{e.message}}"),
      }}
      net.stop(server)
    }},
    Err(e) => io.println("listen=failed ${{e.message}}"),
  }}
  match await http.listen("127.0.0.1", QUICK, hello) {{
    Ok(server) => {{
      let unbounded = http.fetch_of(quick_url, "GET")
      mut unbounded.timeout_ms = 0
      match await http.send(unbounded) {{
        Ok(r) => io.println("zero=${{number.to_string(r.status)}}"),
        Err(e) => io.println("zero=${{e.kind}} ${{e.message}}"),
      }}
      net.stop(server)
    }},
    Err(e) => io.println("quick=failed ${{e.message}}"),
  }}
  io.println("main is returning now")
}}
"#
        ),
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr) = spawn_glyph_bounded(
        &[std::ffi::OsStr::new("run"), entry.as_os_str()],
        std::time::Duration::from_secs(60),
    )
    .expect("the program exits: a request nobody bounded would still be pending");
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(
        stdout.contains("default=30000"),
        "a Fetch from fetch_of carries the 30 000 ms default: {stdout}"
    );
    assert!(
        stdout.contains("bounded=timeout"),
        "a peer that never answers is `kind: \"timeout\"`, not a wait: {stdout}"
    );
    assert!(
        stdout.contains("huge=argument a request deadline must be at most 2147483647ms, got 3000000000"),
        "an unholdable deadline is a usage error naming the limit: {stdout}"
    );
    assert!(
        !stdout.contains("exceeded 3000000000ms"),
        "a 1ms failure must not claim the deadline it was asked for elapsed: {stdout}"
    );
    assert!(
        stdout.contains("zero=200"),
        "timeout_ms 0 keeps meaning no timeout, so a quick server still answers: {stdout}"
    );
    assert!(
        stdout.contains("main is returning now"),
        "the program reached its end: {stdout}"
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
    Err(why) => io.eprintln("cannot bind: ${{why.message}}"),
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
    mut sent.update(fn(k: int) {{ k + 1 }})
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
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert!(stdout.contains("bound"), "the server bound: {stdout}");
    // The handshake: Node's client refuses outright if Sec-WebSocket-Accept is
    // wrong, so any traffic at all proves the digest.
    assert!(
        stdout.contains("server_text=hello"),
        "text reached the server: {stdout}"
    );
    assert!(
        stdout.contains("client_text=echo:hello"),
        "and came back: {stdout}"
    );
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
    assert!(
        stdout.contains("client_close=1000"),
        "a clean close says so: {stdout}"
    );
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
    assert!(
        stdout.contains("balanced=red"),
        "the rotation arm matched: {stdout}"
    );
    // And the arm that does not match falls through to `other` instead of being
    // swallowed by the outer `Node` tag.
    assert!(
        stdout.contains("untouched=black"),
        "the non-matching value fell through: {stdout}"
    );
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
    assert!(
        stdout.contains("a=black"),
        "the Black arm matched: {stdout}"
    );
    assert!(stdout.contains("b=red"), "the Red arm matched: {stdout}");
    assert!(
        stdout.contains("c=leaf"),
        "the else arm took the rest: {stdout}"
    );
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
    assert!(
        stdout.contains("a=deep:inner/outer"),
        "the nested payload-carrying arm matched: {stdout}"
    );
    assert!(
        stdout.contains("b=shallow:solo"),
        "the nested nullary-variant arm matched: {stdout}"
    );
    assert!(
        stdout.contains("c=leaf"),
        "the else arm took the rest: {stdout}"
    );
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
    assert!(
        stdout.contains("balanced=red"),
        "the rotation arm matched: {stdout}"
    );
    assert!(
        stdout.contains("untouched=black"),
        "the non-matching value fell through: {stdout}"
    );
    assert!(
        stdout.contains("deep=2"),
        "the nested arm won at a concrete type: {stdout}"
    );
    assert!(
        stdout.contains("shallow=1"),
        "the flat arm took the rest: {stdout}"
    );
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
    let fixed = build_project_inner(&fixed_src, &fixed_root.join("dist"), false).expect("build");
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
    write_file(
        &plain_src,
        "main.glyph",
        &MAIN.replace("Tree<string>", "Tree"),
    );
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

// ---------------------------------------------------------------------------
// A `//` note on a continued annotation's first line used to make the example
// pass. `raw_args` turns the continuation newline into a space, which joined
// the note to the text after it, so the note swallowed `== false` on re-lex and
// `collect_tests` read the remainder as "assert this is true".
// ---------------------------------------------------------------------------

#[test]
fn a_note_on_an_example_line_does_not_turn_it_green() {
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let root = unique_tmp("exnote");
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\
         \n\
         @example has_bug(3)\n\
         \x20 == false\n\
         @example has_bug(3) // the same claim, with a note on the first line\n\
         \x20 == false\n\
         pub fn has_bug(n: number) -> bool {\n\
         \x20 return true\n\
         }\n\
         \n\
         pub fn main(argv: Array<string>) -> number {\n\
         \x20 return 0\n\
         }\n",
    );
    let src = root.join("src");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        src.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(code, 1, "two false examples must fail: {stderr}");
    assert!(
        stderr.contains("2 of 2 example(s) failed"),
        "both byte-identical claims must fail, not one: {stderr}"
    );
}

#[test]
fn an_example_with_a_bracketed_argument_list_still_runs() {
    // The bracketed spelling was already correct and has to stay correct: the
    // lexer emits no newline at bracket depth (D1), so the comment inside the
    // list keeps a real newline to terminate it.
    if !js_toolchain_available() {
        eprintln!("skipping: node/tsx not available");
        return;
    }
    let root = unique_tmp("exbracket");
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\
         \n\
         @example sum([\n\
         \x20 1, // one\n\
         \x20 2,\n\
         ]) == 99\n\
         pub fn sum(xs: Array<number>) -> number {\n\
         \x20 return 3\n\
         }\n\
         \n\
         pub fn main(argv: Array<string>) -> number {\n\
         \x20 return 0\n\
         }\n",
    );
    let src = root.join("src");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        src.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    assert_eq!(
        code, 1,
        "the bracketed example is false and must fail: {stderr}"
    );
    assert!(
        stderr.contains("1 of 1 example(s) failed"),
        "it runs rather than landing in malformed: {stderr}"
    );
}

#[test]
fn fmt_keeps_a_comment_inside_a_bracketed_annotation_argument_list() {
    let root = unique_tmp("fmtbracketnote");
    let src = "module m\n\
               @example sum([\n\
               \x20 1, // one\n\
               \x20 2,\n\
               ]) == 3\n\
               pub fn sum(xs: Array<number>) -> number {\n\
               \x20 return 3\n\
               }\n";
    write_file(&root, "m.glyph", src);
    let file = root.join("m.glyph");
    glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    let once = std::fs::read_to_string(&file).unwrap();
    for _ in 0..4 {
        glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    }
    let five = std::fs::read_to_string(&file).unwrap();
    assert_eq!(once, five, "formatting is idempotent");
    assert_eq!(
        five.matches("// one").count(),
        1,
        "the bracketed note is kept exactly once:\n{five}"
    );
    assert!(
        !five.lines().any(|l| l.ends_with(' ')),
        "no line gains trailing whitespace:\n{five:?}"
    );
}

#[test]
fn fmt_keeps_a_note_on_a_continued_annotation_line() {
    // The note is no longer part of `raw_args`, so the printer has to emit it
    // through the comment machinery instead. Losing it and duplicating it are
    // both failures.
    let root = unique_tmp("fmtcontnote");
    let src = "module m\n\
               @example has_bug(3) // a note\n\
               \x20 == false\n\
               pub fn has_bug(n: number) -> bool {\n\
               \x20 return true\n\
               }\n";
    write_file(&root, "m.glyph", src);
    let file = root.join("m.glyph");
    glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    let once = std::fs::read_to_string(&file).unwrap();
    for _ in 0..4 {
        glyph_cli::fmt::format_path(&file, false).expect("fmt ok");
    }
    let five = std::fs::read_to_string(&file).unwrap();
    assert_eq!(once, five, "formatting is idempotent:\n{once}\n---\n{five}");
    assert_eq!(
        five.matches("// a note").count(),
        1,
        "the note is neither dropped nor duplicated:\n{five}"
    );
    assert!(
        !five.lines().any(|l| l.ends_with(' ')),
        "no line gains trailing whitespace:\n{five:?}"
    );
    // And the assertion still reads as one: the formatted file re-parses to the
    // same claim the author wrote, note and all.
    let m = glyph_parser::parse(&five).expect("formatted file must re-parse");
    let f = match &m.items[0] {
        glyph_ast::Decl::Fn(f) => f,
        other => panic!("expected Fn, got {other:?}"),
    };
    assert_eq!(
        f.annotations[0].raw_args, "has_bug(3) == false",
        "the assertion survives the round trip:\n{five}"
    );
}

// --- D45: `Nullable<T>` at the boundary -------------------------------------

/// D45. A `Nullable<int>` field takes the two spellings a real API sends for a
/// value that may be missing, `null` and the bare value, and rejects the tagged
/// `Option` encoding, because a non-null value is checked as the inner type and
/// `int` rejects an object. An absent key is still `missing`: `s?: Nullable<int>`
/// is the spelling for absent-or-null-or-value. The bridge to `Option` is an
/// explicit `std/nullable` call, and `json.stringify` of the value is the wire
/// form again, so the round trip holds in both directions.
#[test]
fn a_nullable_field_decodes_null_and_bare_values_and_rejects_the_tagged_form() {
    let root = unique_tmp("nullable_field");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/io
import std/json
import std/nullable
import std/string

type Frame = { op: int, s: Nullable<int> }

fn describe(label: string, raw: string) -> string {
  return match json.parse(raw) {
    Err(_) => label + " => bad json",
    Ok(v) => match Frame.parse(v) {
      Err(issues) => label + " => rejected " + json.stringify(issues),
      Ok(f) => match nullable.to_option(f.s) {
        Some(n) => label + " => Some(" + string.from(n) + ") wire " + json.stringify(f),
        None => label + " => None wire " + json.stringify(f),
      },
    },
  }
}

fn main() -> void {
  io.println(describe("null", "{\"op\":10,\"s\":null}"))
  io.println(describe("absent", "{\"op\":10}"))
  io.println(describe("bare", "{\"op\":10,\"s\":3}"))
  io.println(describe("tagged", "{\"op\":10,\"s\":{\"tag\":\"Some\",\"value\":3}}"))
  let none: Frame = { op: 1, s: nullable.from_option(None), }
  let some: Frame = { op: 2, s: nullable.from_option(Some(7)), }
  io.println("from None is_null=" + string.from(nullable.is_null(none.s)) + " wire " + json.stringify(none))
  io.println("from Some(7) is_null=" + string.from(nullable.is_null(some.s)) + " wire " + json.stringify(some))
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should build and run:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("null => None wire {\"op\":10,\"s\":null}"),
        "a JSON null decodes and round-trips as null:\n{stdout}"
    );
    assert!(
        stdout.contains("absent => rejected ")
            && stdout.contains("\"path\":[\"s\"]")
            && stdout.contains("\"code\":\"missing\""),
        "an absent key is still required:\n{stdout}"
    );
    assert!(
        stdout.contains("bare => Some(3) wire {\"op\":10,\"s\":3}"),
        "a bare value decodes as the value and round-trips as itself:\n{stdout}"
    );
    assert!(
        stdout.contains("tagged => rejected ") && stdout.contains("must be Nullable<int>"),
        "the tagged Option encoding is not a Nullable<int>:\n{stdout}"
    );
    assert!(
        stdout.contains("from None is_null=true wire {\"op\":1,\"s\":null}"),
        "from_option(None) is null on the wire:\n{stdout}"
    );
    assert!(
        stdout.contains("from Some(7) is_null=false wire {\"op\":2,\"s\":7}"),
        "from_option(Some(7)) is the bare value on the wire:\n{stdout}"
    );
}

/// D45. `T` in `Nullable<T>` may not itself be `Nullable` or `Option`: the
/// first has one runtime spelling for two states, the second would put a tagged
/// object under a null-tolerant field, which is the ambiguity the type exists
/// to avoid. One code, E0227, from the typechecker.
#[test]
fn a_nullable_over_an_option_is_rejected_at_build() {
    let root = unique_tmp("nullable_of_option");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module app\n\
         type Frame = { s: Nullable<Option<int>> }\n\
         fn main() -> void {\n  return void\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ran");
    assert!(
        report.diagnostics.iter().any(|d| d.contains("[E0227]")),
        "expected E0227 for Nullable<Option<int>>; got: {:?}",
        report.diagnostics
    );
}

/// G304. A sibling's `pub type MaybeNote = Nullable<Note>` is read as the
/// `Nullable` it names under every import spelling, so the plain dot is E0235,
/// a typo through `?.` is E0210 naming `Note`, and the defaulted chain is
/// clean. Without it the guarantee would depend on where the alias lives.
#[test]
fn a_nullable_read_through_an_imported_alias_is_checked_by_every_spelling() {
    for (import, q) in [("import a { MaybeNote }", ""), ("import a", "a."), ("import a as aa", "aa.")] {
        let root = unique_tmp("g304alias");
        let src = root.join("src");
        write_file(
            &src,
            "a.glyph",
            "module a\n\npub type Note = { name: string }\n\npub type MaybeNote = Nullable<Note>\n",
        );
        write_file(
            &src,
            "b.glyph",
            &format!(
                "module b\n\n{import}\n\n\
                 pub fn dot(n: {q}MaybeNote) -> string {{\n  return n.name\n}}\n\n\
                 pub fn typo(n: {q}MaybeNote) -> string {{\n  return n?.nmae ?? \"\"\n}}\n\n\
                 pub fn fine(n: {q}MaybeNote) -> string {{\n  return n?.name ?? \"\"\n}}\n"
            ),
        );
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
        let d = &report.diagnostics;
        assert_eq!(d.iter().filter(|d| d.contains("[E0235]")).count(), 1, "`{import}`: {d:?}");
        assert!(
            d.iter().any(|d| d.contains("[E0210]") && d.contains("type `Note` has no field `nmae`")),
            "`{import}`: {d:?}"
        );
        assert_eq!(d.len(), 2, "`{import}`: only the two refusals: {d:?}");
    }
}

/// G311, E0236 across the module boundary. A sibling's `pub type MaybeNote =
/// Option<Note>` and `pub type Res<T> = Result<T, string>` are read as the
/// `Option` and the `Result` they name under every import spelling, so `?.`
/// on either is one E0236 each, reported at the link the chain starts from,
/// and the `?.` over a `Nullable` beside them is unchanged. Without it the
/// refusal would depend on where the alias lives.
#[test]
fn an_optional_chain_on_an_imported_option_or_result_alias_is_e0236_by_every_spelling() {
    for (import, q) in [
        ("import a { MaybeNote, Res, Note, Wire }", ""),
        ("import a", "a."),
        ("import a as aa", "aa."),
    ] {
        let root = unique_tmp("g311alias");
        let src = root.join("src");
        write_file(
            &src,
            "a.glyph",
            "module a\n\npub type Note = { name: string, inner: { name: string } }\n\n\
             pub type MaybeNote = Option<Note>\n\npub type Res<T> = Result<T, string>\n\n\
             pub type Wire = Nullable<Note>\n",
        );
        write_file(
            &src,
            "b.glyph",
            &format!(
                "module b\n\n{import}\n\n\
                 pub fn opt(o: {q}MaybeNote) -> string {{\n  let x = o?.inner.name\n  return \"a\"\n}}\n\n\
                 pub fn res(r: {q}Res<{q}Note>) -> string {{\n  let x = r?.name\n  return \"a\"\n}}\n\n\
                 pub fn wire(n: {q}Wire) -> string {{\n  return n?.name ?? \"\"\n}}\n"
            ),
        );
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
        let d = &report.diagnostics;
        assert_eq!(d.iter().filter(|d| d.contains("[E0236]")).count(), 2, "`{import}`: {d:?}");
        assert!(
            d.iter().any(|d| d.contains("[E0236]") && d.contains("`?.` on an `Option<")),
            "`{import}`: {d:?}"
        );
        assert!(
            d.iter().any(|d| d.contains("[E0236]") && d.contains("`?.` on a `Result<")),
            "`{import}`: {d:?}"
        );
        // The unused `let`s draw E0107 warnings since G313; the errors are
        // the two refusals.
        let errors = d.iter().filter(|d| !d.contains("Warning:")).count();
        assert_eq!(errors, 2, "`{import}`: only the two refusals: {d:?}");
    }
}

/// G311, the member set across the module boundary (record 87, Q3). A
/// sibling's `Option` and `Result` aliases are the prelude's under every
/// import spelling, so a plain-dot read of `tag`, `value` or a misspelled
/// field off either is E0210 naming the member set, and `map_err(f)?` on the
/// imported `Result` alias still checks.
#[test]
fn a_plain_dot_member_on_an_imported_option_or_result_alias_is_e0210_by_every_spelling() {
    for (import, q) in [
        ("import a { MaybeNote, Res, Note }", ""),
        ("import a", "a."),
        ("import a as aa", "aa."),
    ] {
        let root = unique_tmp("g311members");
        let src = root.join("src");
        write_file(
            &src,
            "a.glyph",
            "module a\n\npub type Note = { name: string }\n\n\
             pub type MaybeNote = Option<Note>\n\npub type Res<T> = Result<T, string>\n",
        );
        write_file(
            &src,
            "b.glyph",
            &format!(
                "module b\n\n{import}\n\n\
                 pub fn opt(o: {q}MaybeNote) -> string {{\n  let x = o.tag\n  return \"a\"\n}}\n\n\
                 pub fn res(r: {q}Res<{q}Note>) -> string {{\n  let x = r.value\n  let y = r.nmae\n  return \"a\"\n}}\n\n\
                 pub fn ok(r: {q}Res<{q}Note>) -> Result<{q}Note, string> {{\n  let n = r.map_err(fn(e: string) -> string {{ return e }})?\n  return Ok(n)\n}}\n"
            ),
        );
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
        let d = &report.diagnostics;
        assert!(
            d.iter().any(|d| d.contains("[E0210]")
                && d.contains("type `Option<")
                && d.contains("has no field `tag`: an `Option` has no members")),
            "`{import}`: {d:?}"
        );
        assert!(
            d.iter().any(|d| d.contains("[E0210]")
                && d.contains("has no field `value`: a `Result`'s only members are `map` and `map_err`")),
            "`{import}`: {d:?}"
        );
        assert!(
            d.iter().any(|d| d.contains("[E0210]") && d.contains("has no field `nmae`: a `Result`'s")),
            "`{import}`: {d:?}"
        );
        // The unused `let`s draw E0107 warnings since G313; the errors are
        // the three refusals.
        let errors = d.iter().filter(|d| !d.contains("Warning:")).count();
        assert_eq!(errors, 3, "`{import}`: only the three refusals: {d:?}");
    }
}

/// G304 with a generic alias. A sibling's `pub type Maybe<T> = Nullable<T>`
/// applied to `Note` is read as `Nullable<Note>` under every import spelling:
/// the plain dot and the interpolated chain are E0235, a typo through `?.` is
/// E0210 naming `Note`, and the defaulted chain is clean.
#[test]
fn a_nullable_read_through_an_imported_generic_alias_is_checked_by_every_spelling() {
    for (import, q) in [("import a { Maybe, Note }", ""), ("import a", "a."), ("import a as aa", "aa.")] {
        let root = unique_tmp("g304generic");
        let src = root.join("src");
        write_file(
            &src,
            "a.glyph",
            "module a\n\npub type Note = { name: string }\n\npub type Maybe<T> = Nullable<T>\n",
        );
        write_file(
            &src,
            "b.glyph",
            &format!(
                "module b\n\n{import}\n\n\
                 pub fn dot(m: {q}Maybe<{q}Note>) -> string {{\n  return m.name\n}}\n\n\
                 pub fn hi(m: {q}Maybe<{q}Note>) -> string {{\n  return \"hi ${{m?.name}}\"\n}}\n\n\
                 pub fn typo(m: {q}Maybe<{q}Note>) -> string {{\n  return m?.nmae ?? \"\"\n}}\n\n\
                 pub fn fine(m: {q}Maybe<{q}Note>) -> string {{\n  return m?.name ?? \"\"\n}}\n"
            ),
        );
        let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
        let d = &report.diagnostics;
        assert_eq!(d.iter().filter(|d| d.contains("[E0235]")).count(), 2, "`{import}`: {d:?}");
        assert!(
            d.iter().any(|d| d.contains("[E0210]") && d.contains("type `Note` has no field `nmae`")),
            "`{import}`: {d:?}"
        );
        assert_eq!(d.len(), 3, "`{import}`: only the three refusals: {d:?}");
    }
}

#[test]
fn where_over_an_alias_of_a_record_is_e0300_like_the_direct_spelling() {
    // G207. `type Positive = Rec where value.x > 0` refines a record whenever
    // `Rec` is one, and D39 refuses a `where` on a record. The refusal used to
    // read the spelled body, so the inline spelling was E0300 while this one
    // built clean and emitted a working descriptor. The alias hop (D46) makes
    // `Rec` and the base one type, so the two spellings have to agree.
    let root = unique_tmp("g207where");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type Rec = { x: int, }\n\
         type Positive = Rec where value.x > 0\n\
         pub fn go(v: unknown) -> number {\n\
         \x20 return match Positive.parse(v) {\n\
         \x20\x20\x20 Ok(p) => p.x,\n\
         \x20\x20\x20 Err(_) => 0,\n\
         \x20 }\n\
         }\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.contains("[E0300]"))
        .unwrap_or_else(|| panic!("no E0300; diagnostics were: {:?}", report.diagnostics));
    assert!(diag.contains("`Positive` refines `Rec`, a record,"), "names the declaration, its base and the resolved shape: {diag}");
}

#[test]
fn a_module_local_alias_is_a_second_name_for_its_record() {
    // G203 and G206, one program. `type B = A` is a second name for `A`'s
    // declaration (D46): a `B` passes where an `A` is declared and the
    // reverse, where 0.1.115 drew E0211 on a program `tsc --strict` accepts;
    // and a field typo through `B` is E0210 naming the field, where the same
    // program checked clean unless `A` and `B` were imported from a sibling.
    // One spelling, one answer, whichever side of a file boundary it sits on.
    let root = unique_tmp("d45alias");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         type A = { x: number, }\n\
         type B = A\n\
         fn takes_a(a: A) -> number {\n  return a.x\n}\n\
         fn takes_b(b: B) -> number {\n  return b.x\n}\n\
         pub fn both(a: A, b: B) -> number {\n  return takes_a(b) + takes_b(a)\n}\n\
         pub fn typo(b: B) -> number {\n  return b.naem\n}\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("[E0211]")),
        "an alias passes both ways: {:?}",
        report.diagnostics
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.contains("[E0210]"))
        .unwrap_or_else(|| panic!("no E0210; diagnostics were: {:?}", report.diagnostics));
    assert!(diag.contains("naem"), "names the field: {diag}");
}

#[test]
fn parse_on_a_module_local_alias_is_the_records_parse() {
    // D46 at run time. `B.parse` is `A.parse`: the checker types the call
    // through the alias (the unit test in glyph-typechecker pins the field
    // check on the parsed value), and the emitter binds `const B = A;` right
    // after `A`'s descriptor so the call exists at run time whatever order
    // the two declarations were written in.
    let root = unique_tmp("d45parse");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         pub type B = A\n\
         type A = { x: number, }\n\
         pub fn go(v: unknown) -> number {\n\
         \x20 return match B.parse(v) {\n\
         \x20\x20\x20 Ok(b) => b.x,\n\
         \x20\x20\x20 Err(_) => 0,\n\
         \x20 }\n\
         }\n",
    );
    let dist = root.join("dist");
    let report = build_project_inner(&src, &dist, false).expect("build");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(dist.join("main.ts")).expect("main.ts");
    assert!(ts.contains("export type B = A;"), "the type alias: {ts}");
    let a = ts.find("const A = {").expect("A's descriptor");
    let b = ts.find("export const B = A;").expect("B's value alias");
    assert!(b > a, "the value alias follows the descriptor it names: {ts}");
}

#[test]
fn an_imported_const_carries_its_annotation_under_both_import_spellings() {
    // G205. `imported_fn_decl` gave a cross-module `fn` its signature and
    // nothing did the same for a `pub const`, so `ORIGIN` imported by name or
    // reached through a namespace was `Ty::Unknown` and `ORIGIN.rowz` was
    // silent, where the same const read in its own module has been E0210
    // since 0.1.116. `glyph_db::exported_const` reads the annotation across
    // the boundary and both spellings now report the typo naming the record.
    // An unannotated const has its initializer's type (G433), the structural
    // record here, so its typo is E0210 as well, naming no declaration.
    let root = unique_tmp("g205const");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib\n\
         pub type Sheet = { rows: number, cols: number, }\n\
         pub const ORIGIN: Sheet = { rows: 0, cols: 0, }\n\
         pub const LOOSE = { rows: 0, cols: 0, }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import lib { ORIGIN, LOOSE }\n\
         import lib as l\n\
         pub fn named() -> number {\n  return ORIGIN.rowz\n}\n\
         pub fn spaced() -> number {\n  return l.ORIGIN.rowz\n}\n\
         pub fn loose() -> number {\n  return LOOSE.rowz\n}\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let e0210: Vec<&String> = report
        .diagnostics
        .iter()
        .filter(|d| d.contains("[E0210]"))
        .collect();
    assert_eq!(
        e0210.len(),
        3,
        "one per annotated spelling, and one for the unannotated const: {:?}",
        report.diagnostics
    );
    assert_eq!(
        e0210.iter().filter(|d| d.contains("Sheet")).count(),
        2,
        "the annotated spellings name the record: {:?}",
        report.diagnostics
    );
    for diag in e0210 {
        assert!(diag.contains("rowz"), "names the field: {diag}");
    }
}

/// Full `check` on one file: exit code and everything it printed.
fn full_check_of(tag: &str, text: &str) -> (i32, String) {
    let root = unique_tmp(tag);
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("check"), std::ffi::OsStr::new("--no-test"), main.as_os_str()]);
    (code, format!("{stdout}\n{stderr}"))
}

#[test]
fn a_comparison_with_an_unannotated_const_keeps_the_structural_helper() {
    // G433, record 163 (probes k03 and k04). The checker types `const MODE =
    // "read"` and `const N = 5`, and `tsc` gives their emitted `const`s the
    // literal types `"read"` and `5`, which Glyph's checker does not hold
    // for a number. Emitting `N === 6` made full `check` refuse with TS2367 a
    // program Glyph accepts. An operand naming an unannotated const keeps
    // `__glyph_eq`, the emit 0.1.161 gave it, so full `check` agrees with the
    // agent gate. Requires tsc; skipped otherwise.
    if !tsc_available() {
        eprintln!("skipping the record 163 tsc check: tsc not available");
        return;
    }
    let k03 = "module main\n\nimport std/io\n\nconst MODE = \"read\"\nconst N = 5\n\n\
               fn main(argv: Array<string>) -> number {\n  let s = MODE == \"write\"\n  let t = N == 6\n  \
               let h = match MODE {\n    \"read\" => 1,\n  }\n  io.println(\"${s} ${t} ${h}\")\n  return 0\n}\n";
    let (code, all) = full_check_of("g433-k03", k03);
    assert_eq!(code, 0, "{all}");
    assert!(all.contains("tsc --strict passed."), "{all}");

    // k04 adds `m == "zzz"` on a `Mode`, which `tsc` refuses on every release
    // and Glyph does not refuse yet (G438). That TS2367 is the only one; the
    // two comparisons with the constants add none.
    let k04 = "module main\n\nimport std/io\n\ntype Mode = \"read\" | \"write\"\nconst MODE = \"read\"\nconst N = 5\n\n\
               fn f(m: Mode) -> bool {\n  return m == \"zzz\"\n}\n\n\
               fn main(argv: Array<string>) -> number {\n  let s = MODE == \"write\"\n  let t = N == 6\n  \
               io.println(\"${s} ${t}\")\n  return 0\n}\n";
    let (code, all) = full_check_of("g433-k04", k04);
    assert_eq!(code, 1, "{all}");
    assert_eq!(all.matches("[TS2367]").count(), 1, "{all}");
    assert!(all.contains("'Mode' and '\"zzz\"'"), "{all}");

    let root = unique_tmp("g433-emit");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "main.glyph", k03);
    build_project_inner(&src, &out, false).expect("build");
    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts");
    assert!(ts.contains("__glyph_eq(MODE, \"write\")"), "{ts}");
    assert!(ts.contains("__glyph_eq(N, 6)"), "{ts}");
}

#[test]
fn two_siblings_whose_unannotated_consts_read_each_other_build_without_a_panic() {
    // G433. `pub const X = b.Y` beside `pub const Y = a.X` is a query cycle
    // through `exported_const` and `decl_ty`, entered from either side. Both
    // answer `Unknown` there, so the consts are untyped and nothing is
    // refused, where the first build of the rule panicked inside salsa.
    let root = unique_tmp("g433cycle");
    let src = root.join("src");
    write_file(&src, "a.glyph", "module a\nimport b\npub const X = b.Y\n");
    write_file(&src, "b.glyph", "module b\nimport a { X }\npub const Y = X\n");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import a\n\
         import b { Y }\n\
         pub fn f() -> string {\n  let s: string = a.X\n  let t: string = Y\n  return s + t\n}\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("[E0204]")),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn an_imported_unannotated_const_has_one_type_under_both_import_spellings() {
    // G433 (record 158). An unannotated `pub const` reaches a consumer with
    // the type its initializer has in its own module, through one query per
    // declaration, so `import lib { ITEMS }` and `lib.ITEMS` are judged
    // against the same type: the `push` outside `mut` is E0240, the field
    // read where a `string` is declared is E0204, and the string literal
    // keeps its literal type, so passing it where a `Mode` is declared is
    // accepted under both spellings. A type the declaring module names
    // crosses as that declaration, so `COPY.rowz` names `Sheet`.
    let root = unique_tmp("g433const");
    let src = root.join("src");
    write_file(
        &src,
        "lib.glyph",
        "module lib
         pub type Mode = \"read\" | \"write\"
         pub type Sheet = { rows: number, cols: number, }
         pub const MODE = \"read\"
         pub const ITEMS = [1, 2]
         pub const CFG = { n: 1, }
         pub const ORIGIN: Sheet = { rows: 0, cols: 0, }
         pub const COPY = ORIGIN
",
    );
    write_file(
        &src,
        "main.glyph",
        "module main
         import lib { MODE, ITEMS, CFG, COPY, Mode }
         import lib as l
         fn takes(m: Mode) -> number {
  return 0
}
         pub fn named() -> number {
  ITEMS.push(3)
  let x: string = CFG.n
  return takes(MODE) + COPY.rowz
}
         pub fn spaced() -> number {
  l.ITEMS.push(3)
  let x: string = l.CFG.n
  return takes(l.MODE) + l.COPY.rowz
}
",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build");
    let count = |code: &str| report.diagnostics.iter().filter(|d| d.contains(code)).count();
    assert_eq!(count("[E0240]"), 2, "one push per spelling: {:?}", report.diagnostics);
    assert_eq!(count("[E0204]"), 2, "one field read per spelling: {:?}", report.diagnostics);
    assert_eq!(count("[E0211]"), 0, "the literal stands where `Mode` is declared: {:?}", report.diagnostics);
    let e0210: Vec<&String> = report.diagnostics.iter().filter(|d| d.contains("[E0210]")).collect();
    assert_eq!(e0210.len(), 2, "{:?}", report.diagnostics);
    for diag in e0210 {
        assert!(diag.contains("Sheet") && diag.contains("rowz"), "{diag}");
    }
}

#[test]
fn fs_line_reader_k_way_merges_three_sorted_sources() {
    // G105: a file could only be read whole, so a merge of sorted sources that
    // never holds more than one line per source could not be written. This is
    // that merge: one `LineReader` per source, the head line of each in hand,
    // recursion instead of an array of the file, and `next_line` advancing only
    // the reader whose head was emitted. The fixtures cover the line endings a
    // reader has to agree on: `b` is CRLF, `c` has no newline after its last
    // line, and both still merge into eight lines in order.
    if !js_toolchain_available() {
        eprintln!("skipping fs line reader merge run: node/tsx not available");
        return;
    }
    let root = unique_tmp("fslinesmerge");
    let a = root.join("a.txt");
    let b = root.join("b.txt");
    let c = root.join("c.txt");
    std::fs::write(&a, "apple\nfig\nmango\n").expect("write a");
    std::fs::write(&b, "banana\r\ngrape\r\n").expect("write b");
    std::fs::write(&c, "cherry\nkiwi\npear").expect("write c");
    let prog = r#"module prog

import std/fs
import std/io
import std/option { Option, Some, None }
import std/result { Result, Ok, Err }

type Pick = "a" | "b" | "c" | "done"

fn before(x: string, y: Option<string>) -> bool {
  return match y {
    None => true,
    Some(v) => x <= v,
  }
}

fn pick(a: Option<string>, b: Option<string>, c: Option<string>) -> Pick {
  return match a {
    Some(x) => match before(x, b) && before(x, c) {
      true => "a",
      false => pick(None, b, c),
    },
    None => match b {
      Some(y) => match before(y, c) {
        true => "b",
        false => "c",
      },
      None => match c {
        Some(_) => "c",
        None => "done",
      },
    },
  }
}

fn emit(head: Option<string>, r: fs.LineReader) -> Result<Option<string>, fs.FsError> {
  match head {
    None => return Ok(None),
    Some(line) => io.println(line),
  }
  return fs.next_line(r)
}

fn failed(e: fs.FsError) -> number {
  io.println("merge failed: ${e.message}")
  return 5
}

fn merge(
  ra: fs.LineReader, a: Option<string>,
  rb: fs.LineReader, b: Option<string>,
  rc: fs.LineReader, c: Option<string>,
  n: int,
) -> Result<int, fs.FsError> {
  return match pick(a, b, c) {
    "a" => merge(ra, emit(a, ra)?, rb, b, rc, c, n + 1),
    "b" => merge(ra, a, rb, emit(b, rb)?, rc, c, n + 1),
    "c" => merge(ra, a, rb, b, rc, emit(c, rc)?, n + 1),
    "done" => Ok(n),
  }
}

fn start(ra: fs.LineReader, rb: fs.LineReader, rc: fs.LineReader) -> Result<int, fs.FsError> {
  return merge(ra, fs.next_line(ra)?, rb, fs.next_line(rb)?, rc, fs.next_line(rc)?, 0)
}

fn main(argv: Array<string>) -> number {
  return match fs.open_lines("__A__") {
    Err(_) => 2,
    Ok(ra) => match fs.open_lines("__B__") {
      Err(_) => 3,
      Ok(rb) => match fs.open_lines("__C__") {
        Err(_) => 4,
        Ok(rc) => match start(ra, rb, rc) {
          Err(e) => failed(e),
          Ok(n) => match n == 8 {
            true => 0,
            false => 1,
          },
        },
      },
    },
  }
}
"#
    .replace("__A__", &a.display().to_string())
    .replace("__B__", &b.display().to_string())
    .replace("__C__", &c.display().to_string());
    write_file(&root, "prog.glyph", &prog);
    let file = root.join("prog.glyph");
    let (code, stdout, stderr) = spawn_run(&file);
    assert_eq!(
        code, 0,
        "a non-zero code names the failing step: 2..4 open, 5 a read error, 1 the \
         wrong line count\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        stdout, "apple\nbanana\ncherry\nfig\ngrape\nkiwi\nmango\npear\n",
        "the merged sequence must be every line of the three sources in order, \
         with no `\\r` from the CRLF source and the newline-less last line of `c` \
         present\nstderr:\n{stderr}"
    );
}

#[test]
fn fs_next_line_yields_a_last_line_with_no_newline_and_then_stays_at_end() {
    // A file whose last line has no terminator still has that line in it; the
    // earlier `io.read_line` got this right for stdin and the reader has to as
    // well. Past the end the reader is closed and keeps reporting end of input,
    // and an empty file reports it on the first call.
    if !js_toolchain_available() {
        eprintln!("skipping fs next_line run: node/tsx not available");
        return;
    }
    let root = unique_tmp("fslinestail");
    let two = root.join("two.txt");
    let empty = root.join("empty.txt");
    std::fs::write(&two, "one\ntwo").expect("write two");
    std::fs::write(&empty, "").expect("write empty");
    let prog = r#"module prog

import std/fs
import std/io
import std/option { Some, None }
import std/result { Result, Ok, Err }

fn show(step: string, r: fs.LineReader) -> Result<void, fs.FsError> {
  let next = fs.next_line(r)?
  let got = match next {
    Some(line) => "line(${line})",
    None => "end",
  }
  io.println("${step}:${got}")
  return Ok(void)
}

fn walk(two: fs.LineReader, empty: fs.LineReader) -> Result<void, fs.FsError> {
  show("1", two)?
  show("2", two)?
  show("3", two)?
  show("4", two)?
  show("e", empty)?
  return Ok(void)
}

fn main(argv: Array<string>) -> number {
  return match fs.open_lines("__TWO__") {
    Err(_) => 2,
    Ok(two) => match fs.open_lines("__EMPTY__") {
      Err(_) => 3,
      Ok(empty) => match walk(two, empty) {
        Err(_) => 4,
        Ok(_) => 0,
      },
    },
  }
}
"#
    .replace("__TWO__", &two.display().to_string())
    .replace("__EMPTY__", &empty.display().to_string());
    write_file(&root, "prog.glyph", &prog);
    let file = root.join("prog.glyph");
    let (code, stdout, stderr) = spawn_run(&file);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout, "1:line(one)\n2:line(two)\n3:end\n4:end\ne:end\n",
        "stderr:\n{stderr}"
    );
}

#[test]
fn fs_open_lines_reports_a_missing_path_and_a_directory_by_kind() {
    // The two failures a line reader meets, each as a value with a named kind.
    // A missing path fails at `open_lines`, as `read_text` does. A directory
    // opens (POSIX allows an O_RDONLY descriptor on one) and fails on the first
    // read, which is the case the `Result` around `next_line` exists for: were
    // the reader to report that read error as end of input, this program would
    // print `end` and exit 0, and a directory would be indistinguishable from an
    // empty file. `close_lines` twice on the same reader is a no-op, and a
    // reader closed early reports end of input rather than a stale descriptor.
    if !js_toolchain_available() {
        eprintln!("skipping fs open_lines error run: node/tsx not available");
        return;
    }
    let root = unique_tmp("fslineserr");
    let dir = root.join("adir");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let some = root.join("some.txt");
    std::fs::write(&some, "alpha\nbeta\n").expect("write some");
    let missing = root.join("missing.txt");
    let prog = r#"module prog

import std/fs
import std/io
import std/option { Option, Some, None }
import std/result { Result, Ok, Err }

fn kind_name(k: fs.ErrorKind) -> string {
  return match k {
    fs.ErrorKind.NotFound => "NotFound",
    fs.ErrorKind.IsADirectory => "IsADirectory",
    fs.ErrorKind.NotADirectory => "NotADirectory",
    fs.ErrorKind.PermissionDenied => "PermissionDenied",
    fs.ErrorKind.AlreadyExists => "AlreadyExists",
    fs.ErrorKind.Other({ code }) => "Other(${code})",
  }
}

fn describe(r: Result<Option<string>, fs.FsError>) -> string {
  return match r {
    Ok(Some(line)) => "line(${line})",
    Ok(None) => "end",
    Err(e) => "err(${kind_name(e.kind)})",
  }
}

fn main(argv: Array<string>) -> number {
  match fs.open_lines("__MISSING__") {
    Ok(_) => io.println("missing:opened"),
    Err(e) => io.println("missing:${kind_name(e.kind)}"),
  }
  match fs.open_lines("__DIR__") {
    Err(e) => io.println("dir:open:${kind_name(e.kind)}"),
    Ok(r) => io.println("dir:${describe(fs.next_line(r))}"),
  }
  match fs.open_lines("__SOME__") {
    Err(e) => io.println("some:open:${kind_name(e.kind)}"),
    Ok(r) => {
      io.println("some:${describe(fs.next_line(r))}")
      fs.close_lines(r)
      fs.close_lines(r)
      io.println("closed:${describe(fs.next_line(r))}")
    },
  }
  return 0
}
"#
    .replace("__MISSING__", &missing.display().to_string())
    .replace("__DIR__", &dir.display().to_string())
    .replace("__SOME__", &some.display().to_string());
    write_file(&root, "prog.glyph", &prog);
    let file = root.join("prog.glyph");
    let (code, stdout, stderr) = spawn_run(&file);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout,
        "missing:NotFound\ndir:err(IsADirectory)\nsome:line(alpha)\nclosed:end\n",
        "stderr:\n{stderr}"
    );
}

/// `glyph run <file>` as a subprocess, so the program's own stdout is
/// observable. `run_file` reports only the exit code, which is enough for a
/// program that answers with a number and not for one whose output is the
/// claim.
fn spawn_run(file: &Path) -> (i32, String, String) {
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("run"), file.as_os_str()]);
    (code, stdout, stderr)
}

// ---------------------------------------------------------------------------
// G215, G226, G225: an imported declaration is decided like a local one, and
// a namespace import does not switch the checker off for a prelude name.
// ---------------------------------------------------------------------------

/// `glyph check <root> --no-tsc` over a project directory: the exit code and
/// the diagnostic lines. These are the surfaces an agent reads (`check
/// --no-tsc`, the language server, the MCP server), so a program that is red
/// only once `tsc` reads the emitted TypeScript is green to them.
fn check_no_tsc(root: &Path) -> (i32, String) {
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        root.as_os_str(),
        std::ffi::OsStr::new("--no-tsc"),
    ]);
    (code, stderr)
}

#[test]
fn g215_an_imported_union_passed_where_a_string_is_declared_is_e0211() {
    // The ledger's program: `shout(st)` with `st: Status` imported and
    // `fn shout(s: string)`. Exit 0 with no error under 0.1.119; the same
    // program with `Status` declared in the calling module was E0211 since
    // G201.
    let root = unique_tmp("g215");
    write_file(
        &root,
        "lib.glyph",
        "module lib\npub type Status =\n  | Pending\n  | Done\n",
    );
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import lib { Status, Pending }\n\
         fn shout(s: string) -> string {\n  return s\n}\n\
         fn main() {\n  let st: Status = Pending\n  shout(st)\n}\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0211]") && stderr.contains("expected `string`, found `Status`"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn g226_a_local_record_bound_to_a_local_alias_of_an_imported_union_is_e0204() {
    // The ledger's program. `let l: Local = o` was E0204 under 0.1.118, went
    // silent when `Local` started canonicalizing to the imported `Shape`, and
    // is E0204 again by the identity of the two declarations. The other two
    // bindings are one declaration under three names and stay silent, where
    // 0.1.118 called `let b: Local2 = a` a mismatch by spelling.
    let root = unique_tmp("g226");
    write_file(
        &root,
        "lib.glyph",
        "module lib\npub type Shape =\n  | Circle\n  | Square\n",
    );
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import lib { Shape, Circle }\n\
         type Local = Shape\n\
         type Local2 = Shape\n\
         type Other = { x: int }\n\
         fn main() {\n\
         \x20 let o: Other = { x: 1, }\n\
         \x20 let l: Local = o\n\
         \x20 let s: Shape = Circle\n\
         \x20 let a: Local = s\n\
         \x20 let b: Local2 = a\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0204]") && stderr.contains("expected `Local`, found `Other`"),
        "stderr:\n{stderr}"
    );
    assert_eq!(
        stderr.matches("[E0204]").count(),
        1,
        "the two aliases of one declaration must stay silent:\n{stderr}"
    );
}

#[test]
fn g225_a_namespace_import_of_std_string_does_not_hide_a_string_mismatch() {
    // `import std/string` binds `string`; the annotation resolved to the
    // binding and lowered to nothing, so `let x: string = 5` was clean, where
    // the named form `import std/string { trim }` made the same line E0204.
    let root = unique_tmp("g225");
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import std/string\n\
         fn main() {\n\
         \x20 let x: string = 5\n\
         \x20 let y: string = string.trim(\"a\")\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0204]") && stderr.contains("expected `string`, found `number`"),
        "stderr:\n{stderr}"
    );
    assert_eq!(stderr.matches("[E0204]").count(), 1, "stderr:\n{stderr}");
}

#[test]
fn g240_a_jsx_attribute_is_checked_against_the_components_props_record() {
    // The gap. `<Button variant="danger" />`, `<Button variant="nope" />` and
    // `<Button variant={42} />` all exited 0 with no diagnostic under
    // `--no-tsc`: no JSX attribute reached the assignability relation, so the
    // only thing that saw one was `tsc` on the emitted TypeScript, which left
    // `--no-tsc`, the language server, the MCP tools and the playground blind.
    let root = unique_tmp("g240");
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         type Variant = \"primary\" | \"danger\"\n\
         type Props = { variant: Variant, label: string }\n\
         component Button(props: Props) {\n\
         \x20 return <button>{props.label}</button>\n\
         }\n\
         pub component App() {\n\
         \x20 return <Button variant=\"nope\" label=\"go\" />\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0211]") && stderr.contains("expected `Variant`, found `\"nope\"`"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn g240_an_imported_components_props_record_decides_the_same_attribute() {
    // The guarantee may not depend on which module declared the component.
    // The props record reaches the consumer as a `Ty::Imported` and its
    // literal set is read from the declaration there, so the diagnostic names
    // `Variant` under the import exactly as it does at home.
    let root = unique_tmp("g240imported");
    write_file(
        &root,
        "ui.glyph",
        "module ui\n\
         pub type Variant = \"primary\" | \"danger\"\n\
         pub type Props = { variant: Variant, label: string, hint?: string }\n\
         pub component Button(props: Props) {\n\
         \x20 return <button>{props.label}</button>\n\
         }\n",
    );
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import ui { Button }\n\
         pub component App() {\n\
         \x20 return <div>\n\
         \x20   <Button variant={42} label=\"go\" />\n\
         \x20   <Button variant=\"danger\" label=\"go\" bogus=\"x\" />\n\
         \x20 </div>\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0211]") && stderr.contains("expected `Variant`, found `number`"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("[E0210]") && stderr.contains("type `Props` has no field `bogus`"),
        "the imported record keeps its own name in the field diagnostic:\n{stderr}"
    );
}

/// The `ui` module every G242 test imports: one string-literal union and one
/// component whose props record carries it.
const G242_UI: &str = "module ui\n\
     pub type Variant = \"primary\" | \"danger\"\n\
     pub type Props = { variant: Variant, label: string }\n\
     pub component Button(props: Props) {\n\
     \x20 return <button>{props.label}</button>\n\
     }\n";

#[test]
fn g242_a_dotted_element_through_a_namespace_import_is_a_component() {
    // The miscompile. `<ui.Button>` under `import ui` was classified by the
    // first character of its name, so it was an intrinsic: emitted as
    // `React.createElement("ui.Button", ...)`, a string tag `tsc` accepts
    // with any props, so the component never ran. The G240 check never saw
    // it, and the resolver reported `ui` as an unused import. A dotted name
    // is a member access, so it is a component whatever the case of its base.
    let root = unique_tmp("g242ns");
    write_file(&root, "ui.glyph", G242_UI);
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import ui\n\
         pub component App() {\n\
         \x20 return <ui.Button variant=\"nope\" label=\"go\" />\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0211]") && stderr.contains("expected `Variant`, found `\"nope\"`"),
        "the attribute is checked against the props record:\n{stderr}"
    );
    assert!(
        !stderr.contains("[E0106]"),
        "`ui` is used by the element:\n{stderr}"
    );
}

#[test]
fn g242_an_aliased_namespace_import_reaches_the_same_component() {
    // The guarantee may not depend on the import spelling: `import ui as u`
    // and `<u.Button>` is the same declaration as `import ui { Button }` and
    // `<Button>`, and draws the same diagnostic naming the same type.
    let root = unique_tmp("g242alias");
    write_file(&root, "ui.glyph", G242_UI);
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import ui as u\n\
         pub component App() {\n\
         \x20 return <div>\n\
         \x20   <u.Button variant=\"nope\" label=\"go\" />\n\
         \x20   <u.Button variant=\"danger\" label=\"go\" bogus=\"x\" />\n\
         \x20 </div>\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0211]") && stderr.contains("expected `Variant`, found `\"nope\"`"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("[E0210]") && stderr.contains("type `Props` has no field `bogus`"),
        "the imported record keeps its own name:\n{stderr}"
    );
    assert!(!stderr.contains("[E0106]"), "stderr:\n{stderr}");
}

#[test]
fn g242_an_unbound_base_of_a_dotted_element_is_an_unresolved_name() {
    // `nope` names nothing. As an intrinsic it resolved nothing and emitted
    // the string `"nope.Button"`; as the member access it is, its base is a
    // reference like any other.
    let root = unique_tmp("g242unbound");
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         pub component App() {\n\
         \x20 return <nope.Button label=\"go\" />\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("[E0103]") && stderr.contains("nope"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn g242_a_dotted_element_emits_its_tag_as_a_value() {
    // The emitted tag is the member access `ui.Button`, not the string
    // `"ui.Button"`, under both namespace spellings, and a correct program
    // builds clean. An intrinsic stays a string.
    let root = unique_tmp("g242emit");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(&src, "ui.glyph", G242_UI);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import ui\n\
         pub component App() {\n\
         \x20 return <div><ui.Button variant=\"danger\" label=\"go\" /></div>\n\
         }\n",
    );
    write_file(
        &src,
        "other.glyph",
        "module other\n\
         import ui as u\n\
         pub component Other() {\n\
         \x20 return <u.Button variant=\"primary\" label=\"go\" />\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build_project ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    assert!(
        !report.diagnostics.iter().any(|d| d.contains("[E0106]")),
        "diags: {:?}",
        report.diagnostics
    );
    let main = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    assert!(main.contains("React.createElement(ui.Button, "), "{main}");
    assert!(!main.contains("\"ui.Button\""), "{main}");
    assert!(main.contains("React.createElement(\"div\", "), "{main}");
    let other = std::fs::read_to_string(out.join("other.ts")).expect("other.ts written");
    assert!(other.contains("React.createElement(u.Button, "), "{other}");
}

#[test]
fn g240_the_shapes_tsc_accepts_stay_silent() {
    // Run under `tsc --strict` with real `@types/react` before it was written
    // here: every one of these compiles. A spread may carry any prop, so an
    // element holding one is not asked for a missing attribute; children ride
    // as `createElement` varargs rather than as a prop, so a component whose
    // props record declares no `children` takes them; an optional field may be
    // omitted; `key` is React's own attribute; and an intrinsic element's
    // attributes are React's table, not Glyph's.
    let root = unique_tmp("g240ok");
    write_file(
        &root,
        "ui.glyph",
        "module ui\n\
         pub type Variant = \"primary\" | \"danger\"\n\
         pub type Props = { variant: Variant, label: string, hint?: string }\n\
         pub component Button(props: Props) {\n\
         \x20 return <button>{props.label}</button>\n\
         }\n",
    );
    write_file(
        &root,
        "main.glyph",
        "module main\n\
         import ui { Button, Props }\n\
         component Card(props: { title: string }) {\n\
         \x20 return <div>{props.title}</div>\n\
         }\n\
         pub component App(a: { xs: Array<string>, p: Props }) {\n\
         \x20 return <div class=\"x\" data-testid=\"y\">\n\
         \x20   <Button variant=\"danger\" label=\"go\" />\n\
         \x20   <Button variant=\"primary\" label=\"b\" hint=\"h\" key={1} />\n\
         \x20   <Button {...a.p} />\n\
         \x20   <Button {...a.p} variant=\"primary\" />\n\
         \x20   <Card title=\"t\"><span>child</span></Card>\n\
         \x20   <for x in={a.xs}><Card title={x} /></for>\n\
         \x20 </div>\n\
         }\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(
        !stderr.contains("[E02"),
        "no correct shape is refused:\n{stderr}"
    );
}

/// The parts of `@types/react` 18.3 a component build touches, copied in their
/// real shapes: `ReactNode` is the union React renders (an element, text, an
/// iterable of nodes, `null`), a function component returns one, and
/// `createElement` returns an element. No test environment carries React's own
/// types, so this stands in for them; what it must keep faithful is that
/// `ReactElement` refuses `null` and an array while `ReactNode` takes both.
const REACT_TYPES_SUBSET: &str = r#"export interface ReactElement<P = any> {
  type: unknown;
  props: P;
  key: string | null;
}
export type ReactNode =
  | ReactElement
  | string
  | number
  | bigint
  | Iterable<ReactNode>
  | boolean
  | null
  | undefined;
export interface FunctionComponent<P = {}> {
  (props: P): ReactNode;
}
export declare const Fragment: unique symbol;
export declare function createElement(
  type: string | typeof Fragment,
  props?: object | null,
  ...children: ReactNode[]
): ReactElement;
export declare function createElement<P extends {}>(
  type: FunctionComponent<P>,
  props?: P | null,
  ...children: ReactNode[]
): ReactElement<P>;
"#;

/// G247 with `tsc` in the loop. The prelude `Component` used to be written into
/// the TypeScript verbatim, where nothing declares it (`TS2304`), and a module
/// of JSX-returning `fn`s with no `component` never imported React at all. The
/// program covers every shape a component can return: an element, a component
/// element, a fragment, the `null` of a lone `<if>`, an `<if>`/`<else>`
/// ternary, a `<match>` IIFE and a `<for>` array. A lowering to
/// `React.ReactElement` or `React.JSX.Element` fails on the last two.
#[test]
fn g247_the_prelude_component_type_checks_against_react_types() {
    if !tsc_available() {
        eprintln!("skipping G247 tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("g247");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &root,
        "node_modules/@types/react/package.json",
        r#"{ "name": "@types/react", "version": "18.3.0", "types": "index.d.ts" }"#,
    );
    write_file(&root, "node_modules/@types/react/index.d.ts", REACT_TYPES_SUBSET);
    write_file(
        &src,
        "views.glyph",
        "module views\n\
         pub fn view() -> Component {\n\
         \x20 return <p>hi</p>\n\
         }\n\
         pub fn boxed(c: Component, rest: Array<Component>) -> Component {\n\
         \x20 return <div>{c}{rest}</div>\n\
         }\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import views { view, boxed }\n\
         pub type S =\n\
         \x20 | Idle\n\
         \x20 | Loaded({ n: number })\n\
         pub type Slot = { header: Component }\n\
         pub component Inner() -> Component {\n\
         \x20 return <span>x</span>\n\
         }\n\
         pub component Frag() -> Component {\n\
         \x20 return <><Inner /><p>b</p></>\n\
         }\n\
         pub component Lone(props: { flag: bool }) -> Component {\n\
         \x20 return <if cond={props.flag}><p>yes</p></if>\n\
         }\n\
         pub component Both(props: { flag: bool }) -> Component {\n\
         \x20 return <>\n\
         \x20   <if cond={props.flag}><p>yes</p></if>\n\
         \x20   <else><p>no</p></else>\n\
         \x20 </>\n\
         }\n\
         pub component Pick(props: { s: S }) -> Component {\n\
         \x20 return <match value={props.s}>\n\
         \x20   <case Idle><p>idle</p></case>\n\
         \x20   <case Loaded({ n })><p>{n}</p></case>\n\
         \x20 </match>\n\
         }\n\
         pub component Each(props: { xs: Array<string> }) -> Component {\n\
         \x20 return <for x in={props.xs} key={x}><li>{x}</li></for>\n\
         }\n\
         pub component App(props: { slot: Slot }) -> Component {\n\
         \x20 return <div>{view()}{boxed(props.slot.header, [view()])}<Lone flag={true} /><Each xs={[\"a\"]} /></div>\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let views = std::fs::read_to_string(out.join("views.ts")).expect("views.ts written");
    assert!(views.contains("import * as React from \"react\";"), "{views}");
    assert!(views.contains("export function view(): React.ReactNode {"), "{views}");
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("the prelude `Component` did not type-check:\n{msg}"),
        TscOutcome::NotFound => tool_missing("tsc"),
    }
}

/// D6 with `tsc` in the loop: a `<case>` takes a `match` arm's pattern and is
/// lowered as one. A positional payload is read off `value`, a record payload's
/// fields off the flat tag object, a whole record payload is the tag object,
/// and a refutable case joins the arm if-chain. Each lowering has to compile
/// under `tsc --strict`, which is where `bind` failed: it read `__v.items` off
/// a tag object whose payload was under `value`, and tsc rejected the
/// property (G275).
#[test]
fn a_jsx_case_pattern_builds_and_type_checks_against_react_types() {
    if !tsc_available() {
        eprintln!("skipping the case-pattern tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("case_pattern");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &root,
        "node_modules/@types/react/package.json",
        r#"{ "name": "@types/react", "version": "18.3.0", "types": "index.d.ts" }"#,
    );
    write_file(&root, "node_modules/@types/react/index.d.ts", REACT_TYPES_SUBSET);
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         pub type Load =\n\
         \x20 | Loading\n\
         \x20 | Items(Array<string>)\n\
         \x20 | Page({ users: Array<string>, total: number })\n\
         \x20 | Whole({ total: number })\n\
         pub type Resp =\n\
         \x20 | Pending\n\
         \x20 | Done({ status: number, body: string })\n\
         pub component Positional(props: { s: Load }) -> Component {\n\
         \x20 return <match value={props.s}>\n\
         \x20   <case Loading><p>loading</p></case>\n\
         \x20   <case Items(items)><p>{items.length}</p></case>\n\
         \x20   <case Page({ users, total })><p>{users.length} of {total}</p></case>\n\
         \x20   <case Whole(r)><p>{r.total}</p></case>\n\
         \x20 </match>\n\
         }\n\
         pub component Refutable(props: { r: Resp }) -> Component {\n\
         \x20 return <match value={props.r}>\n\
         \x20   <case Pending><p>wait</p></case>\n\
         \x20   <case Done({ status: 404 })><p>missing</p></case>\n\
         \x20   <case Done({ body: b })><p>{b}</p></case>\n\
         \x20 </match>\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts written");
    assert!(ts.contains("const items = __m0.value;"), "{ts}");
    assert!(ts.contains("React.createElement(\"p\", null, items.length)"), "{ts}");
    assert!(ts.contains("const users = __m0.users;"), "{ts}");
    assert!(ts.contains("const total = __m0.total;"), "{ts}");
    assert!(ts.contains("const r = __m0;"), "{ts}");
    assert!(ts.contains("__m0.tag === \"Done\" && __m0.status === 404"), "{ts}");
    assert!(ts.contains("const b = __m0.body;"), "{ts}");
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("a case pattern's lowering did not type-check:\n{msg}"),
        TscOutcome::NotFound => eprintln!("skipping: tsc not found at check time"),
    }
}

/// G243 across a module boundary. A component's parameter type declared in a
/// sibling reaches this module as a `Ty::Imported`, and whether it is a props
/// record is read from the declaration at the end of its chain, so the verdict
/// is the one the same declaration gets locally, under a named and a namespace
/// import alike. The records and the re-exported second name of one draw
/// nothing; a string-literal union, a tagged union and a second name for a
/// primitive are each one E0229 at the parameter.
#[test]
fn an_imported_parameter_type_is_judged_by_its_declaration_for_e0229() {
    let root = unique_tmp("component_param_imported");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "ui.glyph",
        "module ui\n\
         pub type Props = { name: string }\n\
         pub type Rows = Props\n\
         pub type Tier = \"free\" | \"pro\"\n\
         pub type State =\n  | Idle\n  | Busy(number)\n\
         pub type Id = string\n",
    );
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         import ui { Props, Rows, Tier, State, Id }\n\
         import ui\n\
         pub component A(props: Props) {\n  return <p>{props.name}</p>\n}\n\
         pub component B(rows: Rows) {\n  return <p>{rows.name}</p>\n}\n\
         pub component C(qualified: ui.Props) {\n  return <p>{qualified.name}</p>\n}\n\
         pub component D(tier: Tier) {\n  return <p>{\"d\"}</p>\n}\n\
         pub component E(state: State) {\n  return <p>{\"e\"}</p>\n}\n\
         pub component F(id: Id) {\n  return <p>{id}</p>\n}\n\
         pub component G(spelled: ui.Tier) {\n  return <p>{\"g\"}</p>\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build ran");
    let refused: Vec<&String> = report
        .diagnostics
        .iter()
        .filter(|d| d.contains("[E0229]"))
        .collect();
    for param in ["`tier`", "`state`", "`id`", "`spelled`"] {
        assert!(
            refused.iter().any(|d| d.contains(param)),
            "expected E0229 on {param}; got: {:?}",
            report.diagnostics
        );
    }
    assert_eq!(refused.len(), 4, "only the four non-records: {refused:?}");
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.contains("[E02") && !d.contains("[E0229]")),
        "no other typecheck code: {:?}",
        report.diagnostics
    );
}

/// A project whose `extern/hooks.ts` returns a typed tuple, the shape React's
/// `useState` has, and whose `main.glyph` destructures it with `body`. The
/// checker sees the extern function as a value it cannot look into, so the
/// length and the element types are `tsc`'s to prove (D48).
fn destructure_project(tag: &str, body: &str) -> (PathBuf, PathBuf) {
    let root = unique_tmp(tag);
    let src = root.join("src");
    write_file(
        &src,
        "extern/hooks.ts",
        "export function useState<S>(s: S): [S, (v: S) => void] {\n  return [s, (_v: S) => {}];\n}\n",
    );
    write_file(
        &src,
        "main.glyph",
        &format!("module main\n\nimport extern/hooks {{ useState }}\n\nfn main() {{\n{body}\n}}\n"),
    );
    let out = root.join("dist");
    (src, out)
}

#[test]
fn a_destructured_tuple_keeps_its_element_types_under_tsc() {
    // D48. `satisfies readonly [unknown, unknown]` proves the arity without
    // replacing what `tsc` infers, so `count` is a `number` there: assigning it
    // to a `string` is TS2322. Had the clause widened the inference to
    // `unknown`, that line would fail differently (TS2322 against `unknown`)
    // and the clean program below would fail on `count + 1`.
    if !tsc_available() {
        eprintln!("skipping destructuring tsc check: tsc not available");
        return;
    }
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};

    let (src, out) = destructure_project(
        "d48-tsc-clean",
        "  let [count, set_count] = useState(0)\n  set_count(count + 1)\n  let [_, set_name] = useState(\"a\")\n  set_name(\"b\")",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("a correct destructuring failed tsc:\n{msg}"),
        TscOutcome::NotFound => return tool_missing("tsc"),
    }

    let (src, out) = destructure_project(
        "d48-tsc-narrow",
        "  let [count, set_count] = useState(0)\n  let n: string = count\n  set_count(1)\n  print(n)",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Failed(msg) => {
            assert!(
                msg.contains("TS2322") && msg.contains("Type 'number' is not assignable to type 'string'"),
                "expected TS2322 from number to string:\n{msg}"
            );
        }
        other => panic!("`let n: string = count` must be TS2322, got {other:?}"),
    }
}

/// G293 with a generic record. A `Box<T>` whose field type is still a type
/// parameter after substitution is not a `Record<string, string>`: `T` may
/// be anything. Glyph's checker refuses it itself (E0204), so the verdict
/// holds under `--no-tsc`, in the editor and in the playground, as it did in
/// 0.1.131; `tsc` refuses it too (TS2322). `Record<string, unknown>` and
/// `Record<string, T>` accept it.
#[test]
fn a_generic_record_with_an_unsubstituted_field_is_not_a_string_map() {
    let root = unique_tmp("g293-box-t");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &src,
        "main.glyph",
        "module main\n\
         \n\
         type Box<T> = { value: T, }\n\
         \n\
         pub fn f<T>(b: Box<T>) -> Record<string, string> {\n\
         \x20 return b\n\
         }\n\
         \n\
         pub fn g<T>(b: Box<T>) -> Record<string, unknown> {\n\
         \x20 return b\n\
         }\n\
         \n\
         pub fn h<T>(b: Box<T>) -> Record<string, T> {\n\
         \x20 return b\n\
         }\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build runs");
    let d = &report.diagnostics;
    assert_eq!(d.len(), 1, "only `f` is refused: {d:?}");
    assert!(
        d[0].contains("[E0204]") && d[0].contains("expected `Record<string, string>`"),
        "expected E0204 on `return b` in `f`: {d:?}"
    );
}

#[test]
fn a_destructuring_of_the_wrong_arity_is_refused_by_tsc() {
    // The length a checker cannot see is proved by `tsc`: three names over a
    // pair is TS1360 on the emitted `satisfies`.
    if !tsc_available() {
        eprintln!("skipping destructuring arity check: tsc not available");
        return;
    }
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    let (src, out) = destructure_project(
        "d48-tsc-arity",
        "  let [a, b, c] = useState(0)\n  b(a)\n  print(\"${c}\")",
    );
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Failed(msg) => assert!(msg.contains("TS1360"), "expected TS1360:\n{msg}"),
        other => panic!("a triple over a pair must fail tsc, got {other:?}"),
    }
}

#[test]
fn a_destructuring_let_runs_end_to_end() {
    // `glyph run`: a literal counted by Glyph, a nested pattern, a rest, an
    // elision, a tuple from an extern function, and `mut` reassigning a
    // destructured name.
    if !js_toolchain_available() {
        eprintln!("skipping destructuring run: node/tsx not available");
        return;
    }
    let (src, _out) = destructure_project(
        "d48-run",
        "  let [count, set_count] = useState(41)\n  set_count(count)\n  mut count = count + 1\n  let [_, [x, y], ...rest] = [0, [1, 2], 3, 4]\n  print(\"${count} ${x + y} ${rest.length}\")",
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(stdout.trim(), "42 3 2", "{stdout}\n{stderr}");
}

/// A page and a store for a `glyph run` that has no browser. `install` puts a
/// `document` and a `localStorage` on `globalThis`; with `quota_full` set,
/// every write throws the error a full browser store throws, which carries the
/// standard `QuotaExceededError` name.
const FAKE_HOST_TS: &str = r#"export function install(quota_full: boolean): void {
  const items = new Map<string, string>();
  const el = { id: "root" };
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      title: "start",
      getElementById: (id: string) => (id === "root" ? el : null),
      location: { href: "https://example.test:8443/app/page?x=1#top" },
    },
  });
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (k: string) => (items.has(k) ? items.get(k)! : null),
      setItem: (k: string, v: string) => {
        if (quota_full) {
          const e = new Error("the quota has been exceeded");
          e.name = "QuotaExceededError";
          throw e;
        }
        items.set(k, v);
      },
      removeItem: (k: string) => {
        items.delete(k);
      },
    },
  });
}
"#;

/// G254 end to end under node. `std/dom` reads and writes the fake page,
/// `std/storage` round-trips through the fake store, and a store that throws
/// `QuotaExceededError` is an `Err` with `kind: "quota"` rather than a throw.
#[test]
fn g254_the_host_modules_run_against_a_page_and_a_store() {
    if !js_toolchain_available() {
        eprintln!("skipping G254 run: node/tsx not available");
        return;
    }
    let root = unique_tmp("g254_fake_host");
    let src = root.join("src");
    write_file(&src, "extern/fakehost.ts", FAKE_HOST_TS);
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/dom
import std/storage
import std/console
import extern/fakehost { install }

fn main(argv: Array<string>) -> number {
  install(false)
  dom.set_title("hello")
  console.log("title=${dom.title()}")
  match dom.element_by_id("root") {
    Some(_) => console.log("root=Some"),
    None => console.log("root=None"),
  }
  match dom.element_by_id("nope") {
    Some(_) => console.log("nope=Some"),
    None => console.log("nope=None"),
  }
  let u = dom.location()
  console.log("host=${u.host} path=${u.path}")
  match storage.set("k", "v1") {
    Ok(_) => console.log("set=Ok"),
    Err(e) => console.log("set=Err(${e.kind})"),
  }
  match storage.get("k") {
    Some(v) => console.log("get=${v}"),
    None => console.log("get=None"),
  }
  storage.remove("k")
  match storage.get("k") {
    Some(v) => console.log("after=${v}"),
    None => console.log("after=None"),
  }
  install(true)
  match storage.set("k", "v2") {
    Ok(_) => console.log("full=Ok"),
    Err(e) => console.log("full=Err(${e.kind}): ${e.message}"),
  }
  console.error("to-stderr")
  return 0
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run:\n{stdout}\n{stderr}");
    for want in [
        "title=hello",
        "root=Some",
        "nope=None",
        "host=example.test path=/app/page",
        "set=Ok",
        "get=v1",
        "after=None",
        "full=Err(quota): std/storage.set: QuotaExceededError: the quota has been exceeded",
    ] {
        assert!(stdout.contains(want), "missing `{want}` in:\n{stdout}\n{stderr}");
    }
    assert!(stderr.contains("to-stderr"), "console.error goes to stderr:\n{stderr}");
    assert!(!stdout.contains("to-stderr"), "console.error goes to stderr only:\n{stdout}");
}

/// Importing `std/dom` is safe anywhere: nothing in it touches the host at
/// load time, so a module that imports it runs under node as long as it does
/// not call it. Storage with no `localStorage` degrades instead of throwing.
#[test]
fn g254_importing_std_dom_under_node_is_safe_and_storage_degrades() {
    if !js_toolchain_available() {
        eprintln!("skipping G254 run: node/tsx not available");
        return;
    }
    let root = unique_tmp("g254_no_page");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main

import std/dom
import std/storage

pub fn title_line() -> string {
  return "title: ${dom.title()}"
}

fn main(argv: Array<string>) -> number {
  match storage.set("k", "v") {
    Ok(_) => print("set=Ok"),
    Err(e) => print("set=Err(${e.kind})"),
  }
  match storage.get("k") {
    Some(v) => print("get=${v}"),
    None => print("get=None"),
  }
  storage.remove("k")
  return 0
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "importing std/dom must not fail under node:\n{stdout}\n{stderr}");
    assert!(stdout.contains("set=Err(unavailable)"), "{stdout}\n{stderr}");
    assert!(stdout.contains("get=None"), "{stdout}\n{stderr}");
}

/// A call into `std/dom` where there is no document throws, and the message
/// names the function and says why, rather than a `TypeError` about reading
/// `title` of `undefined`.
#[test]
fn g254_a_dom_call_with_no_page_fails_naming_the_function() {
    if !js_toolchain_available() {
        eprintln!("skipping G254 run: node/tsx not available");
        return;
    }
    let root = unique_tmp("g254_title_no_page");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\nimport std/dom\n\nfn main(argv: Array<string>) -> number {\n  print(dom.title())\n  return 0\n}\n",
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_ne!(code, 0, "a dom call with no document must fail:\n{stdout}\n{stderr}");
    assert!(
        stderr.contains("std/dom.title: this runtime has no browser document"),
        "the failure names the function and the reason:\n{stderr}"
    );
}

/// The shape of `react-dom/client` 18.3's `createRoot`, copied from
/// `@types/react-dom`: the container is the DOM's own `Element |
/// DocumentFragment`. `render` takes `unknown` here where React's takes a
/// `ReactNode`, which this test does not exercise. No test environment carries
/// React's types, so this stands in for them the way `REACT_TYPES_SUBSET` does.
const REACT_DOM_CLIENT_SUBSET: &str = r#"export interface RootOptions {
  identifierPrefix?: string;
}
export interface Root {
  render(children: unknown): void;
  unmount(): void;
}
export function createRoot(container: Element | DocumentFragment, options?: RootOptions): Root;
"#;

/// G254 with `tsc` in the loop. `dom.Element` is the host's `HTMLElement` in
/// the TypeScript, which is the reason it is not a branded opaque type: the
/// element `dom.element_by_id` hands back is accepted by React's own
/// `createRoot`. The second half proves the fixture is checking: a string in
/// the same position is a `tsc` error.
#[test]
fn g254_a_dom_element_is_accepted_by_create_root() {
    if !tsc_available() {
        eprintln!("skipping G254 tsc check: tsc not available");
        return;
    }
    let root = unique_tmp("g254_create_root");
    let src = root.join("src");
    let out = root.join("dist");
    write_file(
        &root,
        "node_modules/@types/react-dom/package.json",
        r#"{ "name": "@types/react-dom", "version": "18.3.0", "types": "index.d.ts" }"#,
    );
    write_file(&root, "node_modules/@types/react-dom/index.d.ts", "export {};\n");
    write_file(&root, "node_modules/@types/react-dom/client.d.ts", REACT_DOM_CLIENT_SUBSET);
    let program = |arg: &str| {
        format!(
            "module main\n\n\
             import std/dom\n\
             import react-dom/client {{ createRoot }}\n\n\
             pub fn mount() -> void {{\n\
             \x20 match dom.element_by_id(\"root\") {{\n\
             \x20   Some(root) => createRoot({arg}).render(\"hi\"),\n\
             \x20   None => print(\"no root\"),\n\
             \x20 }}\n\
             }}\n"
        )
    };
    write_file(&src, "main.glyph", &program("root"));
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(msg) => panic!("createRoot(dom.Element) did not type-check:\n{msg}"),
        TscOutcome::NotFound => {
            tool_missing("tsc");
            return;
        }
    }

    write_file(&src, "main.glyph", &program("\"root\""));
    let report = build_project_inner(&src, &out, false).expect("build ok");
    assert!(!report.has_errors(), "diags: {:?}", report.diagnostics);
    match check_with_tsc(&out).expect("run tsc") {
        TscOutcome::Failed(msg) => assert!(
            msg.contains("Element | DocumentFragment"),
            "the refusal is about the container type:\n{msg}"
        ),
        TscOutcome::Passed => panic!("a string passed to createRoot should fail tsc"),
        TscOutcome::NotFound => tool_missing("tsc"),
    }
}

/// G256 under `glyph check --no-tsc`, the surface that used to exit 0: an
/// import of a `std/` module that does not exist is E0104 before `tsc` runs.
#[test]
fn g256_an_import_of_a_missing_std_module_is_e0104_without_tsc() {
    let root = unique_tmp("g256_no_tsc");
    write_file(
        &root,
        "main.glyph",
        "module main\n\nimport std/zzznope { thing }\n\nfn main(argv: Array<string>) -> number {\n  thing()\n  return 0\n}\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_ne!(code, 0, "a missing std module must fail the Glyph stages:\n{stderr}");
    assert!(stderr.contains("[E0104]"), "{stderr}");
    assert!(stderr.contains("std/zzznope"), "{stderr}");
    assert!(!stderr.contains("TS2307"), "the Glyph stage reports it, not tsc:\n{stderr}");
}

#[test]
fn json_parse_with_a_written_type_validates_in_both_spellings_at_run_time() {
    // G297's open half. `import std/json { parse }` then `parse<Doc>(text)`
    // was never rewritten, so it cast: `parse<Doc>("{}")` returned `Ok` on
    // a document with no `title`. And in either spelling a `T` with no
    // descriptor of its own (a primitive, `Option<User>`, an alias of
    // `Array`) cast too. Each is now checked the way a record field of that
    // type is, and the program is run so the arm taken is the evidence.
    let root = unique_tmp("g297-named-run");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        r#"module main
import std/io { println }
import std/json { parse }
import std/json as j
pub type Doc = { title: string, }
pub type User = { name: string, }
pub type Rows = Array<User>
fn say(label: string, ok: bool) {
  println(label + match ok { true => " Ok", false => " Err", })
}
fn main() {
  say("named Doc {}", match parse<Doc>("{}") { Ok(_) => true, Err(_) => false, })
  say("named Doc good", match parse<Doc>("{\"title\": \"t\"}") { Ok(_) => true, Err(_) => false, })
  say("named number str", match parse<number>("\"x\"") { Ok(_) => true, Err(_) => false, })
  say("named int 2.5", match parse<int>("2.5") { Ok(_) => true, Err(_) => false, })
  say("named int 3", match parse<int>("3") { Ok(_) => true, Err(_) => false, })
  say("named unknown", match parse<unknown>("\"x\"") { Ok(_) => true, Err(_) => false, })
  say("ns number str", match j.parse<number>("\"x\"") { Ok(_) => true, Err(_) => false, })
  say("ns option bad", match j.parse<Option<User>>("{\"tag\": \"Some\", \"value\": {}}") { Ok(_) => true, Err(_) => false, })
  say("ns option good", match j.parse<Option<User>>("{\"tag\": \"Some\", \"value\": {\"name\": \"a\"}}") { Ok(_) => true, Err(_) => false, })
  say("named nullable null", match parse<Nullable<User>>("null") { Ok(_) => true, Err(_) => false, })
  say("named record bad", match parse<Record<string, int>>("{\"a\": \"x\"}") { Ok(_) => true, Err(_) => false, })
  say("named rows bad", match parse<Rows>("[{\"name\": 1}]") { Ok(_) => true, Err(_) => false, })
  say("ns rows good", match j.parse<Rows>("[{\"name\": \"a\"}]") { Ok(_) => true, Err(_) => false, })
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            "named Doc {} Err",
            "named Doc good Ok",
            "named number str Err",
            "named int 2.5 Err",
            "named int 3 Ok",
            "named unknown Ok",
            "ns number str Err",
            "ns option bad Err",
            "ns option good Ok",
            "named nullable null Ok",
            "named record bad Err",
            "named rows bad Err",
            "ns rows good Ok",
        ],
        "each document is checked against its written type: {stdout}"
    );
}

/// `json.parse<T>` never casts, the run-time half. A container of `unknown`
/// is checked by its shape: `json.parse<Array<unknown>>("{}")` returned `Ok`
/// and handed the program an object it then treated as an array. An
/// `Array` alias imported from a sibling validates as the local alias does,
/// against the sibling's own record even where this module declares a record
/// of the same name; it used to cast, so `[{"name": 1}]` came back `Ok` as a
/// list of users. And a record holding an `Array<unknown>` is no longer
/// refused, since its field has a complete check. The program is run so the
/// arm taken is the evidence.
#[test]
fn json_parse_checks_a_container_of_unknown_and_an_imported_array_alias_at_run_time() {
    let root = unique_tmp("g297-never-casts-run");
    let src = root.join("src");
    write_file(
        &src,
        "rows.glyph",
        "module rows\npub type User = { name: string, }\npub type Rows = Array<User>\n",
    );
    write_file(
        &src,
        "main.glyph",
        r#"module main
import std/io { println }
import std/json
import std/json { parse }
import rows { Rows }
import rows as r
type User = { id: number, }
type Bag = { items: Array<unknown>, }
fn say(label: string, ok: bool) {
  println(label + match ok { true => " Ok", false => " Err", })
}
fn main() {
  say("arr-unknown object", match json.parse<Array<unknown>>("{}") { Ok(_) => true, Err(_) => false, })
  say("arr-unknown array", match json.parse<Array<unknown>>("[1, \"a\"]") { Ok(_) => true, Err(_) => false, })
  say("rec-unknown array", match parse<Record<string, unknown>>("[1]") { Ok(_) => true, Err(_) => false, })
  say("rec-unknown null", match parse<Record<string, unknown>>("null") { Ok(_) => true, Err(_) => false, })
  say("rec-unknown object", match parse<Record<string, unknown>>("{\"a\": 1}") { Ok(_) => true, Err(_) => false, })
  say("arr-rec numbers", match json.parse<Array<Record<string, unknown>>>("[1, 2]") { Ok(_) => true, Err(_) => false, })
  say("arr-rec objects", match json.parse<Array<Record<string, unknown>>>("[{}, {\"a\": 1}]") { Ok(_) => true, Err(_) => false, })
  say("opt-unknown bad tag", match json.parse<Option<unknown>>("{\"tag\": \"Nope\"}") { Ok(_) => true, Err(_) => false, })
  say("opt-unknown some", match json.parse<Option<unknown>>("{\"tag\": \"Some\", \"value\": 3}") { Ok(_) => true, Err(_) => false, })
  say("bag items number", match json.parse<Bag>("{\"items\": 3}") { Ok(_) => true, Err(_) => false, })
  say("bag items array", match json.parse<Bag>("{\"items\": [1, null]}") { Ok(_) => true, Err(_) => false, })
  say("imported rows bad", match json.parse<Rows>("[{\"name\": 1}]") { Ok(_) => true, Err(_) => false, })
  say("imported rows local shape", match parse<Rows>("[{\"id\": 1}]") { Ok(_) => true, Err(_) => false, })
  say("imported rows good", match parse<Rows>("[{\"name\": \"a\"}]") { Ok(_) => true, Err(_) => false, })
  say("ns rows bad", match json.parse<r.Rows>("[{\"name\": 1}]") { Ok(_) => true, Err(_) => false, })
  say("ns rows good", match json.parse<r.Rows>("[{\"name\": \"a\"}]") { Ok(_) => true, Err(_) => false, })
}
"#,
    );
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            "arr-unknown object Err",
            "arr-unknown array Ok",
            "rec-unknown array Err",
            "rec-unknown null Err",
            "rec-unknown object Ok",
            "arr-rec numbers Err",
            "arr-rec objects Ok",
            "opt-unknown bad tag Err",
            "opt-unknown some Ok",
            "bag items number Err",
            "bag items array Ok",
            "imported rows bad Err",
            "imported rows local shape Err",
            "imported rows good Ok",
            "ns rows bad Err",
            "ns rows good Ok",
        ],
        "each document is checked against its written type: {stdout}"
    );
}

/// `json.parse<T>` never casts, the compile-time half. A `T` nothing can
/// check at run time (a type parameter, an `extern_ts` type) is E0304 in both
/// import spellings. It used to lower to the casting `parse<T>`, so
/// `read<number>("\"x\"")` returned `Ok` holding a string, and the program
/// failed later and somewhere else. `json.parse(text)` with no type argument
/// stays the unchecked read.
#[test]
fn json_parse_of_a_type_with_no_runtime_check_is_e0304_in_both_spellings() {
    let cases = [
        (
            "typeparam-ns",
            "import std/json\nfn read<T>(body: string) -> Result<T, Array<Issue>> {\n  return json.parse<T>(body)\n}\n",
        ),
        (
            "typeparam-named",
            "import std/json { parse }\nfn read<T>(body: string) -> Result<T, Array<Issue>> {\n  return parse<T>(body)\n}\n",
        ),
        (
            "extern-ns",
            "import std/json\ntype Handle = extern_ts(\"{ fd: number }\")\nfn read(body: string) -> Result<Handle, Array<Issue>> {\n  return json.parse<Handle>(body)\n}\n",
        ),
        (
            "extern-named",
            "import std/json { parse }\ntype Handle = extern_ts(\"{ fd: number }\")\nfn read(body: string) -> Result<Handle, Array<Issue>> {\n  return parse<Handle>(body)\n}\n",
        ),
    ];
    for (dir, body) in cases {
        let root = unique_tmp(&format!("g297-refuse-{dir}"));
        write_file(&root, "main.glyph", &format!("module main\n{body}"));
        let (code, stderr) = check_no_tsc(&root);
        assert_ne!(code, 0, "{dir}: a `T` with no runtime check must be refused:\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains("json.parse_with"), "{dir}: the help names the checked read: {stderr}");
    }

    // Positive control: the unchecked read with no type argument is still
    // accepted, as is a container of `unknown`.
    let root = unique_tmp("g297-refuse-control");
    write_file(
        &root,
        "main.glyph",
        "module main\nimport std/json\n\
         pub fn raw(body: string) -> Result<unknown, Array<Issue>> {\n  return json.parse(body)\n}\n\
         pub fn arr(body: string) -> Result<Array<unknown>, Array<Issue>> {\n  return json.parse<Array<unknown>>(body)\n}\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_eq!(code, 0, "the unchecked read and a container of `unknown` are accepted:\n{stderr}");
}

/// A `main` that runs `json.parse<Rows>` on a document shaped like a
/// `{ id }` record and on one shaped like a `{ name }` record, printing the
/// arm each took. `extra` is spliced in after the imports.
fn sibling_alias_main(extra: &str) -> String {
    format!(
        r#"module main
import std/io {{ println }}
import std/json
import rows {{ Rows }}
{extra}
fn say(label: string, ok: bool) {{
  println(label + match ok {{ true => " Ok", false => " Err", }})
}}
fn main() {{
  say("id shape", match json.parse<Rows>("[{{\"id\": 1}}]") {{ Ok(_) => true, Err(_) => false, }})
  say("name shape", match json.parse<Rows>("[{{\"name\": \"a\"}}]") {{ Ok(_) => true, Err(_) => false, }})
}}
"#
    )
}

/// Run `src/main.glyph` and return its stdout lines, failing on a non-zero
/// exit. `glyph run` builds through `tsc --strict` first, so a passing run
/// also says the emitted hidden imports type-check.
fn run_main_lines(src: &Path) -> Vec<String> {
    let entry = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("run"), entry.as_os_str()]);
    assert_eq!(code, 0, "the program should run: {stdout}\n{stderr}");
    stdout.lines().map(str::to_string).collect()
}

/// `rows` imports `User` from `model` and exports `Rows = Array<User>`. The
/// name `User` in that body is `model`'s record, and `json.parse<Rows>`
/// follows it there through the hidden namespace import (G326). Until
/// 0.1.133 it was read in the importer's scope: against `main`'s own `{ id }`
/// record when `main` declared one, so the wrong document came back `Ok`;
/// 0.1.133 to 0.1.139 refused it with E0304. The `{ id }` document fits
/// `main`'s `User` and not `model`'s, so `id shape Err` is the evidence the
/// check is `model`'s.
#[test]
fn json_parse_follows_a_sibling_alias_into_the_module_it_imports_a_record_from() {
    for (dir, extra) in [("with-local", "type User = { id: number, }\n"), ("without-local", "")] {
        let root = unique_tmp(&format!("g326-import-{dir}"));
        let src = root.join("src");
        write_file(&src, "model.glyph", "module model\npub type User = { name: string, }\n");
        write_file(&src, "rows.glyph", "module rows\nimport model { User }\npub type Rows = Array<User>\n");
        write_file(&src, "main.glyph", &sibling_alias_main(extra));
        assert_eq!(
            run_main_lines(&src),
            vec!["id shape Err", "name shape Ok"],
            "{dir}: `Rows` validates against `model`'s `User`"
        );
    }
}

/// The same through the sibling's namespace import: `rows` writes
/// `Array<u.User>` with `import model as u`. `main` binds its own `u` to a
/// different module, whose `User` is the `{ id }` record; the body's `u` is
/// `rows`'s, so the check is `model`'s.
#[test]
fn json_parse_follows_a_sibling_alias_through_its_namespace_import() {
    let root = unique_tmp("g326-ns");
    let src = root.join("src");
    write_file(&src, "model.glyph", "module model\npub type User = { name: string, }\n");
    write_file(&src, "other.glyph", "module other\npub type User = { id: number, }\n");
    write_file(&src, "rows.glyph", "module rows\nimport model as u\npub type Rows = Array<u.User>\n");
    write_file(&src, "main.glyph", &sibling_alias_main("import other as u\nfn touch(x: u.User) -> number { return x.id }\n"));
    assert_eq!(run_main_lines(&src), vec!["id shape Err", "name shape Ok"]);
}

/// A hop that leads back into the importer: `rows` imports `User` from
/// `main`, which imports `Rows` from `rows`. The check names `main`'s own
/// descriptor by its name, with no import of `main` from itself.
#[test]
fn json_parse_follows_a_sibling_alias_back_into_the_importing_module() {
    let root = unique_tmp("g326-loop");
    let src = root.join("src");
    write_file(&src, "rows.glyph", "module rows\nimport main { User }\npub type Rows = Array<User>\n");
    write_file(&src, "main.glyph", &sibling_alias_main("pub type User = { name: string, }\n"));
    assert_eq!(run_main_lines(&src), vec!["id shape Err", "name shape Ok"]);
    let out = root.join("out");
    let (code, _, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("build"),
        std::ffi::OsStr::new("--out"),
        out.as_os_str(),
        src.as_os_str(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let main_ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts");
    assert!(main_ts.contains("User.schema.array()"), "{main_ts}");
    assert!(!main_ts.contains("__glyph_mod_main"), "no import of a module from itself: {main_ts}");
}

/// A chain of hops: `rows` names `mid`'s `Items`, an alias whose body names
/// `item`'s `Item`. Each name is read in the module whose body holds it.
#[test]
fn json_parse_follows_a_sibling_alias_through_two_modules() {
    let root = unique_tmp("g326-chain");
    let src = root.join("src");
    write_file(&src, "item.glyph", "module item\npub type User = { name: string, }\n");
    write_file(&src, "mid.glyph", "module mid\nimport item { User }\npub type Users = Array<User>\n");
    write_file(&src, "rows.glyph", "module rows\nimport mid { Users }\npub type Rows = Array<Users>\n");
    write_file(
        &src,
        "main.glyph",
        r#"module main
import std/io { println }
import std/json
import rows { Rows }
type User = { id: number, }
fn say(label: string, ok: bool) {
  println(label + match ok { true => " Ok", false => " Err", })
}
fn main() {
  say("id shape", match json.parse<Rows>("[[{\"id\": 1}]]") { Ok(_) => true, Err(_) => false, })
  say("flat", match json.parse<Rows>("[{\"name\": \"a\"}]") { Ok(_) => true, Err(_) => false, })
  say("name shape", match json.parse<Rows>("[[{\"name\": \"a\"}], []]") { Ok(_) => true, Err(_) => false, })
}
"#,
    );
    assert_eq!(run_main_lines(&src), vec!["id shape Err", "flat Err", "name shape Ok"]);
}

/// A generic record applied in a sibling's alias body is the sibling's
/// generic, applied to the sibling's argument: declared there, imported there
/// by name, or reached there through a namespace. `main` declares a `Page`
/// and a `User` of its own, with other fields, so a document shaped like
/// `main`'s types is the evidence each check is the sibling's.
#[test]
fn json_parse_follows_a_generic_in_a_sibling_alias_into_its_declaring_module() {
    let bodies = [
        (
            "declared",
            None,
            "module rows\npub type User = { name: string, }\npub type Page<T> = { items: Array<T>, }\npub type Rows = Array<Page<User>>\n",
        ),
        (
            "imported",
            Some("module model\npub type User = { name: string, }\npub type Page<T> = { items: Array<T>, }\n"),
            "module rows\nimport model { Page, User }\npub type Rows = Array<Page<User>>\n",
        ),
        (
            "namespaced",
            Some("module model\npub type User = { name: string, }\npub type Page<T> = { items: Array<T>, }\n"),
            "module rows\nimport model as m\npub type Rows = Array<m.Page<m.User>>\n",
        ),
    ];
    for (dir, model, rows) in bodies {
        let root = unique_tmp(&format!("g326-generic-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", model);
        }
        write_file(&src, "rows.glyph", rows);
        write_file(
            &src,
            "main.glyph",
            r#"module main
import std/io { println }
import std/json
import rows { Rows }
type User = { id: number, }
type Page<T> = { other: T, }
fn touch(p: Page<User>) -> number { return p.other.id }
fn say(label: string, ok: bool) {
  println(label + match ok { true => " Ok", false => " Err", })
}
fn main() {
  say("main page", match json.parse<Rows>("[{\"other\": {\"id\": 1}}]") { Ok(_) => true, Err(_) => false, })
  say("main user", match json.parse<Rows>("[{\"items\": [{\"id\": 1}]}]") { Ok(_) => true, Err(_) => false, })
  say("sibling", match json.parse<Rows>("[{\"items\": [{\"name\": \"a\"}]}]") { Ok(_) => true, Err(_) => false, })
}
"#,
        );
        assert_eq!(
            run_main_lines(&src),
            vec!["main page Err", "main user Err", "sibling Ok"],
            "{dir}: the generic and its argument are the sibling's"
        );
    }
}

/// A record field typed by such an alias: `Doc.parse` checks the field
/// against `model`'s `User`, where it was refused (E0304) from 0.1.133.
#[test]
fn a_descriptor_over_a_sibling_alias_naming_an_imported_record_checks_it() {
    let root = unique_tmp("g326-descriptor");
    let src = root.join("src");
    write_file(&src, "model.glyph", "module model\npub type User = { name: string, }\n");
    write_file(&src, "rows.glyph", "module rows\nimport model { User }\npub type Rows = Array<User>\n");
    write_file(
        &src,
        "main.glyph",
        r#"module main
import std/io { println }
import std/json
import rows { Rows }
type User = { id: number, }
type Doc = { rows: Rows, }
fn say(label: string, ok: bool) {
  println(label + match ok { true => " Ok", false => " Err", })
}
fn main() {
  say("id shape", match json.parse<Doc>("{\"rows\": [{\"id\": 1}]}") { Ok(_) => true, Err(_) => false, })
  say("name shape", match json.parse<Doc>("{\"rows\": [{\"name\": \"a\"}]}") { Ok(_) => true, Err(_) => false, })
}
"#,
    );
    assert_eq!(run_main_lines(&src), vec!["id shape Err", "name shape Ok"]);
}

/// A `main` that runs `json.parse<Rows>` on one document and prints the arm.
fn sibling_generic_main(doc: &str) -> String {
    format!(
        r#"module main
import std/io {{ println }}
import std/json
import rows {{ Rows }}
fn main() {{
  println(match json.parse<Rows>("{doc}") {{ Ok(_) => "Ok", Err(_) => "Err", }})
}}
"#
    )
}

/// The generic hop judges the generic record's own fields in the module
/// that declares it, its type parameters checked by the arguments. A field
/// with no runtime check (`h: H`, an `extern_ts` type) makes the hop E0304
/// naming that field: the descriptor's `is` would test `h` for presence
/// only, and `{"items": [], "h": 5}` came back `Ok` (record 101, probes p3
/// and p4). The same field reached through a nested generic, and through a
/// generic field of the generic, is refused the same way.
#[test]
fn json_parse_refuses_a_sibling_generic_with_an_unchecked_field_of_its_own() {
    let model = "module model\npub type H = extern_ts(\"{ k: string }\")\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, h: H, }\n";
    let page_h = "`Rows`'s body applies `Page`, whose field `h` has type `H`, which has no runtime check";
    let cases = [
        ("p3-imported", Some(model), "module rows\nimport model { Page, User }\npub type Rows = Page<User>\n", page_h),
        (
            "p4-declared",
            None,
            "module rows\npub type H = extern_ts(\"{ k: string }\")\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, h: H, }\npub type Rows = Page<User>\n",
            page_h,
        ),
        ("namespaced", Some(model), "module rows\nimport model as m\npub type Rows = Array<m.Page<m.User>>\n", page_h),
        ("nested", Some(model), "module rows\nimport model { Page, User }\npub type Rows = Page<Page<User>>\n", page_h),
        (
            "generic-field",
            Some("module model\npub type H = extern_ts(\"{ k: string }\")\npub type User = { id: number, }\npub type Meta<T> = { v: T, h: H, }\npub type Page<T> = { items: Array<T>, meta: Meta<T>, }\n"),
            "module rows\nimport model { Page, User }\npub type Rows = Page<User>\n",
            "`Rows`'s body applies `Meta`, whose field `h` has type `H`, which has no runtime check",
        ),
        (
            "interface-field",
            Some("module model\npub interface Shape { k: string, }\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, s: Shape, }\n"),
            "module rows\nimport model { Page, User }\npub type Rows = Page<User>\n",
            "`Rows`'s body applies `Page`, whose field `s` has type `Shape`, which is an interface and has no descriptor",
        ),
    ];
    for (dir, model, rows, expected) in cases {
        let root = unique_tmp(&format!("g326-generic-field-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", model);
        }
        write_file(&src, "rows.glyph", rows);
        write_file(&src, "main.glyph", &sibling_generic_main("{\\\"items\\\": [], \\\"h\\\": 5}"));
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{dir}: a generic with an unchecked field must be refused:\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(expected), "{dir}: the message names `{expected}`: {stderr}");
    }

    // A record field typed by such an alias names the same cause.
    let root = unique_tmp("g326-generic-field-descriptor");
    let src = root.join("src");
    write_file(&src, "model.glyph", model);
    write_file(&src, "rows.glyph", "module rows\nimport model { Page, User }\npub type Rows = Page<User>\n");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport rows { Rows }\ntype Doc = { rows: Rows, }\npub fn read(v: unknown) -> Result<Doc, Array<Issue>> {\n  return Doc.parse(v)\n}\n",
    );
    let (code, stderr) = check_no_tsc(&src);
    assert_ne!(code, 0, "{stderr}");
    assert!(
        stderr.contains("field `rows` has type `Rows`, and `Rows`'s body applies `Page`, whose field `h` has type `H`, which has no runtime check"),
        "{stderr}"
    );
}

/// The generic hop with every field checkable, nested: `Page<Page<User>>`
/// and a generic whose field is another generic of the same module. Run so
/// the arm taken is the evidence each level is checked.
#[test]
fn json_parse_follows_a_nested_sibling_generic_whose_fields_are_all_checked() {
    let model = "module model\npub type User = { name: string, }\npub type Meta<T> = { v: T, n: int, }\npub type Page<T> = { items: Array<T>, meta: Meta<T>, }\n";
    let cases = [
        (
            "nested",
            "module rows\nimport model { Page, User }\npub type Rows = Page<Page<User>>\n",
            [
                ("{\\\"items\\\": [{\\\"items\\\": [{\\\"name\\\": \\\"a\\\"}], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"b\\\"}, \\\"n\\\": 1}}], \\\"meta\\\": {\\\"v\\\": {\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"c\\\"}, \\\"n\\\": 2}}, \\\"n\\\": 3}}", "Ok"),
                ("{\\\"items\\\": [{\\\"items\\\": [{\\\"id\\\": 1}], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"b\\\"}, \\\"n\\\": 1}}], \\\"meta\\\": {\\\"v\\\": {\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"c\\\"}, \\\"n\\\": 2}}, \\\"n\\\": 3}}", "Err"),
                ("{\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"c\\\"}, \\\"n\\\": 2.5}}, \\\"n\\\": 3}}", "Err"),
            ],
        ),
        (
            "namespaced",
            "module rows\nimport model as m\npub type Rows = m.Page<m.User>\n",
            [
                ("{\\\"items\\\": [{\\\"name\\\": \\\"a\\\"}], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"b\\\"}, \\\"n\\\": 1}}", "Ok"),
                ("{\\\"items\\\": [{\\\"name\\\": \\\"a\\\"}], \\\"meta\\\": {\\\"v\\\": {\\\"id\\\": 1}, \\\"n\\\": 1}}", "Err"),
                ("{\\\"items\\\": [{\\\"name\\\": \\\"a\\\"}], \\\"meta\\\": {\\\"v\\\": {\\\"name\\\": \\\"b\\\"}, \\\"n\\\": 1.5}}", "Err"),
            ],
        ),
    ];
    for (dir, rows, docs) in cases {
        for (i, (doc, want)) in docs.iter().enumerate() {
            let root = unique_tmp(&format!("g326-generic-nested-{dir}-{i}"));
            let src = root.join("src");
            write_file(&src, "model.glyph", model);
            write_file(&src, "rows.glyph", rows);
            write_file(&src, "main.glyph", &sibling_generic_main(doc));
            assert_eq!(run_main_lines(&src), vec![want.to_string()], "{dir} document {i}");
        }
    }
}

/// What the hop reaches is judged where it is declared: an interface, a
/// generic union and an `extern_ts` type in the module `rows` imports from
/// are still E0304, each with its own cause, and a type from outside the
/// project (`std/url`) is E0304 naming that. None of them is ever read in
/// `main`'s scope, where `main` declares a checkable `User` of each name.
#[test]
fn json_parse_refuses_what_a_sibling_alias_hop_reaches_with_no_check() {
    let cases = [
        (
            "interface",
            "module model\npub interface User { name: string, }\n",
            "module rows\nimport model { User }\npub type Rows = Array<User>\n",
            "an interface has no descriptor",
        ),
        (
            "generic-union",
            "module model\npub type Box<T> = | Full(T) | Empty\n",
            "module rows\nimport model { Box }\npub type Rows = Array<Box<string>>\n",
            "a generic union has no descriptor",
        ),
        (
            "extern",
            "module model\npub type User = extern_ts(\"{ name: string }\")\n",
            "module rows\nimport model { User }\npub type Rows = Array<User>\n",
            "cannot validate `Rows`: `json.parse<Rows>` has no runtime check to run for it",
        ),
        (
            "outside",
            "module model\npub type Unused = { a: number, }\n",
            "module rows\nimport std/url\npub type Rows = Array<url.Url>\n",
            "`Rows`'s body names `url.Url`, which `rows` imports from outside the project",
        ),
    ];
    for (dir, model, rows, expected) in cases {
        let root = unique_tmp(&format!("g326-refuse-{dir}"));
        let src = root.join("src");
        write_file(&src, "model.glyph", model);
        write_file(&src, "rows.glyph", rows);
        write_file(&src, "main.glyph", &sibling_alias_main("type User = { id: number, }\ntype Box<T> = { v: T, }\nfn touch(b: Box<User>) -> number { return b.v.id }\n"));
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{dir}: must be refused:\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(expected), "{dir}: the message names `{expected}`: {stderr}");
        assert!(
            !stderr.contains("does not yet follow"),
            "{dir}: the hop is made, so nothing may say it is not: {stderr}"
        );
    }
}

/// A record the sibling declares without `pub` cannot be reached through the
/// sibling's exports, so the alias over it is refused rather than read in
/// `main`'s scope, where `main`'s own `User` answered for it.
#[test]
fn json_parse_of_a_sibling_alias_naming_a_private_record_is_e0304() {
    let root = unique_tmp("g297-sibling-private");
    let src = root.join("src");
    write_file(&src, "rows.glyph", "module rows\ntype User = { name: string, }\npub type Rows = Array<User>\n");
    write_file(&src, "main.glyph", &sibling_alias_main("type User = { id: number, }\n"));
    let (code, stderr) = check_no_tsc(&src);
    assert_ne!(code, 0, "must not validate against `main`'s `User`:\n{stderr}");
    assert!(
        stderr.contains("`Rows`'s body names `User`, which `rows` declares without `pub`"),
        "{stderr}"
    );
}

/// E0304 on `json.parse<T>` names the reason `T` has no check. The note used
/// to blame a type parameter, an `extern_ts` type or a package type for
/// every refusal, which is false for a local interface, `Result`, a generic
/// union, and a container of any of them.
#[test]
fn json_parse_e0304_note_names_the_cause_per_type() {
    let cases = [
        ("interface", "interface Shape { name: string, }\n", "Shape", "an interface has no descriptor"),
        ("interface-opt", "interface Shape { name: string, }\n", "Option<Shape>", "an interface has no descriptor"),
        ("result", "", "Result<number, string>", "`Result` has no runtime check"),
        ("result-arr", "", "Array<Result<number, string>>", "`Result` has no runtime check"),
        ("union", "type Box<T> = | Full(T) | Empty\n", "Box<string>", "a generic union has no descriptor"),
        ("union-nullable", "type Box<T> = | Full(T) | Empty\n", "Nullable<Box<string>>", "a generic union has no descriptor"),
    ];
    for (dir, decls, ty, note) in cases {
        let root = unique_tmp(&format!("g297-note-{dir}"));
        write_file(
            &root,
            "main.glyph",
            &format!(
                "module main\nimport std/json\n{decls}pub fn read(body: string) -> Result<{ty}, Array<Issue>> {{\n  return json.parse<{ty}>(body)\n}}\n"
            ),
        );
        let (code, stderr) = check_no_tsc(&root);
        assert_ne!(code, 0, "{dir}: {stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(note), "{dir}: the note names the cause `{note}`: {stderr}");
        assert!(
            !stderr.contains("a type parameter is erased"),
            "{dir}: the note must not blame a type parameter: {stderr}"
        );
    }
    // The existing note stays for the causes it is true of.
    let root = unique_tmp("g297-note-typeparam");
    write_file(
        &root,
        "main.glyph",
        "module main\nimport std/json\npub fn read<T>(body: string) -> Result<T, Array<Issue>> {\n  return json.parse<T>(body)\n}\n",
    );
    let (code, stderr) = check_no_tsc(&root);
    assert_ne!(code, 0, "{stderr}");
    assert!(stderr.contains("a type parameter is erased"), "{stderr}");
}

/// The per-cause note holds for an interface and a generic union imported
/// from a sibling, by name and through a namespace, where the cause is read
/// off the declaring module rather than this one.
#[test]
fn json_parse_e0304_note_names_the_cause_of_an_imported_type() {
    let cases = [
        ("named-interface", "import model { Shape }\n", "Shape", "an interface has no descriptor"),
        ("ns-interface", "import model\n", "model.Shape", "an interface has no descriptor"),
        ("named-union", "import model { Box }\n", "Box<string>", "a generic union has no descriptor"),
        ("ns-union", "import model\n", "model.Box<string>", "a generic union has no descriptor"),
    ];
    for (dir, import, ty, note) in cases {
        let root = unique_tmp(&format!("g297-note-imported-{dir}"));
        write_file(
            &root,
            "model.glyph",
            "module model\npub interface Shape { name: string, }\npub type Box<T> = | Full(T) | Empty\n",
        );
        write_file(
            &root,
            "main.glyph",
            &format!(
                "module main\nimport std/json\n{import}pub fn read(body: string) -> Result<{ty}, Array<Issue>> {{\n  return json.parse<{ty}>(body)\n}}\n"
            ),
        );
        let (code, stderr) = check_no_tsc(&root);
        assert_ne!(code, 0, "{dir}: {stderr}");
        assert!(stderr.contains(note), "{dir}: the note names the cause `{note}`: {stderr}");
        assert!(!stderr.contains("a type parameter is erased"), "{dir}: {stderr}");
    }
}

/// `json.parse` with no type argument is emitted with the `unknown` the
/// checker gave it, in both spellings, called or read as a value. Left out,
/// `tsc` inferred `T` from where the result went, so the bare call became a
/// cast with no `as` (record 89).
#[test]
fn json_parse_with_no_type_argument_is_emitted_as_unknown() {
    let root = unique_tmp("json-parse-unknown-emit");
    write_file(
        &root,
        "main.glyph",
        "module main\nimport std/json\nimport std/json { parse }\n\
         pub fn a(s: string) -> Result<unknown, Array<Issue>> {\n  return json.parse(s)\n}\n\
         pub fn b(s: string) -> Result<unknown, Array<Issue>> {\n  return parse(s)\n}\n\
         pub fn c(s: string) -> Result<unknown, Array<Issue>> {\n  let p = json.parse\n  let q = parse\n  return q(s)\n}\n\
         pub fn d(s: string) -> Result<unknown, Array<Issue>> {\n  return json.parse<unknown>(s)\n}\n",
    );
    let out = root.join("out");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        "build".as_ref(),
        root.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-tsc".as_ref(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts");
    for want in [
        "return json.parse<unknown>(s);",
        "return parse<unknown>(s);",
        "let p = json.parse<unknown>;",
        "let q = parse<unknown>;",
    ] {
        assert!(ts.contains(want), "missing `{want}` in:\n{ts}");
    }
    assert!(!ts.contains("<unknown><unknown>"), "a written `<unknown>` is not doubled:\n{ts}");
}

/// The casts the bare call used to make, each refused by the full check now
/// that the emitted call says `unknown`: an annotation, an argument, a record
/// field, a `json.parse` value, and a generic wrapper that laundered the call
/// for any `T`. Each passed `glyph check` and `tsc --strict` in 0.1.132 and
/// crashed at run time on the first field read.
#[test]
fn a_bare_json_parse_cast_is_refused_by_the_full_check() {
    if !tsc_available() {
        eprintln!("skipping: tsc not found on PATH");
        return;
    }
    let prelude = "module main\nimport std/json\ntype User = { name: string, }\n\
                   type Box = { v: Result<User, Array<Issue>>, }\n\
                   fn take(r: Result<User, Array<Issue>>) -> bool {\n  return true\n}\n";
    let cases = [
        ("let", "pub fn f(s: string) -> bool {\n  let a: Result<User, Array<Issue>> = json.parse(s)\n  return true\n}\n", "TS2322"),
        ("argument", "pub fn f(s: string) -> bool {\n  return take(json.parse(s))\n}\n", "TS2345"),
        ("field", "pub fn f(s: string) -> Box {\n  return { v: json.parse(s), }\n}\n", "TS2322"),
        ("value", "pub fn f(s: string) -> Result<User, Array<Issue>> {\n  let p = json.parse\n  return p(s)\n}\n", "TS2322"),
        ("generic", "pub fn read<T>(s: string) -> Result<T, Array<Issue>> {\n  return json.parse(s)\n}\n", "TS2322"),
    ];
    for (dir, body, code_name) in cases {
        let root = unique_tmp(&format!("json-parse-cast-{dir}"));
        write_file(&root, "main.glyph", &format!("{prelude}{body}"));
        let path = root.join("main.glyph");
        let (code, _stdout, stderr, _) =
            spawn_glyph(&[std::ffi::OsStr::new("check"), path.as_os_str()]);
        assert_eq!(code, 1, "{dir}: the cast must be refused:\n{stderr}");
        assert!(stderr.contains(code_name), "{dir}: expected {code_name}:\n{stderr}");
    }
}

/// The declarations every G347 case shares: an `extern_ts` type, a record,
/// and a generic record whose own field `h` has no runtime check.
const G347_PAGE: &str = "type H = extern_ts(\"{ k: string }\")\ntype User = { id: number, }\ntype Page<T> = { items: Array<T>, h: H, }\n";

/// A generic record's own fields are judged on every path that trusts its
/// descriptor, its type parameters counted as checked by the arguments
/// (G347). `Page<T> = { items: Array<T>, h: H }` with `H` an `extern_ts`
/// type: the descriptor's `is` tests `h` for presence only, and through
/// 0.1.140 `json.parse<Page<User>>`, `Page.parse<User>(v)` and a record
/// field typed `Page<User>` all returned `Ok` for `{"items": [], "h": 5}`
/// (record 101, probes p1, p5 and p6). Each is E0304 naming the field, in
/// the shape the sibling-alias hop has used since 0.1.140.
#[test]
fn a_generic_record_with_an_unchecked_field_of_its_own_is_refused_on_every_path() {
    let page_h = "`Page`'s field `h` has type `H`, which has no runtime check";
    let read = |t: &str| {
        format!(
            "pub fn read(s: string) -> bool {{\n  return match json.parse<{t}>(s) {{ Ok(_) => true, Err(_) => false, }}\n}}\n"
        )
    };
    let local = |extra: &str, body: &str| format!("module main\nimport std/json\n{G347_PAGE}{extra}{body}");
    let cases: Vec<(&str, String, String)> = vec![
        ("json-parse", local("", &read("Page<User>")), format!("cannot validate `Page<User>`: {page_h}")),
        (
            "descriptor-parse",
            local("", "pub fn read(v: unknown) -> bool {\n  return match Page.parse<User>(v) { Ok(_) => true, Err(_) => false, }\n}\n"),
            format!("cannot validate `Page<User>`: {page_h}"),
        ),
        (
            "record-field",
            local("type Doc = { p: Page<User>, }\n", &read("Doc")),
            format!("cannot validate `Doc`: field `p` has type `Page<User>`, and {page_h}"),
        ),
        (
            "record-field-parse",
            local(
                "type Doc = { p: Page<User>, }\n",
                "pub fn read(v: unknown) -> bool {\n  return match Doc.parse(v) { Ok(_) => true, Err(_) => false, }\n}\n",
            ),
            format!("cannot validate `Doc`: field `p` has type `Page<User>`, and {page_h}"),
        ),
        (
            "record-field-deep",
            local("type Doc = { p: Page<User>, }\ntype Outer = { d: Doc, }\n", &read("Outer")),
            format!("cannot validate `Outer`: field `d.p` has type `Page<User>`, and {page_h}"),
        ),
        (
            "record-field-array",
            local("type Doc = { ps: Array<Page<User>>, }\n", &read("Doc")),
            format!("cannot validate `Doc`: field `ps` has type `Array<Page<User>>`, and {page_h}"),
        ),
        ("option", local("", &read("Option<Page<User>>")), format!("cannot validate `Option<Page<User>>`: {page_h}")),
        ("array", local("", &read("Array<Page<User>>")), format!("cannot validate `Array<Page<User>>`: {page_h}")),
        ("nullable", local("", &read("Nullable<Page<User>>")), format!("cannot validate `Nullable<Page<User>>`: {page_h}")),
        (
            "record",
            local("", &read("Record<string, Page<User>>")),
            format!("cannot validate `Record<string, Page<User>>`: {page_h}"),
        ),
        (
            "type-argument",
            local("type Box<T> = { v: T, }\n", &read("Box<Page<User>>")),
            format!("cannot validate `Box<Page<User>>`: {page_h}"),
        ),
        (
            "type-argument-parse",
            local(
                "type Box<T> = { v: T, }\n",
                "pub fn read(v: unknown) -> bool {\n  return match Box.parse<Page<User>>(v) { Ok(_) => true, Err(_) => false, }\n}\n",
            ),
            "cannot validate `Box<Page<User>>`: its type argument `Page<User>` has no runtime check".to_string(),
        ),
        (
            "nested-generic",
            format!(
                "module main\nimport std/json\ntype H = extern_ts(\"{{ k: string }}\")\ntype User = {{ id: number, }}\ntype Meta<T> = {{ v: T, h: H, }}\ntype Page<T> = {{ items: Array<T>, meta: Meta<T>, }}\n{}",
                read("Page<User>")
            ),
            "cannot validate `Page<User>`: `Meta`'s field `h` has type `H`, which has no runtime check".to_string(),
        ),
        (
            "record-in-generic",
            format!(
                "module main\nimport std/json\ntype H = extern_ts(\"{{ k: string }}\")\ntype User = {{ id: number, }}\ntype Inner = {{ h: H, }}\ntype Page<T> = {{ items: Array<T>, u: Inner, }}\n{}",
                read("Page<User>")
            ),
            "cannot validate `Page<User>`: `Page`'s field `u.h` has type `H`, which has no runtime check".to_string(),
        ),
    ];
    for (dir, main, expected) in cases {
        let root = unique_tmp(&format!("g347-local-{dir}"));
        write_file(&root, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&root);
        assert_ne!(code, 0, "{dir}: a generic with an unchecked field must be refused:\n{main}\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(&expected), "{dir}: the message names `{expected}`:\n{stderr}");
    }
}

/// The same refusal across a module boundary: `Page` imported by name, or
/// reached through a namespace, and a record that holds a `Page<User>`
/// declared in `model` and imported whole. The generic is judged where it
/// is declared.
#[test]
fn an_imported_generic_record_with_an_unchecked_field_of_its_own_is_refused() {
    let model = format!(
        "module model\n{}pub type Doc = {{ p: Page<User>, }}\n",
        G347_PAGE.replace("type ", "pub type ")
    );
    let page_h = "`Page`'s field `h` has type `H`, which has no runtime check";
    let cases = [
        (
            "named-json-parse",
            "module main\nimport std/json\nimport model { Page, User }\npub fn read(s: string) -> bool {\n  return match json.parse<Page<User>>(s) { Ok(_) => true, Err(_) => false, }\n}\n",
            format!("cannot validate `Page<User>`: {page_h}"),
        ),
        (
            "named-descriptor-parse",
            "module main\nimport model { Page, User }\npub fn read(v: unknown) -> bool {\n  return match Page.parse<User>(v) { Ok(_) => true, Err(_) => false, }\n}\n",
            format!("cannot validate `Page<User>`: {page_h}"),
        ),
        (
            "namespaced-descriptor-parse",
            "module main\nimport model as m\npub fn read(v: unknown) -> bool {\n  return match m.Page.parse<m.User>(v) { Ok(_) => true, Err(_) => false, }\n}\n",
            format!("cannot validate `m.Page<m.User>`: {page_h}"),
        ),
        (
            "imported-record",
            "module main\nimport std/json\nimport model { Doc }\npub fn read(s: string) -> bool {\n  return match json.parse<Doc>(s) { Ok(_) => true, Err(_) => false, }\n}\n",
            format!("cannot validate `Doc`: field `p` has type `Page<User>`, and {page_h}"),
        ),
        (
            "local-record",
            "module main\nimport std/json\nimport model { Page, User }\ntype Mine = { p: Page<User>, }\npub fn read(s: string) -> bool {\n  return match json.parse<Mine>(s) { Ok(_) => true, Err(_) => false, }\n}\n",
            format!("cannot validate `Mine`: field `p` has type `Page<User>`, and {page_h}"),
        ),
    ];
    for (dir, main, expected) in cases {
        let root = unique_tmp(&format!("g347-imported-{dir}"));
        let src = root.join("src");
        write_file(&src, "model.glyph", &model);
        write_file(&src, "main.glyph", main);
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{dir}: a generic with an unchecked field must be refused:\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(&expected), "{dir}: the message names `{expected}`:\n{stderr}");
    }
}

/// Record 103's `priv` model: `Page`'s field `meta` is typed by a record
/// `model` keeps private, and that record checks every field.
const G350_PRIV: &str = "module model\ntype Meta = { n: int, }\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, meta: Meta, }\n";

/// Record 103's `p2` model: the private record's own field is an
/// `extern_ts` type, so `Meta.is` tests it for presence only.
const G350_P2: &str = "module model\ntype H = extern_ts(\"{ k: string }\")\ntype Meta = { h: H, }\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, meta: Meta, }\n";

/// A sibling whose alias applies `model`'s `Page`: the generic hop.
const G350_ROWS: &str = "module rows\nimport model { Page, User }\npub type Rows = Page<User>\n";

/// Every path that trusts `Page<User>`'s descriptor, as a function from a
/// `Page` document to the arm taken: `(name, declarations, body)`. The
/// document is wrapped where the path reads `Page` inside something else.
fn g350_paths() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("json-parse", "", "return match json.parse<Page<User>>(s) { Ok(_) => \"Ok\", Err(_) => \"Err\", }"),
        ("namespace", "", "return match json.parse<m.Page<m.User>>(s) { Ok(_) => \"Ok\", Err(_) => \"Err\", }"),
        (
            "descriptor-parse",
            "",
            "return match json.parse<unknown>(s) {\n    Ok(v) => match Page.parse<User>(v) { Ok(_) => \"Ok\", Err(_) => \"Err\", },\n    Err(_) => \"bad json\",\n  }",
        ),
        (
            "record-field",
            "type Doc = { p: Page<User>, }\n",
            "return match json.parse<Doc>(\"{\\\"p\\\": ${s}}\") { Ok(_) => \"Ok\", Err(_) => \"Err\", }",
        ),
        (
            "option-field",
            "type Opt = { p: Option<Page<User>>, }\n",
            "return match json.parse<Opt>(\"{\\\"p\\\": {\\\"tag\\\": \\\"Some\\\", \\\"value\\\": ${s}}}\") { Ok(_) => \"Ok\", Err(_) => \"Err\", }",
        ),
        ("array", "", "return match json.parse<Array<Page<User>>>(\"[${s}]\") { Ok(_) => \"Ok\", Err(_) => \"Err\", }"),
        (
            "record",
            "",
            "return match json.parse<Record<string, Page<User>>>(\"{\\\"k\\\": ${s}}\") { Ok(_) => \"Ok\", Err(_) => \"Err\", }",
        ),
        ("hop", "", "return match json.parse<Rows>(s) { Ok(_) => \"Ok\", Err(_) => \"Err\", }"),
        (
            "hop-field",
            "type HopDoc = { r: Rows, }\n",
            "return match json.parse<HopDoc>(\"{\\\"r\\\": ${s}}\") { Ok(_) => \"Ok\", Err(_) => \"Err\", }",
        ),
        (
            "type-argument",
            "type Box<T> = { v: T, }\n",
            "return match json.parse<unknown>(\"{\\\"v\\\": ${s}}\") {\n    Ok(v) => match Box.parse<Page<User>>(v) { Ok(_) => \"Ok\", Err(_) => \"Err\", },\n    Err(_) => \"bad json\",\n  }",
        ),
        (
            "is-pattern",
            "",
            "return match json.parse<unknown>(s) {\n    Ok(v) => match v { is Page<User> => \"Ok\", _ => \"Err\", },\n    Err(_) => \"bad json\",\n  }",
        ),
    ]
}

/// A `main` module holding the paths given, each as `pub fn <name>(s)`,
/// with a `main` that prints each path's arm for each document.
fn g350_main(paths: &[(&str, &str, &str)], docs: &[&str]) -> String {
    let mut out = String::from(
        "module main\nimport std/io { println }\nimport std/json\nimport model { Page, User }\nimport model as m\nimport rows { Rows }\n",
    );
    for (_, decls, _) in paths {
        out.push_str(decls);
    }
    for (name, _, body) in paths {
        out.push_str(&format!("pub fn read_{}(s: string) -> string {{\n  {body}\n}}\n", name.replace('-', "_")));
    }
    out.push_str("fn main() {\n");
    for doc in docs {
        out.push_str(&format!("  let d{} = \"{doc}\"\n", out.matches("let d").count()));
    }
    for (name, _, _) in paths {
        for i in 0..docs.len() {
            out.push_str(&format!("  println(read_{}(d{i}))\n", name.replace('-', "_")));
        }
    }
    out.push_str("}\n");
    out
}

/// G350: an imported generic is judged by its declaring module's recorded
/// walk, as an imported record is. Record 103's `priv`: `Page`'s field is
/// typed by a record `model` keeps private, and that record checks every
/// field, so `Page<User>` is accepted on every path that trusts its
/// descriptor and validates at run time: a good document is `Ok`, one that
/// breaks the private record's field (`n: 1.5` for an `int`) or the
/// argument's (`{"id": "x"}`) is `Err`. From 0.1.141 to 0.1.143 each of
/// these was E0304, "`Meta` is private to `model`".
#[test]
fn an_imported_generic_with_a_checked_private_record_field_validates_on_every_path() {
    let good = "{\\\"items\\\": [{\\\"id\\\": 1}], \\\"meta\\\": {\\\"n\\\": 1}}";
    let bad_private = "{\\\"items\\\": [{\\\"id\\\": 1}], \\\"meta\\\": {\\\"n\\\": 1.5}}";
    let bad_arg = "{\\\"items\\\": [{\\\"id\\\": \\\"x\\\"}], \\\"meta\\\": {\\\"n\\\": 1}}";
    let paths = g350_paths();
    let root = unique_tmp("g350-priv");
    let src = root.join("src");
    write_file(&src, "model.glyph", G350_PRIV);
    write_file(&src, "rows.glyph", G350_ROWS);
    write_file(&src, "main.glyph", &g350_main(&paths, &[good, bad_private, bad_arg]));
    let (code, stderr) = check_no_tsc(&src);
    assert_eq!(code, 0, "{stderr}");
    let lines = run_main_lines(&src);
    let expected: Vec<&str> = paths.iter().flat_map(|_| ["Ok", "Err", "Err"]).collect();
    assert_eq!(lines, expected, "one Ok, Err, Err per path, in the order {:?}", paths.iter().map(|p| p.0).collect::<Vec<_>>());
}

/// G350's other half: record 103's `p2`, where the private record's own
/// field has no runtime check. Judged by `model`'s walk, every path is
/// still refused, and the message names the field that is unchecked
/// (`meta.h`, an `extern_ts` type) rather than the record being private.
/// The hop names the alias whose body applies `Page`.
#[test]
fn an_imported_generic_with_an_unchecked_private_record_field_is_refused_for_that_field() {
    let field = "`Page`'s field `meta.h` has type `H`, which has no runtime check";
    for path in g350_paths() {
        let root = unique_tmp(&format!("g350-p2-{}", path.0));
        let src = root.join("src");
        write_file(&src, "model.glyph", G350_P2);
        write_file(&src, "rows.glyph", G350_ROWS);
        let main = g350_main(&[path], &[]);
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{}: the hole must be refused:\n{main}\n{stderr}", path.0);
        assert!(stderr.contains("[E0304]"), "{}: {stderr}", path.0);
        let expected = if path.0.starts_with("hop") {
            format!("`Rows`'s body applies {}", field.replacen("`Page`'s field", "`Page`, whose field", 1))
        } else {
            field.to_string()
        };
        assert!(stderr.contains(&expected), "{}: the message names `{expected}`:\n{stderr}", path.0);
        assert!(!stderr.contains("is private to"), "{}: the cause is the field, not privacy:\n{stderr}", path.0);
        assert!(!stderr.contains("G350"), "{}: {stderr}", path.0);
    }
}

/// A private field of an imported generic that is itself a generic, and
/// one nested across three modules: each is judged by the walks its
/// modules recorded. Checkable, the generic validates at run time;
/// with an `extern_ts` field at the bottom, it is refused naming that
/// field.
#[test]
fn an_imported_generic_is_judged_through_private_generics_and_nested_modules() {
    let main = |t: &str, docs: [&str; 3]| {
        format!(
            "module main\nimport std/io {{ println }}\nimport std/json\nimport model {{ Page, User }}\nfn main() {{\n{}}}\n",
            docs.iter()
                .map(|d| format!("  println(match json.parse<{t}>(\"{d}\") {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }})\n"))
                .collect::<String>()
        )
    };
    // `Page`'s field applies a generic `model` keeps private.
    let generic_meta = "module model\ntype Meta<T> = { v: T, n: int, }\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, meta: Meta<T>, }\n";
    let root = unique_tmp("g350-private-generic");
    let src = root.join("src");
    write_file(&src, "model.glyph", generic_meta);
    write_file(
        &src,
        "main.glyph",
        &main(
            "Page<User>",
            [
                "{\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"id\\\": 1}, \\\"n\\\": 1}}",
                "{\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"id\\\": 1}, \\\"n\\\": 1.5}}",
                "{\\\"items\\\": [], \\\"meta\\\": {\\\"v\\\": {\\\"id\\\": \\\"x\\\"}, \\\"n\\\": 1}}",
            ],
        ),
    );
    assert_eq!(run_main_lines(&src), vec!["Ok", "Err", "Err"]);
    let root = unique_tmp("g350-private-generic-hole");
    let src = root.join("src");
    write_file(
        &src,
        "model.glyph",
        "module model\ntype H = extern_ts(\"{ k: string }\")\ntype Meta<T> = { v: T, h: H, }\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, meta: Meta<T>, }\n",
    );
    write_file(&src, "main.glyph", &main("Page<User>", ["{}", "{}", "{}"]));
    let (code, stderr) = check_no_tsc(&src);
    assert_ne!(code, 0, "{stderr}");
    assert!(stderr.contains("`Meta`'s field `h` has type `H`, which has no runtime check"), "{stderr}");

    // Nested: `Page<Page<User>>`, whose private `Meta` holds a record of a
    // third module, `base`.
    let base = |inner: &str| format!("module base\npub type H = extern_ts(\"{{ k: string }}\")\npub type Inner = {{ {inner} }}\n");
    let model = "module model\nimport base { Inner }\ntype Meta = { i: Inner, }\npub type User = { id: number, }\npub type Page<T> = { items: Array<T>, meta: Meta, }\n";
    let root = unique_tmp("g350-nested");
    let src = root.join("src");
    write_file(&src, "base.glyph", &base("n: int,"));
    write_file(&src, "model.glyph", model);
    let outer = |n: &str, id: &str| {
        format!("{{\\\"items\\\": [{{\\\"items\\\": [{{\\\"id\\\": {id}}}], \\\"meta\\\": {{\\\"i\\\": {{\\\"n\\\": 1}}}}}}], \\\"meta\\\": {{\\\"i\\\": {{\\\"n\\\": {n}}}}}}}")
    };
    let (good, bad_n, bad_id) = (outer("1", "1"), outer("1.5", "1"), outer("1", "\\\"x\\\""));
    write_file(&src, "main.glyph", &main("Page<Page<User>>", [&good, &bad_n, &bad_id]));
    assert_eq!(run_main_lines(&src), vec!["Ok", "Err", "Err"]);
    let root = unique_tmp("g350-nested-hole");
    let src = root.join("src");
    write_file(&src, "base.glyph", &base("h: H,"));
    write_file(&src, "model.glyph", model);
    write_file(&src, "main.glyph", &main("Page<Page<User>>", ["{}", "{}", "{}"]));
    let (code, stderr) = check_no_tsc(&src);
    assert_ne!(code, 0, "{stderr}");
    assert!(stderr.contains("`Page`'s field `meta.i.h` has type `H`, which has no runtime check"), "{stderr}");
}

/// Two generics of two modules that apply each other: following one's
/// recorded walk into the other and back ends, as a recursive generic of
/// one module does, and the pair validates at run time. With an unchecked
/// field in the second, the first is refused for it.
#[test]
fn a_cycle_of_generics_across_modules_is_judged_and_ends() {
    let b = |extra: &str| {
        format!("module b\nimport a {{ A }}\ntype H = extern_ts(\"{{ k: string }}\")\ntype Priv = {{ n: int, }}\npub type B<T> = {{ w: T, p: Priv, a: Option<A<T>>, {extra}}}\n")
    };
    let a = "module a\nimport b { B }\npub type A<T> = { v: T, b: Option<B<T>>, }\n";
    // An `Option` is held as `{"tag": "Some", "value": …}` or `{"tag": "None"}`.
    let doc = |n: &str, w: &str| {
        format!("{{\\\"v\\\": 1, \\\"b\\\": {{\\\"tag\\\": \\\"Some\\\", \\\"value\\\": {{\\\"w\\\": {w}, \\\"p\\\": {{\\\"n\\\": {n}}}, \\\"a\\\": {{\\\"tag\\\": \\\"Some\\\", \\\"value\\\": {{\\\"v\\\": 4, \\\"b\\\": {{\\\"tag\\\": \\\"None\\\"}}}}}}}}}}}}")
    };
    let main = format!(
        "module main\nimport std/io {{ println }}\nimport std/json\nimport a {{ A }}\nfn main() {{\n{}}}\n",
        [doc("3", "2"), doc("3.5", "2"), doc("3", "2.5")]
            .iter()
            .map(|d| format!("  println(match json.parse<A<int>>(\"{d}\") {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }})\n"))
            .collect::<String>()
    );
    let main = main.as_str();
    let root = unique_tmp("g350-cycle");
    let src = root.join("src");
    write_file(&src, "a.glyph", a);
    write_file(&src, "b.glyph", &b(""));
    write_file(&src, "main.glyph", main);
    assert_eq!(run_main_lines(&src), vec!["Ok", "Err", "Err"]);
    let root = unique_tmp("g350-cycle-hole");
    let src = root.join("src");
    write_file(&src, "a.glyph", a);
    write_file(&src, "b.glyph", &b("h: H, "));
    write_file(&src, "main.glyph", main);
    let (code, stderr) = check_no_tsc(&src);
    assert_ne!(code, 0, "{stderr}");
    assert!(stderr.contains("`B`'s field `h` has type `H`, which has no runtime check"), "{stderr}");
}

/// A generic declared in the module that applies it reads its private
/// records with that module's own resolution, as an imported one now
/// does through its module's walk: both validate.
#[test]
fn a_local_generic_with_a_private_record_field_validates() {
    let root = unique_tmp("g347-private-local");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/io { println }\nimport std/json\ntype Meta = { n: int, }\ntype User = { id: number, }\ntype Page<T> = { items: Array<T>, meta: Meta, }\nfn main() {\n  println(match json.parse<Page<User>>(\"{\\\"items\\\": [{\\\"id\\\": 1}], \\\"meta\\\": {\\\"n\\\": 1}}\") { Ok(_) => \"Ok\", Err(_) => \"Err\", })\n  println(match json.parse<Page<User>>(\"{\\\"items\\\": [], \\\"meta\\\": {\\\"n\\\": 1.5}}\") { Ok(_) => \"Ok\", Err(_) => \"Err\", })\n}\n",
    );
    assert_eq!(run_main_lines(&src), vec!["Ok", "Err"]);
}

/// A generic whose every field has a check still validates on each path the
/// refusal now covers, run so the arm taken is the evidence: a good
/// document is `Ok` and one that breaks the generic's own field (`n: 1.5`
/// for an `int`) or the argument's (`{"name": 1}`) is `Err`.
#[test]
fn a_generic_record_whose_fields_are_all_checked_still_validates_on_every_path() {
    let decls = "type User = { name: string, }\ntype Page<T> = { items: Array<T>, n: int, }\ntype Doc = { p: Page<User>, }\n";
    let good = "{\\\"items\\\": [{\\\"name\\\": \\\"a\\\"}], \\\"n\\\": 1}";
    let bad_own = "{\\\"items\\\": [{\\\"name\\\": \\\"a\\\"}], \\\"n\\\": 1.5}";
    let bad_arg = "{\\\"items\\\": [{\\\"name\\\": 1}], \\\"n\\\": 1}";
    let lines = |head: &str, call: &dyn Fn(&str) -> String| {
        let mut body = format!("{head}fn main() {{\n");
        for doc in [good, bad_own, bad_arg] {
            body.push_str(&format!("  println({})\n", call(doc)));
        }
        body.push_str("}\n");
        body
    };
    let json_parse = |t: &'static str| {
        move |doc: &str| format!("match json.parse<{t}>(\"{doc}\") {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }}")
    };
    let wrap = |t: &'static str, pre: &'static str, post: &'static str| {
        move |doc: &str| {
            format!("match json.parse<{t}>(\"{pre}{doc}{post}\") {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }}")
        }
    };
    let descriptor = |recv: &'static str, arg: &'static str| {
        move |doc: &str| {
            format!(
                "match json.parse(\"{doc}\") {{ Ok(v) => match {recv}.parse<{arg}>(v) {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }}, Err(_) => \"Err\", }}"
            )
        }
    };
    let local_head = format!("module main\nimport std/io {{ println }}\nimport std/json\n{decls}");
    let named_head = "module main\nimport std/io { println }\nimport std/json\nimport model { Page, User, Doc }\n";
    let ns_head = "module main\nimport std/io { println }\nimport std/json\nimport model as m\n";
    let cases: Vec<(&str, String)> = vec![
        ("local-json-parse", lines(&local_head, &json_parse("Page<User>"))),
        ("local-descriptor", lines(&local_head, &descriptor("Page", "User"))),
        ("local-record-field", lines(&local_head, &wrap("Doc", "{\\\"p\\\": ", "}"))),
        ("local-option", lines(&local_head, &wrap("Option<Page<User>>", "{\\\"tag\\\": \\\"Some\\\", \\\"value\\\": ", "}"))),
        ("local-array", lines(&local_head, &wrap("Array<Page<User>>", "[", "]"))),
        ("local-record", lines(&local_head, &wrap("Record<string, Page<User>>", "{\\\"k\\\": ", "}"))),
        ("named-json-parse", lines(named_head, &json_parse("Page<User>"))),
        ("named-descriptor", lines(named_head, &descriptor("Page", "User"))),
        ("named-record", lines(named_head, &wrap("Doc", "{\\\"p\\\": ", "}"))),
        ("namespaced-descriptor", lines(ns_head, &descriptor("m.Page", "m.User"))),
    ];
    for (dir, main) in cases {
        let root = unique_tmp(&format!("g347-control-{dir}"));
        let src = root.join("src");
        write_file(&src, "model.glyph", &format!("module model\n{}", decls.replace("type ", "pub type ")));
        write_file(&src, "main.glyph", &main);
        assert_eq!(run_main_lines(&src), vec!["Ok", "Err", "Err"], "{dir}:\n{main}");
    }
}

/// Declaring a record that holds an unchecked generic is fine, and its
/// descriptor is emitted as before: `Doc`'s check still calls `Page`'s on the
/// field (`Page.__is`, with the depth, D50), which tests everything `Page` can check. Only a use that reports
/// the whole value checked is refused, so the refusal must not weaken the
/// emitted check to a presence test.
#[test]
fn declaring_a_record_over_an_unchecked_generic_emits_its_descriptor_unchanged() {
    let root = unique_tmp("g347-emit");
    write_file(&root, "main.glyph", &format!("module main\n{G347_PAGE}pub type Doc = {{ p: Page<User>, }}\n"));
    let out = root.join("out");
    let (code, _stdout, stderr, _) = spawn_glyph(&[
        "build".as_ref(),
        root.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-tsc".as_ref(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let ts = std::fs::read_to_string(out.join("main.ts")).expect("main.ts");
    assert!(
        ts.contains("&& Page.__is((value as Record<string, unknown>).p, __d + 1, User.__is)"),
        "{ts}"
    );
    assert!(
        ts.contains("if (!(Page.__is((value as Record<string, unknown>).p, __d + 1, User.__is))) {"),
        "{ts}"
    );
}

/// One G348 refusal case: its directory, the `model` and `rows` modules it
/// needs, `main`, and the message expected.
type G348Case = (&'static str, Option<String>, Option<String>, String, String);

/// A tagged union's variant payload is walked like a record's fields, on
/// every path that trusts the union's descriptor (G348). `U = A({ h: H }) |
/// B` with `H` an `extern_ts` type: the union's `is` checks the tag and
/// tests `h` for presence only, and through 0.1.140 `json.parse<U>`, a
/// record field `{ u: U }`, an imported `Array<U>`, a sibling alias `Rows =
/// Array<U>` and a generic's field `h: U` reached through a hop all returned
/// `Ok` for `{"tag":"A","h":5}` (record 102, probes u1 to u4 and g7). Each is
/// E0304 naming the variant and the field, the rule G347 uses for a generic
/// record's own fields.
#[test]
fn a_union_whose_variant_payload_has_an_unchecked_field_is_refused_on_every_path() {
    let union = "pub type H = extern_ts(\"{ k: string }\")\npub type U = A({ h: H }) | B\n";
    let u_h = "field `h` of variant `A` has type `H`, which has no runtime check";
    let read = |t: &str| {
        format!(
            "pub fn read(s: string) -> bool {{\n  return match json.parse<{t}>(s) {{ Ok(_) => true, Err(_) => false, }}\n}}\n"
        )
    };
    let local = |extra: &str, body: &str| format!("module main\nimport std/json\n{union}{extra}{body}");
    let cases: Vec<G348Case> = vec![
        ("local", None, None, local("", &read("U")), format!("cannot validate `U`: {u_h}")),
        (
            "local-descriptor",
            None,
            None,
            local("", "pub fn read(v: unknown) -> bool {\n  return match U.parse(v) { Ok(_) => true, Err(_) => false, }\n}\n"),
            format!("cannot validate `U`: {u_h}"),
        ),
        (
            "record-field",
            None,
            None,
            local("type W = { u: U, }\n", &read("W")),
            "cannot validate `W`: field `u.h` of variant `A` has type `H`, which has no runtime check".to_string(),
        ),
        ("array", None, None, local("", &read("Array<U>")), format!("cannot validate `U`: {u_h}")),
        ("option", None, None, local("", &read("Option<U>")), format!("cannot validate `U`: {u_h}")),
        (
            "value-payload",
            None,
            None,
            "module main\nimport std/json\ntype H = extern_ts(\"{ k: string }\")\ntype V = A(H) | B\n".to_string() + &read("V"),
            "cannot validate `V`: field `value` of variant `A` has type `H`, which has no runtime check".to_string(),
        ),
        (
            "local-generic-field",
            None,
            None,
            local("type Page<T> = { items: Array<T>, h: U, }\n", &read("Page<number>")),
            "cannot validate `Page<number>`: `Page`'s field `h.h` of variant `A` has type `H`, which has no runtime check".to_string(),
        ),
        (
            "u4-imported",
            Some(format!("module model\n{union}")),
            None,
            "module main\nimport std/json\nimport model { U }\n".to_string() + &read("Array<U>"),
            format!("cannot validate `U`: {u_h}"),
        ),
        (
            "u2-hop-imported",
            Some(format!("module model\n{union}")),
            Some("module rows\nimport model { U }\npub type Rows = Array<U>\n".to_string()),
            "module main\nimport std/json\nimport rows { Rows }\n".to_string() + &read("Rows"),
            format!("cannot validate `U`: {u_h}"),
        ),
        (
            "u3-hop-declared",
            None,
            Some(format!("module rows\n{union}pub type Rows = Array<U>\n")),
            "module main\nimport std/json\nimport rows { Rows }\n".to_string() + &read("Rows"),
            format!("cannot validate `U`: {u_h}"),
        ),
        (
            "g7-generic-hop",
            Some(format!("module model\n{union}pub type User = {{ id: number, }}\npub type Page<T> = {{ items: Array<T>, h: U, }}\n")),
            Some("module rows\nimport model { Page, User }\npub type Rows = Page<User>\n".to_string()),
            "module main\nimport std/json\nimport rows { Rows }\n".to_string() + &read("Rows"),
            "`Rows`'s body applies `Page`, whose field `h.h` of variant `A` has type `H`, which has no runtime check".to_string(),
        ),
    ];
    for (dir, model, rows, main, expected) in cases {
        let root = unique_tmp(&format!("g348-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", &model);
        }
        if let Some(rows) = rows {
            write_file(&src, "rows.glyph", &rows);
        }
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{dir}: a union with an unchecked payload field must be refused:\n{main}\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(&expected), "{dir}: the message names `{expected}`:\n{stderr}");
    }
}

/// A union whose every payload has a check still validates on each path,
/// run so the arm taken is the evidence: a good `A` and a `B` are `Ok`, an
/// `A` whose payload breaks its field (`n: 1.5` for an `int`) and an
/// unknown tag are `Err`.
#[test]
fn a_union_whose_payloads_are_all_checked_still_validates_on_every_path() {
    let union = "pub type U = A({ n: int }) | B\n";
    let docs = [
        ("[{\\\"tag\\\": \\\"A\\\", \\\"n\\\": 1}]", "Ok"),
        ("[{\\\"tag\\\": \\\"B\\\"}]", "Ok"),
        ("[{\\\"tag\\\": \\\"A\\\", \\\"n\\\": 1.5}]", "Err"),
        ("[{\\\"tag\\\": \\\"C\\\"}]", "Err"),
    ];
    let main = |head: &str, t: &str| {
        let mut body = format!("module main\nimport std/io {{ println }}\nimport std/json\n{head}fn main() {{\n");
        for (doc, _) in docs {
            body.push_str(&format!(
                "  println(match json.parse<{t}>(\"{doc}\") {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }})\n"
            ));
        }
        body.push_str("}\n");
        body
    };
    let cases = [
        ("local", None, None, main(union, "Array<U>")),
        ("record", None, None, main(&format!("{union}type W = {{ us: Array<U>, }}\n"), "W").replace("[{", "{\\\"us\\\": [{").replace("}]", "}]}")),
        ("imported", Some(format!("module model\n{union}")), None, main("import model { U }\n", "Array<U>")),
        (
            "hop",
            Some(format!("module model\n{union}")),
            Some("module rows\nimport model { U }\npub type Rows = Array<U>\n"),
            main("import rows { Rows }\n", "Rows"),
        ),
    ];
    for (dir, model, rows, main) in cases {
        let root = unique_tmp(&format!("g348-control-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", &model);
        }
        if let Some(rows) = rows {
            write_file(&src, "rows.glyph", rows);
        }
        write_file(&src, "main.glyph", &main);
        let want: Vec<String> = docs.iter().map(|(_, w)| w.to_string()).collect();
        assert_eq!(run_main_lines(&src), want, "{dir}:\n{main}");
    }
}

/// The type argument of a generic descriptor call is walked like the type
/// `json.parse` reads (record 105, B1). `Page.parse<U>(v)` lowers to
/// `Page.parse<U>(v, (__cv) => U.is(__cv))`, so it trusts `U`'s descriptor
/// exactly as `U.parse` and `json.parse<Page<U>>` do, and through the
/// 0.1.141 build it returned `Ok` for `{"items": [{"tag": "A", "h": 5}]}`
/// while both of those refused `U`. A record argument (`Doc`, and `W`
/// holding `U`) was `Ok` on 0.1.140 too. Each spelling, local, imported by
/// name and through a namespace, is E0304 in the words the other two paths
/// use, naming the argument and the field.
#[test]
fn a_generic_descriptor_call_walks_its_type_argument() {
    let decls = "pub type H = extern_ts(\"{ k: string }\")\npub type U = A({ h: H, }) | B\npub type Doc = { h: H, }\npub type W = { u: U, }\npub type Page<T> = { items: Array<T>, }\n";
    let call = |recv: &str, arg: &str| {
        format!(
            "pub fn read(v: unknown) -> bool {{\n  return match {recv}.parse<{arg}>(v) {{ Ok(_) => true, Err(_) => false, }}\n}}\n"
        )
    };
    let u_h = "cannot validate `U`: field `h` of variant `A` has type `H`, which has no runtime check";
    let local = |body: String| format!("module main\n{decls}{body}");
    let cases: Vec<(&str, Option<String>, String, String)> = vec![
        ("a12-union", None, local(call("Page", "U")), u_h.to_string()),
        (
            "a12b-record",
            None,
            local(call("Page", "Doc")),
            "cannot validate `Doc`: field `h` has type `H`, which has no runtime check".to_string(),
        ),
        (
            "a12d-record-holding-union",
            None,
            local(call("Page", "W")),
            "cannot validate `W`: field `u.h` of variant `A` has type `H`, which has no runtime check".to_string(),
        ),
        ("array-of-union", None, local(call("Page", "Array<U>")), u_h.to_string()),
        ("option-of-union", None, local(call("Page", "Option<U>")), u_h.to_string()),
        (
            "named-import",
            Some(format!("module model\n{decls}")),
            format!("module main\nimport model {{ Page, U }}\n{}", call("Page", "U")),
            u_h.to_string(),
        ),
        (
            "named-import-record",
            Some(format!("module model\n{decls}")),
            format!("module main\nimport model {{ Page, Doc }}\n{}", call("Page", "Doc")),
            "cannot validate `Doc`: field `h` has type `H`, which has no runtime check".to_string(),
        ),
        (
            "ns-bad",
            Some(format!("module model\n{decls}")),
            format!("module main\nimport model as m\n{}", call("m.Page", "m.U")),
            u_h.to_string(),
        ),
        (
            "ns-record-holding-union",
            Some(format!("module model\n{decls}")),
            format!("module main\nimport model as m\n{}", call("m.Page", "m.W")),
            "cannot validate `W`: field `u.h` of variant `A` has type `H`, which has no runtime check".to_string(),
        ),
    ];
    for (dir, model, main, expected) in cases {
        let root = unique_tmp(&format!("b1-genarg-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", &model);
        }
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{dir}: the type argument's unchecked field must be refused:\n{main}\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(&expected), "{dir}: the message names `{expected}`:\n{stderr}");
    }
}

/// A type argument whose every field and payload has a check still
/// validates through the generic's descriptor, run so the arm taken is the
/// evidence: a good document is `Ok`, one that breaks a payload field is
/// `Err`, in each spelling the refusal above covers.
#[test]
fn a_generic_descriptor_call_with_a_checked_type_argument_still_validates() {
    let decls = "pub type U = A({ n: int, }) | B\npub type Doc = { n: int, }\npub type W = { u: U, }\npub type Page<T> = { items: Array<T>, }\n";
    let main = |head: &str, recv: &str, arg: &str, good: &str, bad: &str| {
        format!(
            "module main\nimport std/io {{ println }}\n{head}fn main() {{\n  let good: unknown = {good}\n  let bad: unknown = {bad}\n  println(match {recv}.parse<{arg}>(good) {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }})\n  println(match {recv}.parse<{arg}>(bad) {{ Ok(_) => \"Ok\", Err(_) => \"Err\", }})\n}}\n"
        )
    };
    let ug = "{ items: [{ tag: \"A\", n: 1 }, { tag: \"B\" }] }";
    let ub = "{ items: [{ tag: \"A\", n: 1.5 }] }";
    let dg = "{ items: [{ n: 1 }] }";
    let db = "{ items: [{ n: \"x\" }] }";
    let wg = "{ items: [{ u: { tag: \"A\", n: 1 } }] }";
    let wb = "{ items: [{ u: { tag: \"A\", n: 1.5 } }] }";
    let cases: Vec<(&str, Option<String>, String)> = vec![
        ("union", None, main(decls, "Page", "U", ug, ub)),
        ("record", None, main(decls, "Page", "Doc", dg, db)),
        ("record-holding-union", None, main(decls, "Page", "W", wg, wb)),
        ("named", Some(format!("module model\n{decls}")), main("import model { Page, U }\n", "Page", "U", ug, ub)),
        ("namespace", Some(format!("module model\n{decls}")), main("import model as m\n", "m.Page", "m.W", wg, wb)),
    ];
    for (dir, model, main) in cases {
        let root = unique_tmp(&format!("b1-genarg-control-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", &model);
        }
        write_file(&src, "main.glyph", &main);
        assert_eq!(run_main_lines(&src), vec!["Ok", "Err"], "{dir}:\n{main}");
    }
}
/// G313 (record 88): `glyph build` runs the lint tier on a module whose only
/// errors are type errors, so the unused binding is reported beside the
/// E0200 rather than after the E0200 is fixed. The emitter still does not
/// run on a module with an error: nothing is written for it.
#[test]
fn build_lints_a_module_with_a_type_error_and_does_not_emit_it() {
    let root = unique_tmp("g313build");
    let src = root.join("src");
    let out = root.join("out");
    write_file(
        &src,
        "main.glyph",
        "module main\n\npub type S =\n  | A\n  | B\n  | C\n\n\
         pub fn f(s: S) -> number {\n  return match s {\n    A => 1,\n    B => 2,\n  }\n}\n\n\
         pub fn g() -> number {\n  let x = 1\n  return 1\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build");
    let codes: Vec<(&str, &str)> = report
        .structured
        .iter()
        .map(|d| (d.code.as_str(), d.severity.as_str()))
        .collect();
    assert_eq!(codes, [("E0200", "error"), ("E0107", "warning")], "{:?}", report.diagnostics);
    assert_eq!(report.error_count, 1, "a lint is not an error");
    assert!(report.emitted.is_empty(), "a module with an error is not emitted");

    // A resolve error still stops the lint tier: the resolution map is not
    // complete, and a used binding could read as dead.
    write_file(
        &src,
        "main.glyph",
        "module main\n\npub fn g() -> number {\n  let x = 1\n  return y\n}\n",
    );
    let report = build_project_inner(&src, &out, false).expect("build");
    assert!(
        report.structured.iter().all(|d| d.code != "E0107"),
        "no lint past a resolve error: {:?}",
        report.diagnostics
    );
}

/// The `[E0304]` headline of a `check --no-tsc` run: the diagnostic line
/// itself, without the source excerpt, so two spellings of one claim can be
/// compared word for word.
fn e0304_headline(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .find(|l| l.contains("[E0304]"))
        .map(|l| l.trim().to_string())
}

/// G349: an `is T` pattern is judged by the walk `T.is` uses. The pattern
/// lowers to the same descriptor call `T.is(v)` makes and narrows the
/// scrutinee on its answer, but through the 0.1.141 build only `T.is(v)`
/// and `json.parse<T>` were refused for a field with no runtime check:
/// `match v { is Doc => … }` took the `Doc` arm for `{"h": 5}`. Each shape
/// here is now E0304, and where the same claim has a call spelling
/// (`T.is(v)`, or `G.parse<X>(v)` for a generic) the headline is that
/// call's, word for word, whether the type is declared here, imported by
/// name or reached through a namespace.
#[test]
fn an_is_pattern_is_judged_by_the_walk_its_descriptor_uses() {
    let decls = "pub type H = extern_ts(\"{ k: string }\")\n\
        pub type Doc = { h: H, }\n\
        pub type D2 = Doc\n\
        pub type U = A({ h: H, }) | B\n\
        pub type Outer = { d: Doc, }\n\
        pub type Box<T> = { v: T, h: H, }\n\
        pub type Page<T> = { items: Array<T>, }\n\
";
    let pattern = |pat: &str| {
        format!("pub fn read(v: unknown) -> bool {{\n  return match v {{ is {pat} => true, _ => false, }}\n}}\n")
    };
    let is_call = |recv: &str| format!("pub fn read(v: unknown) -> bool {{\n  return {recv}.is(v)\n}}\n");
    let parse_call = |recv: &str, arg: &str| {
        format!(
            "pub fn read(v: unknown) -> bool {{\n  return match {recv}.parse<{arg}>(v) {{ Ok(_) => true, Err(_) => false, }}\n}}\n"
        )
    };
    let doc_h = "cannot validate `Doc`: field `h` has type `H`, which has no runtime check";
    let u_h = "cannot validate `U`: field `h` of variant `A` has type `H`, which has no runtime check";
    // (dir, imports, the pattern's program body, the call spelling of the
    // same claim if it has one, the message).
    let cases: Vec<(&str, &str, String, Option<String>, &str)> = vec![
        ("record", "", pattern("Doc"), Some(is_call("Doc")), doc_h),
        (
            "alias",
            "",
            pattern("D2"),
            Some(is_call("D2")),
            "cannot validate `D2`: field `h` has type `H`, which has no runtime check",
        ),
        ("union", "", pattern("U"), Some(is_call("U")), u_h),
        (
            "nested",
            "",
            pattern("Outer"),
            Some(is_call("Outer")),
            "cannot validate `Outer`: field `d.h` has type `H`, which has no runtime check",
        ),
        ("array", "", pattern("Array<Doc>"), Some(is_call("Doc")), doc_h),
        (
            "generic",
            "",
            pattern("Box<int>"),
            Some(parse_call("Box", "int")),
            "cannot validate `Box<int>`: `Box`'s field `h` has type `H`, which has no runtime check",
        ),
        (
            "bare-generic",
            "",
            pattern("Box"),
            None,
            "cannot validate `Box`: `Box`'s field `h` has type `H`, which has no runtime check",
        ),
        ("generic-union-arg", "", pattern("Page<U>"), Some(parse_call("Page", "U")), u_h),
        ("generic-record-arg", "", pattern("Page<Doc>"), Some(parse_call("Page", "Doc")), doc_h),
        ("generic-array-arg", "", pattern("Page<Array<U>>"), Some(parse_call("Page", "Array<U>")), u_h),
        ("named", "import model { Doc }\n", pattern("Doc"), Some(is_call("Doc")), doc_h),
        ("named-union", "import model { U }\n", pattern("U"), Some(is_call("U")), u_h),
        (
            "named-generic",
            "import model { Page, U }\n",
            pattern("Page<U>"),
            Some(parse_call("Page", "U")),
            u_h,
        ),
        ("namespace", "import model as m\n", pattern("m.Doc"), Some(is_call("m.Doc")), doc_h),
        (
            "namespace-generic",
            "import model as m\n",
            pattern("m.Box<int>"),
            Some(parse_call("m.Box", "int")),
            "cannot validate `m.Box<int>`: `Box`'s field `h` has type `H`, which has no runtime check",
        ),
        (
            "namespace-generic-arg",
            "import model as m\n",
            pattern("m.Page<m.U>"),
            Some(parse_call("m.Page", "m.U")),
            u_h,
        ),
    ];
    for (dir, imports, body, call, expected) in cases {
        let run = |tag: &str, body: &str| {
            let root = unique_tmp(&format!("g349-{dir}-{tag}"));
            let src = root.join("src");
            let main = if imports.is_empty() {
                format!("module main\n{decls}{body}")
            } else {
                write_file(&src, "model.glyph", &format!("module model\n{decls}"));
                format!("module main\n{imports}{body}")
            };
            write_file(&src, "main.glyph", &main);
            let (code, stderr) = check_no_tsc(&src);
            (code, stderr, main)
        };
        let (code, stderr, main) = run("pattern", &body);
        assert_ne!(code, 0, "{dir}: the pattern must be refused:\n{main}\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: {stderr}");
        assert!(stderr.contains(expected), "{dir}: the message names `{expected}`:\n{stderr}");
        if let Some(call) = call {
            let (call_code, call_stderr, call_main) = run("call", &call);
            assert_ne!(call_code, 0, "{dir}: the call spelling is refused too:\n{call_main}\n{call_stderr}");
            assert_eq!(
                e0304_headline(&stderr),
                e0304_headline(&call_stderr),
                "{dir}: the pattern and the call are refused in the same words"
            );
        }
    }
}

/// G349's own reproduction, run: `{"h": 5}` took the `Doc` arm. With the
/// pattern refused, the program does not build at all, and the same
/// program over a record whose fields are all checked still narrows.
#[test]
fn an_is_pattern_on_a_record_with_an_unchecked_field_does_not_build() {
    let root = unique_tmp("g349-ledger");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/json\nimport std/console\n\
         type H = extern_ts(\"{ k: string }\")\ntype Doc = { h: H, }\n\
         fn f(v: unknown) -> string {\n  return match v { is Doc => \"is Doc\", _ => \"other\", }\n}\n\
         pub fn main() -> void {\n  match json.parse(\"{\\\"h\\\":5}\") {\n    Ok(v) => console.log(f(v)),\n    Err(_) => console.log(\"bad json\"),\n  }\n}\n",
    );
    let (code, stderr) = check_no_tsc(&src);
    assert_ne!(code, 0, "{stderr}");
    assert!(
        stderr.contains("[E0304]")
            && stderr.contains("cannot validate `Doc`: field `h` has type `H`, which has no runtime check"),
        "{stderr}"
    );
}

/// The controls for G349: an `is T` pattern whose descriptor checks every
/// field still compiles and narrows at run time, taking the arm for a value
/// that fits and the fallback for one that breaks a field: a record, a second
/// name, a union, a nested record, an array, and a generic applied to a
/// primitive and to a union, local and imported by name. A bare generic and
/// a namespaced type have no `is` lowering (E0300), so they have no control.
#[test]
fn an_is_pattern_whose_descriptor_checks_every_field_still_narrows() {
    let decls = "pub type Doc = { n: int, }\n\
        pub type D2 = Doc\n\
        pub type V = A({ n: int, }) | B\n\
        pub type Outer = { d: Doc, }\n\
        pub type Box<T> = { v: T, k: string, }\n\
        pub type Page<T> = { items: Array<T>, }\n";
    let main = |head: &str, pat: &str, good: &str, bad: &str| {
        format!(
            "module main\nimport std/io {{ println }}\n{head}\
             fn pick(v: unknown) -> string {{\n  return match v {{ is {pat} => \"yes\", _ => \"no\", }}\n}}\n\
             fn main() {{\n  let good: unknown = {good}\n  let bad: unknown = {bad}\n  println(pick(good))\n  println(pick(bad))\n}}\n"
        )
    };
    let local = |pat: &str, good: &str, bad: &str| main(decls, pat, good, bad);
    let cases: Vec<(&str, Option<String>, String)> = vec![
        ("record", None, local("Doc", "{ n: 1 }", "{ n: 1.5 }")),
        ("alias", None, local("D2", "{ n: 1 }", "{ n: \"x\" }")),
        ("union", None, local("V", "{ tag: \"A\", n: 1 }", "{ tag: \"A\", n: 1.5 }")),
        ("nested", None, local("Outer", "{ d: { n: 1 } }", "{ d: { n: \"x\" } }")),
        ("array", None, local("Array<Doc>", "[{ n: 1 }, { n: 2 }]", "[{ n: 1 }, { n: 1.5 }]")),
        ("generic", None, local("Box<int>", "{ v: 1, k: \"a\" }", "{ v: 1.5, k: \"a\" }")),
        (
            "generic-union-arg",
            None,
            local("Page<V>", "{ items: [{ tag: \"A\", n: 1 }, { tag: \"B\" }] }", "{ items: [{ tag: \"A\", n: 1.5 }] }"),
        ),
        (
            "named",
            Some(format!("module model\n{decls}")),
            main("import model { Doc }\n", "Doc", "{ n: 1 }", "{ n: 1.5 }"),
        ),
        (
            "named-generic",
            Some(format!("module model\n{decls}")),
            main("import model { Page, V }\n", "Page<V>", "{ items: [{ tag: \"B\" }] }", "{ items: [{ tag: \"A\" }] }"),
        ),
    ];
    for (dir, model, main) in cases {
        let root = unique_tmp(&format!("g349-control-{dir}"));
        let src = root.join("src");
        if let Some(model) = model {
            write_file(&src, "model.glyph", &model);
        }
        write_file(&src, "main.glyph", &main);
        assert_eq!(run_main_lines(&src), vec!["yes", "no"], "{dir}:\n{main}");
    }
}

/// A namespaced generic `is w.Box<int>` has no lowering (G356), and is
/// E0300 whether or not this module declares a `Box` of its own. Through
/// 0.1.142 the lowering looked the generic up by its last segment, so with
/// a local `Box` the pattern ran the local `Box.is` under `w.Box`'s name:
/// `{ v: 1 }` took the `w.Box` arm, and `check` and `tsc` both passed
/// (record 107). The judgment still runs first, so a namespaced generic
/// with an unchecked field is E0304 (above).
#[test]
fn a_namespaced_generic_is_pattern_has_no_lowering() {
    let w = "module w\npub type Box<T> = { v: T, k: string, }\n";
    let body = |import: &str, local: &str, pat: &str| {
        format!(
            "module main\n{import}\n{local}pub fn read(v: unknown) -> bool {{\n  return match v {{ is {pat} => true, _ => false, }}\n}}\n"
        )
    };
    for (dir, main) in [
        ("local-box", body("import w", "type Box<T> = { v: T, }\n", "w.Box<int>")),
        ("no-local-box", body("import w", "", "w.Box<int>")),
        ("aliased", body("import w as m", "type Box<T> = { v: T, }\n", "m.Box<int>")),
    ] {
        let root = unique_tmp(&format!("g356-{dir}"));
        let src = root.join("src");
        write_file(&src, "w.glyph", w);
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_ne!(code, 0, "{dir}: refused:\n{main}\n{stderr}");
        assert!(stderr.contains("[E0300]"), "{dir}: E0300:\n{stderr}");
    }
}

/// The program G357's runtime tests run: `u` reads a JSON text into an
/// `unknown`, so the bad value is one a boundary could hand the pattern,
/// and `f` matches it against `pat`, printing which arm ran for the good
/// text and then the bad one.
fn g357_program(decls: &str, pat: &str, good: &str, bad: &str) -> String {
    let lit = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "module main\nimport std/console\nimport std/json\n\
         fn u(t: string) -> unknown {{ return match json.parse(t) {{ Ok(x) => x, Err(e) => \"PARSEERR\", }} }}\n\
         type H = extern_ts(\"{{ k: string }}\")\n{decls}\
         fn f(v: unknown) -> string {{ return match v {{ is {pat} => \"arm\", _ => \"other\", }} }}\n\
         pub fn main() -> void {{\n\
         \x20 console.log(f(u(\"{}\")))\n\
         \x20 console.log(f(u(\"{}\")))\n\
         }}\n",
        lit(good),
        lit(bad)
    )
}

/// G357: `is Record<K, V>` and `is Array<X>` narrow the scrutinee to the
/// container with its parts typed, so each part is checked with `V`'s or
/// `X`'s check, at any depth and inside a generic's argument. Through
/// 0.1.144 `is Record<string, int>` tested only "an object, not an array"
/// and took its arm for `{"a": "x"}`, and `is Array<int>` (with any element
/// type `is` had no lowering for of its own) was `Array.isArray` alone. A
/// container of `unknown` is checked by its shape, which is all its type
/// says, and the bad value for those two is one of the wrong shape.
#[test]
fn an_is_container_pattern_checks_every_part_at_run_time() {
    let doc = "type Doc = { n: int, }\n";
    let bx = "type Box<T> = { v: T, k: string, }\n";
    let cases: Vec<(&str, &str, &str, &str, &str)> = vec![
        ("rec-int", "", "Record<string, int>", r#"{"a":1}"#, r#"{"a":"x"}"#),
        ("rec-int-float", "", "Record<string, int>", r#"{"a":1}"#, r#"{"a":1.5}"#),
        ("rec-doc", doc, "Record<string, Doc>", r#"{"a":{"n":1}}"#, r#"{"a":{"n":"x"}}"#),
        ("rec-arr-int", "", "Record<string, Array<int>>", r#"{"a":[1]}"#, r#"{"a":["x"]}"#),
        ("rec-lit", "", "Record<string, \"x\" | \"y\">", r#"{"a":"x"}"#, r#"{"a":"q"}"#),
        ("rec-unknown", "", "Record<string, unknown>", r#"{"a":1}"#, "[1]"),
        ("arr-int", "", "Array<int>", "[1]", r#"["x"]"#),
        ("arr-string", "", "Array<string>", r#"["a"]"#, "[1]"),
        ("arr-lit", "", "Array<\"x\" | \"y\">", r#"["x"]"#, r#"["q"]"#),
        ("arr-nullable", "", "Array<Nullable<int>>", "[null,1]", r#"["x"]"#),
        ("arr-doc", doc, "Array<Doc>", r#"[{"n":1}]"#, r#"[{"n":"x"}]"#),
        ("arr-arr-doc", doc, "Array<Array<Doc>>", r#"[[{"n":1}]]"#, r#"[[{"n":"x"}]]"#),
        ("arr-rec-int", "", "Array<Record<string, int>>", r#"[{"a":1}]"#, r#"[{"a":"x"}]"#),
        ("arr-unknown", "", "Array<unknown>", "[1]", "{}"),
        ("arr-arr-unknown", "", "Array<Array<unknown>>", "[[1]]", "[1]"),
        ("box-rec-int", bx, "Box<Record<string, int>>", r#"{"v":{"a":1},"k":"s"}"#, r#"{"v":{"a":"x"},"k":"s"}"#),
        ("box-arr-int", bx, "Box<Array<int>>", r#"{"v":[1],"k":"s"}"#, r#"{"v":["x"],"k":"s"}"#),
    ];
    for (dir, decls, pat, good, bad) in cases {
        let root = unique_tmp(&format!("g357-run-{dir}"));
        let src = root.join("src");
        let main = g357_program(decls, pat, good, bad);
        write_file(&src, "main.glyph", &main);
        assert_eq!(run_main_lines(&src), vec!["arm", "other"], "{dir}:\n{main}");
    }
}

/// G357: a container pattern whose part type has no runtime check is
/// E0304, as `json.parse<T>` of the same type is, at the top, in element
/// position, inside a generic's argument and in a `<case>`. A part that is
/// a record with an unchecked field is refused in the words `Doc.is(v)`
/// gets. Through 0.1.144 every one of these exited 0 and took its arm for
/// the bad value (records 107 and 108's probes).
#[test]
fn an_is_container_pattern_whose_part_has_no_check_is_e0304() {
    let doc_u = "type Doc = { h: H, }\n";
    let bx = "type Box<T> = { v: T, k: string, }\n";
    let doc_h = "cannot validate `Doc`: field `h` has type `H`, which has no runtime check";
    let box_doc = format!("{bx}{doc_u}");
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        ("arr-h", "", "Array<H>", "cannot validate `Array<H>`: its type argument `H` has no runtime check"),
        ("rec-h", "", "Record<string, H>", "cannot validate `Record<string, H>`: its type argument `H` has no runtime check"),
        ("arr-arr-h", "", "Array<Array<H>>", "cannot validate `Array<H>`: its type argument `H` has no runtime check"),
        ("rec-arr-h", "", "Record<string, Array<H>>", "cannot validate `Array<H>`: its type argument `H` has no runtime check"),
        ("rec-doc", doc_u, "Record<string, Doc>", doc_h),
        ("arr-rec-doc", doc_u, "Array<Record<string, Doc>>", doc_h),
        ("arr-arr-doc", doc_u, "Array<Array<Doc>>", doc_h),
        ("box-rec-doc", &box_doc, "Box<Record<string, Doc>>", doc_h),
        ("box-arr-h", bx, "Box<Array<H>>", "its type argument `Array<H>` has no runtime check"),
    ];
    for (dir, decls, pat, expected) in cases {
        let root = unique_tmp(&format!("g357-refuse-{dir}"));
        let src = root.join("src");
        let main = g357_program(decls, pat, "[]", "[]");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        let headline = e0304_headline(&stderr).unwrap_or_else(|| panic!("{dir}: E0304:\n{stderr}"));
        assert!(headline.contains(expected), "{dir}: `{expected}` in:\n{headline}");
    }

    // The same judgment in a `<case is …>`, which lowers through the same
    // type-guard chain.
    let root = unique_tmp("g357-refuse-jsx");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\ntype H = extern_ts(\"{ k: string }\")\ntype Props = { v: unknown, }\n\
         pub component C(props: Props) -> Component {\n\
         \x20 return <match value={props.v}>\n\
         \x20\x20\x20 <case is Record<string, H>><p>a</p></case>\n\
         \x20\x20\x20 <case else><p>b</p></case>\n\
         \x20 </match>\n}\n",
    );
    let (code, stderr) = check_no_tsc(&src);
    assert_eq!(code, 1, "jsx: refused:\n{stderr}");
    let headline = e0304_headline(&stderr).unwrap_or_else(|| panic!("jsx: E0304:\n{stderr}"));
    assert!(
        headline.contains("cannot validate `Record<string, H>`: its type argument `H` has no runtime check"),
        "jsx:\n{headline}"
    );
}

/// G366: a namespaced generic application is never checked with a local
/// generic of the same name. Through 0.1.144 the field check looked the
/// generic up by the last segment of its path, so with `type Box<T> = { q:
/// T }` declared here and `w.Box<T> = { v: T }` imported, `json.parse<
/// w.Box<int>>` returned `Err` for `{"v":1}` and `Ok` for `{"q":1}`, and so
/// did the same type inside an `Array`, a record field typed `w.Box<int>`,
/// and (from G357's part checks) a wrapped part of an `is` container
/// pattern. Each is E0304 now, the refusal the same spellings already got
/// without a local `Box` (G354), which stays as it was. A namespaced type
/// as the argument of a local generic is not a namespaced application and
/// still validates.
#[test]
fn a_namespaced_generic_is_never_checked_with_a_local_generic_of_the_same_name() {
    let w = "module w\npub type Box<T> = { v: T, }\npub type P = { n: int, }\n";
    let header = "module main\nimport std/console\nimport std/json\nimport w\n";
    let jp = |ty: &str| {
        format!("pub fn g(t: string) -> bool {{ return match json.parse<{ty}>(t) {{ Ok(_) => true, Err(_) => false, }} }}\n")
    };
    let is = |ty: &str| {
        format!("pub fn f(v: unknown) -> bool {{ return match v {{ is {ty} => true, _ => false, }} }}\n")
    };
    let local = "type Box<T> = { q: T, }\n";
    let cases: Vec<(&str, String)> = vec![
        ("json-parse", format!("{local}{}", jp("w.Box<int>"))),
        ("json-parse-array", format!("{local}{}", jp("Array<w.Box<int>>"))),
        ("record-field", format!("{local}type Doc = {{ b: w.Box<int>, }}\n{}", jp("Doc"))),
        ("is-array-part", format!("{local}{}", is("Array<Nullable<w.Box<int>>>"))),
        ("is-record-part", format!("{local}{}", is("Record<string, Nullable<w.Box<int>>>"))),
        ("no-shadow", jp("w.Box<int>")),
    ];
    for (dir, body) in cases {
        let root = unique_tmp(&format!("g366-{dir}"));
        let src = root.join("src");
        write_file(&src, "w.glyph", w);
        let main = format!("{header}{body}");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        assert!(stderr.contains("[E0304]"), "{dir}: E0304:\n{stderr}");
    }

    // A local generic applied to a namespaced record still validates.
    let root = unique_tmp("g366-local-box-of-ns");
    let src = root.join("src");
    write_file(&src, "w.glyph", w);
    write_file(
        &src,
        "main.glyph",
        &format!(
            "{header}type Box<T> = {{ q: T, }}\n{}\
             pub fn main() -> void {{\n\
             \x20 console.log(match g(\"{{\\\"q\\\":{{\\\"n\\\":1}}}}\") {{ true => \"Ok\", false => \"Err\", }})\n\
             \x20 console.log(match g(\"{{\\\"q\\\":{{\\\"n\\\":\\\"x\\\"}}}}\") {{ true => \"Ok\", false => \"Err\", }})\n\
             }}\n",
            jp("Box<w.P>")
        ),
    );
    assert_eq!(run_main_lines(&src), vec!["Ok", "Err"]);
}

/// G366 with its bare name in scope: a namespaced generic application is
/// checked when its bare name resolves to the same declaration, so
/// `json.parse<m.Page<m.User>>` beside `import model { Page, User }`
/// validates, a good document `Ok` and a bad one `Err` (0.1.144's G350
/// paths). When the bare name is a different generic, `Page` imported from
/// another module, the application is E0304 rather than checked with that
/// other `Page`.
#[test]
fn a_namespaced_generic_is_checked_only_when_its_bare_name_is_the_same_generic() {
    let model = "module model\npub type User = { id: int, }\npub type Page<T> = { items: Array<T>, }\n";
    let root = unique_tmp("g366-same-bare-name");
    let src = root.join("src");
    write_file(&src, "model.glyph", model);
    write_file(
        &src,
        "main.glyph",
        "module main\nimport std/console\nimport std/json\nimport model { Page, User }\nimport model as m\n\
         pub fn g(t: string) -> string { return match json.parse<m.Page<m.User>>(t) { Ok(_) => \"Ok\", Err(_) => \"Err\", } }\n\
         pub fn main() -> void {\n\
         \x20 console.log(g(\"{\\\"items\\\":[{\\\"id\\\":1}]}\"))\n\
         \x20 console.log(g(\"{\\\"items\\\":[{\\\"id\\\":\\\"x\\\"}]}\"))\n\
         }\n",
    );
    assert_eq!(run_main_lines(&src), vec!["Ok", "Err"]);

    let root = unique_tmp("g366-other-bare-name");
    let src = root.join("src");
    write_file(&src, "model.glyph", model);
    write_file(&src, "other.glyph", "module other\npub type Page<T> = { rows: Array<T>, }\n");
    let main = "module main\nimport std/json\nimport model as m\nimport other { Page }\n\
         pub fn g(t: string) -> bool { return match json.parse<m.Page<int>>(t) { Ok(_) => true, Err(_) => false, } }\n\
         pub fn h(p: Page<int>) -> int { return 0 }\n";
    write_file(&src, "main.glyph", main);
    let (code, stderr) = check_no_tsc(&src);
    assert_eq!(code, 1, "refused, not checked with `other`'s `Page`:\n{main}\n{stderr}");
    assert!(stderr.contains("[E0304]"), "{stderr}");
}

/// G362: an `is` pattern whose type, or any part of it, is a type parameter
/// of the enclosing callable is E0304, the rule `json.parse<T>` has. A type
/// parameter is erased at run time, so there is nothing to check. The name
/// is read by scope, so inside `fn h<Doc>` the pattern `is Doc` names the
/// parameter even beside a module-level `type Doc`; through 0.1.145 it
/// lowered to that record's `Doc.is(v)` while the checker narrowed `v` to
/// the parameter, so `check --no-tsc` exited 0 and the program crashed or,
/// in part position (`is Array<T>`), passed full `check` and checked the
/// elements against the record (records 108 and 111). With no record of the
/// same name the bare `is T` was E0300 and `is Array<T>` already E0304; both
/// read the same as the shadowed forms now.
#[test]
fn an_is_pattern_on_a_type_parameter_is_e0304_whatever_shares_its_name() {
    let head = "module main\nimport std/console\n";
    let fn_is = |gen: &str, pat: &str| {
        format!("fn f<{gen}>(v: unknown) -> string {{ return match v {{ is {pat} => \"arm\", _ => \"other\", }} }}\n")
    };
    let bare = |p: &str, f: &str| {
        format!("cannot validate `{p}`: `{p}` is a type parameter of `{f}`, which has no runtime check")
    };
    let part = |c: &str, p: &str| {
        format!("cannot validate `{c}`: its type argument `{p}` is a type parameter of `f`, which has no runtime check")
    };
    let doc = "type Doc = { n: int, }\n";
    let t = "type T = { n: int, }\n";
    let bx = "type Box<X> = { v: X, }\n";
    let cases: Vec<(&str, String, String)> = vec![
        // Record 108's probe `fa-typeparam2`.
        (
            "fa-typeparam2",
            format!("{head}import std/json\nimport std/string\n\
                 fn u(t: string) -> unknown {{ return match json.parse(t) {{ Ok(x) => x, Err(e) => \"PARSEERR\", }} }}\n\
                 {doc}fn h<Doc>(v: unknown, d: Doc) -> Doc {{ return match v {{ is Doc => v, _ => d, }} }}\n\
                 pub fn main() -> void {{\n  let s: string = h<string>(u(\"{{\\\"n\\\":1}}\"), \"fallback\")\n  console.log(string.upper(s))\n}}\n"),
            bare("Doc", "h"),
        ),
        // Record 108's probe `fa-typeparam3`: the record's own unchecked
        // field is not what the pattern names.
        (
            "fa-typeparam3",
            "module main\ntype H = extern_ts(\"{ k: string }\")\ntype Doc = { h: H, }\n\
             pub fn h<Doc>(v: unknown, d: Doc) -> Doc { return match v { is Doc => v, _ => d, } }\n"
                .to_string(),
            bare("Doc", "h"),
        ),
        ("bare-no-record", format!("{head}{}", fn_is("T", "T")), bare("T", "f")),
        // Record 111's probe `q/t2`.
        ("array-shadow", format!("{head}{t}{}", fn_is("T", "Array<T>")), part("Array<T>", "T")),
        ("array-no-record", format!("{head}{}", fn_is("T", "Array<T>")), part("Array<T>", "T")),
        ("record-shadow", format!("{head}{t}{}", fn_is("T", "Record<string, T>")), part("Record<string, T>", "T")),
        ("box-shadow", format!("{head}{t}{bx}{}", fn_is("T", "Box<T>")), part("Box<T>", "T")),
        ("box-no-record", format!("{head}{bx}{}", fn_is("T", "Box<T>")), part("Box<T>", "T")),
        ("option-shadow", format!("{head}{t}{}", fn_is("T", "Array<Option<T>>")), part("Option<T>", "T")),
        ("nullable-shadow", format!("{head}{t}{}", fn_is("T", "Array<Nullable<T>>")), part("Nullable<T>", "T")),
        (
            "nested-shadow",
            format!("{head}{t}{}", fn_is("T", "Record<string, Array<T>>")),
            part("Array<T>", "T"),
        ),
        (
            "box-array-shadow",
            format!("{head}{t}{bx}{}", fn_is("T", "Box<Array<T>>")),
            part("Array<T>", "T"),
        ),
        // The second of two type parameters.
        ("second-param", format!("{head}{t}{}", fn_is("A, T", "Array<T>")), part("Array<T>", "T")),
        // A lambda inside the generic function sees its type parameters.
        (
            "nested-lambda",
            format!("{head}import std/array\n{doc}\
                 fn f<Doc>(vs: Array<unknown>) -> Array<string> {{ return array.map(vs, fn(v) -> string {{ return match v {{ is Doc => \"doc\", _ => \"other\", }} }}) }}\n"),
            bare("Doc", "f"),
        ),
        // A `<case is T>` in a generic component.
        (
            "jsx-case",
            format!("module main\n{doc}\
                 pub component V<Doc>(props: {{ v: unknown, }}) -> Component {{\n\
                 \x20 return <match value={{props.v}}>\n\
                 \x20\x20\x20 <case is Doc><p>doc</p></case>\n\
                 \x20\x20\x20 <case else><p>other</p></case>\n\
                 \x20 </match>\n}}\n"),
            bare("Doc", "V"),
        ),
        (
            "jsx-case-part",
            format!("module main\n{doc}\
                 pub component V<Doc>(props: {{ v: unknown, }}) -> Component {{\n\
                 \x20 return <match value={{props.v}}>\n\
                 \x20\x20\x20 <case is Array<Doc>><p>doc</p></case>\n\
                 \x20\x20\x20 <case else><p>other</p></case>\n\
                 \x20 </match>\n}}\n"),
            "cannot validate `Array<Doc>`: its type argument `Doc` is a type parameter of `V`, which has no runtime check".to_string(),
        ),
    ];
    for (dir, main, expected) in cases {
        let root = unique_tmp(&format!("g362-{dir}"));
        let src = root.join("src");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        let headline = e0304_headline(&stderr).unwrap_or_else(|| panic!("{dir}: E0304:\n{main}\n{stderr}"));
        assert!(headline.contains(&expected), "{dir}: `{expected}` in:\n{headline}");
    }
}

/// G362's controls: the record a type parameter shadows is still the record
/// everywhere the parameter is not in scope. `is Doc` outside any generic,
/// and inside a generic whose parameter has another name, checks `Doc`, so
/// the good document takes the arm and the bad one does not.
#[test]
fn an_is_pattern_naming_a_record_outside_a_same_named_type_parameter_still_checks_it() {
    let head = "module main\nimport std/console\nimport std/json\n\
                fn u(t: string) -> unknown { return match json.parse(t) { Ok(x) => x, Err(e) => \"PARSEERR\", } }\n\
                type Doc = { n: int, }\n";
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        ("outside", "fn g(v: unknown) -> string { return match v { is Doc => \"arm\", _ => \"other\", } }\n", "g", r#"{"n":1}"#),
        ("other-param", "fn g<U>(v: unknown, d: U) -> string { return match v { is Doc => \"arm\", _ => \"other\", } }\n", "g<string>", r#"{"n":1}"#),
        ("other-param-array", "fn g<U>(v: unknown, d: U) -> string { return match v { is Array<Doc> => \"arm\", _ => \"other\", } }\n", "g<string>", r#"[{"n":1}]"#),
    ];
    for (dir, f, call, good) in cases {
        let bad = good.replace('1', "\"x\"");
        let lit = |s: &str| s.replace('"', "\\\"");
        let args = |doc: &str| match call {
            "g" => format!("u(\"{}\")", lit(doc)),
            _ => format!("u(\"{}\"), \"d\"", lit(doc)),
        };
        let main = format!(
            "{head}{f}pub fn main() -> void {{\n  console.log({call}({}))\n  console.log({call}({}))\n}}\n",
            args(good),
            args(&bad)
        );
        let root = unique_tmp(&format!("g362-control-{dir}"));
        let src = root.join("src");
        write_file(&src, "main.glyph", &main);
        assert_eq!(run_main_lines(&src), vec!["arm", "other"], "{dir}:\n{main}");
    }
}

/// G362 on every other path that trusts a descriptor: inside `fn h<Doc>`,
/// `json.parse<Doc>`, a container of the parameter, a generic descriptor's
/// type argument, and `Doc.parse`/`Doc.is`/`Doc.schema` (the value `Doc`
/// is the parameter there, as the checker reads it) are E0304 in the words
/// the `is` pattern gets. Through 0.1.145 each ran the module's `Doc`
/// check and exited 0, and `Doc.parse(v)` returned the record typed as the
/// parameter, which full `check` caught as TS2719 and `run --no-tsc` crashed
/// on.
#[test]
fn a_descriptor_check_on_a_type_parameter_is_e0304_whatever_shares_its_name() {
    let head = "module main\nimport std/json\ntype Doc = { n: int, }\ntype Box<X> = { v: X, }\n";
    let body = |b: &str| format!("{head}pub fn h<Doc>(t: string, v: unknown) -> bool {{ return {b} }}\n");
    let bare = "cannot validate `Doc`: `Doc` is a type parameter of `h`, which has no runtime check".to_string();
    let part = |c: &str| {
        format!("cannot validate `{c}`: its type argument `Doc` is a type parameter of `h`, which has no runtime check")
    };
    let ok = "{ Ok(_) => true, Err(_) => false, }";
    let cases: Vec<(&str, String, String)> = vec![
        // Record 108's follow-up probe `jp_shadow`.
        ("json-parse", body(&format!("match json.parse<Doc>(t) {ok}")), bare.clone()),
        ("json-parse-array", body(&format!("match json.parse<Array<Doc>>(t) {ok}")), part("Array<Doc>")),
        ("json-parse-option", body(&format!("match json.parse<Option<Doc>>(t) {ok}")), part("Option<Doc>")),
        (
            "json-parse-named",
            format!("module main\nimport std/json {{ parse }}\ntype Doc = {{ n: int, }}\n\
                 pub fn h<Doc>(t: string) -> bool {{ return match parse<Doc>(t) {ok} }}\n"),
            bare.clone(),
        ),
        // `ta_shadow`.
        ("generic-type-argument", body(&format!("match Box.parse<Doc>(v) {ok}")), part("Box<Doc>")),
        ("generic-type-argument-part", body(&format!("match Box.parse<Array<Doc>>(v) {ok}")), part("Array<Doc>")),
        ("value-parse", body(&format!("match Doc.parse(v) {ok}")), bare.clone()),
        ("value-is", body("Doc.is(v)"), bare.clone()),
        ("value-schema", body(&format!("match json.parse_with(t, Doc.schema) {ok}")), bare.clone()),
        (
            "value-schema-array",
            body(&format!("match json.parse_with(t, Doc.schema.array()) {ok}")),
            bare.clone(),
        ),
        // A lambda inside the generic function.
        (
            "lambda-json-parse",
            format!("{head}import std/array\n\
                 pub fn h<Doc>(ts: Array<string>) -> Array<bool> {{ return array.map(ts, fn(t) -> bool {{ return match json.parse<Doc>(t) {ok} }}) }}\n"),
            bare.clone(),
        ),
    ];
    for (dir, main, expected) in cases {
        let root = unique_tmp(&format!("g362-desc-{dir}"));
        let src = root.join("src");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        let headline = e0304_headline(&stderr).unwrap_or_else(|| panic!("{dir}: E0304:\n{main}\n{stderr}"));
        assert!(headline.contains(&expected), "{dir}: `{expected}` in:\n{headline}");
    }
}

/// The controls for the test above: the same calls on the module's `Doc`
/// outside any generic, and inside a generic whose parameter has another
/// name, still validate, so the good input passes and the bad one fails.
#[test]
fn a_descriptor_check_on_a_record_outside_a_same_named_type_parameter_still_validates() {
    let head = "module main\nimport std/console\nimport std/json\n\
                type Doc = { n: int, }\ntype Box<X> = { v: X, }\n\
                fn u(t: string) -> unknown { return match json.parse(t) { Ok(x) => x, Err(e) => \"PARSEERR\", } }\n";
    let ok = "{ Ok(_) => \"ok\", Err(_) => \"err\", }";
    let checks: Vec<(&str, String, &str, &str)> = vec![
        ("json-parse", format!("match json.parse<Doc>(t) {ok}"), r#"{"n":1}"#, r#"{"n":"x"}"#),
        ("json-parse-array", format!("match json.parse<Array<Doc>>(t) {ok}"), r#"[{"n":1}]"#, r#"[{"n":"x"}]"#),
        ("generic-type-argument", format!("match Box.parse<Doc>(u(t)) {ok}"), r#"{"v":{"n":1}}"#, r#"{"v":{"n":"x"}}"#),
        ("value-parse", format!("match Doc.parse(u(t)) {ok}"), r#"{"n":1}"#, r#"{"n":"x"}"#),
        ("value-is", "match Doc.is(u(t)) { true => \"ok\", false => \"err\", }".to_string(), r#"{"n":1}"#, r#"{"n":"x"}"#),
        ("value-schema", format!("match json.parse_with(t, Doc.schema) {ok}"), r#"{"n":1}"#, r#"{"n":"x"}"#),
    ];
    let lit = |s: &str| s.replace('"', "\\\"");
    for (dir, check, good, bad) in checks {
        for (shape, sig, call) in [("outside", "fn g(t: string)", "g"), ("other-param", "fn g<U>(t: string)", "g<string>")] {
            let main = format!(
                "{head}{sig} -> string {{ return {check} }}\n\
                 pub fn main() -> void {{\n  console.log({call}(\"{}\"))\n  console.log({call}(\"{}\"))\n}}\n",
                lit(good),
                lit(bad)
            );
            let root = unique_tmp(&format!("g362-desc-control-{dir}-{shape}"));
            let src = root.join("src");
            write_file(&src, "main.glyph", &main);
            assert_eq!(run_main_lines(&src), vec!["ok", "err"], "{dir} {shape}:\n{main}");
        }
    }
}

/// The G367 parity program for one spelling of a `Record`'s key type `k`:
/// every path that checks a `Record<k, int>` (its own `json.parse`, a
/// record holding it through `json.parse`, `parse`, `is`, an `is` pattern
/// and `json.parse_with` of its schema, an `is Record` pattern, an
/// `Array`'s element, a `Nullable` and an `Option` field, a generic's
/// argument through `json.parse` and its descriptor, a tagged union's
/// payload, and an alias of another module that spells the key there) runs
/// on each input, and prints one `label path verdict` line per pair.
fn g367_program(k: &str, decls: &str, inputs: &[(&str, &str, &str)]) -> String {
    let rec = format!("Record<{k}, int>");
    let ok = "{ Ok(_) => \"Ok\", Err(_) => \"Err\", }";
    let paths: Vec<(&str, String)> = vec![
        ("jp", format!("match json.parse<{rec}>(txt) {ok}")),
        ("jdoc", format!("match json.parse<Doc>(wm(txt)) {ok}")),
        ("docparse", format!("match Doc.parse(u(wm(txt))) {ok}")),
        ("schema", format!("match json.parse_with(wm(txt), Doc.schema) {ok}")),
        ("jarr", format!("match json.parse<Array<{rec}>>(\"[\" + txt + \"]\") {ok}")),
        ("jnull", format!("match json.parse<DN>(wm(txt)) {ok}")),
        ("jopt", format!("match json.parse<DO>(\"{{\\\"m\\\":{{\\\"tag\\\":\\\"Some\\\",\\\"value\\\":\" + txt + \"}}}}\") {ok}")),
        ("jbox", format!("match json.parse<Box<{rec}>>(wv(txt)) {ok}")),
        ("boxparse", format!("match Box.parse<{rec}>(u(wv(txt))) {ok}")),
        ("junion", format!("match json.parse<U>(\"{{\\\"tag\\\":\\\"A\\\",\\\"value\\\":\" + txt + \"}}\") {ok}")),
        ("hop", format!("match json.parse<M>(txt) {ok}")),
        ("docis", "b(Doc.is(u(wm(txt))))".to_string()),
        ("isdoc", "b(match u(wm(txt)) { is Doc => true, _ => false, })".to_string()),
        ("isrec", format!("b(match u(txt) {{ is {rec} => true, _ => false, }})")),
        ("isarr", format!("b(match u(\"[\" + txt + \"]\") {{ is Array<{rec}> => true, _ => false, }})")),
    ];
    let mut out = format!(
        "module main\nimport std/console\nimport std/json\nimport s {{ M }}\n{decls}\
         type Doc = {{ m: {rec}, }}\n\
         type DN = {{ m: Nullable<{rec}>, }}\n\
         type DO = {{ m: Option<{rec}>, }}\n\
         type Box<T> = {{ v: T, }}\n\
         type U = A({rec}) | B\n\
         fn u(txt: string) -> unknown {{ return match json.parse(txt) {{ Ok(x) => x, Err(e) => \"PARSEERR\", }} }}\n\
         fn wm(txt: string) -> string {{ return \"{{\\\"m\\\":\" + txt + \"}}\" }}\n\
         fn wv(txt: string) -> string {{ return \"{{\\\"v\\\":\" + txt + \"}}\" }}\n\
         fn b(r: bool) -> string {{ return match r {{ true => \"Ok\", false => \"Err\", }} }}\n"
    );
    for (name, check) in &paths {
        out.push_str(&format!("fn {name}(txt: string) -> string {{ return {check} }}\n"));
    }
    out.push_str("fn all(l: string, txt: string) -> void {\n");
    for (name, _) in &paths {
        out.push_str(&format!("  console.log(l + \" {name} \" + {name}(txt))\n"));
    }
    out.push_str("  return void\n}\npub fn main() -> void {\n");
    for (label, text, _) in inputs {
        let text = text.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("  all(\"{label}\", \"{text}\")\n"));
    }
    out.push_str("}\n");
    out
}

/// One parity case: its name, `K` as main spells it, main's declarations,
/// module `t` if any, module `s`'s body, and the inputs with their verdicts.
type G367Case<'a> = (&'a str, &'a str, String, Option<&'a str>, String, &'a [(&'a str, &'a str, &'a str)]);

/// G367: a `Record`'s keys are checked against its key type `K`, with one
/// verdict on every path that checks the `Record`, however `K` is spelled.
/// A string-literal union `K` (declared here, imported by name, through a
/// namespace, an alias chain, written inline, a single literal) admits
/// exactly its members, every one present; a `where`-refined string admits
/// each key its predicate passes; `int`, `number` and a refinement of one
/// admit the canonical spelling of a number its check passes. `string`
/// admits any key, as before. Through 0.1.147 every path returned `Ok` for
/// a stray key, a missing member and a non-numeric key alike (record 113).
#[test]
fn a_record_key_type_is_checked_with_one_verdict_on_every_path() {
    let tier = "pub type Tier = \"a\" | \"b\"\n";
    let lit: &[(&str, &str, &str)] = &[
        ("good", r#"{"a":1,"b":2}"#, "Ok"),
        ("badkey", r#"{"a":1,"z":2}"#, "Err"),
        ("missing", r#"{"a":1}"#, "Err"),
        ("empty", "{}", "Err"),
        ("extra", r#"{"a":1,"b":2,"c":3}"#, "Err"),
        ("badvalue", r#"{"a":1,"b":"x"}"#, "Err"),
    ];
    let one: &[(&str, &str, &str)] = &[
        ("good", r#"{"a":1}"#, "Ok"),
        ("badkey", r#"{"z":1}"#, "Err"),
        ("missing", "{}", "Err"),
        ("extra", r#"{"a":1,"c":2}"#, "Err"),
    ];
    let slug: &[(&str, &str, &str)] = &[
        ("good", r#"{"ab":1,"cd":2}"#, "Ok"),
        ("badkey", r#"{"ab":1,"z":2}"#, "Err"),
        ("empty", "{}", "Ok"),
    ];
    let int: &[(&str, &str, &str)] = &[
        ("good", r#"{"1":1,"-2":2}"#, "Ok"),
        ("badkey", r#"{"x":1}"#, "Err"),
        ("lead0", r#"{"01":1}"#, "Err"),
        ("frac", r#"{"1.5":1}"#, "Err"),
        ("empty", "{}", "Ok"),
    ];
    let number: &[(&str, &str, &str)] = &[
        ("good", r#"{"1.5":1,"-2":2}"#, "Ok"),
        ("badkey", r#"{"x":1}"#, "Err"),
        ("lead0", r#"{"01":1}"#, "Err"),
    ];
    let small: &[(&str, &str, &str)] = &[
        ("good", r#"{"1":1,"9":2}"#, "Ok"),
        ("badkey", r#"{"12":1}"#, "Err"),
        ("word", r#"{"x":1}"#, "Err"),
    ];
    let string: &[(&str, &str, &str)] = &[
        ("good", r#"{"a":1,"zzz":2}"#, "Ok"),
        ("empty", "{}", "Ok"),
        ("badvalue", r#"{"a":"x"}"#, "Err"),
    ];
    let small_t = "pub type Small = int where value < 10\n";
    let slug_t = "pub type Slug = string where value.length > 1\n";
    // (case, K in main, main's declarations, module `t` if any, module `s`'s
    // body, which exports `M` as the `Record` with `K` spelled in `s`, inputs)
    let cases: Vec<G367Case> = vec![
        ("local", "Tier", "type Tier = \"a\" | \"b\"\n".into(), None, format!("{tier}pub type M = Record<Tier, int>\n"), lit),
        ("imported", "Tier", "import t { Tier }\n".into(), Some(tier), "import t { Tier }\npub type M = Record<Tier, int>\n".into(), lit),
        ("namespaced", "t.Tier", "import t\n".into(), Some(tier), "import t\npub type M = Record<t.Tier, int>\n".into(), lit),
        ("alias-chain", "T3", "type Tier = \"a\" | \"b\"\ntype T2 = Tier\ntype T3 = T2\n".into(), None,
         format!("{tier}pub type T2 = Tier\npub type M = Record<T2, int>\n"), lit),
        ("imported-alias", "T2", "import t { Tier }\ntype T2 = Tier\n".into(), Some(tier),
         "import t\npub type T2 = t.Tier\npub type M = Record<T2, int>\n".into(), lit),
        ("inline", "\"a\" | \"b\"", String::new(), None, "pub type M = Record<\"a\" | \"b\", int>\n".into(), lit),
        ("single", "One", "type One = \"a\"\n".into(), None, "pub type One = \"a\"\npub type M = Record<One, int>\n".into(), one),
        ("slug", "Slug", slug_t.trim_start_matches("pub ").into(), None, format!("{slug_t}pub type M = Record<Slug, int>\n"), slug),
        ("ns-slug", "t.Slug", "import t\n".into(), Some(slug_t), "import t\npub type M = Record<t.Slug, int>\n".into(), slug),
        ("int", "int", String::new(), None, "pub type M = Record<int, int>\n".into(), int),
        ("number", "number", String::new(), None, "pub type M = Record<number, int>\n".into(), number),
        ("small", "Small", small_t.trim_start_matches("pub ").into(), None, format!("{small_t}pub type M = Record<Small, int>\n"), small),
        ("imported-small", "Small", "import t { Small }\n".into(), Some(small_t), "import t { Small }\npub type M = Record<Small, int>\n".into(), small),
        ("ns-small", "t.Small", "import t\n".into(), Some(small_t), "import t\npub type M = Record<t.Small, int>\n".into(), small),
        ("string", "string", String::new(), None, "pub type M = Record<string, int>\n".into(), string),
    ];
    std::thread::scope(|scope| {
        let runs: Vec<_> = cases
            .iter()
            .map(|(case, k, decls, t, s, inputs)| {
                scope.spawn(move || {
                    let root = unique_tmp(&format!("g367-{case}"));
                    let src = root.join("src");
                    let main = g367_program(k, decls, inputs);
                    write_file(&src, "main.glyph", &main);
                    write_file(&src, "s.glyph", &format!("module s\n{s}"));
                    if let Some(t) = t {
                        write_file(&src, "t.glyph", &format!("module t\n{t}"));
                    }
                    (case, inputs, main, run_main_lines(&src))
                })
            })
            .collect();
        for run in runs {
            let (case, inputs, main, lines) = run.join().expect("a parity run panicked");
            for (label, _, expected) in inputs.iter() {
                let verdicts: Vec<&String> = lines
                    .iter()
                    .filter(|l| l.split(' ').next() == Some(label))
                    .collect();
                assert_eq!(verdicts.len(), 15, "{case} {label}: one line per path:\n{lines:?}\n{main}");
                for line in verdicts {
                    assert!(line.ends_with(&format!(" {expected}")), "{case} {label}: expected {expected}, got `{line}`\n{main}");
                }
            }
        }
    });
}

/// G377: a `where` over a string-literal union, a single literal included,
/// is E0300 at the declaration under both `check` modes, by every spelling:
/// inline, a local alias, an alias chain, imported by name, namespaced, a
/// second name a sibling declares, and a chain of three modules. Through
/// 0.1.151 each checked clean; `tsc` read `Tier where value != "c"` as all
/// of `Tier`, the run-time check refused `"c"`, and `json.parse<Record<T2,
/// int>>` had no document both would accept (record 119). The diagnostic
/// names the declaration and its base as written, lists the members the
/// base resolves to wherever they are declared, quotes the predicate as
/// written, and offers no subset (record 120).
#[test]
fn a_where_over_a_string_literal_union_is_e0300_by_every_spelling() {
    let tiers = "module tiers\npub type Tier = \"a\" | \"b\" | \"c\"\npub type One = \"a\"\n";
    let hop = "module hop\nimport tiers { Tier }\npub type Tier2 = Tier\n";
    let x1 = "module x1\npub type Deep = \"p\" | \"q\"\n";
    let x2 = "module x2\nimport x1 as q\npub type Mid = q.Deep\n";
    let abc = "`\"a\" | \"b\" | \"c\"`";
    // (case, main's declarations, headline start, text the help holds)
    let cases: Vec<(&str, String, String, String)> = vec![
        ("inline", "type T2 = \"a\" | \"b\" where value != \"a\"\n".into(),
         "`T2` refines `\"a\" | \"b\"`, a string-literal union".into(), "`value != \"a\"`".into()),
        ("single-literal", "type One = \"a\"\ntype T2 = One where value.length > 0\n".into(),
         "`T2` refines `One`, a single string literal".into(), "`One` is `\"a\"`".into()),
        ("local", "type Plan = \"free\" | \"pro\" | \"team\"\npub type Paid = Plan where value != \"free\"\n".into(),
         "`Paid` refines `Plan`, a string-literal union".into(),
         "Declare `Paid` as the members of `Plan` it keeps, written as a union of their own. `Plan` is `\"free\" | \"pro\" | \"team\"`, and `Paid` is meant to hold the ones that pass `value != \"free\"`. A union of some of `Plan`'s members stands wherever `Plan` is declared (D30 compares literal sets).".into()),
        ("chain", "type Tier = \"a\" | \"b\" | \"c\"\ntype T1 = Tier\ntype T3 = T1\ntype T2 = T3 where value != \"c\"\n".into(),
         "`T2` refines `T3`, a string-literal union".into(), format!("`T3` is {abc}")),
        ("by-name", "import tiers { Tier }\ntype T2 = Tier where value != \"c\"\n".into(),
         "`T2` refines `Tier`, a string-literal union".into(), format!("`Tier` is {abc}")),
        ("namespaced", "import tiers as t\ntype T2 = t.Tier where value != \"c\"\n".into(),
         "`T2` refines `t.Tier`, a string-literal union".into(), format!("`t.Tier` is {abc}")),
        ("imported-alias", "import tiers\ntype L = tiers.Tier\ntype T2 = L where value != \"c\"\n".into(),
         "`T2` refines `L`, a string-literal union".into(), format!("`L` is {abc}")),
        ("sibling-hop", "import hop { Tier2 }\ntype T2 = Tier2 where value != \"c\"\n".into(),
         "`T2` refines `Tier2`, a string-literal union".into(), format!("`Tier2` is {abc}")),
        ("three-modules", "import x2 as w\ntype T2 = w.Mid where value != \"p\"\n".into(),
         "`T2` refines `w.Mid`, a string-literal union".into(), "`w.Mid` is `\"p\" | \"q\"`, and `T2` is meant to hold the ones that pass `value != \"p\"`".into()),
    ];
    for (dir, decls, start, holds) in cases {
        let root = unique_tmp(&format!("g377-{dir}"));
        let src = root.join("src");
        write_file(&src, "tiers.glyph", tiers);
        write_file(&src, "hop.glyph", hop);
        write_file(&src, "x1.glyph", x1);
        write_file(&src, "x2.glyph", x2);
        let main = format!("module main\n{decls}pub fn main() -> void {{ return void }}\n");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused under --no-tsc:\n{main}\n{stderr}");
        assert!(stderr.contains("E0300") && stderr.contains(&start), "{dir}: the headline `{start}`:\n{stderr}");
        assert!(!stderr.contains("not implemented"), "{dir}: a refusal, not a missing feature:\n{stderr}");
        assert!(stderr.contains(&holds), "{dir}: the help holds `{holds}`:\n{stderr}");
        assert!(!stderr.contains("__glyph_eq"), "{dir}: the predicate as written:\n{stderr}");
        let (code, _stdout, stderr, _) = spawn_glyph(&[std::ffi::OsStr::new("check"), src.as_os_str()]);
        assert_eq!(code, 1, "{dir}: refused under full check:\n{main}\n{stderr}");
        assert!(stderr.contains("E0300"), "{dir}: E0300 under full check:\n{stderr}");
    }
}

/// Record 120, probes `d` and `g`: a `where` over a record or a tagged
/// union that a sibling re-exports under a second name of its own is E0300.
/// 0.1.149 followed the chain one hop into the sibling and stopped, so both
/// checked clean; the G377 walk follows it to the declaration.
#[test]
fn a_where_over_a_record_or_tagged_union_two_modules_away_is_e0300() {
    for (dir, files, decls, start) in [
        (
            "record",
            [("r1.glyph", "module r1\npub type Rec = { x: int, }\n"), ("r2.glyph", "module r2\nimport r1 { Rec }\npub type Rec2 = Rec\n")],
            "import r2 { Rec2 }\npub type T = Rec2 where value.x > 0\n",
            "`T` refines `Rec2`, a record",
        ),
        (
            "tagged-union",
            [("u1.glyph", "module u1\npub type Sh = | A | B\n"), ("u2.glyph", "module u2\nimport u1 { Sh }\npub type Sh2 = Sh\n")],
            "import u2 { Sh2 }\npub type T = Sh2 where true\n",
            "`T` refines `Sh2`, a tagged union",
        ),
    ] {
        let root = unique_tmp(&format!("r120-{dir}"));
        let src = root.join("src");
        for (name, text) in files {
            write_file(&src, name, text);
        }
        let main = format!("module main\n{decls}pub fn main() -> void {{ return void }}\n");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        assert!(stderr.contains("E0300") && stderr.contains(start), "{dir}: `{start}`:\n{stderr}");
    }
}

/// The controls for the test above: a refined `string` whose predicate
/// admits finitely many strings, and the subset written as its own union,
/// both check clean, and the subset stands where the full union is declared.
#[test]
fn a_where_over_a_primitive_and_a_subset_union_still_check_clean() {
    let root = unique_tmp("g377-controls");
    let src = root.join("src");
    let main = "module main\nimport std/console\n\
                type Tier = \"a\" | \"b\" | \"c\"\n\
                type T2 = \"a\" | \"b\"\n\
                type AB = string where value == \"a\" || value == \"b\"\n\
                type Small = int where value < 10\n\
                fn widen(t: T2) -> Tier { return t }\n\
                fn label(t: T2) -> string { return match t { \"a\" => \"A\", \"b\" => \"B\", } }\n\
                pub fn main() -> void {\n  console.log(label(\"b\") + widen(\"a\"))\n}\n";
    write_file(&src, "main.glyph", main);
    let (code, stderr) = check_no_tsc(&src);
    assert_eq!(code, 0, "controls check clean:\n{stderr}");
    assert_eq!(run_main_lines(&src), vec!["Ba"]);
}

/// Record 119: a numeric `Record` key is accepted exactly when it is a
/// string `m[n] = v` can create for some `n` of the key type. `NaN` and
/// `±Infinity` are `number` keys a program can write, `1e+21` is the
/// canonical spelling of an integer D31 accepts as an `int`, and `1e21`,
/// `-0`, `01` and `9007199254740993` are no number's spelling. This pins
/// the whole table and the round trip of a map Glyph wrote, so the rule
/// cannot drift without this test saying so.
#[test]
fn a_numeric_record_key_is_accepted_exactly_when_it_is_a_canonical_spelling() {
    let root = unique_tmp("canonical-numeric-keys");
    let src = root.join("src");
    let main = "module main\nimport std/console\nimport std/json\n\
                type Wrap = { x: int, }\n\
                fn num(text: string) -> string { return match json.parse<Record<number, int>>(text) { Ok(r) => \"Ok\", Err(e) => \"Err\", } }\n\
                fn int_key(text: string) -> string { return match json.parse<Record<int, int>>(text) { Ok(r) => \"Ok\", Err(e) => \"Err\", } }\n\
                pub fn main() -> void {\n\
                \x20 let ks = [\"NaN\", \"Infinity\", \"-Infinity\", \"1e+21\", \"1e21\", \"-0\", \"0\", \"9007199254740993\", \"1.5\", \"01\",]\n\
                \x20 for k in ks {\n\
                \x20\x20\x20 let q = \"\\\"\"\n\
                \x20\x20\x20 let text = \"{\" + q + k + q + \":1}\"\n\
                \x20\x20\x20 console.log(k + \" \" + num(text) + \" \" + int_key(text))\n\
                \x20 }\n\
                \x20 let w = match json.parse<Wrap>(\"{\\\"x\\\": 1e21}\") { Ok(v) => \"Ok\", Err(e) => \"Err\", }\n\
                \x20 console.log(\"int-value-1e21 \" + w)\n\
                \x20 let m: Record<number, int> = {}\n\
                \x20 mut m[0 / 0] = 1\n\
                \x20 mut m[1 / 0] = 2\n\
                \x20 let back = json.stringify(m)\n\
                \x20 console.log(back + \" \" + num(back))\n\
                }\n";
    write_file(&src, "main.glyph", main);
    assert_eq!(
        run_main_lines(&src),
        vec![
            "NaN Ok Err",
            "Infinity Ok Err",
            "-Infinity Ok Err",
            "1e+21 Ok Ok",
            "1e21 Err Err",
            "-0 Err Err",
            "0 Ok Ok",
            "9007199254740993 Err Err",
            "1.5 Ok Err",
            "01 Err Err",
            "int-value-1e21 Ok",
            "{\"NaN\":1,\"Infinity\":2} Ok",
        ],
        "the canonical-key table:\n{main}"
    );
}

/// G367: a check of a `Record` whose key type has no runtime check is
/// E0304, naming the key type, at the top, as a record's field, inside an
/// `Array`, as a generic's argument and in an `is` pattern. Through 0.1.147
/// the key type was never read, so each of these exited 0.
#[test]
fn a_record_whose_key_type_has_no_runtime_check_is_e0304() {
    let h = "type H = extern_ts(\"string\")\n";
    let jp = |ty: &str| format!("fn p(s: string) -> bool {{ return match json.parse<{ty}>(s) {{ Ok(_) => true, Err(_) => false, }} }}\n");
    let cases: Vec<(&str, String, &str)> = vec![
        ("json-parse", jp("Record<H, int>"), "cannot validate `Record<H, int>`: its key type `H` has no runtime check"),
        ("field", format!("type Doc = {{ m: Record<H, int>, }}\n{}", jp("Doc")),
         "cannot validate `Doc`: field `m` has type `Record<H, int>`, whose key type `H` has no runtime check"),
        ("array", jp("Array<Record<H, int>>"),
         "cannot validate `Array<Record<H, int>>`: its part `Record<H, int>` has key type `H`, which has no runtime check"),
        ("type-argument",
         "type Box<T> = { v: T, }\nfn p(s: unknown) -> bool { return match Box.parse<Record<H, int>>(s) { Ok(_) => true, Err(_) => false, } }\n".into(),
         "cannot validate `Box<Record<H, int>>`: its type argument `Record<H, int>` has key type `H`, which has no runtime check"),
        ("is-pattern", "fn p(s: unknown) -> bool { return match s { is Record<H, int> => true, _ => false, } }\n".into(),
         "cannot validate `Record<H, int>`: its key type `H` has no runtime check"),
    ];
    for (dir, body, expected) in cases {
        let root = unique_tmp(&format!("g367-nokey-{dir}"));
        let src = root.join("src");
        let main = format!("module main\nimport std/json\n{h}{body}pub fn main() -> void {{ return void }}\n");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        let headline = e0304_headline(&stderr).unwrap_or_else(|| panic!("{dir}: E0304:\n{stderr}"));
        assert!(headline.contains(expected), "{dir}: `{expected}` in:\n{headline}");
    }
}

/// G367: a generic record with a `Record` field keyed by one of its type
/// parameters checks that key with the checker passed for the parameter,
/// which tests one key and cannot list a literal union's members, so the
/// totality the written-out `Record<Tier, int>` gets cannot hold. Every
/// check through such a generic with an argument other than `string` is
/// E0304: `json.parse`, its descriptor, an `is` pattern, a field typed by
/// the application, a generic applying it with its own parameter, and an
/// alias of another module applying it. With `string` it is checked and
/// runs. Through 0.1.147 `json.parse<Idx<Tier>>` returned `Ok` for
/// `{"m":{"zzz":1}}` (record 113, probe v3). Lifting this is G374.
#[test]
fn a_record_keyed_by_a_generic_type_parameter_is_e0304_unless_the_argument_is_string() {
    let w = "module w\npub type Tier = \"a\" | \"b\"\npub type Idx<K: string> = { m: Record<K, int>, }\n\
             pub type Page<T: string> = { idx: Idx<T>, }\npub type I = Idx<Tier>\npub type S = Idx<string>\n";
    let header = "module main\nimport std/console\nimport std/json\nimport w { Tier, Idx, Page, I, S }\n";
    let jp = |ty: &str| format!("fn p(s: string) -> bool {{ return match json.parse<{ty}>(s) {{ Ok(_) => true, Err(_) => false, }} }}\n");
    let key = "and a key type given by a type parameter is not checked at run time";
    let cases: Vec<(&str, String, &str)> = vec![
        ("json-parse", jp("Idx<Tier>"), "cannot validate `Idx<Tier>`: `Idx`'s field `m` has type `Record<K, int>`"),
        ("descriptor", "fn p(s: unknown) -> bool { return match Idx.parse<Tier>(s) { Ok(_) => true, Err(_) => false, } }\n".into(),
         "cannot validate `Idx<Tier>`: `Idx`'s field `m` has type `Record<K, int>`"),
        ("is-pattern", "fn p(s: unknown) -> bool { return match s { is Idx<Tier> => true, _ => false, } }\n".into(),
         "cannot validate `Idx<Tier>`: `Idx`'s field `m` has type `Record<K, int>`"),
        ("field", format!("type Doc = {{ i: Idx<Tier>, }}\n{}", jp("Doc")),
         "cannot validate `Doc`: field `i` has type `Idx<Tier>`, and `Idx`'s field `m` has type `Record<K, int>`"),
        ("through-a-generic", jp("Page<Tier>"), "cannot validate `Page<Tier>`: `Idx`'s field `m` has type `Record<K, int>`"),
        ("sibling-alias", jp("I"), "cannot validate `I`: `I`'s body applies `Idx`, whose field `m` has type `Record<K, int>`"),
    ];
    for (dir, body, expected) in cases {
        let root = unique_tmp(&format!("g367-keyparam-{dir}"));
        let src = root.join("src");
        write_file(&src, "w.glyph", w);
        let main = format!("{header}{body}pub fn main() -> void {{ return void }}\n");
        write_file(&src, "main.glyph", &main);
        let (code, stderr) = check_no_tsc(&src);
        assert_eq!(code, 1, "{dir}: refused:\n{main}\n{stderr}");
        let headline = e0304_headline(&stderr).unwrap_or_else(|| panic!("{dir}: E0304:\n{stderr}"));
        assert!(headline.contains(expected) && headline.contains(key), "{dir}: `{expected}` in:\n{headline}");
    }

    // With `string` for the key every spelling is checked, and runs.
    let root = unique_tmp("g367-keyparam-string");
    let src = root.join("src");
    write_file(&src, "w.glyph", w);
    let ok = "{ Ok(_) => \"Ok\", Err(_) => \"Err\", }";
    write_file(
        &src,
        "main.glyph",
        &format!(
            "{header}fn u(t: string) -> unknown {{ return match json.parse(t) {{ Ok(x) => x, Err(e) => \"PARSEERR\", }} }}\n\
             fn a(t: string) -> string {{ return match json.parse<Idx<string>>(t) {ok} }}\n\
             fn b(t: string) -> string {{ return match Idx.parse<string>(u(t)) {ok} }}\n\
             fn c(t: string) -> string {{ return match json.parse<S>(t) {ok} }}\n\
             fn d(t: string) -> string {{ return match json.parse<Page<string>>(\"{{\\\"idx\\\":\" + t + \"}}\") {ok} }}\n\
             pub fn main() -> void {{\n\
             \x20 let good = \"{{\\\"m\\\":{{\\\"zzz\\\":1}}}}\"\n\
             \x20 let bad = \"{{\\\"m\\\":{{\\\"zzz\\\":\\\"x\\\"}}}}\"\n\
             \x20 console.log(a(good) + b(good) + c(good) + d(good))\n\
             \x20 console.log(a(bad) + b(bad) + c(bad) + d(bad))\n\
             }}\n"
        ),
    );
    assert_eq!(run_main_lines(&src), vec!["OkOkOkOk", "ErrErrErrErr"]);
}

// G391 (record 119). An optional field is an absent own key or a `T`. A key
// holding JSON `null`, or a host `undefined`, is a `type` issue unless `T` is
// `Nullable`. Until 0.1.152 the presence test read `null` as absence, so every
// route below returned `Ok` holding a `null` where the type said `number`.
// Each route runs the four inputs in one order: a key holding `null`, a key
// holding `undefined`, the key absent, the key holding a number.
const OPTIONAL_PARITY_SIDE: &str = "module side\npub type SOpts = { timeout_ms?: number, }\n";
const OPTIONAL_PARITY_MAIN: &str = r#"module main

import std/io
import std/json
import std/json { parse }
import side { SOpts }
import side

type Opts = { timeout_ms?: number, }
type Wrap = { o: Opts, }
type Many = { xs: Array<Opts>, }
type M = { m: Record<string, Opts>, }
type Shape = On(Opts) | Off
type Box<T> = { v: T, }
type Deep = { o?: Opts, }
type OW = { x: Option<Opts>, }

fn ok<T>(r: Result<T, Array<Issue>>) -> string {
  return match r { Ok(_) => "Ok", Err(_) => "Err", }
}

fn yes(b: bool) -> string {
  return match b { true => "Ok", false => "Err", }
}

fn is_opts(v: unknown) -> bool {
  return match v { is Opts => true, else => false, }
}

fn is_box(v: unknown) -> bool {
  return match v { is Box<Opts> => true, else => false, }
}

fn host(js: string) -> unknown {
  return extern_ts("eval(js)")
}

fn raw(text: string) -> unknown {
  return match json.parse(text) { Ok(v) => v, Err(_) => "unparseable", }
}

fn row(name: string, a: string, b: string, c: string, d: string) {
  io.println(name + " " + a + " " + b + " " + c + " " + d)
}

fn main(argv: Array<string>) -> number {
  // The four inputs: a JSON null, a host undefined, the key absent, a number.
  // JSON text cannot carry `undefined`, so a text route's undefined column
  // runs the entry the text route rewrites to on the host value.
  let n = "{\"timeout_ms\":null}"
  let a = "{}"
  let f = "{\"timeout_ms\":5}"
  let u = host("({ timeout_ms: undefined })")
  row(
    "json.parse<Opts>",
    ok(json.parse<Opts>(n)),
    ok(Opts.schema.parse(u)),
    ok(json.parse<Opts>(a)),
    ok(json.parse<Opts>(f)),
  )
  row(
    "parse<Opts>",
    ok(parse<Opts>(n)),
    ok(Opts.schema.parse(u)),
    ok(parse<Opts>(a)),
    ok(parse<Opts>(f)),
  )
  row(
    "Opts.parse",
    ok(Opts.parse(raw(n))),
    ok(Opts.parse(u)),
    ok(Opts.parse(raw(a))),
    ok(Opts.parse(raw(f))),
  )
  row(
    "Opts.is",
    yes(Opts.is(raw(n))),
    yes(Opts.is(u)),
    yes(Opts.is(raw(a))),
    yes(Opts.is(raw(f))),
  )
  row(
    "is Opts",
    yes(is_opts(raw(n))),
    yes(is_opts(u)),
    yes(is_opts(raw(a))),
    yes(is_opts(raw(f))),
  )
  row(
    "parse_with",
    ok(json.parse_with(n, Opts.schema)),
    ok(Opts.schema.parse(u)),
    ok(json.parse_with(a, Opts.schema)),
    ok(json.parse_with(f, Opts.schema)),
  )
  row(
    "Wrap",
    ok(json.parse<Wrap>("{\"o\":" + n + "}")),
    ok(Wrap.parse(host("({ o: { timeout_ms: undefined } })"))),
    ok(json.parse<Wrap>("{\"o\":" + a + "}")),
    ok(json.parse<Wrap>("{\"o\":" + f + "}")),
  )
  row(
    "Many",
    ok(json.parse<Many>("{\"xs\":[" + n + "]}")),
    ok(Many.parse(host("({ xs: [{ timeout_ms: undefined }] })"))),
    ok(json.parse<Many>("{\"xs\":[" + a + "]}")),
    ok(json.parse<Many>("{\"xs\":[" + f + "]}")),
  )
  row(
    "M",
    ok(json.parse<M>("{\"m\":{\"k\":" + n + "}}")),
    ok(M.parse(host("({ m: { k: { timeout_ms: undefined } } })"))),
    ok(json.parse<M>("{\"m\":{\"k\":" + a + "}}")),
    ok(json.parse<M>("{\"m\":{\"k\":" + f + "}}")),
  )
  row(
    "Box<Opts>",
    ok(json.parse<Box<Opts>>("{\"v\":" + n + "}")),
    yes(is_box(host("({ v: { timeout_ms: undefined } })"))),
    ok(json.parse<Box<Opts>>("{\"v\":" + a + "}")),
    ok(json.parse<Box<Opts>>("{\"v\":" + f + "}")),
  )
  row(
    "Deep",
    ok(json.parse<Deep>("{\"o\":null}")),
    ok(Deep.parse(host("({ o: undefined })"))),
    ok(json.parse<Deep>("{}")),
    ok(json.parse<Deep>("{\"o\":" + f + "}")),
  )
  row(
    "Shape",
    ok(json.parse<Shape>("{\"tag\":\"On\",\"value\":" + n + "}")),
    ok(Shape.parse(host("({ tag: \"On\", value: { timeout_ms: undefined } })"))),
    ok(json.parse<Shape>("{\"tag\":\"On\",\"value\":" + a + "}")),
    ok(json.parse<Shape>("{\"tag\":\"On\",\"value\":" + f + "}")),
  )
  row(
    "Option<Opts>",
    ok(json.parse<Option<Opts>>("{\"tag\":\"Some\",\"value\":" + n + "}")),
    ok(OW.parse(host("({ x: { tag: \"Some\", value: { timeout_ms: undefined } } })"))),
    ok(json.parse<Option<Opts>>("{\"tag\":\"Some\",\"value\":" + a + "}")),
    ok(json.parse<Option<Opts>>("{\"tag\":\"Some\",\"value\":" + f + "}")),
  )
  row(
    "sibling by name",
    ok(json.parse<SOpts>(n)),
    ok(SOpts.parse(u)),
    ok(json.parse<SOpts>(a)),
    ok(json.parse<SOpts>(f)),
  )
  row(
    "sibling by namespace",
    ok(json.parse<side.SOpts>(n)),
    ok(side.SOpts.parse(u)),
    ok(json.parse<side.SOpts>(a)),
    ok(json.parse<side.SOpts>(f)),
  )
  return 0
}
"#;

const OPTIONAL_ROUTES_MAIN: &str = r#"module main

import std/io
import std/json

type Opts = { t?: number, }
type U = On({ t?: number }) | Off
type In = { o: { t?: number }, }
type G<T> = { t?: T, }
@open
type Op = { t?: number, }
type A = Opts
type NS = { t?: Nullable<int>, }
type O2 = { t?: Option<number>, }
type R = { t: Option<number>, }

fn ok<T>(r: Result<T, Array<Issue>>) -> string {
  return match r { Ok(_) => "Ok", Err(_) => "Err", }
}

fn yes(b: bool) -> string {
  return match b { true => "Ok", false => "Err", }
}

fn is_g(v: unknown) -> bool {
  return match v { is G<number> => true, else => false, }
}

fn issues<T>(r: Result<T, Array<Issue>>) -> string {
  return match r { Ok(_) => "Ok", Err(e) => json.stringify(e), }
}

fn host(js: string) -> unknown {
  return extern_ts("eval(js)")
}

fn row(name: string, a: string, b: string, c: string, d: string) {
  io.println(name + " " + a + " " + b + " " + c + " " + d)
}

fn main(argv: Array<string>) -> number {
  let n = "{\"t\":null}"
  let a = "{}"
  let f = "{\"t\":5}"
  let u = host("({ t: undefined })")
  row(
    "union inline payload",
    ok(json.parse<U>("{\"tag\":\"On\",\"t\":null}")),
    ok(U.parse(host("({ tag: \"On\", t: undefined })"))),
    ok(json.parse<U>("{\"tag\":\"On\"}")),
    ok(json.parse<U>("{\"tag\":\"On\",\"t\":5}")),
  )
  row(
    "inline record field",
    ok(json.parse<In>("{\"o\":" + n + "}")),
    ok(In.parse(host("({ o: { t: undefined } })"))),
    ok(json.parse<In>("{\"o\":" + a + "}")),
    ok(json.parse<In>("{\"o\":" + f + "}")),
  )
  row(
    "generic record",
    ok(json.parse<G<number>>(n)),
    yes(is_g(u)),
    ok(json.parse<G<number>>(a)),
    ok(json.parse<G<number>>(f)),
  )
  row("open record", ok(json.parse<Op>(n)), ok(Op.parse(u)), ok(json.parse<Op>(a)), ok(json.parse<Op>(f)))
  row("alias", ok(json.parse<A>(n)), ok(Opts.parse(u)), ok(json.parse<A>(a)), ok(json.parse<A>(f)))
  row("nullable", ok(json.parse<NS>(n)), ok(NS.parse(u)), ok(json.parse<NS>(a)), ok(json.parse<NS>("{\"t\":3}")))
  match json.parse<NS>(n) {
    Ok(_) => io.println("nullable null Ok"),
    Err(e) => io.println("nullable null " + json.stringify(e)),
  }
  match json.parse<O2>(n) {
    Ok(o) => {
      let r: R = { t: Some(1), ...o }
      let s = match r.t { Some(x) => "some ${x}", None => "none", }
      io.println("n5 " + s)
    },
    Err(e) => io.println("n5 refused " + json.stringify(e)),
  }
  io.println("null " + issues(Opts.parse(host("({ t: null })"))))
  io.println("undefined " + issues(Opts.parse(u)))
  io.println("wrong " + issues(json.parse<Opts>("{\"t\":\"x\"}")))
  return 0
}
"#;

/// One verdict per input on every route a `?` field is checked by: the two
/// `json.parse` spellings, `T.parse`, `T.is`, `is T`, `parse_with`, a field,
/// an array element, a `Record` value, a generic argument, an optional field
/// of the record's own type, a union payload, an `Option` payload, and a
/// sibling's record by name and by namespace. `is` and `parse` agree because
/// they share one presence test.
#[test]
fn an_optional_field_has_one_verdict_per_input_on_every_route() {
    let root = unique_tmp("g391-parity");
    let src = root.join("src");
    write_file(&src, "side.glyph", OPTIONAL_PARITY_SIDE);
    write_file(&src, "main.glyph", OPTIONAL_PARITY_MAIN);
    let routes = [
        "json.parse<Opts>",
        "parse<Opts>",
        "Opts.parse",
        "Opts.is",
        "is Opts",
        "parse_with",
        "Wrap",
        "Many",
        "M",
        "Box<Opts>",
        "Deep",
        "Shape",
        "Option<Opts>",
        "sibling by name",
        "sibling by namespace",
    ];
    let expected: Vec<String> = routes.iter().map(|r| format!("{r} Err Err Ok Ok")).collect();
    assert_eq!(run_main_lines(&src), expected);
}

/// The declaration forms a `?` field can sit in, the `Nullable` spelling
/// that takes null, the `Option` field that crashed a later `match` (b/n5),
/// and the three issue messages.
#[test]
fn an_optional_field_refuses_null_in_every_declaration_form() {
    let root = unique_tmp("g391-routes");
    let src = root.join("src");
    write_file(&src, "main.glyph", OPTIONAL_ROUTES_MAIN);
    assert_eq!(
        run_main_lines(&src),
        vec![
            "union inline payload Err Err Ok Ok",
            "inline record field Err Err Ok Ok",
            "generic record Err Err Ok Ok",
            "open record Err Err Ok Ok",
            "alias Err Err Ok Ok",
            // `t?: Nullable<int>`: null, absent and 3 are each `Ok`, and a
            // key holding `undefined` is refused.
            "nullable Ok Err Ok Ok",
            "nullable null Ok",
            // b/n5: refused at the decode, so the `match` never reads `.tag`
            // off a null.
            "n5 refused [{\"path\":[\"t\"],\"message\":\"field `t` is null; declare it `t?: Nullable<number>` to accept null\",\"code\":\"type\"}]",
            "null [{\"path\":[\"t\"],\"message\":\"field `t` is null; declare it `t?: Nullable<number>` to accept null\",\"code\":\"type\"}]",
            "undefined [{\"path\":[\"t\"],\"message\":\"field `t` is undefined; omit the key\",\"code\":\"type\"}]",
            "wrong [{\"path\":[\"t\"],\"message\":\"field `t` must be number\",\"code\":\"type\"}]",
        ]
    );
}

// Record 124. The null issue's suggestion is pasted by whoever reads it, so
// it must compile. An `Option` reached through an alias, under any import
// spelling, is suggested as `Nullable` of its argument, since `Nullable` of
// an `Option` under any name is E0227; where that argument is itself an
// `Option` there is no spelling, and where the chain names a sibling's own
// type the suggestion is left out. (A generic alias of `Option` is E0304 at
// `parse`, so no run reaches its hint.)
const NULL_HINTS_SIDE: &str = r#"module side
pub type SMaybe = Option<int>
pub type SRec = { a: int, }
pub type SMaybeRec = Option<SRec>

"#;

const NULL_HINTS_MAIN: &str = r#"module main

import std/io
import std/json
import side { SMaybe, SMaybeRec }
import side

type MaybeInt = Option<int>
type Maybe<T> = Option<T>
type MaybeMaybe = Option<MaybeInt>
type Inner = { a: int, }
type Tier = "a" | "b"
type L = side.SMaybe
type A1 = { t?: Option<int>, }
type A2 = { t?: MaybeInt, }
type A4 = { t?: Option<Option<int>>, }
type A5 = { t?: MaybeMaybe, }
type A6 = { t?: Inner, }
type A7 = { t?: Array<Tier>, }
type A8 = { t?: Record<string, int>, }
type A9 = { t?: { a: int, b?: string }, }
type A10 = { t?: SMaybe, }
type A11 = { t?: side.SMaybe, }
type A12 = { t?: SMaybeRec, }
type A13 = { t?: int, }
type A15 = { t?: L, }

fn hint<T>(name: string, r: Result<T, Array<Issue>>) {
  let m = match r {
    Ok(_) => "Ok",
    Err(e) => e[0].message,
  }
  io.println(name + " " + m)
}

fn main(argv: Array<string>) -> number {
  let n = "{\"t\":null}"
  hint("A1", json.parse<A1>(n))
  hint("A2", json.parse<A2>(n))
  hint("A4", json.parse<A4>(n))
  hint("A5", json.parse<A5>(n))
  hint("A6", json.parse<A6>(n))
  hint("A7", json.parse<A7>(n))
  hint("A8", json.parse<A8>(n))
  hint("A9", json.parse<A9>(n))
  hint("A10", json.parse<A10>(n))
  hint("A11", json.parse<A11>(n))
  hint("A12", json.parse<A12>(n))
  hint("A13", json.parse<A13>(n))
  hint("A15", json.parse<A15>(n))
  return 0
}

"#;

/// The types `field `t` is null; declare it `t?: <T>` ...` lines suggest.
fn suggested_null_types(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .flat_map(|l| l.split("declare it `").skip(1))
        .filter_map(|rest| rest.split('`').next())
        .map(|decl| decl.split_once(": ").expect("a field declaration").1.to_string())
        .collect()
}

/// Every suggestion compiles where it was printed: `type ProbeN = { t?: T, }`
/// appended to the module that printed it checks clean under full `check`.
fn assert_suggestions_compile(label: &str, files: &[(&str, &str)], suggestions: &[String]) {
    assert!(!suggestions.is_empty(), "{label}: no suggestion to check");
    let root = unique_tmp(&format!("g391-probe-{label}"));
    let src = root.join("src");
    for (name, text) in files {
        let mut text = text.to_string();
        if *name == "main.glyph" {
            for (i, ty) in suggestions.iter().enumerate() {
                text.push_str(&format!("\ntype Probe{i} = {{ t?: {ty}, }}\n"));
            }
        }
        write_file(&src, name, &text);
    }
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        std::ffi::OsStr::new("--no-test"),
        main.as_os_str(),
    ]);
    assert_eq!(code, 0, "{label}: a suggested type does not compile ({suggestions:?}):\n{stdout}\n{stderr}");
}

#[test]
fn every_null_suggestion_names_a_type_that_compiles() {
    let root = unique_tmp("g391-hints");
    let src = root.join("src");
    write_file(&src, "side.glyph", NULL_HINTS_SIDE);
    write_file(&src, "main.glyph", NULL_HINTS_MAIN);
    let lines = run_main_lines(&src);
    let spell = |t: &str| format!("field `t` is null; declare it `t?: {t}` to accept null");
    let none = "field `t` is null, and its type has no spelling that accepts null".to_string();
    let omit = "field `t` is null, and its type does not accept null".to_string();
    assert_eq!(
        lines,
        vec![
            format!("A1 {}", spell("Nullable<int>")),
            format!("A2 {}", spell("Nullable<int>")),
            format!("A4 {none}"),
            format!("A5 {none}"),
            format!("A6 {}", spell("Nullable<Inner>")),
            format!("A7 {}", spell("Nullable<Array<Tier>>")),
            format!("A8 {}", spell("Nullable<Record<string, int>>")),
            format!("A9 {}", spell("Nullable<{ a: int, b?: string }>")),
            format!("A10 {}", spell("Nullable<int>")),
            format!("A11 {}", spell("Nullable<int>")),
            format!("A12 {omit}"),
            format!("A13 {}", spell("Nullable<int>")),
            format!("A15 {}", spell("Nullable<int>")),
        ]
    );
    assert_suggestions_compile(
        "hints",
        &[("side.glyph", NULL_HINTS_SIDE), ("main.glyph", NULL_HINTS_MAIN)],
        &suggested_null_types(&lines),
    );

    // The hints the declaration-forms test prints compile too.
    let root = unique_tmp("g391-hints-routes");
    let src = root.join("src");
    write_file(&src, "main.glyph", OPTIONAL_ROUTES_MAIN);
    let lines = run_main_lines(&src);
    assert_suggestions_compile("routes", &[("main.glyph", OPTIONAL_ROUTES_MAIN)], &suggested_null_types(&lines));
}

/// G373. A `Record`'s key is invariant whichever spelling reaches the key: a
/// sibling's union imported by name, through a namespace, through a local
/// alias of an alias, and through a sibling's second name for a third
/// module's union, and a sibling's refined type the same. Each refusal is the
/// same E0204 with the same note, and the same-domain pairings across those
/// spellings are accepted.
#[test]
fn a_record_key_is_invariant_by_every_spelling() {
    let root = unique_tmp("g373spell");
    let src = root.join("src");
    write_file(
        &src,
        "tiers.glyph",
        "module tiers\n\npub type Tier = \"a\" | \"b\"\n\npub type Slug = string where value.length > 1\n\npub type SlugAgain = Slug\n",
    );
    write_file(&src, "mid.glyph", "module mid\n\nimport tiers as q\n\npub type MidTier = q.Tier\n");
    write_file(
        &src,
        "main.glyph",
        "module main\n\nimport tiers { Tier }\nimport tiers as t\nimport mid { MidTier }\n\n\
         type Local = Tier\n\ntype LocalLocal = Local\n\n\
         pub fn by_name(r: Record<string, int>) -> Record<Tier, int> {\n  return r\n}\n\n\
         pub fn by_ns(r: Record<string, int>) -> Record<t.Tier, int> {\n  return r\n}\n\n\
         pub fn by_alias(r: Record<string, int>) -> Record<LocalLocal, int> {\n  return r\n}\n\n\
         pub fn by_hop(r: Record<string, int>) -> Record<MidTier, int> {\n  return r\n}\n\n\
         pub fn back(r: Record<MidTier, int>) -> Record<string, int> {\n  return r\n}\n\n\
         pub fn slug(r: Record<string, int>) -> Record<t.Slug, int> {\n  return r\n}\n\n\
         pub fn ok_ns(r: Record<Tier, int>) -> Record<t.Tier, int> {\n  return r\n}\n\n\
         pub fn ok_hop(r: Record<LocalLocal, int>) -> Record<MidTier, int> {\n  return r\n}\n\n\
         pub fn ok_slug(r: Record<t.Slug, int>) -> Record<t.SlugAgain, int> {\n  return r\n}\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
    let d = &report.diagnostics;
    let refused: Vec<&String> = d.iter().filter(|d| d.contains("[E0204]")).collect();
    assert_eq!(refused.len(), 6, "six refusals, nothing else: {d:?}");
    for key in ["`Tier`", "`t.Tier`", "`LocalLocal`", "`MidTier`", "`t.Slug`"] {
        assert!(
            refused.iter().any(|r| r.contains(&format!("may hold keys outside {key}"))),
            "{key}: {d:?}"
        );
    }
    assert_eq!(d.len(), 6, "the same-domain pairings are accepted: {d:?}");
}

/// G373, the `std/record` signatures through the emitted TypeScript. A
/// literal-union map keeps its key through `set`, `keys` is `Array<Tier>`,
/// and `tsc --strict` agrees with Glyph on every line.
#[test]
fn std_record_keeps_a_literal_union_key_under_tsc() {
    let root = unique_tmp("g373std");
    let src = root.join("src");
    write_file(
        &src,
        "main.glyph",
        "module main\n\nimport std/record\n\ntype Tier = \"a\" | \"b\"\n\n\
         pub fn bump(r: Record<Tier, int>) -> Record<Tier, int> {\n  return record.set(r, \"a\", 9)\n}\n\n\
         pub fn ks(r: Record<Tier, int>) -> Array<Tier> {\n  return record.keys(r)\n}\n\n\
         pub fn nks(r: Record<int, int>) -> Array<string> {\n  return record.keys(r)\n}\n\n\
         pub fn one(r: Record<Tier, int>) -> Option<int> {\n  return record.get(r, \"b\")\n}\n\n\
         pub fn any(r: Record<Tier, int>, k: string) -> bool {\n  return record.has(r, k)\n}\n\n\
         pub fn drop(r: Record<string, int>) -> Record<string, int> {\n  return record.remove(r, \"z\")\n}\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    use glyph_cli::runtime::{check_with_tsc, TscOutcome};
    match check_with_tsc(&root.join("dist")).expect("run tsc") {
        TscOutcome::Passed => {}
        TscOutcome::Failed(out) => panic!("tsc --strict refused the emitted std/record calls:\n{out}"),
        TscOutcome::NotFound => tool_missing("tsc"),
    }
}

/// G373. `record.remove` on a `Record` whose key has a fixed member set is
/// E0211 by every spelling of the key: local, imported by name, namespaced, a
/// single literal, and through an alias of the map. A refined key and an open
/// key are not refused.
#[test]
fn record_remove_on_a_finite_key_is_e0211_by_every_spelling() {
    let root = unique_tmp("g373remove");
    let src = root.join("src");
    write_file(&src, "tiers.glyph", "module tiers\n\npub type Tier = \"a\" | \"b\"\n");
    write_file(
        &src,
        "main.glyph",
        "module main\n\nimport std/record\nimport tiers { Tier }\nimport tiers as t\n\n\
         type Local = \"x\" | \"y\"\n\ntype One = \"only\"\n\ntype Scores = Record<Tier, int>\n\n\
         type Slug = string where value.length > 1\n\n\
         pub fn local(r: Record<Local, int>) -> Record<Local, int> {\n  return record.remove(r, \"x\")\n}\n\n\
         pub fn named(r: Record<Tier, int>) -> Record<Tier, int> {\n  return record.remove(r, \"a\")\n}\n\n\
         pub fn spaced(r: Record<t.Tier, int>) -> Record<t.Tier, int> {\n  return record.remove(r, \"a\")\n}\n\n\
         pub fn one(r: Record<One, int>) -> Record<One, int> {\n  return record.remove(r, \"only\")\n}\n\n\
         pub fn alias(r: Scores) -> Scores {\n  return record.remove(r, \"b\")\n}\n\n\
         pub fn refined(r: Record<Slug, int>, k: Slug) -> Record<Slug, int> {\n  return record.remove(r, k)\n}\n\n\
         pub fn open(r: Record<string, int>) -> Record<string, int> {\n  return record.remove(r, \"z\")\n}\n",
    );
    let report = build_project_inner(&src, &root.join("dist"), false).expect("build runs");
    let d = &report.diagnostics;
    let refused: Vec<&String> = d.iter().filter(|d| d.contains("[E0211]") && d.contains("record.remove takes a Record")).collect();
    assert_eq!(refused.len(), 5, "five refusals: {d:?}");
    for subject in [
        "`Record<Local, int>` holds every `Local`",
        "`Record<Tier, int>` holds every `Tier`",
        "`Record<t.Tier, int>` holds every `t.Tier`",
        "`Record<One, int>` holds every `One`",
        "`Scores` holds every `Tier`",
    ] {
        assert!(refused.iter().any(|r| r.contains(subject)), "{subject}: {d:?}");
    }
    assert_eq!(d.len(), 5, "the refined and open keys are not refused: {d:?}");
}

/// G385 (record 129, D54): one function per refused conversion. Each marked
/// line holds exactly one conversion of a value with no text form.
const NO_TEXT_FORM_MAIN: &str = r#"module main

import std/string
import std/decimal

type User = { id: number, name: string, }
type Shape = Circle(number) | Square(number)

fn greet() -> string {
  return "hi"
}

fn rec(user: User) -> string {
  return "user: ${user}"
}

fn rec_plus(user: User) -> string {
  return "user: " + user
}

fn rec_from(user: User) -> string {
  return string.from(user)
}

fn rec_raw(user: User) -> string {
  return """user: ${user}"""
}

fn opt(o: Option<number>) -> string {
  return "opt: ${o}"
}

fn opt_plus(o: Option<string>) -> string {
  return "opt: " + o
}

fn res(r: Result<number, string>) -> string {
  return "res: ${r}"
}

fn shape(s: Shape) -> string {
  return "shape: ${s}"
}

fn names(names: Array<string>) -> string {
  return "names: ${names}"
}

fn nums(xs: Array<number>) -> string {
  return "nums: ${xs}"
}

fn dec(d: decimal.Decimal) -> string {
  return "dec: ${d}"
}

fn nul(n: Nullable<string>) -> string {
  return "nul: ${n}"
}

fn nul_num(n: Nullable<number>) -> string {
  return "count: ${n}"
}

fn func() -> string {
  return "fn: ${greet}"
}

fn unk(v: unknown) -> string {
  return "unk: ${v}"
}

fn gen<T>(x: T) -> string {
  return "gen: ${x}"
}

fn unk_plus(v: unknown) -> string {
  return "a" + v
}

fn unk_lambda() -> fn(unknown) -> string {
  return fn(p: unknown) -> string { return "${p}" }
}
"#;

/// The E0237 each marked line draws, keyed by the line's text: the message,
/// then the help, exactly as the checker computes them.
fn no_text_form_expected() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "return \"user: ${user}\"",
            "`${user}` turns a `User` into text, and a record has no text form: it prints `[object Object]`",
            "Interpolate a field, such as `${user.id}`.",
        ),
        (
            "return \"user: \" + user",
            "`+` joins a `User` to a string, and a record has no text form: it prints `[object Object]`",
            "Join a field, such as `user.id`.",
        ),
        (
            "return string.from(user)",
            "`string.from(user)` turns a `User` into text, and a record has no text form: it prints `[object Object]`",
            "Convert a field, such as `string.from(user.id)`.",
        ),
        (
            "return \"\"\"user: ${user}\"\"\"",
            "`${user}` turns a `User` into text, and a record has no text form: it prints `[object Object]`",
            "Interpolate a field, such as `${user.id}`.",
        ),
        (
            "return \"opt: ${o}\"",
            "`${o}` turns an `Option<number>` into text, and an `Option` has no text form: it prints `[object Object]`",
            "Give each case its text with a `match` first, `let o_text = match o { Some(v) => \"${v}\", None => \"none\", }`, then interpolate `${o_text}`.",
        ),
        (
            "return \"opt: \" + o",
            "`+` joins an `Option<string>` to a string, and an `Option` has no text form: it prints `[object Object]`",
            "Give each case its text with a `match` first, `let o_text = match o { Some(v) => \"${v}\", None => \"none\", }`, then join `o_text`.",
        ),
        (
            "return \"res: ${r}\"",
            "`${r}` turns a `Result<number, string>` into text, and a `Result` has no text form: it prints `[object Object]`",
            "Give each case its text with a `match` first, `let r_text = match r { Ok(v) => \"${v}\", Err(e) => e, }`, then interpolate `${r_text}`.",
        ),
        (
            "return \"shape: ${s}\"",
            "`${s}` turns a `Shape` into text, and a tagged union has no text form: it prints `[object Object]`",
            "`match` on it with an arm for each variant (`Circle`, `Square`), and give each arm its text.",
        ),
        (
            "return \"names: ${names}\"",
            "`${names}` turns an `Array<string>` into text, and an array has no text form: it prints its elements joined by commas, with no spaces",
            "Join the elements first, `let names_text = string.join(names, \", \")`, then interpolate `${names_text}`.",
        ),
        (
            "return \"nums: ${xs}\"",
            "`${xs}` turns an `Array<number>` into text, and an array has no text form: it prints its elements joined by commas, with no spaces",
            "Join the elements first, `let xs_text = string.join(array.map(xs, fn(x: number) -> string { return \"${x}\" }), \", \")` (with `import std/array`), then interpolate `${xs_text}`.",
        ),
        (
            "return \"dec: ${d}\"",
            "`${d}` turns a `decimal.Decimal` into text, and a `Decimal` is an object with no text form: it prints `[object Object]`",
            "Call its `to_string`, `${d.to_string()}`.",
        ),
        (
            "return \"nul: ${n}\"",
            "`${n}` turns a `Nullable<string>` into text, and a `Nullable` has no text form when it is null: it prints `null`",
            "Give the null a value with `??` first, `let n_text = n ?? \"\"`, then interpolate `${n_text}`.",
        ),
        (
            "return \"count: ${n}\"",
            "`${n}` turns a `Nullable<number>` into text, and a `Nullable` has no text form when it is null: it prints `null`",
            "Give the null a value with `??` first, `let n_value = n ?? 0`, then interpolate `${n_value}`.",
        ),
        (
            "return \"fn: ${greet}\"",
            "`${greet}` turns a `fn() -> string` into text, and a function has no text form: it prints the function's source code",
            "Call it, `${greet()}`.",
        ),
        (
            "return \"unk: ${v}\"",
            "`${v}` turns an `unknown` into text, and an `unknown` may hold a record, which has no text form: it prints `[object Object]`",
            "Say the conversion out loud with `${string.from(v)}`; if `v` may hold a record it still prints `[object Object]`, so `match` it first: `match v { is string => \"${v}\", is number => \"${v}\", else => \"?\", }`.",
        ),
        (
            "return \"a\" + v",
            "`+` joins an `unknown` to a string, and an `unknown` may hold a record, which has no text form: it prints `[object Object]`",
            "Say the conversion out loud with `string.from(v)`; if `v` may hold a record it still prints `[object Object]`, so `match` it first: `match v { is string => \"${v}\", is number => \"${v}\", else => \"?\", }`.",
        ),
        (
            "return fn(p: unknown) -> string { return \"${p}\" }",
            "`${p}` turns an `unknown` into text, and an `unknown` may hold a record, which has no text form: it prints `[object Object]`",
            "Say the conversion out loud with `${string.from(p)}`; if `p` may hold a record it still prints `[object Object]`, so `match` it first: `match p { is string => \"${p}\", is number => \"${p}\", else => \"?\", }`.",
        ),
        (
            "return \"gen: ${x}\"",
            "`${x}` turns a `T` into text, and a `T` may be a record, which has no text form: it prints `[object Object]`",
            "A `T` could be a record. Take a `fn(T) -> string` that formats it, and call that.",
        ),
    ]
}

/// The E0237 diagnostics `check --no-tsc --json` reports for `text`, as
/// `(line text, message, help)`.
fn no_text_form_diagnostics(label: &str, text: &str) -> Vec<(String, String, String)> {
    let root = unique_tmp(label);
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let (_, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--no-test", "--json", "{src}"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{label}: check --json prints JSON ({e}):\n{stdout}\n{stderr}"));
    let lines: Vec<&str> = text.lines().collect();
    v["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter(|d| d["code"] == "E0237")
        .map(|d| {
            let line = d["range"]["start"]["line"].as_u64().expect("a line") as usize;
            (
                lines[line - 1].trim().to_string(),
                d["message"].as_str().unwrap_or_default().to_string(),
                d["help"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Every `${...}` in `help` holds no string literal: D22's lexer limit means
/// one there does not compile, so a help may only bind text with `let` first.
fn assert_no_literal_inside_interpolation(help: &str) {
    let mut rest = help;
    while let Some(i) = rest.find("${") {
        let after = &rest[i + 2..];
        let end = after.find('}').unwrap_or(after.len());
        assert!(
            !after[..end].contains('"'),
            "a help puts a string literal inside `${{}}`: {help}"
        );
        rest = &after[end..];
    }
}

/// `line` with its last `from` replaced: the conversion is the last thing on
/// each probe line, and an earlier match is the literal text before it.
fn replace_last(line: &str, from: &str, to: &str) -> String {
    let i = line.rfind(from).expect("the conversion is on the line");
    format!("{}{to}{}", &line[..i], &line[i + from.len()..])
}

/// Apply one E0237 help to the module: a `let` the help names goes on the
/// line before, and the conversion is rewritten the way the help says. A help
/// with no snippet (prose) leaves the line alone and answers `false`.
fn apply_no_text_help(text: &str, line_text: &str, help: &str) -> (String, bool) {
    let snippets: Vec<&str> = help.split('`').skip(1).step_by(2).collect();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let at = lines
        .iter()
        .position(|l| l.trim() == line_text)
        .unwrap_or_else(|| panic!("no line `{line_text}`"));
    let indent: String = lines[at].chars().take_while(|c| c.is_whitespace()).collect();
    // The value as written and the conversion it stands in.
    let (value, conversion) = if let Some(i) = line_text.find("${") {
        let v = &line_text[i + 2..i + 2 + line_text[i + 2..].find('}').unwrap()];
        (v.to_string(), format!("${{{v}}}"))
    } else if let Some(i) = line_text.find("string.from(") {
        let v = &line_text[i + 12..line_text.rfind(')').unwrap()];
        (v.to_string(), format!("string.from({v})"))
    } else {
        let v = line_text.rsplit("+ ").next().unwrap().to_string();
        (v.clone(), v)
    };
    if let Some(binding) = snippets.iter().find(|s| s.starts_with("let ")) {
        let name = binding[4..].split(' ').next().unwrap().to_string();
        let replacement = if conversion.starts_with("${") {
            format!("${{{name}}}")
        } else {
            name.clone()
        };
        lines.insert(at, format!("{indent}{binding}"));
        lines[at + 1] = replace_last(&lines[at + 1], &conversion, &replacement);
        let mut out = lines.join("\n");
        out.push('\n');
        // `std/array` is named in the help when the module lacks it.
        if help.contains("`import std/array`") {
            out = out.replacen("import std/string\n", "import std/string\nimport std/array\n", 1);
        }
        return (out, true);
    }
    let Some(snippet) = snippets.iter().rev().find(|s| {
        s.contains(&format!("{value}."))
            || s.contains(&format!("{value}("))
            || s.contains(&format!("({value})"))
    }) else {
        return (text.to_string(), false);
    };
    // A `${...}` snippet stands for the text it interpolates, which beside
    // a `+` is the expression inside it.
    let snippet = match (conversion.starts_with("${"), snippet.strip_prefix("${")) {
        (false, Some(inner)) => inner.trim_end_matches('}'),
        _ => snippet,
    };
    lines[at] = replace_last(&lines[at], &conversion, snippet);
    let mut out = lines.join("\n");
    out.push('\n');
    (out, true)
}

/// G385. Each conversion of a value with no text form is E0237 with the
/// message and help computed from the program, in all three spellings and in
/// a `"""` literal. Then every help with a snippet is applied to its line and
/// the result passes the full check, `tsc` included, so a suggestion cannot
/// name code that does not compile.
#[test]
fn a_value_with_no_text_form_is_refused_and_every_help_compiles() {
    let got = no_text_form_diagnostics("g385-refused", NO_TEXT_FORM_MAIN);
    let expected = no_text_form_expected();
    for (line, message, help) in &expected {
        let hits: Vec<_> = got.iter().filter(|(l, ..)| l == line).collect();
        assert_eq!(hits.len(), 1, "one E0237 on `{line}`: {got:#?}");
        assert_eq!(hits[0].1, *message, "message on `{line}`");
        assert_eq!(hits[0].2, *help, "help on `{line}`");
        assert_no_literal_inside_interpolation(help);
    }
    assert_eq!(got.len(), expected.len(), "no other E0237: {got:#?}");

    let mut fixed = NO_TEXT_FORM_MAIN.to_string();
    let mut applied = 0;
    for (line, _, help) in &expected {
        let (next, did) = apply_no_text_help(&fixed, line, help);
        if did {
            applied += 1;
            fixed = next;
        } else {
            // A prose help: the case keeps its refusal, so drop the function
            // from the program that has to compile.
            let start = fixed.find(line).expect("the line is still there");
            let fn_start = fixed[..start].rfind("\nfn ").expect("its function") + 1;
            let fn_end = start + fixed[start..].find("\n}\n").expect("its end") + 3;
            fixed.replace_range(fn_start..fn_end, "");
        }
    }
    assert_eq!(applied, 16, "the helps with a snippet:\n{fixed}");
    let root = unique_tmp("g385-applied");
    let src = root.join("src");
    write_file(&src, "main.glyph", &fixed);
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        std::ffi::OsStr::new("--no-test"),
        main.as_os_str(),
    ]);
    assert_eq!(code, 0, "an applied help does not compile:\n{fixed}\n{stdout}\n{stderr}");
    // "Compiles" means under `tsc` too, wherever this machine has it.
    if tsc_available() {
        assert!(
            stderr.contains("tsc --strict passed"),
            "the applied helps were not type-checked by tsc:\n{stderr}"
        );
    }
}

/// G385. Text-like values convert in every spelling: a `string`, a literal
/// union, a second name and a `where` refinement of `string`, `int`,
/// `number`, `bigint`, `bool`, a field read, `never`, and an inline
/// `string | number`. A value Glyph's checker does not type (`a + b`, whose
/// result is `Unknown` until G382) is not judged, and that edge is pinned here
/// so it stays a documented gap rather than drifting either way.
#[test]
fn a_text_like_value_converts_and_an_untyped_one_is_not_judged() {
    let text = r#"module main

import std/string

type Tier = "free" | "pro"
type UserId = string
type Name = string where value.length > 0
type User = { id: number, name: string, }

fn show(s: string, t: Tier, id: UserId, n: Name, i: int, x: number, b: bigint, ok: bool, user: User, p: string | number) -> string {
  let one = "${s} ${t} ${id} ${n} ${i} ${x} ${b} ${ok} ${user.name} ${p}"
  let two = """${s} ${t} ${user.id}"""
  let three = "a " + s + " " + t + " " + id + " " + i + " " + ok + " " + user.name
  let four = string.from(i) + string.from(ok) + string.from(t)
  return one + two + three + four
}

fn untyped(a: number, c: number) -> string {
  return "sum: ${a + c}" + string.from(a * c)
}
"#;
    let got = no_text_form_diagnostics("g385-accepted", text);
    assert!(got.is_empty(), "a text-like or untyped value is refused: {got:#?}");
}

/// Record 130, F2: `string.from` takes a declared `unknown`, the documented
/// way to read a failed task's error, while `${}` and `+` still refuse one.
/// An omitted lambda annotation takes its type from its position (D55): over
/// an `Array<number>` the parameter is a `number`, which has a text form.
/// Over the `Issue.path` idiom the checker has no element type to give
/// (`Issue.path` is not typed), so `p` stays not typed here and E0237 leaves
/// it alone. An `is` arm narrows, so interpolating inside one is accepted.
///
/// The pinned hole this test held until D55, `array.map(users, fn(p) ..)`
/// over `Array<User>`, is refused now, in
/// `an_omitted_lambda_annotation_typed_from_its_callee_is_judged`.
#[test]
fn string_from_takes_unknown_and_an_untyped_lambda_parameter_is_not_judged() {
    let text = r#"module main

import std/string
import std/array

type User = { id: number, name: string, }

fn nums(xs: Array<number>) -> Array<string> {
  return array.map(xs, fn(p) -> string { return "${p}" })
}

fn path(issue: Issue) -> string {
  return string.join(array.map(issue.path, fn(p) -> string { return string.from(p) }), ".")
}

fn failed(e: unknown) -> string {
  return "failed: " + string.from(e)
}

fn narrowed(v: unknown) -> string {
  let a = match v { is string => "c ${v}", else => "?", }
  let b = match v { is number => "b ${v}", else => "?", }
  return a + b
}
"#;
    let got = no_text_form_diagnostics("g385-f2", text);
    assert!(got.is_empty(), "accepted forms are refused: {got:#?}");
}

/// D55, flipping the hole Release B pinned as G414: an omitted lambda
/// annotation takes the callee's parameter type with its type arguments
/// bound from the arguments before it, so `p` is the `User` that `users`
/// holds and `${p}` is E0237, as a written `fn(p: User)` already was. Before
/// D55 `p` was not typed here and this program printed `[object Object]`
/// with no diagnostic from Glyph or `tsc`.
#[test]
fn an_omitted_lambda_annotation_typed_from_its_callee_is_judged() {
    let text = r#"module main

import std/array

type User = { id: number, name: string, }

fn contextual_record(users: Array<User>) -> Array<string> {
  return array.map(users, fn(p) -> string { return "${p}" })
}
"#;
    let got = no_text_form_diagnostics("d55-pinned-hole", text);
    assert_eq!(got.len(), 1, "{got:#?}");
    assert_eq!(got[0].0, r#"return array.map(users, fn(p) -> string { return "${p}" })"#);
    assert!(got[0].1.contains("`${p}` turns a `User` into text"), "{got:#?}");
    assert_eq!(got[0].2, "Interpolate a field, such as `${p.id}`.");
}

/// Every diagnostic `check --no-tsc` reports for `text`, as (code, the
/// trimmed source line, message).
fn no_tsc_diagnostics(label: &str, text: &str) -> Vec<(String, String, String)> {
    let root = unique_tmp(label);
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let (_, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--no-test", "--json", "{src}"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{label}: check --json prints JSON ({e}):\n{stdout}\n{stderr}"));
    let lines: Vec<&str> = text.lines().collect();
    v["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .map(|d| {
            let line = d["range"]["start"]["line"].as_u64().expect("a line") as usize;
            (
                d["code"].as_str().unwrap_or_default().to_string(),
                lines[line - 1].trim().to_string(),
                d["message"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// D55: every position that expects a function type gives an omitted
/// parameter its type, so a field typo is E0210 under `--no-tsc` in each,
/// where Release B left every one of them to `tsc`: a stdlib callee (with a
/// second parameter bound by an earlier argument), a generic Glyph callee,
/// an annotated `let` and module `const` (through an alias), a `return` from
/// a function declared to return one, and a field of a record literal
/// standing where a record type is declared. A lambda with no such position
/// stays not typed here, so `q.anything` is not judged.
#[test]
fn an_omitted_lambda_annotation_takes_its_type_from_each_slot() {
    let text = r#"module main

import std/array

type User = { id: number, name: string, }
type Show = fn(User) -> string
type Handlers = { on_user: fn(User) -> string, }

const SHOW: Show = fn(u) -> string { return u.nmae }

fn gen<T>(xs: Array<T>, f: fn(T) -> string) -> Array<string> {
  return array.map(xs, f)
}

pub fn slots(users: Array<User>) -> number {
  let a = array.map(users, fn(u) -> string { return u.nmae })
  let b = gen(users, fn(u) -> string { return u.nmae })
  let c = array.fold(users, 0, fn(acc, u) -> number { return acc + u.idd })
  let f: fn(User) -> string = fn(u) -> string { return u.nmae }
  let h: Handlers = { on_user: fn(u) -> string { return u.nmae }, }
  let g = fn(q) -> string { return q.anything }
  let _all = [a, b, f, h, g]
  return c
}

pub fn ret() -> Show {
  return fn(u) -> string { return u.nmae }
}
"#;
    let got = no_tsc_diagnostics("d55-slots", text);
    let e0210: Vec<&str> = got.iter().filter(|d| d.0 == "E0210").map(|d| d.1.as_str()).collect();
    assert_eq!(
        e0210,
        vec![
            "const SHOW: Show = fn(u) -> string { return u.nmae }",
            "let a = array.map(users, fn(u) -> string { return u.nmae })",
            "let b = gen(users, fn(u) -> string { return u.nmae })",
            "let c = array.fold(users, 0, fn(acc, u) -> number { return acc + u.idd })",
            "let f: fn(User) -> string = fn(u) -> string { return u.nmae }",
            "let h: Handlers = { on_user: fn(u) -> string { return u.nmae }, }",
            "return fn(u) -> string { return u.nmae }",
        ],
        "{got:#?}"
    );
    assert!(!got.iter().any(|d| d.1.contains("q.anything")), "{got:#?}");
}

/// Record 142, B1: the method spelling of a callback gets the answer the
/// function spelling does. The generated method rows model these callbacks
/// as `unknown` (or leave the method untyped where TypeScript declares
/// overloads), so before the context-only slots `users.map(fn(p) ..)` was
/// unjudged while `array.map(users, fn(p) ..)` was E0237, and an agent could
/// silence the refusal by switching spellings. `reduce` types the
/// accumulator from its seed and `sort` passes two elements. The `sort` is a
/// `mut` statement since G380: it changes `users`, so its value is not read.
#[test]
fn a_method_callback_is_judged_like_the_function_spelling() {
    let text = r#"module main

import std/array

type User = { id: number, name: string, }

pub fn both(users: Array<User>) -> number {
  let a1 = array.map(users, fn(p) -> string { return "${p}" })
  let a2 = users.map(fn(p) -> string { return "${p}" })
  let b1 = array.filter(users, fn(p) -> bool { return p.nmae == "a" })
  let b2 = users.filter(fn(p) -> bool { return p.nmae == "a" })
  let c1 = array.find(users, fn(p) -> bool { return p.nmae == "a" })
  let c2 = users.find(fn(p) -> bool { return p.nmae == "a" })
  users.forEach(fn(p) { let _s = p.nmae })
  let d = users.reduce(fn(acc, p) -> number { return acc + p.idd }, 0)
  mut users.sort(fn(x, y) -> number { return x.idd - y.id })
  let _all = [b1, b2, c1, c2]
  return a1.length + a2.length + d
}
"#;
    let got = no_tsc_diagnostics("d55-methods", text);
    let errors: Vec<(&str, &str)> =
        got.iter().filter(|d| d.0.starts_with("E0")).filter(|d| d.0 != "E0107").map(|d| (d.0.as_str(), d.1.as_str())).collect();
    assert_eq!(
        errors,
        vec![
            ("E0237", r#"let a1 = array.map(users, fn(p) -> string { return "${p}" })"#),
            ("E0237", r#"let a2 = users.map(fn(p) -> string { return "${p}" })"#),
            ("E0210", r#"let b1 = array.filter(users, fn(p) -> bool { return p.nmae == "a" })"#),
            ("E0210", r#"let b2 = users.filter(fn(p) -> bool { return p.nmae == "a" })"#),
            ("E0210", r#"let c1 = array.find(users, fn(p) -> bool { return p.nmae == "a" })"#),
            ("E0210", r#"let c2 = users.find(fn(p) -> bool { return p.nmae == "a" })"#),
            // The method spelling of `find` is refused for its result since
            // 0.1.161 (G392, D56); its callback is still judged.
            ("E0242", r#"let c2 = users.find(fn(p) -> bool { return p.nmae == "a" })"#),
            ("E0210", "users.forEach(fn(p) { let _s = p.nmae })"),
            ("E0210", "let d = users.reduce(fn(acc, p) -> number { return acc + p.idd }, 0)"),
            ("E0210", "mut users.sort(fn(x, y) -> number { return x.idd - y.id })"),
        ],
        "{got:#?}"
    );
}

/// Every diagnostic `check --no-tsc` reports for `text` except unused-name
/// lints, as (code, 1-based line, message, help).
fn no_tsc_diagnostics_with_help(label: &str, text: &str) -> Vec<(String, usize, String, String)> {
    let root = unique_tmp(label);
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let (_, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--no-test", "--json", "{src}"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{label}: check --json prints JSON ({e}):\n{stdout}\n{stderr}"));
    v["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter(|d| d["code"] != "E0107")
        .map(|d| {
            (
                d["code"].as_str().unwrap_or_default().to_string(),
                d["range"]["start"]["line"].as_u64().expect("a line") as usize,
                d["message"].as_str().unwrap_or_default().to_string(),
                d["help"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Record 143, item 1: the E0238 help for a pending tail reads the callback
/// slot the checker typed the lambda from, so a built-in method's slot
/// chooses the rewrite its function spelling would. `forEach` drops what its
/// callback returns, so the help is the `task.detach` form; `map` keeps it,
/// so the help is the loop over the receiver. Before the method slots were
/// recorded both got the generic "write `async fn(p)`" help, which turns a
/// `forEach` into one that starts every save at once and a `map` into an
/// array of pending values.
#[test]
fn a_method_callback_pending_tail_gets_the_help_for_its_slot() {
    let text = r#"module main

type User = { id: number, name: string, }

async fn save(id: number) -> number {
  return id
}

pub fn each(users: Array<User>) -> void {
  users.forEach(fn(p) { save(p.id) })
}

pub fn mapped(users: Array<User>) -> number {
  let xs = users.map(fn(p) { save(p.id) })
  return xs.length
}
"#;
    let got = no_tsc_diagnostics_with_help("d55-pending-slot", text);
    let e0238: Vec<(usize, &str)> =
        got.iter().filter(|d| d.0 == "E0238").map(|d| (d.1, d.3.as_str())).collect();
    assert_eq!(
        e0238,
        vec![
            (
                10,
                "This callback is called without being awaited, so it cannot wait. Start the work on \
                 purpose: `fn(p) { task.detach(async fn() -> void { await save(p.id) }) }` (with \
                 `import std/task`)."
            ),
            (
                14,
                "`users.map` does not wait for its callback. Await each call in a loop, `for p in \
                 users { ... await save(p.id) ... }`, or run them together with `task.all`."
            ),
        ],
        "{got:#?}"
    );
}

/// Record 143, items 2 and 3: a contextually typed parameter that has no
/// text form, or a pending call made with it, draws exactly one diagnostic
/// in a `${}` and in a `+` chain, in both spellings of the call: E0237 for
/// the record, E0238 for the pending value, never both. The E0237 message
/// names the type the parameter took from its position.
#[test]
fn a_contextual_parameter_draws_one_text_or_pending_diagnostic() {
    let text = r#"module main

import std/array

type User = { id: number, name: string, }

async fn load(u: User) -> string {
  return u.name
}

pub fn a(users: Array<User>) -> Array<string> {
  return array.map(users, fn(p) -> string { return "${p}" })
}

pub fn b(users: Array<User>) -> Array<string> {
  return users.map(fn(p) -> string { return "x " + p + "!" })
}

pub fn c(users: Array<User>) -> Array<string> {
  return array.map(users, fn(p) -> string { return "${load(p)}" })
}

pub fn d(users: Array<User>) -> Array<string> {
  return users.map(fn(p) -> string { return "x " + load(p) })
}
"#;
    let got = no_tsc_diagnostics_with_help("d55-one-diagnostic", text);
    let codes: Vec<(&str, usize)> = got.iter().map(|d| (d.0.as_str(), d.1)).collect();
    assert_eq!(codes, vec![("E0237", 12), ("E0237", 16), ("E0238", 20), ("E0238", 24)], "{got:#?}");
    assert!(got[0].2.contains("`${p}` turns a `User` into text"), "{got:#?}");
    assert!(got[1].2.contains("`+` joins a `User` to a string"), "{got:#?}");
}

/// A two-binding `for` over a declared `unknown` that holds an array binds a
/// numeric index at run time. The emitter took the record form for any typed
/// iterand that was not an `Array`, so inside an `is Array<unknown>` arm the
/// index was a string and `i + 1` printed `01,11`. Contextual typing (D55)
/// gives `01_validator`'s `parse: fn(input) { .. }` the slot's declared
/// `unknown`, which put its array loop on that path; the iterand now defers
/// to the run time as an untyped one does. The bindings are `unknown` to both
/// checkers (record 146), so `i + 1` is refused by `tsc`; this runs it with
/// `--no-tsc` to pin what the loop binds.
#[test]
fn a_declared_unknown_array_loop_binds_a_numeric_index() {
    if !js_toolchain_available() {
        eprintln!("skipping the declared-unknown loop run: node/tsx not available");
        return;
    }
    let root = unique_tmp("d55-unknown-pairs");
    let text = r#"module main

fn last_index(input: unknown) -> string {
  return match input {
    is Array<unknown> => {
      let found: Array<string> = []
      for i, item in input {
        mut found.push("${i + 1}")
      }
      found.join(",")
    },
    else => "none",
  }
}

fn main() -> void {
  print(last_index(["a", "b"]))
}
"#;
    write_file(&root.join("src"), "main.glyph", text);
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert_eq!(stdout.trim(), "1,2", "{stderr}");
}

/// Linus's record 146 probe, `t/a.glyph`: the run-time pairs of a declared
/// `unknown` loop, over an array at the top level and inside an
/// `is Array<unknown>` arm, give numeric indexes (`1,2,` for both), as they
/// must for the validator's array loop.
#[test]
fn a_declared_unknown_loop_over_an_array_prints_numeric_indexes() {
    if !js_toolchain_available() {
        eprintln!("skipping the record 146 run: node/tsx not available");
        return;
    }
    let root = unique_tmp("d55-r146-a");
    let text = r#"module main

import std/io
import std/json

fn walk(v: unknown) -> string {
  let out = ""
  for i, x in v {
    mut out = out + "${i + 1},"
  }
  return out
}

fn arm(v: unknown) -> string {
  return match v {
    is Array<unknown> => {
      let out = ""
      for i, x in v {
        mut out = out + "${i + 1},"
      }
      out
    },
    else => "no",
  }
}

fn main(argv: Array<string>) -> number {
  match json.parse<unknown>("[10, 20]") {
    Ok(v) => {
      io.println(walk(v))
      io.println(arm(v))
    },
    Err(e) => io.println("err"),
  }
  return 0
}
"#;
    write_file(&root.join("src"), "main.glyph", text);
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["1,2,", "1,2,"], "{stderr}");
}

/// Record 146, B1: the bindings of a loop over a declared `unknown` are
/// `unknown` to `tsc` as they are to Glyph's checker. The helper the loop
/// emits is declared `[any, any]`, so this program passed both checks on the
/// release branch and `glyph run` crashed with `s.toUpperCase is not a
/// function` on `[10, 20]`. Full `check` refuses each use now: TS2322 where
/// the index is declared a `string` or a `number`, TS18046 where a method is
/// called on it.
#[test]
fn a_declared_unknown_loop_binding_is_unknown_to_tsc() {
    if !tsc_available() {
        eprintln!("skipping the record 146 tsc check: tsc not available");
        return;
    }
    let text = r#"module main

fn walk(v: unknown) -> string {
  let out = ""
  for i, x in v {
    let n: number = i
    let s: string = i
    mut out = out + s.toUpperCase() + i.toUpperCase() + "${n},"
  }
  return out
}
"#;
    let root = unique_tmp("d55-r146-b");
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("check"), std::ffi::OsStr::new("--no-test"), main.as_os_str()]);
    assert_eq!(code, 1, "{stdout}\n{stderr}");
    let all = format!("{stdout}\n{stderr}");
    assert!(all.contains("[TS2322] Error: tsc: Type 'unknown' is not assignable to type 'number'."), "{all}");
    assert!(all.contains("[TS2322] Error: tsc: Type 'unknown' is not assignable to type 'string'."), "{all}");
    assert!(all.contains("[TS18046] Error: tsc: 'i' is of type 'unknown'."), "{all}");
}

/// `examples/01_validator.glyph` passes full `check`, `tsc` included, now that
/// its array loop's index is `unknown` to both checkers: it reads the index
/// only through `string.from`, which takes a declared `unknown` (record 146).
#[test]
fn the_validator_example_passes_full_check() {
    if !tsc_available() {
        eprintln!("skipping the validator tsc check: tsc not available");
        return;
    }
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/01_validator.glyph");
    let text = std::fs::read_to_string(&example).expect("the validator example");
    let root = unique_tmp("d55-validator");
    let src = root.join("src");
    write_file(&src, "main.glyph", &text);
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("check"), std::ffi::OsStr::new("--no-test"), main.as_os_str()]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stderr.contains("tsc --strict passed"), "{stderr}");
}

/// Record 142, B2: an attribute of a component element is a field of the
/// props object the element passes, so a lambda in it takes the field's
/// type, as the record-literal form of the same props already did. A DOM
/// element's attribute (`onClick` on a `<button>`) is typed by React's
/// declarations, which the checker does not hold, so `e.nmae7` there is
/// left to `tsc`.
#[test]
fn a_component_attribute_lambda_takes_the_props_field_type() {
    let text = r#"module main

type User = { name: string, }
type CardProps = { user: User, on_pick: fn(User) -> void, }

component Card(props: CardProps) -> Component {
  return <button>{props.user.name}</button>
}

pub component List(props: { users: Array<User>, }) -> Component {
  let u: User = { name: "a" }
  let _p: CardProps = { user: u, on_pick: fn(x) { let _s = x.nmae5 } }
  return <Card user={u} on_pick={fn(x) { let _s = x.nmae6 }} />
}

pub component Dom(props: { users: Array<User>, }) -> Component {
  return <button onClick={fn(e) { let _s = e.nmae7 }}>x</button>
}
"#;
    let got = no_tsc_diagnostics("d55-jsx", text);
    let e0210: Vec<&str> = got.iter().filter(|d| d.0 == "E0210").map(|d| d.1.as_str()).collect();
    assert_eq!(
        e0210,
        vec![
            "let _p: CardProps = { user: u, on_pick: fn(x) { let _s = x.nmae5 } }",
            "return <Card user={u} on_pick={fn(x) { let _s = x.nmae6 }} />",
        ],
        "{got:#?}"
    );
    assert!(!got.iter().any(|d| d.1.contains("nmae7")), "{got:#?}");
}

/// D55 refuses nothing `tsc` accepts in the common callback shapes, and the
/// types it gives are the ones `tsc` infers: a string literal seed widens to
/// `string` (`acc == ""` compares strings, not two literals), a literal-union
/// element stays the union, a written type argument binds, a second
/// parameter after a bound one is typed, and a callee type parameter nothing
/// has bound is left not typed. The full `check` runs `tsc` too.
#[test]
fn contextually_typed_callbacks_compile_under_tsc() {
    let text = r#"module main

import std/array
import std/string
import std/io

type User = { id: number, name: string, tags: Array<string>, }
type Tier = "free" | "pro"

fn apply<T>(x: T, f: fn(T) -> T) -> T {
  return f(x)
}

fn each<T>(xs: Array<T>, f: fn(T, number) -> string) -> Array<string> {
  return array.map(xs, fn(x) -> string { return f(x, 0) })
}

fn pick<T>(f: fn(T) -> string) -> string {
  return "picked"
}

pub fn run(users: Array<User>, tiers: Array<Tier>) -> string {
  let names = array.map(users, fn(u) -> string { return u.name })
  let joined = array.fold(users, "", fn(acc, u) -> string { return "${acc},${u.name}" })
  let same = array.fold(users, "", fn(acc, u) -> string {
    let first = acc == ""
    return match first { true => u.name, false => acc, }
  })
  let total = array.fold(users, 0, fn(acc, u) -> number { return acc + u.id })
  let sorted = array.sort(users, fn(a, b) -> number { return a.id - b.id })
  let pros = array.filter(tiers, fn(t) -> bool { return t == "pro" })
  let found = array.find(users, fn(u) -> bool { return u.name == "x" })
  let tagged = array.flat_map(users, fn(u) -> Array<string> { return u.tags })
  let two = apply(2, fn(n) -> number { return n * 2 })
  let typed = apply<User>({ id: 0, name: "", tags: [], }, fn(u) -> User { return u })
  let shown = each(users, fn(u, i) -> string { return "${u.name} ${i}" })
  let open = pick(fn(x) -> string { return "x" })
  let sep = ","
  let shown_text = string.join(shown, sep)
  let pros_text = string.join(pros, sep)
  let tagged_text = string.join(tagged, sep)
  io.println("${total} ${two} ${typed.name} ${shown_text} ${pros_text} ${open}")
  io.println("${tagged_text} ${array.len(sorted)} ${same}")
  let _f = found
  return string.join(names, sep) + joined
}
"#;
    let got = no_tsc_diagnostics("d55-accepted", text);
    assert!(got.is_empty(), "contextually typed callbacks are refused: {got:#?}");
    let root = unique_tmp("d55-accepted-tsc");
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) =
        spawn_glyph(&[std::ffi::OsStr::new("check"), std::ffi::OsStr::new("--no-test"), main.as_os_str()]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    if tsc_available() {
        assert!(stderr.contains("tsc --strict passed"), "{stderr}");
    }
}

/// D55 leaves a written annotation alone: `fn(p: unknown)` over an
/// `Array<number>` is the declared `unknown`, so `${p}` is refused, where the
/// position alone would have typed `p` a `number`. A callback slot whose
/// type the checker does not know gives no type: `Issue.path` is not typed
/// (G407), so the `forEach` method on it has no element type to pass, and
/// `p.nmae` there is not judged here.
#[test]
fn a_written_annotation_wins_and_an_unknown_slot_types_nothing() {
    let text = r#"module main

import std/array
import std/io

type User = { id: number, name: string, }

pub fn written(xs: Array<number>) -> Array<string> {
  return array.map(xs, fn(p: unknown) -> string { return "${p}" })
}

pub fn unmodeled(issue: Issue) -> void {
  issue.path.forEach(fn(p) { io.println(p.nmae) })
}
"#;
    let got = no_tsc_diagnostics("d55-written", text);
    let codes: Vec<(&str, &str)> = got.iter().map(|d| (d.0.as_str(), d.1.as_str())).collect();
    assert_eq!(
        codes,
        vec![("E0237", r#"return array.map(xs, fn(p: unknown) -> string { return "${p}" })"#)],
        "{got:#?}"
    );
}

/// Record 130: both snippets of the `unknown` help compile, `tsc` included.
#[test]
fn both_snippets_of_the_unknown_help_compile() {
    let help_of = |src: &str| -> String {
        let got = no_text_form_diagnostics("g385-unknown-help", src);
        assert_eq!(got.len(), 1, "{got:#?}");
        got[0].2.clone()
    };
    let refused = "module main\n\nimport std/string\n\npub fn show(v: unknown) -> string {\n  return \"v: ${v}\"\n}\n";
    let help = help_of(refused);
    let snippets: Vec<&str> = help.split('`').skip(1).step_by(2).collect();
    let said = snippets.iter().find(|s| s.starts_with("${string.from")).expect("the string.from snippet");
    let matched = snippets.iter().find(|s| s.starts_with("match v")).expect("the match snippet");
    let programs = [
        refused.replace("\"v: ${v}\"", &format!("\"v: {said}\"")),
        refused.replace("return \"v: ${v}\"", &format!("return {matched}")),
    ];
    for (i, program) in programs.iter().enumerate() {
        let root = unique_tmp(&format!("g385-unknown-applied-{i}"));
        let src = root.join("src");
        write_file(&src, "main.glyph", program);
        let main = src.join("main.glyph");
        let (code, stdout, stderr, _) = spawn_glyph(&[
            std::ffi::OsStr::new("check"),
            std::ffi::OsStr::new("--no-test"),
            main.as_os_str(),
        ]);
        assert_eq!(code, 0, "an unknown-help snippet does not compile:\n{program}\n{stdout}\n{stderr}");
        if tsc_available() {
            assert!(stderr.contains("tsc --strict passed"), "{stderr}");
        }
    }
}

/// Record 139, B1: a `+` chain is judged whole. A `+` result is `Unknown`
/// until G382, so a pairwise rule missed `u` in both of these. Each is one
/// E0237 at `u`, with the computed help. A field read in a chain is text, and
/// a chain whose operands are all `Unknown` is not judged.
#[test]
fn a_plus_chain_is_judged_whole() {
    let text = r#"module main

type User = { name: string, }

pub fn chained(u: User, name: string, n: number) -> string {
  let a1 = "Hello " + name + ", " + u
  let a9 = ("a" + name) + u
  let a5 = "x" + n + u.name
  return a1 + a9 + a5
}

pub fn untyped(a: number, c: number) -> string {
  let x = (a * c) + (a - c)
  return "${x}"
}
"#;
    let got = no_text_form_diagnostics("g385-plus-chain", text);
    let message = "`+` joins a `User` to a string, and a record has no text form: it prints `[object Object]`";
    let help = "Join a field, such as `u.name`.";
    assert_eq!(
        got,
        vec![
            ("let a1 = \"Hello \" + name + \", \" + u".to_string(), message.to_string(), help.to_string()),
            ("let a9 = (\"a\" + name) + u".to_string(), message.to_string(), help.to_string()),
        ],
        "one E0237 at `u` in each chain, nothing else"
    );
}

/// Record 139, B2: beside a `+`, the `unknown` help's call is the operand
/// itself, `string.from(v)`, and it compiles there under the full check.
#[test]
fn the_unknown_help_beside_a_plus_compiles() {
    let refused = "module main\n\nimport std/string\n\npub fn show(v: unknown) -> string {\n  return \"v: \" + v\n}\n";
    let got = no_text_form_diagnostics("g385-unknown-plus", refused);
    assert_eq!(got.len(), 1, "{got:#?}");
    let help = &got[0].2;
    let said = help.split('`').nth(1).expect("the first snippet");
    assert_eq!(said, "string.from(v)", "{help}");
    let program = refused.replace("\"v: \" + v", &format!("\"v: \" + {said}"));
    let root = unique_tmp("g385-unknown-plus-applied");
    let src = root.join("src");
    write_file(&src, "main.glyph", &program);
    let main = src.join("main.glyph");
    let (code, stdout, stderr, _) = spawn_glyph(&[
        std::ffi::OsStr::new("check"),
        std::ffi::OsStr::new("--no-test"),
        main.as_os_str(),
    ]);
    assert_eq!(code, 0, "the + snippet does not compile:\n{program}\n{stdout}\n{stderr}");
    if tsc_available() {
        assert!(stderr.contains("tsc --strict passed"), "{stderr}");
    }
}

/// G387's probe (record 129): five ways of building a query from a run-time
/// `name`, and two constant queries beside them. One line is added to the
/// record's file, the `built` count: the rows the five built queries return or
/// change. `rows` also counts `f`, a parameterized query that runs after the
/// `UPDATE` and so matches the payload once the `UPDATE` stores it.
const G387_PROBE: &str = r#"module main

import std/sqlite
import std/io

const TABLE: string = "users"

fn count(db: sqlite.Db, name: string) -> number {
  let a = db.query("SELECT * FROM users WHERE name = '${name}'", [])
  let q = "SELECT * FROM users WHERE name = '${name}'"
  let b = db.query(q, [])
  let c = db.query("SELECT * FROM users WHERE name = '" + name + "'", [])
  let d = db.query_one("SELECT * FROM ${TABLE} WHERE name = ?", [name])
  let e = db.run("DELETE FROM users WHERE name = '${name}'", [])
  db.exec("UPDATE users SET name = '${name}'")
  let f = db.query("SELECT * FROM users WHERE name = ?", [name])
  let n = match d { Some(_) => 1, None => 0, }
  let built = a.length + b.length + c.length + e
  io.println("built ${built}")
  return a.length + b.length + c.length + f.length + e + n
}

fn main(argv: Array<string>) -> number {
  let db = sqlite.open(":memory:")
  db.exec("CREATE TABLE users (name TEXT)")
  db.exec("INSERT INTO users VALUES ('ada'), ('linus')")
  let evil = "x' OR '1'='1"
  let total = count(db, evil)
  io.println("rows ${total}")
  return 0
}
"#;

/// The backticked snippets of a help, in order.
fn backticked(help: &str) -> Vec<String> {
    help.split('`').skip(1).step_by(2).map(str::to_string).collect()
}

/// G387, D52: every E0239 on the probe carries a rewrite, and applying each
/// one as written gives a program that `glyph check` accepts (with `tsc`) and
/// that returns no rows for the injection payload. The help is applied the
/// way an agent would: a `let q = ...` snippet replaces that `let`, and a call
/// snippet replaces the call on the diagnostic's line. Nothing is typed in
/// by hand, so a help that does not compile fails here.
#[test]
fn g387_applying_every_e0239_rewrite_compiles_and_returns_no_rows() {
    if !js_toolchain_available() {
        eprintln!("skipping G387 rewrite run: node/tsx not available");
        return;
    }
    let root = unique_tmp("g387_rewrite");
    write_file(&root, "src/main.glyph", G387_PROBE);
    let (code, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--json", "{src}"]);
    assert_eq!(code, 1, "the probe is refused:\n{stdout}\n{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("check --json prints JSON");
    let diags: Vec<&serde_json::Value> = v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["code"] == "E0239")
        .collect();
    assert_eq!(diags.len(), 5, "one E0239 per built query:\n{stdout}");

    let mut lines: Vec<String> = G387_PROBE.lines().map(str::to_string).collect();
    for d in &diags {
        let line = d["range"]["start"]["line"].as_u64().expect("line") as usize - 1;
        let help = d["help"].as_str().expect("help");
        let snippets: Vec<String> = backticked(help)
            .into_iter()
            .filter(|s| s.starts_with("let ") || (s.starts_with("db.") && s.contains('[') && !s.contains("[values]")))
            .collect();
        assert!(!snippets.is_empty(), "the help carries a rewrite: {help}");
        for s in snippets {
            if let Some(rest) = s.strip_prefix("let ") {
                let name = rest.split(' ').next().expect("a name");
                let at = lines
                    .iter()
                    .position(|l| l.trim_start().starts_with(&format!("let {name} = ")))
                    .expect("the let the help rewrites");
                lines[at] = format!("  {s}");
            } else {
                let l = &lines[line];
                let start = l.find("db.").expect("the call on the diagnostic's line");
                let end = l.rfind(')').expect("the call's close") + 1;
                lines[line] = format!("{}{s}{}", &l[..start], &l[end..]);
            }
        }
    }
    let fixed = lines.join("\n") + "\n";
    write_file(&root, "src/main.glyph", &fixed);
    let check_args: &[&str] = if tsc_available() {
        &["check", "{src}"]
    } else {
        &["check", "--no-tsc", "{src}"]
    };
    let (code, stdout, stderr) = glyph_in(&root, check_args);
    assert_eq!(code, 0, "the rewritten probe compiles:\n{fixed}\n{stdout}\n{stderr}");
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 0, "{fixed}\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("built 0"),
        "the payload matches nothing once it is a parameter:\n{fixed}\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("rows 2"), "only `f`, after the `UPDATE`:\n{fixed}\n{stdout}");
}

/// Record 132, B2: a constant from another module gets one diagnosis whatever
/// the import spelling. `BASE` imported by name and `lib.BASE` through the
/// namespace draw the same E0239, with the same message and help.
#[test]
fn g387_an_imported_constant_reads_the_same_by_name_and_through_the_namespace() {
    let root = unique_tmp("g387_b2");
    write_file(
        &root,
        "src/lib.glyph",
        "module lib\n\npub const BASE: string = \"SELECT * FROM users\"\n",
    );
    write_file(
        &root,
        "src/main.glyph",
        "module main\n\nimport lib\nimport lib { BASE }\nimport std/sqlite\n\nfn main(argv: Array<string>) -> number {\n  let db = sqlite.open(\":memory:\")\n  let a = db.query(BASE, [])\n  let b = db.query(lib.BASE, [])\n  return a.length + b.length\n}\n",
    );
    let (code, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--json", "{src}"]);
    assert_eq!(code, 1, "{stdout}\n{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("check --json prints JSON");
    let got: Vec<(String, String)> = v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["code"] == "E0239")
        .map(|d| {
            (
                d["message"].as_str().unwrap_or_default().to_string(),
                d["help"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(got.len(), 2, "{stdout}");
    assert_eq!(got[0], got[1], "the import spelling does not change the diagnosis");
    assert!(got[0].0.contains("`BASE` comes from another module"), "{got:?}");
    assert!(got[0].1.contains("pub const BASE: sqlite.Sql"), "{got:?}");
}

/// The end of the call that starts at byte `start` of `line` (the index just
/// past its closing parenthesis), skipping parentheses inside string
/// literals.
fn call_end(line: &str, start: usize) -> usize {
    let mut depth = 0;
    let mut in_str = false;
    let bytes = line.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'(' if !in_str => depth += 1,
            b')' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("no end to the call in {line}");
}

/// Record 132, B1: every rewrite the help offers in a value position is run
/// against node:sqlite. Each must return 0 rows for its injection payload and
/// the same count as the constant query written out for a benign input. The
/// `LIMIT` value is an `int`, which cannot carry an injection, so only its
/// benign count is compared.
#[test]
fn g387_every_offered_rewrite_runs_on_node_sqlite() {
    if !js_toolchain_available() {
        eprintln!("skipping G387 node:sqlite rewrites: node/tsx not available");
        return;
    }
    let cases = r#"fn q1(db: sqlite.Db, x: string) -> number {
  return db.query("SELECT * FROM users WHERE name = '${x}'", []).length
}

fn q2(db: sqlite.Db, id: string) -> number {
  return db.query("SELECT * FROM users WHERE id = ${id}", []).length
}

fn q3(db: sqlite.Db, n: int) -> number {
  return db.query("SELECT * FROM users LIMIT ${n}", []).length
}

fn q4(db: sqlite.Db, a: string, b: string) -> number {
  return db.run("INSERT INTO users (name, id) VALUES ('${a}', ${b})", [])
}

fn q5(db: sqlite.Db, id: string) -> number {
  return db.query("SELECT * FROM users WHERE id IN (0, ${id})", []).length
}
"#;
    let main = r#"
fn main(argv: Array<string>) -> number {
  let db = sqlite.open(":memory:")
  db.exec("CREATE TABLE users (id INTEGER, name TEXT)")
  db.exec("INSERT INTO users VALUES (1, 'ada'), (2, 'linus')")
  let ada = "ada"
  let one = "1"
  let quote = "x' OR '1'='1"
  let or_all = "1 OR 1=1"
  let close = "1) OR (1=1"
  let b1 = q1(db, ada)
  let k1 = db.query("SELECT * FROM users WHERE name = 'ada'", []).length
  let p1 = q1(db, quote)
  io.println("q1 benign=${b1} const=${k1} payload=${p1}")
  let b2 = q2(db, one)
  let k2 = db.query("SELECT * FROM users WHERE id = 1", []).length
  let p2 = q2(db, or_all)
  io.println("q2 benign=${b2} const=${k2} payload=${p2}")
  let b3 = q3(db, 1)
  let k3 = db.query("SELECT * FROM users LIMIT 1", []).length
  io.println("q3 benign=${b3} const=${k3} payload=0")
  let bob = "bob"
  let three = "3"
  let b4 = q4(db, bob, three)
  let k4 = db.run("INSERT INTO users (name, id) VALUES ('bob', 3)", [])
  let evil = "x', 9), ('evil', 10) --"
  let five = "5"
  let _ = q4(db, evil, five)
  let p4 = db.query("SELECT * FROM users WHERE name = 'evil'", []).length
  io.println("q4 benign=${b4} const=${k4} payload=${p4}")
  let b5 = q5(db, one)
  let k5 = db.query("SELECT * FROM users WHERE id IN (0, 1)", []).length
  let p5 = q5(db, close)
  io.println("q5 benign=${b5} const=${k5} payload=${p5}")
  return 0
}
"#;
    let head = "module main\n\nimport std/sqlite\nimport std/io\n\n";
    let probe = format!("{head}{cases}{main}");
    let root = unique_tmp("g387_node");
    write_file(&root, "src/main.glyph", &probe);
    let (code, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--json", "{src}"]);
    assert_eq!(code, 1, "the built queries are refused:\n{stdout}\n{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("check --json prints JSON");
    let diags: Vec<&serde_json::Value> = v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["code"] == "E0239")
        .collect();
    assert_eq!(diags.len(), 5, "one E0239 per case:\n{stdout}");

    // Record 132's three probes where a parameter cannot stand: the help
    // offers no `?` rewrite for any of them, since run on node:sqlite the old
    // rewrites threw (`FROM ?`), sorted by a constant (`ORDER BY ?`) or
    // dropped the second statement (`run` of two).
    let names = format!(
        "{head}pub fn n(db: sqlite.Db, table: string, col: string, first: string) -> number {{\n  let a = db.query(\"SELECT * FROM ${{table}} WHERE id = ?\", [1])\n  let b = db.query(\"SELECT * FROM users ORDER BY ${{col}}\", [])\n  db.exec(\"DELETE FROM a WHERE x = '${{first}}'; DELETE FROM b\")\n  return a.length + b.length\n}}\n"
    );
    let nroot = unique_tmp("g387_node_names");
    write_file(&nroot, "src/main.glyph", &names);
    let (_, nout, _) = glyph_in(&nroot, &["check", "--no-tsc", "--json", "{src}"]);
    let nv: serde_json::Value = serde_json::from_str(&nout).expect("check --json prints JSON");
    let nhelps: Vec<String> = nv["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["code"] == "E0239")
        .map(|d| d["help"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(nhelps.len(), 3, "{nout}");
    for h in &nhelps {
        assert!(
            !backticked(h).iter().any(|s| s.starts_with("db.") && s.contains('?')),
            "no placeholder rewrite where SQL needs a name or one statement: {h}"
        );
    }

    let mut lines: Vec<String> = probe.lines().map(str::to_string).collect();
    for d in &diags {
        let line = d["range"]["start"]["line"].as_u64().expect("line") as usize - 1;
        let help = d["help"].as_str().expect("help");
        let rewrite = backticked(help)
            .into_iter()
            .find(|s| s.starts_with("db.") && s.contains('[') && !s.contains("[values]"))
            .unwrap_or_else(|| panic!("a value position gets a rewrite: {help}"));
        let l = lines[line].clone();
        let start = l.find("db.").expect("the call");
        let end = call_end(&l, start);
        lines[line] = format!("{}{rewrite}{}", &l[..start], &l[end..]);
    }
    let fixed = lines.join("\n") + "\n";
    write_file(&root, "src/main.glyph", &fixed);
    let (code, stdout, stderr) = glyph_in(&root, &["run", "--no-tsc", "{src}"]);
    assert_eq!(code, 0, "{fixed}\n{stdout}\n{stderr}");
    for case in ["q1", "q2", "q3", "q4", "q5"] {
        let row = stdout
            .lines()
            .find(|l| l.starts_with(&format!("{case} ")))
            .unwrap_or_else(|| panic!("{case} ran:\n{stdout}"));
        let field = |k: &str| -> String {
            row.split(' ')
                .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(field("benign"), field("const"), "{case}: same rows as the constant query:\n{fixed}\n{stdout}");
        assert_eq!(field("payload"), "0", "{case}: the payload matches nothing:\n{fixed}\n{stdout}");
    }
}

/// Records 131, 139 and 141: a pending value in each text conversion, a `+`
/// chain included, is exactly one E0238 and never E0237, since `await` is the
/// fix and E0238 owns it.
#[test]
fn a_pending_value_converted_to_text_is_e0238_alone() {
    let text = r#"module main

import std/string

async fn load() -> number {
  return 1
}

pub async fn pend(name: string) -> string {
  let a = "x ${load()}"
  let b = "a" + load()
  let c = string.from(load())
  let d = "a" + name + load() + "b"
  return a + b + c + d
}
"#;
    let root = unique_tmp("g385-pending");
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let (_, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--no-test", "--json", "{src}"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}\n{stderr}"));
    let lines: Vec<(u64, String)> = v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["severity"] == "error")
        .map(|d| (d["range"]["start"]["line"].as_u64().unwrap(), d["code"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        lines,
        vec![
            (10, "E0238".to_string()),
            (11, "E0238".to_string()),
            (12, "E0238".to_string()),
            (13, "E0238".to_string()),
        ],
        "one E0238 per conversion and no E0237: {stdout}"
    );
}

/// Record 132, ruling 5, and record 141: `sqlite.Sql` is a `string` whose
/// text is fixed when the program is written, so it is text-like in `${}` and
/// beside `+`, and what it builds stays a constant query (no E0239).
#[test]
fn a_sqlite_sql_value_becomes_text_and_stays_constant() {
    let text = r#"module main

import std/sqlite

pub fn sql_text(sql: sqlite.Sql) -> string {
  let a = "${sql} LIMIT 1"
  let b = "SELECT " + sql
  return a + b
}

pub fn sql_run(db: sqlite.Db, sql: sqlite.Sql) -> number {
  let rows = db.query("${sql} LIMIT 1", [])
  let more = db.query("SELECT " + sql, [])
  return rows.length + more.length
}
"#;
    let root = unique_tmp("g385-sql");
    let src = root.join("src");
    write_file(&src, "main.glyph", text);
    let (code, stdout, stderr) = glyph_in(&root, &["check", "--no-tsc", "--no-test", "{src}"]);
    assert_eq!(code, 0, "a `sqlite.Sql` in text is refused:\n{stdout}\n{stderr}");
    assert!(!stdout.contains("E0237") && !stdout.contains("E0239") && !stderr.contains("E0237") && !stderr.contains("E0239"), "{stdout}\n{stderr}");
}

/// Record 141: the `match` the `unknown` help points at, with an `else` that
/// gives empty text, is accepted now that an `is` arm narrows (Release A).
#[test]
fn an_is_arm_interpolation_after_the_unknown_help_is_accepted() {
    let text = "module main\n\npub fn show(v: unknown) -> string {\n  return match v { is string => \"${v}\", else => \"\" }\n}\n";
    let got = no_text_form_diagnostics("g385-is-arm", text);
    assert!(got.is_empty(), "{got:#?}");
}
