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
fn cheatsheet_shows_both_std_time_import_lines() {
    // The two import lines buy different names, and the cheatsheet used to show
    // only one. Under `import std/time` everything is namespaced: the bare name
    // `Duration` is unresolved (E0103) and the constructor is
    // `time.Duration.ms(n)`. The bare `Duration`, in type position as well as
    // value position, comes from `import std/time { Duration }`. A reader who
    // sees only the first line has no answer to "how do I write `x: Duration`?",
    // so both must be on the page.
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("time.Duration.ms(n)"),
        "std/time cheatsheet lost the namespaced `time.Duration.ms(n)` form"
    );
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("import std/time { Duration }"),
        "std/time cheatsheet does not say which import buys the bare `Duration`"
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
fn stdlib_reference_documents_every_runtime_export() {
    // Guard against docs/reference/stdlib.md drifting behind the real stdlib
    // surface. A runtime function added without a doc entry sends an agent
    // reaching for an escape hatch for something that already exists
    // (time.format_iso was undocumented and did exactly that). Every
    // `export function`/`export const` in a std/*.ts must appear in the
    // reference. Substring-by-word, so a genuine internal helper can be named in
    // any prose form; the guard only catches a wholly absent export.
    let reference =
        fs::read_to_string(repo_file("docs/reference/stdlib.md")).expect("read stdlib.md");
    let documented: std::collections::HashSet<&str> = reference
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .collect();
    let std_dir = repo_file("glyph-compiler/runtime/std");
    let mut missing: Vec<String> = Vec::new();
    for entry in fs::read_dir(&std_dir).expect("read std dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ts") {
            continue;
        }
        let module = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let src = fs::read_to_string(&path).expect("read std file");
        for line in src.lines() {
            let trimmed = line.trim_start();
            let rest = trimmed
                .strip_prefix("export function ")
                .or_else(|| trimmed.strip_prefix("export const "));
            if let Some(rest) = rest {
                let name = rest
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if !name.is_empty() && !documented.contains(name) {
                    missing.push(format!("std/{module}: {name}"));
                }
            }
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "stdlib exports missing from docs/reference/stdlib.md (document them): {missing:?}"
    );
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

/// The resolver's export seed lists every name the runtime actually exports.
///
/// The seed decides two checks: `import std/fs { write }` (E0105) and, since
/// G27, `fs.write(...)` through a namespace. A name the runtime exports but the
/// seed omits turns a working call into an error, and until the namespace check
/// existed the gap was invisible: nothing named-imports most of these. That is
/// how a test fixture came to call `fs.write` where the function is
/// `write_text`, under `--no-tsc`, passing for months.
#[test]
fn the_resolver_seed_lists_every_runtime_export() {
    use std::collections::BTreeSet;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let seed_src = std::fs::read_to_string(root.join("crates/glyph-resolver/src/module_graph.rs"))
        .expect("read module_graph.rs");

    let mut missing: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(root.join("runtime/std")).expect("read runtime/std") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ts") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let key = format!("std/{stem}");
        // Only modules the seed already claims to describe.
        if !seed_src.contains(&format!("\"{key}\"")) {
            continue;
        }
        // The seed for one module is the `&[ ... ]` slice after its key.
        let after = match seed_src.split_once(&format!("\"{key}\"")) {
            Some((_, rest)) => rest,
            None => continue,
        };
        // The slice ends at `],`, not `];` — getting this wrong made the check
        // silently skip every module, which is why it is negative-tested.
        let block = match after.split_once("],") {
            Some((b, _)) => b,
            None => panic!("{key}: could not find the end of its seed list"),
        };
        let listed: BTreeSet<&str> = block
            .split('"')
            .skip(1)
            .step_by(2)
            .collect();

        let src = std::fs::read_to_string(&path).expect("read runtime module");
        for line in src.lines() {
            let name = line
                .strip_prefix("export function ")
                .or_else(|| line.strip_prefix("export async function "))
                .or_else(|| line.strip_prefix("export type "))
                .or_else(|| line.strip_prefix("export const "));
            let Some(rest) = name else { continue };
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if ident.is_empty() || listed.contains(ident.as_str()) {
                continue;
            }
            missing.push(format!("{key}: {ident}"));
        }
    }

    assert!(
        missing.is_empty(),
        "the runtime exports names the resolver seed does not list, so importing or \
         calling them is a false E0105: {missing:?}. Add them to the module's entry in \
         `module_graph.rs`."
    );
}
