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

/// A `match` in a D57 position hands back one of its arms, so a key an arm
/// writes is judged: an arm expression, the tail of an arm block, and a
/// nested `match` (record 160, probe g02; `tsc` refuses each as TS2353).
#[test]
fn match_arms_in_a_declared_position_are_judged() {
    let cases: &[(&str, &str)] = &[
        (
            "arm_expression",
            "  let m: Opt = match argv.length > 0 {\n    true => { name: \"m\", extra: 8, },\n    false => { name: \"n\", },\n  }\n  io.println(m.name)\n",
        ),
        (
            "arm_block_tail",
            "  let m: Opt = match argv.length > 0 {\n    true => {\n      let n = \"m\"\n      { name: n, extra: 8, }\n    },\n    false => { name: \"n\", },\n  }\n  io.println(m.name)\n",
        ),
        (
            "nested_match",
            "  let m: Opt = match argv.length > 0 {\n    true => match argv.length > 1 {\n      true => { name: \"a\", extra: 8, },\n      false => { name: \"b\", },\n    },\n    false => { name: \"n\", },\n  }\n  io.println(m.name)\n",
        ),
    ];
    for (name, body) in cases {
        let dir = project(name, &[("main.glyph", &program(body))]);
        let d = one_e0210(&dir);
        assert!(
            d["message"].as_str().unwrap_or_default().contains("type `Opt` has no field `extra`"),
            "{name}: {d}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A lambda with no written return type passed to `array.map` returns what
/// its own body returns: D55 gives it parameter types only, the slot's `U`
/// is bound from the lambda, and `tsc` accepts the extra key too. Pinned as
/// accepted (record 160).
#[test]
fn a_map_lambda_tail_with_no_declared_return_is_not_judged() {
    let source = "module main\n\nimport std/io\nimport std/array\n\ntype Opt = { name: string, nick?: string, }\n\npub fn main(argv: Array<string>) -> number {\n  let mapped = array.map([\"a\"], fn(x) { { name: x, extra: 9, } })\n  io.println(\"${array.len(mapped)}\")\n  return 0\n}\n";
    let dir = project("map_tail", &[("main.glyph", source)]);
    let (code, out) = check(&dir);
    assert!(!out.contains("E0210"), "{out}");
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The values at the positions D57 added are compared too (record 160, item
/// 2): a `Some`, `Ok` or `Err` argument, a variant payload, a `mut` target's
/// literal and a `match` arm, each refused where `tsc` refuses it (TS2322).
/// A variant payload is a constructor argument, so it is E0211; the rest
/// stand in a binding or a `return` and are E0204.
#[test]
fn values_at_the_added_positions_are_compared() {
    let cases: &[(&str, &str, &str)] = &[
        ("some_value", "  let f: Option<Opt> = Some({ name: 4, })\n  match f { Some(x) => io.println(x.name), None => io.println(\"n\"), }\n", "E0204"),
        ("payload_value", "  let g = Loaded({ name: 3, })\n  match g { Loaded(x) => io.println(x.name), Loading => io.println(\"l\"), }\n", "E0211"),
        ("mut_field_value", "  let o: Outer = { inner: { name: \"x\", }, }\n  mut o.inner = { name: 6, }\n  io.println(o.inner.name)\n", "E0204"),
        ("mut_binding_value", "  let l: Local = { name: \"l\", }\n  mut l = { name: 7, }\n  io.println(l.name)\n", "E0204"),
        ("arm_value", "  let m: Opt = match argv.length > 0 {\n    true => { name: 8, },\n    false => { name: \"n\", },\n  }\n  io.println(m.name)\n", "E0204"),
        ("some_primitive", "  let s: Option<string> = Some(5)\n  match s { Some(x) => io.println(x), None => io.println(\"n\"), }\n", "E0204"),
    ];
    for (name, body, code) in cases {
        let dir = project(name, &[("main.glyph", &program(body))]);
        let (exit, out) = glyph(&dir, &["check", "--no-tsc", "--json", "src"]);
        assert_eq!(exit, 1, "{name}: {out}");
        let json: serde_json::Value = serde_json::from_str(&out).expect("check --json is JSON");
        let errors: Vec<&serde_json::Value> = json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["severity"] == "error")
            .collect();
        assert_eq!(errors.len(), 1, "{name}: one refusal, not two: {out}");
        assert_eq!(errors[0]["code"], *code, "{name}: {out}");
        assert_eq!(errors[0]["actual"], "number", "{name}: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    for (name, f) in [
        ("ok_value", "fn ret() -> Result<Opt, string> {\n  return Ok({ name: 2, })\n}\n"),
        ("err_value", "fn ret() -> Result<number, Opt> {\n  return Err({ name: 2, })\n}\n"),
    ] {
        let source = format!("{DECLS}{f}");
        let dir = project(name, &[("main.glyph", &source)]);
        let (exit, out) = check(&dir);
        assert_eq!(exit, 1, "{name}: {out}");
        assert_eq!(out.matches("[E0204]").count(), 1, "{name}: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Correct values at the same positions draw nothing, a generic payload is
/// not compared against its unbound parameter, and a container the relation
/// refuses as a whole is one refusal.
#[test]
fn correct_values_and_generic_payloads_are_accepted() {
    let body = "\
\x20 let f: Option<Opt> = Some({ name: \"x\", })\n\
\x20 let g = Loaded({ name: \"y\", nick: \"n\", })\n\
\x20 let q = Good(5)\n\
\x20 let r: Res<number> = Good(6)\n\
\x20 let o: Outer = { inner: { name: \"x\", }, }\n\
\x20 mut o.inner = { name: \"z\", }\n\
\x20 match f { Some(x) => io.println(x.name), None => io.println(\"n\"), }\n\
\x20 match g { Loaded(x) => io.println(x.name), Loading => io.println(\"l\"), }\n\
\x20 match q { Good(_) => io.println(\"g\"), Bad => io.println(\"b\"), }\n\
\x20 match r { Good(x) => io.println(\"${x}\"), Bad => io.println(\"b\"), }\n\
\x20 io.println(o.inner.name)\n";
    let dir = project("values_ok", &[("main.glyph", &program(body))]);
    let (code, out) = check(&dir);
    assert!(!out.contains("Error"), "{out}");
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Record 160, departure 3: the constructor judgment follows what the callee
/// resolves to, never its spelling. A variant reached by each of the three
/// spellings (declared here, imported by name, read through a namespace) is
/// judged under a declared union; a local function that happens to be named
/// like a variant of that union is not, and its own parameter decides.
#[test]
fn a_constructor_is_judged_by_what_its_name_resolves_to() {
    let shapes = "module shapes\n\
\n\
pub type Opt = { name: string, nick?: string, }\n\
pub type Load = Loading | Loaded(Opt)\n";
    let cases: &[(&str, &str)] = &[
        (
            "declared_here",
            "module main\n\ntype Opt = { name: string, }\ntype Load = Loading | Loaded(Opt)\n\npub fn f() -> Load {\n  let l: Load = Loaded({ name: \"a\", extra: 1, })\n  return l\n}\n",
        ),
        (
            "imported_by_name",
            "module main\n\nimport shapes { Load, Loaded }\n\npub fn f() -> Load {\n  let l: Load = Loaded({ name: \"a\", extra: 1, })\n  return l\n}\n",
        ),
        (
            "through_a_namespace",
            "module main\n\nimport shapes\n\npub fn f() -> shapes.Load {\n  let l: shapes.Load = shapes.Loaded({ name: \"a\", extra: 1, })\n  return l\n}\n",
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

    let lookalike = "module main\n\
\n\
import shapes { Load }\n\
\n\
fn Loaded(o: { name: string, extra: number, }) -> Load {\n\
\x20 return pick(o.extra)\n\
}\n\
\n\
fn pick(n: number) -> Load {\n\
\x20 return pick(n)\n\
}\n\
\n\
pub fn f() -> Load {\n\
\x20 let l: Load = Loaded({ name: \"a\", extra: 1, })\n\
\x20 return l\n\
}\n";
    let dir = project("lookalike", &[("shapes.glyph", shapes), ("main.glyph", lookalike)]);
    let (code, out) = check(&dir);
    assert!(!out.contains("E0210") && !out.contains("Error"), "{out}");
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Record 160, item 3. Inside a `${}` a diagnostic's range used to start one
/// column early (G435, fixed in 0.1.164), and a rename written there landed
/// on the wrong bytes: a literal key in a template's second interpolation
/// became `nickc`. With exact spans, and the fix driver's guard reading a
/// literal key by its text, every distance-one rename inside an
/// interpolation applies at the right bytes and the file re-checks clean;
/// a name with no distance-one match (`nikc`, a transposition) declines and
/// leaves the file byte-identical.
#[test]
fn renames_inside_an_interpolation_apply_at_the_right_bytes() {
    let decls = "module main\n\
\n\
import std/io\n\
\n\
type Opt = { name: string, nick?: string }\n\
\n\
fn take(o: Opt) -> string {\n\
\x20 return o.name\n\
}\n\
\n";
    let literal = |second: &str| {
        format!(
            "{decls}pub fn main(argv: Array<string>) -> number {{\n\
\x20 let n = \"a\"\n\
\x20 let k = \"b\"\n\
\x20 io.println(\"t=${{take({{ name: n, nickx: k }})}} end\")\n\
\x20 io.println(\"u=${{take({{ name: n, xnick: k }})}} end\")\n\
\x20 io.println(\"v=${{take({{ name: n, nicks: k }})}} w=${{take({{ name: n, {second}: k }})}}\")\n\
\x20 return 0\n\
}}\n"
        )
    };
    let read = |second: &str| {
        format!(
            "module main\n\
\n\
import std/io\n\
\n\
type B = {{ nick: string, name: string }}\n\
\n\
pub fn main(argv: Array<string>) -> number {{\n\
\x20 let b: B = {{ nick: \"a\", name: \"c\" }}\n\
\x20 io.println(\"v=${{b.name}} w=${{b.{second}}}\")\n\
\x20 return 0\n\
}}\n"
        )
    };

    // x02 as written: three keys a character from `nick` are renamed in
    // place, `nikc` is left and still refused.
    let dir = project("x02", &[("main.glyph", &literal("nikc"))]);
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert_eq!(out.matches("applied E0210").count(), 3, "{out}");
    assert!(out.contains("declined E0210") && out.contains("`nikc`"), "{out}");
    let after = source_of(&dir);
    assert_eq!(
        after,
        literal("nikc")
            .replace("nickx: k", "nick: k")
            .replace("xnick: k", "nick: k")
            .replace("nicks: k", "nick: k"),
        "only the three keys change"
    );
    let (code, out) = check(&dir);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out.matches("[E0210]").count(), 1, "only `nikc` is left: {out}");
    let _ = std::fs::remove_dir_all(&dir);

    // The same with a distance-one key in the second interpolation: all four
    // apply and the file checks clean.
    let dir = project("x02_clean", &[("main.glyph", &literal("nik"))]);
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert_eq!(out.matches("applied E0210").count(), 4, "{out}");
    let after = source_of(&dir);
    assert!(after.contains("w=${take({ name: n, nick: k })}"), "{after}");
    let (code, out) = check(&dir);
    assert_eq!(code, 0, "{out}\n{after}");
    let _ = std::fs::remove_dir_all(&dir);

    // x03 as written: `nikc` has no distance-one match, so the read rename
    // declines and the file is untouched.
    let dir = project("x03", &[("main.glyph", &read("nikc"))]);
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert!(!out.contains("applied E0210") && out.contains("declined E0210"), "{out}");
    assert_eq!(source_of(&dir), read("nikc"), "the file is untouched");
    let _ = std::fs::remove_dir_all(&dir);

    // A distance-one read in the second interpolation is renamed in place.
    let dir = project("x03_clean", &[("main.glyph", &read("nik"))]);
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert!(out.contains("applied E0210"), "{out}");
    assert_eq!(source_of(&dir), read("nick"));
    let (code, out) = check(&dir);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Records 167 and 168 found a range moved by escapes inside the `${}` of a
/// `"..."` string landing on a variable of the same name (d01) or on an
/// identical object literal (d04, d05), so the fix driver declined there.
/// G441 (record 166) made a backslash in that position a parse error, so
/// these programs no longer reach the checker or the fix driver: `check`
/// refuses each at the first escape with E0002, `fix` applies nothing, and
/// every file is byte-identical. x02's renames and a key outside any
/// template still apply; `renames_inside_an_interpolation_apply_at_the_right_bytes`
/// and `a_misspelled_optional_field_is_renamed_by_fix` hold that.
#[test]
fn an_escape_inside_an_interpolation_stops_check_and_fix_at_parse() {
    let d01 = "module main\n\
\n\
import std/io\n\
\n\
type Opt = { name: string, nick?: string }\n\
\n\
fn f(s: string) -> string {\n\
\x20 return s\n\
}\n\
fn pick(a: string, b: string, o: Opt) -> string {\n\
\x20 return o.name\n\
}\n\
\n\
pub fn main(argv: Array<string>) -> number {\n\
\x20 let nik = \"v\"\n\
\x20 let k = \"w\"\n\
\x20 io.println(\"r=${pick(f(\\\"\\t\\\"), f(\\\"\\\"), { name: nik, nik: k })}\")\n\
\x20 return 0\n\
}\n";
    for (name, source) in [("d01", d01), ("d04", D04), ("d05", D05)] {
        let dir = project(name, &[("main.glyph", source)]);
        let (code, out) = check(&dir);
        assert_eq!(code, 1, "{name}: {out}");
        assert!(out.contains("[E0002]") && out.contains("inside the `${}` of a `\"...\"` string"), "{name}: {out}");
        // Reported at the first escape: its line and its 1-based column.
        let at = source.find("\\\"").unwrap();
        let line = source[..at].matches('\n').count() + 1;
        let col = at - source[..at].rfind('\n').unwrap();
        assert!(out.contains(&format!("main:{line}:{col}")), "{name}: the first escape at {line}:{col}: {out}");
        assert!(!out.contains("[E0210]"), "{name}: {out}");
        let (_, out) = glyph(&dir, &["fix", "src"]);
        assert!(!out.contains("applied"), "{name}: {out}");
        assert_eq!(source_of(&dir), source, "{name}: the file is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// G441, record 174 (F1): the E0002 help for a backslash inside a `${}` is
/// computed from the code, and both spellings it writes compile. Each
/// program is refused; the help's `let` form (the binding on the line above,
/// the string rewritten to interpolate `label`) and its `"""` form are each
/// applied as an agent would, and the full check, `tsc` included, exits 0
/// and the run prints the same text both ways.
#[test]
fn both_spellings_the_escape_help_writes_compile() {
    let between = |text: &str, open: &str, close: &str| -> String {
        let a = text.find(open).unwrap_or_else(|| panic!("no {open:?} in {text}")) + open.len();
        let b = text[a..].find(close).unwrap_or_else(|| panic!("no {close:?} after {open:?} in {text}"));
        text[a..a + b].to_string()
    };
    for (name, line, printed) in [
        // Record 174's s03.
        ("s03", r#"io.println("a${x + \"\\u{e9}\"}")"#, "av\u{e9}"),
        ("s01", r#"io.println("<${x}> ${f(\"x\")} end")"#, "<v> x end"),
    ] {
        let program = |line: &str, pre: &str| {
            format!(
                "module main\n\nimport std/io\n\nfn f(s: string) -> string {{\n  return s\n}}\n\npub fn main(argv: Array<string>) -> number {{\n  let x = \"v\"\n{pre}  {line}\n  return 0\n}}\n"
            )
        };
        let dir = project(name, &[("main.glyph", &program(line, ""))]);
        let (code, out) = glyph(&dir, &["check", "--no-tsc", "--json", "src"]);
        assert_eq!(code, 1, "{name}: {out}");
        let json: serde_json::Value = serde_json::from_str(&out).expect("check --json is JSON");
        let help = json["diagnostics"][0]["help"].as_str().expect("a help").to_string();
        let _ = std::fs::remove_dir_all(&dir);

        let bind = between(&help, "`let label = ", "`, then");
        let bound = between(&help, "`, then `", "`.");
        let triple = format!("\"\"\"{}\"\"\"", between(&help, "`\"\"\"", "\"\"\"`"));
        let at = line.find('"').unwrap();
        let end = line.rfind('"').unwrap() + 1;
        let forms = [
            program(&format!("{}{bound}{}", &line[..at], &line[end..]), &format!("  let label = {bind}\n")),
            program(&format!("{}{triple}{}", &line[..at], &line[end..]), ""),
        ];
        for (k, src) in forms.iter().enumerate() {
            let dir = project(&format!("{name}_{k}"), &[("main.glyph", src)]);
            let (code, out) = glyph(&dir, &["check", "src"]);
            assert_eq!(code, 0, "{name} form {k}: {help}\n{src}\n{out}");
            let (code, out) = glyph(&dir, &["run", "src/main.glyph"]);
            assert_eq!(code, 0, "{name} form {k}: {out}");
            assert!(out.contains(printed), "{name} form {k} printed: {out}\n{src}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// G441. The escape-region decline record 168 added to the fix guard is
/// gone: after the parse error it could only fire in a `"""..."""` string,
/// whose ranges were always exact, and there it declined a correct repair.
/// A key and a read after a `"a\tb"` inside a `"""` interpolation are
/// renamed at their own bytes, and the file checks clean.
#[test]
fn a_repair_after_a_backslash_in_a_triple_quoted_interpolation_applies() {
    let source = r##"module main

import std/io

type Opt = { name: string, nick?: string }

fn f(s: string) -> string {
  return s
}
fn take(o: Opt) -> string {
  return o.name
}

pub fn main(argv: Array<string>) -> number {
  let b: Opt = { name: "a", nick: "b" }
  io.println("""x${f("a\tb") + b.nik} ${take({ name: f("\\"), nik: "k" })}""")
  return 0
}
"##;
    let dir = project("t441", &[("main.glyph", source)]);
    let (code, out) = check(&dir);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out.matches("[E0210]").count(), 2, "{out}");
    let (_, out) = glyph(&dir, &["fix", "src"]);
    assert_eq!(out.matches("applied E0210").count(), 2, "{out}");
    assert!(!out.contains("declined"), "{out}");
    assert_eq!(source_of(&dir), source.replace("b.nik}", "b.nick}").replace("nik: \"k\"", "nick: \"k\""));
    let (code, out) = check(&dir);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

const D04: &str = r##"module main

import std/io

type Opt = { name: string, nick?: string }

fn f(s: string) -> string {
  return s
}
fn two(a: string, o: Opt, p: Opt) -> string {
  return o.name
}

fn main(argv: Array<string>) -> number {
  let k = "w"
  io.println("r=${two(f(\"\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\"), { name: k, nik: k }, { name: k, nik: k })}")
  return 0
}
"##;

const D05: &str = r##"module main

import std/io

type Opt = { name: string, nick?: string }
type Other = { name: string, nik?: string, nick?: string }

fn f(s: string) -> string {
  return s
}
fn two(a: string, o: Other, p: Opt) -> string {
  return o.name
}

fn main(argv: Array<string>) -> number {
  let k = "w"
  io.println("r=${two(f(\"\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\"), { name: k, nik: k }, { name: k, nik: k })}")
  return 0
}
"##;
