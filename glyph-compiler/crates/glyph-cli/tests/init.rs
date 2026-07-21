//! `glyph init` scaffolding: the starter is written, re-running never clobbers,
//! and the generated program compiles through the real pipeline.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use glyph_cli::build::build_project_inner;
use glyph_cli::init::scaffold;

fn unique_tmp() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("glyph_init_{}_{}", std::process::id(), n))
}

#[test]
fn scaffold_writes_a_runnable_starter() {
    let dir = unique_tmp();
    let report = scaffold(&dir).expect("scaffold");
    assert_eq!(report.created.len(), 4, "expected four files created");
    assert!(report.skipped.is_empty());

    for rel in ["src/main.glyph", "src/.types/README.md", "package.json", ".gitignore"] {
        assert!(dir.join(rel).exists(), "missing scaffolded file: {rel}");
    }

    let pkg = std::fs::read_to_string(dir.join("package.json")).unwrap();
    assert!(pkg.contains("\"glyph\""), "package.json lacks the glyph key");
    assert!(pkg.contains(&format!("\"name\": \"{}\"", dir.file_name().unwrap().to_string_lossy())));
    // C5: the toolchain is pinned so `glyph run`/`build` resolve a consistent
    // TypeScript across a team. The scaffold must be valid JSON with both pins.
    let parsed: serde_json::Value = serde_json::from_str(&pkg).expect("package.json is valid JSON");
    let dev = &parsed["devDependencies"];
    assert!(dev["typescript"].is_string(), "pins typescript: {pkg}");
    assert!(dev["tsx"].is_string(), "pins tsx: {pkg}");

    // The generated program must compile through the real pipeline (no tsc needed
    // here; build_project_inner emits TypeScript and reports diagnostics).
    let out = unique_tmp();
    let build = build_project_inner(&dir.join("src"), &out, false).expect("build");
    assert!(
        !build.has_errors(),
        "scaffolded main.glyph did not compile: {:?}",
        build.diagnostics
    );
}

#[test]
fn re_running_never_overwrites() {
    let dir = unique_tmp();
    scaffold(&dir).expect("first scaffold");
    std::fs::write(dir.join("src/main.glyph"), "module main\n// edited by the user\n")
        .expect("edit main");

    let second = scaffold(&dir).expect("second scaffold");
    assert_eq!(second.created.len(), 0, "second run should create nothing");
    assert_eq!(second.skipped.len(), 4, "second run should skip all four files");

    let main = std::fs::read_to_string(dir.join("src/main.glyph")).unwrap();
    assert!(main.contains("edited by the user"), "user edit was clobbered");
}
