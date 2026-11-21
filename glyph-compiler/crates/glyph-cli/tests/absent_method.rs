//! A built-in method that answers absence with `undefined` is refused (G392,
//! D56, decision records 153 and 154).
//!
//! E0242 refuses `find`, `findLast` and `at` on an `Array<T>` and `at`,
//! `codePointAt` and `match` on a `string`, in every position, and names the
//! `std/*` function that returns an `Option`. Each test checks a real project
//! through the binary; the help tests apply the help and build the result,
//! and the fix tests run `glyph fix` and check the file again.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_absent_{prefix}_{}_{}", std::process::id(), n);
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

/// `src` is accepted by Glyph's checker and by the full check, `tsc`
/// included.
fn assert_accepted(label: &str, src: &str) {
    let v = check(&project(label, src), false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{label}: expected no errors:\n{src}\n{v:#}");
}

/// A module with `imports`, a `User` record, and a `main` whose body is
/// `stmts` after a few bindings.
fn program(imports: &str, stmts: &str) -> String {
    format!(
        "module main\n\
         \n\
         {imports}\
         type User = {{ id: string, name: string, }}\n\
         type Row = {{ items: Array<number>, }}\n\
         type S = {{ opt?: Array<number>, }}\n\
         \n\
         fn greet(u: User) -> string {{ return u.name }}\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let users: Array<User> = [{{ id: \"a\", name: \"Ann\", }}]\n\
         \x20 let xs = [3, 1, 2]\n\
         \x20 let s = \"abc\"\n\
         \x20 let rows: Array<Row> = [{{ items: [1], }}]\n\
         \x20 let so: S = {{ opt: [1], }}\n\
         {stmts}\n\
         \x20 print(\"${{xs.length}} ${{users.length}} ${{s}} ${{rows.length}}\")\n\
         \x20 return 0\n\
         }}\n"
    )
}

fn one(src: &str) -> (String, String) {
    let found = refusals(src);
    assert_eq!(
        found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        vec!["E0242"],
        "exactly one E0242 expected:\n{src}\n{found:#?}"
    );
    let (_, m, h) = found.into_iter().next().unwrap();
    (m, h)
}

const PRED: &str = "fn(x: User) -> bool { return x.id == \"zz\" }";
const NOTE: &str = "`None` when the index is past the end or falls on the second half of a character outside \
     the Basic Multilingual Plane, the 16-bit range (emoji, for example)";

/// Record 153's builder checklist, item 4: each refusal with its exact text.
#[test]
fn each_absent_method_is_refused_with_its_text() {
    let find_msg = "the call to `users.find` returns `User | undefined`; Glyph writes a value that may be missing as an `Option`";
    let find_help = "pass `users` and the same callback to `array.find`, which returns `Option<User>`, and `match` it (with `import std/array`)";
    let cases: Vec<(String, &str, String)> = vec![
        (format!("  let a = users.find({PRED}).name\n  print(a)"), find_msg, find_help.to_string()),
        (
            "  let b = greet(users.at(3))\n  print(b)".to_string(),
            "`users.at(3)` returns `User | undefined`; Glyph writes a value that may be missing as an `Option`",
            "write `array.get(users, 3)`, which returns `Option<User>`, and `match` it (with `import std/array`)".to_string(),
        ),
        (
            "  let n = xs.at(-1) ?? 0\n  print(\"${n}\")".to_string(),
            "`xs.at(-1)` returns `number | undefined`; Glyph writes a value that may be missing as an `Option`",
            "write `match array.get(xs, array.len(xs) - 1) { Some(v) => v, None => 0, }` (with `import std/array`)"
                .to_string(),
        ),
        (
            "  let hit = xs.find(fn(x: number) -> bool { return x > 2 }) ?? 0\n  print(\"${hit}\")".to_string(),
            "the call to `xs.find` returns `number | undefined`; Glyph writes a value that may be missing as an `Option`",
            "pass `xs` and the same callback to `array.find`, and `match` the `Option<number>` it returns, with `Some(v) => v` and the default in the `None` arm (with `import std/array`)".to_string(),
        ),
        (
            "  print(\"${s.at(7)}\")".to_string(),
            "`s.at(7)` returns `string | undefined`; Glyph writes a value that may be missing as an `Option`",
            format!("write `string.char_at(s, 7)`, which returns `Option<string>`: {NOTE} (with `import std/string`)"),
        ),
        (
            "  let d = s.codePointAt(0) + 1\n  print(\"${d}\")".to_string(),
            "`s.codePointAt(0)` returns `number | undefined`; Glyph writes a value that may be missing as an `Option`",
            format!("write `string.code_point_at(s, 0)`, which returns `Option<number>`: {NOTE} (with `import std/string`)"),
        ),
        (
            "  let m = s.match(\"b\")\n  print(\"${m}\")".to_string(),
            "`s.match(\"b\")` returns `RegExpMatchArray | null`; Glyph writes a value that may be missing as an `Option`",
            "to test, write `regex.matches(\"b\", s)`; for the groups, `regex.captures(\"b\", s)`, which returns `[]` when nothing matches (with `import std/regex`)".to_string(),
        ),
        (format!("  let e = users.find({PRED})?.name\n  print(\"${{e}}\")"), find_msg, find_help.to_string()),
        (
            "  let r = rows[0].items.find(fn(x: number) -> bool { return x > 0 })\n  print(\"${r}\")".to_string(),
            "the call to `rows[0].items.find` returns `number | undefined`; Glyph writes a value that may be missing as an `Option`",
            "pass `rows[0].items` and the same callback to `array.find`, which returns `Option<number>`, and `match` it (with `import std/array`)".to_string(),
        ),
        (
            "  let o = so.opt?.find(fn(x: number) -> bool { return x > 0 })\n  print(\"${o}\")".to_string(),
            "the call to `so.opt?.find` returns `number | undefined`; Glyph writes a value that may be missing as an `Option`",
            "pass `so.opt` and the same callback to `array.find`, which returns `Option<number>`, and `match` it (with `import std/array`)".to_string(),
        ),
        (
            format!("  let label = match users.find({PRED}) {{\n    Some(v) => v.name,\n    None => \"none\",\n  }}\n  print(label)"),
            find_msg,
            find_help.to_string(),
        ),
    ];
    for (stmts, msg, help) in cases {
        let (m, h) = one(&program("", &stmts));
        assert_eq!(m, msg, "{stmts}");
        assert_eq!(h, help, "{stmts}");
    }
}

/// A help names the module's own import of the namespace and drops the
/// import note when it is there.
#[test]
fn the_help_writes_the_module_s_own_namespace() {
    let (_, help) = one(&program("import std/array as arr\n\n", "  let b = greet(users.at(0))\n  print(b)"));
    assert_eq!(help, "write `arr.get(users, 0)`, which returns `Option<User>`, and `match` it");
}

/// Item 5: the `Option` spellings, `mut` on a pure call, an npm value and
/// the methods that answer "not found" with a value stay as they were.
#[test]
fn the_option_spellings_and_the_unjudged_calls_are_accepted() {
    assert_accepted(
        "option",
        &program(
            "import std/array\nimport std/string\n\n",
            "  let a = match array.find(users, fn(u) { u.id == \"a\" }) { Some(u) => u.name, None => \"none\", }\n\
             \x20 let b = match array.get(xs, 0) { Some(v) => v, None => 0, }\n\
             \x20 let c = match array.find_last(xs, fn(x: number) -> bool { return x > 1 }) { Some(v) => v, None => 0, }\n\
             \x20 let d = match string.char_at(s, 0) { Some(ch) => ch, None => \"\", }\n\
             \x20 let e = match string.code_point_at(s, 0) { Some(n) => n, None => 0, }\n\
             \x20 mut xs.pop()\n\
             \x20 let f = s.charAt(0)\n\
             \x20 let g = s.indexOf(\"b\")\n\
             \x20 print(\"${a} ${b} ${c} ${d} ${e} ${f} ${g}\")",
        ),
    );
    // `mut xs.at(0)` is one fault, E0241, and no E0242 beside it.
    let found = refusals(&program("", "  mut xs.at(0)"));
    assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), vec!["E0241"], "{found:#?}");
    // `pop` used as a value is E0240's alone.
    let found = refusals(&program("", "  let p = xs.pop()\n  print(\"${p}\")"));
    assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), vec!["E0240"], "{found:#?}");
}

/// A receiver the checker types `Unknown` is not judged.
#[test]
fn an_unknown_receiver_is_not_judged() {
    let src = "module main\n\
        \n\
        import std/json\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let v = json.parse(\"[1]\")\n\
        \x20 let w = match v { Ok(x) => x, Err(_) => 0, }\n\
        \x20 let h = w.find(fn(x) { x == 1 })\n\
        \x20 let i = w.at(0)\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert!(found.iter().all(|f| f.0 != "E0242"), "{found:#?}");
}

/// Item 6: every help, applied, passes the full check, and probe `e` prints
/// what it should.
#[test]
fn every_help_applied_compiles_and_runs() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/array\n\
        import std/string\n\
        import std/regex\n\
        \n\
        type User = { id: string, name: string, }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let users: Array<User> = [{ id: \"a\", name: \"Ann\", }]\n\
        \x20 let s = \"a😀\"\n\
        \x20 let n = match array.get(xs, array.len(xs) - 1) { Some(v) => v, None => 0, }\n\
        \x20 let hit = match array.find(xs, fn(x: number) -> bool { return x > 2 }) { Some(v) => v, None => 0, }\n\
        \x20 let name = match array.find(users, fn(u) { u.id == \"a\" }) { Some(u) => u.name, None => \"none\", }\n\
        \x20 let last = match array.find_last(xs, fn(x: number) -> bool { return x < 3 }) { Some(v) => v, None => 0, }\n\
        \x20 let third = match array.get(users, 3) { Some(u) => u.name, None => \"none\", }\n\
        \x20 let c = match string.char_at(s, 1) { Some(ch) => ch, None => \"none\", }\n\
        \x20 let end = match string.char_at(s, string.len(s) - 1) { Some(ch) => ch, None => \"none\", }\n\
        \x20 let cp = match string.code_point_at(s, 0) { Some(v) => v, None => 0, }\n\
        \x20 let m = regex.matches(\"b\", \"abc\")\n\
        \x20 let g = regex.captures(\"(b)\", \"abc\")\n\
        \x20 io.println(\"${n} ${hit} ${name} ${last} ${third} ${c} ${end} ${cp} ${m} ${g.length}\")\n\
        \x20 return 0\n\
        }\n";
    let dir = project("help", src);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{v:#}");
    let out = glyph(&dir, &["run", "src/main.glyph"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 3 Ann 2 none 😀 none 97 true 1"),
        "unexpected output:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Record 154: the whole character at a code-unit index, or `None`. Lone
/// surrogates are built with `string.slice`, since Glyph's `\u{..}` escape
/// refuses a surrogate.
#[test]
fn the_string_accessors_never_return_half_a_pair() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/string\n\
        \n\
        fn show(o: Option<string>) -> string {\n\
        \x20 return match o { Some(c) => \"Some(${c})\", None => \"None\", }\n\
        }\n\
        \n\
        fn num(o: Option<number>) -> string {\n\
        \x20 return match o { Some(c) => \"Some(${c})\", None => \"None\", }\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let s = \"a😀\"\n\
        \x20 io.println(\"${show(string.char_at(s, 0))} ${show(string.char_at(s, 1))} ${show(string.char_at(s, 2))} ${show(string.char_at(s, 0 - 1))} ${show(string.char_at(s, 3))}\")\n\
        \x20 io.println(\"${num(string.code_point_at(s, 0))} ${num(string.code_point_at(s, 1))} ${num(string.code_point_at(s, 2))} ${num(string.code_point_at(s, 0 - 1))} ${num(string.code_point_at(s, 3))}\")\n\
        \x20 let hi = string.slice(\"😀\", 0, 1)\n\
        \x20 let lo = string.slice(\"😀\", 1, 2)\n\
        \x20 let mid = \"x${hi}y\"\n\
        \x20 io.println(\"${show(string.char_at(s, 1.5))} ${show(string.char_at(mid, 1))} ${num(string.code_point_at(hi, 0))} ${show(string.char_at(lo, 0))} ${num(string.code_point_at(lo, 0))} ${show(string.char_at(mid, 2))}\")\n\
        \x20 return 0\n\
        }\n";
    let dir = project("strings", src);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{v:#}");
    let out = glyph(&dir, &["run", "src/main.glyph"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "Some(a) Some(😀) None None None\nSome(97) Some(128512) None None None\nNone None None None None Some(y)"
        ),
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

/// Item 7: `find`, `findLast` and a literal `at` are rewritten, a `??`
/// default becomes the `match`, and `import std/array` is added.
#[test]
fn fix_rewrites_the_array_lookups_and_adds_the_import() {
    let src = "module main\n\
        \n\
        import std/io\n\
        \n\
        type User = { id: string, name: string, }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let users: Array<User> = [{ id: \"a\", name: \"Ann\", }]\n\
        \x20 let n = xs.at(-1) ?? 0\n\
        \x20 let hit = xs.find(fn(x: number) -> bool { return x > 2 }) ?? 0\n\
        \x20 let low = xs.findLast(fn(x: number) -> bool { return x < 3 }) ?? 0\n\
        \x20 let first = xs.at(0) ?? 9\n\
        \x20 let label = match users.find(fn(u: User) -> bool { return u.id == \"a\" }) {\n\
        \x20   Some(u) => u.name,\n\
        \x20   None => \"none\",\n\
        \x20 }\n\
        \x20 io.println(\"${n} ${hit} ${low} ${first} ${label}\")\n\
        \x20 return 0\n\
        }\n";
    let (dir, fixed, out) = fix(src);
    assert_eq!(out.matches("applied E0242").count(), 5, "{out}");
    assert!(fixed.contains("import std/io\nimport std/array\n"), "{fixed}");
    assert!(
        fixed.contains("  let n = match array.get(xs, array.len(xs) - 1) {\n    Some(v) => v,\n    None => 0,\n  }\n"),
        "{fixed}"
    );
    assert!(fixed.contains("let hit = match array.find(xs, fn(x: number) -> bool { return x > 2 }) {"), "{fixed}");
    assert!(fixed.contains("let low = match array.find_last(xs, fn(x: number) -> bool { return x < 3 }) {"), "{fixed}");
    assert!(fixed.contains("let first = match array.get(xs, 0) {"), "{fixed}");
    assert!(fixed.contains("let label = match array.find(users, fn(u: User) -> bool { return u.id == \"a\" }) {"), "{fixed}");
    assert_fix_settles(&dir, &fixed);
    let run = glyph(&dir, &["run", "src/main.glyph"]);
    assert!(String::from_utf8_lossy(&run.stdout).contains("2 3 2 3 Ann"), "{}", String::from_utf8_lossy(&run.stdout));
}

/// A module with no import gets one under its `module` line.
#[test]
fn fix_adds_the_first_import_under_the_module_line() {
    let src = "module main\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let n = xs.at(1) ?? 0\n\
        \x20 print(\"${n}\")\n\
        \x20 return 0\n\
        }\n";
    let (dir, fixed, _) = fix(src);
    assert!(fixed.starts_with("module main\n\nimport std/array\n\nfn main"), "{fixed}");
    assert_fix_settles(&dir, &fixed);
}

/// The fix declines, with the reason, where the rewrite is not determined.
#[test]
fn fix_declines_with_the_reason() {
    let cases = [
        // A computed index may be negative, and `at` counts from the end.
        (
            "  let i = 1\n  let n = xs.at(i) ?? 0\n  print(\"${n}\")",
            "the index is not an integer literal, so whether `at` counts from the end is decided at run time, and `array.get` never does",
        ),
        // A negative index on a value no name holds would be written twice.
        (
            "  let n = xs.filter(fn(x: number) -> bool { return x > 0 }).at(-1) ?? 0\n  print(\"${n}\")",
            "a negative index counts from the end, so the rewrite writes the receiver twice, and the receiver is not a name, a field or an index; bind it with `let` first",
        ),
        (
            "  let c = s.at(1) ?? \"\"\n  print(c)",
            "`char_at`/`code_point_at` return `None` where `at`/`codePointAt` return half of a surrogate pair or count from the end, so the rewrite would change behaviour; the help names the call",
        ),
        (
            "  let c = s.at(-1) ?? \"\"\n  print(c)",
            "`char_at`/`code_point_at` return `None` where `at`/`codePointAt` return half of a surrogate pair or count from the end, so the rewrite would change behaviour; the help names the call",
        ),
        (
            "  let c = s.codePointAt(0) ?? 0\n  print(\"${c}\")",
            "`char_at`/`code_point_at` return `None` where `at`/`codePointAt` return half of a surrogate pair or count from the end, so the rewrite would change behaviour; the help names the call",
        ),
        (
            "  let o = so.opt?.find(fn(x: number) -> bool { return x > 0 }) ?? 0\n  print(\"${o}\")",
            "the call is written `?.`, so the receiver may be missing, and `array.find` needs it present; what a missing receiver should give is not determined",
        ),
    ];
    for (stmts, why) in cases {
        let src = program("", stmts);
        let (dir, fixed, out) = fix(&src);
        glyph(&dir, &["fmt", "src"]);
        let formatted = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
        assert_eq!(fixed, formatted, "the file changed:\n{out}");
        assert!(out.contains("declined E0242") && !out.contains("applied E0242"), "{out}");
        assert!(out.contains(why), "expected the reason `{why}`:\n{out}");
    }
}

/// A binding already called `array` stops the import, with the reason.
#[test]
fn fix_declines_the_import_when_the_name_is_taken() {
    let src = "module main\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let array = [3, 1, 2]\n\
        \x20 let n = array.at(0) ?? 0\n\
        \x20 print(\"${n}\")\n\
        \x20 return 0\n\
        }\n";
    let (_, fixed, out) = fix(src);
    assert!(fixed.contains("let n = array.at(0) ?? 0") && !out.contains("applied E0242"), "{out}\n{fixed}");
    assert!(out.contains("the name `array` is already bound here"), "{out}");
}

/// Record 155, F1: `let u = xs.find(f)` then `let v = u ?? 0` prints `v=0` on
/// 0.1.160. Rewriting only the `find` would hand `??` an `Option`, which is
/// never nullish, and the program would print `v=[object Object]` with every
/// check passing. The fix declines it, names the position, and leaves the
/// file as it was.
#[test]
fn fix_declines_a_rewrite_whose_value_is_read_on() {
    let src = "module main\n\
        \n\
        import std/io\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let u = xs.find(fn(x: number) -> bool { return x > 9 })\n\
        \x20 let v = u ?? 0\n\
        \x20 io.println(\"v=${v}\")\n\
        \x20 return 0\n\
        }\n";
    let (dir, fixed, out) = fix(src);
    glyph(&dir, &["fmt", "src"]);
    assert_eq!(fixed, std::fs::read_to_string(dir.join("src/main.glyph")).unwrap(), "{out}");
    assert!(fixed.contains("let u = xs.find(") && !fixed.contains("import std/array"), "{fixed}");
    assert!(
        out.contains("declined E0242") && out.contains("it initializes a `let` with no declared type"),
        "{out}"
    );
    // Every other position that reads the value as the element declines too,
    // naming what reads it.
    let cases = [
        ("  let a = xs.find(fn(x: number) -> bool { return x > 1 }).toFixed(1)\n  print(a)", "its value is read with `.toFixed`"),
        ("  let b = greet(users.at(0))\n  print(b)", "it is an argument where the parameter is `User`"),
        ("  let c: number = xs.at(0)\n  print(\"${c}\")", "it initializes a `let` declared `number`"),
        ("  let d = xs.at(0) + 1\n  print(\"${d}\")", "it is an operand of an operator"),
        ("  print(\"${xs.at(0)}\")", "it is turned into text in a `${}`"),
    ];
    for (stmts, why) in cases {
        let (_, _, out) = fix(&program("", stmts));
        assert!(out.contains("declined E0242") && !out.contains("applied E0242"), "{stmts}\n{out}");
        assert!(out.contains(why), "expected `{why}`:\n{out}");
    }
}

/// Record 155, F1: each position that uses the value up is rewritten, and the
/// result passes the full check and prints what it should.
#[test]
fn fix_rewrites_where_the_option_is_used_up() {
    let src = "module main\n\
        \n\
        import std/io\n\
        \n\
        fn show(o: Option<number>) -> string {\n\
        \x20 return match o { Some(v) => \"${v}\", None => \"none\", }\n\
        }\n\
        \n\
        fn first(xs: Array<number>) -> Option<number> {\n\
        \x20 return xs.find(fn(x: number) -> bool { return x > 1 })\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 xs.find(fn(x: number) -> bool { return x > 9 })\n\
        \x20 let a = xs.find(fn(x: number) -> bool { return x > 9 }) ?? 0\n\
        \x20 let b = match xs.find(fn(x: number) -> bool { return x < 3 }) { Some(v) => v, None => 0, }\n\
        \x20 let c: Option<number> = xs.at(5)\n\
        \x20 let d = show(xs.findLast(fn(x: number) -> bool { return x > 1 }))\n\
        \x20 io.println(\"${a} ${b} ${show(c)} ${d} ${show(first(xs))}\")\n\
        \x20 return 0\n\
        }\n";
    let (dir, fixed, out) = fix(src);
    assert_eq!(out.matches("applied E0242").count(), 6, "{out}");
    assert!(!out.contains("declined E0242"), "{out}");
    assert_fix_settles(&dir, &fixed);
    let run = glyph(&dir, &["run", "src/main.glyph"]);
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("0 1 none 2 3"),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

/// Record 155, F3: a type parameter is named in the message and the help.
#[test]
fn a_type_parameter_is_named() {
    let src = "module main\n\
        \n\
        fn g<T>(ys: Array<T>) -> T {\n\
        \x20 return ys.at(0)\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 print(\"${g([1])}\")\n\
        \x20 return 0\n\
        }\n";
    let (msg, help) = one(src);
    assert_eq!(msg, "`ys.at(0)` returns `T | undefined`; Glyph writes a value that may be missing as an `Option`");
    assert_eq!(help, "write `array.get(ys, 0)`, which returns `Option<T>`, and `match` it (with `import std/array`)");
}

/// Record 155: when `array` is already bound, the help says to import
/// `std/array` under another name, as the fix's decline does.
#[test]
fn the_help_matches_the_decline_when_the_name_is_taken() {
    let (_, help) = one(&program("", "  let array = [9]\n  let c = xs.at(0) ?? array.length\n  print(\"${c}\")"));
    assert_eq!(
        help,
        "write `match array.get(xs, 0) { Some(v) => v, None => array.length, }` (import `std/array` under another name, since `array` is already bound here, and call through that name)"
    );
}
