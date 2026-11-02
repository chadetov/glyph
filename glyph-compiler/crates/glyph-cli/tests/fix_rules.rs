//! `glyph fix`'s diagnostic-driven rules, run the way a caller runs them.
//!
//! Each test writes a wrong program, runs the real binary's `fix` over it, and
//! then runs `check` over what it wrote. The assertion that matters is the
//! second command's exit code: a repair that leaves the file compiling is the
//! only evidence that the rewrite is right, and it is evidence a snapshot of
//! the rewritten text cannot give.
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

/// Run `glyph fix src` in `dir` and return (exit code, combined output).
fn fix_status(dir: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(["fix", "src"])
        .output()
        .expect("run glyph fix");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
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
// E0200: reported, never written
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

/// The compiler knows which cases a match is missing and not what any of them
/// should do. `glyph fix` used to write one arm per case with a body that
/// printed and exited, and the program then passed `check`, `tsc` and `build`
/// with the missing case moved to run time. It writes nothing now: the match
/// is declined by name, the file is what it was, and `check` still refuses it.
#[test]
fn e0200_is_reported_and_left_unrepaired() {
    let dir = project("e0200", PAYMENT);
    assert_eq!(check(&dir).0, 1, "the fixture starts non-exhaustive");

    let (code, out) = fix_status(&dir);
    assert_eq!(code, 0, "a declined repair is not a failed run:\n{out}");
    assert!(
        out.contains("declined E0200")
            && out.contains("does not determine what a missing arm should do")
            && out.contains("`Paid`, `Refunded`"),
        "the decline names the error and the cases it left for the author:\n{out}"
    );
    assert!(!out.contains("applied E0200"), "no arm is written:\n{out}");
    assert_eq!(source_of(&dir), PAYMENT, "the file is byte-for-byte untouched");

    let (code, output) = check(&dir);
    assert_eq!(code, 1, "the match is still refused:\n{output}");
    assert!(output.contains("E0200"), "{output}");
}

/// A match over a union another module declares is declined the same way, and
/// neither module is touched.
#[test]
fn e0200_over_an_imported_union_is_reported_and_left_unrepaired() {
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
        out.contains("declined E0200") && out.contains("`Paid`, `Cancelled`"),
        "{out}"
    );
    assert!(!out.contains("applied E0200"), "{out}");
    assert_eq!(source_of_module(&dir, "main.glyph"), main, "the match's module is untouched");
    assert_eq!(source_of_module(&dir, "orders.glyph"), ORDERS, "the union's module is untouched");
    assert_eq!(check(&dir).0, 1, "and still does not compile");
}

/// A string-literal union (D30) is a closed set of cases too, and its match is
/// declined with the literals named.
#[test]
fn e0200_over_a_string_literal_union_is_reported_and_left_unrepaired() {
    let before = "module main\n\
                  \n\
                  type Mode = \"read\" | \"write\" | \"admin\"\n\
                  \n\
                  pub fn label(m: Mode) -> string {\n\
                  \x20 return match m {\n\
                  \x20   \"read\" => \"r\",\n\
                  \x20 }\n\
                  }\n";
    let dir = project("e0200lit", before);
    let out = fix(&dir);
    assert!(out.contains("declined E0200"), "{out}");
    assert!(!out.contains("applied E0200"), "{out}");
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

/// The E0220 rule still reads the match's E0200: renaming `Loadedd` to
/// `Loaded` is safe because `Loaded` is one of the cases the match is missing.
/// The head is repaired, the rest of the match is declined, and the program
/// still does not compile until someone writes the `Failed` arm.
#[test]
fn e0220_repairs_a_head_in_a_non_exhaustive_match_and_e0200_is_declined() {
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
    assert!(first.contains("declined E0200"), "{first}");
    assert!(!first.contains("applied E0200"), "{first}");
    let after = source_of(&dir);
    assert!(after.contains("Loaded(x) => x,"), "{after}");
    assert!(!after.contains("Failed =>"), "no arm is written for `Failed`:\n{after}");

    let second = fix(&dir);
    assert!(
        second.contains("declined E0200") && second.contains("`Failed`"),
        "the second run names the one case left:\n{second}"
    );
    assert_eq!(source_of(&dir), after, "and writes nothing");
    let (code, output) = check(&dir);
    assert_eq!(code, 1, "{output}");
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


/// G240. An E0210 now also fires at a JSX attribute the props record does not
/// declare, and that span runs from the attribute's name to the end of its
/// value rather than being a `.field` read. The rule reads the attribute form
/// first, so the dots inside a `{cfg.mode}` value never look like the access
/// it used to expect.
#[test]
fn e0210_renames_a_misspelled_jsx_attribute() {
    let dir = project(
        "e0210jsx",
        "module main\n\
         \n\
         type Props = { variant: string, label: string }\n\
         \n\
         component Button(props: Props) {\n\
         \x20 return <button>{props.label}</button>\n\
         }\n\
         \n\
         pub component App() {\n\
         \x20 return <Button variant=\"danger\" labl=\"go\" />\n\
         }\n",
    );
    let out = fix(&dir);
    assert!(out.contains("applied E0210"), "{out}");
    assert!(
        out.contains("renamed the attribute to `label`"),
        "the report says what it edited: {out}"
    );
    let after = source_of(&dir);
    assert!(
        after.contains("<Button variant=\"danger\" label=\"go\" />"),
        "only the attribute name is rewritten:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}

/// The value of an attribute may hold dots of its own. Before the attribute
/// form was read first, this declined with "not a plain name" after reading
/// `mode}` as the field.
#[test]
fn e0210_renames_a_misspelled_jsx_attribute_whose_value_holds_a_dot() {
    let dir = project(
        "e0210jsxdot",
        "module main\n\
         \n\
         type Cfg = { mode: string }\n\
         type Props = { variant: string, label: string }\n\
         \n\
         component Button(props: Props) {\n\
         \x20 return <button>{props.label}</button>\n\
         }\n\
         \n\
         pub component App(c: Cfg) {\n\
         \x20 return <Button varient={c.mode} label=\"go\" />\n\
         }\n",
    );
    let out = fix(&dir);
    assert!(out.contains("applied E0210"), "{out}");
    let after = source_of(&dir);
    assert!(
        after.contains("<Button variant={c.mode} label=\"go\" />"),
        "the name is rewritten and the value is untouched:\n{after}"
    );
    let (code, output) = check(&dir);
    assert_eq!(code, 0, "{output}\n{after}");
}
