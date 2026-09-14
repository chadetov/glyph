//! `glyph fix`'s diagnostic-driven rules, run the way a caller runs them.
//!
//! Each test writes a wrong program, runs the real binary's `fix` over it, and
//! then runs `check` over what it wrote. The assertion that matters is the
//! second command's exit code: a repair that leaves the file compiling is the
//! only evidence that the arms, the patterns and the import are right, and it
//! is evidence a snapshot of the rewritten text cannot give.
//!
//! The refusal tests assert the opposite pair: the file is byte-for-byte what
//! it was, and the command said which rule stopped and why. A rule that goes
//! quiet when it cannot answer is worse than one that never ran, because the
//! caller reads "nothing to fix" as "nothing is wrong".

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// A uniquely named temp project with `src/main.glyph` holding `source`.
fn project(prefix: &str, source: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_fix_rules_{prefix}_{}_{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp project");
    std::fs::write(dir.join("src/main.glyph"), source).expect("write fixture");
    dir
}

/// Run `glyph fix src` in `dir` and return its combined output.
fn fix(dir: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(["fix", "src"])
        .output()
        .expect("run glyph fix");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Run `glyph check --no-tsc src` in `dir` and return (exit code, output).
fn check(dir: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(["check", "--no-tsc", "src"])
        .output()
        .expect("run glyph check");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn source_of(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("src/main.glyph")).expect("read fixture")
}

// ---------------------------------------------------------------------------
// E0200
// ---------------------------------------------------------------------------

const PAYMENT: &str = "module main\n\
\n\
type Payment =\n\
\x20 | Pending\n\
\x20 | Paid({ transaction_id: string, amount: int })\n\
\x20 | Refunded(string)\n\
\n\
pub fn describe(p: Payment) -> string {\n\
\x20 return match p {\n\
\x20   Pending => \"pending\",\n\
\x20 }\n\
}\n";

#[test]
fn e0200_adds_one_arm_per_missing_variant_and_the_result_compiles() {
    let dir = project("e0200", PAYMENT);
    assert_eq!(check(&dir).0, 1, "the fixture starts non-exhaustive");

    let out = fix(&dir);
    assert!(
        out.contains("applied E0200") && out.contains("`Paid`") && out.contains("`Refunded`"),
        "fix must report both arms it wrote:\n{out}"
    );

    let after = source_of(&dir);
    // The record payload is destructured by field name, the bare payload is
    // bound whole, and both arms are marked as unwritten.
    assert!(
        after.contains("Paid({ transaction_id, amount }) => {"),
        "record payload destructured by the field names the checker holds:\n{after}"
    );
    assert!(
        after.contains("Refunded(payload) => {"),
        "a non-record payload is bound whole:\n{after}"
    );
    assert_eq!(
        after.matches("// TODO(glyph fix)").count(),
        2,
        "each written arm is marked unwritten:\n{after}"
    );
    assert!(
        after.contains("import std/process") && after.contains("process.exit(1)"),
        "the body leaves through a `never`, and the import to reach it is added:\n{after}"
    );

    let (code, output) = check(&dir);
    assert_eq!(code, 0, "the repaired program compiles:\n{output}\n{after}");
}

#[test]
fn e0200_over_a_string_literal_union_writes_the_literals() {
    let dir = project(
        "e0200lit",
        "module main\n\
         \n\
         type Plan = \"free\" | \"pro\" | \"team\"\n\
         \n\
         pub fn label(p: Plan) -> string {\n\
         \x20 return match p {\n\
         \x20   \"free\" => \"f\",\n\
         \x20 }\n\
         }\n",
    );
    let out = fix(&dir);
    assert!(out.contains("applied E0200"), "{out}");
    let after = source_of(&dir);
    assert!(after.contains("\"pro\" => {"), "{after}");
    assert!(after.contains("\"team\" => {"), "{after}");
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}

/// A payload field that is a legal record field and an illegal binding name
/// (`default`, which TypeScript reserves) must not be destructured. The rule
/// does not carry its own copy of that list: it offers the destructured form to
/// the compiler's own collect and resolve stages and drops it when they answer
/// worse than they did before, which is why this test asserts on the written
/// shape rather than only on the exit code.
#[test]
fn e0200_binds_the_payload_whole_when_a_field_name_is_reserved() {
    let dir = project(
        "e0200reserved",
        "module main\n\
         \n\
         type Feed =\n\
         \x20 | Loading\n\
         \x20 | Loaded({ default: string, class: int })\n\
         \n\
         pub fn render(f: Feed) -> string {\n\
         \x20 return match f {\n\
         \x20   Loading => \"l\",\n\
         \x20 }\n\
         }\n",
    );
    fix(&dir);
    let after = source_of(&dir);
    assert!(
        after.contains("Loaded(payload) => {"),
        "a reserved field name forces the whole-payload binding:\n{after}"
    );
    assert!(
        !after.contains("{ default,"),
        "the destructured form must not survive:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}

/// `Result` is a prelude union: no project declares it, so no tool keys its
/// variants and there are no payload shapes to write patterns from. The rule
/// says that rather than inventing `Err(e)`.
#[test]
fn e0200_declines_a_union_no_project_declares() {
    let before = "module main\n\
                  \n\
                  pub fn res(r: Result<int, string>) -> int {\n\
                  \x20 return match r {\n\
                  \x20   Ok(v) => v,\n\
                  \x20 }\n\
                  }\n";
    let dir = project("e0200builtin", before);
    let out = fix(&dir);
    assert!(
        out.contains("declined E0200") && out.contains("not declared in this project"),
        "the refusal names its reason:\n{out}"
    );
    assert_eq!(source_of(&dir), before, "the file is untouched");
    assert_eq!(check(&dir).0, 1, "and still does not compile");
}

/// The arm bodies leave through `std/process.exit`, so a module that has
/// already taken the name `process` cannot be repaired without rewriting
/// something the author wrote. It declines instead.
#[test]
fn e0200_declines_when_the_module_has_taken_the_name_process() {
    let before = "module main\n\
                  \n\
                  type Feed = | Loading | Loaded | Failed\n\
                  \n\
                  fn process(x: int) -> int {\n\
                  \x20 return x\n\
                  }\n\
                  \n\
                  pub fn render(f: Feed) -> int {\n\
                  \x20 return match f {\n\
                  \x20   Loading => process(1),\n\
                  \x20 }\n\
                  }\n";
    let dir = project("e0200taken", before);
    let out = fix(&dir);
    assert!(
        out.contains("declined E0200") && out.contains("already bound in this module"),
        "{out}"
    );
    assert_eq!(source_of(&dir), before, "the file is untouched");
}

// ---------------------------------------------------------------------------
// E0220
// ---------------------------------------------------------------------------

#[test]
fn e0220_applies_the_one_suggestion_the_checker_computed() {
    let dir = project(
        "e0220",
        "module main\n\
         \n\
         type Feed = | Loading | Loaded(string) | Failed\n\
         \n\
         pub fn render(f: Feed) -> string {\n\
         \x20 return match f {\n\
         \x20   Loading => \"l\",\n\
         \x20   Loadedd(x) => x,\n\
         \x20   Failed => \"f\",\n\
         \x20 }\n\
         }\n",
    );
    let out = fix(&dir);
    assert!(out.contains("applied E0220"), "{out}");
    let after = source_of(&dir);
    assert!(after.contains("Loaded(x) => x,"), "{after}");
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}

/// The suggestion is a nearest name, not a coverage answer. Writing `Loading`
/// over a match that already has a `Loading` arm produces E0305, an arm that
/// can never run, so the rule stops when the suggested variant is not one the
/// match is missing.
#[test]
fn e0220_declines_when_the_suggested_variant_is_already_matched() {
    let before = "module main\n\
                  \n\
                  type Feed = | Loading | Loaded | Failed\n\
                  \n\
                  pub fn render(f: Feed) -> string {\n\
                  \x20 return match f {\n\
                  \x20   Loading => \"l\",\n\
                  \x20   Loadign => \"x\",\n\
                  \x20   Loaded => \"d\",\n\
                  \x20   Failed => \"f\",\n\
                  \x20 }\n\
                  }\n";
    let dir = project("e0220dup", before);
    let out = fix(&dir);
    assert!(
        out.contains("declined E0220") && out.contains("E0305"),
        "the refusal names the error the repair would have caused:\n{out}"
    );
    assert_eq!(source_of(&dir), before, "the file is untouched");
}

/// An arm head that is not a variant leaves the match's coverage unsettled, so
/// the arms E0200 would add could duplicate the one the rename produces. The
/// head is repaired and the arms wait for the next run.
#[test]
fn e0200_waits_for_a_bad_arm_head_in_the_same_match() {
    let dir = project(
        "e0200after220",
        "module main\n\
         \n\
         type Feed = | Loading | Loaded(string) | Failed\n\
         \n\
         pub fn render(f: Feed) -> string {\n\
         \x20 return match f {\n\
         \x20   Loading => \"l\",\n\
         \x20   Loadedd(x) => x,\n\
         \x20 }\n\
         }\n",
    );
    let first = fix(&dir);
    assert!(first.contains("applied E0220"), "{first}");
    assert!(
        first.contains("declined E0200") && first.contains("Run `glyph fix` again"),
        "{first}"
    );
    let second = fix(&dir);
    assert!(second.contains("applied E0200"), "{second}");
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{}", source_of(&dir));
}

// ---------------------------------------------------------------------------
// E0210
// ---------------------------------------------------------------------------

#[test]
fn e0210_renames_the_field_when_exactly_one_is_a_character_away() {
    let dir = project(
        "e0210",
        "module main\n\
         \n\
         type Inner = { total: int }\n\
         type Outer = { inner: Inner }\n\
         \n\
         pub fn read(o: Outer) -> int {\n\
         \x20 return o.inner.totl\n\
         }\n",
    );
    let out = fix(&dir);
    assert!(out.contains("applied E0210"), "{out}");
    let after = source_of(&dir);
    assert!(
        after.contains("return o.inner.total"),
        "only the field name is rewritten, not the access:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}

#[test]
fn e0210_declines_when_no_declared_field_is_a_character_away() {
    let before = "module main\n\
                  \n\
                  type Sheet = { name: string, total: int }\n\
                  \n\
                  pub fn read(s: Sheet) -> int {\n\
                  \x20 return s.total_amount\n\
                  }\n";
    let dir = project("e0210far", before);
    let out = fix(&dir);
    assert!(
        out.contains("declined E0210") && out.contains("no declared field is one character away"),
        "{out}"
    );
    assert_eq!(source_of(&dir), before);
}

#[test]
fn e0210_declines_when_two_declared_fields_are_a_character_away() {
    let before = "module main\n\
                  \n\
                  type Sheet = { tota: int, totl: int }\n\
                  \n\
                  pub fn read(s: Sheet) -> int {\n\
                  \x20 return s.total\n\
                  }\n";
    let dir = project("e0210two", before);
    let out = fix(&dir);
    assert!(
        out.contains("declined E0210") && out.contains("2 declared fields"),
        "{out}"
    );
    assert_eq!(source_of(&dir), before);
}
