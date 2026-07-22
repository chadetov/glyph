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
    ResolvedRef, StdlibStubs, SymbolKind,
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

/// The kind of a document-outline / workspace symbol — maps to an editor icon.
#[derive(Clone, Copy)]
pub enum OutlineKind {
    Function,
    Type,
    Constant,
    Variant,
}

/// One node in the document outline. `span` is the declaration's byte range
/// (used for both the symbol range and its selection range).
pub struct OutlineSymbol {
    pub name: String,
    pub kind: OutlineKind,
    pub span: (u32, u32),
    pub children: Vec<OutlineSymbol>,
}

/// Where a go-to-definition target lives.
pub enum Definition {
    /// In the current file, at byte range `[start, end)`.
    Here(u32, u32),
    /// In another module — the server resolves `module_path` to a file and
    /// finds the declaration named `name`.
    InModule { module_path: String, name: String },
}

/// Find a top-level declaration (or union variant) named `name` in an outline,
/// returning its byte span. Used to locate a cross-module definition target.
pub fn find_symbol_span(outline: &[OutlineSymbol], name: &str) -> Option<(u32, u32)> {
    for sym in outline {
        if sym.name == name {
            return Some(sym.span);
        }
        if let Some(span) = find_symbol_span(&sym.children, name) {
            return Some(span);
        }
    }
    None
}

/// Why a rename request was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum RenameError {
    /// The new name is not a legal Glyph identifier.
    InvalidIdentifier,
    /// The new name is a reserved keyword.
    ReservedKeyword,
    /// The cursor is not on a renameable binding.
    NoBinding,
    /// The cursor is on a module-level declaration; a safe rename needs the
    /// cross-file workspace index (not yet built), since the declaration may be
    /// referenced from other modules.
    ModuleLevelUnsupported,
}

/// A top-level declaration's name and whole-declaration span `(start, end)`, or
/// `None` for an `import` (nothing to rename or reference by name).
fn top_decl_name_and_span(decl: &Decl) -> Option<(&str, (u32, u32))> {
    match decl {
        Decl::Fn(f) => Some((f.name.as_ref(), (f.span.start, f.span.end))),
        Decl::Component(c) => Some((c.name.as_ref(), (c.span.start, c.span.end))),
        Decl::Const(c) => Some((c.name.as_ref(), (c.span.start, c.span.end))),
        Decl::Type(t) => Some((t.name.as_ref(), (t.span.start, t.span.end))),
        Decl::Import(_) => None,
    }
}

/// The span of the first whole-word occurrence of `name` in `text[start..end]`,
/// as absolute byte offsets. "Whole word" means neither neighbour is an
/// identifier character, so searching a `fn foo` declaration for `foo` skips a
/// coincidental substring in the keyword or a longer identifier.
fn whole_word_span(text: &str, start: u32, end: u32, name: &str) -> Option<(u32, u32)> {
    let s = start as usize;
    let e = (end as usize).min(text.len());
    let hay = text.get(s..e)?;
    let bytes = hay.as_bytes();
    let mut from = 0;
    while let Some(rel) = hay.get(from..).and_then(|h| h.find(name)) {
        let at = from + rel;
        let after = at + name.len();
        let before_ok = at == 0 || !is_ident_char(bytes[at - 1] as char);
        let after_ok = after >= hay.len() || !is_ident_char(bytes[after] as char);
        if before_ok && after_ok {
            return Some(((s + at) as u32, (s + after) as u32));
        }
        from = at + 1;
    }
    None
}

/// The name span of a local binding whose def-site starts at `def_start`. The
/// resolver records a local's def-site as the binding statement start (the
/// `let`/`for` keyword, or a parameter), so the name is the first whole word
/// at/after it — the binding always names itself before any use.
fn local_name_span(text: &str, def_start: u32, name: &str) -> Option<(u32, u32)> {
    whole_word_span(text, def_start, text.len() as u32, name)
}

/// True when `c` may appear inside a Glyph identifier.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True when `s` is a syntactically legal Glyph identifier (leading letter or
/// `_`, then letters/digits/`_`). Keyword-ness is checked separately.
fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(is_ident_char)
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

    /// Where the name reference covering `offset` is defined, for
    /// go-to-definition: within this file (`Here`), or in another module (an
    /// imported name — `InModule`, which the server resolves to a file). A
    /// prelude built-in or no reference yields `None`.
    pub fn definition(&self, offset: usize) -> Option<Definition> {
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
            ResolvedRef::Local(def_start) => Some(Definition::Here(def_start, def_start)),
            ResolvedRef::Module(id) => {
                let sym = self.resolved.symbols.table.get(id)?;
                match &sym.kind {
                    // An imported name: jump to its declaration in the target
                    // module's file (resolved by the server over the workspace).
                    SymbolKind::ImportNamed { path, original } => Some(Definition::InModule {
                        module_path: path
                            .segments
                            .iter()
                            .map(|s| s.as_ref())
                            .collect::<Vec<_>>()
                            .join("/"),
                        name: original.to_string(),
                    }),
                    // A module-level declaration in this file.
                    _ => Some(Definition::Here(sym.span.start, sym.span.start)),
                }
            }
            ResolvedRef::Prelude(_) => None,
        }
    }

    /// The canonical definition offset identifying the binding a `ResolvedRef`
    /// points at — a local's def-site, or a module symbol's declaration start —
    /// so two references to the same binding share one identity. `None` for a
    /// prelude built-in (nothing in this document defines it).
    fn ref_identity(&self, r: ResolvedRef) -> Option<u32> {
        match r {
            ResolvedRef::Local(def_start) => Some(def_start),
            ResolvedRef::Module(id) => Some(self.resolved.symbols.table.get(id)?.span.start),
            ResolvedRef::Prelude(_) => None,
        }
    }

    /// The binding at `offset` as `(identity, name, is_local)`, whether the cursor
    /// sits on a *reference* or on the *definition's* name. `None` for a prelude
    /// built-in, whitespace, or a position with no resolvable name. `is_local`
    /// distinguishes a function-body binding (safe to rename in isolation) from a
    /// module-level declaration (whose renames would need the workspace index).
    fn binding_at(&self, offset: usize, text: &str) -> Option<(u32, String, bool)> {
        // 1. A reference position: the innermost resolution covering `offset`.
        let mut best: Option<(u32, (u32, u32), ResolvedRef)> = None;
        for (span, r) in self.resolved.resolutions.iter() {
            if (span.start as usize) <= offset && offset < (span.end as usize) {
                let width = span.end - span.start;
                if best.as_ref().map_or(true, |(w, _, _)| width < *w) {
                    best = Some((width, (span.start, span.end), r));
                }
            }
        }
        if let Some((_, (s, e), r)) = best {
            let id = self.ref_identity(r)?;
            let name = text.get(s as usize..e as usize)?.to_string();
            let is_local = matches!(r, ResolvedRef::Local(_));
            return Some((id, name, is_local));
        }
        // 2. A definition position: a top-level declaration's name.
        for decl in &self.module.items {
            if let Some((name, span)) = top_decl_name_and_span(decl) {
                if let Some((ns, ne)) = whole_word_span(text, span.0, span.1, name) {
                    if (ns as usize) <= offset && offset < (ne as usize) {
                        return Some((span.0, name.to_string(), false));
                    }
                }
            }
        }
        // 3. A definition position: a local binding's name. A local's def-site
        //    start (`Local(def_start)`) points at the binding statement (the
        //    `let`/`for` keyword or the parameter), not the name, so find the
        //    name as the first whole word at/after that start. Each reference
        //    supplies the name.
        for (span, r) in self.resolved.resolutions.iter() {
            if let ResolvedRef::Local(def_start) = r {
                let Some(name) = text.get(span.start as usize..span.end as usize) else {
                    continue;
                };
                if let Some((ns, ne)) = local_name_span(text, def_start, name) {
                    if (ns as usize) <= offset && offset < (ne as usize) {
                        return Some((def_start, name.to_string(), true));
                    }
                }
            }
        }
        None
    }

    /// Every occurrence span `[start, end)` of the binding identified by
    /// `identity`/`name`/`is_local`, sorted and de-duplicated. Reference sites
    /// come from the resolution table; the declaration's own name span is added
    /// when `include_decl`.
    fn occurrences(
        &self,
        identity: u32,
        name: &str,
        is_local: bool,
        text: &str,
        include_decl: bool,
    ) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        for (span, r) in self.resolved.resolutions.iter() {
            if self.ref_identity(r) == Some(identity) {
                out.push((span.start, span.end));
            }
        }
        if include_decl {
            let decl_span = if is_local {
                // `identity` is the binding statement's start; the name is the
                // first whole word at/after it.
                local_name_span(text, identity, name)
            } else {
                // The module symbol's name sits inside its declaration; find it
                // by whole word so `fn`/`type`/`const` keywords are skipped.
                self.module
                    .items
                    .iter()
                    .find_map(|d| top_decl_name_and_span(d).filter(|(_, s)| s.0 == identity))
                    .and_then(|(n, s)| whole_word_span(text, s.0, s.1, n))
                    .or(Some((identity, identity + name.len() as u32)))
            };
            if let Some(span) = decl_span {
                out.push(span);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Every reference to the name at `offset`, as byte spans `[start, end)`. The
    /// declaration site is included when `include_decl`. File-scoped: it sees the
    /// open document only, so a symbol used from another module is under-reported
    /// until the workspace index lands. Empty when `offset` is not on a name.
    pub fn references(&self, offset: usize, text: &str, include_decl: bool) -> Vec<(u32, u32)> {
        match self.binding_at(offset, text) {
            Some((identity, name, is_local)) => {
                self.occurrences(identity, &name, is_local, text, include_decl)
            }
            None => Vec::new(),
        }
    }

    /// The edit spans for renaming the binding at `offset` to `new_name`: every
    /// occurrence (declaration included) to overwrite with `new_name`. Restricted
    /// to **local bindings** — a `let`, parameter, `match`/`for`/lambda binding —
    /// which cannot be referenced from another file, so a file-local rename is
    /// complete. A module-level declaration is refused (`ModuleLevelUnsupported`)
    /// until the workspace index can find its cross-file references. The new name
    /// is validated as a legal, non-keyword identifier.
    pub fn rename_edits(
        &self,
        offset: usize,
        text: &str,
        new_name: &str,
    ) -> Result<Vec<(u32, u32)>, RenameError> {
        if !is_valid_ident(new_name) {
            return Err(RenameError::InvalidIdentifier);
        }
        if KEYWORDS.contains(&new_name) {
            return Err(RenameError::ReservedKeyword);
        }
        let (identity, name, is_local) =
            self.binding_at(offset, text).ok_or(RenameError::NoBinding)?;
        if !is_local {
            return Err(RenameError::ModuleLevelUnsupported);
        }
        Ok(self.occurrences(identity, &name, is_local, text, true))
    }

    /// The document outline: this module's top-level declarations, with a tagged
    /// union's variant constructors nested as children. Used for the editor
    /// outline, breadcrumbs, and the symbol picker.
    pub fn document_symbols(&self) -> Vec<OutlineSymbol> {
        module_outline(&self.module)
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

/// The top-level outline of a parsed module (used for both per-file document
/// symbols and the workspace symbol index). A tagged union's variants nest as
/// children.
pub fn module_outline(module: &glyph_ast::Module) -> Vec<OutlineSymbol> {
    let mut out = Vec::new();
    for decl in &module.items {
        let sym = match decl {
            Decl::Fn(f) => OutlineSymbol {
                name: f.name.to_string(),
                kind: OutlineKind::Function,
                span: (f.span.start, f.span.end),
                children: Vec::new(),
            },
            Decl::Component(c) => OutlineSymbol {
                name: c.name.to_string(),
                kind: OutlineKind::Function,
                span: (c.span.start, c.span.end),
                children: Vec::new(),
            },
            Decl::Const(c) => OutlineSymbol {
                name: c.name.to_string(),
                kind: OutlineKind::Constant,
                span: (c.span.start, c.span.end),
                children: Vec::new(),
            },
            Decl::Type(t) => {
                let children = match &t.body {
                    TypeExpr::Union { variants, .. } => variants
                        .iter()
                        .map(|v| OutlineSymbol {
                            name: v.name.to_string(),
                            kind: OutlineKind::Variant,
                            span: (v.span.start, v.span.end),
                            children: Vec::new(),
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                OutlineSymbol {
                    name: t.name.to_string(),
                    kind: OutlineKind::Type,
                    span: (t.span.start, t.span.end),
                    children,
                }
            }
            Decl::Import(_) => continue,
        };
        out.push(sym);
    }
    out
}

/// Parse `text` and return its top-level outline, or an empty list if it does
/// not parse. Parse-only (no resolve/typecheck) — fast enough to run over every
/// file for the workspace symbol index.
pub fn outline_of(text: &str) -> Vec<OutlineSymbol> {
    glyph_parser::parse(text)
        .map(|m| module_outline(&m))
        .unwrap_or_default()
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
        match a.definition(call).expect("resolves") {
            Definition::Here(start, _) => {
                assert!(start < call as u32, "def at {start} should precede call at {call}");
            }
            Definition::InModule { .. } => panic!("a same-file call should resolve Here"),
        }
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
    fn document_symbols_list_decls_and_nested_variants() {
        let a = analyze_full(
            "module x\ntype Color = Red | Blue\nfn paint() -> number {\n  return 1\n}\nconst N = 5\n",
        )
        .expect("parses");
        let syms = a.document_symbols();
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Color") && names.contains(&"paint") && names.contains(&"N"), "{names:?}");
        let color = syms.iter().find(|s| s.name == "Color").unwrap();
        let kids: Vec<&str> = color.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(kids, ["Red", "Blue"]);
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

    fn spans_text<'a>(text: &'a str, spans: &[(u32, u32)]) -> Vec<&'a str> {
        spans
            .iter()
            .map(|(s, e)| &text[*s as usize..*e as usize])
            .collect()
    }

    #[test]
    fn references_finds_a_local_from_its_uses_and_declaration() {
        let text =
            "module x\nfn f() -> number {\n  let count = 1\n  return count + count\n}\n";
        let a = analyze_full(text).expect("parses");
        // From a use site: declaration + both uses.
        let use1 = text.rfind("count").unwrap();
        let refs = a.references(use1, text, true);
        assert_eq!(refs.len(), 3, "{:?}", spans_text(text, &refs));
        assert!(spans_text(text, &refs).iter().all(|s| *s == "count"));
        // Without the declaration: only the two uses.
        assert_eq!(a.references(use1, text, false).len(), 2);
    }

    #[test]
    fn references_works_from_the_definition_name() {
        let text =
            "module x\nfn f() -> number {\n  let count = 1\n  return count + count\n}\n";
        let a = analyze_full(text).expect("parses");
        let def = text.find("count").unwrap(); // the `let count` binding site
        assert_eq!(a.references(def, text, true).len(), 3);
    }

    #[test]
    fn references_finds_a_module_function_uses_and_declaration() {
        let text = "module x\nfn helper() -> number {\n  return 1\n}\nfn main() -> number {\n  return helper() + helper()\n}\n";
        let a = analyze_full(text).expect("parses");
        let def = text.find("helper").unwrap(); // declaration name
        let refs = a.references(def, text, true);
        assert_eq!(refs.len(), 3, "decl + two calls: {:?}", spans_text(text, &refs));
        assert!(spans_text(text, &refs).iter().all(|s| *s == "helper"));
    }

    #[test]
    fn rename_a_local_edits_every_occurrence() {
        let text = "module x\nfn f() -> number {\n  let count = 1\n  return count + count\n}\n";
        let a = analyze_full(text).expect("parses");
        let def = text.find("count").unwrap();
        let edits = a.rename_edits(def, text, "total").expect("renameable");
        assert_eq!(edits.len(), 3);
        assert!(spans_text(text, &edits).iter().all(|s| *s == "count"));
    }

    #[test]
    fn rename_refuses_module_level_declarations() {
        let text = "module x\nfn helper() -> number {\n  return 1\n}\n";
        let a = analyze_full(text).expect("parses");
        let def = text.find("helper").unwrap();
        assert_eq!(
            a.rename_edits(def, text, "helper2"),
            Err(RenameError::ModuleLevelUnsupported)
        );
    }

    #[test]
    fn rename_rejects_keywords_and_bad_identifiers() {
        let text = "module x\nfn f() -> number {\n  let count = 1\n  return count\n}\n";
        let a = analyze_full(text).expect("parses");
        let def = text.find("count").unwrap();
        assert_eq!(
            a.rename_edits(def, text, "match"),
            Err(RenameError::ReservedKeyword)
        );
        assert_eq!(
            a.rename_edits(def, text, "2bad"),
            Err(RenameError::InvalidIdentifier)
        );
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
