//! Canonical view (Q32, tractable core).
//!
//! `canonical_view` renders a Glyph file in the stable, agent-facing form an AI
//! agent reads and references: the `glyph fmt` canonical layout, every content
//! line tagged with an explicit `Lddd` number, and a per-declaration content
//! fingerprint.
//!
//! Two properties make it useful to an agent:
//!
//! - **Line numbers are decoupled from physical position.** Metadata lines
//!   (the `#`-prefixed fingerprints and header) sit *between* numbered lines, so
//!   an `Lddd` number names a canonical-text line regardless of how many marker
//!   lines precede it. A later text<->canonical position mapper (v1.1) has a
//!   stable coordinate to map onto.
//! - **Fingerprints are invariant under reformatting.** The fingerprint is
//!   FNV-1a/64 over a declaration's *canonical* bytes, so reindenting,
//!   reflowing, or whitespace edits leave it unchanged; it moves only when the
//!   declaration's content does. It is a content identifier, not a cryptographic
//!   hash, and is not collision-proof against an adversary.
//!
//! The function is pure (`&str -> Result<String, _>`): the CLI (`glyph
//! canonical`) and the LSP custom request both call it and are tested against
//! it without a runtime.

use crate::format_module;
use glyph_ast::{Decl, ModulePath};

/// Why a source file has no canonical view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// The source did not parse; the canonical view is only defined for source
    /// the formatter can lay out. Carries the rendered parse error.
    Parse(String),
}

impl std::fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalError::Parse(reason) => write!(f, "parse error: {reason}"),
        }
    }
}

impl std::error::Error for CanonicalError {}

/// Render `source` as its canonical agent view (see module docs).
pub fn canonical_view(source: &str) -> Result<String, CanonicalError> {
    let module =
        glyph_parser::parse(source).map_err(|e| CanonicalError::Parse(format!("{e:?}")))?;
    let comments = glyph_lexer::comments(source);
    let canonical = format_module(&module, &comments, source);

    // Re-parse the canonical text so declaration spans are in *canonical*
    // coordinates (the formatter's output always parses; should that ever fail,
    // degrade to a numbered view with no per-declaration fingerprints rather
    // than erroring).
    let reparsed = glyph_parser::parse(&canonical).ok();
    let module_path = reparsed
        .as_ref()
        .and_then(|m| m.module_path.as_ref())
        .map(path_string);

    let markers = reparsed
        .as_ref()
        .map(|m| collect_markers(&canonical, &m.items))
        .unwrap_or_default();

    Ok(render(&canonical, &markers, module_path.as_deref()))
}

/// A `#`-prefixed metadata line to emit immediately before a given canonical
/// line index.
struct Marker {
    /// 0-based canonical line index this marker precedes.
    line: usize,
    text: String,
}

fn collect_markers(canonical: &str, items: &[Decl]) -> Vec<Marker> {
    let line_starts = line_start_offsets(canonical);
    let mut markers = Vec::with_capacity(items.len());
    for decl in items {
        let (start, end, kind, name) = decl_extent(decl);
        let slice = &canonical[start.min(canonical.len())..end.min(canonical.len())];
        let fp = fnv1a_64(slice.as_bytes());
        markers.push(Marker {
            line: line_of(&line_starts, start),
            text: format!("# @hash:fnv1a:{fp:016x}  {kind} {name}"),
        });
    }
    markers
}

/// Byte extent (`start..end`), kind keyword, and name of a declaration in
/// canonical coordinates. The start is pulled back to the first annotation so
/// the fingerprint covers a decorated declaration's annotations (which precede
/// the keyword the decl span begins at).
fn decl_extent(d: &Decl) -> (usize, usize, &'static str, String) {
    match d {
        Decl::Import(i) => (
            i.span.start as usize,
            i.span.end as usize,
            "import",
            path_string(&i.path),
        ),
        Decl::Fn(f) => (
            ann_start(f.annotations.first().map(|a| a.span.start), f.span.start),
            f.span.end as usize,
            if f.is_async { "async fn" } else { "fn" },
            f.name.to_string(),
        ),
        Decl::Type(t) => (
            ann_start(t.annotations.first().map(|a| a.span.start), t.span.start),
            t.span.end as usize,
            if t.is_resource { "resource type" } else { "type" },
            t.name.to_string(),
        ),
        Decl::Const(c) => (
            ann_start(c.annotations.first().map(|a| a.span.start), c.span.start),
            c.span.end as usize,
            "const",
            c.name.to_string(),
        ),
        Decl::Component(c) => (
            ann_start(c.annotations.first().map(|a| a.span.start), c.span.start),
            c.span.end as usize,
            "component",
            c.name.to_string(),
        ),
    }
}

fn ann_start(first_annotation: Option<u32>, decl_start: u32) -> usize {
    first_annotation.unwrap_or(decl_start).min(decl_start) as usize
}

fn path_string(p: &ModulePath) -> String {
    p.segments
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("/")
}

/// Assemble the numbered view, interleaving fingerprint markers above the lines
/// they annotate. Line numbers are padded to the width of the largest number
/// (minimum 3: `L001`).
fn render(canonical: &str, markers: &[Marker], module_path: Option<&str>) -> String {
    let lines: Vec<&str> = split_lines(canonical);
    let width = line_number_width(lines.len());
    let file_fp = fnv1a_64(canonical.as_bytes());

    let mut out = String::new();
    out.push_str("# glyph canonical view\n");
    out.push_str(&format!(
        "# @hash:fnv1a:{file_fp:016x}  module {}\n",
        module_path.unwrap_or("(none)")
    ));

    for (i, line) in lines.iter().enumerate() {
        for m in markers.iter().filter(|m| m.line == i) {
            out.push_str(&m.text);
            out.push('\n');
        }
        out.push_str(&format!("L{:0width$}  {}\n", i + 1, line, width = width));
    }
    out
}

/// Split into lines without a trailing empty element. The formatter always ends
/// in exactly one `\n`; dropping the final empty split keeps the line count
/// equal to the number of textual lines.
fn split_lines(text: &str) -> Vec<&str> {
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('\n').collect()
    }
}

fn line_number_width(count: usize) -> usize {
    let digits = count.max(1).to_string().len();
    digits.max(3)
}

/// Byte offset at which each line begins (line 0 starts at 0).
fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (idx, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

/// 0-based line index containing byte `offset`.
fn line_of(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

/// FNV-1a/64. A fixed, version-independent content hash (unlike
/// `std::hash::DefaultHasher`, whose algorithm is explicitly unstable across
/// releases) so a fingerprint an agent records stays valid forever.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(src: &str) -> String {
        canonical_view(src).expect("parses")
    }

    #[test]
    fn numbers_every_canonical_line() {
        let v = view("fn main() -> number {\n  return 1\n}\n");
        // Header (2 lines) + one fingerprint marker + 3 numbered lines.
        assert!(v.contains("# glyph canonical view"));
        assert!(v.contains("L001  fn main() -> number {"));
        assert!(v.contains("L002    return 1"));
        assert!(v.contains("L003  }"));
    }

    #[test]
    fn fingerprints_each_declaration() {
        let v = view("fn a() -> number {\n  return 1\n}\nfn b() -> number {\n  return 2\n}\n");
        let hashes: Vec<&str> = v
            .lines()
            .filter(|l| l.starts_with("# @hash") && l.contains("fn "))
            .collect();
        assert_eq!(hashes.len(), 2, "one fingerprint per fn");
        assert!(hashes[0].contains("fn a"));
        assert!(hashes[1].contains("fn b"));
    }

    #[test]
    fn fingerprint_is_stable_under_reformatting() {
        let tidy = view("fn a(x: number) -> number {\n  return x\n}\n");
        let messy = view("fn   a(  x:number )->number{\n      return    x\n}\n");
        let fp = |s: &str| {
            s.lines()
                .find(|l| l.contains("fn a"))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_string()
        };
        assert_eq!(fp(&tidy), fp(&messy));
    }

    #[test]
    fn fingerprint_changes_with_content() {
        let one = view("fn a() -> number {\n  return 1\n}\n");
        let two = view("fn a() -> number {\n  return 2\n}\n");
        let fp = |s: &str| {
            s.lines()
                .find(|l| l.contains("fn a"))
                .unwrap()
                .to_string()
        };
        assert_ne!(fp(&one), fp(&two));
    }

    #[test]
    fn carries_module_path_in_header() {
        let v = view("module app/main\n\nfn a() -> number {\n  return 1\n}\n");
        assert!(v.contains("module app/main"));
    }

    #[test]
    fn unparseable_source_is_an_error() {
        assert!(matches!(
            canonical_view("fn ("),
            Err(CanonicalError::Parse(_))
        ));
    }

    #[test]
    fn fingerprint_covers_annotations() {
        // The annotation precedes the `fn` keyword the decl span starts at;
        // changing only the annotation must still move the fingerprint.
        let with = view("@example a() == 1\nfn a() -> number {\n  return 1\n}\n");
        let without = view("@example a() == 2\nfn a() -> number {\n  return 1\n}\n");
        let fp = |s: &str| {
            s.lines()
                .find(|l| l.contains("# @hash") && l.contains("fn a"))
                .unwrap()
                .to_string()
        };
        assert_ne!(fp(&with), fp(&without));
    }
}
