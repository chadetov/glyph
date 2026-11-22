//! The stdlib `Set`, `Store` and `Deque` are typed by Glyph's checker (G423,
//! decision record 157 section 2a).
//!
//! Before 0.1.163 a value of any of the three was `Unknown`, so a wrong
//! argument, an unknown member, a mismatched read and the value itself where a
//! `number` was declared all passed `glyph check --no-tsc` and only `tsc`
//! refused them. Each refusal here is one `tsc` makes too (TS2322, TS2345,
//! TS2339) or D54's, and each test runs the binary the way an agent does.
//!
//! What these types change about mutation is the next release's: a call that
//! changes a `Set` is not judged by E0240 or E0241 yet, and
//! `mutation_needs_mut.rs` pins that edge.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn project(src: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("glyph_state_types_{}_{}", std::process::id(), n));
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

/// A module importing the three state modules, with `body` as `main`'s.
fn program(body: &str) -> String {
    format!(
        "module main\n\
         \n\
         import std/io\n\
         import std/set\n\
         import std/store\n\
         import std/collections\n\
         \n\
         type Holder = {{ s: store.Store<number>, }}\n\
         \n\
         const HITS: store.Store<number> = store.create(0)\n\
         \n\
         fn main(argv: Array<string>) -> number {{\n\
         \x20 let seen = set.create<string>()\n\
         \x20 let q = collections.deque<number>()\n\
         {body}\n\
         \x20 io.println(\"${{seen.size()}} ${{q.len()}} ${{HITS.get()}}\")\n\
         \x20 return 0\n\
         }}\n"
    )
}

/// Each line `tsc` refuses in decision record 157's probe b01, alone in a
/// program, is one Glyph refusal under `--no-tsc` with exactly this code and
/// message. The `tsc` code each one matches is in the comment beside it.
#[test]
fn every_b01_line_tsc_refuses_is_a_glyph_refusal() {
    let cases: [(&str, &str, &str); 7] = [
        // TS2322: Type 'Set<string>' is not assignable to type 'number'.
        (
            "  let a: number = set.create<string>()\n  io.println(\"${a}\")",
            "E0204",
            "type mismatch: expected `number`, found `set.Set<string>`",
        ),
        // TS2322: Type 'Store<number>' is not assignable to type 'number'.
        (
            "  let b: number = store.create(0)\n  io.println(\"${b}\")",
            "E0204",
            "type mismatch: expected `number`, found `store.Store<number>`",
        ),
        // TS2322: Type 'Deque<number>' is not assignable to type 'number'.
        (
            "  let c: number = collections.deque<number>()\n  io.println(\"${c}\")",
            "E0204",
            "type mismatch: expected `number`, found `collections.Deque<number>`",
        ),
        // TS2322: Type 'number' is not assignable to type 'string'. The probe
        // reads an unannotated `const`; see the test below for that spelling.
        (
            "  let wrong: string = HITS.get()\n  io.println(wrong)",
            "E0204",
            "type mismatch: expected `string`, found `number`",
        ),
        // TS2345: Argument of type 'number' is not assignable to parameter of
        // type 'string'.
        (
            "  seen.add(5)",
            "E0211",
            "argument type mismatch: expected `string`, found `number`",
        ),
        // TS2339: Property 'nope' does not exist on type 'Set<string>'.
        (
            "  let z = seen.nope()\n  io.inspect(z)",
            "E0210",
            "type `set.Set<string>` has no field `nope`",
        ),
        // A field declared `store.Store<number>` lowers to it, so a write
        // through the field path is checked too (TS2345 under `tsc`).
        (
            "  let h: Holder = { s: store.create(1), }\n  h.s.set(\"x\")",
            "E0211",
            "argument type mismatch: expected `number`, found `string`",
        ),
    ];
    for (body, code, message) in cases {
        let src = program(body);
        let found = refusals(&src);
        assert_eq!(
            found,
            vec![(code.to_string(), message.to_string())],
            "for:\n{src}"
        );
    }
}

/// The lines of b01 `tsc` accepts stay accepted here, under `--no-tsc` and
/// under the full check: each is a correct use of the member it calls. The
/// `mut` before `seen.add` and `seen.has` is not judged in this release.
#[test]
fn the_correct_uses_of_each_member_are_accepted() {
    let src = program(
        "  seen.add(\"a\")\n\
         \x20 mut seen.add(\"b\")\n\
         \x20 mut seen.has(\"a\")\n\
         \x20 let gone: bool = seen.remove(\"a\")\n\
         \x20 let all: Array<string> = seen.values()\n\
         \x20 q.push_back(1)\n\
         \x20 q.push_front(0)\n\
         \x20 let first: number = match q.pop_front() { Some(v) => v, None => 0, }\n\
         \x20 let last: number = match q.peek_back() { Some(v) => v, None => 0, }\n\
         \x20 let more: Array<number> = q.values()\n\
         \x20 HITS.set(3)\n\
         \x20 mut HITS.update(fn(n) { return n + 1 })\n\
         \x20 let h: Holder = { s: store.create(1), }\n\
         \x20 h.s.set(2)\n\
         \x20 let label = store.create(\"a\")\n\
         \x20 label.set(\"b\")\n\
         \x20 let uniq: Array<string> = set.unique([\"a\", \"a\"])\n\
         \x20 io.println(\"${gone} ${all.length} ${first} ${last} ${more.length} ${label.get()} ${uniq.length}\")",
    );
    assert_eq!(refusals(&src), Vec::<(String, String)>::new(), "{src}");
    let full = check(&project(&src), false);
    assert_eq!(full["ok"], true, "the full check refused it:\n{src}\n{full:#}");
}

/// `store.create("a")` holds a `string`, not the literal `"a"`, which is the
/// binding `tsc` infers; anything else would refuse `s.set("b")`.
#[test]
fn a_fresh_literal_argument_binds_its_widened_type() {
    let found = refusals(&program(
        "  let s = store.create(\"a\")\n  s.set(\"b\")\n  let n: number = s.get()\n  io.println(\"${n}\")",
    ));
    assert_eq!(
        found,
        vec![("E0204".to_string(), "type mismatch: expected `number`, found `string`".to_string())]
    );
}

/// With no type argument and no argument, `T` stays undecided: no member's
/// argument is refused, and the type renders `?` where it is undecided.
#[test]
fn set_create_with_nothing_to_bind_stays_undecided() {
    let found = refusals(&program(
        "  let u = set.create()\n  u.add(5)\n  u.add(\"x\")\n  let n: number = u\n  io.println(\"${n}\")",
    ));
    assert_eq!(
        found,
        vec![("E0204".to_string(), "type mismatch: expected `number`, found `set.Set<?>`".to_string())]
    );
}

/// `update` takes a `fn(T) -> T`, so an unannotated lambda's parameter is the
/// store's `T` (D55), and a lambda declared to return another type is refused.
#[test]
fn update_types_its_lambda_from_the_position() {
    let found = refusals(&program(
        "  HITS.update(fn(n) {\n    let s: string = n\n    return n\n  })",
    ));
    assert_eq!(
        found,
        vec![("E0204".to_string(), "type mismatch: expected `string`, found `number`".to_string())]
    );
    let found = refusals(&program("  HITS.update(fn(n) -> string { return \"x\" })"));
    assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["E0211"]);
}

/// The three spellings of the type agree: the namespace path, the named
/// import, and a different element type is a different type.
#[test]
fn the_written_annotation_lowers_by_either_spelling() {
    let src = "module main\n\
               \n\
               import std/set\n\
               import std/set { Set }\n\
               \n\
               fn take(s: Set<string>) -> number { return s.size() }\n\
               \n\
               fn main(argv: Array<string>) -> number {\n\
               \x20 let a: set.Set<string> = set.create<number>()\n\
               \x20 return take(a) + take(set.create<number>())\n\
               }\n";
    let found = refusals(src);
    assert_eq!(
        found,
        vec![
            (
                "E0204".to_string(),
                "type mismatch: expected `set.Set<string>`, found `set.Set<number>`".to_string()
            ),
            (
                "E0211".to_string(),
                "argument type mismatch: expected `set.Set<string>`, found `set.Set<number>`"
                    .to_string()
            ),
        ],
        "{src}"
    );
}

/// D54: a `Set` has no text form, and `${seen}` prints `[object Object]`.
/// This is the one refusal here `tsc` does not make.
#[test]
fn interpolating_a_set_is_refused() {
    let found = refusals(&program("  io.println(\"${seen}\")"));
    assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["E0237"], "{found:?}");
    assert!(found[0].1.contains("`set.Set<string>`"), "{found:?}");
}

/// An unannotated `const` is `Unknown` by a standing rule (an annotated one is
/// its annotation), so b01's `const HITS = store.create(0)` spelling still
/// reads nothing off `HITS`. Pinned so the edge is seen when that rule moves.
#[test]
fn an_unannotated_const_store_is_not_typed() {
    let src = "module main\n\
               \n\
               import std/store\n\
               \n\
               const HITS = store.create(0)\n\
               \n\
               fn main(argv: Array<string>) -> number {\n\
               \x20 let wrong: string = HITS.get()\n\
               \x20 print(wrong)\n\
               \x20 return 0\n\
               }\n";
    assert_eq!(refusals(src), Vec::<(String, String)>::new());
}
