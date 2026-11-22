//! `??` on an `Option` or a `Result` is E0236 (G429, decision record 157).
//!
//! An `Option` and a `Result` are tagged objects at run time, so `??` never
//! replaces one. A left operand that cannot be absent is refused whatever the
//! default is; one that can (a `?.` chain, a field declared `f?:`, an index
//! read) is refused unless the default is the container, and then the `??` is
//! typed as the container. Each test checks a real project through the
//! binary; the help test builds and runs every help form, and the fix tests
//! run `glyph fix` and check the file again.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_coalesce_{prefix}_{}_{}", std::process::id(), n);
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

/// `check --json` on the project, `tsc` included unless `no_tsc`.
fn check(dir: &Path, no_tsc: bool) -> Value {
    let mut args = vec!["check", "--no-test", "--json"];
    if no_tsc {
        args.push("--no-tsc");
    }
    args.push("src");
    let out = glyph(dir, &args);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("check did not print JSON ({e}):\n{text}\n{}", String::from_utf8_lossy(&out.stderr))
    })
}

fn errors(v: &Value) -> Vec<Value> {
    v["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["severity"] == "error")
        .cloned()
        .collect()
}

/// One refusal: its line, code, message and help.
#[derive(Debug, Clone)]
struct Refusal {
    line: u64,
    code: String,
    message: String,
    help: String,
}

fn refusals_in(dir: &Path) -> Vec<Refusal> {
    errors(&check(dir, true))
        .iter()
        .map(|d| Refusal {
            line: d["range"]["start"]["line"].as_u64().unwrap_or(0),
            code: d["code"].as_str().unwrap_or("").to_string(),
            message: d["message"].as_str().unwrap_or("").to_string(),
            help: d["help"].as_str().unwrap_or("").to_string(),
        })
        .collect()
}

/// Every error in `src`, checked without `tsc`.
fn refusals(src: &str) -> Vec<Refusal> {
    refusals_in(&project("r", src))
}

/// A module with a few declarations and a `main` whose body is `stmts`.
fn program(stmts: &str) -> String {
    format!(
        "module main\n\
         \n\
         import std/array\n\
         \n\
         type R = {{ opt: Option<number>, }}\n\
         type Box = {{ n?: number, inner?: R, o?: Option<number>, }}\n\
         type Maybe = Option<number>\n\
         \n\
         fn first(xs: Array<number>) -> Option<number> {{\n\
         \x20 return array.get(xs, 0)\n\
         }}\n\
         \n\
         fn get_r(s: string) -> Result<number, string> {{\n\
         \x20 return Ok(1)\n\
         }}\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let xs = [3, 1, 2]\n\
         \x20 let u = array.find(xs, fn(x: number) -> bool {{ return x > 9 }})\n\
         \x20 let b: Box = {{ inner: {{ opt: Some(2), }}, }}\n\
         \x20 let opts: Array<Option<number>> = [Some(1)]\n\
         {stmts}\n\
         \x20 print(\"${{xs.length}} ${{opts.length}}\")\n\
         \x20 return 0\n\
         }}\n"
    )
}

/// The one error `stmts` draws, which must be an E0236: (message, help).
fn one(stmts: &str) -> (String, String) {
    let src = program(stmts);
    let found = refusals(&src);
    assert_eq!(
        found.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(),
        vec!["E0236"],
        "exactly one E0236 expected:\n{src}\n{found:#?}"
    );
    let r = found.into_iter().next().unwrap();
    (r.message, r.help)
}

/// `stmts` draws no error at all.
fn none(stmts: &str) {
    let src = program(stmts);
    let found = refusals(&src);
    assert!(found.is_empty(), "expected no errors:\n{src}\n{found:#?}");
}

/// Record 157's probe a01: the six `??` it lists that hand back a tag
/// object, and the three it lists that must stay accepted.
const A01: &str = "module main

import std/io
import std/array

type R = { opt: Option<number>, }
type Box = { n?: number, inner?: R, }
type Maybe = Option<number>

fn get_r(s: string) -> Result<number, string> {
  return Ok(1)
}

fn pick<T>(x: T, d: T) -> T {
  return x ?? d
}

fn nn(n: Nullable<R>) -> number {
  let v9 = n?.opt ?? 0
  io.println(\"${v9}\")
  return 0
}

fn main(argv: Array<string>) -> number {
  let xs = [3, 1, 2]
  let u = array.find(xs, fn(x: number) -> bool { return x > 9 })
  let v = u ?? 0
  let v2 = u ?? None
  let v3 = u ?? Some(4)
  let m: Maybe = None
  let v4 = m ?? 1
  let r = get_r(\"x\")
  let v5 = r ?? 0
  let b: Box = { inner: { opt: Some(2), }, }
  let v6 = b.inner?.opt ?? 0
  let v7 = b.inner?.opt ?? None
  let v8 = b.n ?? 0
  let oo: Option<Option<number>> = Some(None)
  let v10 = oo ?? 0
  let v11 = pick(u, None)
  let k = 5
  let v12 = k ?? 0
  io.println(\"${v} ${v4} ${v5} ${v6} ${v8} ${v12}\")
  return 0
}
";

/// Checklist 2: each a01 line. Every `??` on an `Option` or `Result` is one
/// E0236 at its line; `b.n ?? 0`, `x ?? d` on a `T`, `k ?? 0` on a `number`
/// and the accepted `b.inner?.opt ?? None` draw nothing.
#[test]
fn each_line_of_the_a01_probe() {
    let found = refusals(A01);
    let lines: Vec<(u64, &str)> = found.iter().map(|r| (r.line, r.code.as_str())).collect();
    assert_eq!(
        lines,
        vec![
            (19, "E0236"),
            (27, "E0236"),
            (28, "E0236"),
            (29, "E0236"),
            (31, "E0236"),
            (33, "E0236"),
            (35, "E0236"),
            (39, "E0236"),
        ],
        "{found:#?}"
    );
    let at = |line: u64| found.iter().find(|r| r.line == line).unwrap();
    assert_eq!(
        at(27).message,
        "`??` on an `Option<number>`: an `Option` is never null or undefined, so `?? 0` never applies and the value is the `Option` itself"
    );
    assert_eq!(
        at(27).help,
        "Open it with `match u { Some(n) => n, None => 0 }`. If the absence is a JSON `null` from the wire, declare the type `Nullable<number>` (D45)."
    );
    // The alias is read as the `Option` it names.
    assert!(at(31).message.starts_with("`??` on an `Option<number>`"), "{:?}", at(31));
    assert_eq!(
        at(33).message,
        "`??` on a `Result<number, string>`: a `Result` is never null or undefined, so `?? 0` never applies and the value is the `Result` itself"
    );
    assert_eq!(at(33).help, "Open it with `match r { Ok(n) => n, Err(_) => 0 }`.");
    assert_eq!(
        at(35).message,
        "`b.inner?.opt` is an `Option<number>` when present: `?? 0` replaces only its absence, so a present `b.inner?.opt` hands back the `Option`, not its value"
    );
    assert_eq!(
        at(35).help,
        "Open it with `match b.inner?.opt ?? None { Some(n) => n, None => 0 }`. If the absence is a JSON `null` from the wire, declare the type `Nullable<number>` (D45)."
    );
    // A `?.` over a `Nullable` is a chain that can be absent too, and the
    // payload's name does not shadow the `n` the `match` reads.
    assert_eq!(
        at(19).help,
        "Open it with `match n?.opt ?? None { Some(v) => v, None => 0 }`. If the absence is a JSON `null` from the wire, declare the type `Nullable<number>` (D45)."
    );
    // A default that is itself an `Option` never applies either.
    assert_eq!(at(28).help, "Write `u` alone; it is already the `Option`, and `?? None` never applies.");
    assert_eq!(
        at(29).help,
        "To replace a `None` with `Some(4)`, write `match u { Some(n) => Some(n), None => Some(4) }`; otherwise write `u` alone."
    );
    // A nested `Option` gets no `Nullable` pointer.
    assert_eq!(at(39).help, "Open it with `match oo { Some(o) => o, None => 0 }`.");
}

/// Checklist 9: a01 is exit 0 on 0.1.161 under both checks; on this branch
/// it is exit 1 under both, so the release is breaking under full `check`.
#[test]
fn the_a01_probe_fails_both_checks() {
    let dir = project("a01", A01);
    for args in [&["check", "--no-test", "--no-tsc", "src"][..], &["check", "--no-test", "src"][..]] {
        let out = glyph(&dir, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}:\n{}", String::from_utf8_lossy(&out.stdout));
    }
}

/// An `Option` out of a call, and a `match` that is one, cannot be absent.
#[test]
fn an_option_from_a_call_or_a_match_is_refused() {
    let (m, h) = one("  let c = first(xs) ?? 0\n  print(\"${c}\")");
    assert!(m.starts_with("`??` on an `Option<number>`"), "{m}");
    assert!(h.contains("`match first(xs) { Some(n) => n, None => 0 }`"), "{h}");
    let (m, _) = one(
        "  let ok = xs.length > 1\n  let c = match ok { true => first(xs), false => first(xs) } ?? 0\n  print(\"${c}\")",
    );
    assert!(m.starts_with("`??` on an `Option<number>`"), "{m}");
}

/// An `Array` index read is `(a)` (record 159): Glyph's emitted index
/// throws a `RangeError` out of range and never gives `undefined`, so
/// neither `?? None` nor `?? 0` ever applies.
#[test]
fn an_index_read_cannot_be_absent() {
    let (m, h) = one("  let i = opts[0] ?? None\n  let j = match i { Some(n) => n, None => 0 }\n  print(\"${j}\")");
    assert!(m.starts_with("`??` on an `Option<number>`"), "{m}");
    assert_eq!(h, "Write `opts[0]` alone; it is already the `Option`, and `?? None` never applies.");
    let (m, h) = one("  let i = opts[0] ?? 0\n  print(\"${i}\")");
    assert!(m.starts_with("`??` on an `Option<number>`"), "{m}");
    assert!(h.contains("`match opts[0] { Some(n) => n, None => 0 }`"), "{h}");
}

/// A type parameter is named as the program wrote it, never `?` (F1).
#[test]
fn a_type_parameter_is_shown_by_its_name() {
    let src = "module main\n\
        \n\
        fn gen<T>(o: Option<T>, d: T) -> T {\n\
        \x20 return o ?? d\n\
        }\n";
    let found = refusals(src);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].message.starts_with("`??` on an `Option<T>`"), "{found:#?}");
    assert!(found[0].help.contains("`Nullable<T>`"), "{found:#?}");
}

/// A default whose type the checker does not know gets the value form,
/// with the default written in the `None` arm, never the container form
/// (F2). `u ?? w ?? 0` is `u ?? (w ?? 0)`, and the inner `??` is refused,
/// so the outer default is untyped.
#[test]
fn an_untyped_default_gets_the_value_form() {
    let src = program("  let w = first(xs)\n  let n = u ?? w ?? 0\n  print(\"${n}\")");
    let found = refusals(&src);
    assert_eq!(found.len(), 2, "{src}\n{found:#?}");
    let outer = found.iter().find(|r| r.message.contains("`?? w ?? 0`")).expect("the outer `??`");
    assert!(outer.help.starts_with("Open it with `match u { Some(n) => n, None => w ?? 0 }`"), "{outer:#?}");
}

/// Record 159, B1: inside a `${}` the rewrite would write a `match` into
/// the string's text. Both rules decline there and the help hoists the
/// `match` into a `let`; the file is unchanged, and the `??` outside the
/// string beside them is still rewritten.
#[test]
fn fix_declines_inside_an_interpolation_for_both_rules() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/array\n\
        \n\
        fn id(n: number) -> number {\n\
        \x20 return n\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let u = array.find(xs, fn(x: number) -> bool { return x > 9 })\n\
        \x20 io.println(\"t=${u ?? 0}\")\n\
        \x20 let s = \"${u ?? 4} and\"\n\
        \x20 io.println(\"t=${xs.find(fn(x: number) -> bool { return x > 9 }) ?? 0}\")\n\
        \x20 let a = id(u ?? 1)\n\
        \x20 io.println(\"${a} ${s}\")\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    let inside: Vec<&Refusal> = found.iter().filter(|r| r.line == 13 || r.line == 14).collect();
    assert_eq!(inside.len(), 2, "{found:#?}");
    for r in inside {
        assert!(r.help.starts_with("Bind it before the string, `let t = match u { Some(n) => n, None => "), "{r:#?}");
        assert!(r.help.contains("`, then interpolate `${t}`"), "{r:#?}");
    }
    let call = found.iter().find(|r| r.code == "E0242").expect("the E0242");
    assert!(call.help.contains("inside a `${}`, bind the `match` before the string with `let t =`, then interpolate `${t}`"), "{call:#?}");
    let (dir, fixed, out) = fix(src);
    assert_eq!(out.matches("declined E0236").count(), 2, "{out}");
    assert!(out.contains("declined E0242") && out.contains("the call is inside a `${}`"), "{out}");
    assert!(out.contains("applied E0236") && out.matches("applied").count() == 1, "{out}");
    assert!(fixed.contains("  io.println(\"t=${u ?? 0}\")\n  let s = \"${u ?? 4} and\"\n"), "{fixed}");
    assert!(fixed.contains("xs.find(fn(x: number) -> bool { return x > 9 }) ?? 0}"), "{fixed}");
    assert!(fixed.contains("  let a = id(match u {\n    Some(n) => n,\n    None => 1,\n  })\n"), "{fixed}");
    // What is left is the refusals inside the strings, and nothing new.
    let left = refusals_in(&dir);
    assert_eq!(left.len(), 3, "{left:#?}");
}

/// Record 159, f07, and G435: the E0210 range inside a `${}` is the
/// member read itself, as it is outside a string, so the read fix renames
/// `nik` to `nick` and the file checks clean.
#[test]
fn a_field_typo_inside_an_interpolation_is_renamed() {
    let src = "module main\n\
        \n\
        import std/io\n\
        \n\
        type B = { nick: string, }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let b: B = { nick: \"a\", }\n\
        \x20 io.println(\"n=${b.nik} end\")\n\
        \x20 return 0\n\
        }\n";
    let dir = project("typo_range", src);
    let found = errors(&check(&dir, true));
    assert_eq!(found.len(), 1, "{found:#?}");
    let range = &found[0]["range"];
    let (s, e) = (range["start"]["offset"].as_u64().unwrap(), range["end"]["offset"].as_u64().unwrap());
    assert_eq!(&src[s as usize..e as usize], "b.nik", "{found:#?}");
    assert_eq!(range["start"]["col"], 19, "{found:#?}");
    let (dir, fixed, out) = fix(src);
    assert!(fixed.contains("io.println(\"n=${b.nick} end\")"), "{fixed}");
    assert!(out.contains("applied E0210"), "{out}");
    assert_fix_settles(&dir, &fixed);
}

/// Record 160, x03: on 0.1.161 the read rename in a second interpolation
/// wrote `b.nickc`, because the range inside a `${}` was one column early.
/// With the range on `b.nikc` the rule judges the name written there, which
/// is a transposition and not one character away, so it declines for that
/// reason, as it does outside a string.
#[test]
fn a_field_read_rename_in_a_second_interpolation_reads_the_right_name() {
    let src = "module main\n\
        \n\
        import std/io\n\
        \n\
        type B = { nick: string, name: string, }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let b: B = { nick: \"a\", name: \"c\", }\n\
        \x20 io.println(\"v=${b.name} w=${b.nikc}\")\n\
        \x20 return 0\n\
        }\n";
    let (_, fixed, out) = fix(src);
    assert!(fixed.contains("io.println(\"v=${b.name} w=${b.nikc}\")"), "{fixed}");
    assert!(out.contains("declined E0210") && !out.contains("applied"), "{out}");
    assert!(out.contains("nothing the type declares is one character away from `nikc`"), "{out}");
}

/// G435: a one-character typo in a later interpolation is renamed in place,
/// after an escaped quote, a `\${`, non-ASCII text and a line break earlier
/// in the string.
#[test]
fn a_field_typo_after_escapes_and_non_ascii_text_is_renamed_in_place() {
    let src = "module main\n\
        \n\
        import std/io\n\
        \n\
        type B = { nick: string, name: string, }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let b: B = { nick: \"a\", name: \"c\", }\n\
        \x20 io.println(\"v=${b.name} \\\" \\${b} \u{e9}\u{1F600}\nw=${b.nik} ${b.nme}\")\n\
        \x20 return 0\n\
        }\n";
    let (dir, fixed, out) = fix(src);
    assert_eq!(out.matches("applied E0210").count(), 2, "{out}");
    assert!(fixed.contains("\"v=${b.name} \\\" \\${b} \u{e9}\u{1F600}\nw=${b.nick} ${b.name}\""), "{fixed}");
    assert_fix_settles(&dir, &fixed);
}

/// F3: a revert names the line in the file as `glyph fix` leaves it, after
/// the rewrites it kept above the error.
#[test]
fn a_revert_names_the_line_in_the_file_as_written() {
    let src = "module main\n\
        \n\
        import std/array\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let u = array.find(xs, fn(x: number) -> bool { return x > 9 })\n\
        \x20 let v = u ?? 0\n\
        \x20 let bad = u ?? 1\n\
        \x20 let s: string = bad\n\
        \x20 print(\"${v} ${s}\")\n\
        \x20 return 0\n\
        }\n";
    let (_, fixed, out) = fix(src);
    let line = fixed.lines().position(|l| l == "  let s: string = bad").expect("the line") + 1;
    assert_eq!(line, 13, "{fixed}");
    assert!(out.contains("expected `string`, found `number` (main.glyph:13), so it was reverted"), "{out}");
}

/// `b.o ?? None` is typed `Option<number>`, so the `match` the help writes
/// is held to its arms: a missing `None` arm is E0200 (probe a04).
#[test]
fn the_accepted_form_is_checked_for_exhaustiveness() {
    none("  let a = b.o ?? None\n  let j = match a { Some(n) => n, None => 0 }\n  print(\"${j}\")");
    let src = program("  let a = match b.o ?? None {\n    Some(v) => v,\n  }\n  print(\"${a}\")");
    let found = refusals(&src);
    assert_eq!(found.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), vec!["E0200"], "{found:#?}");
    let (m, _) = one("  let a = b.o ?? 0\n  print(\"${a}\")");
    assert!(m.starts_with("`b.o` is an `Option<number>` when present"), "{m}");
}

/// A JSX child is a position like any other.
#[test]
fn a_jsx_child_is_refused() {
    let src = "module main\n\
        \n\
        component Label(props: { u: Option<string>, }) {\n\
        \x20 return <span>{props.u ?? \"none\"}</span>\n\
        }\n";
    let found = refusals(src);
    assert_eq!(found.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), vec!["E0236"], "{found:#?}");
    assert!(found[0].help.contains("`match props.u { Some(s) => s, None => \"none\" }`"), "{found:#?}");
}

/// Not judged: a type parameter, a `Nullable`, a value the checker does not
/// type, an optional `number` field, and a plain `number`.
#[test]
fn what_is_not_judged_stays_accepted() {
    let src = "module main\n\
        \n\
        type Box = { n?: number, }\n\
        \n\
        fn pick<T>(x: T, d: T) -> T {\n\
        \x20 return x ?? d\n\
        }\n\
        \n\
        fn nul(n: Nullable<number>) -> number {\n\
        \x20 return n ?? 0\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let b: Box = { n: 1, }\n\
        \x20 let v = json.parse(\"1\")\n\
        \x20 let raw = match v { Ok(x) => x, Err(_) => 0, }\n\
        \x20 let w = raw ?? 0\n\
        \x20 let k = 5\n\
        \x20 print(\"${pick(1, 2)} ${nul(null)} ${b.n ?? 0} ${k ?? 0} ${w}\")\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert!(found.iter().all(|f| f.code != "E0236"), "{found:#?}");
}

/// A `??` whose left side is an E0242 call draws E0242 only.
#[test]
fn a_find_call_on_the_left_is_e0242_only() {
    let src = program("  let h = xs.find(fn(x: number) -> bool { return x > 9 }) ?? 0\n  print(\"${h}\")");
    let found = refusals(&src);
    assert_eq!(found.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), vec!["E0242"], "{found:#?}");
}

/// The help offers `r?` only inside a callable that returns a `Result`.
#[test]
fn propagation_is_offered_only_under_a_result_return() {
    let src = "module main\n\
        \n\
        fn get_r(s: string) -> Result<number, string> {\n\
        \x20 return Ok(1)\n\
        }\n\
        \n\
        fn twice(s: string) -> Result<number, string> {\n\
        \x20 let r = get_r(s)\n\
        \x20 let n = r ?? 0\n\
        \x20 return Ok(n)\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let r = get_r(\"x\")\n\
        \x20 let n = r ?? 0\n\
        \x20 print(\"${n}\")\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert_eq!(found[0].help, "Open it with `match r { Ok(n) => n, Err(_) => 0 }`, or propagate it with `r?`.");
    assert_eq!(found[1].help, "Open it with `match r { Ok(n) => n, Err(_) => 0 }`.");
}

/// A sibling's `Option` and `Result` aliases are the prelude's under every
/// import spelling, so the refusal does not depend on where the alias lives.
#[test]
fn an_imported_alias_is_refused_by_every_spelling() {
    for (import, q) in [("import a { MaybeNum, Res }", ""), ("import a", "a."), ("import a as aa", "aa.")] {
        let dir = unique_tmp("alias");
        std::fs::write(
            dir.join("src/a.glyph"),
            "module a\n\npub type MaybeNum = Option<number>\n\npub type Res<T> = Result<T, string>\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.glyph"),
            format!(
                "module main\n\n{import}\n\n\
                 pub fn opt(o: {q}MaybeNum) -> number {{\n  let x = o ?? 0\n  print(\"${{x}}\")\n  return 0\n}}\n\n\
                 pub fn res(r: {q}Res<number>) -> number {{\n  let x = r ?? 0\n  print(\"${{x}}\")\n  return 0\n}}\n"
            ),
        )
        .unwrap();
        let found = refusals_in(&dir);
        assert_eq!(found.len(), 2, "`{import}`: {found:#?}");
        assert!(found[0].message.starts_with("`??` on an `Option<number>`"), "`{import}`: {found:#?}");
        assert!(found[1].message.starts_with("`??` on a `Result<number, string>`"), "`{import}`: {found:#?}");
    }
}

/// Checklist 3: every help form, applied, passes the full check and runs.
#[test]
fn every_help_applied_compiles_and_runs() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/array\n\
        \n\
        type R = { opt: Option<number>, }\n\
        type Box = { n?: number, inner?: R, o?: Option<number>, }\n\
        type Maybe = Option<number>\n\
        \n\
        fn get_r(s: string) -> Result<number, string> {\n\
        \x20 return Ok(1)\n\
        }\n\
        \n\
        fn twice(s: string) -> Result<number, string> {\n\
        \x20 let r = get_r(s)\n\
        \x20 let n = r?\n\
        \x20 return Ok(n * 2)\n\
        }\n\
        \n\
        fn nn(n: Nullable<R>) -> number {\n\
        \x20 return match n?.opt ?? None { Some(v) => v, None => 0 }\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let u = array.find(xs, fn(x: number) -> bool { return x > 9 })\n\
        \x20 let v = match u { Some(n) => n, None => 0 }\n\
        \x20 let v2 = u\n\
        \x20 let v3 = match u { Some(n) => Some(n), None => Some(4) }\n\
        \x20 let m: Maybe = None\n\
        \x20 let v4 = match m { Some(n) => n, None => 1 }\n\
        \x20 let r = get_r(\"x\")\n\
        \x20 let v5 = match r { Ok(n) => n, Err(_) => 0 }\n\
        \x20 let v5b = match r { Ok(n) => Ok(n), Err(_) => Ok(0) }\n\
        \x20 let b: Box = { inner: { opt: Some(2), }, }\n\
        \x20 let v6 = match b.inner?.opt ?? None { Some(n) => n, None => 0 }\n\
        \x20 let v7 = match b.o ?? None { Some(n) => n, None => 0 }\n\
        \x20 let oo: Option<Option<number>> = Some(None)\n\
        \x20 let v10 = match oo { Some(o) => o, None => 0 }\n\
        \x20 let opts: Array<Option<number>> = [Some(1)]\n\
        \x20 let v11 = match opts[0] { Some(n) => n, None => 0 }\n\
        \x20 let w = match v3 { Some(n) => n, None => 0 }\n\
        \x20 let w2 = match v5b { Ok(n) => n, Err(_) => 0 }\n\
        \x20 let w3 = match v2 { Some(n) => n, None => 0 }\n\
        \x20 let t = match twice(\"x\") { Ok(n) => n, Err(_) => 0 }\n\
        \x20 io.println(\"${v} ${w3} ${w} ${v4} ${v5} ${w2} ${v6} ${v7} ${v11} ${nn({ opt: Some(5), })} ${array.len([v10])} ${t}\")\n\
        \x20 return 0\n\
        }\n";
    let dir = project("help", src);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{v:#}");
    let out = glyph(&dir, &["run", "src/main.glyph"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("0 0 4 1 1 1 2 0 1 5 1 2"),
        "unexpected output:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Run `glyph fix` on `src`, laid out by `glyph fmt` first so a later `fmt`
/// can only be reacting to the fix; the fixed file and the command's output.
fn fix(src: &str) -> (PathBuf, String, String) {
    let dir = project("fix", src);
    glyph(&dir, &["fmt", "src"]);
    let out = glyph(&dir, &["fix", "src"]);
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let fixed = std::fs::read_to_string(dir.join("src/main.glyph")).expect("read fixed file");
    (dir, fixed, text)
}

/// The fixed file passes the full check, a second `fix` changes nothing, and
/// `fmt` leaves it alone.
fn assert_fix_settles(dir: &Path, fixed: &str) {
    let v = check(dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "the fixed file does not check:\n{fixed}\n{v:#}");
    glyph(dir, &["fix", "src"]);
    let again = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
    assert_eq!(again, fixed, "a second `glyph fix` changed the file");
    glyph(dir, &["fmt", "src"]);
    let formatted = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
    assert_eq!(formatted, fixed, "`glyph fmt` changed the fixed file");
}

/// Checklist 4: the fix writes the `match` for an `Option` that cannot be
/// absent and for one that can, says what changes, and settles.
#[test]
fn fix_writes_the_match_and_says_what_changes() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/array\n\
        \n\
        type R = { opt: Option<number>, }\n\
        type Box = { inner?: R, }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let u = array.find(xs, fn(x: number) -> bool { return x > 9 })\n\
        \x20 let v = u ?? 7\n\
        \x20 let b: Box = { inner: { opt: Some(2), }, }\n\
        \x20 let w = b.inner?.opt ?? 0\n\
        \x20 io.println(\"${v} ${w}\")\n\
        \x20 return 0\n\
        }\n";
    let (dir, fixed, out) = fix(src);
    assert!(
        out.contains("applied E0236 in src/main.glyph: `u ?? 7` handed back the `Option`; it now gives its value or `7`"),
        "{out}"
    );
    assert!(
        out.contains(
            "applied E0236 in src/main.glyph: `b.inner?.opt ?? 0` handed back the `Option` when present; it now gives its value or `0`"
        ),
        "{out}"
    );
    assert!(fixed.contains("  let v = match u {\n    Some(n) => n,\n    None => 7,\n  }\n"), "{fixed}");
    assert!(fixed.contains("  let w = match b.inner?.opt ?? None {\n    Some(n) => n,\n    None => 0,\n  }\n"), "{fixed}");
    assert_fix_settles(&dir, &fixed);
    let run = glyph(&dir, &["run", "src/main.glyph"]);
    assert!(String::from_utf8_lossy(&run.stdout).contains("7 2"), "{}", String::from_utf8_lossy(&run.stdout));
}

/// The rewrite gives the expression its payload's type, so a slot that took
/// it unchecked can now refuse it: the file is put back and the decline
/// names the new error. A sibling rewrite in the same file is kept.
#[test]
fn fix_reverts_a_rewrite_that_causes_a_new_error() {
    let src = "module main\n\
        \n\
        import std/array\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let u = array.find(xs, fn(x: number) -> bool { return x > 9 })\n\
        \x20 let s: string = u ?? 0\n\
        \x20 let k = u ?? 7\n\
        \x20 print(\"${s} ${k}\")\n\
        \x20 return 0\n\
        }\n";
    let (_, fixed, out) = fix(src);
    assert!(fixed.contains("  let s: string = u ?? 0\n"), "{fixed}");
    assert!(fixed.contains("  let k = match u {\n"), "{fixed}");
    assert!(
        out.contains(
            "declined E0236 in src/main.glyph: the rewrite gives the expression its payload's type, and the check then reports [E0204] type mismatch: expected `string`, found `number` (main.glyph:8), so it was reverted"
        ),
        "{out}"
    );
}

/// Each condition the fix is held to declines with its reason, and leaves
/// the file as it was.
#[test]
fn fix_declines_with_the_reason() {
    let cases = [
        (
            "  let r = get_r(\"x\")\n  let n = r ?? 0\n  print(\"${n}\")",
            "an `Err` can be dropped, propagated with `?` or handled, and which one is meant is not determined",
        ),
        (
            "  let n = u ?? None\n  print(\"${n}\")",
            "`None` is an `Option` itself, or may be one, so whether it was meant as the value or as the whole `Option` is not determined",
        ),
        (
            "  let n = u ?? Some(4)\n  print(\"${n}\")",
            "`Some(4)` is an `Option` itself, or may be one",
        ),
        (
            "  let n = u ?? \"x\"\n  print(\"${n}\")",
            "`\"x\"` is not of the `Option`'s payload type, `number`, so the `match` that gives the value or the default would not check",
        ),
        (
            "  let oo: Option<Option<number>> = Some(None)\n  let n = oo ?? 0\n  print(\"${n}\")",
            "`0` is not of the `Option`'s payload type, `Option<number>`",
        ),
        (
            "  let ok = xs.length > 1\n  let n = match ok { true => first(xs), false => first(xs) } ?? 0\n  print(\"${n}\")",
            "the left side is not a name, a field, a call or an index",
        ),
    ];
    for (stmts, why) in cases {
        let src = program(stmts);
        let (dir, fixed, out) = fix(&src);
        glyph(&dir, &["fmt", "src"]);
        let formatted = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
        assert_eq!(fixed, formatted, "the file changed:\n{out}");
        assert!(out.contains("declined E0236") && !out.contains("applied E0236"), "{out}");
        assert!(out.contains(why), "expected the reason `{why}`:\n{out}");
    }
}

/// A payload type the checker has not decided declines: inside `fn f<T>`,
/// whether `d` is the value is not determined.
#[test]
fn fix_declines_an_undecided_payload() {
    let src = "module main\n\
        \n\
        fn pick<T>(o: Option<T>, d: T) -> T {\n\
        \x20 let x = o ?? d\n\
        \x20 return d\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 print(\"${pick(Some(1), 2)}\")\n\
        \x20 return 0\n\
        }\n";
    let (_, fixed, out) = fix(src);
    assert!(fixed.contains("let x = o ?? d"), "{fixed}");
    assert!(out.contains("the `Option`'s payload type is not decided"), "{out}");
}
