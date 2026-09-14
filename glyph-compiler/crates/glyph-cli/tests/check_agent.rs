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
