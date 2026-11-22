//! The stdlib `Set`, `Store` and `Deque` are typed by Glyph's checker (G423,
//! decision record 157 section 2a).
//!
//! Before 0.1.163 a value of any of the three was `Unknown`, so a wrong
//! argument, an unknown member, a mismatched read and the value itself where a
//! `number` was declared all passed `glyph check --no-tsc` and only `tsc`
//! refused them. Each refusal here is one `tsc` makes too (TS2322, TS2345,
//! TS2339) or D54's, and each test runs the binary the way an agent does.
//!
//! Record 158 (F-B) widened the object rule to every modeled stdlib type:
//! `decimal.Decimal` and `url.Url` are never a scalar either, and a record the
//! program declares is refused where any of them is declared.
//!
//! What these types change about mutation is G423 step 2's: a call that
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

/// An unannotated `const` has its initializer's type (G433, record 158 F-A),
/// so b01's `const HITS = store.create(0)` spelling is a `store.Store<number>`
/// and `HITS.get()` where a `string` is declared is E0204, as it is for the
/// annotated spelling. The flip of the pin this release left for G433.
#[test]
fn an_unannotated_const_store_is_typed() {
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
    let found = refusals(src);
    assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["E0204"], "{found:?}");
}

/// The three modeled types held by unannotated consts are read as they are in
/// a `let`: a wrong argument is E0211, a wrong read E0204, a `match` over a
/// `pop_front` without `None` E0200, and `${SEEN}` E0237. Ordinary use of
/// the same constants is refused nothing.
#[test]
fn unannotated_const_set_store_and_deque_are_judged_like_lets() {
    let head = "module main\n\nimport std/io\nimport std/set\nimport std/store\nimport std/collections\n\n\
                const SEEN = set.create<string>()\nconst HITS = store.create(0)\nconst Q = collections.deque<number>()\n\n\
                fn main(argv: Array<string>) -> number {\n";
    let ok = format!(
        "{head}  SEEN.add(\"a\")\n  HITS.set(HITS.get() + 1)\n  Q.push_back(1)\n  let n: number = HITS.get()\n  \
         let has = SEEN.has(\"a\")\n  let first = match Q.pop_front() {{\n    Some(v) => v,\n    None => 0,\n  }}\n  \
         HITS.update(fn(v) {{ v + 1 }})\n  io.println(\"${{n}} ${{has}} ${{first}}\")\n  return 0\n}}\n"
    );
    assert_eq!(refusals(&ok), Vec::<(String, String)>::new());
    let bad = format!(
        "{head}  SEEN.add(5)\n  let w: string = HITS.get()\n  let first = match Q.pop_front() {{\n    Some(v) => v,\n  }}\n  \
         io.println(\"${{SEEN}} ${{w}} ${{first}}\")\n  return 0\n}}\n"
    );
    let mut codes: Vec<String> = refusals(&bad).into_iter().map(|f| f.0).collect();
    codes.sort();
    assert_eq!(codes, ["E0200", "E0204", "E0211", "E0237"]);
}

/// A project of several modules, as `(file name, source)`.
fn project_modules(modules: &[(&str, &str)]) -> PathBuf {
    let dir = project("module main\n");
    for (name, src) in modules {
        std::fs::write(dir.join("src").join(name), src).expect("write module");
    }
    dir
}

/// Every error in a multi-module project under `--no-tsc`, as `(code, message)`.
fn project_refusals(modules: &[(&str, &str)]) -> Vec<(String, String)> {
    check(&project_modules(modules), true)["diagnostics"]
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

fn pair(code: &str, message: &str) -> (String, String) {
    (code.to_string(), message.to_string())
}

/// Record 158, F-B. Every stdlib type whose shape the checker models is an
/// object at run time, so a `number` is refused where one is declared and one
/// is refused where a `number` is declared, `decimal.Decimal` and `url.Url`
/// as much as `store.Store<T>`. `tsc` refuses each (TS2322, TS2345).
#[test]
fn a_modeled_stdlib_type_is_never_a_scalar_in_either_direction() {
    let src = "module main\n\
               \n\
               import std/decimal\n\
               import std/url\n\
               \n\
               fn num(n: number) -> number { return n }\n\
               \n\
               fn host(u: url.Url) -> string { return u.host }\n\
               \n\
               fn main(argv: Array<string>) -> number {\n\
               \x20 let d = decimal.from_int(1, 2)\n\
               \x20 let e: number = d\n\
               \x20 let f: decimal.Decimal = 5\n\
               \x20 let u = match url.parse(\"https://a.b/\") { Ok(v) => v, Err(_) => { return 1 }, }\n\
               \x20 let p = num(u)\n\
               \x20 let h = host(3)\n\
               \x20 return num(d) + e + p + string.len(h) + num(f.to_number())\n\
               }\n";
    assert_eq!(
        refusals(src),
        vec![
            pair("E0204", "type mismatch: expected `number`, found `decimal.Decimal`"),
            pair("E0204", "type mismatch: expected `decimal.Decimal`, found `number`"),
            pair("E0211", "argument type mismatch: expected `number`, found `url.Url`"),
            pair("E0211", "argument type mismatch: expected `url.Url`, found `number`"),
            pair("E0211", "argument type mismatch: expected `number`, found `decimal.Decimal`"),
        ],
        "{src}"
    );
}

/// Record 158, F-B. A record this program declares is a different declaration
/// from a modeled stdlib type even when it carries the same members (D49), so
/// it is refused where a `store.Store<number>` is declared, as a `FakeUrl` is
/// where a `url.Url` is. `tsc` accepts the `Fake` structurally; Glyph does not.
#[test]
fn a_lookalike_record_is_refused_where_a_stdlib_type_is_declared() {
    let src = "module main\n\
               \n\
               import std/store\n\
               \n\
               type Fake = { get: fn() -> number, set: fn(number) -> void, update: fn(fn(number) -> number) -> void, }\n\
               \n\
               fn takes(s: store.Store<number>) -> number { return s.get() }\n\
               \n\
               fn main(argv: Array<string>) -> number {\n\
               \x20 let f: Fake = { get: fn() -> number { return 1 }, set: fn(x: number) -> void { }, update: fn(g: fn(number) -> number) -> void { }, }\n\
               \x20 let s: store.Store<number> = f\n\
               \x20 return takes(f) + takes(s)\n\
               }\n";
    assert_eq!(
        refusals(src),
        vec![
            pair("E0204", "type mismatch: expected `store.Store<number>`, found `Fake`"),
            pair("E0211", "argument type mismatch: expected `store.Store<number>`, found `Fake`"),
        ],
        "{src}"
    );
}

/// A sibling module's function that returns a `store.Store<number>` hands its
/// caller the typed store under both import spellings.
#[test]
fn a_store_from_a_sibling_module_is_typed_under_both_spellings() {
    let counter = "module counter\n\
                   \n\
                   import std/store\n\
                   \n\
                   pub fn make() -> store.Store<number> {\n\
                   \x20 return store.create(0)\n\
                   }\n";
    for (import, call) in [("import counter", "counter.make()"), ("import counter { make }", "make()")] {
        let main = format!(
            "module main\n\
             \n\
             {import}\n\
             \n\
             fn main(argv: Array<string>) -> number {{\n\
             \x20 let s = {call}\n\
             \x20 s.set(\"x\")\n\
             \x20 let w: string = s.get()\n\
             \x20 print(w)\n\
             \x20 return 0\n\
             }}\n"
        );
        assert_eq!(
            project_refusals(&[("main.glyph", &main), ("counter.glyph", counter)]),
            vec![
                pair("E0211", "argument type mismatch: expected `number`, found `string`"),
                pair("E0204", "type mismatch: expected `string`, found `number`"),
            ],
            "{main}"
        );
    }
}

/// A catch `tsc` does not make: `pop_front` returns an `Option`, so a `match`
/// over it with no `None` arm is E0200. Under 0.1.161 the receiver was
/// `Unknown`, both checkers passed it, and an empty deque bound `undefined`.
#[test]
fn a_match_on_pop_front_without_none_is_refused() {
    let found = refusals(&program(
        "  let top = match q.pop_front() {\n    Some(x) => x,\n  }\n  io.println(\"${top}\")",
    ));
    assert_eq!(
        found,
        vec![pair("E0200", "non-exhaustive match on `Option`: missing variants `None`")]
    );
}

/// Record 161. The other direction: a modeled stdlib value where a record the
/// program declares is expected is another declaration there too, generic
/// (`store.Store<number>`) or not (`decimal.Decimal`), even when the record
/// carries the same members. `tsc` accepts both by shape.
#[test]
fn a_stdlib_value_is_refused_where_a_lookalike_record_is_declared() {
    let src = "module main\n\
               \n\
               import std/store\n\
               import std/decimal\n\
               \n\
               type Fake = { get: fn() -> number, set: fn(number) -> void, update: fn(fn(number) -> number) -> void, }\n\
               type Money = { to_string: fn() -> string, to_number: fn() -> number, }\n\
               \n\
               fn price(m: Money) -> number { return m.to_number() }\n\
               \n\
               fn main(argv: Array<string>) -> number {\n\
               \x20 let st = store.create(1)\n\
               \x20 let f: Fake = st\n\
               \x20 let d = decimal.from_int(1, 2)\n\
               \x20 return f.get() + price(d)\n\
               }\n";
    assert_eq!(
        refusals(src),
        vec![
            pair("E0204", "type mismatch: expected `Fake`, found `store.Store<number>`"),
            pair("E0211", "argument type mismatch: expected `Money`, found `decimal.Decimal`"),
        ],
        "{src}"
    );
}
