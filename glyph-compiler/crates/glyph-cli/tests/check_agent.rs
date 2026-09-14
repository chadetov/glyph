//! `glyph check --agent`: the `--json` object plus what the next edit needs.
//!
//! The test that matters is the cross-module one. An `E0200` over a union the
//! module imports is the case where a plain diagnostic sends an agent looking:
//! the variants it has to write arms for are in another file, and the payload
//! of each is in the declaration rather than in the message. So the assertion
//! is that the union arrives described, with its variants and their payloads,
//! from a run that asked one question.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_check_agent_{prefix}_{}_{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp project");
    dir
}

/// Run `glyph check` with `extra` flags and parse the JSON it printed.
fn check_json(dir: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["check", "--no-tsc", "--no-test"];
    args.extend_from_slice(extra);
    args.push("src");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(&args)
        .output()
        .expect("run glyph check");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "check {extra:?} did not print JSON ({e}):\nstdout:\n{text}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// A project whose `main` matches on a union declared in a sibling module and
/// misses two of its three variants.
fn imported_union_project() -> PathBuf {
    let dir = unique_tmp("imported_union");
    std::fs::write(
        dir.join("src/model.glyph"),
        "module model\n\
         \n\
         pub type Payment =\n\
         \x20 | Pending\n\
         \x20 | Paid({ transaction_id: string, amount: int })\n\
         \x20 | Refunded(string)\n",
    )
    .expect("write model");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n\
         \n\
         import model { Payment, Pending }\n\
         \n\
         pub fn describe(p: Payment) -> string {\n\
         \x20 return match p {\n\
         \x20   Pending => \"pending\",\n\
         \x20 }\n\
         }\n\
         \n\
         pub fn main() -> void {\n\
         \x20 print(describe(Pending))\n\
         }\n",
    )
    .expect("write main");
    dir
}

#[test]
fn agent_states_the_constraints_an_e0200_repair_must_keep() {
    let dir = imported_union_project();
    let value = check_json(&dir, &["--agent"]);
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics array");
    let e0200 = diagnostics
        .iter()
        .find(|d| d["code"] == "E0200")
        .expect("an E0200 over the imported union");

    let constraints: Vec<&str> = e0200["constraints"]
        .as_array()
        .expect("constraints array")
        .iter()
        .map(|c| c.as_str().expect("constraint is a string"))
        .collect();
    assert!(
        constraints
            .iter()
            .any(|c| c.contains("preserve exhaustiveness")
                && c.contains("`Paid`")
                && c.contains("`Refunded`")),
        "the first constraint names the cases an arm is owed for: {constraints:?}"
    );
    assert!(
        constraints
            .iter()
            .any(|c| c.contains("do not add an `else` arm")
                && c.contains("declared in this project")),
        "a union this project declares must not be silenced with a catch-all: {constraints:?}"
    );
}

#[test]
fn agent_carries_the_imported_union_with_its_variants() {
    let dir = imported_union_project();
    let value = check_json(&dir, &["--agent"]);
    let e0200 = value["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .find(|d| d["code"] == "E0200")
        .cloned()
        .expect("an E0200 over the imported union");

    let symbols = e0200["symbols"].as_array().expect("symbols array");
    let union = symbols
        .iter()
        .find(|s| s["entity"] == "model::Payment")
        .unwrap_or_else(|| {
            panic!(
                "the union the match is over must be described, got {:?} (absent: {})",
                symbols
                    .iter()
                    .map(|s| s["entity"].clone())
                    .collect::<Vec<_>>(),
                e0200["symbols_absent"]
            )
        });

    assert_eq!(union["kind"], "union");
    assert_eq!(union["module"], "model");
    let variants = union["variants"].as_array().expect("variants array");
    let names: Vec<&str> = variants
        .iter()
        .map(|v| v["name"].as_str().expect("variant name"))
        .collect();
    assert_eq!(names, vec!["Pending", "Paid", "Refunded"]);
    let paid = variants
        .iter()
        .find(|v| v["name"] == "Paid")
        .expect("the Paid variant");
    assert_eq!(
        paid["payload"], "{ transaction_id: string, amount: number }",
        "the payload shape an arm's pattern has to match arrives with the variant"
    );

    // The declaration the diagnostic sits in is described too, so an agent
    // editing `describe` has its signature without a second call.
    assert!(
        symbols.iter().any(|s| s["entity"] == "main::describe"),
        "the enclosing declaration is described as well"
    );
}

/// `--agent` adds keys and changes nothing else: a reader of `--json` reads the
/// same object, and a reader of `--agent` finds both keys on every diagnostic
/// whether or not the compiler had anything to put in them.
#[test]
fn agent_is_the_json_object_plus_two_keys() {
    let dir = imported_union_project();
    let plain = check_json(&dir, &["--json"]);
    let agent = check_json(&dir, &["--agent"]);

    for key in ["ok", "errors", "warnings", "tsc", "examples"] {
        assert_eq!(plain[key], agent[key], "`{key}` is unchanged by --agent");
    }

    let plain_diagnostics = plain["diagnostics"].as_array().expect("array");
    let agent_diagnostics = agent["diagnostics"].as_array().expect("array");
    assert_eq!(plain_diagnostics.len(), agent_diagnostics.len());

    for (p, a) in plain_diagnostics.iter().zip(agent_diagnostics) {
        let mut stripped = a.clone();
        let object = stripped.as_object_mut().expect("diagnostic object");
        assert!(object.remove("constraints").is_some(), "constraints present");
        assert!(object.remove("symbols").is_some(), "symbols present");
        assert!(
            object.remove("symbols_absent").is_some(),
            "symbols_absent present"
        );
        assert_eq!(p, &stripped, "every other field is untouched");
    }
}

/// A code the compiler holds no repair invariant for gets an empty list, not a
/// sentence written to fill it.
#[test]
fn a_code_with_no_stated_invariant_gets_an_empty_constraint_list() {
    let dir = unique_tmp("no_constraint");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n\
         \n\
         pub fn main() -> void {\n\
         \x20 print(nope)\n\
         }\n",
    )
    .expect("write main");
    let value = check_json(&dir, &["--agent"]);
    let unresolved = value["diagnostics"]
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["code"] == "E0103")
        .expect("an unresolved-name diagnostic");
    assert_eq!(
        unresolved["constraints"].as_array().expect("array").len(),
        0,
        "E0103 states no repair invariant, so the list is empty"
    );
}

/// A tagged union's `alternatives` are variant names, and a variant with a
/// payload is a constructor rather than a value. The constraint used to say
/// "the accepted values here are `Red`, `Green`", and an agent that took the
/// compiler at its word and wrote `takesColor(Green)` got `TS2345: Argument of
/// type '(fields: { hex: string; }) => Color' is not assignable to parameter
/// of type 'Color'`. It says the `construct` spelling now, which is the form a
/// value is written in.
#[test]
fn the_accepted_values_of_a_tagged_union_are_its_construct_spellings() {
    let dir = unique_tmp("alt_variants");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n\
         \n\
         type Color = | Red | Green({ hex: string })\n\
         \n\
         pub fn takesColor(c: Color) -> int { return 1 }\n\
         \n\
         pub fn bad() -> int { return takesColor(3) }\n",
    )
    .expect("write main");
    let value = check_json(&dir, &["--agent"]);
    let d = value["diagnostics"]
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["code"] == "E0211")
        .expect("an argument mismatch");
    let constraints: Vec<&str> = d["constraints"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    let accepted = constraints
        .iter()
        .find(|c| c.contains("this position accepts"))
        .unwrap_or_else(|| panic!("no accepted-values constraint in {constraints:?}"));
    assert!(
        accepted.contains("`Green({ hex: string })`") && accepted.contains("`Red`"),
        "the payload is part of how a value is written: {accepted}"
    );
    assert!(
        !accepted.contains("the accepted values here are `Red`, `Green`."),
        "the bare variant list is not a list of values: {accepted}"
    );
}

/// A string-literal union's `alternatives` are the literals with their quotes
/// stripped, so rendering them in backticks made them identifiers. `read` is
/// not a value; `"read"` is.
#[test]
fn the_accepted_values_of_a_string_literal_union_are_quoted() {
    let dir = unique_tmp("alt_literals");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n\
         \n\
         type Mode = \"read\" | \"write\"\n\
         \n\
         pub fn m() -> Mode { return \"nope\" }\n",
    )
    .expect("write main");
    let value = check_json(&dir, &["--agent"]);
    let d = value["diagnostics"]
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["code"] == "E0204")
        .expect("a type mismatch");
    let constraints: Vec<&str> = d["constraints"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    let accepted = constraints
        .iter()
        .find(|c| c.contains("this position accepts"))
        .unwrap_or_else(|| panic!("no accepted-values constraint in {constraints:?}"));
    assert!(
        accepted.contains("`\"read\"`") && accepted.contains("`\"write\"`"),
        "the literals carry their quotes: {accepted}"
    );
}

/// And where the compiler holds no form to write a value in, no sentence is
/// written. An inline union names no declaration, so `glyph_symbol` has
/// nothing to describe and the constraint list holds only the one invariant
/// the compiler does state.
#[test]
fn an_inline_union_gets_no_accepted_values_sentence() {
    let dir = unique_tmp("alt_inline");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n\
         \n\
         pub fn takesMode(m: \"read\" | \"write\") -> int { return 1 }\n\
         \n\
         pub fn bad() -> int { return takesMode(3) }\n",
    )
    .expect("write main");
    let value = check_json(&dir, &["--agent"]);
    let d = value["diagnostics"]
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["code"] == "E0211")
        .expect("an argument mismatch");
    let constraints: Vec<&str> = d["constraints"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    assert!(
        constraints.iter().all(|c| !c.contains("this position accepts")),
        "no declaration to read a written form from, so no sentence: {constraints:?}"
    );
    assert!(
        constraints.iter().any(|c| c.contains("do not cast")),
        "the invariant the compiler does hold is still stated: {constraints:?}"
    );
}

/// The root the `file` strings are spelled under, on the surface that used to
/// carry no root at all. `glyph query diagnostics` has carried `project_root`
/// since the diagnostic became an object; `check --json` and `check --agent`
/// handed an agent a relative path and nothing to join it to.
#[test]
fn check_json_names_the_root_its_file_paths_are_relative_to() {
    for (name, layout) in [
        ("src_layout", vec![("src/main.glyph", true)]),
        ("flat_layout", vec![("main.glyph", false)]),
        ("nested", vec![("src/sub/deep.glyph", true)]),
    ] {
        let dir = unique_tmp(name);
        // `unique_tmp` makes `src/` for the other fixtures; a flat project is
        // one that does not have it, and an empty `src/` is still a `src/`.
        if layout.iter().all(|(rel, _)| !rel.starts_with("src/")) {
            let _ = std::fs::remove_dir(dir.join("src"));
        }
        for (rel, _) in &layout {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            let module = rel.trim_end_matches(".glyph").trim_start_matches("src/");
            std::fs::write(
                &path,
                format!("module {module}\n\npub fn f() -> int {{ return nope }}\n"),
            )
            .expect("write module");
        }
        let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
            .current_dir(&dir)
            .args(["check", "--no-tsc", "--no-test", "--json", "."])
            .output()
            .expect("run glyph check");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let value: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name}: no JSON ({e}): {text}"));
        let root = value["project_root"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: no `project_root` in {value}"));
        let diagnostics = value["diagnostics"].as_array().expect("array");
        assert!(!diagnostics.is_empty(), "{name}: the fixture draws nothing");
        for d in diagnostics {
            let file = d["file"].as_str().expect("file");
            let joined = dir.join(root).join(file);
            assert!(
                joined.exists(),
                "{name}: `project_root` joined to `file` has to open, got {}",
                joined.display()
            );
        }
    }
}

/// The `t1` layout from the review: `src/main.glyph` and no `package.json`.
/// The diagnostic keyed the module `src/main` while `glyph_symbol`, called by
/// the same process, replied that the project holds `main`, so `symbols` was
/// empty on every diagnostic and the reason given was the compiler disagreeing
/// with itself. The unmarked directory gets the same `src/` rule a marked one
/// gets now, so both surfaces count from `src`.
#[test]
fn an_unmarked_src_tree_keys_its_modules_the_way_the_tools_do() {
    let dir = unique_tmp("unmarked_src");
    std::fs::write(
        dir.join("src/main.glyph"),
        "module main\n\
         \n\
         type Status = | Open | Closed\n\
         \n\
         pub fn describe(s: Status) -> string {\n\
         \x20 return match s {\n\
         \x20   Open => \"open\",\n\
         \x20 }\n\
         }\n",
    )
    .expect("write main");
    let value = check_json(&dir, &["--agent"]);
    let d = value["diagnostics"]
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["code"] == "E0200")
        .expect("a non-exhaustive match");
    assert_eq!(d["module"], "main", "the module is keyed from `src`: {d}");
    assert_eq!(d["cause"], "main::Status", "and so is the symbol at fault: {d}");
    assert_eq!(
        d["symbols_absent"].as_array().expect("array").len(),
        0,
        "no symbol is absent, because the tool and the diagnostic count from one root: {d}"
    );
    let described: Vec<&str> = d["symbols"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|s| s["entity"].as_str())
        .collect();
    assert!(
        described.contains(&"main::Status"),
        "the union arrives described: {described:?}"
    );
}
