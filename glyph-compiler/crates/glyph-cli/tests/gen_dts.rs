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

/// `glyph check --no-tsc` on `dir`, as the diagnostic lines it prints (the
/// lines opening with a bracketed code), so a test can count them the way the
/// gap ledger's reproductions do.
fn check_no_tsc(root: &Path, dir: &str) -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(["check", "--no-tsc", dir])
        .current_dir(root)
        .output()
        .expect("run glyph check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    text.lines()
        .filter(|l| l.starts_with("[E"))
        .map(String::from)
        .collect()
}

/// G108. The three-group reproduction from the ledger: a class, a utility
/// type and a host type, each referenced from a record. Before, all three were
/// references to names never written and `check --no-tsc` on the result was
/// three `[E0103]`s. Now the class anchors to the package and the host type to
/// the global, each as an `extern_ts` alias with no descriptor, and the one
/// diagnostic left is the utility type, which `gen` names in its note.
#[test]
fn gen_dts_anchors_a_class_and_a_host_type_and_notes_omit_by_name() {
    let root = project_with_package(
        "extern",
        "export interface Options { gfm: boolean; silent: boolean; }\n\
         export type Trimmed = Omit<Options, \"silent\">;\n\
         export declare class Lexer { constructor(options?: Options); lex(src: string): string[]; }\n\
         export interface Doc { lexer: Lexer; opts: Trimmed; pattern: RegExp; title: string; }\n",
    );
    let Gen::Ok { notes, .. } = gen_dts(&root) else {
        eprintln!("skipping: node/typescript not available");
        return;
    };
    let text = std::fs::read_to_string(root.join("src/types/marky.glyph")).expect("generated file");
    assert!(text.contains("type Lexer = extern_ts(\"import('marky').Lexer\")"), "got:\n{text}");
    assert!(text.contains("type RegExp = extern_ts(\"globalThis.RegExp\")"), "got:\n{text}");
    assert!(text.contains("type Trimmed = Omit<Options, \"silent\">"), "got:\n{text}");

    let unresolved: Vec<&String> = notes.iter().filter(|n| n.contains("could not be resolved")).collect();
    assert_eq!(unresolved.len(), 1, "only the utility type is left; notes: {notes:?}");
    assert!(unresolved[0].contains("`Omit`"), "notes: {notes:?}");

    let diags = check_no_tsc(&root, "src");
    assert_eq!(diags.len(), 1, "exactly one diagnostic, for Omit; got: {diags:?}");
    assert!(
        diags[0].contains("[E0103]") && diags[0].contains("`Omit`"),
        "got: {diags:?}"
    );
}

fn tsc_available() -> bool {
    Command::new("tsc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `glyph build <dir> --out <out>` at `root`: the diagnostic lines it prints
/// and its tail summary lines, so a test can assert on what a user sees.
fn build(root: &Path, dir: &str) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(["build", dir, "--out", "dist"])
        .current_dir(root)
        .output()
        .expect("run glyph build");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (text.contains("tsc --strict passed"), text)
}

/// G209. The declarations `gen dts` writes were not `pub`, so the flow the
/// guide documents (`import types/<pkg> { T }` from another module) was
/// `[E0105] T is not exported` against every generated file, and had been
/// since the generator existed. This is the confirmation the G108 entry had to
/// run on a hand-patched copy, as a test: generate, import the generated module
/// from `main.glyph`, construct the class with `new`, call a method on the
/// field, and pass `glyph build` with `tsc`. The `extern_ts` anchors are `pub`
/// too, so the importing module can name them in an annotation.
#[test]
fn gen_dts_module_imports_and_the_class_field_typechecks_through_new() {
    let root = project_with_package(
        "import",
        "export interface Options { gfm: boolean; silent: boolean; }\n\
         export declare class Lexer { constructor(options?: Options); lex(src: string): string[]; }\n\
         export interface Doc { lexer: Lexer; pattern: RegExp; title: string; }\n",
    );
    let Gen::Ok { .. } = gen_dts(&root) else {
        eprintln!("skipping: node/typescript not available");
        return;
    };
    let text = std::fs::read_to_string(root.join("src/types/marky.glyph")).expect("generated file");
    assert!(text.contains("pub type Doc ="), "the record is exported; got:\n{text}");
    assert!(text.contains("pub type Options ="), "got:\n{text}");
    assert!(
        text.contains("pub type Lexer = extern_ts(") && text.contains("pub type RegExp = extern_ts("),
        "the anchors are exported too; got:\n{text}"
    );

    write_file(
        &root,
        "src/main.glyph",
        r#"module main

import marky { Lexer }
import types/marky { Doc, RegExp }

fn main(argv: Array<string>) -> number {
  let lx = new Lexer()
  let re: RegExp = extern_ts("/a+/")
  let d: Doc = { lexer: lx, pattern: re, title: "t" }
  let toks = d.lexer.lex("a b")
  print(toks[0])
  return 0
}
"#,
    );
    let diags = check_no_tsc(&root, "src");
    assert!(diags.is_empty(), "the generated module imports as written; got: {diags:?}");

    if !tsc_available() {
        eprintln!("skipping the tsc half: tsc not available");
        return;
    }
    let (passed, out) = build(&root, "src");
    assert!(passed, "new Lexer() into the record and a method call on the field:\n{out}");

    // The class is real to `tsc`: a misspelt method is an error mapped to the
    // Glyph source, not a silent `any`.
    let main = root.join("src/main.glyph");
    let wrong = std::fs::read_to_string(&main).unwrap().replace("d.lexer.lex(", "d.lexer.lexx(");
    std::fs::write(&main, wrong).unwrap();
    let (passed, out) = build(&root, "src");
    assert!(!passed, "a wrong method name must fail tsc:\n{out}");
    assert!(
        out.contains("Property 'lexx' does not exist on type 'Lexer'") && out.contains("main:"),
        "mapped to the Glyph source:\n{out}"
    );
}
