//! A built-in member TypeScript's library marks `@deprecated` draws warning
//! E0244 (G389, decision record 182, section 5b).
//!
//! The 16 string members TypeScript 6.0.3 deprecates (`substr`, `trimLeft`,
//! `trimRight` and the 13 HTML methods) are warned on, read or called,
//! through `?.` too. The message quotes the tag and the help names what to
//! write instead. A warning changes no exit code. Each test checks a real
//! project through the binary, and the help tests write the help's spelling
//! back and build it with `tsc`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_deprecated_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
    dir
}

fn project(prefix: &str, src: &str) -> PathBuf {
    let dir = unique_tmp(prefix);
    std::fs::write(dir.join("src/main.glyph"), src).expect("write module");
    dir
}

fn glyph(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run glyph")
}

/// `check --json` on the project, `tsc` included unless `no_tsc`, with the
/// exit code.
fn check(dir: &Path, no_tsc: bool) -> (Option<i32>, Value) {
    let mut args = vec!["check", "--no-test", "--json"];
    if no_tsc {
        args.push("--no-tsc");
    }
    args.push("src");
    let out = glyph(dir, &args);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let v = serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("check did not print JSON ({e}):\n{text}\n{}", String::from_utf8_lossy(&out.stderr))
    });
    (out.status.code(), v)
}

/// Every diagnostic but the unused-name lint, as `(code, severity, message,
/// help)`.
fn diagnostics(v: &Value) -> Vec<(String, String, String, String)> {
    let s = |d: &Value, k: &str| d[k].as_str().unwrap_or("").to_string();
    v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["code"] != "E0107")
        .map(|d| (s(d, "code"), s(d, "severity"), s(d, "message"), s(d, "help")))
        .collect()
}

/// A module whose `main` runs `stmts` after binding a string `s`, an
/// optional field `o.label` and a `url`.
fn program(stmts: &str) -> String {
    format!(
        "module main\n\
         \n\
         type O = {{ label?: string, }}\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let s = \"  abcdef\"\n\
         \x20 let url = \"https://example.com\"\n\
         \x20 let o: O = {{ label: \"xy\", }}\n\
         \x20 let i = argv.length\n\
         \x20 let n = 2\n\
         {stmts}\n\
         \x20 print(\"${{s}} ${{url}} ${{i}} ${{n}}\")\n\
         \x20 return 0\n\
         }}\n"
    )
}

/// The one diagnostic `stmts` draws without `tsc`, which must be an E0244
/// warning on a run that exits 0, as `(message, help)`.
fn one(stmts: &str) -> (String, String) {
    let (code, v) = check(&project("one", &program(stmts)), true);
    let found = diagnostics(&v);
    assert_eq!(
        found.iter().map(|f| (f.0.as_str(), f.1.as_str())).collect::<Vec<_>>(),
        vec![("E0244", "warning")],
        "exactly one E0244 warning expected:\n{stmts}\n{found:#?}"
    );
    assert_eq!(code, Some(0), "a warning does not fail the check:\n{stmts}");
    let (_, _, m, h) = found.into_iter().next().unwrap();
    (m, h)
}

/// `stmts` draws nothing, and the full check, `tsc` included, passes.
fn assert_clean(label: &str, stmts: &str) {
    let src = program(stmts);
    let (code, v) = check(&project(label, &src), false);
    assert_eq!(code, Some(0), "{label}:\n{src}\n{v:#}");
    assert!(diagnostics(&v).is_empty(), "{label}: expected no diagnostics:\n{src}\n{v:#}");
}

const LEGACY: &str = "A legacy feature for browser compatibility";

/// Record 182's checklist: each warning with its exact text.
#[test]
fn each_deprecated_member_is_warned_with_its_text() {
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "  let a = s.substr(1, 2)\n  print(a)",
            format!("`s.substr` is deprecated: {LEGACY}"),
            "write `s.slice(1, 3)`, which reads the same characters",
        ),
        (
            "  let a = s.substr(2)\n  print(a)",
            format!("`s.substr` is deprecated: {LEGACY}"),
            "write `s.slice(2)`, which reads the same characters",
        ),
        (
            "  let a = s.substr(-3)\n  print(a)",
            format!("`s.substr` is deprecated: {LEGACY}"),
            "write `s.slice(-3)`, which reads the same characters",
        ),
        (
            "  let a = s.substr(i, n)\n  print(a)",
            format!("`s.substr` is deprecated: {LEGACY}"),
            "write `s.slice(i, i + n)`, which reads the same characters when `i` and `n` are zero or more",
        ),
        (
            "  let a = s.substr(-3, 2)\n  print(a)",
            format!("`s.substr` is deprecated: {LEGACY}"),
            "write `slice(start, start + length)` in its place, which reads the same characters when the start \
             and the length are zero or more",
        ),
        (
            "  let a = s.trimLeft()\n  print(a)",
            format!("`s.trimLeft` is deprecated: {LEGACY}. Use `trimStart` instead"),
            "write `s.trimStart()`, the same function under its standard name",
        ),
        (
            "  let a = s.trimRight()\n  print(a)",
            format!("`s.trimRight` is deprecated: {LEGACY}. Use `trimEnd` instead"),
            "write `s.trimEnd()`, the same function under its standard name",
        ),
        (
            "  let a = s.bold()\n  print(a)",
            format!("`s.bold` is deprecated: {LEGACY}"),
            "build the markup as text: `\"<b>${s}</b>\"`",
        ),
        (
            "  let a = s.link(url)\n  print(a)",
            format!("`s.link` is deprecated: {LEGACY}"),
            "build the markup as text: `\"<a href=\\\"${url}\\\">${s}</a>\"`; `link` also writes each `\"` in `url` \
             as `&quot;`",
        ),
        (
            "  let a = \"x\".fontcolor(\"red\")\n  print(a)",
            format!("`fontcolor` is deprecated: {LEGACY}"),
            "build the markup as text: `\"<font color=\\\"${color}\\\">${text}</font>\"`; `fontcolor` also writes \
             each `\"` in `color` as `&quot;`",
        ),
        // An element read by index, typed as E0240 and E0242 type it.
        (
            "  let a = argv[0].trimRight()\n  print(a)",
            format!("`argv[0].trimRight` is deprecated: {LEGACY}. Use `trimEnd` instead"),
            "write `argv[0].trimEnd()`, the same function under its standard name",
        ),
        (
            "  let a = s.fontsize(3)\n  print(a)",
            format!("`s.fontsize` is deprecated: {LEGACY}"),
            "build the markup as text: `\"<font size=\\\"3\\\">${s}</font>\"`",
        ),
        // `?.` on an optional field: the field's `string` is the receiver.
        (
            "  let a = o.label?.substr(1, 2) ?? \"\"\n  print(a)",
            format!("`o.label?.substr` is deprecated: {LEGACY}"),
            "write `o.label?.slice(1, 3)`, which reads the same characters",
        ),
    ];
    for (stmts, msg, help) in cases {
        let (m, h) = one(stmts);
        assert_eq!(m, msg, "{stmts}");
        assert_eq!(h, help, "{stmts}");
    }
}

/// Each help's spelling, written back in place of the call, checks clean
/// with `tsc` and prints what the deprecated call printed.
#[test]
fn each_help_spelling_builds_and_prints_the_same() {
    let pairs = [
        ("s.substr(1, 2)", "s.slice(1, 3)"),
        ("s.substr(i, n)", "s.slice(i, i + n)"),
        ("s.substr(-3)", "s.slice(-3)"),
        ("s.trimLeft()", "s.trimStart()"),
        ("s.trimRight()", "s.trimEnd()"),
        ("s.bold()", "\"<b>${s}</b>\""),
        ("s.link(url)", "\"<a href=\\\"${url}\\\">${s}</a>\""),
        ("s.fontsize(3)", "\"<font size=\\\"3\\\">${s}</font>\""),
        ("o.label?.substr(1, 2) ?? \"\"", "o.label?.slice(1, 3) ?? \"\""),
    ];
    for (old, new) in pairs {
        let before = project("before", &program(&format!("  let a = {old}\n  print(\"[${{a}}]\")")));
        let after_src = program(&format!("  let a = {new}\n  print(\"[${{a}}]\")"));
        assert_clean("after", &format!("  let a = {new}\n  print(\"[${{a}}]\")"));
        let after = project("after", &after_src);
        let run = |dir: &Path| {
            let out = glyph(dir, &["run", "src/main.glyph"]);
            assert!(out.status.success(), "run failed:\n{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        assert_eq!(run(&before), run(&after), "`{old}` and `{new}` print differently");
    }
}

/// A read that is not a call is warned too, beside E0234's refusal of the
/// detached method.
#[test]
fn a_read_alone_is_warned_beside_e0234() {
    let (_, v) = check(&project("read", &program("  let f = s.trimLeft\n  print(\"${f}\")")), true);
    let found = diagnostics(&v);
    let e0244: Vec<_> = found.iter().filter(|d| d.0 == "E0244").collect();
    assert_eq!(e0244.len(), 1, "{found:#?}");
    assert_eq!(e0244[0].1, "warning");
    assert_eq!(e0244[0].3, "write `s.trimStart`, the same function under its standard name");
    assert!(found.iter().any(|d| d.0 == "E0234"), "{found:#?}");
}

/// The members TypeScript does not deprecate draw nothing, `substring` and
/// the standard names of the renamed pair included, and neither does an
/// array or a number method.
#[test]
fn the_current_members_draw_nothing() {
    assert_clean(
        "current",
        "  let a = s.substring(1, 2)\n\
         \x20 let b = s.slice(1, 3)\n\
         \x20 let c = s.trimStart()\n\
         \x20 let d = s.trimEnd()\n\
         \x20 let e = [1, 2].includes(1)\n\
         \x20 let f = n.toFixed(1)\n\
         \x20 print(\"${a} ${b} ${c} ${d} ${e} ${f}\")",
    );
}

/// A warning changes no exit code: `build` succeeds with the warning on
/// stderr, and the human renderer labels it a warning.
#[test]
fn the_build_succeeds_with_the_warning() {
    let dir = project("build", &program("  let a = s.substr(1, 2)\n  print(a)"));
    let out = glyph(&dir, &["build", "--no-test", "--out", "dist", "src"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "build failed:\n{err}");
    assert!(err.contains("E0244"), "the warning is shown:\n{err}");
    assert!(err.to_lowercase().contains("warning"), "labelled a warning:\n{err}");
}

/// `glyph fix` writes nothing for E0244 (record 182: `substr` to `slice`
/// evaluates the start twice, and the renames are a later release's).
#[test]
fn fix_leaves_the_call_as_written() {
    let src = program("  let a = s.trimLeft()\n  print(a)");
    let dir = project("fix", &src);
    let _ = glyph(&dir, &["fix", "src"]);
    assert_eq!(std::fs::read_to_string(dir.join("src/main.glyph")).unwrap(), src);
}
