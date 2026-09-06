//! End-to-end tests for `glyph gen dts` against an installed package.
//!
//! Each test stages a project with a fake package under `node_modules`, runs
//! the real `glyph` binary from the project root (package resolution reads
//! `node_modules` from the current directory, so this is the one place a
//! subprocess is needed), and then checks the generated module the way a user
//! would, with `glyph check --no-tsc` and, where `tsc` is available, a full
//! build.
//!
//! `gen dts` shells out to `node` and the `typescript` package. Where either is
//! absent or is the 7.x native port the helper cannot load, the command reports
//! that cleanly and the test is skipped, the same contract the unit tests in
//! `gen.rs` hold.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_gen_dts_test_{prefix}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_file(dir: &Path, relpath: &str, text: &str) {
    let p = dir.join(relpath);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(&p, text).expect("write file");
}

/// A project root with a fake installed package `marky` whose declarations are
/// `dts`. The `glyph` key marks the resolution root, as `glyph init` writes it.
fn project_with_package(prefix: &str, dts: &str) -> PathBuf {
    let root = unique_tmp(prefix);
    write_file(&root, "package.json", r#"{ "name": "proj", "version": "0.0.0", "glyph": {} }"#);
    write_file(
        &root,
        "node_modules/marky/package.json",
        r#"{ "name": "marky", "version": "1.0.0", "types": "index.d.ts", "main": "index.js" }"#,
    );
    write_file(&root, "node_modules/marky/index.d.ts", dts);
    write_file(
        &root,
        "node_modules/marky/index.js",
        "export class Lexer { lex(s) { return s.split(\" \"); } }\n",
    );
    root
}

/// The outcome of `glyph gen dts marky --out src/types` run at `root`.
enum Gen {
    /// The command succeeded; the notes it printed, in order.
    Ok { notes: Vec<String>, summary: String },
    /// `node` or a usable `typescript` is not available here.
    ToolchainMissing,
}

fn gen_dts(root: &Path) -> Gen {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(["gen", "dts", "marky", "--out", "src/types"])
        .current_dir(root)
        .output()
        .expect("run glyph gen dts");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        if stderr.contains("`node` not found")
            || stderr.contains("`typescript` package is not resolvable")
            || stderr.contains("compiler API could not be loaded")
        {
            return Gen::ToolchainMissing;
        }
        panic!("glyph gen dts failed:\n{stderr}");
    }
    let notes = stderr
        .lines()
        .filter_map(|l| l.strip_prefix("glyph gen: note: "))
        .map(String::from)
        .collect();
    let summary = stderr
        .lines()
        .find(|l| l.contains("type(s) written"))
        .unwrap_or("")
        .to_string();
    Gen::Ok { notes, summary }
}

/// G208. `gen dts` used to drop every member of an interface that was not a
/// property, with no note: `Client` below materialized as `{ url: string }`
/// and the run reported `1 type(s) written` and nothing else. A user
/// materializing a method-bearing API lost its whole method surface and was
/// told nothing. Now each dropped member is a note naming the owner, the
/// member and the reason, surfaced the same way an unresolvable reference is.
#[test]
fn gen_dts_notes_each_method_signature_it_drops() {
    let root = project_with_package(
        "methods",
        "export interface Client { url: string; fetch(path: string): Promise<string>; close(): void; }\n",
    );
    let Gen::Ok { notes, summary } = gen_dts(&root) else {
        eprintln!("skipping: node/typescript not available");
        return;
    };
    let text = std::fs::read_to_string(root.join("src/types/marky.glyph")).expect("generated file");
    assert!(text.contains("type Client = { url: string }"), "got:\n{text}");
    assert!(!text.contains("fetch"), "a method has no wire shape and is not a field:\n{text}");

    let dropped: Vec<&String> = notes.iter().filter(|n| n.contains("no wire shape")).collect();
    assert_eq!(dropped.len(), 2, "one note per dropped member; notes: {notes:?}");
    assert!(
        dropped.iter().any(|n| n.contains("`Client.fetch`") && n.contains("method signature")),
        "notes: {notes:?}"
    );
    assert!(dropped.iter().any(|n| n.contains("`Client.close`")), "notes: {notes:?}");
    assert!(
        summary.contains("1 type(s) written") && summary.contains("note(s)"),
        "the summary line counts the notes so they cannot pass unseen: {summary}"
    );
}
