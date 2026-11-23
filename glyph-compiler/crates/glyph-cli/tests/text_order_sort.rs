//! A sort with no comparator over values that are not text (G388, decision
//! record 182, section 5a).
//!
//! JavaScript's `sort()` and `toSorted()` with no comparator compare the
//! text of each element, so `[100, 9, 25]` sorts as `100, 25, 9`. E0243
//! refuses both unless the elements are text: a `string`, a string-literal
//! union, a refined `string`, or a second name for one. The help writes the
//! comparator in the form the call was written in, and the help tests apply
//! it the way an agent would and run the full check, `tsc` included.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_text_order_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
    dir
}

fn project(prefix: &str, src: &str) -> PathBuf {
    let dir = unique_tmp(prefix);
    std::fs::write(dir.join("src").join("main.glyph"), src).expect("write module");
    dir
}

/// `check --json` on the project, `tsc` included unless `no_tsc`.
fn check(dir: &Path, no_tsc: bool) -> Value {
    let mut args = vec!["check", "--no-test", "--json"];
    if no_tsc {
        args.push("--no-tsc");
    }
    args.push("src");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(&args)
        .output()
        .expect("run glyph check");
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

/// Every error in `src`, checked without `tsc`, as `(code, message, help)`.
fn refusals(src: &str) -> Vec<(String, String, String)> {
    errors(&check(&project("r", src), true))
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap_or("").to_string(),
                d["message"].as_str().unwrap_or("").to_string(),
                d["help"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn codes(found: &[(String, String, String)]) -> Vec<&str> {
    let mut c: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
    c.sort();
    c
}

/// `src` passes the full check, `tsc` included.
fn assert_accepted(label: &str, src: &str) {
    let v = check(&project(label, src), false);
    assert!(
        v["ok"] == true && errors(&v).is_empty(),
        "{label}: expected no errors:\n{src}\n{v:#}"
    );
}

/// The declarations every program below may name.
const DECLS: &str = "type Tier = \"free\" | \"pro\"\n\
    type Name = string\n\
    type Plan = Tier\n\
    type Slug = string where value.length > 0\n\
    type Label = Slug\n\
    type User = { name: string, }\n\
    type Shape = Circle(number) | Square(number)\n\
    type Count = int\n\
    pub interface Named {\n\
    \x20 name: string\n\
    }\n";

/// A module whose function `work` takes `xs: Array<elem>` and a comparator
/// `cmp` of the type `cmp_ty` (when given), and runs `stmt`.
fn program(generics: &str, elem: &str, cmp_ty: Option<&str>, stmt: &str) -> String {
    let cmp = cmp_ty.map(|t| format!(", cmp: {t}")).unwrap_or_default();
    format!(
        "module main\n\
         \n\
         import std/array\n\
         \n\
         {DECLS}\n\
         pub fn work{generics}(xs: Array<{elem}>{cmp}) -> number {{\n\
         {stmt}\n\
         \x20 return xs.length\n\
         }}\n"
    )
}

/// Every element type the rule refuses, with the generics `work` declares.
const REFUSED: [(&str, &str); 14] = [
    ("", "number"),
    ("", "int"),
    ("", "bigint"),
    ("", "Count"),
    ("", "bool"),
    ("", "User"),
    ("", "Named"),
    ("", "Shape"),
    ("", "Option<number>"),
    ("", "Nullable<string>"),
    ("", "Array<string>"),
    ("", "unknown"),
    ("<T>", "T"),
    // A bounded parameter is read as its bound (G263).
    ("<T: Named>", "T"),
];

/// Every element type the rule accepts, or does not judge.
const ACCEPTED: [(&str, &str); 10] = [
    ("", "string"),
    ("", "Tier"),
    ("", "\"a\" | \"b\""),
    ("", "Name"),
    ("", "Plan"),
    ("", "Slug"),
    ("", "Label"),
    // A parameter bounded by text is text, through a chain of bounds too.
    ("<T: string>", "T"),
    ("<T: Tier>", "T"),
    ("<U: string, T: U>", "T"),
];

#[test]
fn each_refused_element_type_is_e0243_in_every_form() {
    for (g, elem) in REFUSED {
        let st = refusals(&program(g, elem, None, "  mut xs.sort()"));
        assert_eq!(codes(&st), vec!["E0243"], "{elem}, `mut xs.sort()`:\n{st:#?}");
        let ts = refusals(&program(g, elem, None, "  let ys = xs.toSorted()\n  print(\"${ys.length}\")"));
        assert_eq!(codes(&ts), vec!["E0243"], "{elem}, `xs.toSorted()`:\n{ts:#?}");
        let v = refusals(&program(g, elem, None, "  let ys = xs.sort()\n  print(\"${ys.length}\")"));
        assert_eq!(codes(&v), vec!["E0240", "E0243"], "{elem}, `let ys = xs.sort()`:\n{v:#?}");
        let bare = refusals(&program(g, elem, None, "  xs.sort()"));
        assert_eq!(codes(&bare), vec!["E0240", "E0243"], "{elem}, `xs.sort()`:\n{bare:#?}");
    }
}

#[test]
fn text_elements_and_a_written_comparator_are_accepted() {
    for (g, elem) in ACCEPTED {
        for stmt in ["  mut xs.sort()", "  let ys = xs.toSorted()\n  print(\"${ys.length}\")"] {
            let found = refusals(&program(g, elem, None, stmt));
            assert!(found.is_empty(), "{elem}, {stmt}:\n{found:#?}");
        }
    }
    for (g, elem) in REFUSED {
        let ty = format!("fn(a: {elem}, b: {elem}) -> number");
        for stmt in ["  mut xs.sort(cmp)", "  let ys = xs.toSorted(cmp)\n  print(\"${ys.length}\")"] {
            let found = refusals(&program(g, elem, Some(&ty), stmt));
            assert!(found.is_empty(), "{elem}, {stmt}:\n{found:#?}");
        }
    }
}

#[test]
fn the_message_names_the_type_and_the_order_it_gives() {
    let (_, m, h) = refusals(&program("", "number", None, "  mut xs.sort()")).remove(0);
    assert_eq!(
        m,
        "`xs.sort()` with no comparator compares `number` values as text, so `[100, 9, 25]` sorts as `100, 25, 9`"
    );
    assert_eq!(h, "pass a comparator: `mut xs.sort(fn(a: number, b: number) -> number { return a - b })`");
    let (_, m, h) = refusals(&program("", "int", None, "  let ys = xs.toSorted()\n  print(\"${ys.length}\")")).remove(0);
    assert_eq!(
        m,
        "`xs.toSorted()` with no comparator compares `int` values as text, so `[100, 9, 25]` sorts as `100, 25, 9`"
    );
    // `toSorted` is newer than the `es2022` library Glyph's tsconfig asks
    // for, so the help writes the copy `array.sort` returns (record 187).
    assert_eq!(
        h,
        "write `array.sort(xs, fn(a: int, b: int) -> number { return a - b })`, which returns a sorted copy"
    );
    let (_, m, h) = refusals(&program("", "User", None, "  mut xs.sort()")).remove(0);
    assert_eq!(m, "`xs.sort()` with no comparator compares `User` values as text");
    assert_eq!(
        h,
        "pass a comparator: `mut xs.sort(cmp)`, where `cmp` is a `fn(a: User, b: User) -> number` that is negative when `a` comes first"
    );
    // `a - b` on two `bigint`s is a `bigint`, so no comparator is written.
    let (_, _, h) = refusals(&program("", "bigint", None, "  mut xs.sort()")).remove(0);
    assert!(h.contains("`fn(a: bigint, b: bigint) -> number`"), "{h}");
}

#[test]
fn e0240_and_e0243_on_a_bare_sort_write_the_same_call() {
    let found = refusals(&program("", "number", None, "  xs.sort()"));
    let want = "`mut xs.sort(fn(a: number, b: number) -> number { return a - b })`";
    for (code, _, help) in &found {
        assert!(help.contains(want), "{code}: {help}");
    }
    let found = refusals(&program("", "number", None, "  let ys = xs.sort()\n  print(\"${ys.length}\")"));
    let want = "`array.sort(xs, fn(a: number, b: number) -> number { return a - b })`";
    for (code, _, help) in &found {
        assert!(help.contains(want), "{code}: {help}");
    }
}

/// The first code span of `help`.
fn first_code(help: &str) -> &str {
    help.split('`').nth(1).expect("a code span")
}

/// The comparator type the help names for `cmp`, when it names one.
fn cmp_type(help: &str) -> Option<&str> {
    let rest = help.split("`cmp` is a `").nth(1)?;
    rest.split('`').next()
}

/// The line a help's code replaces the refused line with.
type Apply = fn(&str) -> String;

/// Each help, applied the way an agent would apply it, passes the full
/// check, `tsc` included, with no `toSorted` left in it: the refused line replaced by the help's code, and
/// `cmp` given the type the help names, when it names one.
#[test]
fn every_help_applied_compiles() {
    let forms: [(&str, Apply); 4] = [
        ("  mut xs.sort()", |c| format!("  {c}")),
        ("  xs.sort()", |c| format!("  {c}")),
        ("  let ys = xs.sort()\n  print(\"${ys.length}\")", |c| format!("  let ys = {c}\n  print(\"${{ys.length}}\")")),
        ("  let ys = xs.toSorted()\n  print(\"${ys.length}\")", |c| {
            format!("  let ys = {c}\n  print(\"${{ys.length}}\")")
        }),
    ];
    for (g, elem) in REFUSED {
        for (stmt, applied) in &forms {
            for (code, _, help) in refusals(&program(g, elem, None, stmt)) {
                let fixed = program(g, elem, cmp_type(&help), &applied(first_code(&help)));
                assert!(!fixed.contains("toSorted"), "{help}");
                assert_accepted(&format!("{code}-{elem}"), &fixed);
            }
        }
    }
}

#[test]
fn the_numeric_help_sorts_by_value() {
    let src = "module main\n\
        \n\
        import std/string\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let scores = [100, 9, 25]\n\
        \x20 mut scores.sort()\n\
        \x20 print(string.join(scores.map(fn(n: number) -> string { \"${n}\" }), \",\"))\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert_eq!(codes(&found), vec!["E0243"], "{found:#?}");
    let fixed = src.replace("mut scores.sort()", first_code(&found[0].2));
    let dir = project("run", &fixed);
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["run", "src/main.glyph"])
        .output()
        .expect("run glyph run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("9,25,100"), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn a_receiver_with_no_type_and_array_sort_are_not_judged() {
    // `array.sort` always takes a comparator.
    let found = refusals(&program("", "number", Some("fn(a: number, b: number) -> number"), "  let ys = array.sort(xs, cmp)\n  print(\"${ys.length}\")"));
    assert!(found.is_empty(), "{found:#?}");
    // An `extern_ts` value is a receiver the checker cannot type.
    let src = "module main\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs: extern_ts(\"number[]\") = [3, 1]\n\
        \x20 mut xs.sort()\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert!(found.iter().all(|f| f.0 != "E0243"), "{found:#?}");
}

#[test]
fn glyph_fix_leaves_a_bare_sort_that_needs_a_comparator() {
    let dir = project("fix", &program("", "number", None, "  xs.sort()"));
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["fix", "src"])
        .output()
        .expect("run glyph fix");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("declined E0240") && !text.contains("applied E0240"), "{text}");
    // The one reason that applied, not a list of every possible one.
    assert!(text.contains("needs a comparator (E0243)") && !text.contains("deferred"), "{text}");
    let after = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
    assert!(after.contains("\n  xs.sort()\n"), "{after}");
}

/// `array.sort` copies already, so a help over `xs.slice().sort()` sorts
/// `xs` and writes no second copy.
#[test]
fn a_copying_help_drops_a_slice_before_the_sort() {
    for stmt in [
        "  let ys = xs.slice().sort()\n  print(\"${ys.length}\")",
        "  let ys = xs.slice().toSorted()\n  print(\"${ys.length}\")",
    ] {
        let found = refusals(&program("", "number", None, stmt));
        assert!(!found.is_empty(), "{stmt}");
        for (code, _, help) in &found {
            assert!(help.contains("`array.sort(xs, fn(a: number, b: number) -> number { return a - b })`"), "{code}: {help}");
            assert!(!help.contains("slice"), "{code}: {help}");
        }
    }
}

/// `glyph fix` names the one reason it declines an E0240, for each position
/// the checker places a call in (record 192). The reason is read off the
/// diagnostic's message, so this pins each wording: a change to one would
/// otherwise drop that position to the generic sentence unnoticed.
#[test]
fn glyph_fix_names_the_reason_for_each_declined_position() {
    let body = |stmts: &str| {
        format!(
            "module main\n\
             \n\
             fn make() -> Array<number> {{\n\
             \x20 return [1]\n\
             }}\n\
             \n\
             fn tail(ys: Array<number>) -> number {{\n\
             {stmts}\n\
             }}\n\
             \n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 print(\"${{tail([2])}}\")\n\
             \x20 return 0\n\
             }}\n"
        )
    };
    let cases = [
        ("  ys.push(4)", "the call is the body's last expression, so its value is returned"),
        ("  let n = ys.pop()\n  return ys.length", "the call's value is used"),
        (
            "  match ys.length > 0 {\n    true => ys.push(1),\n    false => {},\n  }\n  return ys.length",
            "the call is an arm of a `match`, which needs a block to hold a statement",
        ),
        ("  mut make().push(1)\n  return ys.length", "the call changes a value no name holds"),
        ("  defer ys.pop()\n  return ys.length", "the call is deferred, and `defer` takes no `mut`"),
        (
            "  ys.sort()\n  return ys.length",
            "the sort compares values that are not text and needs a comparator (E0243), which the help writes",
        ),
    ];
    for (stmts, reason) in cases {
        let dir = project("fixwhy", &body(stmts));
        let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
            .current_dir(&dir)
            .args(["fix", "src"])
            .output()
            .expect("run glyph fix");
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let declines: Vec<&str> = text.lines().filter(|l| l.contains("declined E0240")).collect();
        assert_eq!(declines.len(), 1, "{stmts}:\n{text}");
        assert!(
            declines[0].ends_with(&format!(
                "{reason}, so the repair depends on what it was for; the help names one"
            )) || declines[0].ends_with(reason),
            "{stmts}: expected `{reason}`:\n{text}"
        );
        assert!(!text.contains("stands where `mut ` alone"), "{stmts}: the generic reason:\n{text}");
    }
}
