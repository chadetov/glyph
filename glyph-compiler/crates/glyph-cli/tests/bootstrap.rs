//! Guards for the agent bootstrap that `glyph llms` prints.
//!
//! `AGENTS.md` is the single source; it is embedded into the binary
//! (`glyph_cli::LLMS_BOOTSTRAP`) and mirrored to `llms.txt` and `web/llms.txt`
//! (the latter is served at glyphlang.io/llms.txt). These tests keep the
//! embedded copy real and the mirrors in step, so the three never drift.

use std::fs;
use std::path::PathBuf;

fn repo_file(rel: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "..", rel]
        .iter()
        .collect()
}

#[test]
fn embedded_bootstrap_is_the_real_agents_md() {
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("# Glyph for agents"),
        "embedded bootstrap does not look like AGENTS.md"
    );
    let on_disk = fs::read_to_string(repo_file("AGENTS.md"))
        .expect("read AGENTS.md");
    assert_eq!(
        glyph_cli::LLMS_BOOTSTRAP, on_disk,
        "the embedded bootstrap is stale; rebuild after editing AGENTS.md"
    );
}

#[test]
fn cheatsheet_jsx_example_uses_single_brace_interpolation() {
    // JSX child interpolation is single-brace `{name}` (see examples/03).
    // `${name}` in a JSX child is not template-string syntax: the `$` becomes
    // literal text and the emitted component renders "Hello, $Alice". The
    // headline Greeting example in the bootstrap must use the canonical form.
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("<span>Hello, {name}</span>"),
        "cheatsheet Greeting example lost its single-brace JSX interpolation"
    );
    assert!(
        !glyph_cli::LLMS_BOOTSTRAP.contains("<span>Hello, ${name}</span>"),
        "cheatsheet Greeting example uses `${{name}}` in a JSX child; \
         that leaks a literal `$` into rendered text (use `{{name}}`)"
    );
}

#[test]
fn cheatsheet_duration_constructor_is_namespaced() {
    // Under `import std/time`, imports are namespaced: the bare name `Duration`
    // is unresolved (E0103) and the constructor must be called as
    // `time.Duration.ms(n)`. The std/time cheatsheet must show the namespaced
    // form so the example it advertises actually resolves.
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("time.Duration.ms(n)"),
        "std/time cheatsheet lost the namespaced `time.Duration.ms(n)` form"
    );
    assert!(
        !glyph_cli::LLMS_BOOTSTRAP.contains("// Duration.ms(n)"),
        "std/time cheatsheet shows bare `Duration.ms(n)`; under a namespaced \
         `import std/time` that name is unresolved (E0103) \u{2014} use `time.Duration.ms(n)`"
    );
}

#[test]
fn root_and_web_mirrors_match_agents_md() {
    let agents = fs::read_to_string(repo_file("AGENTS.md")).expect("read AGENTS.md");
    for mirror in ["llms.txt", "web/llms.txt"] {
        let text = fs::read_to_string(repo_file(mirror))
            .unwrap_or_else(|e| panic!("read {mirror}: {e}"));
        assert_eq!(
            agents, text,
            "{mirror} has drifted from AGENTS.md; re-copy AGENTS.md over it"
        );
    }
}

#[test]
fn agents_md_inlines_every_diagnostic_code() {
    // The npm README promises the agent bootstrap carries the full diagnostic
    // catalogue. Keep that true: every `E0xxx` documented in the error-codes
    // catalogue must appear in AGENTS.md, so adding a code without a bootstrap
    // row fails here instead of silently making the README a lie.
    let catalogue = fs::read_to_string(repo_file("docs/error-codes.md")).expect("read error-codes.md");
    let agents = fs::read_to_string(repo_file("AGENTS.md")).expect("read AGENTS.md");
    // Extract every `E0` followed by exactly three digits (a diagnostic code).
    let bytes = catalogue.as_bytes();
    let mut codes: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 2] == b"E0" && bytes[i + 2..i + 5].iter().all(|b| b.is_ascii_digit()) {
            codes.push(String::from_utf8_lossy(&bytes[i..i + 5]).into_owned());
            i += 5;
        } else {
            i += 1;
        }
    }
    codes.sort();
    codes.dedup();
    let missing: Vec<&String> = codes.iter().filter(|c| !agents.contains(c.as_str())).collect();
    assert!(
        missing.is_empty(),
        "AGENTS.md is missing diagnostic codes documented in docs/error-codes.md: {missing:?} \
         (add a row to the 'Diagnostic codes' table, then re-mirror to llms.txt)"
    );
}
