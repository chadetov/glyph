//! ariadne-rendered diagnostic strings.
//!
//! The day-11 cut printed one-line strings like
//! `app.glyph: import: \`bogus\` is not exported by \`lib\``. Day 13
//! replaces those with multi-line ariadne reports that show the source
//! line, a caret pointer, and the error message in context — an
//! incremental step toward Phase 1 week 7's full "Elm quality" audit.
//!
//! Color output is enabled by default; tests pass `false` via the
//! `with_color` parameter to produce stable, ANSI-free snapshots.

use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};

use glyph_ast::Span;
use glyph_emit::EmitError;
use glyph_parser::ParseError;
use glyph_resolver::ResolveError;
use glyph_typechecker::{Severity, TypeError};

/// Render a `ParseError` as an ariadne report. `path` is the
/// source-id used by ariadne's cache; `source` is the file's text.
/// Returns the rendered diagnostic as a String (multi-line, with or
/// without ANSI color depending on `with_color`).
pub fn render_parse_error(
    path: &str,
    source: &str,
    err: &ParseError,
    with_color: bool,
) -> String {
    let span = err.span();
    let message = format!("{err}");
    build_report(
        path,
        source,
        span,
        "parse",
        &message,
        err.code(),
        err.help().as_deref(),
        None,
        with_color,
        Severity::Error,
    )
}

/// Render a `ResolveError` (covers DuplicateName, RelativeImport,
/// UnresolvedName, UnresolvedModule, UnknownExportedName) as an ariadne
/// report.
pub fn render_resolve_error(
    path: &str,
    source: &str,
    err: &ResolveError,
    with_color: bool,
) -> String {
    let span = err.span();
    let message = format!("{err}");
    let stage = stage_label_for(err);
    let severity = match err.severity() {
        glyph_resolver::Severity::Warning => Severity::Warning,
        glyph_resolver::Severity::Error => Severity::Error,
    };
    build_report(
        path,
        source,
        span,
        stage,
        &message,
        err.code(),
        err.help(),
        None,
        with_color,
        severity,
    )
}

/// Render a `TypeError` (day-14: non-exhaustive match) as an ariadne
/// report. Single-stage tag (`typecheck`) for now; future
/// bidirectional-checker errors can refine the tag if it helps the
/// reader.
pub fn render_type_error(
    path: &str,
    source: &str,
    err: &TypeError,
    with_color: bool,
) -> String {
    let span = err.span();
    let message = format!("{err}");
    build_report(
        path,
        source,
        span,
        "typecheck",
        &message,
        err.code(),
        err.help(),
        err.note(),
        with_color,
        err.severity(),
    )
}

/// Render an `EmitError` (a construct whose TS emission isn't implemented
/// yet) as an ariadne report under the `emit` stage tag.
pub fn render_emit_error(
    path: &str,
    source: &str,
    err: &EmitError,
    with_color: bool,
) -> String {
    let span = err.span();
    let message = format!("{err}");
    build_report(
        path,
        source,
        span,
        "emit",
        &message,
        err.code(),
        err.help(),
        err.note(),
        with_color,
        Severity::Error,
    )
}

/// Render an external (`tsc`) diagnostic remapped onto Glyph source: `span` is
/// the originating Glyph span, `code` the `tsc` code (`TS2339`), `message` the
/// text after it. Shown under a `tsc` stage tag so the reader knows it came from
/// the TypeScript back-end.
pub fn render_tsc_error(
    path: &str,
    source: &str,
    span: Span,
    code: &str,
    message: &str,
    note: Option<&str>,
    with_color: bool,
) -> String {
    build_report(
        path,
        source,
        span,
        "tsc",
        message,
        code,
        Some("This is a TypeScript back-end error mapped to your Glyph source. The generated `.ts` is in the build output if you need the exact position."),
        note,
        with_color,
        Severity::Error,
    )
}

/// Map each `ResolveError` variant to a stage tag that appears in the
/// label text. Stages let the reader distinguish "collect-time" issues
/// (duplicate names) from "resolve-time" (unresolved name) from
/// "import-time" (unknown export). Day 11 used these as inline prefixes
/// in the one-line diagnostics; here they live on the label.
pub fn stage_label_for(err: &ResolveError) -> &'static str {
    match err {
        ResolveError::DuplicateName { .. } => "collect",
        ResolveError::RelativeImport { .. } => "collect",
        ResolveError::BarrelFile { .. } => "collect",
        ResolveError::UnknownExportedName { .. } => "import",
        ResolveError::UnresolvedName { .. } => "resolve",
        ResolveError::UnresolvedModule { .. } => "import",
        ResolveError::UnusedImport { .. } => "lint",
        ResolveError::UnusedBinding { .. } => "lint",
        ResolveError::UnreachableCode { .. } => "lint",
        ResolveError::ReservedWordName { .. } => "collect",
        ResolveError::ShadowedGlobalName { .. } => "collect",
        ResolveError::PrimitiveUnionType { .. } => "collect",
        ResolveError::NoExportSurface { .. } => "lint",
    }
}

/// Build and render an ariadne report for one diagnostic, including its stable
/// code (`[E0042]` in the header), an actionable `help` line, and an optional
/// background `note`.
#[allow(clippy::too_many_arguments)]
fn build_report(
    path: &str,
    source: &str,
    span: Span,
    stage: &str,
    message: &str,
    code: &str,
    help: Option<&str>,
    note: Option<&str>,
    with_color: bool,
    severity: Severity,
) -> String {
    // A warning is surfaced but does not fail the build; render it yellow under
    // `ReportKind::Warning`, an error red under `ReportKind::Error`.
    let (report_kind, label_color) = match severity {
        Severity::Warning => (ReportKind::Warning, Color::Yellow),
        Severity::Error => (ReportKind::Error, Color::Red),
    };
    let path_owned = path.to_string();
    let raw_range = span.start as usize..span.end as usize;
    // Defensive clamp: a malformed span shouldn't crash ariadne. This
    // mirrors `canonical_bytes`'s tolerance in glyph-db — production
    // code prefers a usable-if-imperfect diagnostic over a panic.
    let range = clamp_range(raw_range, source);
    // The report's primary span is the *clamped* one. Passing the raw span
    // (whose start could be past-end) would render the location header as
    // `path:?:?` even though the clamp salvaged the label range.
    let mut label = Label::new((path_owned.clone(), range.clone()))
        .with_message(message.to_string());
    if with_color {
        // Per-label color overrides the Config setting, so we ONLY set
        // it when the caller wants color. Tests pass `with_color: false`
        // to get a stable byte-stable snapshot.
        label = label.with_color(label_color);
    }

    // **IndexType::Byte**: Glyph's `Span` carries byte offsets (the
    // lexer advances by byte). ariadne's default `IndexType::Char` would
    // mis-align the caret on any source containing multi-byte UTF-8 —
    // and silently drop the label entirely when the byte-offset exceeds
    // the file's char count. Switching to byte indexing makes the
    // caret correct for arbitrary Glyph source.
    let config = Config::default()
        .with_color(with_color)
        .with_index_type(IndexType::Byte);
    // ariadne 0.6 takes the primary span itself rather than an id plus an
    // offset, so the span is built the same way the label's is.
    let mut builder = Report::build(report_kind, (path_owned.clone(), range.clone()))
        .with_code(code)
        .with_message(format!("{stage}: {message}"))
        .with_label(label)
        .with_config(config);
    if let Some(help) = help {
        builder = builder.with_help(help);
    }
    // Link the diagnostic to its documentation. A Glyph `E`-code has a section in
    // the error-codes reference (anchored by the lowercased code) and a
    // `glyph --explain` entry; append that as a note so the fix is one click or
    // one command away. tsc-passthrough codes (`TS…`) have no Glyph doc section.
    let doc_note = code.starts_with('E').then(|| {
        format!(
            "docs: https://github.com/chadetov/glyph/blob/main/docs/error-codes.md#{} \
             (or run `glyph --explain {code}`)",
            code.to_lowercase(),
        )
    });
    match (note, doc_note.as_deref()) {
        (Some(n), Some(d)) => builder = builder.with_note(format!("{n}\n{d}")),
        (Some(n), None) => builder = builder.with_note(n),
        (None, Some(d)) => builder = builder.with_note(d),
        (None, None) => {}
    }
    let report = builder.finish();

    let mut buf: Vec<u8> = Vec::new();
    // The cache's source-id type must match the Report's S::SourceId,
    // which the builder infers from the `path_owned` argument (a String).
    // ariadne's Cache impl for `(Id, Source<I>)` requires `Id` to match
    // exactly, so we use String here too.
    let cache = (path_owned.clone(), Source::from(source));
    // `write` returns io::Result; the underlying writer is a Vec<u8>
    // which can't fail at the IO layer.
    let _ = report.write(cache, &mut buf);
    String::from_utf8(buf).unwrap_or_else(|_| {
        // ariadne sometimes emits non-UTF8 sequences when terminal
        // detection produces unexpected escapes. With `with_color(false)`
        // the output is plain UTF-8, so this branch should never fire
        // in tests; in production, fall back to the message-only string.
        format!("{stage}: {message}")
    })
}

fn clamp_range(range: std::ops::Range<usize>, source: &str) -> std::ops::Range<usize> {
    let len = source.len();
    let start = range.start.min(len);
    let end = range.end.min(len).max(start);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_parse_error_with_source_context() {
        let source = "module x\nfn main(\n";
        let err = glyph_parser::parse(source).expect_err("should fail to parse");
        let out = render_parse_error("test.glyph", source, &err, false);
        // The rendered output should reference the file and contain the
        // failing source line. Ariadne's exact glyphs vary by config so
        // we test structural properties, not byte equality.
        assert!(out.contains("test.glyph"), "missing path; got:\n{out}");
        assert!(
            out.contains("parse"),
            "missing stage tag; got:\n{out}"
        );
    }

    #[test]
    fn renders_a_resolve_error_with_source_context() {
        // Construct a ResolveError directly so the test doesn't depend
        // on whatever the resolver currently produces for a given input.
        let err = ResolveError::UnknownExportedName {
            name: "bogus".to_string(),
            module: "lib".to_string(),
            suggestion: String::new(),
            span: Span::new(9, 14),
        };
        let source = "module x\nimport lib { bogus }\n";
        let out = render_resolve_error("app.glyph", source, &err, false);
        assert!(out.contains("app.glyph"), "missing path; got:\n{out}");
        assert!(out.contains("bogus"), "missing offending name; got:\n{out}");
        assert!(
            out.contains("import"),
            "missing stage tag; got:\n{out}"
        );
    }

    #[test]
    fn renders_with_byte_indexing_for_multibyte_source() {
        // ariadne's default IndexType::Char would mis-align (or drop)
        // the label here: the offending span's bytes are AFTER a
        // multi-byte UTF-8 string. With IndexType::Byte the caret
        // points at the right place.
        let source = "module x\nconst MSG = \"café ☕\"\nimport lib { bogus }\n";
        let import_start = source.find("bogus").unwrap();
        let import_end = import_start + "bogus".len();
        let err = ResolveError::UnknownExportedName {
            name: "bogus".to_string(),
            module: "lib".to_string(),
            suggestion: String::new(),
            span: Span::new(import_start as u32, import_end as u32),
        };
        let out = render_resolve_error("app.glyph", source, &err, false);
        assert!(out.contains("bogus"), "missing offending name:\n{out}");
        // The source line that contains `bogus` must appear in the
        // rendered output. Without IndexType::Byte, ariadne's
        // get_offset_line returns None and the label (with its source
        // context) is silently dropped.
        assert!(
            out.contains("import lib { bogus }"),
            "missing source line — likely byte/char index mismatch:\n{out}"
        );
    }

    #[test]
    fn clamp_range_handles_out_of_bounds_spans() {
        let s = "abc";
        assert_eq!(clamp_range(0..3, s), 0..3);
        assert_eq!(clamp_range(0..100, s), 0..3);
        assert_eq!(clamp_range(100..200, s), 3..3);
        // Inverted span clamps to an empty range at start. The reversed range
        // is the input under test, so clippy's warning about it is the thing
        // this line exists to exercise.
        #[allow(clippy::reversed_empty_ranges)]
        let inverted = 2..1;
        assert_eq!(clamp_range(inverted, s), 2..2);
    }
}
