//! A project-level banned import (D74, record 196 section 9): every probe in
//! the record's table, run through `glyph check` and `glyph build` on the
//! record's fixture.
//!
//! The fixture bans `axios` (use `http/client`) and `std/fs` (use `storage`,
//! allowed in `storage` and under `migrations/`), with `src/storage.glyph` and
//! `src/http/client.glyph` present. Each probe is its own copy of the fixture
//! with one file changed, because one error fails the whole project.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

const MANIFEST: &str = r#"{
  "name": "p",
  "glyph": {
    "src": "src",
    "banned_imports": {
      "axios":  { "use": "http/client", "reason": "requests go through the retry and auth wrapper" },
      "std/fs": { "use": "storage", "allow_in": ["storage", "migrations/"] }
    }
  }
}
"#;

const STORAGE: &str = "module storage\n\nimport std/fs\n\npub fn exists(path: string) -> bool {\n  return fs.exists(path)\n}\n";
const CLIENT: &str = "module http/client\n\npub fn get(url: string) -> string {\n  return url\n}\n";
const MAIN: &str = "module main\n\nimport storage\nimport http/client\n\npub fn main() -> void {\n  print(client.get(\"x\"))\n  let ok = storage.exists(\"y\")\n  print(\"${ok}\")\n}\n";

fn fixture(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("glyph_banned_{name}_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/http")).expect("fixture dirs");
    std::fs::create_dir_all(dir.join("src/migrations")).expect("fixture dirs");
    std::fs::write(dir.join("package.json"), MANIFEST).expect("manifest");
    std::fs::write(dir.join("src/storage.glyph"), STORAGE).expect("storage");
    std::fs::write(dir.join("src/http/client.glyph"), CLIENT).expect("client");
    std::fs::write(dir.join("src/main.glyph"), MAIN).expect("main");
    dir
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, text).expect("write");
}

/// `glyph <cmd> --no-tsc --no-test --json .` in `dir` (with `--out dist` for
/// a build): the exit code and the
/// diagnostics, as `(code, message, file)`.
fn run(dir: &Path, cmd: &str) -> (i32, Vec<(String, String, String)>) {
    let mut args = vec![cmd, "--no-tsc", "--no-test", "--json"];
    if cmd == "build" {
        args.extend(["--out", "dist"]);
    }
    args.push(".");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(dir)
        .args(&args)
        .output()
        .expect("run glyph");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let json: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "{cmd} printed no JSON ({e}):\n{text}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let diags = json["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap_or("").to_string(),
                d["message"].as_str().unwrap_or("").to_string(),
                d["file"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    (out.status.code().unwrap_or(-1), diags)
}

/// Both `check` and `build` give `dir` the same answer, and that answer.
fn both(dir: &Path) -> (i32, Vec<(String, String, String)>) {
    let checked = run(dir, "check");
    let built = run(dir, "build");
    assert_eq!(checked.0, built.0, "check and build disagree on the exit code");
    let codes = |d: &[(String, String, String)]| d.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(codes(&checked.1), codes(&built.1), "check and build disagree on the codes");
    let _ = std::fs::remove_dir_all(dir.join("dist"));
    checked
}

fn errors(d: &[(String, String, String)]) -> Vec<&str> {
    d.iter().filter(|x| x.0 != "E0106").map(|x| x.0.as_str()).collect()
}

#[test]
fn the_fixture_itself_is_clean_and_the_wrapper_may_import_the_banned_module() {
    let dir = fixture("clean");
    let (code, d) = both(&dir);
    assert_eq!(code, 0, "{d:?}");
    assert!(errors(&d).is_empty(), "{d:?}");
}

#[test]
fn main_importing_a_banned_std_module_is_e0115_naming_the_replacement() {
    let dir = fixture("std_fs");
    write(&dir, "src/main.glyph", "module main\n\nimport std/fs\n\npub fn main() -> void {\n  let ok = fs.exists(\"y\")\n  print(\"${ok}\")\n}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    assert_eq!(errors(&d), vec!["E0115"], "{d:?}");
    assert_eq!(d[0].1, "`std/fs` is banned in this project");
    assert_eq!(d[0].2, "main.glyph");

    // The help, which names `use` and never `allow_in`.
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["check", "--no-tsc", "--no-test", "--json", "."])
        .output()
        .expect("run");
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let help = json["diagnostics"][0]["help"].as_str().expect("help");
    assert_eq!(
        help,
        "import `storage` instead; `glyph query exports --module storage` lists what it offers. The ban is set in `glyph.banned_imports` in package.json."
    );
    assert!(!help.contains("allow_in"));
}

#[test]
fn a_banned_package_that_is_not_installed_is_e0115_only() {
    let dir = fixture("axios");
    write(&dir, "src/main.glyph", "module main\n\nimport axios { default as ax }\n\npub fn main() -> void {\n  print(ax)\n}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    assert_eq!(errors(&d), vec!["E0115"], "no E0104 beside it: {d:?}");
    assert_eq!(
        d[0].1,
        "`axios` is banned in this project: requests go through the retry and auth wrapper"
    );
}

#[test]
fn a_subpath_of_a_banned_package_says_which_key_bans_it() {
    let dir = fixture("subpath");
    write(&dir, "src/main.glyph", "module main\n\nimport axios/lib/core { x }\n\npub fn main() -> void {\n  print(x)\n}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    assert_eq!(errors(&d), vec!["E0115"], "{d:?}");
    assert_eq!(
        d[0].1,
        "`axios/lib/core` is banned in this project (it is under `axios`): requests go through the retry and auth wrapper"
    );
}

#[test]
fn a_package_whose_name_only_starts_with_a_banned_key_is_not_banned() {
    let dir = fixture("retry");
    write(&dir, "src/main.glyph", "module main\n\nimport axios-retry { default as retry }\n\npub fn main() -> void {\n  print(retry)\n}\n");
    let plain = fixture("retry_unbanned");
    write(&plain, "package.json", "{ \"name\": \"p\", \"glyph\": { \"src\": \"src\" } }\n");
    write(&plain, "src/main.glyph", "module main\n\nimport axios-retry { default as retry }\n\npub fn main() -> void {\n  print(retry)\n}\n");
    let banned = both(&dir);
    let unbanned = both(&plain);
    assert!(!banned.1.iter().any(|d| d.0 == "E0115"), "{:?}", banned.1);
    assert_eq!(errors(&banned.1), errors(&unbanned.1), "the ban changed the answer");
}

#[test]
fn a_module_under_an_allowed_directory_may_import_it() {
    let dir = fixture("allowed_dir");
    write(&dir, "src/migrations/m1.glyph", "module migrations/m1\n\nimport std/fs\n\npub fn run(path: string) -> bool {\n  return fs.exists(path)\n}\n");
    write(&dir, "src/main.glyph", "module main\n\nimport storage\nimport http/client\nimport migrations/m1\n\npub fn main() -> void {\n  print(client.get(\"x\"))\n  let ok = storage.exists(\"y\")\n  print(\"${ok}\")\n  let ran = m1.run(\"z\")\n  print(\"${ran}\")\n}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 0, "{d:?}");
    assert!(errors(&d).is_empty(), "{d:?}");
}

#[test]
fn each_malformed_glyph_key_is_e0116_at_the_manifest() {
    let cases: [(&str, &str, &str); 5] = [
        ("\"allow_in\"", "\"alow_in\"", "`glyph.banned_imports.std/fs.alow_in` is not a field Glyph reads (known: `use`, `reason`, `allow_in`); did you mean `allow_in`?"),
        ("\"banned_imports\"", "\"banned_import\"", "did you mean `banned_imports`?"),
        ("\"http/client\"", "\"http/clinet\"", "`glyph.banned_imports.axios.use` names `http/clinet`, which is not a module of this project"),
        ("\"migrations/\"", "\"nope\"", "`glyph.banned_imports.std/fs.allow_in` names `nope`, which is not a module of this project"),
        ("\"use\": \"storage\"", "\"use\": \"axios\"", "`glyph.banned_imports.std/fs.use` names `axios`, which is itself banned"),
    ];
    for (from, to, message) in cases {
        let dir = fixture("e0116");
        write(&dir, "package.json", &MANIFEST.replace(from, to));
        let (code, d) = both(&dir);
        assert_eq!(code, 1, "{to}: {d:?}");
        let e0116: Vec<_> = d.iter().filter(|x| x.0 == "E0116").collect();
        assert_eq!(e0116.len(), 1, "{to}: {d:?}");
        assert!(e0116[0].1.contains(message), "{to}: {}", e0116[0].1);
        assert_eq!(e0116[0].2, "../package.json");
    }
}

#[test]
fn a_banned_specifier_in_an_extern_ts_body_is_e0115() {
    let dir = fixture("extern");
    write(&dir, "src/main.glyph", "module main\n\npub fn main() -> void {\n  let ax = extern_ts(\"require('axios')\")\n  print(ax)\n}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    assert_eq!(errors(&d), vec!["E0115"], "{d:?}");
    assert!(d[0].1.starts_with("`axios` is banned in this project"));
}

#[test]
fn a_generated_boundary_module_importing_the_package_is_e0115() {
    // The shape `glyph gen dts` writes for a package: a module of the project
    // importing it. It is not in `allow_in`, so it may not.
    let dir = fixture("gen");
    write(
        &dir,
        "src/axios_types.glyph",
        "// Generated by `glyph gen dts axios`. Do not edit.\nmodule axios_types\n\nimport axios { default as ax }\n\npub fn instance() -> unknown {\n  return ax\n}\n",
    );
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    let e0115: Vec<_> = d.iter().filter(|x| x.0 == "E0115").collect();
    assert_eq!(e0115.len(), 1, "{d:?}");
    assert_eq!(e0115[0].2, "axios_types.glyph");
}

#[test]
fn an_unknown_glyph_key_is_e0116_and_a_non_string_src_still_marks_the_project() {
    let dir = fixture("bogus");
    write(&dir, "package.json", "{\"name\": \"p\", \"glyph\": {\"src\": \"src\", \"bogus\": 1}}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    assert_eq!(errors(&d), vec!["E0116"], "{d:?}");

    let dir = fixture("src_number");
    write(&dir, "package.json", "{\"name\": \"p\", \"glyph\": {\"src\": 5}}\n");
    let (code, d) = both(&dir);
    assert_eq!(code, 1);
    assert_eq!(errors(&d), vec!["E0116"], "{d:?}");
}

#[test]
fn a_ban_is_not_inherited_from_an_outer_project() {
    let dir = fixture("nested");
    write(&dir, "inner/package.json", "{\"name\": \"inner\", \"glyph\": {}}\n");
    write(&dir, "inner/main.glyph", "module main\n\nimport std/fs\n\npub fn main() -> void {\n  let ok = fs.exists(\"y\")\n  print(\"${ok}\")\n}\n");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["check", "--no-tsc", "--no-test", "--json", "inner/main.glyph"])
        .output()
        .expect("run");
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(out.status.code(), Some(0), "{json}");
}

#[test]
fn checking_one_file_applies_its_projects_bans() {
    let dir = fixture("one_file");
    write(&dir, "src/main.glyph", "module main\n\nimport std/fs\n\npub fn main() -> void {\n  let ok = fs.exists(\"y\")\n  print(\"${ok}\")\n}\n");
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .current_dir(&dir)
        .args(["check", "--no-tsc", "--no-test", "--json", "src/main.glyph"])
        .output()
        .expect("run");
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(out.status.code(), Some(1), "{json}");
    assert_eq!(json["diagnostics"][0]["code"], "E0115", "{json}");
}
