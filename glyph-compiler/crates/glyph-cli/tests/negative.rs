//! Negative-example suite: every `tests/negative/*.glyph` must fail to compile
//! with the error code named in its sibling `*.expected_error` file.
//!
//! Each case is built in isolation (its own temp directory). Building uses the
//! same pipeline the `glyph` binary does, so the codes asserted here are exactly
//! what a user sees. No `tsc`/`tsx` is needed — these never reach emission.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_cli::build::build_project_inner;

fn negative_dir() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "..",
        "tests",
        "negative",
    ]
    .iter()
    .collect()
}

fn unique_tmp() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("glyph_negative_{}_{}", std::process::id(), n));
    fs::create_dir_all(dir.join("src")).expect("mkdir temp src");
    dir
}

#[test]
fn every_negative_case_fails_with_its_expected_code() {
    let dir = negative_dir();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("glyph"))
        .collect();
    entries.sort();
    assert!(
        entries.len() >= 15,
        "expected at least 15 negative cases (the plan's bar), found {}",
        entries.len()
    );

    for glyph_path in &entries {
        let name = glyph_path.file_stem().unwrap().to_string_lossy().into_owned();
        let expected = fs::read_to_string(glyph_path.with_extension("expected_error"))
            .unwrap_or_else(|e| panic!("{name}: missing .expected_error: {e}"));
        let code = expected.trim();
        assert!(
            code.starts_with('E') && code.len() == 5,
            "{name}: malformed expected code {code:?}"
        );

        let source = fs::read_to_string(glyph_path).unwrap();
        let root = unique_tmp();
        fs::write(root.join("src").join(format!("{name}.glyph")), &source).unwrap();
        let report = build_project_inner(&root.join("src"), &root.join("out"), false)
            .unwrap_or_else(|e| panic!("{name}: build did not run: {e}"));

        assert!(
            report.has_errors(),
            "{name}: expected to fail with {code}, but it compiled clean"
        );
        let tag = format!("[{code}]");
        assert!(
            report.diagnostics.iter().any(|d| d.contains(&tag)),
            "{name}: expected code {code} not found.\n--- diagnostics ---\n{}",
            report.diagnostics.join("\n")
        );
    }
}
