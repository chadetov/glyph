//! WebAssembly bindings for the Glyph compiler.
//!
//! Exposes one function, [`compile`], that turns a Glyph source string into the
//! emitted TypeScript plus diagnostics, entirely in memory: the same front end
//! `glyph build` runs (parse, resolve, typecheck, emit), with no filesystem, so
//! it runs in a browser. This is the engine behind the web playground.
//!
//! The dependency set is deliberately the WASM-safe core only (lexer, ast,
//! parser, resolver, typechecker, emit). It does NOT pull in `glyph-db` (salsa),
//! `glyph-cli` (filesystem), or `glyph-lsp` (tokio), none of which target
//! `wasm32-unknown-unknown`.
//!
//! **The phases are the same; the input is not, and the difference is visible
//! in the output.** `glyph build` compiles a project and hands the emitter a
//! [`ProjectTables`] built from every module in it. This surface has one source
//! string, and six of those tables are read only for an *imported* name, so a
//! module importing a sibling emits a bare specifier here where `glyph build`
//! emits a relative one, and validates a field typed by that sibling with a
//! presence check where `glyph build` writes the check the type declares. No
//! amount of shared code closes that: the sibling's source is not here. What
//! shared code does buy is that a table the emitter grows reaches both surfaces
//! at once, and that this one reports exactly which imports carry the caveat,
//! in `assumed_external_imports`. See G170 in `docs/dogfooding-gaps.md`.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use glyph_emit::{emit_module, ProjectTables};
use glyph_lexer::Span;
use glyph_resolver::{
    build_prelude, collect_module_symbols, module_lints, path_key, resolve_module, verify_imports,
    StdlibStubs,
};
use glyph_typechecker::assign_types;

/// Install a panic hook so a Rust panic surfaces in the browser console as a
/// real message instead of an opaque `unreachable`. Called once when the module
/// is instantiated.
#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// One diagnostic, in both byte and line/character (UTF-16) coordinates so a
/// browser editor can place a marker without re-deriving positions.
#[derive(Serialize)]
struct Diagnostic {
    code: String,
    message: String,
    /// `"error"` or `"warning"`, read off the diagnostic itself rather than
    /// assumed. An advisory lint and a `Result` that is never used are
    /// warnings in `glyph check`, and reporting them as errors here would make
    /// the playground disagree with the compiler about whether a program is
    /// broken.
    severity: &'static str,
    start_byte: u32,
    end_byte: u32,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

/// The result of compiling one Glyph source string.
#[derive(Serialize)]
struct CompileOutput {
    /// The emitted TypeScript, or `null` if a parse/symbol error stopped the
    /// pipeline before emission.
    ts: Option<String>,
    /// Every diagnostic from every phase. Empty means a clean compile.
    diagnostics: Vec<Diagnostic>,
    /// Every import path this compile treated as an external npm package
    /// because it has no project to check it against.
    ///
    /// `glyph build` resolves an import against every `.glyph` file in the
    /// project. This surface has one source string, so for any path that is
    /// not `std/*` or `extern/*` it cannot tell a sibling module from a
    /// package, and it assumes a package. When the assumption is wrong the
    /// emitted TypeScript is not what `glyph build` writes: the specifier
    /// stays bare instead of becoming relative, and a field typed by that
    /// module's type falls back to a presence check instead of the check the
    /// type declares. Naming the paths lets the page say exactly which imports
    /// carry that caveat, and say nothing at all when none do.
    assumed_external_imports: Vec<String>,
}

/// Compile a Glyph source string. Returns a JSON string of `CompileOutput`
/// (`{ ts, diagnostics }`) so the browser side needs no schema knowledge beyond
/// `JSON.parse`.
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let out = compile_inner(source);
    serde_json::to_string(&out).unwrap_or_else(|_| {
        String::from(r#"{"ts":null,"diagnostics":[{"code":"E9999","message":"internal serialization error","severity":"error","start_byte":0,"end_byte":0,"start_line":0,"start_col":0,"end_line":0,"end_col":0}]}"#)
    })
}

/// The compiler version this WASM module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn compile_inner(source: &str) -> CompileOutput {
    let index = LineIndex::new(source);

    let module = match glyph_parser::parse(source) {
        Ok(m) => m,
        Err(e) => {
            return CompileOutput {
                ts: None,
                diagnostics: vec![error(&index, source, e.span(), e.code(), &format!("{e}"), e.help().as_deref())],
                assumed_external_imports: Vec::new(),
            };
        }
    };

    // Which imports this compile cannot check against a project. Computed off
    // the AST, so it is available even when a later phase stops the pipeline.
    let assumed_external_imports = assumed_external_imports(&module);

    let mut diagnostics = Vec::new();

    let symbols = match collect_module_symbols(&module) {
        Ok(s) => s,
        Err(errors) => {
            // Symbol collection failed (duplicate decl, D15 barrel file, …);
            // later phases need the table, so report and stop.
            for e in &errors {
                diagnostics.push(resolve_diag(&index, source, e));
            }
            return CompileOutput { ts: None, diagnostics, assumed_external_imports };
        }
    };

    // Import verification against the stdlib stub graph (a single open file has
    // no project graph, so sibling/external imports are permissively skipped).
    let stdlib = StdlibStubs::new();
    for e in verify_imports(&module, &stdlib) {
        diagnostics.push(resolve_diag(&index, source, &e));
    }

    let prelude = build_prelude();
    let (resolved, resolve_errors) = resolve_module(&module, symbols, &prelude);
    for e in &resolve_errors {
        diagnostics.push(resolve_diag(&index, source, e));
    }

    let (types, type_errors) = assign_types(&module, &resolved, &prelude);
    for e in &type_errors {
        let severity = match e.severity() {
            glyph_typechecker::Severity::Error => "error",
            _ => "warning",
        };
        diagnostics.push(diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help(), severity));
    }

    // Advisory lints (unused import, unused binding, unreachable code), which
    // `glyph check` and `glyph build` both report. They are computed only when
    // nothing errored, exactly as `glyph build` computes them: on a module that
    // did not resolve cleanly the resolution map is incomplete, and a binding
    // that is used would be reported as dead.
    //
    // The one lint deliberately not run here is G124's reachability warning. It
    // asks whether any *other* module in the project imports this one, and this
    // surface has no other module. Answering it either way would be a guess, so
    // it is absent.
    if !diagnostics.iter().any(|d| d.severity == "error") {
        for e in module_lints(&module, &resolved) {
            diagnostics.push(resolve_diag(&index, source, &e));
        }
    }

    // The project scan, run over the modules this surface has, which is one.
    // Sharing `ProjectTables` with `glyph build` is what keeps a table the
    // emitter grows from reaching one surface and not the other; it does not
    // conjure the sibling sources this surface does not hold, which is what
    // `assumed_external_imports` above exists to say out loud.
    let tables = ProjectTables::from_modules([(PLAYGROUND_MODULE_PATH, Some(&module))]);

    // Emit best-effort: the playground shows the TypeScript even when later-phase
    // diagnostics exist (it is what the writer is iterating toward). A genuine
    // emit error is reported and yields no TS.
    let ts = match emit_module(
        &module,
        &resolved,
        &types,
        &prelude,
        tables.emit_context(PLAYGROUND_MODULE_PATH),
    ) {
        Ok(ts) => Some(ts),
        Err(e) => {
            diagnostics.push(error(&index, source, e.span(), e.code(), &format!("{e}"), e.help()));
            None
        }
    };

    CompileOutput { ts, diagnostics, assumed_external_imports }
}

/// The module path this surface compiles under. Empty, which
/// `runtime_specifier` reads as depth zero, so the emitted `.glyph-runtime/`
/// and sibling specifiers are the ones a module at the project *root* gets.
/// A `module sub/a` header in the source does not change it, because it does
/// not change it in `glyph build` either: there the path comes from where the
/// file sits, and the header is only a diagnostic anchor.
const PLAYGROUND_MODULE_PATH: &str = "";

/// Every import path this surface has to treat as an external npm package
/// because it holds no project to check it against. `std/*` and `extern/*` are
/// excluded: both emit the same specifier here as in `glyph build`, and neither
/// consults a project table. Sorted and deduplicated so the page renders the
/// same list for the same source.
fn assumed_external_imports(module: &glyph_ast::Module) -> Vec<String> {
    let mut paths: std::collections::BTreeSet<String> = Default::default();
    for item in &module.items {
        let glyph_ast::Decl::Import(imp) = item else {
            continue;
        };
        let path = path_key(&imp.path);
        if path.starts_with("std/") || path.starts_with("extern/") {
            continue;
        }
        paths.insert(path);
    }
    paths.into_iter().collect()
}

/// Build a `Diagnostic` from a resolver error, reading its severity off the
/// error rather than assuming one. The resolver is the phase that produces both
/// hard errors and advisory lints, so this is where getting the severity wrong
/// would paint a lint red.
fn resolve_diag(
    index: &LineIndex,
    source: &str,
    e: &glyph_resolver::ResolveError,
) -> Diagnostic {
    let severity = match e.severity() {
        glyph_resolver::Severity::Error => "error",
        _ => "warning",
    };
    diag(index, source, e.span(), e.code(), &format!("{e}"), e.help(), severity)
}

/// An error-severity `Diagnostic`, for the phases that only produce errors.
fn error(
    index: &LineIndex,
    source: &str,
    span: Span,
    code: &str,
    message: &str,
    help: Option<&str>,
) -> Diagnostic {
    diag(index, source, span, code, message, help, "error")
}

/// Build a `Diagnostic` from a span, appending the error's `help` to the message
/// (the Elm-quality bar the rest of the toolchain uses).
fn diag(
    index: &LineIndex,
    source: &str,
    span: Span,
    code: &str,
    message: &str,
    help: Option<&str>,
    severity: &'static str,
) -> Diagnostic {
    let message = match help {
        Some(h) => format!("{message}\n{h}"),
        None => message.to_string(),
    };
    let (start_line, start_col) = index.position(source, span.start as usize);
    let (end_line, end_col) = index.position(source, span.end as usize);
    Diagnostic {
        code: code.to_string(),
        message,
        severity,
        start_byte: span.start,
        end_byte: span.end,
        start_line,
        start_col,
        end_line,
        end_col,
    }
}

/// Byte-offset ↔ (line, UTF-16 character) mapping, mirroring the LSP's
/// `LineIndex` so the playground's coordinates match the editor's.
struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { line_starts }
    }

    /// Zero-based `(line, character)` for a byte `offset`; `character` is a
    /// UTF-16 code-unit count from the line start.
    fn position(&self, text: &str, offset: usize) -> (u32, u32) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l.saturating_sub(1),
        };
        let line_start = self.line_starts[line];
        let character = text
            .get(line_start..offset.min(text.len()))
            .map_or(0, |s| s.encode_utf16().count());
        (line as u32, character as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_program_emits_ts_and_no_diagnostics() {
        let json = compile("fn main() -> number {\n  return 1\n}\n");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["ts"].is_string(), "expected emitted TS");
        assert!(v["ts"].as_str().unwrap().contains("function main"));
        assert_eq!(v["diagnostics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn type_error_is_reported_with_code_and_position() {
        let json = compile("fn main() -> number {\n  return \"oops\"\n}\n");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let diags = v["diagnostics"].as_array().unwrap();
        assert!(!diags.is_empty());
        assert!(diags[0]["code"].as_str().unwrap().starts_with("E02"));
        assert_eq!(diags[0]["start_line"], 1);
    }

    #[test]
    fn parse_error_stops_with_no_ts() {
        let json = compile("fn (");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["ts"].is_null());
        assert!(!v["diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn advisory_lints_are_reported_as_warnings_like_glyph_check() {
        // `glyph check` on this module reports E0106 and E0107 as warnings.
        // The playground used to report neither, so the same file answered
        // differently depending on which surface compiled it.
        let json = compile(
            "module main\nimport std/io\n\npub fn main() -> void {\n  let unused = 1\n  return void\n}\n",
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let diags = v["diagnostics"].as_array().unwrap();
        let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
        assert!(codes.contains(&"E0106"), "unused import: {codes:?}");
        assert!(codes.contains(&"E0107"), "unused binding: {codes:?}");
        for d in diags {
            assert_eq!(
                d["severity"], "warning",
                "a lint is a warning, not an error: {d}"
            );
        }
        // A warning never blocks emission, here as in `glyph build`.
        assert!(v["ts"].is_string(), "warnings still emit TS");
    }

    #[test]
    fn a_sibling_import_is_named_as_assumed_external() {
        // The playground holds one module, so it cannot know whether `palette`
        // is a module of the writer's project or an npm package. It compiles it
        // as a package and says exactly which imports it made that assumption
        // about, because for a project module `glyph build` emits a different
        // specifier and a stronger runtime check.
        let json = compile(
            "module main\nimport palette { Colour }\n\npub type Row = {\n  kind: Colour,\n  label: string,\n}\n",
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["assumed_external_imports"],
            serde_json::json!(["palette"]),
            "the divergent import is named: {v}"
        );
    }

    #[test]
    fn a_std_only_module_assumes_nothing() {
        // Absence means there is nothing to warn about, not that the check was
        // skipped: `std/*` resolves the same on both surfaces, and so does
        // `extern/*`.
        let json = compile(
            "module main\nimport std/io\nimport extern/raw { helper }\n\npub fn main() -> void {\n  io.println(helper())\n  return void\n}\n",
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["assumed_external_imports"],
            serde_json::json!([]),
            "std and extern imports resolve identically on both surfaces: {v}"
        );
    }

    #[test]
    fn the_reachability_lint_is_absent_because_this_surface_cannot_answer_it() {
        // `glyph check` reports E0112 on this module: nothing is `pub`, there is
        // no `main`, and no module in the project imports it. That last clause is
        // a fact about files this surface does not hold, so it is not answered
        // here rather than guessed. The other advisory lints, which read only
        // this module, are reported.
        let json = compile(
            "module pricing\n\ntype Plan =\n  | Free\n  | Pro\n\nfn cost(p: Plan) -> number {\n  return match p {\n    Free => 0,\n    Pro => 12,\n  }\n}\n",
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["diagnostics"].as_array().unwrap().len(),
            0,
            "a project-wide lint is not guessed at from one module: {v}"
        );
    }

    #[test]
    fn version_is_reported() {
        assert!(!version().is_empty());
    }
}
