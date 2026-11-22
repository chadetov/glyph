//! D57 (G273): a key an object literal writes must be a field of the record
//! type it is written as.
//!
//! Through 0.1.161 `let b: Opt = { name: "b", extra: 1 }` passed `glyph check
//! --no-tsc` and only `tsc` refused it (TS2353), so a misspelled optional
//! field (`nik` for `nick`) dropped its value silently on every surface that
//! checks without `tsc`: the editor, the MCP tools, the edit gate. Each
//! position below is one decision record 157 lists, refused here as E0210 at
//! the key, the same code a read of the field draws.
//!
//! The accepting half matters as much: a binding holding a wider record, a
//! field that arrives through a spread, a declared `Record<K, V>`, `unknown`
//! and a type parameter are all accepted, and so is a literal that writes
//! nothing undeclared.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn project(prefix: &str, modules: &[(&str, &str)]) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_literal_fields_{prefix}_{}_{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp project");
    for (name, source) in modules {
        std::fs::write(dir.join("src").join(name), source).expect("write module");
    }
    dir
}

fn glyph(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run glyph");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn check(dir: &Path) -> (i32, String) {
    glyph(dir, &["check", "--no-tsc", "src"])
}

fn source_of(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("src/main.glyph")).expect("read fixture")
}

/// The declarations record 157's probe c01 uses, before `main`.
const DECLS: &str = "module main\n\
\n\
import std/io\n\
\n\
type Opt = { name: string, nick?: string, }\n\
type Outer = { inner: Opt, }\n\
type Load = Loading | Loaded(Opt)\n\
interface Named { name: string }\n\
@open\n\
type Wire = { id: number, }\n\
type Local = { name: string, }\n\
type Box<T> = { value: T, }\n\
type Res<T> = Good(T) | Bad\n\
\n\
fn take(o: Opt) -> number {\n\
\x20 return 0\n\
}\n\
\n\
fn named(n: Named) -> number {\n\
\x20 return 0\n\
}\n\
\n\
fn apply(f: fn(number) -> Local) -> number {\n\
\x20 return 0\n\
}\n\
\n\
fn gen<T>(x: T) -> T {\n\
\x20 return x\n\
}\n\
\n\
fn boxed<T>(b: Box<T>) -> number {\n\
\x20 return 0\n\
}\n\
\n";

/// A program whose `main` body is `body`, after `DECLS`.
fn program(body: &str) -> String {
    format!(
        "{DECLS}pub fn main(argv: Array<string>) -> number {{\n{body}\x20 return 0\n}}\n"
    )
}

/// The one E0210 a refused program draws, as the JSON diagnostic.
fn one_e0210(dir: &Path) -> serde_json::Value {
    let (code, out) = glyph(dir, &["check", "--no-tsc", "--json", "src"]);
    assert_eq!(code, 1, "{out}");
    let json: serde_json::Value = serde_json::from_str(&out).expect("check --json is JSON");
    let diags: Vec<serde_json::Value> = json["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter(|d| d["severity"] == "error")
        .cloned()
        .collect();
    assert_eq!(diags.len(), 1, "exactly one error: {out}");
    assert_eq!(diags[0]["code"], "E0210", "{out}");
    diags[0].clone()
}

/// Every position record 157 names, each refused under `--no-tsc` as E0210
/// at the key, naming the declared type. Each case is its own program so one
/// refusal cannot stand in for another.
#[test]
fn every_listed_position_refuses_an_undeclared_key() {
    let cases: &[(&str, &str, &str)] = &[
        ("let", "  let b: Opt = { name: \"b\", extra: 1, }\n  io.println(b.name)\n", "Opt"),
        ("spread_then_key", "  let a: Opt = { name: \"a\", }\n  let c: Opt = { ...a, extra: 2, }\n  io.println(c.name)\n", "Opt"),
        ("argument", "  let d = take({ name: \"x\", extra: 3, })\n  io.println(\"${d}\")\n", "Opt"),
        ("nested", "  let e: Outer = { inner: { name: \"x\", extra: 5, }, }\n  io.println(e.inner.name)\n", "Opt"),
        ("some", "  let f: Option<Opt> = Some({ name: \"x\", extra: 6, })\n  match f { Some(x) => io.println(x.name), None => io.println(\"n\"), }\n", "Opt"),
        ("variant_payload", "  let g = Loaded({ name: \"x\", extra: 7, })\n  match g { Loaded(x) => io.println(x.name), Loading => io.println(\"l\"), }\n", "Opt"),
        ("array_element", "  let h: Array<Opt> = [{ name: \"a\", extra: 8, }]\n  io.println(\"${h.length}\")\n", "Opt"),
        ("interface", "  let i = named({ name: \"x\", extra: 9, })\n  io.println(\"${i}\")\n", "Named"),
        ("open_record", "  let j: Wire = { id: 1, extra: 10, }\n  io.println(\"${j.id}\")\n", "Wire"),
        ("mut_binding", "  let l: Local = { name: \"l\", }\n  mut l = { name: \"m\", extra: 11, }\n  io.println(l.name)\n", "Local"),
        ("mut_field", "  let o: Box<Local> = { value: { name: \"v\", }, }\n  mut o.value = { name: \"w\", extra: 12, }\n  io.println(o.value.name)\n", "Local"),
        ("lambda_return", "  let p = apply(fn(n) { return { name: \"p\", extra: 13, } })\n  io.println(\"${p}\")\n", "Local"),
        ("lambda_tail", "  let p = apply(fn(n) { { name: \"p\", extra: 14, } })\n  io.println(\"${p}\")\n", "Local"),
        ("generic_union_payload", "  let q: Res<Local> = Good({ name: \"q\", extra: 15, })\n  match q { Good(x) => io.println(x.name), Bad => io.println(\"b\"), }\n", "Local"),
        ("generic_record_parameter", "  let r = boxed({ value: 1, extra: 16, })\n  io.println(\"${r}\")\n", "Box<T>"),
    ];
    for (name, body, owner) in cases {
        let dir = project(name, &[("main.glyph", &program(body))]);
        let d = one_e0210(&dir);
        let message = d["message"].as_str().unwrap_or_default();
        assert!(
            message.contains(&format!("type `{owner}` has no field `extra`")),
            "{name}: {d}"
        );
        // The key is underlined, not the literal: the range is `extra` alone.
        let start = d["range"]["start"]["offset"].as_u64().unwrap() as usize;
        let end = d["range"]["end"]["offset"].as_u64().unwrap() as usize;
        assert_eq!(&source_of(&dir)[start..end], "extra", "{name}: {d}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A `return` under a declared return type, and `Ok`/`Err` under a declared
/// `Result`.
#[test]
fn returns_and_result_constructors_refuse_an_undeclared_key() {
    let cases: &[(&str, &str)] = &[
        ("return", "fn ret() -> Opt {\n  return { name: \"r\", extra: 4, }\n}\n"),
        ("ok", "fn ret() -> Result<Opt, string> {\n  return Ok({ name: \"r\", extra: 4, })\n}\n"),
        ("err", "fn ret() -> Result<number, Opt> {\n  return Err({ name: \"r\", extra: 4, })\n}\n"),
    ];
    for (name, f) in cases {
        let source = format!("{DECLS}{f}");
        let dir = project(name, &[("main.glyph", &source)]);
        let d = one_e0210(&dir);
        assert!(
            d["message"].as_str().unwrap_or_default().contains("type `Opt` has no field `extra`"),
            "{name}: {d}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// An imported record and an imported union's variant, in both import
/// spellings: the guarantee must not depend on how the name was imported.
#[test]
fn imported_records_and_variants_refuse_in_every_import_spelling() {
    let shapes = "module shapes\n\
\n\
pub type Opt = { name: string, nick?: string, }\n\
pub type Load = Loading | Loaded(Opt)\n";
    let cases: &[(&str, &str)] = &[
        (
            "named_record",
            "module main\n\nimport shapes { Opt }\n\npub fn f() -> Opt {\n  return { name: \"a\", extra: 1, }\n}\n",
        ),
        (
            "namespace_record",
            "module main\n\nimport shapes\n\npub fn f() -> shapes.Opt {\n  return { name: \"a\", extra: 1, }\n}\n",
        ),
        (
            "named_variant",
            "module main\n\nimport shapes { Load, Loaded }\n\npub fn f() -> Load {\n  let l = Loaded({ name: \"a\", extra: 1, })\n  return l\n}\n",
        ),
        (
            "namespace_variant",
            "module main\n\nimport shapes\n\npub fn f() -> shapes.Load {\n  let l = shapes.Loaded({ name: \"a\", extra: 1, })\n  return l\n}\n",
        ),
    ];
    for (name, main) in cases {
        let dir = project(name, &[("shapes.glyph", shapes), ("main.glyph", main)]);
        let d = one_e0210(&dir);
        assert!(
            d["message"].as_str().unwrap_or_default().contains("has no field `extra`"),
            "{name}: {d}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// What stays accepted: a wider binding (width subtyping, c01's `wide`),
/// fields that arrive through a spread, a declared `Record<string, V>`,
/// `unknown`, a type parameter, and a literal with only declared fields.
#[test]
fn wider_values_spreads_maps_and_type_parameters_are_not_judged() {
    let body = "\
\x20 let wide = { name: \"w\", extra: 11, }\n\
\x20 let l: Opt = wide\n\
\x20 let s: Opt = { ...wide, }\n\
\x20 let m: Record<string, number> = { a: 1, b: 2, }\n\
\x20 let u: unknown = { anything: 1, }\n\
\x20 let g = gen({ name: \"g\", free: 1, })\n\
\x20 let k: Opt = { name: \"k\", nick: \"n\", }\n\
\x20 let w = take(wide)\n\
\x20 io.println(\"${l.name}${s.name}${g.name}${k.name}${w}\")\n\
\x20 io.println(\"done\")\n";
    let dir = project("accepted", &[("main.glyph", &program(body))]);
    let (code, out) = check(&dir);
    let errors: Vec<&str> = out.lines().filter(|l| l.contains("Error")).collect();
    assert!(errors.is_empty(), "no error expected: {out}");
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The typo case: E0210 with the nearest declared field, help that names the
/// three repairs, and `glyph fix` applying the rename. The result checks
/// clean, `glyph fmt --check` leaves it alone, and a second `fix` changes
/// nothing.
#[test]
fn a_misspelled_optional_field_is_renamed_by_fix() {
    let body = "  let k: Opt = { name: \"b\", nik: \"typo\", }\n  io.println(k.name)\n";
    let dir = project("typo", &[("main.glyph", &program(body))]);
    // Start from formatted source, so `fmt --check` below judges the rewrite.
    glyph(&dir, &["fmt", "src"]);
    let d = one_e0210(&dir);
    assert!(
        d["message"]
            .as_str()
            .unwrap_or_default()
            .contains("type `Opt` has no field `nik`; did you mean `nick`?"),
        "{d}"
    );
    let help = d["help"].as_str().unwrap_or_default();
    assert!(
        help.contains("Rename it to `nick`, delete it, or declare `nik` in `Opt`."),
        "{help}"
    );
    assert_eq!(d["alternatives"], serde_json::json!(["name", "nick"]), "{d}");

    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert!(
        out.contains("applied E0210") && out.contains("renamed the literal field to `nick`"),
        "{out}"
    );
    let after = source_of(&dir);
    assert!(after.contains("nick: \"typo\"") && !after.contains("nik:"), "{after}");
    let (code, out) = check(&dir);
    assert_eq!(code, 0, "{out}\n{after}");
    let (code, out) = glyph(&dir, &["fmt", "--check", "src"]);
    assert_eq!(code, 0, "{out}");
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert!(!out.contains("applied"), "a second run applies nothing: {out}");
    assert_eq!(source_of(&dir), after);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A literal that already writes `nick`: renaming `nik` to it would write the
/// field twice, so the message drops the suggestion, the help does not offer
/// it, and `glyph fix` declines and says why.
#[test]
fn the_fix_declines_when_the_literal_already_writes_the_field() {
    let body =
        "  let k: Opt = { name: \"b\", nick: \"n\", nik: \"typo\", }\n  io.println(k.name)\n";
    let dir = project("twice", &[("main.glyph", &program(body))]);
    let d = one_e0210(&dir);
    let message = d["message"].as_str().unwrap_or_default();
    assert!(message.contains("type `Opt` has no field `nik`"), "{d}");
    assert!(!message.contains("did you mean"), "{d}");
    let help = d["help"].as_str().unwrap_or_default();
    assert!(!help.contains("Rename it to `nick`"), "{help}");

    let before = source_of(&dir);
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert!(
        out.contains("declined E0210")
            && out.contains("the literal already writes `nick`, so renaming `nik` to it would write that field twice"),
        "{out}"
    );
    assert_eq!(source_of(&dir), before, "the file is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

/// One key reached from two positions (a variant payload under a declared
/// union) draws one E0210, not two.
#[test]
fn a_key_reached_twice_is_reported_once() {
    let body = "  let g: Load = Loaded({ name: \"x\", extra: 7, })\n  match g { Loaded(x) => io.println(x.name), Loading => io.println(\"l\"), }\n";
    let dir = project("once", &[("main.glyph", &program(body))]);
    one_e0210(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}
