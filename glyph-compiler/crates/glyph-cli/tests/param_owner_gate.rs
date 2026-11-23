//! G314's entry gate (decision record 175, item 1): no type parameter reaches
//! the assignability relation without an owner.
//!
//! A type parameter is decided only when its owner is the `fn` or
//! `component` whose body is being checked. One whose owner was missed
//! (`ParamOwner::Unresolved`) is never decided, so a lowering path that loses
//! owners would make the checker silently weaker. This test checks every unit
//! of the corpus in one process and reads what the relation recorded. A
//! failure names the pairing; the fix belongs in the path that built the
//! parameter, not in a rule that decides it by name.
//!
//! The units are the ones the two-binary run uses: every app directory, every
//! standalone `.glyph` file under `examples/` and `benchmarks/glyph/` in a
//! directory of its own, each `catches/*` directory, each
//! `tests/exact-or-absent/*/src`, and each entry of `tests/negative/`.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn glyph_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "node_modules") {
                continue;
            }
            glyph_files_under(&p, out);
        } else if p.extension().is_some_and(|e| e == "glyph") {
            out.push(p);
        }
    }
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// Each unit as a directory to check, with the files to copy into a fresh
/// directory first when the unit is not a directory of its own.
enum Unit {
    InPlace(PathBuf),
    Copied(Vec<PathBuf>),
}

fn units(root: &Path) -> Vec<Unit> {
    let mut units = Vec::new();
    let apps = root.join("examples/apps");
    for d in sorted_dirs(&apps) {
        units.push(Unit::InPlace(d));
    }
    let mut files = Vec::new();
    glyph_files_under(&root.join("examples"), &mut files);
    glyph_files_under(&root.join("benchmarks/glyph"), &mut files);
    for f in files {
        if f.starts_with(&apps) && f.parent() != Some(apps.as_path()) {
            continue;
        }
        units.push(Unit::Copied(vec![f]));
    }
    for d in sorted_dirs(&root.join("catches")) {
        let mut own = Vec::new();
        if let Ok(rd) = fs::read_dir(&d) {
            own = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "glyph"))
                .collect();
        }
        if !own.is_empty() {
            own.sort();
            units.push(Unit::Copied(own));
        }
    }
    for d in sorted_dirs(&root.join("tests/exact-or-absent")) {
        if d.join("src").is_dir() {
            units.push(Unit::InPlace(d.join("src")));
        }
    }
    let negative = root.join("tests/negative");
    let mut entries: Vec<PathBuf> = fs::read_dir(&negative)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            units.push(Unit::InPlace(p));
        } else if p.extension().is_some_and(|e| e == "glyph") {
            units.push(Unit::Copied(vec![p]));
        }
    }
    units
}

#[test]
fn no_type_parameter_reaches_the_relation_without_an_owner() {
    let root = repo_root();
    let scratch = std::env::temp_dir().join(format!("glyph-param-owner-gate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let units = units(&root);
    assert!(units.len() > 200, "the corpus walk found only {} units", units.len());
    let mut failures = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        let dir = match unit {
            Unit::InPlace(d) => d.clone(),
            Unit::Copied(files) => {
                let d = scratch.join(format!("u{i}"));
                fs::create_dir_all(&d).expect("scratch dir");
                for f in files {
                    fs::copy(f, d.join(f.file_name().expect("file name"))).expect("copy unit");
                }
                d
            }
        };
        let (before, _) = glyph_typechecker::unowned_params_at_relation();
        // The verdict is not the point: a unit with errors still runs the
        // relation over everything it reached.
        let _ = glyph_cli::check_path(&dir, false, false);
        let (after, sample) = glyph_typechecker::unowned_params_at_relation();
        if after > before {
            let name = match unit {
                Unit::InPlace(d) => d.display().to_string(),
                Unit::Copied(files) => files[0].display().to_string(),
            };
            let first = sample.get(before).cloned().unwrap_or_default();
            failures.push(format!("{name}: {} pairing(s), first: {first}", after - before));
        }
    }
    let _ = fs::remove_dir_all(&scratch);
    assert!(
        failures.is_empty(),
        "a type parameter with no owner reached the relation:\n{}",
        failures.join("\n")
    );
}
