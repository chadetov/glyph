//! WebAssembly bindings for the Glyph compiler.
//!
//! Exposes one function, [`compile`], that turns a Glyph source string into the
//! emitted TypeScript plus diagnostics, entirely in memory — the exact front
//! end the LSP and `glyph build` run (parse → resolve → typecheck → emit), but
//! with no filesystem, so it runs in a browser. This is the engine behind the
//! web playground.
//!
//! The dependency set is deliberately the WASM-safe core only (lexer, ast,
//! parser, resolver, typechecker, emit). It does NOT pull in `glyph-db` (salsa),
//! `glyph-cli` (filesystem), or `glyph-lsp` (tokio) — none of which target
//! `wasm32-unknown-unknown`. The front-end pipeline is the same one the LSP's
//! `analysis::analyze` runs, replicated here against those crates directly.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use glyph_emit::{emit_module, EmitContext};
use glyph_lexer::Span;
use glyph_resolver::{
    build_prelude, collect_module_symbols, resolve_module, verify_imports, StdlibStubs,
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
                diagnostics: vec![diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help())],
            };
        }
    };

    let mut diagnostics = Vec::new();

    let symbols = match collect_module_symbols(&module) {
        Ok(s) => s,
        Err(errors) => {
            // Symbol collection failed (duplicate decl, D15 barrel file, …);
            // later phases need the table, so report and stop.
            for e in &errors {
                diagnostics.push(diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help()));
            }
            return CompileOutput { ts: None, diagnostics };
        }
    };

    // Import verification against the stdlib stub graph (a single open file has
    // no project graph, so sibling/external imports are permissively skipped).
    let stdlib = StdlibStubs::new();
    for e in verify_imports(&module, &stdlib) {
        diagnostics.push(diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help()));
    }

    let prelude = build_prelude();
    let (resolved, resolve_errors) = resolve_module(&module, symbols, &prelude);
    for e in &resolve_errors {
        diagnostics.push(diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help()));
    }

    let (types, type_errors) = assign_types(&module, &resolved, &prelude);
    for e in &type_errors {
        diagnostics.push(diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help()));
    }

    // Emit best-effort: the playground shows the TypeScript even when later-phase
    // diagnostics exist (it is what the writer is iterating toward). A genuine
    // emit error is reported and yields no TS.
    let ts = match emit_module(&module, &resolved, &types, &prelude, EmitContext::single()) {
        Ok(ts) => Some(ts),
        Err(e) => {
            diagnostics.push(diag(&index, source, e.span(), e.code(), &format!("{e}"), e.help()));
            None
        }
    };

    CompileOutput { ts, diagnostics }
}

/// Build a `Diagnostic` from a span, appending the error's `help` to the message
/// (the Elm-quality bar the rest of the toolchain uses).
fn diag(
    index: &LineIndex,
    source: &str,
    span: Span,
    code: &str,
    message: &str,
    help: Option<&'static str>,
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
        severity: "error",
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
    fn version_is_reported() {
        assert!(!version().is_empty());
    }
}
