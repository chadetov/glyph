//! Structured diagnostics for `--json`.
//!
//! The text pipeline renders each diagnostic to an ariadne report string. For
//! agents (and any tool) consuming Glyph's output, `--json` emits the same
//! diagnostics as structured data instead: a stable code, severity, message,
//! file, and a 1-based line/column range, plus the help and note. The build's
//! own diagnostics and the remapped `tsc` errors flow through the same shape.
//! The shape round-trips (it deserializes as well as serializes) because
//! `glyph run` caches a build's diagnostics beside its output in this format.
//!
//! A resolve/typecheck/emit diagnostic that lands inside a top-level
//! declaration also carries `entity`, the same `module::name` identity
//! `glyph_variants` reports for a match site over that declaration (0.1.107).
//! An agent can then act on a batch of `--json` diagnostics by entity without
//! re-deriving "which function is this" from a line number, which shifts
//! under every unrelated edit above the site. A diagnostic from a stage that
//! has no parsed module to look the entity up in (a parse failure, or a `tsc`
//! error remapped from emitted TS) carries no entity rather than a guess.

use serde::{Deserialize, Serialize};

use glyph_ast::Span;
use glyph_emit::EmitError;
use glyph_parser::ParseError;
use glyph_resolver::ResolveError;
use glyph_typechecker::{Severity, TypeError};

/// One structured diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    /// `"error"` or `"warning"`.
    pub severity: String,
    pub message: String,
    pub file: String,
    pub range: Range,
    /// The compiler stage (`parse`/`resolve`/`typecheck`/`emit`/`tsc`).
    pub stage: String,
    /// The `module::name` identity of the top-level declaration this
    /// diagnostic's span sits inside, when one is known (see `entity_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

/// A source position: 1-based `line`/`col` plus the byte `offset`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        entity: Option<String>,
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
            entity,
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
    // No `entity`: a file that failed to parse has no declaration table to
    // look one up in.
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        "error",
        "parse",
        format!("{err}"),
        err.help().as_deref(),
        None,
        None,
    )
}

pub fn from_resolve_error(
    file: &str,
    source: &str,
    err: &ResolveError,
    stage: &str,
    module: &glyph_ast::Module,
) -> Diagnostic {
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
        entity_id(file, module, err.span().start),
    )
}

pub fn from_type_error(
    file: &str,
    source: &str,
    err: &TypeError,
    module: &glyph_ast::Module,
) -> Diagnostic {
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
        entity_id(file, module, err.span().start),
    )
}

/// The `module::name` identity of the top-level declaration enclosing byte
/// `offset`, in the same spelling `glyph_variants` reports for a match site
/// over the same declaration (see `glyph-lsp/src/mcp.rs`'s `render_key`).
/// `None` when no top-level declaration contains the offset (a diagnostic on
/// the `module` line itself, or on an import).
///
/// Without this, an agent acting on a `--json` diagnostic has to re-derive
/// "which function is this" from a line number, and a line number shifts
/// under every unrelated edit above the site — exactly the identity-loss
/// problem the `module::name` convention exists to prevent elsewhere.
pub fn entity_id(module_path: &str, module: &glyph_ast::Module, offset: u32) -> Option<String> {
    module.items.iter().find_map(|item| {
        let span = item.span();
        if span.start <= offset && offset < span.end {
            item.name().map(|name| format!("{module_path}::{name}"))
        } else {
            None
        }
    })
}

pub fn from_emit_error(
    file: &str,
    source: &str,
    err: &EmitError,
    module: &glyph_ast::Module,
) -> Diagnostic {
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
        entity_id(file, module, err.span().start),
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

    /// Regression guard for the has_catch_all bug shape (0.1.107): a `--json`
    /// diagnostic on a non-exhaustive match must be attributable to the exact
    /// function it sits in, not just "the file", so an agent can act on a
    /// batch of these without re-parsing every site to find out which
    /// function each line number currently belongs to. The identity must also
    /// agree with what `glyph_variants` reports for the same declaration
    /// (`module::name`), so the two answers can be cross-referenced.
    #[test]
    fn entity_id_names_the_enclosing_declaration() {
        let src = "module main\n\n\
            fn describe_exhaustive(s: string) -> string {\n    s\n}\n\n\
            fn describe_catchall(s: string) -> string {\n    s\n}\n";
        let module = glyph_parser::parse(src).expect("fixture parses");

        let exhaustive_offset = src.find("describe_exhaustive").unwrap() as u32;
        assert_eq!(
            entity_id("main", &module, exhaustive_offset),
            Some("main::describe_exhaustive".to_string())
        );

        // A second declaration in the same file must not be misattributed to
        // the first just because it comes later.
        let catchall_offset = src.find("describe_catchall").unwrap() as u32;
        assert_eq!(
            entity_id("main", &module, catchall_offset),
            Some("main::describe_catchall".to_string())
        );

        // An offset before any declaration (the `module` line) names nothing.
        assert_eq!(entity_id("main", &module, 0), None);
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
            None,
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"code\":\"E0200\""), "{json}");
        assert!(json.contains("\"severity\":\"error\""), "{json}");
        assert!(!json.contains("help"), "no help key when None: {json}");
    }
}
