//! Drift guard for the `stdlib_fn_ty` table in `assign.rs`.
//!
//! The table hand-models stdlib signatures, and the docs generalize from it:
//! `docs/reference/stdlib.md`, `AGENTS.md` and `llms.txt` all promise that a
//! `match` over a stdlib error's `kind` is held to the same exhaustiveness bar
//! as a union you declared. That promise only holds for functions the table
//! actually carries. A `Result`-returning export the table forgets types
//! `Unknown`, its `match` is unchecked, and a missing arm becomes a run-time
//! `throw` on a build that passed `tsc --strict`. `fs.append_text` and
//! `fs.make_dir` shipped in exactly that state.
//!
//! So: every `Result`-returning export under `runtime/std/*.ts` is either in the
//! table or on the exclusion list below, with a reason. Adding a stdlib function
//! that returns a `Result` and forgetting to model it fails the build.

use std::fs;
use std::path::PathBuf;

use glyph_ast::{Decl, Stmt};
use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};
use glyph_typechecker::{assign_types, Ty};

/// `Result`-returning stdlib exports that are deliberately not modeled, each
/// with the reason. These are the functions whose `Ok`/`Err` types the v1 table
/// cannot express, not ones that were overlooked.
const EXCLUDED: &[(&str, &str, &str)] = &[
    (
        "std/json",
        "parse",
        "generic in the parsed type; the Ok type comes from the call-site \
         annotation, which the table cannot express",
    ),
    (
        "std/json",
        "parse_with",
        "generic in the parsed type; the Ok type comes from the schema argument",
    ),
    (
        "std/result",
        "Ok",
        "prelude constructor, typed by the prelude rather than by this table",
    ),
    (
        "std/result",
        "Err",
        "prelude constructor, typed by the prelude rather than by this table",
    ),
    (
        "std/decimal",
        "decimal",
        "returns Result<Decimal, string>; Decimal is not yet a modeled stdlib type",
    ),
    (
        "std/http",
        "serve",
        "returns Promise<Result<void, string>>; the error is a bare string, so \
         modeling it buys no exhaustiveness",
    ),
    (
        "std/test",
        "property",
        "optional trailing `count` argument, so a modeled arity would report a \
         false E0213 on every two-argument call",
    ),
];

fn runtime_std_dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "runtime", "std"]
        .iter()
        .collect()
}

/// Every `export function` / `export async function` in `source` whose return
/// type mentions `Result<`, as `(name, is_result)` pairs.
///
/// Signatures can span several lines, so the declaration is joined from the
/// `export` line up to the line that opens the body, and the return type is
/// read as whatever follows the last `): ` in that joined text.
fn result_returning_exports(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let rest = match trimmed.strip_prefix("export ") {
            Some(r) => r,
            None => continue,
        };
        let rest = rest.strip_prefix("async ").unwrap_or(rest);
        let rest = match rest.strip_prefix("function ") {
            Some(r) => r,
            None => continue,
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let mut joined = String::new();
        for line in lines.iter().skip(i) {
            joined.push(' ');
            joined.push_str(line.trim());
            if line.trim_end().ends_with('{') {
                break;
            }
        }
        let return_ty = match joined.rfind("): ") {
            Some(at) => &joined[at + 3..],
            None => continue,
        };
        if return_ty.contains("Result<") {
            out.push(name);
        }
    }
    out
}

/// Whether the checker gives `<module>.<name>` a function type returning a
/// prelude `Result`. Probed through the real pipeline (a member access on a
/// namespace import) rather than by calling the private table directly.
fn models_a_result(module_key: &str, name: &str) -> bool {
    let namespace = module_key.rsplit('/').next().unwrap();
    let src = format!(
        "module x\nimport {module_key}\n\
         fn probe() -> void {{\n  let f = {namespace}.{name}\n  return void\n}}\n"
    );
    let module = glyph_parser::parse(&src).expect("parse failed");
    let symbols = collect_module_symbols(&module).expect("collect failed");
    let prelude = build_prelude();
    let (resolved, _errs) = resolve_module(&module, symbols, &prelude);
    let (tm, _ty_errs) = assign_types(&module, &resolved, &prelude);

    let Decl::Fn(f) = &module.items[1] else {
        panic!("second decl is not a fn");
    };
    let Stmt::Let(l) = &f.body.stmts[0] else {
        panic!("first stmt is not a let");
    };
    let Ty::Fn { return_ty, .. } = tm.get(l.value.span()) else {
        return false;
    };
    matches!(
        &**return_ty,
        Ty::App { base, .. }
            if matches!(&**base, Ty::Named { path, .. } if path.last().map(|s| s.as_ref()) == Some("Result"))
    )
}

#[test]
fn every_result_returning_stdlib_export_is_modeled_or_excluded() {
    let dir = runtime_std_dir();
    let mut modules: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ts"))
        .collect();
    modules.sort();

    let mut unmodeled: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for path in &modules {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let module_key = format!("std/{stem}");
        let source =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        for name in result_returning_exports(&source) {
            if EXCLUDED
                .iter()
                .any(|(m, n, _)| *m == module_key && *n == name)
            {
                continue;
            }
            checked += 1;
            if !models_a_result(&module_key, &name) {
                unmodeled.push(format!("{module_key}: `{name}`"));
            }
        }
    }

    assert!(
        checked >= 12,
        "expected the runtime to have many modeled Result-returning exports, found {checked}"
    );
    assert!(
        unmodeled.is_empty(),
        "{} Result-returning stdlib export(s) are missing from `stdlib_fn_ty` in \
         glyph-typechecker/src/assign.rs. An unmodeled one types `Unknown`, so a \
         `match` over its error is not exhaustiveness-checked and a missing arm \
         throws at run time. Add a table row, or add it to EXCLUDED here with a \
         reason:\n  {}",
        unmodeled.len(),
        unmodeled.join("\n  ")
    );
}

/// The exclusion list is a promise that each entry was considered. Guard it
/// against becoming stale: an entry naming a function the runtime no longer
/// exports is dead weight that hides the next real hole.
#[test]
fn every_exclusion_names_a_real_runtime_export() {
    let dir = runtime_std_dir();
    let mut stale: Vec<String> = Vec::new();
    for (module_key, name, _reason) in EXCLUDED {
        let stem = module_key.rsplit('/').next().unwrap();
        let path = dir.join(format!("{stem}.ts"));
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                stale.push(format!("{module_key}: no such runtime module"));
                continue;
            }
        };
        if !result_returning_exports(&source).iter().any(|n| n == name) {
            stale.push(format!(
                "{module_key}: `{name}` is not a Result-returning export"
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "stale EXCLUDED entries:\n  {}",
        stale.join("\n  ")
    );
}
