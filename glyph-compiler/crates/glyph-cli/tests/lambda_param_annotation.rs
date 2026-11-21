//! A lambda parameter's type is the one written (record 130, Release A).
//!
//! Until this release an omitted lambda annotation and a written `unknown`
//! were the same AST node, and the formatter and emitter read both as
//! "omitted": `fn(p: unknown) -> number { return p + 1 }` was reprinted as
//! `fn(p)`, emitted as `(p)`, and `tsc` inferred `number` for a parameter the
//! program declared `unknown`, so full `glyph check` passed. Now the written
//! annotation is emitted and `tsc` refuses the body, as it refuses the same
//! function written directly in TypeScript. The omitted form is unchanged.

use glyph_cli::test_support::tsc_available;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn stage(name: &str, main: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "glyph_lambda_param_{name}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(
        root.join("package.json"),
        format!("{{ \"name\": \"{name}\", \"version\": \"0.0.0\", \"glyph\": {{}} }}\n"),
    )
    .expect("write package.json");
    std::fs::write(root.join("src").join("main.glyph"), main).expect("write module");
    root
}

fn run_check(root: &Path, extra: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .arg("check")
        .arg(".")
        .args(extra)
        .current_dir(root)
        .output()
        .expect("run glyph check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

const WRITTEN: &str = "module main\n\nimport std/array\n\npub fn f(xs: Array<number>) -> Array<number> {\n  return array.map(xs, fn(p: unknown) -> number { return p + 1 })\n}\n";

const OMITTED: &str = "module main\n\nimport std/array\n\npub fn f(xs: Array<number>) -> Array<number> {\n  return array.map(xs, fn(p) -> number { return p + 1 })\n}\n";

/// The written `unknown` reaches `tsc`, which refuses `p + 1` on it. 0.1.153
/// passed this program: the annotation never reached the emitted file.
#[test]
fn a_written_unknown_lambda_param_is_checked_by_tsc() {
    if !tsc_available() {
        eprintln!("skipping: tsc is not on the PATH");
        return;
    }
    let root = stage("written", WRITTEN);
    let (ok, text) = run_check(&root, &["--no-test"]);
    assert!(!ok, "full check must fail on a written `unknown`:\n{text}");
    assert!(text.contains("TS18046"), "expected tsc's TS18046:\n{text}");
}

/// The omitted form still emits bare and `tsc` infers `number` from
/// `array.map`'s signature, so the same body passes.
#[test]
fn an_omitted_lambda_annotation_still_passes_full_check() {
    if !tsc_available() {
        eprintln!("skipping: tsc is not on the PATH");
        return;
    }
    let root = stage("omitted", OMITTED);
    let (ok, text) = run_check(&root, &["--no-test"]);
    assert!(ok, "full check must pass on an omitted annotation:\n{text}");
}
