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

/// A temp project holding several modules under `src/`, as
/// `(file name, source)`. The cross-module rules need one.
fn project_modules(prefix: &str, modules: &[(&str, &str)]) -> PathBuf {
    let dir = project(prefix, "module main\n");
    for (name, source) in modules {
        std::fs::write(dir.join("src").join(name), source).expect("write module");
    }
    dir
}

fn source_of_module(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join("src").join(name)).expect("read fixture")
}

/// The union `main` matches on in the cross-module tests, in its own module.
const ORDERS: &str = "module orders\n\
\n\
pub type OrderStatus =\n\
\x20 | Pending\n\
\x20 | Paid({ transaction_id: string })\n\
\x20 | Cancelled\n";


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
    // Every byte the rule writes is in the report, the import included: the
    // command's own output is what a CI log or a review captures.
    assert!(
        out.contains("wrote `import std/process` for the arm bodies"),
        "the import the arm bodies need is reported, not only written:\n{out}"
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

/// `Result` is declared by `std/result`, which the prelude re-exports, so the
/// E0200 it draws carries `cause: "std/result::Result"`, `glyph_symbol` keys
/// its variants and the rule has the payload shapes it needs (G231). Before
/// that identity existed this case was a decline.
///
/// No import is written. `Ok` and `Err` are in scope in every module through
/// the prelude, which is what makes `match r { Ok(v) => v, }` a program
/// somebody writes in the first place.
#[test]
fn e0200_repairs_a_match_on_the_prelude_result() {
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
        out.contains("applied E0200") && out.contains("`Err`"),
        "the repair names the variant it added:\n{out}"
    );
    let after = source_of(&dir);
    assert!(
        after.contains("Err(payload) => {"),
        "the missing arm is written with the payload the union declares:\n{after}"
    );
    assert!(
        !after.contains("import std/result"),
        "the prelude already binds `Ok` and `Err`, so no import is written:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}

/// The prelude's residue is still not a declaration. `Array` is a prelude name
/// no stdlib module declares, so a match that needs its cases has nothing to
/// key and the rule says so rather than inventing an address (G231).
#[test]
fn e0200_declines_a_match_on_a_bare_unkeyable_name() {
    let before = "module main\n\
                  \n\
                  pub fn pick(n: number) -> string {\n\
                  \x20 return match n {\n\
                  \x20   1 => \"one\",\n\
                  \x20 }\n\
                  }\n";
    let dir = project("e0218bare", before);
    let out = fix(&dir);
    assert!(
        !out.contains("applied E0200"),
        "a number match has no variant set to repair from:\n{out}"
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


// ---------------------------------------------------------------------------
// E0200 across a module boundary (G236)
// ---------------------------------------------------------------------------

/// The finding. The rule used to verify its own work by resolving the
/// candidate file alone, so `Paid` and `Cancelled` read as unresolved names
/// and it declined with a sentence that named nothing. The check runs against
/// the project now, and the rule brings the variants it writes into scope.
#[test]
fn e0200_repairs_a_match_over_an_imported_union_and_imports_the_variants() {
    let main = "module main\n\
                import orders { OrderStatus, Pending }\n\
                \n\
                pub fn describe(s: OrderStatus) -> string {\n\
                \x20 return match s {\n\
                \x20   Pending => \"waiting\",\n\
                \x20 }\n\
                }\n";
    let dir = project_modules("e0200imported", &[("orders.glyph", ORDERS), ("main.glyph", main)]);
    assert_eq!(check(&dir).0, 1, "the fixture starts non-exhaustive");

    let out = fix(&dir);
    assert!(
        out.contains("applied E0200") && out.contains("`Paid`") && out.contains("`Cancelled`"),
        "the imported union is repaired in one run:\n{out}"
    );
    assert!(
        out.contains("`Paid`, `Cancelled` into `import orders`"),
        "the names it added to the import list are reported:\n{out}"
    );
    let after = source_of(&dir);
    assert!(
        after.contains("import orders { OrderStatus, Pending, Paid, Cancelled }"),
        "the variants the arms name are brought into scope:\n{after}"
    );
    assert!(
        after.contains("Paid({ transaction_id }) => {") && after.contains("Cancelled => {"),
        "the patterns are written bare under a named import:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "the repaired program compiles:\n{output}\n{after}");
}

/// The review's own spelling: every variant already in the import list. The
/// unused-import rule runs first and strips the ones the partial match does
/// not use, so this is the same repair as the test above by the time the
/// E0200 rule sees the file, and it has to come out the same way.
#[test]
fn e0200_repairs_an_imported_union_whose_variants_were_already_imported() {
    let main = "module main\n\
                import orders { OrderStatus, Pending, Paid, Cancelled }\n\
                \n\
                pub fn describe(s: OrderStatus) -> string {\n\
                \x20 return match s {\n\
                \x20   Pending => \"waiting\",\n\
                \x20 }\n\
                }\n";
    let dir = project_modules("e0200imported2", &[("orders.glyph", ORDERS), ("main.glyph", main)]);
    let out = fix(&dir);
    assert!(out.contains("applied E0200"), "{out}");
    let after = source_of(&dir);
    assert!(
        after.contains("import orders { OrderStatus, Pending, Paid, Cancelled }"),
        "the import list is what it was:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "the repaired program compiles:\n{output}\n{after}");
}

/// A namespace import binds the module and not the variants, so the patterns
/// are written through the binding and no import is added.
#[test]
fn e0200_writes_the_namespace_spelling_for_a_namespace_import() {
    let main = "module main\n\
                import orders\n\
                \n\
                pub fn describe(s: orders.OrderStatus) -> string {\n\
                \x20 return match s {\n\
                \x20   orders.Pending => \"waiting\",\n\
                \x20 }\n\
                }\n";
    let dir = project_modules("e0200namespace", &[("orders.glyph", ORDERS), ("main.glyph", main)]);
    let out = fix(&dir);
    assert!(out.contains("applied E0200"), "{out}");
    let after = source_of(&dir);
    assert!(
        after.contains("orders.Paid({ transaction_id }) => {")
            && after.contains("orders.Cancelled => {"),
        "the patterns go through the namespace binding:\n{after}"
    );
    assert!(
        after.contains("import orders\n"),
        "and nothing is added to an import that binds no variants:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "the repaired program compiles:\n{output}\n{after}");
    assert_eq!(source_of_module(&dir, "orders.glyph"), ORDERS, "the union's module is untouched");
}

/// A decline names what stopped it. Bringing `Paid` into scope would collide
/// with a declaration the author wrote, and the rule does not rename either.
#[test]
fn e0200_declines_when_a_variant_name_is_already_bound_here() {
    let main = "module main\n\
                import orders { OrderStatus, Pending }\n\
                \n\
                type Paid = { x: int }\n\
                \n\
                pub fn size(p: Paid) -> int {\n\
                \x20 return p.x\n\
                }\n\
                \n\
                pub fn describe(s: OrderStatus) -> string {\n\
                \x20 return match s {\n\
                \x20   Pending => \"waiting\",\n\
                \x20 }\n\
                }\n";
    let dir = project_modules("e0200collide", &[("orders.glyph", ORDERS), ("main.glyph", main)]);
    let before = source_of(&dir);
    let out = fix(&dir);
    assert!(
        out.contains("declined E0200")
            && out.contains("`Paid` is already bound in this module")
            && !out.contains("do not collect cleanly"),
        "the refusal names the variant that stopped it:\n{out}"
    );
    assert_eq!(source_of(&dir), before, "the file is untouched");
    assert_eq!(check(&dir).0, 1, "and still does not compile");
}
