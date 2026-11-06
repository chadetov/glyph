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
    let on_disk = fs::read_to_string(repo_file("AGENTS.md")).expect("read AGENTS.md");
    assert_eq!(
        glyph_cli::LLMS_BOOTSTRAP,
        on_disk,
        "the embedded bootstrap is stale; rebuild after editing AGENTS.md"
    );
}

#[test]
fn cheatsheet_jsx_example_uses_single_brace_interpolation() {
    // JSX child interpolation is single-brace `{name}` (see examples/03).
    // `${name}` in a JSX child is not template-string syntax: the `$` becomes
    // literal text and the emitted component renders "Hello, $Alice". The
    // headline Greeting example in the bootstrap must use the canonical form.
    // It reads `props.name` since E0229: a component's one parameter is its
    // props record.
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("<span>Hello, {props.name}</span>"),
        "cheatsheet Greeting example lost its single-brace JSX interpolation"
    );
    assert!(
        !glyph_cli::LLMS_BOOTSTRAP.contains("<span>Hello, ${props.name}</span>"),
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
fn cheatsheet_documents_std_random_signatures() {
    // The "standard library" paragraph names `std/random` alongside a dozen
    // other modules with no signature block of its own, unlike std/time and
    // std/record just below it. An agent asked for a seeded, reproducible
    // random number (`seeded(42).int(0, 10)`) has nothing on this page to
    // copy from and reaches for `Math.random()` or an `extern_ts` escape
    // hatch instead of the stdlib module that already does it. Guard that the
    // constructor and at least one instance method are spelled out as call
    // signatures, not just the module name.
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("seeded(seed: number) -> Rng"),
        "cheatsheet has no std/random constructor signature for `seeded`"
    );
    assert!(
        glyph_cli::LLMS_BOOTSTRAP.contains("rng.int(lo: number, hi: number) -> number"),
        "cheatsheet has no std/random `.int` method signature"
    );
}

#[test]
fn root_and_web_mirrors_match_agents_md() {
    let agents = fs::read_to_string(repo_file("AGENTS.md")).expect("read AGENTS.md");
    for mirror in ["llms.txt", "web/llms.txt"] {
        let text =
            fs::read_to_string(repo_file(mirror)).unwrap_or_else(|e| panic!("read {mirror}: {e}"));
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
        for item in glyph_cli::runtime::exported_items(&src) {
            if !item.is_type && !documented.contains(item.name.as_str()) {
                missing.push(format!("std/{module}: {}", item.name));
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
    let catalogue =
        fs::read_to_string(repo_file("docs/error-codes.md")).expect("read error-codes.md");
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
    let missing: Vec<&String> = codes
        .iter()
        .filter(|c| !agents.contains(c.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "AGENTS.md is missing diagnostic codes documented in docs/error-codes.md: {missing:?} \
         (add a row to the 'Diagnostic codes' table, then re-mirror to llms.txt)"
    );
}

#[test]
fn bootstrap_documents_the_legal_escape_sequences() {
    // The "Template strings" section covers interpolation and the raw `"""`
    // form but never listed the six escapes a string literal actually
    // decodes. An agent that writes `\q` (or guesses at `\0`, `\x41`, a bare
    // `\u1b`) gets E0001 with nothing on this page to check against, and
    // `\u{HEX}` — the escape that lets a Glyph program emit ANSI color codes
    // or other control characters — is undiscoverable by reading the
    // bootstrap at all. Guard that every legal escape is spelled out.
    for escape in ["\\n", "\\t", "\\r", "\\\"", "\\\\", "\\u{HEX}"] {
        assert!(
            glyph_cli::LLMS_BOOTSTRAP.contains(escape),
            "bootstrap is missing the escape sequence `{escape}`; an agent has \
             no on-page reference for the legal string escapes"
        );
    }
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
        let listed: BTreeSet<&str> = block.split('"').skip(1).step_by(2).collect();

        let src = std::fs::read_to_string(&path).expect("read runtime module");
        for item in glyph_cli::runtime::exported_items(&src) {
            if listed.contains(item.name.as_str()) {
                continue;
            }
            missing.push(format!("{key}: {}", item.name));
        }
    }

    assert!(
        missing.is_empty(),
        "the runtime exports names the resolver seed does not list, so importing or \
         calling them is a false E0105: {missing:?}. Add them to the module's entry in \
         `module_graph.rs`."
    );
}

/// The resolver's seed and the runtime name the same set of `std/` modules.
///
/// Since G256 the seed is the authority on which `std/` paths exist: an import
/// of one it does not hold is E0104. A runtime module left out of it would turn
/// every import of a module the compiler ships into a false error, and a seeded
/// module with no runtime file would pass every Glyph stage and fail in `tsc`,
/// which is the failure G256 removed.
#[test]
fn the_resolver_seed_and_the_runtime_name_the_same_std_modules() {
    use std::collections::BTreeSet;
    let runtime: BTreeSet<String> = fs::read_dir(repo_file("glyph-compiler/runtime/std"))
        .expect("read runtime/std")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ts"))
        .map(|p| format!("std/{}", p.file_stem().unwrap().to_string_lossy()))
        .collect();
    let seeded: BTreeSet<String> = glyph_resolver::StdlibStubs::new()
        .iter()
        .map(|(k, _)| k.to_string())
        .collect();
    let unseeded: Vec<_> = runtime.difference(&seeded).collect();
    assert!(
        unseeded.is_empty(),
        "the runtime ships std modules the resolver seed does not list, so importing them \
         is a false E0104: {unseeded:?}. Add them to `StdlibStubs::new` in `module_graph.rs`."
    );
    let phantom: Vec<_> = seeded.difference(&runtime).collect();
    assert!(
        phantom.is_empty(),
        "the resolver seed lists std modules with no runtime file, so an import of one \
         passes the Glyph stages and fails in tsc: {phantom:?}"
    );
}

/// The three host modules a browser program imports load in a page (G254).
///
/// `std/dom`, `std/storage` and `std/console` exist because `std/io` imports
/// `node:fs` at load time and so cannot go into a bundle. A `node:` import
/// anywhere in their import closure would put them back in that position, and
/// nothing else would notice: node runs them, `tsc` accepts them, and only a
/// bundler building a page refuses. So this walks each one's sibling imports
/// to a fixed point and reads every import specifier in the closure.
#[test]
fn the_browser_host_modules_import_nothing_from_node() {
    use std::collections::BTreeSet;
    fn specifiers(src: &str) -> Vec<String> {
        // Every `from "x"` and side-effect `import "x"`, read from code lines
        // only, so a comment that names `node:fs` is not an import of it.
        let mut out = Vec::new();
        for line in src.lines() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with('*') || code.starts_with("/*") {
                continue;
            }
            for marker in ["from \"", "import \"", "import(\""] {
                let mut rest = code;
                while let Some(at) = rest.find(marker) {
                    let after = &rest[at + marker.len()..];
                    let end = after.find('"').unwrap_or(after.len());
                    out.push(after[..end].to_string());
                    rest = &after[end..];
                }
            }
        }
        out
    }
    let dir = repo_file("glyph-compiler/runtime/std");
    for root in ["dom", "storage", "console"] {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut todo = vec![root.to_string()];
        while let Some(m) = todo.pop() {
            if !seen.insert(m.clone()) {
                continue;
            }
            let src = fs::read_to_string(dir.join(format!("{m}.ts")))
                .unwrap_or_else(|e| panic!("read std/{m}.ts: {e}"));
            for spec in specifiers(&src) {
                match spec.strip_prefix("./") {
                    Some(sibling) => todo.push(sibling.to_string()),
                    None => panic!(
                        "std/{root} reaches std/{m}, which imports `{spec}`; a browser bundle \
                         cannot load it. Its whole import closure must be host-free."
                    ),
                }
            }
            assert!(
                !src.contains("\"node:"),
                "std/{m} (in std/{root}'s closure) names a `node:` module in code"
            );
        }
        assert!(seen.contains(root), "std/{root} was not read");
    }
}

/// The type-only table the emitter reads must match the runtime it describes.
///
/// `is_stdlib_type_only` decides whether an emitted import carries the inline
/// `type` modifier. A name missing from the table emits without it, which
/// `tsc` elides and a type-stripper does not, so the import fails to link
/// against a module with no such runtime export (G114) — and the build stays
/// green the whole way, because `tsc` is what `glyph build` runs.
///
/// A name in the table that the runtime does export as a value would be marked
/// `type` and then *elided*, so the program would lose a binding it needs. Both
/// directions are checked here.
#[test]
fn stdlib_type_only_exports_match_the_runtime() {
    use std::collections::BTreeSet;
    let runtime = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../runtime/std"));
    let mut declared: BTreeSet<(String, String)> = BTreeSet::new();
    for (module, name) in glyph_resolver::stdlib_type_only_pairs() {
        declared.insert((module.to_string(), name.to_string()));
    }

    let mut actual: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in fs::read_dir(runtime).expect("read runtime/std") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ts") {
            continue;
        }
        let module = format!("std/{}", path.file_stem().unwrap().to_str().unwrap());
        let src = fs::read_to_string(&path).expect("read std module");
        let (mut types, mut values) = (BTreeSet::new(), BTreeSet::new());
        for item in glyph_cli::runtime::exported_items(&src) {
            if item.is_type {
                types.insert(item.name);
            } else {
                values.insert(item.name);
            }
        }
        // Type-only means: declared as a type, with no value of the same name.
        // A Glyph-declared type always ships a descriptor `const`, which is why
        // only the hand-written standard library needs this table at all.
        for name in types.difference(&values) {
            actual.insert((module.clone(), name.clone()));
        }
    }

    let missing: Vec<_> = actual.difference(&declared).collect();
    assert!(
        missing.is_empty(),
        "the runtime declares a type-only export the emitter does not know about, so an \
         import of it emits without `type` and will not link once types are stripped: \
         {missing:?}. Add it to STDLIB_TYPE_ONLY in `stdlib_types.rs`."
    );

    let stale: Vec<_> = declared.difference(&actual).collect();
    assert!(
        stale.is_empty(),
        "the table names something the runtime does not export as a type-only name, so its \
         import would be marked `type` and elided, losing a binding the program needs: \
         {stale:?}"
    );
}

/// The checker's table of the stdlib's string-literal unions is the runtime's
/// declarations, union for union and literal for literal (G294). A literal the
/// runtime adds and the table lacks would let a `match` the checker calls
/// exhaustive throw at run time; one the table keeps after the runtime drops
/// it would demand an arm for a value that never arrives.
#[test]
fn stdlib_string_literal_unions_match_the_runtime() {
    use std::collections::BTreeMap;
    let runtime = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../runtime/std"));
    let declared: BTreeMap<(String, String), Vec<String>> =
        glyph_typechecker::ty::BUILTIN_STRING_LITERAL_UNIONS
            .iter()
            .map(|(m, n, vs)| {
                ((m.to_string(), n.to_string()), vs.iter().map(|v| v.to_string()).collect())
            })
            .collect();

    let mut actual: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for entry in fs::read_dir(runtime).expect("read runtime/std") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ts") {
            continue;
        }
        let module = format!("std/{}", path.file_stem().unwrap().to_str().unwrap());
        let src = fs::read_to_string(&path).expect("read std module");
        // `export type Name = "a" | "b";`, on one line or spread over several.
        let mut rest = src.as_str();
        while let Some(at) = rest.find("export type ") {
            rest = &rest[at + "export type ".len()..];
            let Some(eq) = rest.find('=') else { break };
            let name = rest[..eq].trim();
            let Some(end) = rest[eq..].find(';') else { break };
            let body = &rest[eq + 1..eq + end];
            let members: Vec<&str> =
                body.split('|').map(str::trim).filter(|m| !m.is_empty()).collect();
            let literal = |m: &&str| m.len() >= 2 && m.starts_with('"') && m.ends_with('"');
            if !members.is_empty()
                && members.iter().all(literal)
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                actual.insert(
                    (module.clone(), name.to_string()),
                    members.iter().map(|m| m[1..m.len() - 1].to_string()).collect(),
                );
            }
        }
    }
    assert!(!actual.is_empty(), "the scan found no union at all, so it is not reading the runtime");
    assert_eq!(
        declared, actual,
        "BUILTIN_STRING_LITERAL_UNIONS in `glyph-typechecker/src/ty.rs` disagrees with the \
         runtime's `export type X = \"a\" | \"b\"` declarations (left: the table, right: the runtime)"
    );
}

/// Every HTTP verb takes its deadline from `fetch_of`, so the 30 000 ms default
/// is written in one place (G128).
///
/// The default cannot be observed behaviourally without waiting it out, which
/// a test suite does not do, so this reads the runtime source instead, the way
/// `the_resolver_seed_lists_every_runtime_export` does. Two facts hold the
/// guarantee: no verb spells its own bounds (each body builds its `Fetch` with
/// `fetch_of`), and the literal `timeout_ms: 0`, which used to be the default
/// in three places, appears nowhere in the module. A verb that regrows its own
/// literal fails here even though every behavioural test still passes.
#[test]
fn every_http_verb_takes_its_deadline_from_fetch_of() {
    let src = fs::read_to_string(repo_file("glyph-compiler/runtime/std/http.ts"))
        .expect("read runtime/std/http.ts");
    assert!(
        !src.contains("timeout_ms: 0"),
        "runtime/std/http.ts writes `timeout_ms: 0` somewhere, so a request built there is \
         unbounded; the default belongs in fetch_of alone"
    );
    for verb in ["get", "post", "put", "patch", "del", "head"] {
        let header = format!("export async function {verb}(");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("http.ts no longer exports `{verb}`"));
        let body = &src[start..];
        let end = body.find("\n}\n").expect("a function body ends");
        let body = &body[..end];
        assert!(
            body.contains("fetch_of("),
            "http.{verb} does not build its request with fetch_of, so it does not carry the \
             default deadline:\n{body}"
        );
    }
}

/// The bootstrap's tool section names the tools the server serves, and counts
/// them correctly.
///
/// `AGENTS.md` said the MCP server "exposes five tools" for two releases while
/// it served seven, and the two the server's own `instructions` string tells an
/// agent to reach for first appeared in the document zero times (G222). A
/// sentence nothing checks is how that survives, so this reads `tool_specs()`
/// and the document and compares them in both directions.
#[test]
fn the_bootstrap_names_every_tool_the_server_serves_and_no_other() {
    let agents = fs::read_to_string(repo_file("AGENTS.md")).expect("read AGENTS.md");
    let specs = glyph_lsp::tool_specs();
    let served: Vec<String> = specs
        .as_array()
        .expect("tool_specs answers an array")
        .iter()
        .filter_map(|t| t.get("name")?.as_str().map(str::to_string))
        .collect();
    assert!(!served.is_empty(), "the server serves no tools");

    // Every tool the server serves is in the document.
    for name in &served {
        assert!(
            agents.contains(name.as_str()),
            "the server serves `{name}` and AGENTS.md never names it"
        );
    }

    // Every `glyph_*` name the document uses is a tool the server serves.
    let mut named: Vec<String> = Vec::new();
    let bytes = agents.as_bytes();
    let mut i = 0;
    while let Some(at) = agents[i..].find("glyph_") {
        let start = i + at;
        let mut end = start + "glyph_".len();
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        named.push(agents[start..end].to_string());
        i = end;
    }
    named.sort();
    named.dedup();
    for name in &named {
        assert!(
            served.contains(name),
            "AGENTS.md names `{name}`, which the server does not serve. The tools are: {served:?}"
        );
    }

    // The count sentence agrees with the tool list.
    const NUMBERS: &[(&str, usize)] = &[
        ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6),
        ("seven", 7), ("eight", 8), ("nine", 9), ("ten", 10), ("eleven", 11),
        ("twelve", 12), ("thirteen", 13), ("fourteen", 14), ("fifteen", 15),
    ];
    let claim = agents
        .split("exposes ")
        .nth(1)
        .and_then(|rest| rest.split(" tools").next())
        .map(str::to_string);
    let claim = claim.unwrap_or_else(|| {
        panic!("AGENTS.md no longer says how many tools the server exposes; it must, or this gate checks nothing")
    });
    let claimed = NUMBERS
        .iter()
        .find(|(word, _)| *word == claim)
        .map(|(_, n)| *n)
        .or_else(|| claim.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("`exposes {claim} tools` is not a number this gate can read"));
    assert_eq!(
        claimed,
        served.len(),
        "AGENTS.md says the server exposes {claimed} tools and it serves {}",
        served.len()
    );
}

/// `glyph mcp --help` names every tool the server serves, and counts them.
///
/// The help listed five tools (diagnostics, hover, definition, references,
/// symbols) while the server served eleven. It is a doc comment on the clap
/// command, so nothing generated it and nothing checked it; this reads
/// `tool_specs()` and the help the binary prints and compares them.
#[test]
fn mcp_help_names_every_tool_the_server_serves() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(["mcp", "--help"])
        .output()
        .expect("run glyph mcp --help");
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    let served: Vec<String> = glyph_lsp::tool_specs()
        .as_array()
        .expect("tool_specs answers an array")
        .iter()
        .filter_map(|t| t.get("name")?.as_str().map(str::to_string))
        .collect();
    assert!(!served.is_empty(), "the server serves no tools");
    let words: Vec<&str> = help
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .collect();
    for name in &served {
        let short = name.strip_prefix("glyph_").unwrap_or(name);
        assert!(
            words.contains(&short),
            "the server serves `{name}` and `glyph mcp --help` does not name `{short}`:\n{help}"
        );
    }
    const NUMBERS: &[(&str, usize)] = &[
        ("five", 5), ("six", 6), ("seven", 7), ("eight", 8), ("nine", 9), ("ten", 10),
        ("eleven", 11), ("twelve", 12), ("thirteen", 13), ("fourteen", 14), ("fifteen", 15),
        ("sixteen", 16), ("seventeen", 17), ("eighteen", 18), ("nineteen", 19), ("twenty", 20),
    ];
    let claim = help
        .split(" as ")
        .nth(1)
        .and_then(|rest| rest.split(" tools").next())
        .map(str::trim)
        .unwrap_or_else(|| panic!("`glyph mcp --help` no longer says how many tools it serves:\n{help}"));
    let claimed = NUMBERS
        .iter()
        .find(|(w, _)| *w == claim)
        .map(|(_, n)| *n)
        .unwrap_or_else(|| panic!("`as {claim} tools` is not a number this gate can read"));
    assert_eq!(claimed, served.len(), "`glyph mcp --help` says {claimed} tools:\n{help}");
}
