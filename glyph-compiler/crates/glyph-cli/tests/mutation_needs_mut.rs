//! A call that changes its receiver is a `mut` statement (G380, D5 as
//! amended for 0.1.160, decision record 150).
//!
//! E0240 refuses one of the nine mutating `Array<T>` methods anywhere but as
//! the whole call of a `mut` statement, and under `mut` on a receiver that is
//! not a place. E0241 refuses `mut` before a call Glyph knows changes
//! nothing. Each test checks a real project through the binary, the way an
//! agent sees it, and the help tests apply the help and build the result.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_mutation_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
    dir
}

/// A project holding `modules` under `src/`, as `(file name, source)`.
fn project_modules(prefix: &str, modules: &[(&str, &str)]) -> PathBuf {
    let dir = unique_tmp(prefix);
    for (name, src) in modules {
        std::fs::write(dir.join("src").join(name), src).expect("write module");
    }
    dir
}

fn project(prefix: &str, src: &str) -> PathBuf {
    project_modules(prefix, &[("main.glyph", src)])
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

/// `src` is accepted by Glyph's checker and by the full check, `tsc`
/// included.
fn assert_accepted(label: &str, src: &str) {
    let v = check(&project(label, src), false);
    assert!(
        v["ok"] == true && errors(&v).is_empty(),
        "{label}: expected no errors:\n{src}\n{v:#}"
    );
}

/// A `main` whose body is `stmts`, after a comparator and a few arrays.
fn program(stmts: &str) -> String {
    format!(
        "module main\n\
         \n\
         fn cmp(a: number, b: number) -> number {{ return a - b }}\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let xs = [3, 1, 2]\n\
         \x20 let ys = [4]\n\
         {stmts}\n\
         \x20 print(\"${{xs.length}} ${{ys.length}}\")\n\
         \x20 return 0\n\
         }}\n"
    )
}

fn one(src: &str, code: &str) -> (String, String) {
    let found = refusals(src);
    assert_eq!(
        found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        vec![code],
        "exactly one {code} expected:\n{src}\n{found:#?}"
    );
    let (_, m, h) = found.into_iter().next().unwrap();
    (m, h)
}

/// The nine mutating methods, each with arguments it accepts.
const NINE: [(&str, &str); 9] = [
    ("push", "5"),
    ("pop", ""),
    ("shift", ""),
    ("unshift", "0"),
    ("splice", "0, 1"),
    ("sort", "cmp"),
    ("reverse", ""),
    ("fill", "0"),
    ("copyWithin", "0, 1"),
];

#[test]
fn each_mutating_method_as_a_bare_statement_is_e0240_and_mut_is_accepted() {
    for (m, args) in NINE {
        let call = format!("xs.{m}({args})");
        let (msg, help) = one(&program(&format!("  {call}")), "E0240");
        assert_eq!(
            msg,
            format!("`{call}` changes `xs` in place, and a call that changes a value is written as a `mut` statement")
        );
        assert_eq!(help, format!("write `mut {call}`, so `grep \"^\\s*mut \"` finds every change to `xs`"));
        assert_accepted(m, &program(&format!("  mut {call}")));
    }
}

#[test]
fn each_mutating_method_used_as_a_value_is_e0240() {
    for (m, args) in NINE {
        let call = format!("xs.{m}({args})");
        let (msg, _) = one(&program(&format!("  let _v = {call}")), "E0240");
        assert_eq!(msg, format!("`{call}` changes `xs` in place, and its value is used here"), "{m}");
    }
}

/// The reproduction from record 150: a helper sorts its parameter while
/// reading the smallest element.
#[test]
fn a_sort_returned_from_a_helper_is_e0240_with_the_copying_help() {
    let src = "module main\n\
        fn cmp(a: number, b: number) -> number { return a - b }\n\
        fn smallest(xs: Array<number>) -> number {\n\
        \x20 return xs.sort(cmp)[0]\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 print(\"${smallest([3, 1, 2])}\")\n\
        \x20 return 0\n\
        }\n";
    let (msg, help) = one(src, "E0240");
    assert_eq!(msg, "`xs.sort(cmp)` changes `xs` in place, and its value is used here");
    assert_eq!(
        help,
        "`array.sort(xs, cmp)` returns a sorted copy and leaves `xs` alone (with `import std/array`)"
    );
}

#[test]
fn every_value_position_is_refused() {
    let positions = [
        "  let n = xs.pop()\n  print(\"${n}\")",
        "  print(\"${xs.push(1)}\")",
        "  let k = string.from(xs.push(1))\n  print(k)",
        "  let t = xs.push(1) + 1\n  print(\"${t}\")",
        "  let s = xs.filter(fn(x: number) -> bool { return x > 1 }).sort(cmp)\n  print(\"${s.length}\")",
    ];
    for p in positions {
        let found = refusals(&program(p));
        assert!(
            found.iter().any(|f| f.0 == "E0240"),
            "expected E0240 for:\n{p}\n{found:#?}"
        );
    }
}

#[test]
fn a_call_on_a_value_nothing_holds_is_e0240_under_mut() {
    let src = "module main\n\
        fn make() -> Array<number> { return [1] }\n\
        fn cmp(a: number, b: number) -> number { return a - b }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let ys = [3, 1]\n\
        \x20 mut make().push(1)\n\
        \x20 mut ys.filter(fn(x: number) -> bool { return x > 0 }).sort(cmp)\n\
        \x20 print(\"${ys.length}\")\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert_eq!(found[0].1, "`.push(1)` adds to the new array `make()` returns, and nothing keeps it");
    assert_eq!(
        found[0].2,
        "bind the array with `let` first, `let ys = make()`, then write `mut ys.push(1)`"
    );
    assert_eq!(found[1].1, "`.sort(cmp)` sorts the new array `ys.filter(...)` returns, and nothing keeps it");
    assert!(found[1].2.starts_with("`array.sort(ys.filter(...), cmp)` returns the sorted array"), "{}", found[1].2);
    assert!(found.iter().all(|f| f.0 == "E0240"));
}

#[test]
fn a_statement_match_arm_and_a_block_tail_and_a_callback_are_refused() {
    let arm = program("  match xs.length > 1 {\n    true => xs.push(1),\n    false => {},\n  }");
    let (msg, help) = one(&arm, "E0240");
    assert!(msg.starts_with("`xs.push(1)` changes `xs` in place"), "{msg}");
    assert_eq!(help, "write the arm as a block holding the statement: `{ mut xs.push(1) }`");
    assert_accepted(
        "arm-block",
        &program("  match xs.length > 1 {\n    true => { mut xs.push(1) },\n    false => {},\n  }"),
    );

    let tail = program("  match xs.length > 1 {\n    true => { xs.push(1) },\n    false => {},\n  }");
    one(&tail, "E0240");

    let callback = program("  ys.forEach(fn(x: number) -> void { xs.push(x) })");
    let (msg, _) = one(&callback, "E0240");
    assert!(msg.starts_with("`xs.push(x)` changes `xs`"), "{msg}");
    assert_accepted("callback", &program("  ys.forEach(fn(x: number) -> void { mut xs.push(x) })"));
}

#[test]
fn mut_on_a_call_that_changes_nothing_is_e0241() {
    let (msg, help) = one(&program("  mut xs.concat(ys)"), "E0241");
    assert_eq!(
        msg,
        "`concat` returns a new array and leaves `xs` unchanged; this `mut` marks no change and the result is discarded"
    );
    assert_eq!(help, "to keep it, write `mut xs = xs.concat(ys)`");

    let (msg, help) = one(&program("  let s = \"abc\"\n  mut s.toUpperCase()\n  print(s)"), "E0241");
    assert!(msg.starts_with("`toUpperCase` returns a new string and leaves `s` unchanged"), "{msg}");
    assert_eq!(help, "to keep it, write `mut s = s.toUpperCase()`");

    one(&program("  mut xs.map(fn(x: number) -> number { return x + 1 })"), "E0241");

    let with_array = program("  mut array.push(xs, 1)").replacen("module main\n", "module main\nimport std/array\n", 1);
    let (msg, help) = one(&with_array, "E0241");
    assert_eq!(
        msg,
        "`array.push(xs, 1)` returns a new array and leaves `xs` unchanged; this `mut` marks no change and the result is discarded"
    );
    assert_eq!(help, "to keep it, write `mut xs = array.push(xs, 1)`, or change it in place with `mut xs.push(1)`");

    let with_io = program("  mut io.println(\"x\")").replacen("module main\n", "module main\nimport std/io\n", 1);
    let (_, help) = one(&with_io, "E0241");
    assert_eq!(
        help,
        "drop the `mut`: a function call is not written `mut`; the change it makes is a `mut` statement inside it (D5)"
    );
}

/// A user module's function under `mut` is E0241, decided by what the name
/// resolves to.
#[test]
fn mut_on_a_sibling_module_function_is_e0241() {
    let heap = "module heap\n\
        pub fn push(h: Array<number>, v: number) -> void {\n\
        \x20 mut h.push(v)\n\
        }\n";
    let main = "module main\n\
        import heap\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let h = [1]\n\
        \x20 mut heap.push(h, 5)\n\
        \x20 print(\"${h.length}\")\n\
        \x20 return 0\n\
        }\n";
    let v = check(&project_modules("sibling", &[("main.glyph", main), ("heap.glyph", heap)]), true);
    let errs = errors(&v);
    assert_eq!(errs.len(), 1, "{v:#}");
    assert_eq!(errs[0]["code"], "E0241");
}

/// A local `result` that shadows `std/result` is a value, so a mutating call
/// on it is a write, not a module function.
#[test]
fn a_local_that_shadows_a_stdlib_module_is_a_place() {
    let src = "module main\n\
        import std/result\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let result = [1]\n\
        \x20 mut result.push(2)\n\
        \x20 print(\"${result.length}\")\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert!(found.iter().all(|f| f.0 != "E0240" && f.0 != "E0241"), "{found:#?}");
}

#[test]
fn the_writes_that_stay_accepted() {
    assert_accepted(
        "places",
        "module main\n\
         type State = { xs: Array<number>, grid: Array<Array<number>>, }\n\
         const ITEMS: Array<number> = [1, 2]\n\
         fn cmp(a: number, b: number) -> number { return a - b }\n\
         fn helper(xs: Array<number>) -> void {\n\
         \x20 mut xs.sort(cmp)\n\
         }\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let s: State = { xs: [1], grid: [[1], [2]], }\n\
         \x20 mut s.xs.push(2)\n\
         \x20 mut s.grid[0].push(5)\n\
         \x20 mut ITEMS.push(3)\n\
         \x20 helper(s.xs)\n\
         \x20 print(\"${s.xs.length} ${s.grid[0].length} ${ITEMS.length}\")\n\
         \x20 return 0\n\
         }\n",
    );
}

/// The stdlib `Set` is not typed by Glyph's checker yet (G423), so a write
/// to one is judged in neither direction until that gap closes.
#[test]
fn a_stdlib_set_receiver_is_not_judged_until_g423() {
    for line in ["  mut seen.add(\"a\")", "  seen.add(\"b\")"] {
        let src = format!(
            "module main\n\
             import std/set\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 let seen = set.create<string>()\n\
             {line}\n\
             \x20 return 0\n\
             }}\n"
        );
        let found = refusals(&src);
        assert!(found.iter().all(|f| f.0 != "E0240" && f.0 != "E0241"), "G423 edge moved:\n{src}\n{found:#?}");
    }
}

/// A receiver the checker cannot type (an `extern_ts` value, standing in for
/// an npm one) is judged in neither direction.
#[test]
fn an_untyped_receiver_is_not_judged() {
    for line in ["  mut v.push(1)", "  v.push(1)", "  mut v.concat([1])"] {
        let src = format!(
            "module main\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 let v = extern_ts(\"[] as any\")\n\
             {line}\n\
             \x20 return 0\n\
             }}\n"
        );
        let found = refusals(&src);
        assert!(found.iter().all(|f| f.0 != "E0240" && f.0 != "E0241"), "{src}\n{found:#?}");
    }
}

/// An omitted lambda parameter whose position supplies no type is not typed
/// here, so a write through it is not judged (record 150, section 4). This
/// pins the edge: when contextual typing reaches the position, it moves.
#[test]
fn an_unannotated_lambda_parameter_with_no_context_is_not_judged() {
    let src = "module main\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let f = fn(acc) { acc.push(1) }\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(src);
    assert!(found.iter().all(|f| f.0 != "E0240"), "{found:#?}");
}

/// `let n = mut xs.pop()` and `mut let` stay parse errors: `mut` is a
/// statement prefix, never an expression operator.
#[test]
fn mut_is_not_an_expression_operator() {
    for line in ["  let n = mut xs.pop()", "  mut let n = xs.pop()"] {
        let v = check(&project("syntax", &program(line)), true);
        let errs = errors(&v);
        assert!(!errs.is_empty(), "{line} was accepted");
        assert!(errs.iter().all(|e| e["stage"] == "parse"), "{line}: {v:#}");
    }
}

/// The reference program for the helps (record 150, `j_help`): every value
/// spelling the E0240 help names, written out, passes the full check and
/// prints what it should.
#[test]
fn every_help_spelling_compiles_and_runs() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/array\n\
        \n\
        fn cmp(a: number, b: number) -> number { return a - b }\n\
        \n\
        fn smallest(xs: Array<number>) -> number {\n\
        \x20 return array.sort(xs, cmp)[0]\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3, 1, 2]\n\
        \x20 let last = array.get(xs, array.len(xs) - 1)\n\
        \x20 mut xs.pop()\n\
        \x20 let first = array.get(xs, 0)\n\
        \x20 mut xs.shift()\n\
        \x20 let removed = array.slice(xs, 0, 0 + 1)\n\
        \x20 mut xs.splice(0, 1)\n\
        \x20 mut xs.push(7)\n\
        \x20 let n = array.len(xs)\n\
        \x20 let sorted = array.sort(xs.filter(fn(x: number) -> bool { return x > 0 }), cmp)\n\
        \x20 let back = array.reverse(xs)\n\
        \x20 let s = smallest([9, 4])\n\
        \x20 let pushed = array.push(xs, 1)\n\
        \x20 let front = array.concat([0], xs)\n\
        \x20 let shown = match last { Some(v) => v, None => 0, }\n\
        \x20 let shown2 = match first { Some(v) => v, None => 0, }\n\
        \x20 io.println(\"${shown} ${shown2} ${removed.length} ${n} ${sorted.length} ${back.length} ${s} ${xs[0]} ${pushed.length} ${front.length}\")\n\
        \x20 return 0\n\
        }\n";
    let dir = project("help", src);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{v:#}");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["run", "src/main.glyph"])
        .output()
        .expect("run glyph run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 3 1 1 1 1 4 7 2 2"),
        "unexpected output:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Each computed value help, applied the way an agent would (the refused
/// statement replaced by the help's spelling), passes the full check.
#[test]
fn each_value_help_applied_compiles() {
    let cases = [
        ("  let v = xs.sort(cmp)\n  print(\"${v.length}\")", "  let v = array.sort(xs, cmp)\n  print(\"${v.length}\")"),
        ("  let v = xs.reverse()\n  print(\"${v.length}\")", "  let v = array.reverse(xs)\n  print(\"${v.length}\")"),
        (
            "  let v = xs.pop()\n  print(\"${v}\")",
            "  let last = array.get(xs, array.len(xs) - 1)\n  mut xs.pop()\n  let v = match last { Some(x) => x, None => 0, }\n  print(\"${v}\")",
        ),
        (
            "  let v = xs.shift()\n  print(\"${v}\")",
            "  let first = array.get(xs, 0)\n  mut xs.shift()\n  let v = match first { Some(x) => x, None => 0, }\n  print(\"${v}\")",
        ),
        (
            "  let v = xs.splice(0, 1)\n  print(\"${v.length}\")",
            "  let removed = array.slice(xs, 0, 0 + 1)\n  mut xs.splice(0, 1)\n  print(\"${removed.length}\")",
        ),
        ("  let v = xs.push(5)\n  print(\"${v}\")", "  mut xs.push(5)\n  print(\"${array.len(xs)}\")"),
        ("  let v = xs.unshift(0)\n  print(\"${v}\")", "  let v = array.concat([0], xs)\n  print(\"${v.length}\")"),
        ("  let v = xs.fill(0)\n  print(\"${v.length}\")", "  mut xs.fill(0)\n  print(\"${xs.length}\")"),
    ];
    for (refused, applied) in cases {
        let found = refusals(&program(refused));
        assert!(found.iter().any(|f| f.0 == "E0240"), "{refused}\n{found:#?}");
        let fixed = program(applied).replacen("module main\n", "module main\nimport std/array\n", 1);
        assert_accepted("applied", &fixed);
    }
}
