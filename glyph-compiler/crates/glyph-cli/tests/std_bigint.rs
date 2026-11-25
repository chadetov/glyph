//! `std/bigint`, the two conversions between `number` and `bigint` (G571,
//! decision record 204, ruling 2).
//!
//! `bigint.from_number` is `Err` for a fraction, `NaN` and both infinities,
//! where `BigInt(n)` throws. `bigint.to_number` is `Err` past
//! `Number.MAX_SAFE_INTEGER` in either direction, where `Number(b)` rounds
//! without a word. Each boundary is run here through the real runtime, under
//! `glyph run`, and again as an `@example` that `glyph check` executes.

use glyph_cli::test_support::js_toolchain_available;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_std_bigint_{prefix}_{}_{}", std::process::id(), n);
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
    dir
}

fn project(prefix: &str, src: &str) -> PathBuf {
    let dir = unique_tmp(prefix);
    std::fs::write(dir.join("src/main.glyph"), src).expect("write module");
    dir
}

fn glyph(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run glyph");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Every boundary, as `@example`s on two small functions that render the
/// `Result` as text, and as lines `main` prints with the full `Err` message.
const BOUNDARIES: &str = r#"module main

import std/bigint
import std/result { Ok }

@example from(9007199254740991) == "Ok(9007199254740991)"
@example from(9007199254740992) == "Ok(9007199254740992)"
@example from(-9007199254740991) == "Ok(-9007199254740991)"
@example from(-0) == "Ok(0)"
@example from(0) == "Ok(0)"
@example from(1.5) == "Err"
@example from(-0.5) == "Err"
@example from(0 / 0) == "Err"
@example from(1 / 0) == "Err"
@example from(-1 / 0) == "Err"
pub fn from(n: number) -> string {
  return match bigint.from_number(n) {
    Ok(b) => "Ok(${b.toString()})",
    Err(_) => "Err",
  }
}

@example to(9007199254740991n) == "Ok(9007199254740991)"
@example to(-9007199254740991n) == "Ok(-9007199254740991)"
@example to(9007199254740992n) == "Err"
@example to(-9007199254740992n) == "Err"
@example to(-0n) == "Ok(0)"
@example to(123456789012345678901234567890n) == "Err"
@example bigint.to_number(5n) == Ok(5)
@example bigint.from_number(5) == Ok(5n)
pub fn to(b: bigint) -> string {
  return match bigint.to_number(b) {
    Ok(n) => "Ok(${n})",
    Err(_) => "Err",
  }
}

fn show_from(label: string, n: number) -> void {
  match bigint.from_number(n) {
    Ok(b) => print("${label}=Ok(${b.toString()})"),
    Err(e) => print("${label}=Err(${e})"),
  }
}

fn show_to(label: string, b: bigint) -> void {
  match bigint.to_number(b) {
    Ok(n) => print("${label}=Ok(${n})"),
    Err(e) => print("${label}=Err(${e})"),
  }
}

pub fn main() -> number {
  show_from("from:max", 9007199254740991)
  show_from("from:max1", 9007199254740992)
  show_from("from:negzero", -0)
  show_from("from:half", 1.5)
  show_from("from:nan", 0 / 0)
  show_from("from:inf", 1 / 0)
  show_from("from:neginf", -1 / 0)
  show_to("to:max", 9007199254740991n)
  show_to("to:max1", 9007199254740992n)
  show_to("to:negmax", -9007199254740991n)
  show_to("to:negmax1", -9007199254740992n)
  show_to("to:negzero", -0n)
  return 0
}
"#;

#[test]
fn every_boundary_runs_to_the_value_the_ruling_names() {
    if !js_toolchain_available() {
        eprintln!("skipping std/bigint run: node/tsx not available");
        return;
    }
    let dir = project("run", BOUNDARIES);
    let (code, stdout, stderr) = glyph(&dir, &["run", "src/main.glyph"]);
    assert_eq!(code, Some(0), "the program should run:\n{stdout}\n{stderr}");
    for want in [
        "from:max=Ok(9007199254740991)",
        "from:max1=Ok(9007199254740992)",
        "from:negzero=Ok(0)",
        "from:half=Err(std/bigint.from_number: 1.5 is not a whole number)",
        "from:nan=Err(std/bigint.from_number: NaN is not a whole number)",
        "from:inf=Err(std/bigint.from_number: Infinity is not a whole number)",
        "from:neginf=Err(std/bigint.from_number: -Infinity is not a whole number)",
        "to:max=Ok(9007199254740991)",
        "to:max1=Err(std/bigint.to_number: 9007199254740992 is outside the range a number holds exactly (-9007199254740991 to 9007199254740991))",
        "to:negmax=Ok(-9007199254740991)",
        "to:negmax1=Err(std/bigint.to_number: -9007199254740992 is outside the range a number holds exactly (-9007199254740991 to 9007199254740991))",
        "to:negzero=Ok(0)",
    ] {
        assert!(
            stdout.lines().any(|l| l == want),
            "missing `{want}` in:\n{stdout}\n{stderr}"
        );
    }
}

/// The `@example`s above run under `glyph check`, and they are not decoration:
/// the same file with one boundary written wrong fails, naming that example.
#[test]
fn the_boundary_examples_run_and_a_wrong_one_fails() {
    if !js_toolchain_available() {
        eprintln!("skipping std/bigint examples: node/tsx not available");
        return;
    }
    let dir = project("examples", BOUNDARIES);
    let (code, stdout, stderr) = glyph(&dir, &["check", "src"]);
    assert_eq!(code, Some(0), "the examples should pass:\n{stdout}\n{stderr}");

    let wrong = BOUNDARIES.replace(
        "@example to(9007199254740992n) == \"Err\"",
        "@example to(9007199254740992n) == \"Ok(9007199254740992)\"",
    );
    assert_ne!(wrong, BOUNDARIES);
    let dir = project("examples_wrong", &wrong);
    let (code, stdout, stderr) = glyph(&dir, &["check", "src"]);
    assert_eq!(code, Some(1), "a wrong boundary must fail:\n{stdout}\n{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains("example failed: main::to example #3"),
        "the failure is a failing example:\n{stdout}\n{stderr}"
    );
}

/// E0258's help names the two conversions now that they exist, and the
/// program the `--explain` text gives as the fix builds clean under `tsc`.
#[test]
fn e0258_names_the_conversions_and_the_explained_fix_builds() {
    let dir = project(
        "e0258",
        "module main\n\npub fn total(id: bigint, n: number) -> bigint {\n  return id + n\n}\n",
    );
    let (code, stdout, _) = glyph(&dir, &["check", "--no-test", "--no-tsc", "--json", "src"]);
    assert_eq!(code, Some(1));
    let v: Value = serde_json::from_str(&stdout).expect("json");
    let d = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "E0258")
        .unwrap_or_else(|| panic!("no E0258:\n{stdout}"));
    let help = d["help"].as_str().unwrap_or("");
    assert!(
        help.contains("bigint.from_number(n)") && help.contains("bigint.to_number(b)"),
        "{help}"
    );
    assert!(!help.contains("no conversion"), "{help}");

    let (_, explain, _) = glyph(&dir, &["--explain", "E0258"]);
    assert!(explain.contains("bigint.from_number(n)?"), "{explain}");

    let fixed = project(
        "e0258_fixed",
        "module main\n\nimport std/bigint\n\npub fn total(id: bigint, n: number) -> Result<bigint, string> {\n  let m = bigint.from_number(n)?\n  return Ok(id + m)\n}\n",
    );
    let (code, stdout, stderr) = glyph(&fixed, &["check", "--no-test", "src"]);
    if js_toolchain_available() {
        assert_eq!(code, Some(0), "the explained fix should build:\n{stdout}\n{stderr}");
    }
}

/// The first E0258 diagnostic `src` produces, as its help text.
fn e0258_help(prefix: &str, src: &str) -> String {
    let dir = project(prefix, src);
    let (code, stdout, _) = glyph(&dir, &["check", "--no-test", "--no-tsc", "--json", "src"]);
    assert_eq!(code, Some(1), "{stdout}");
    let v: Value = serde_json::from_str(&stdout).expect("json");
    v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "E0258")
        .unwrap_or_else(|| panic!("no E0258:\n{stdout}"))["help"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

/// Record 210: E0258's help states the range `to_number` accepts, and it is
/// the range the runtime enforces. The help names `Number.MAX_SAFE_INTEGER`
/// and its negation; the runtime compares against exactly those two; and the
/// value one past either end is `Err` when run (`2^53` itself is, so a help
/// saying "within 2^53" was wrong at the one value that matters).
#[test]
fn e0258_help_names_the_bound_to_number_enforces() {
    let help = e0258_help(
        "e0258_bound",
        "module main\n\npub fn total(id: bigint, n: number) -> bigint {\n  return id + n\n}\n",
    );
    assert!(
        help.contains("from `-Number.MAX_SAFE_INTEGER` to `Number.MAX_SAFE_INTEGER`"),
        "{help}"
    );
    assert!(!help.contains("2^53"), "{help}");

    let runtime = include_str!("../../../runtime/std/bigint.ts");
    assert!(runtime.contains("const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);"), "{runtime}");
    assert!(runtime.contains("if (b > MAX_SAFE || b < -MAX_SAFE) {"), "{runtime}");

    // The numbers the boundary examples in `BOUNDARIES` use are that bound.
    let max_safe: u64 = (1 << 53) - 1;
    assert!(BOUNDARIES.contains(&format!("@example to({max_safe}n) == \"Ok({max_safe})\"")));
    assert!(BOUNDARIES.contains(&format!("@example to(-{max_safe}n) == \"Ok(-{max_safe})\"")));
    let past = max_safe + 1;
    assert!(BOUNDARIES.contains(&format!("@example to({past}n) == \"Err\"")));
    assert!(BOUNDARIES.contains(&format!("@example to(-{past}n) == \"Err\"")));
}

/// Record 210: a fractional literal beside a `bigint` gets a help that says no
/// `bigint` holds a fraction, rather than naming `from_number`, which is `Err`
/// on that literal. A whole literal still gets the `123n` spelling, and a
/// literal like `2.0` holds a whole number, so it keeps the conversion help.
#[test]
fn e0258_on_a_fractional_literal_says_no_bigint_holds_a_fraction() {
    let help = e0258_help(
        "e0258_frac",
        "module main\n\npub fn f(x: bigint) -> bigint {\n  return x + 1.5\n}\n",
    );
    assert!(help.contains("No `bigint` holds a fraction"), "{help}");
    assert!(help.contains("`1.5`"), "{help}");
    assert!(!help.contains("from_number"), "{help}");

    let help = e0258_help(
        "e0258_whole",
        "module main\n\npub fn f(x: bigint) -> bigint {\n  return x + 3\n}\n",
    );
    assert!(help.contains("`3n`"), "{help}");

    let help = e0258_help(
        "e0258_whole_float",
        "module main\n\npub fn f(x: bigint) -> bigint {\n  return x + 2.0\n}\n",
    );
    assert!(help.contains("bigint.from_number(n)"), "{help}");
}
