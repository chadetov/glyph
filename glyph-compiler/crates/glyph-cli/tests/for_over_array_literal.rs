//! G447. A `for` loop over a written array literal binds the element type the
//! same loop over a `let`-bound array does.
//!
//! Through 0.1.167 the loop widened the element type against the literal
//! itself, which paired an array expression with a type that is not its type.
//! Any one-argument applied element was rebuilt around the literal's elements:
//! `for s in [seen]` bound `s` as `set.Set<set.Set<number>>`, so `s.has(1)`
//! was E0211, and `for w in [[1]]` bound `w` as `Array<Array<number>>`.
//!
//! Each test here states one loop in both spellings, `for x in [..]` and `let
//! xs = [..]` then `for x in xs`, and asserts the two read the same, accepted
//! or refused with the same message.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn project(src: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("glyph_for_literal_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
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

/// Every error in `src` under `--no-tsc`, as `(code, message)`.
fn refusals(src: &str) -> Vec<(String, String)> {
    check(&project(src), true)["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["severity"] == "error")
        .map(|d| {
            (
                d["code"].as_str().unwrap_or("").to_string(),
                d["message"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

const PRELUDE: &str = "module main\n\
                       \n\
                       import std/io\n\
                       import std/set\n\
                       import std/store\n\
                       \n\
                       type Point = { x: number, y: number, }\n\
                       \n";

/// `main` with `setup` first, then a loop over `literal` binding `x` and
/// running `body`, written once over the literal and once over a `let`.
fn both_spellings(setup: &str, literal: &str, body: &str) -> (String, String) {
    let direct = format!(
        "{PRELUDE}fn main() {{\n{setup}\n  for x in {literal} {{\n{body}\n  }}\n}}\n"
    );
    let named = format!(
        "{PRELUDE}fn main() {{\n{setup}\n  let xs = {literal}\n  for x in xs {{\n{body}\n  }}\n}}\n"
    );
    (direct, named)
}

/// The loop over the literal reads exactly as the loop over the bound array,
/// and that reading is `expected`.
fn reads_as_named(setup: &str, literal: &str, body: &str, expected: &[(&str, &str)]) {
    let (direct, named) = both_spellings(setup, literal, body);
    let expected: Vec<(String, String)> =
        expected.iter().map(|(c, m)| (c.to_string(), m.to_string())).collect();
    assert_eq!(refusals(&named), expected, "the let-bound loop:\n{named}");
    assert_eq!(refusals(&direct), expected, "the literal loop:\n{direct}");
}

/// Record 171's probe a08: a set in a literal is iterated as a set.
#[test]
fn a_set_in_an_array_literal_is_iterated_as_a_set() {
    reads_as_named(
        "  let seen = set.create([1])",
        "[seen]",
        "    let c = x.has(1)\n    io.println(\"${c}\")",
        &[],
    );
}

/// Record 171's probe a08: the element of `[[1]]` is `Array<number>`, and the
/// refusal names it.
#[test]
fn a_nested_array_literal_binds_the_inner_array() {
    reads_as_named(
        "",
        "[[1]]",
        "    let k: string = x\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `Array<number>`")],
    );
    reads_as_named(
        "",
        "[[1], [2, 3]]",
        "    let n: number = x.length\n    io.println(\"${n}\")",
        &[],
    );
    reads_as_named(
        "",
        "[[[1]]]",
        "    let k: string = x\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `Array<Array<number>>`")],
    );
}

/// Another stdlib type with one argument, the shape the bug rebuilt.
#[test]
fn a_store_in_an_array_literal_is_iterated_as_a_store() {
    reads_as_named(
        "  let a = store.create(1)\n  let b = store.create(2)",
        "[a, b]",
        "    let n: number = x.get()\n    io.println(\"${n}\")",
        &[],
    );
    reads_as_named(
        "  let a = store.create(1)",
        "[a]",
        "    let k: string = x.get()\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `number`")],
    );
}

/// An `Option` element is an applied type with one argument too.
#[test]
fn an_option_in_an_array_literal_is_iterated_as_an_option() {
    reads_as_named(
        "  let o: Option<number> = Some(1)",
        "[o, o]",
        "    let n: number = match x {\n      Some(v) => v,\n      None => 0,\n    }\n    io.println(\"${n}\")",
        &[],
    );
    reads_as_named(
        "  let o: Option<number> = Some(1)",
        "[o]",
        "    let k: string = x\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `Option<number>`")],
    );
}

/// A record element, written out and through a name.
#[test]
fn a_record_in_an_array_literal_is_iterated_as_the_record() {
    reads_as_named(
        "  let p: Point = { x: 1, y: 2, }",
        "[p, p]",
        "    let n: number = x.x + x.y\n    io.println(\"${n}\")",
        &[],
    );
    reads_as_named(
        "  let p: Point = { x: 1, y: 2, }",
        "[p]",
        "    let k: string = x.x\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `number`")],
    );
}

/// Fresh string literals still widen to `string` (G237), so the loop over a
/// written list of words is a loop over strings in both spellings.
#[test]
fn string_literals_still_widen() {
    reads_as_named(
        "",
        "[\"read\", \"write\"]",
        "    let n: number = x\n    io.println(\"${n}\")",
        &[("E0204", "type mismatch: expected `number`, found `string`")],
    );
    // Two inner literals of different words (`[["read"], ["write"]]`) are an
    // array of `Array<?>` at a `let` too, which is G450; the loop reads what
    // the `let` reads either way.
    reads_as_named(
        "",
        "[[\"read\"], [\"read\"]]",
        "    let k: string = x\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `Array<string>`")],
    );
}

/// A literal mixing a written element with a named one.
#[test]
fn a_mixed_literal_reads_as_the_let_bound_array() {
    reads_as_named(
        "  let seen = set.create([1])",
        "[seen, set.create([2])]",
        "    let c = x.has(1)\n    io.println(\"${c}\")",
        &[],
    );
    reads_as_named(
        "  let inner = [1]",
        "[inner, [2, 3]]",
        "    let k: string = x\n    io.println(k)",
        &[("E0204", "type mismatch: expected `string`, found `Array<number>`")],
    );
}

/// The index form types the element binding the same way and leaves the
/// index a number.
#[test]
fn the_index_form_binds_the_element_type() {
    let ok = format!(
        "{PRELUDE}fn main() {{\n  let seen = set.create([1])\n  for i, s in [seen] {{\n    let c = s.has(i)\n    io.println(\"${{c}}\")\n  }}\n}}\n"
    );
    assert_eq!(refusals(&ok), Vec::<(String, String)>::new(), "{ok}");
    let bad = format!(
        "{PRELUDE}fn main() {{\n  for i, w in [[1]] {{\n    let k: string = w\n    io.println(\"${{k}} ${{i}}\")\n  }}\n}}\n"
    );
    assert_eq!(
        refusals(&bad),
        vec![(
            "E0204".to_string(),
            "type mismatch: expected `string`, found `Array<number>`".to_string()
        )],
        "{bad}"
    );
}

/// A wrong argument to the element's own method is still refused, and names
/// the element's type argument rather than the element.
#[test]
fn a_real_mismatch_inside_the_loop_is_still_refused() {
    reads_as_named(
        "  let seen = set.create([1])",
        "[seen]",
        "    let c = x.has(\"a\")\n    io.println(\"${c}\")",
        &[("E0211", "argument type mismatch: expected `number`, found `string`")],
    );
}

/// What the wrong element type let through under `--no-tsc`, and `tsc`
/// refused (TS2345, TS2322): the set passed to its own `has`, and the inner
/// array declared as the outer one. Both are Glyph's refusals now.
#[test]
fn what_the_wrong_element_type_let_through_is_refused() {
    reads_as_named(
        "  let seen = set.create([1])",
        "[seen]",
        "    let c = x.has(seen)\n    io.println(\"${c}\")",
        &[("E0211", "argument type mismatch: expected `number`, found `set.Set<number>`")],
    );
    reads_as_named(
        "",
        "[[1]]",
        "    let bad: Array<Array<number>> = x\n    io.println(\"${bad.length}\")",
        &[(
            "E0204",
            "type mismatch: expected `Array<Array<number>>`, found `Array<number>`",
        )],
    );
}

/// The accepted a08 program passes the full check, `tsc` included.
#[test]
fn the_accepted_loops_pass_the_full_check() {
    let src = format!(
        "{PRELUDE}fn main() {{\n\
         \x20 let seen = set.create([1])\n\
         \x20 for s in [seen] {{\n    let c = s.has(1)\n    io.println(\"${{c}}\")\n  }}\n\
         \x20 for w in [[1]] {{\n    let n: number = w.length\n    io.println(\"${{n}}\")\n  }}\n\
         }}\n"
    );
    assert_eq!(refusals(&src), Vec::<(String, String)>::new(), "{src}");
    let full = check(&project(&src), false);
    assert_eq!(full["ok"], true, "the full check refused it:\n{src}\n{full:#}");
}
