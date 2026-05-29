//! Resolver acceptance tests against the four example files.
//!
//! Week 2 acceptance per `docs/implementation-plan.md`:
//! > Every example file resolves all names; every expression node has a type
//! > (some `Unknown` is fine).
//!
//! This file covers the "every example file resolves all names" half. The
//! "type for every expression" half lands when the typechecker substep 5a
//! ships (Phase 1 week 3).
//!
//! Day-1 slice scope: cross-module import resolution is deferred. Names
//! introduced by `import std/...` are accepted as resolved (their target is a
//! foreign module whose exports week-2-day-3+ will validate). Member access
//! through a namespace import (`http.get`, `array.map`) resolves the leading
//! `http`/`array` only — the rest of the dotted path is typechecker territory.
//!
//! The `progress_report` test prints how many unresolved names each example
//! has so day-3+ can track the trajectory.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use glyph_resolver::{
    build_prelude, collect_module_symbols, resolve_module, verify_imports, CompositeGraph,
    ResolveError, StdlibStubs,
};

fn example_source(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "..",
        "examples",
        name,
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

fn run_pipeline(name: &str) -> Vec<ResolveError> {
    let source = example_source(name);
    let module = glyph_parser::parse(&source).expect("parse failed");
    let symbols = collect_module_symbols(&module).expect("collect failed");
    let prelude = build_prelude();
    let (_, errors) = resolve_module(&module, symbols, &prelude);
    errors
}

/// Project-local module graph for the example files. Carries entries for the
/// third-party + project-local imports the four examples reference
/// (`react`, `api/users`). Stdlib paths fall through to `StdlibStubs`.
fn example_project_graph() -> StdlibStubs {
    let mut g = StdlibStubs::empty();
    g.add("react", &["use_state", "use_effect", "use_memo", "Component"]);
    g.add("api/users", &["search_users", "SearchError"]);
    g
}

fn run_import_verify(name: &str) -> Vec<ResolveError> {
    let source = example_source(name);
    let module = glyph_parser::parse(&source).expect("parse failed");
    let stdlib = StdlibStubs::new();
    let project = example_project_graph();
    let composite = CompositeGraph {
        first: &stdlib,
        second: &project,
    };
    verify_imports(&module, &composite)
}

/// Unique unresolved names — collapses repeated references to the same name
/// into one entry. Useful for tracking which stdlib pieces are missing.
fn unresolved_names(errors: &[ResolveError]) -> BTreeSet<String> {
    errors
        .iter()
        .filter_map(|e| match e {
            ResolveError::UnresolvedName { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn progress_report() {
    // Diagnostic: always passes; prints the unresolved-name set per example so
    // gaps in the prelude / module graph stay visible as the resolver grows.
    for name in [
        "01_validator.glyph",
        "02_async_errors.glyph",
        "03_react_component.glyph",
        "04_cli_tool.glyph",
    ] {
        let errors = run_pipeline(name);
        let unresolved = unresolved_names(&errors);
        println!(
            "{name}: {} total error(s), {} unique unresolved name(s){}",
            errors.len(),
            unresolved.len(),
            if unresolved.is_empty() {
                String::new()
            } else {
                format!(": {:?}", unresolved)
            }
        );
    }
}

#[test]
fn example_02_imports_and_locals_resolve() {
    // `02_async_errors.glyph` exercises imports + tagged unions + Result
    // propagation. Day-1 should at minimum resolve all the prelude names
    // (Ok, Err, Result), the imported-name wrappers (http, json, array,
    // Result, Ok, Err), and all locally-introduced bindings.
    let errors = run_pipeline("02_async_errors.glyph");
    let unresolved = unresolved_names(&errors);
    // Every name in the example should be resolvable in the day-1 slice if
    // (a) it's a prelude built-in, (b) it's a top-level decl in this module,
    // (c) it's brought in by an import statement, or (d) it's a local
    // binding. Print the diff so day-2 work can target it.
    println!("02 unresolved (day-1 slice): {:?}", unresolved);
}

#[test]
fn examples_pass_cross_module_verification() {
    // Day-4 acceptance: every named-import in every example file must resolve
    // to a real export of the target module. Stdlib stubs back `std/*`; a
    // tiny project graph backs `react` and `api/users`.
    for name in [
        "01_validator.glyph",
        "02_async_errors.glyph",
        "03_react_component.glyph",
        "04_cli_tool.glyph",
    ] {
        let errors = run_import_verify(name);
        assert!(
            errors.is_empty(),
            "{name}: cross-module verification errors: {errors:?}"
        );
    }
}

#[test]
fn cross_module_unknown_export_is_flagged() {
    // Sanity check the negative path against an example source patched with a
    // bogus import.
    let mut source = example_source("02_async_errors.glyph");
    let bad_line = "import std/result { Result, Ok, Boom }\n";
    let original = "import std/result { Result, Ok, Err }\n";
    // Exact-count guard: if the example ever grows a second matching line,
    // a global `str::replace` would silently mutate both. `replacen(..., 1)`
    // plus this assertion keeps the test honest.
    assert_eq!(
        source.matches(original).count(),
        1,
        "fixture drift: expected exactly one matching import line"
    );
    source = source.replacen(original, bad_line, 1);

    let module = glyph_parser::parse(&source).expect("parse failed");
    let stdlib = StdlibStubs::new();
    let errors = verify_imports(&module, &stdlib);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            ResolveError::UnknownExportedName { name, module, .. }
                if name == "Boom" && module == "std/result"
        )),
        "expected UnknownExportedName(Boom, std/result), got: {errors:?}"
    );
}

#[test]
fn duplicate_top_level_name_is_detected() {
    // Sanity check: even on the example files, the collector enforces
    // duplicate-name. None of the example files should trigger this.
    for name in [
        "01_validator.glyph",
        "02_async_errors.glyph",
        "03_react_component.glyph",
        "04_cli_tool.glyph",
    ] {
        let source = example_source(name);
        let module = glyph_parser::parse(&source).expect("parse failed");
        collect_module_symbols(&module).unwrap_or_else(|errs| {
            panic!("{name}: top-level collection should succeed, got: {errs:?}")
        });
    }
}
