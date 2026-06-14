//! Pure analysis used by the language server: run the compiler front end over a
//! single in-memory document and collect diagnostics, plus a byte-offset →
//! line/character index for mapping spans to LSP positions.
//!
//! This module holds no `tower-lsp` types, so it is unit-testable without an LSP
//! runtime. The server (`lib.rs`) converts `GlyphDiagnostic` to the protocol
//! type using `LineIndex`.

use glyph_ast::{Decl, Module, TypeExpr};
use glyph_resolver::{
    build_prelude, collect_module_symbols, resolve_module, verify_imports, ResolvedModule,
    ResolvedRef, StdlibStubs,
};
use glyph_typechecker::{assign_types, display_ty, TypeMap};

/// One diagnostic in source-byte coordinates, independent of the LSP protocol.
pub struct GlyphDiagnostic {
    /// Byte offsets into the source `[start, end)`.
    pub start: u32,
    pub end: u32,
    /// The human-readable message (the error's `Display`), with its `help`
    /// appended on a second line when present (the Elm-quality bar).
    pub message: String,
    /// The stable diagnostic code (e.g. `E0204`).
    pub code: String,
}

/// Run the compiler front end (parse → resolve → typecheck) over `text` and
/// collect every diagnostic. Import verification uses the stdlib stub graph, so
/// `std/*` import mistakes are caught; sibling/external imports are permissively
/// skipped (a single open file has no project graph). A parse failure short-
/// circuits — downstream phases cannot run without an AST.
pub fn analyze(text: &str) -> Vec<GlyphDiagnostic> {
    let module = match glyph_parser::parse(text) {
        Ok(m) => m,
        Err(e) => {
            return vec![GlyphDiagnostic {
                start: e.span().start,
                end: e.span().end,
                message: with_help(format!("{e}"), e.help()),
                code: e.code().to_string(),
            }]
        }
    };

    let mut out = Vec::new();

    let symbols = match collect_module_symbols(&module) {
        Ok(s) => s,
        Err(errors) => {
            // Symbol collection failed (e.g. a duplicate declaration or a D15
            // barrel file); report those and stop — later phases need the table.
            for e in errors {
                out.push(resolve_diag(&e));
            }
            return out;
        }
    };

    let stdlib = StdlibStubs::new();
    for e in verify_imports(&module, &stdlib) {
        out.push(resolve_diag(&e));
    }

    let prelude = build_prelude();
    let (resolved, resolve_errors) = resolve_module(&module, symbols, &prelude);
    for e in &resolve_errors {
        out.push(resolve_diag(e));
    }

    let (_types, type_errors) = assign_types(&module, &resolved, &prelude);
    for e in &type_errors {
        out.push(GlyphDiagnostic {
            start: e.span().start,
            end: e.span().end,
            message: with_help(format!("{e}"), e.help()),
            code: e.code().to_string(),
        });
    }

    out
}

/// A fully analyzed document: the resolution and type side tables. Hover and
/// go-to-definition query these by source offset. `None` from `analyze_full`
/// means the document did not parse. (Neither table borrows the AST — spans are
/// plain byte offsets — so the `Module` is dropped after analysis.)
pub struct Analysis {
    module: Module,
    resolved: ResolvedModule,
    types: TypeMap,
}

/// Parse, resolve, and typecheck `text`, returning the analysis for
/// position-based queries. `None` if the document does not parse.
pub fn analyze_full(text: &str) -> Option<Analysis> {
    let module = glyph_parser::parse(text).ok()?;
    let symbols = collect_module_symbols(&module).ok()?;
    let prelude = build_prelude();
    let (resolved, _errs) = resolve_module(&module, symbols, &prelude);
    let (types, _terrs) = assign_types(&module, &resolved, &prelude);
    Some(Analysis {
        module,
        resolved,
        types,
    })
}

/// What kind of thing a completion item names — maps to an editor icon.
pub enum CompletionTag {
    Keyword,
    Function,
    Type,
    Variant,
    Value,
}

pub struct Completion {
    pub label: String,
    pub tag: CompletionTag,
}

/// Glyph keywords offered in completion.
const KEYWORDS: &[&str] = &[
    "module", "import", "fn", "type", "component", "const", "let", "mut", "match", "return",
    "loop", "for", "in", "break", "continue", "async", "await", "owned", "resource", "is",
    "else", "true", "false", "void",
];

impl Analysis {
    /// The rendered type of the innermost typed expression covering `offset`,
    /// for hover. `None` when no typed expression is there or its type is the
    /// not-yet-inferred placeholder.
    pub fn hover(&self, offset: usize) -> Option<String> {
        let mut best: Option<(u32, String)> = None;
        for (span, ty) in self.types.iter() {
            if (span.start as usize) <= offset && offset < (span.end as usize) {
                let width = span.end - span.start;
                if best.as_ref().map_or(true, |(w, _)| width < *w) {
                    best = Some((width, display_ty(ty)));
                }
            }
        }
        best.map(|(_, rendered)| rendered).filter(|s| s != "?")
    }

    /// The definition location (byte `[start, end)`) of the name reference
    /// covering `offset`, for go-to-definition. A local binding or module-level
    /// declaration resolves within this file; a prelude built-in (or no
    /// reference) yields `None`. Cross-module targets await workspace support.
    pub fn definition(&self, offset: usize) -> Option<(u32, u32)> {
        let mut best: Option<(u32, ResolvedRef)> = None;
        for (span, r) in self.resolved.resolutions.iter() {
            if (span.start as usize) <= offset && offset < (span.end as usize) {
                let width = span.end - span.start;
                if best.as_ref().map_or(true, |(w, _)| width < *w) {
                    best = Some((width, r));
                }
            }
        }
        match best.map(|(_, r)| r)? {
            ResolvedRef::Local(def_start) => Some((def_start, def_start)),
            ResolvedRef::Module(id) => {
                let sym = self.resolved.symbols.table.get(id)?;
                Some((sym.span.start, sym.span.start))
            }
            ResolvedRef::Prelude(_) => None,
        }
    }

    /// Completion candidates: Glyph keywords, this module's top-level
    /// declarations (and a union's variant constructors), and the prelude names.
    /// A flat list the editor filters by the typed prefix; member completion
    /// (after `.`) is a later increment.
    pub fn completions(&self) -> Vec<Completion> {
        let mut out = base_completions();

        for decl in &self.module.items {
            match decl {
                Decl::Fn(f) => out.push(Completion {
                    label: f.name.to_string(),
                    tag: CompletionTag::Function,
                }),
                Decl::Component(c) => out.push(Completion {
                    label: c.name.to_string(),
                    tag: CompletionTag::Function,
                }),
                Decl::Const(c) => out.push(Completion {
                    label: c.name.to_string(),
                    tag: CompletionTag::Value,
                }),
                Decl::Type(t) => {
                    out.push(Completion {
                        label: t.name.to_string(),
                        tag: CompletionTag::Type,
                    });
                    // A tagged union's variants are constructors in value scope.
                    if let TypeExpr::Union { variants, .. } = &t.body {
                        for v in variants {
                            out.push(Completion {
                                label: v.name.to_string(),
                                tag: CompletionTag::Variant,
                            });
                        }
                    }
                }
                Decl::Import(_) => {}
            }
        }

        out
    }
}

/// Keyword and prelude completions, independent of any document. The server
/// falls back to these when the open file does not parse — exactly when
/// completion is most useful (mid-edit).
pub fn base_completions() -> Vec<Completion> {
    let mut out: Vec<Completion> = KEYWORDS
        .iter()
        .map(|k| Completion {
            label: (*k).to_string(),
            tag: CompletionTag::Keyword,
        })
        .collect();

    // Prelude names (`Result`, `Ok`, `Option`, `string`, `print`, …). Tag by a
    // light case heuristic: the prelude tagged-union constructors are variants,
    // other uppercase-initial names are types, the rest values.
    for name in build_prelude().by_name.keys() {
        let s = name.as_ref();
        let tag = if matches!(s, "Ok" | "Err" | "Some" | "None") {
            CompletionTag::Variant
        } else if s.chars().next().is_some_and(|c| c.is_uppercase()) {
            CompletionTag::Type
        } else {
            CompletionTag::Value
        };
        out.push(Completion {
            label: s.to_string(),
            tag,
        });
    }
    out
}

fn resolve_diag(e: &glyph_resolver::ResolveError) -> GlyphDiagnostic {
    GlyphDiagnostic {
        start: e.span().start,
        end: e.span().end,
        message: with_help(format!("{e}"), e.help()),
        code: e.code().to_string(),
    }
}

fn with_help(message: String, help: Option<&'static str>) -> String {
    match help {
        Some(h) => format!("{message}\n{h}"),
        None => message,
    }
}

/// Maps byte offsets to LSP line/character positions. `character` is counted in
/// UTF-16 code units, as the LSP spec requires by default.
pub struct LineIndex {
    /// Byte offset of the start of each line (line 0 starts at 0).
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { line_starts }
    }

    /// `(line, character)` for a byte `offset` into `text`, both zero-based;
    /// `character` is a UTF-16 code-unit count from the line start.
    pub fn position(&self, text: &str, offset: usize) -> (u32, u32) {
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

    /// The byte offset of LSP position `(line, character)` in `text`, where
    /// `character` is a UTF-16 code-unit count. The inverse of `position`, used
    /// to map a hover/definition request to a source offset.
    pub fn offset(&self, text: &str, line: u32, character: u32) -> usize {
        let line = line as usize;
        let Some(&line_start) = self.line_starts.get(line) else {
            return text.len();
        };
        let mut utf16 = 0u32;
        let mut byte = line_start;
        for ch in text[line_start..].chars() {
            if utf16 >= character || ch == '\n' {
                break;
            }
            utf16 += ch.len_utf16() as u32;
            byte += ch.len_utf8();
        }
        byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_program_has_no_diagnostics() {
        let diags = analyze("module x\nfn f() -> number {\n  return 1\n}\n");
        assert!(diags.is_empty(), "{:?}", diags.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn parse_error_is_reported_with_code() {
        // `let mut` is not valid Glyph; the parser reports it.
        let diags = analyze("module x\nfn f() -> number {\n  let mut x = 1\n  return x\n}\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E0002");
    }

    #[test]
    fn type_error_is_reported() {
        // A field typo is caught by the typechecker (E0210).
        let diags = analyze("module x\ntype U = { name: string }\nfn f(u: U) -> string {\n  return u.naem\n}\n");
        assert!(diags.iter().any(|d| d.code == "E0210"), "{:?}", diags.iter().map(|d| &d.code).collect::<Vec<_>>());
    }

    #[test]
    fn line_index_maps_offsets() {
        let text = "ab\ncde\nf";
        let idx = LineIndex::new(text);
        assert_eq!(idx.position(text, 0), (0, 0));
        assert_eq!(idx.position(text, 1), (0, 1));
        assert_eq!(idx.position(text, 3), (1, 0)); // start of line 1 ("cde")
        assert_eq!(idx.position(text, 5), (1, 2));
        assert_eq!(idx.position(text, 7), (2, 0)); // "f"
    }

    #[test]
    fn hover_shows_expression_type() {
        let text = "module x\nfn f() -> number {\n  let n = 41\n  return n\n}\n";
        let a = analyze_full(text).expect("parses");
        let off = text.find("41").unwrap();
        assert_eq!(a.hover(off), Some("number".to_string()));
    }

    #[test]
    fn goto_definition_resolves_a_module_call() {
        let text = "module x\nfn helper() -> number {\n  return 1\n}\nfn main() -> number {\n  return helper()\n}\n";
        let a = analyze_full(text).expect("parses");
        let call = text.rfind("helper").unwrap(); // the call site
        let def = a.definition(call).expect("resolves");
        // The definition points at the `helper` declaration, before the call.
        assert!(def.0 < call as u32, "def {def:?} should precede call at {call}");
    }

    #[test]
    fn completions_include_keywords_decls_and_prelude() {
        let a = analyze_full(
            "module x\ntype Color = Red | Blue\nfn paint() -> number {\n  return 1\n}\n",
        )
        .expect("parses");
        let labels: Vec<String> = a.completions().into_iter().map(|c| c.label).collect();
        for want in ["fn", "paint", "Color", "Red", "Result", "Ok"] {
            assert!(labels.iter().any(|l| l == want), "missing {want} in {labels:?}");
        }
    }

    #[test]
    fn base_completions_have_keywords_and_prelude() {
        let labels: Vec<String> = base_completions().into_iter().map(|c| c.label).collect();
        assert!(labels.iter().any(|l| l == "match"));
        assert!(labels.iter().any(|l| l == "Option"));
    }

    #[test]
    fn offset_is_inverse_of_position() {
        let text = "ab\ncde\nf";
        let idx = LineIndex::new(text);
        assert_eq!(idx.offset(text, 0, 0), 0);
        assert_eq!(idx.offset(text, 1, 2), 5); // 'e' in "cde"
        // round-trip
        let (l, c) = idx.position(text, 5);
        assert_eq!(idx.offset(text, l, c), 5);
    }

    #[test]
    fn line_index_counts_utf16() {
        // A non-BMP char (😀) is two UTF-16 code units.
        let text = "a😀b";
        let idx = LineIndex::new(text);
        // byte offset of 'b' is 1 (a) + 4 (😀) = 5; expect character 3 (1 + 2).
        assert_eq!(idx.position(text, 5), (0, 3));
    }
}
