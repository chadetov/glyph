//! What an object literal's type decides, both ways (G232).
//!
//! Until 0.1.123 an object literal was `Ty::Unknown`, so nothing compared it
//! to anything: `let g: string = { x: 1 }` passed `glyph check --no-tsc` with
//! an unused-variable lint while `tsc` refused it, and the same silence
//! covered a `return`, a call argument and a record field. The checker now
//! synthesizes the record type the literal writes, which reaches the record
//! recursion in `definitely_incompatible` and the width-subtyping comparison
//! that were already there and had no value to read.
//!
//! Two corpora, because a rule that starts refusing can be wrong in two
//! directions and only one of them is visible from the diagnostics it
//! produces.
//!
//! `REFUSED` is the sweep for what is now caught. Every program in it was run
//! under the published 0.1.123 before it was added: each one passed
//! `glyph check --no-tsc` there and was refused by `tsc --strict` through
//! `glyph check`, and the `tsc` code that refused it is recorded on the entry.
//! That pairing is the whole claim, so the entries carry it rather than a
//! reader having to take it on trust.
//!
//! `ACCEPTED` is the sweep for what must not start being refused, and it is
//! the half a diagnostic sweep cannot find. Every program in it compiles under
//! `tsc --strict`, so a Glyph diagnostic on one of them is a program the
//! back end accepts and the front end rejected.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// A program in either corpus: a directory name and the modules it holds as
/// `(relative path under src/, source)`.
struct Program {
    name: &'static str,
    modules: &'static [(&'static str, &'static str)],
}

/// A program the checker must refuse, with the code it must refuse it under
/// and the `tsc` error that refused it under 0.1.123.
struct Refused {
    program: Program,
    /// The Glyph code the refusal carries.
    code: &'static str,
    /// A fragment of the message, so a refusal that moves to a different
    /// pairing does not pass as this one.
    message: &'static str,
    /// The `tsc --strict` error the same program drew under the published
    /// 0.1.123, which reported no Glyph diagnostic for it. Documentation of
    /// the run that established the refusal, not an assertion: a Glyph error
    /// stops the pipeline before the back end, so the two cannot both be
    /// observed on one invocation.
    tsc: &'static str,
}

const REFUSED: &[Refused] = &[
    // The entry from the gap itself.
    Refused {
        program: Program {
            name: "record_into_primitive",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 fn main() -> void {\n\
                 \x20 let g: string = { x: 1 }\n\
                 \x20 io.println(g)\n\
                 }\n",
            )],
        },
        code: "E0204",
        message: "expected `string`, found `record`",
        tsc: "TS2322: Type '{ x: number; }' is not assignable to type 'string'.",
    },
    Refused {
        program: Program {
            name: "missing_required_field",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type User = { name: string, age: number }\n\
                 fn main() -> void {\n\
                 \x20 let u: User = { name: \"a\" }\n\
                 \x20 io.println(u.name)\n\
                 }\n",
            )],
        },
        code: "E0204",
        message: "expected `User`, found `record`",
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; }' but required in type 'User'.",
    },
    // A `return` against a declared return type, both against a primitive and
    // against a record: the position the gap named beside the `let`.
    Refused {
        program: Program {
            name: "return_into_primitive",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 fn r() -> string { return { x: 1 } }\n\
                 fn main() -> void { io.println(r()) }\n",
            )],
        },
        code: "E0204",
        message: "expected `string`, found `record`",
        tsc: "TS2322: Type '{ x: number; }' is not assignable to type 'string'.",
    },
    Refused {
        program: Program {
            name: "return_missing_field",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type User = { name: string, age: number }\n\
                 fn r() -> User { return { name: \"a\" } }\n\
                 fn main() -> void { io.println(r().name) }\n",
            )],
        },
        code: "E0204",
        message: "expected `User`, found `record`",
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; }' but required in type 'User'.",
    },
    Refused {
        program: Program {
            name: "argument_missing_field",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type User = { name: string, age: number }\n\
                 fn takes(u: User) -> string { return u.name }\n\
                 fn main() -> void { io.println(takes({ name: \"a\" })) }\n",
            )],
        },
        code: "E0211",
        message: "expected `User`, found `record`",
        tsc: "TS2345: Argument of type '{ name: string; }' is not assignable to parameter of type 'User'.",
    },
    // A literal as another record's field value: the rule recursing one level
    // in, with the inner literal underlined.
    Refused {
        program: Program {
            name: "field_value_missing_field",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type User = { name: string, age: number }\n\
                 type Wrap = { who: User }\n\
                 fn main() -> void {\n\
                 \x20 let w: Wrap = { who: { name: \"a\" } }\n\
                 \x20 io.println(w.who.name)\n\
                 }\n",
            )],
        },
        code: "E0204",
        message: "expected `User`, found `record`",
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; }' but required in type 'User'.",
    },
    // The shape the release plan named: the type survives the binding, so the
    // use site is decided the way the written literal already was.
    Refused {
        program: Program {
            name: "binding_then_argument",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type User = { name: string, age: number }\n\
                 fn takes(u: User) -> string { return u.name }\n\
                 fn main() -> void {\n\
                 \x20 let c = { name: \"a\" }\n\
                 \x20 io.println(takes(c))\n\
                 }\n",
            )],
        },
        code: "E0211",
        message: "expected `User`, found `record`",
        tsc: "TS2345: Argument of type '{ name: string; }' is not assignable to parameter of type 'User'.",
    },
    // And the binding widens a fresh literal field, exactly as TypeScript
    // does: `let c = { mode: "read" }` is a `{ mode: string }`, so the call is
    // refused rather than accepted on a narrower type than the program that
    // runs has.
    Refused {
        program: Program {
            name: "binding_widens_a_fresh_literal_field",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type Mode = \"read\" | \"write\"\n\
                 fn takes(m: Mode) -> string { return m }\n\
                 fn main() -> void {\n\
                 \x20 let c = { mode: \"read\" }\n\
                 \x20 io.println(takes(c.mode))\n\
                 }\n",
            )],
        },
        code: "E0211",
        message: "expected `Mode`, found `string`",
        tsc: "TS2345: Argument of type 'string' is not assignable to parameter of type 'Mode'.",
    },
    // A field read off a binding the literal typed. The record was there to
    // check the access against for the first time.
    Refused {
        program: Program {
            name: "field_typo_on_an_inferred_binding",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 fn main() -> void {\n\
                 \x20 let h = { one: 1, two: \"x\" }\n\
                 \x20 io.println(h.three)\n\
                 }\n",
            )],
        },
        code: "E0210",
        message: "has no field `three`",
        tsc: "TS2339: Property 'three' does not exist on type '{ one: number; two: string; }'.",
    },
    // `Nullable<T>` admits a `T` or the null a boundary produced (D45), and a
    // written record is not null, so the pairing is the one against `T`.
    Refused {
        program: Program {
            name: "nullable_missing_field",
            modules: &[(
                "main.glyph",
                "module main\n\
                 import std/io\n\
                 type User = { name: string, age: number }\n\
                 fn maybe(u: Nullable<User>) -> string { return \"ok\" }\n\
                 fn main() -> void { io.println(maybe({ name: \"a\" })) }\n",
            )],
        },
        code: "E0211",
        message: "expected `Nullable<User>`, found `record`",
        tsc: "TS2345: Argument of type '{ name: string; }' is not assignable to parameter of type 'User'.",
    },
    // A record declared in another module, read through the same field set
    // every other cross-module rule reads.
    Refused {
        program: Program {
            name: "imported_record_missing_field",
            modules: &[
                (
                    "catalog.glyph",
                    "module catalog\npub type Sheet = { rows: number, name: string }\n",
                ),
                (
                    "main.glyph",
                    "module main\n\
                     import std/io\n\
                     import catalog { Sheet }\n\
                     fn main() -> void {\n\
                     \x20 let s: Sheet = { rows: 1 }\n\
                     \x20 io.println(number.to_string(s.rows))\n\
                     }\n",
                ),
            ],
        },
        code: "E0204",
        message: "expected `Sheet`, found `record`",
        tsc: "TS2741: Property 'name' is missing in type '{ rows: number; }' but required in type 'Sheet'.",
    },
];

const ACCEPTED: &[Program] = &[
    // The ordinary correct literal, at each of the positions the refusals
    // above cover.
    Program {
        name: "complete_record",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type User = { name: string, age: number }\n\
             fn takes(u: User) -> string { return u.name }\n\
             fn make() -> User { return { name: \"m\", age: 0 } }\n\
             fn main() -> void {\n\
             \x20 let u: User = { name: \"a\", age: 1 }\n\
             \x20 let c = { name: \"b\", age: 2 }\n\
             \x20 io.println(u.name)\n\
             \x20 io.println(takes(c))\n\
             \x20 io.println(takes({ name: \"d\", age: 3 }))\n\
             \x20 io.println(make().name)\n\
             }\n",
        )],
    },
    // An optional field may be absent or present, and `optional` is read on
    // the declared side, not guessed from the literal.
    Program {
        name: "optional_field",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type Opt = { name: string, nick?: string }\n\
             fn main() -> void {\n\
             \x20 let a: Opt = { name: \"a\" }\n\
             \x20 let b: Opt = { name: \"b\", nick: \"bb\" }\n\
             \x20 io.println(a.name)\n\
             \x20 io.println(b.name)\n\
             }\n",
        )],
    },
    // A `Record<K, V>` map is written as an object literal, which is why the
    // record-against-a-container pairing stays undetermined for every
    // container but `Nullable`.
    Program {
        name: "map_written_as_a_literal",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             import std/array\n\
             import std/record\n\
             fn main() -> void {\n\
             \x20 let m: Record<string, number> = { one: 1, two: 2 }\n\
             \x20 io.println(number.to_string(array.len(record.keys(m))))\n\
             }\n",
        )],
    },
    // A correct record where a `Nullable<T>` is declared, the accepting face
    // of the refusal above.
    Program {
        name: "nullable_complete_record",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type User = { name: string, age: number }\n\
             fn maybe(u: Nullable<User>) -> string { return \"ok\" }\n\
             fn main() -> void {\n\
             \x20 let n: Nullable<User> = { name: \"a\", age: 1 }\n\
             \x20 io.println(maybe(n))\n\
             \x20 io.println(maybe({ name: \"b\", age: 2 }))\n\
             }\n",
        )],
    },
    // A spread contributes fields this walk cannot enumerate, so the literal
    // keeps no synthesized type and nothing is claimed about its field set.
    Program {
        name: "spread",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type User = { name: string, age: number }\n\
             fn main() -> void {\n\
             \x20 let base = { name: \"a\", age: 1 }\n\
             \x20 let u: User = { ...base }\n\
             \x20 let v: User = { ...base, name: \"b\" }\n\
             \x20 io.println(u.name)\n\
             \x20 io.println(v.name)\n\
             }\n",
        )],
    },
    // A literal nested in another literal, in an array, and behind a generic
    // record's argument.
    Program {
        name: "nested_and_generic",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             import std/array\n\
             type User = { name: string, age: number }\n\
             type Wrap = { who: User }\n\
             type Box<T> = { value: T }\n\
             fn main() -> void {\n\
             \x20 let w: Wrap = { who: { name: \"a\", age: 1 } }\n\
             \x20 let xs: Array<User> = [{ name: \"b\", age: 2 }]\n\
             \x20 let b: Box<string> = { value: \"c\" }\n\
             \x20 let inline: { inner: User } = { inner: { name: \"d\", age: 3 } }\n\
             \x20 io.println(w.who.name)\n\
             \x20 io.println(number.to_string(array.len(xs)))\n\
             \x20 io.println(b.value)\n\
             \x20 io.println(inline.inner.name)\n\
             }\n",
        )],
    },
    // A field whose value the checker cannot type keeps `Unknown` and sinks
    // nothing around it: the record still answers the missing-field question,
    // and the undecidable field is left alone.
    Program {
        name: "undecidable_field_value",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             import std/json\n\
             type Raw = { name: string, body: unknown }\n\
             fn main() -> void {\n\
             \x20 let r: Raw = { name: \"a\", body: json.parse(\"{}\") }\n\
             \x20 io.println(r.name)\n\
             }\n",
        )],
    },
    // A string-literal union field written as a literal, which is the pairing
    // G230 and G237 decide and which the synthesized record must not disturb.
    Program {
        name: "string_literal_union_field",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type Mode = \"read\" | \"write\"\n\
             type Cfg = { mode: Mode, name: string }\n\
             fn takes(c: Cfg) -> string { return c.name }\n\
             fn main() -> void {\n\
             \x20 let c: Cfg = { mode: \"read\", name: \"a\" }\n\
             \x20 io.println(takes({ mode: \"write\", name: \"b\" }))\n\
             \x20 io.println(c.mode)\n\
             }\n",
        )],
    },
    // A variant's record payload, and an interface satisfied by member shape.
    Program {
        name: "variant_payload_and_interface",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type Shape = | Circle({ r: number }) | Blank\n\
             interface Named { name: string }\n\
             fn area(s: Shape) -> number {\n\
             \x20 return match s {\n\
             \x20   Circle({ r }) => r,\n\
             \x20   Blank => 0,\n\
             \x20 }\n\
             }\n\
             fn takes(n: Named) -> string { return n.name }\n\
             fn main() -> void {\n\
             \x20 io.println(number.to_string(area(Circle({ r: 2 }))))\n\
             \x20 io.println(takes({ name: \"a\" }))\n\
             }\n",
        )],
    },
    // An unannotated `const` still lowers to no type at all (G39). The
    // synthesized record types the *expression*; what a `const` declaration
    // infers is untouched, so a use of one is as unchecked as it was.
    Program {
        name: "unannotated_const",
        modules: &[(
            "main.glyph",
            "module main\n\
             import std/io\n\
             type User = { name: string, age: number }\n\
             const ANON = { name: \"anon\", age: 0 }\n\
             fn takes(u: User) -> string { return u.name }\n\
             fn main() -> void { io.println(takes(ANON)) }\n",
        )],
    },
];

/// The excess-property policy, which this change does not touch and must not.
///
/// TypeScript refuses an undeclared key on a fresh object literal (TS2353) and
/// Glyph accepts it: width subtyping is the rule the record comparison
/// applies, so an extra field on the value's side is fine. That divergence
/// predates G232 and widening the rule to cover it is a separate decision
/// about a shape with no missing field in it. Pinned here so the policy cannot
/// change without this test saying so.
const EXTRA_FIELDS_ACCEPTED: &[Program] = &[Program {
    name: "extra_fields",
    modules: &[(
        "main.glyph",
        "module main\n\
         import std/io\n\
         type User = { name: string, age: number }\n\
         interface Named { name: string }\n\
         fn takes(n: Named) -> string { return n.name }\n\
         fn main() -> void {\n\
         \x20 let u: User = { name: \"a\", age: 1, extra: true }\n\
         \x20 io.println(takes({ name: \"b\", age: 2 }))\n\
         \x20 io.println(u.name)\n\
         }\n",
    )],
}];

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_object_literal_{prefix}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn stage(program: &Program) -> PathBuf {
    let root = unique_tmp(program.name);
    std::fs::write(
        root.join("package.json"),
        format!(
            "{{ \"name\": \"{}\", \"version\": \"0.0.0\", \"glyph\": {{}} }}\n",
            program.name
        ),
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    for (rel, text) in program.modules {
        std::fs::write(root.join("src").join(rel), text).expect("write module");
    }
    root
}

fn run_check(root: &Path, extra: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_glyph"));
    cmd.arg("check").arg(".").args(extra).current_dir(root);
    let out = cmd.output().expect("run glyph check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn tsc_available() -> bool {
    Command::new("tsc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Every program the published 0.1.123 accepted and `tsc` refused is now
/// refused by the Glyph checker alone, under the code and the pairing the
/// entry names.
#[test]
fn the_refused_corpus_is_refused_without_tsc() {
    for case in REFUSED {
        let root = stage(&case.program);
        let (ok, text) = run_check(&root, &["--no-tsc", "--no-test"]);
        assert!(
            !ok,
            "`{}` was accepted; `tsc` refuses it with {}\n{text}",
            case.program.name, case.tsc
        );
        assert!(
            text.contains(case.code) && text.contains(case.message),
            "`{}` was refused, but not as `{}` / `{}`\n{text}",
            case.program.name,
            case.code,
            case.message
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The refusal reaches `--json` under the same code, with the declared type
/// under `expected` by its own name rather than as a category word. An agent
/// reads that key, not the rendered frame.
#[test]
fn the_refusal_names_the_declared_type_in_json() {
    let case = REFUSED
        .iter()
        .find(|c| c.program.name == "missing_required_field")
        .expect("the missing-field case is in the corpus");
    let root = stage(&case.program);
    let (ok, text) = run_check(&root, &["--no-tsc", "--no-test", "--json"]);
    assert!(!ok, "`missing_required_field` was accepted\n{text}");
    let json: serde_json::Value = serde_json::from_str(&text).expect("check --json is JSON");
    let diags = json["diagnostics"].as_array().expect("diagnostics array");
    let d = diags
        .iter()
        .find(|d| d["code"] == "E0204")
        .unwrap_or_else(|| panic!("no E0204 in {text}"));
    assert_eq!(d["expected"], "User", "{d}");
    assert_eq!(d["actual"], "record", "{d}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Not one of these draws a Glyph diagnostic. The sweep a diagnostic-side test
/// cannot do: a rule that starts refusing correct programs shows up only here.
#[test]
fn the_accepted_corpus_draws_no_glyph_diagnostic() {
    for program in ACCEPTED.iter().chain(EXTRA_FIELDS_ACCEPTED) {
        let root = stage(program);
        let (ok, text) = run_check(&root, &["--no-tsc", "--no-test"]);
        let diags: Vec<&str> = text.lines().filter(|l| l.starts_with('[')).collect();
        assert!(
            ok && diags.is_empty(),
            "`{}` is a correct program; got: {diags:?}\n{text}",
            program.name
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// And the emitted TypeScript compiles, which is what makes the silence above
/// agreement with `tsc` rather than a second blind spot.
///
/// `EXTRA_FIELDS_ACCEPTED` is deliberately not here: `tsc` refuses an excess
/// property on a fresh object literal and Glyph accepts it, so that program is
/// pinned as accepted by Glyph and nothing is claimed about the back end.
#[test]
fn the_accepted_corpus_passes_tsc_strict() {
    if !tsc_available() {
        eprintln!("skipping: tsc is not on the PATH");
        return;
    }
    for program in ACCEPTED {
        let root = stage(program);
        let (ok, text) = run_check(&root, &["--no-test"]);
        assert!(
            ok && text.contains("tsc --strict passed"),
            "`{}` did not pass the full check:\n{text}",
            program.name
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
