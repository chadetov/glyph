//! Remap `tsc` diagnostics back onto Glyph source.
//!
//! `glyph build --check` type-checks the emitted TypeScript with `tsc`, which
//! reports errors at positions in the generated `.ts`. That loses source
//! locality — the biggest gap against the "Elm-quality errors" bar. The emitter
//! produces a coarse source map (`(byte offset in .ts, Glyph span)` checkpoints
//! at each declaration and top-level statement); here we parse `tsc`'s output,
//! find the checkpoint at or before each error's position, and re-render the
//! message against the original `.glyph` file with an ariadne caret.
//!
//! Errors we can't attribute to a generated module (a stdlib `.ts`, an
//! unparseable line, a trailing "Found N errors." summary) pass through
//! verbatim, so nothing is ever dropped.

use glyph_ast::Span;

use crate::diagnostic::{Diagnostic, Pos, Range};
use crate::render::render_tsc_error;

/// The per-module data needed to remap a `tsc` error: the emitted file's path
/// (to match `tsc`'s output), the Glyph source (to render against), the emitted
/// TypeScript (to turn a line/col into a byte offset), and the source map.
#[derive(Debug)]
pub struct ModuleMap {
    /// The emitted `.ts` path relative to the out dir, e.g. `main.ts`.
    pub ts_rel: String,
    /// The Glyph source path shown in the rendered diagnostic.
    pub glyph_path: String,
    /// The Glyph source text.
    pub glyph_source: String,
    /// The emitted TypeScript text.
    pub ts_source: String,
    /// `(byte offset in `ts_source`, Glyph span)`, strictly increasing.
    pub source_map: Vec<(usize, Span)>,
}

impl ModuleMap {
    /// The Glyph span for a 1-based `(line, col)` in the emitted `.ts`: the last
    /// source-map checkpoint at or before that byte offset.
    fn span_for(&self, line: usize, col: usize) -> Option<Span> {
        let offset = line_col_to_byte(&self.ts_source, line, col);
        self.source_map
            .iter()
            .rev()
            .find(|(o, _)| *o <= offset)
            .map(|(_, span)| *span)
    }
}

/// Rewrite `tsc`'s output, mapping each error whose file is one of our generated
/// modules back onto its Glyph source. Lines that don't parse as a located error
/// or don't belong to a known module are kept as-is.
pub fn remap_tsc_output(raw: &str, maps: &[ModuleMap], with_color: bool) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        match parse_tsc_line(line) {
            Some(err) => match find_module(maps, err.path).and_then(|m| {
                m.span_for(err.line, err.col)
                    .map(|span| (m, span))
            }) {
                Some((m, span)) => {
                    out.push_str(&render_tsc_error(
                        &m.glyph_path,
                        &m.glyph_source,
                        span,
                        err.code,
                        err.message,
                        with_color,
                    ));
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                None => {
                    out.push_str(line);
                    out.push('\n');
                }
            },
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Like [`remap_tsc_output`], but produces structured diagnostics for `--json`.
/// A mappable error is rendered against its Glyph source (with a remapped span);
/// an unmappable one keeps its `.ts` location so nothing is dropped.
pub fn remap_tsc_to_diagnostics(raw: &str, maps: &[ModuleMap]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let Some(err) = parse_tsc_line(line) else {
            continue;
        };
        match find_module(maps, err.path).and_then(|m| m.span_for(err.line, err.col).map(|s| (m, s)))
        {
            Some((m, span)) => out.push(Diagnostic::new(
                &m.glyph_path,
                &m.glyph_source,
                span,
                err.code,
                "error",
                "tsc",
                err.message.to_string(),
                None,
                None,
            )),
            None => {
                let at = Pos {
                    line: err.line as u32,
                    col: err.col as u32,
                    offset: 0,
                };
                out.push(Diagnostic {
                    code: err.code.to_string(),
                    severity: "error".to_string(),
                    message: err.message.to_string(),
                    file: err.path.to_string(),
                    range: Range {
                        start: at.clone(),
                        end: at,
                    },
                    stage: "tsc".to_string(),
                    help: None,
                    note: None,
                });
            }
        }
    }
    out
}

struct TscError<'a> {
    path: &'a str,
    line: usize,
    col: usize,
    code: &'a str,
    message: &'a str,
}

/// Parse one `tsc` diagnostic line: `path(line,col): error TSxxxx: message`.
fn parse_tsc_line(line: &str) -> Option<TscError<'_>> {
    let err_at = line.find("): error TS")?;
    let before = &line[..err_at]; // `path(line,col`
    let open = before.rfind('(')?;
    let path = &before[..open];
    let (l, c) = before[open + 1..].split_once(',')?;
    let line_no: usize = l.trim().parse().ok()?;
    let col_no: usize = c.trim().parse().ok()?;

    // After `): error `: `TSxxxx: message`.
    let rest = &line[err_at + "): error ".len()..];
    let (code, message) = rest.split_once(':')?;
    Some(TscError {
        path,
        line: line_no,
        col: col_no,
        code: code.trim(),
        message: message.trim(),
    })
}

/// Find the module whose emitted `.ts` matches `tsc`'s reported path. `tsc`
/// prints an out-dir-relative or absolute path; we match on the trailing
/// segment being our `ts_rel` at a path boundary.
fn find_module<'a>(maps: &'a [ModuleMap], tsc_path: &str) -> Option<&'a ModuleMap> {
    let norm = tsc_path.replace('\\', "/");
    maps.iter().find(|m| {
        norm == m.ts_rel
            || norm.ends_with(&format!("/{}", m.ts_rel))
    })
}

/// Byte offset of a 1-based `(line, col)` in `src`. `col` counts characters
/// (as `tsc` reports); the returned offset is in bytes so it can be compared
/// against the byte-keyed source map.
fn line_col_to_byte(src: &str, line: usize, col: usize) -> usize {
    let mut offset = 0usize;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let col_off = l
                .char_indices()
                .nth(col.saturating_sub(1))
                .map(|(b, _)| b)
                .unwrap_or(l.len());
            return offset + col_off;
        }
        offset += l.len();
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    #[test]
    fn parses_a_tsc_error_line() {
        let e = parse_tsc_line("out/main.ts(59,26): error TS2339: Property 'value' does not exist.")
            .expect("parsed");
        assert_eq!(e.path, "out/main.ts");
        assert_eq!((e.line, e.col), (59, 26));
        assert_eq!(e.code, "TS2339");
        assert_eq!(e.message, "Property 'value' does not exist.");
    }

    #[test]
    fn remaps_onto_glyph_source() {
        // Two Glyph "declarations": the error's .ts offset falls in the second.
        let glyph = "module main\nfn a() -> number { return 1 }\nfn bad() -> string { return 2 }\n";
        let ts = "line0\nline1\nline2 with the error here\n";
        // Checkpoint at offset 0 -> span of `a`; at offset 12 -> span of `bad`.
        let a_span = span(12, 41);
        let bad_span = span(42, 73);
        let m = ModuleMap {
            ts_rel: "main.ts".to_string(),
            glyph_path: "main.glyph".to_string(),
            glyph_source: glyph.to_string(),
            ts_source: ts.to_string(),
            source_map: vec![(0, a_span), (12, bad_span)],
        };
        // The error is on ts line 3 (offset 12..), so it maps to `bad_span`.
        let raw = "out/main.ts(3,7): error TS2322: Type 'number' is not assignable to type 'string'.\n";
        let out = remap_tsc_output(raw, std::slice::from_ref(&m), false);
        assert!(out.contains("main.glyph"), "renders against glyph: {out}");
        assert!(out.contains("TS2322"), "keeps the tsc code: {out}");
        assert!(!out.contains("main.ts(3,7)"), "the raw .ts location is gone: {out}");
    }

    #[test]
    fn passes_through_unknown_files_and_summaries() {
        let m = ModuleMap {
            ts_rel: "main.ts".to_string(),
            glyph_path: "main.glyph".to_string(),
            glyph_source: "module main\n".to_string(),
            ts_source: "x\n".to_string(),
            source_map: vec![(0, span(0, 11))],
        };
        let raw = "std/http.ts(4,2): error TS1005: ';' expected.\nFound 1 error.\n";
        let out = remap_tsc_output(raw, std::slice::from_ref(&m), false);
        assert!(out.contains("std/http.ts(4,2)"), "stdlib error passes through: {out}");
        assert!(out.contains("Found 1 error."), "summary passes through: {out}");
    }
}
