//! Structured diagnostics for `--json`.
//!
//! The text pipeline renders each diagnostic to an ariadne report string. For
//! agents (and any tool) consuming Glyph's output, `--json` emits the same
//! diagnostics as structured data instead: a stable code, severity, message,
//! file, and a 1-based line/column range, plus the help and note. The build's
//! own diagnostics and the remapped `tsc` errors flow through the same shape.

use serde::Serialize;

use glyph_ast::Span;
use glyph_emit::EmitError;
use glyph_parser::ParseError;
use glyph_resolver::ResolveError;
use glyph_typechecker::{Severity, TypeError};

/// One structured diagnostic.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: String,
    /// `"error"` or `"warning"`.
    pub severity: String,
    pub message: String,
    pub file: String,
    pub range: Range,
    /// The compiler stage (`parse`/`resolve`/`typecheck`/`emit`/`tsc`).
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

/// A source position: 1-based `line`/`col` plus the byte `offset`.
#[derive(Debug, Clone, Serialize)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
    pub offset: u32,
}

impl Diagnostic {
    /// Build a diagnostic, computing line/col for the span's endpoints from
    /// `source`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        file: &str,
        source: &str,
        span: Span,
        code: &str,
        severity: &str,
        stage: &str,
        message: String,
        help: Option<&str>,
        note: Option<&str>,
    ) -> Self {
        Diagnostic {
            code: code.to_string(),
            severity: severity.to_string(),
            message,
            file: file.to_string(),
            range: Range {
                start: pos_of(source, span.start),
                end: pos_of(source, span.end),
            },
            stage: stage.to_string(),
            help: help.map(str::to_string),
            note: note.map(str::to_string),
        }
    }
}

/// 1-based line/column (and the byte offset) of `offset` within `source`.
pub fn pos_of(source: &str, offset: u32) -> Pos {
    let clamped = (offset as usize).min(source.len());
    let mut line = 1u32;
    let mut last_line_start = 0usize;
    for (i, b) in source.as_bytes().iter().enumerate() {
        if i >= clamped {
            break;
        }
        if *b == b'\n' {
            line += 1;
            last_line_start = i + 1;
        }
    }
    // Column counts characters, not bytes, from the line start.
    let col = source[last_line_start..clamped].chars().count() as u32 + 1;
    Pos { line, col, offset }
}

pub fn from_parse_error(file: &str, source: &str, err: &ParseError) -> Diagnostic {
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        "error",
        "parse",
        format!("{err}"),
        err.help(),
        None,
    )
}

pub fn from_resolve_error(file: &str, source: &str, err: &ResolveError, stage: &str) -> Diagnostic {
    let severity = match err.severity() {
        glyph_resolver::Severity::Warning => "warning",
        glyph_resolver::Severity::Error => "error",
    };
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        severity,
        stage,
        format!("{err}"),
        err.help(),
        None,
    )
}

pub fn from_type_error(file: &str, source: &str, err: &TypeError) -> Diagnostic {
    let severity = match err.severity() {
        Severity::Warning => "warning",
        Severity::Error => "error",
    };
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        severity,
        "typecheck",
        format!("{err}"),
        err.help(),
        err.note(),
    )
}

pub fn from_emit_error(file: &str, source: &str, err: &EmitError) -> Diagnostic {
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        "error",
        "emit",
        format!("{err}"),
        err.help(),
        err.note(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_of_computes_line_and_col() {
        let src = "abc\ndef\nghij";
        // offset 0 -> 1:1
        let p = pos_of(src, 0);
        assert_eq!((p.line, p.col), (1, 1));
        // offset of 'e' (index 5) -> line 2, col 2
        let p = pos_of(src, 5);
        assert_eq!((p.line, p.col), (2, 2));
        // offset of 'g' (index 8) -> line 3, col 1
        let p = pos_of(src, 8);
        assert_eq!((p.line, p.col), (3, 1));
    }

    #[test]
    fn serializes_without_empty_optionals() {
        let d = Diagnostic::new(
            "main.glyph",
            "module main\n",
            Span::new(0, 6),
            "E0200",
            "error",
            "typecheck",
            "boom".to_string(),
            None,
            None,
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"code\":\"E0200\""), "{json}");
        assert!(json.contains("\"severity\":\"error\""), "{json}");
        assert!(!json.contains("help"), "no help key when None: {json}");
    }
}
