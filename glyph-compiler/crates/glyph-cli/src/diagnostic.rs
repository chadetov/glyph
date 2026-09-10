//! Structured diagnostics for `--json`.
//!
//! The text pipeline renders each diagnostic to an ariadne report string. For
//! agents (and any tool) consuming Glyph's output, `--json` emits the same
//! diagnostics as structured data instead: a stable code, severity, message,
//! file, and a 1-based line/column range, plus the help and note. The build's
//! own diagnostics and the remapped `tsc` errors flow through the same shape.
//!
//! The type itself lives in `glyph_lsp::diagnostic`, because the
//! `glyph_diagnostics` MCP tool answers with it too and the two surfaces must
//! not be able to disagree about the shape of one fact (G219). This module is
//! the CLI's name for it: `crate::diagnostic::Diagnostic` is
//! `glyph_lsp::diagnostic::Diagnostic`, and `glyph build` and `glyph check`
//! serialize what the MCP server serializes.

pub use glyph_lsp::diagnostic::{
    entity_id, from_emit_error, from_parse_error, from_resolve_error, from_type_error, pos_of,
    stage_label_for, Diagnostic, Pos, Range, UnionEntity,
};
