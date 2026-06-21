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
