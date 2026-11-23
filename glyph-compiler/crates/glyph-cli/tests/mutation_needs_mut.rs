//! A call that changes its receiver is a `mut` statement (G380, D5 as
//! amended for 0.1.160, decision record 150).
//!
//! E0240 refuses one of the nine mutating `Array<T>` methods anywhere but as
//! the whole call of a `mut` statement, and under `mut` on a receiver that is
//! not a place. E0241 refuses `mut` before a call Glyph knows changes
//! nothing. Since 0.1.167 (G423 step 2, decision record 157) the writing
//! members of the stdlib `Set`, `Store` and `Deque` follow the same rule,
//! every `Decimal` member and their readers take E0241, and a `sqlite.Db`
//! member is judged in neither direction. Each test checks a real project through the binary, the way an
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
    assert_eq!(found[1].1, "`.sort(cmp)` sorts the new array a call returns, and nothing keeps it");
    assert!(found[1].2.starts_with("`array.sort` returns the sorted array"), "{}", found[1].2);
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

/// The stdlib `Set` was typed in 0.1.163 (G423, step 1) and its writes are
/// judged since 0.1.167 (step 2, decision record 157, section 2b): a bare
/// `seen.add("b")` is E0240, and `mut seen.add("a")` is the statement D5
/// asks for.
#[test]
fn a_stdlib_set_write_is_judged() {
    let src = |line: &str| {
        format!(
            "module main\n\
             import std/set\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 let seen = set.create<string>()\n\
             {line}\n\
             \x20 print(\"${{seen.size()}}\")\n\
             \x20 return 0\n\
             }}\n"
        )
    };
    let (msg, help) = one(&src("  seen.add(\"b\")"), "E0240");
    assert_eq!(
        msg,
        "`seen.add(\"b\")` changes `seen` in place, and a call that changes a value is written as a `mut` statement"
    );
    assert_eq!(help, "write `mut seen.add(\"b\")`, so `grep \"^\\s*mut \"` finds every change to `seen`");
    assert_accepted("set-mut", &src("  mut seen.add(\"a\")"));
}

/// A `main` with one of each stdlib state holder, a record holding a store,
/// a module `const` store and a helper that takes an `Option`, then `stmts`.
fn stdlib_program(stmts: &str) -> String {
    format!(
        "module main\n\
         \n\
         import std/set\n\
         import std/store\n\
         import std/collections\n\
         \n\
         const HITS = store.create(0)\n\
         \n\
         type Box = {{ state: store.Store<number>, }}\n\
         \n\
         fn take_bool(b: bool) -> number {{ return 0 }}\n\
         \n\
         fn take_opt(o: Option<number>) -> number {{ return 0 }}\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let seen = set.create<number>([1, 2])\n\
         \x20 let st = store.create(1)\n\
         \x20 let q = collections.deque<number>([1, 2, 3])\n\
         \x20 let c: Box = {{ state: store.create(5), }}\n\
         {stmts}\n\
         \x20 print(\"${{seen.size()}} ${{st.get()}} ${{q.len()}} ${{c.state.get()}} ${{HITS.get()}}\")\n\
         \x20 return 0\n\
         }}\n"
    )
}

/// The eight writing members of the stdlib `Set`, `Store` and `Deque`, each
/// on a receiver that holds the right element type, with arguments it
/// accepts.
const WRITERS: [(&str, &str); 8] = [
    ("seen", "add(3)"),
    ("seen", "remove(1)"),
    ("st", "set(2)"),
    ("st", "update(fn(n) { n + 1 })"),
    ("q", "push_back(4)"),
    ("q", "push_front(0)"),
    ("q", "pop_back()"),
    ("q", "pop_front()"),
];

/// Each writing member is E0240 as a bare statement, through a field path
/// and on a module `const`, and the `mut` statement is accepted by Glyph
/// and by `tsc` (G423 step 2).
#[test]
fn each_stdlib_writing_member_as_a_bare_statement_is_e0240_and_mut_is_accepted() {
    for (recv, call) in WRITERS {
        let found = refusals(&stdlib_program(&format!("  {recv}.{call}")));
        assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["E0240"], "{recv}.{call}: {found:#?}");
        let want = format!("changes `{recv}` in place, and a call that changes a value is written as a `mut` statement");
        assert!(found[0].1.contains(&want), "{recv}.{call}: {found:#?}");
        assert_accepted(recv, &stdlib_program(&format!("  mut {recv}.{call}")));
    }
    for line in ["  c.state.set(2)", "  HITS.set(2)", "  HITS.update(fn(n) { n + 1 })"] {
        let (msg, _) = one(&stdlib_program(line), "E0240");
        assert!(msg.contains("a call that changes a value is written as a `mut` statement"), "{line}: {msg}");
        assert_accepted("place", &stdlib_program(&format!("  mut {}", line.trim())));
    }
}

/// A writing member whose value is read is E0240 in every value position:
/// a `let`, an argument, a `match` scrutinee and an interpolation.
#[test]
fn each_stdlib_writing_member_used_as_a_value_is_e0240() {
    for (recv, call) in WRITERS {
        let found = refusals(&stdlib_program(&format!("  let _v = {recv}.{call}")));
        assert!(
            found.iter().any(|f| f.0 == "E0240" && f.1.contains("and its value is used here")),
            "{recv}.{call}: {found:#?}"
        );
    }
    let positions = [
        "  let n = take_bool(seen.remove(1))",
        "  let n = take_opt(q.pop_front())",
        "  let n = match q.pop_back() { Some(v) => v, None => 0, }\n  print(\"${n}\")",
        "  let n = match seen.remove(2) { true => 1, false => 0, }\n  print(\"${n}\")",
    ];
    for line in positions {
        let found = refusals(&stdlib_program(line));
        assert!(
            found.iter().any(|f| f.0 == "E0240" && f.1.contains("and its value is used here")),
            "{line}: {found:#?}"
        );
    }
}

/// A pop interpolated into a string is one fault, E0240's: the `match` an
/// E0237 help would write over `q.pop_front()` is refused by E0240 too.
#[test]
fn an_interpolated_pop_is_e0240_only() {
    let (msg, _) = one(&stdlib_program("  print(\"${q.pop_front()}\")"), "E0240");
    assert!(msg.ends_with("and its value is used here"), "{msg}");
}

/// A receiver reached through an alias, and a store `const` imported from
/// a sibling under either import spelling, are judged the same way.
#[test]
fn a_stdlib_receiver_through_an_alias_or_an_import_is_judged() {
    let src = "module main\n\
        import std/set\n\
        type Seen = set.Set<string>\n\
        fn note(s: Seen) -> void {\n\
        \x20 s.add(\"x\")\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 note(set.create<string>())\n\
        \x20 return 0\n\
        }\n";
    one(src, "E0240");
    let lib = "module lib\nimport std/store\npub const HITS = store.create(0)\n";
    for (import, call) in [("import lib { HITS }", "HITS.set(1)"), ("import lib", "lib.HITS.set(1)")] {
        let main = format!(
            "module main\n{import}\nfn main(argv: Array<string>) -> number {{\n\x20 {call}\n\x20 return 0\n}}\n"
        );
        let dir = project_modules("import", &[("lib.glyph", lib), ("main.glyph", &main)]);
        let found: Vec<String> =
            errors(&check(&dir, true)).iter().map(|d| d["code"].as_str().unwrap_or("").to_string()).collect();
        assert_eq!(found, ["E0240"], "{import}");
    }
}

/// `mut` before a member that changes nothing is E0241: the readers of the
/// three state holders and every member of a `decimal.Decimal`, which is
/// immutable, so `mut d.add(e)` throws the sum away.
#[test]
fn mut_on_a_stdlib_reader_or_any_decimal_member_is_e0241() {
    for (recv, call) in [
        ("seen", "has(1)"),
        ("seen", "size()"),
        ("seen", "values()"),
        ("st", "get()"),
        ("q", "peek_front()"),
        ("q", "peek_back()"),
        ("q", "len()"),
        ("q", "values()"),
    ] {
        let (msg, help) = one(&stdlib_program(&format!("  mut {recv}.{call}")), "E0241");
        assert!(msg.ends_with("this `mut` marks no change and the result is discarded"), "{recv}.{call}: {msg}");
        assert_eq!(help, format!("drop the `mut`; to keep the result, bind it: `let result = {recv}.{call}`"));
    }
    let decimal = |line: &str| {
        format!(
            "module main\n\
             import std/decimal\n\
             fn f(d: decimal.Decimal, e: decimal.Decimal) -> number {{\n\
             {line}\n\
             \x20 return 0\n\
             }}\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 return 0\n\
             }}\n"
        )
    };
    for (call, own) in [
        ("add(e)", true),
        ("sub(e)", true),
        ("mul(e)", true),
        ("div(e, 2)", true),
        ("round(1)", true),
        ("neg()", true),
        ("abs()", true),
        ("cmp(e)", false),
        ("eq(e)", false),
        ("is_zero()", false),
        ("is_negative()", false),
        ("scale()", false),
        ("to_string()", false),
        ("to_number()", false),
    ] {
        let (msg, help) = one(&decimal(&format!("  mut d.{call}")), "E0241");
        let name = call.split('(').next().unwrap();
        match own {
            true => {
                assert_eq!(
                    msg,
                    format!("`{name}` returns a new `decimal.Decimal` and leaves `d` unchanged; this `mut` marks no change and the result is discarded")
                );
                assert_eq!(help, format!("to keep it, write `mut d = d.{call}`"));
            }
            false => {
                assert_eq!(
                    msg,
                    format!("`{name}` reads `d` and leaves it unchanged; this `mut` marks no change and the result is discarded")
                );
            }
        }
    }
    let applied = "module main\n\
        import std/decimal\n\
        fn f(d: decimal.Decimal, e: decimal.Decimal) -> string {\n\
        \x20 mut d = d.add(e)\n\
        \x20 return d.to_string()\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 return 0\n\
        }\n";
    assert_accepted("decimal-help", applied);
}

/// Every `sqlite.Db` member is Outside: its writes land in a database, not
/// in the program's data, so it is judged in neither direction.
#[test]
fn a_database_member_is_judged_in_neither_direction() {
    for line in [
        "  mut db.exec(\"select 1\")",
        "  db.exec(\"select 1\")",
        "  mut db.run(\"delete from t\", [])",
        "  db.run(\"delete from t\", [])",
        "  mut db.close()",
        "  db.close()",
        "  mut db.last_insert_id()",
    ] {
        let src = format!(
            "module main\n\
             import std/sqlite\n\
             fn f(db: sqlite.Db) -> number {{\n\
             {line}\n\
             \x20 return 0\n\
             }}\n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 return 0\n\
             }}\n"
        );
        let found = refusals(&src);
        assert!(found.is_empty(), "{line}: {found:#?}");
    }
}

/// The help for a writing member whose value is read names the reader that
/// gives the same answer before the change; a member that returns nothing
/// has only the statement form (record 157, section 2b).
#[test]
fn stdlib_value_helps_read_first_or_say_there_is_no_value() {
    let cases = [
        (
            "  let gone = seen.remove(1)\n  print(\"${gone}\")",
            "read whether it is there first with `let had = seen.has(1)`, which is the value `remove` returns, then write `mut seen.remove(1)` as a statement",
        ),
        (
            "  let first = q.pop_front()\n  print(\"${q.len()}\")",
            "read the front first with `let front = q.peek_front()`, which is the value `pop_front` returns, then write `mut q.pop_front()` as a statement",
        ),
        (
            "  let last = q.pop_back()\n  print(\"${q.len()}\")",
            "read the back first with `let back = q.peek_back()`, which is the value `pop_back` returns, then write `mut q.pop_back()` as a statement",
        ),
        (
            "  let r = st.set(3)",
            "`set` returns nothing, so there is no value to use: write `mut st.set(3)` as a statement",
        ),
        (
            "  let r = seen.add(3)",
            "`add` returns nothing, so there is no value to use: write `mut seen.add(3)` as a statement",
        ),
    ];
    for (line, want) in cases {
        let found = refusals(&stdlib_program(line));
        let e = found.iter().find(|f| f.0 == "E0240").unwrap_or_else(|| panic!("{line}: {found:#?}"));
        assert_eq!(e.2, want, "{line}");
    }
    let tails = "module main\n\
        import std/set\n\
        import std/collections\n\
        fn tail_pop(q: collections.Deque<number>) -> Option<number> {\n\
        \x20 q.pop_front()\n\
        }\n\
        fn tail_rm(s: set.Set<string>) -> bool {\n\
        \x20 s.remove(\"a\")\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 return 0\n\
        }\n";
    let helps: Vec<String> = refusals(tails).into_iter().filter(|f| f.0 == "E0240").map(|f| f.2).collect();
    assert_eq!(
        helps,
        [
            "read the front first, `let front = q.peek_front()`, then write `mut q.pop_front()` as a statement and return `front`",
            "read whether it is there first, `let had = s.has(\"a\")`, then write `mut s.remove(\"a\")` as a statement and return `had`",
        ]
    );
}

/// `HITS.get().push(1)` changes the store through the array `get()` hands
/// back. It stays E0240 under `mut` (the receiver is not a place), and the
/// message says the store holds the array rather than that nothing keeps
/// it; the help writes the change through the store's `update`.
#[test]
fn a_write_through_a_stores_array_says_the_store_holds_it() {
    let src = |line: &str| {
        format!(
            "module main\n\
             import std/store\n\
             const HITS = store.create([3, 1])\n\
             fn main(argv: Array<string>) -> number {{\n\
             {line}\n\
             \x20 print(\"${{HITS.get().length}}\")\n\
             \x20 return 0\n\
             }}\n"
        )
    };
    for line in ["  mut HITS.get().push(1)", "  HITS.get().push(1)"] {
        let (msg, help) = one(&src(line), "E0240");
        assert_eq!(
            msg,
            "`.push(1)` adds to the array `HITS.get()` returns, which `HITS` still holds, so it changes `HITS` with no `mut` line naming it"
        );
        assert!(!msg.contains("nothing keeps"), "{msg}");
        assert_eq!(
            help,
            "write the change through the store: `mut HITS.update(fn(current) { array.push(current, 1) })` (with `import std/array`)"
        );
    }
    let (_, help) = one(&src("  mut HITS.get().sort()"), "E0240");
    assert_eq!(
        help,
        "write the change through the store: `mut HITS.update(fn(current) { array.sort(current, fn(a: number, b: number) -> number { return a - b }) })` (with `import std/array`)"
    );
    // A set no name holds gets the bind-first help whether or not its value
    // is read: a read-first help would call `mk()` a second time.
    let fresh = "module main\n\
        import std/set\n\
        fn mk() -> set.Set<number> {\n\
        \x20 return set.create<number>([1])\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let gone = mk().remove(1)\n\
        \x20 print(\"${gone}\")\n\
        \x20 return 0\n\
        }\n";
    let (msg, help) = one(fresh, "E0240");
    assert_eq!(
        msg,
        "`.remove(1)` changes the set `mk()` returns, and a call result is not a place, so no `mut` line names what changes"
    );
    assert_eq!(help, "bind it with `let` first, `let s = mk()`, then write `mut s.remove(1)`");
    // A set's `values()` is a copy, so a change to it is lost, as the
    // message for a fresh array says.
    let copy = "module main\n\
        import std/set\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let s = set.create<number>([1])\n\
        \x20 mut s.values().push(2)\n\
        \x20 return 0\n\
        }\n";
    let (msg, _) = one(copy, "E0240");
    assert!(msg.contains("nothing keeps it"), "{msg}");
}

/// A set or deque a store holds is written through the store too (record
/// 171, F2): `ST.get().add(2)` says `ST` still holds the set, and the help
/// writes `mut ST.update(...)` with the change as a `mut` statement inside
/// the callback, never `let s = ST.get()`, the G394 alias write. The help's
/// program, on separate lines as Glyph needs, passes the full check and
/// runs.
#[test]
fn a_write_through_a_stores_set_or_deque_says_the_store_holds_it() {
    let src = |line: &str| {
        format!(
            "module main\n\
             import std/set\n\
             import std/store\n\
             import std/collections\n\
             const ST = store.create(set.create([1]))\n\
             const SQ = store.create(collections.deque([1]))\n\
             fn main(argv: Array<string>) -> number {{\n\
             {line}\n\
             \x20 print(\"${{ST.get().size()}} ${{SQ.get().len()}}\")\n\
             \x20 return 0\n\
             }}\n"
        )
    };
    for line in ["  ST.get().add(2)", "  mut ST.get().add(2)", "  let had = ST.get().remove(1)"] {
        let found = refusals(&src(line));
        let e = found.iter().find(|f| f.0 == "E0240").unwrap_or_else(|| panic!("{line}: {found:#?}"));
        assert!(e.1.ends_with("which `ST` still holds, so it changes `ST` with no `mut` line naming it"), "{line}: {}", e.1);
        assert!(e.2.starts_with("write the change through the store"), "{line}: {}", e.2);
        assert!(!e.2.contains("let s ="), "{line}: {}", e.2);
    }
    let (msg, help) = one(&src("  SQ.get().push_back(2)"), "E0240");
    assert_eq!(
        msg,
        "`.push_back(2)` changes the deque `SQ.get()` returns, which `SQ` still holds, so it changes `SQ` with no `mut` line naming it"
    );
    assert_eq!(
        help,
        "write the change through the store, as a `mut` statement inside the callback that then returns the deque: `mut SQ.update(fn(current) {`, then `mut current.push_back(2)` and `current` on lines of their own, then `})`"
    );
    let applied = src(
        "  mut ST.update(fn(current) {\n    mut current.add(2)\n    current\n  })\n  \
         mut SQ.update(fn(current) {\n    mut current.push_back(2)\n    current\n  })",
    );
    let dir = project("held-stdlib", &applied);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{applied}\n{v:#}");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["run", "src/main.glyph"])
        .output()
        .expect("run glyph run");
    assert!(String::from_utf8_lossy(&out.stdout).contains("2 2"), "{}", String::from_utf8_lossy(&out.stdout));
}

/// E0241's help never reassigns a name that cannot be reassigned (record
/// 171, F1): a module `const` is E0212, and an imported name belongs to
/// another module, so for a `Decimal` and an array alike the help binds the
/// result. A local keeps the reassignment. Each help, applied, passes the
/// full check.
#[test]
fn e0241_on_a_const_or_imported_receiver_binds_the_result() {
    let lib = "module lib\npub const NUMS = [1, 2]\n";
    let main = |body: &str| {
        format!(
            "module main\n\
             import std/decimal\n\
             import lib\n\
             import lib {{ NUMS }}\n\
             const ONE = decimal.from_int(1, 0)\n\
             const ITEMS = [1, 2]\n\
             fn main(argv: Array<string>) -> number {{\n\
             {body}\n\
             \x20 return 0\n\
             }}\n"
        )
    };
    let cases = [
        ("  mut ONE.add(ONE)", "to keep it, bind it: `let result = ONE.add(ONE)`"),
        ("  mut ITEMS.concat(NUMS)", "to keep it, bind it: `let result = ITEMS.concat(NUMS)`"),
        ("  mut NUMS.concat(ITEMS)", "to keep it, bind it: `let result = NUMS.concat(ITEMS)`"),
        ("  mut lib.NUMS.concat(ITEMS)", "to keep it, bind it: `let result = lib.NUMS.concat(ITEMS)`"),
        ("  let local = [1]\n  mut local.concat(ITEMS)", "to keep it, write `mut local = local.concat(ITEMS)`"),
    ];
    for (body, want) in cases {
        let dir = project_modules("const-help", &[("lib.glyph", lib), ("main.glyph", &main(body))]);
        let found: Vec<(String, String)> = errors(&check(&dir, true))
            .iter()
            .map(|d| (d["code"].as_str().unwrap_or("").to_string(), d["help"].as_str().unwrap_or("").to_string()))
            .collect();
        assert_eq!(found, [("E0241".to_string(), want.to_string())], "{body}");
    }
    let applied = main(
        "  let result = ONE.add(ONE)\n  let r2 = ITEMS.concat(NUMS)\n  let r3 = lib.NUMS.concat(ITEMS)\n  \
         print(\"${result.to_string()} ${r2.length} ${r3.length}\")",
    );
    let dir = project_modules("const-applied", &[("lib.glyph", lib), ("main.glyph", &applied)]);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{applied}\n{v:#}");
}

/// `let _ = <mutating call>` throws the value away, so its help writes only
/// the statement form, for an array as for a deque; the read-first help
/// would be advice for a value nobody keeps.
#[test]
fn a_discarded_let_gets_the_statement_help() {
    let (msg, help) = one(&program("  let _ = xs.pop()"), "E0240");
    assert_eq!(msg, "`xs.pop()` changes `xs` in place, and `let _ =` throws its value away");
    assert_eq!(
        help,
        "write `mut xs.pop()` in place of the `let _ =`: the value was discarded, so the line does the same, and `grep \"^\\s*mut \"` finds the change"
    );
    let found = refusals(&stdlib_program("  let _ = q.pop_front()"));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].2.starts_with("write `mut q.pop_front()` in place of the `let _ =`"), "{found:#?}");
}

/// Each stdlib help spelling, written out, passes the full check and runs.
#[test]
fn every_stdlib_help_spelling_compiles_and_runs() {
    let src = "module main\n\
        \n\
        import std/io\n\
        import std/array\n\
        import std/set\n\
        import std/store\n\
        import std/collections\n\
        import std/decimal\n\
        \n\
        const HITS = store.create([3, 1])\n\
        \n\
        fn mk() -> set.Set<string> {\n\
        \x20 return set.create<string>()\n\
        }\n\
        \n\
        fn tail_pop(q: collections.Deque<number>) -> Option<number> {\n\
        \x20 let front = q.peek_front()\n\
        \x20 mut q.pop_front()\n\
        \x20 return front\n\
        }\n\
        \n\
        fn tail_rm(s: set.Set<string>) -> bool {\n\
        \x20 let had = s.has(\"a\")\n\
        \x20 mut s.remove(\"a\")\n\
        \x20 return had\n\
        }\n\
        \n\
        fn bump(d: decimal.Decimal) -> string {\n\
        \x20 mut d = d.add(d)\n\
        \x20 return d.to_string()\n\
        }\n\
        \n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let seen = set.create<string>([\"a\"])\n\
        \x20 let had = seen.has(\"a\")\n\
        \x20 mut seen.remove(\"a\")\n\
        \x20 let q = collections.deque<number>([1, 2, 3])\n\
        \x20 let front = q.peek_front()\n\
        \x20 mut q.pop_front()\n\
        \x20 let back = q.peek_back()\n\
        \x20 mut q.pop_back()\n\
        \x20 mut HITS.update(fn(current) { array.push(current, 1) })\n\
        \x20 mut HITS.update(fn(current) { array.sort(current, fn(a: number, b: number) -> number { return a - b }) })\n\
        \x20 mut HITS.update(fn(current) { array.reverse(current) })\n\
        \x20 mut HITS.update(fn(current) { array.concat([0], current) })\n\
        \x20 let s = mk()\n\
        \x20 mut s.add(\"y\")\n\
        \x20 let one = match decimal.decimal(\"1.5\") { Ok(d) => bump(d), Err(_) => \"\", }\n\
        \x20 let f = match front { Some(v) => v, None => 0, }\n\
        \x20 let b = match back { Some(v) => v, None => 0, }\n\
        \x20 let t = match tail_pop(q) { Some(v) => v, None => 0, }\n\
        \x20 io.println(\"${had} ${f} ${b} ${t} ${HITS.get().length} ${HITS.get()[0]} ${s.size()} ${one} ${tail_rm(seen)}\")\n\
        \x20 return 0\n\
        }\n";
    let dir = project("stdlib-help", src);
    let v = check(&dir, false);
    assert!(v["ok"] == true && errors(&v).is_empty(), "{v:#}");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["run", "src/main.glyph"])
        .output()
        .expect("run glyph run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("true 1 3 2 4 0 1 3.0 false"),
        "unexpected output:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
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
        \x20 let plain = array.sort([3, 1], fn(a: number, b: number) -> number { return a - b })\n\
        \x20 let words = array.sort([\"b\", \"a\"], fn(a: string, b: string) -> number { return a.localeCompare(b) })\n\
        \x20 let shown = match last { Some(v) => v, None => 0, }\n\
        \x20 let shown2 = match first { Some(v) => v, None => 0, }\n\
        \x20 io.println(\"${shown} ${shown2} ${removed.length} ${n} ${sorted.length} ${back.length} ${s} ${xs[0]} ${pushed.length} ${front.length} ${plain[0]} ${words[0]}\")\n\
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
        stdout.contains("2 3 1 1 1 1 4 7 2 2 1 a"),
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

/// No help puts `...` inside a code span, where it reads as a spread, and
/// none names a comparator the program does not have (record 151, F5).
#[test]
fn helps_quote_no_elision_and_no_invented_comparator() {
    let src = program(
        "  let ws = [\"b\", \"a\"]\n  \
         let _a = xs.sort()\n  \
         let _b = ws.sort()\n  \
         let _c = xs.sort(fn(a: number, b: number) -> number { return a - b })\n  \
         let _d = xs.filter(fn(x: number) -> bool { return x > 1 }).reverse()\n  \
         mut xs.filter(fn(x: number) -> bool { return x > 1 }).push(9)",
    );
    let found = refusals(&src);
    assert_eq!(found.len(), 5, "{found:#?}");
    for (_, msg, help) in &found {
        for text in [msg, help] {
            for span in text.split('`').skip(1).step_by(2) {
                assert!(!span.contains("..."), "a code span holds `...`: {text}");
            }
        }
        assert!(!help.contains("cmp"), "an invented comparator: {help}");
    }
    let help_of = |subject: &str| {
        found
            .iter()
            .find(|f| f.1.starts_with(subject))
            .map(|f| f.2.clone())
            .unwrap_or_else(|| panic!("no refusal of {subject}: {found:#?}"))
    };
    assert!(
        help_of("`xs.sort()`").contains("`array.sort(xs, fn(a: number, b: number) -> number { return a - b })`"),
        "{found:#?}"
    );
    assert!(
        help_of("`ws.sort()`").contains("`array.sort(ws, fn(a: string, b: string) -> number { return a.localeCompare(b) })`"),
        "{found:#?}"
    );
    assert!(
        help_of("the call to `xs.sort`").starts_with("`array.sort` with the same comparator returns a sorted copy"),
        "{found:#?}"
    );
}

/// A block's last expression is its value when the block's value is used,
/// so a mutating call there is a value, not a statement, and `glyph fix`
/// leaves it alone (record 151, F1). A `void` body's tail is a statement.
#[test]
fn a_consumed_tail_is_a_value_and_a_void_tail_is_a_statement() {
    let returned = "module main\n\
        fn f(ys: Array<number>) -> number {\n\
        \x20 ys.push(4)\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 print(\"${f([1])}\")\n\
        \x20 return 0\n\
        }\n";
    let (msg, help) = one(returned, "E0240");
    assert_eq!(msg, "`ys.push(4)` changes `ys` in place, and as the last expression of the body its value is returned");
    assert_eq!(
        help,
        "write `mut ys.push(4)` as a statement, then return the value explicitly, `return array.len(ys)` (with `import std/array`)"
    );
    let applied = returned
        .replace("  ys.push(4)\n", "  mut ys.push(4)\n  return array.len(ys)\n")
        .replacen("module main\n", "module main\nimport std/array\n", 1);
    assert_accepted("tail-applied", &applied);

    let tail_match = "module main\n\
        fn f(c: bool, ys: Array<number>) -> number {\n\
        \x20 match c {\n\
        \x20   true => ys.push(1),\n\
        \x20   false => 0,\n\
        \x20 }\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 print(\"${f(true, [1])}\")\n\
        \x20 return 0\n\
        }\n";
    let (msg, help) = one(tail_match, "E0240");
    assert!(msg.ends_with("its value is used here"), "{msg}");
    assert!(!help.contains("{ mut"), "a tail match arm is not a statement arm: {help}");

    let lambda = program("  let _lens = array.map(xs, fn(x: number) -> number { ys.push(x) })")
        .replacen("module main\n", "module main\nimport std/array\n", 1);
    let (msg, _) = one(&lambda, "E0240");
    assert!(msg.ends_with("its value is returned"), "{msg}");

    let void_tail = "module main\n\
        fn f(ys: Array<number>) -> void {\n\
        \x20 ys.push(4)\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 f([1])\n\
        \x20 return 0\n\
        }\n";
    let (msg, _) = one(void_tail, "E0240");
    assert!(msg.ends_with("written as a `mut` statement"), "{msg}");

    let arm_block = program("  let _n = match xs.length > 1 {\n    true => { xs.push(1) },\n    false => 0,\n  }");
    let (msg, help) = one(&arm_block, "E0240");
    assert!(msg.ends_with("as the last expression of the arm its value is used"), "{msg}");
    assert!(help.contains("end the block with the value, `array.len(xs)`"), "{help}");
}

/// A receiver reached through an array index is judged: the element of an
/// `Array<E>` is an `E` for this rule (record 151, F2).
#[test]
fn a_receiver_reached_through_an_index_is_judged() {
    let head = "module main\n\
        type Row = { items: Array<number>, }\n\
        fn cmp(a: number, b: number) -> number { return a - b }\n\
        fn first_sorted(rows: Array<Row>) -> number {\n\
        \x20 return rows[0].items.sort(cmp)[0]\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let rows: Array<Row> = [{ items: [2, 1], }]\n\
        \x20 let grid = [[1], [2]]\n\
        \x20 grid[1].push(6)\n\
        \x20 rows[0].items.push(0)\n\
        \x20 print(\"${first_sorted(rows)} ${grid.length}\")\n\
        \x20 return 0\n\
        }\n";
    let found = refusals(head);
    let lines: Vec<&str> = found.iter().filter(|f| f.0 == "E0240").map(|f| f.1.as_str()).collect();
    assert_eq!(lines.len(), 3, "{found:#?}");
    assert!(lines[0].starts_with("`rows[0].items.sort(cmp)` changes `rows[0].items`"), "{lines:?}");
    assert!(lines[1].starts_with("`grid[1].push(6)` changes `grid[1]`"), "{lines:?}");
    assert!(lines[2].starts_with("`rows[0].items.push(0)`"), "{lines:?}");
    assert_accepted(
        "index-mut",
        "module main\n\
         type Row = { items: Array<number>, }\n\
         fn main(argv: Array<string>) -> number {\n\
         \x20 let rows: Array<Row> = [{ items: [2, 1], }]\n\
         \x20 let grid = [[1], [2]]\n\
         \x20 let i = 1\n\
         \x20 mut grid[i].push(6)\n\
         \x20 mut rows[0].items.push(0)\n\
         \x20 print(\"${grid.length} ${rows.length}\")\n\
         \x20 return 0\n\
         }\n",
    );
}

/// A write through an optional field is spelled `mut s.opt?.push(1)`, the
/// one spelling `tsc` accepts; assignment through `?.` stays refused
/// (record 151, F3).
#[test]
fn a_write_through_an_optional_field_takes_mut_before_the_optional_call() {
    let src = "module main\n\
        type S = { opt?: Array<number>, }\n\
        fn f(s: S) -> void {\n\
        \x20 s.opt?.push(1)\n\
        }\n\
        fn main(argv: Array<string>) -> number {\n\
        \x20 let xs = [3]\n\
        \x20 f({ opt: xs, })\n\
        \x20 print(\"${xs.length}\")\n\
        \x20 return 0\n\
        }\n";
    let (msg, help) = one(src, "E0240");
    assert!(msg.starts_with("`s.opt?.push(1)` changes `s.opt` in place"), "{msg}");
    assert!(help.starts_with("write `mut s.opt?.push(1)`"), "{help}");
    let fixed = src.replace("  s.opt?.push(1)\n", "  mut s.opt?.push(1)\n");
    assert_accepted("optional-mut", &fixed);

    let assign = src.replace("  s.opt?.push(1)\n", "  mut s.opt?.n = 1\n");
    let v = check(&project("optional-assign", &assign), true);
    let errs = errors(&v);
    assert!(errs.iter().any(|e| e["code"] == "E0002"), "{v:#}");
}

/// A deferred mutating call gets the spelling that keeps it deferred: the
/// change in a function, and `defer` calling it (record 151, F4).
#[test]
fn a_deferred_mutating_call_is_written_through_a_function() {
    let (msg, help) = one(&program("  defer xs.push(1)"), "E0240");
    assert_eq!(msg, "`xs.push(1)` is a deferred call that changes `xs`, and `defer` takes no `mut`");
    assert_eq!(
        help,
        "write the change in a function and defer the call: `let undo = fn() -> void { mut xs.push(1) }`, then `defer undo()`"
    );
    assert_accepted(
        "defer-applied",
        &program("  let undo = fn() -> void {\n    mut xs.push(1)\n  }\n  defer undo()"),
    );
}
