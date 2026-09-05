//! Pure analysis used by the language server: run the compiler front end over a
//! single in-memory document and collect diagnostics, plus a byte-offset →
//! line/character index for mapping spans to LSP positions.
//!
//! This module holds no `tower-lsp` types, so it is unit-testable without an LSP
//! runtime. The server (`lib.rs`) converts `GlyphDiagnostic` to the protocol
//! type using `LineIndex`.

use std::collections::BTreeSet;

use glyph_ast::{
    Block, Decl, Expr, ImportKind, JsxAttr, JsxChild, JsxElement, Module, TypeExpr,
};
use glyph_resolver::{
    build_prelude, collect_module_symbols, module_lints, resolve_module, verify_imports,
    QualifiedTypeRef, ResolvedModule, ResolvedRef, StdlibStubs, SymbolId, SymbolKind,
};
use glyph_typechecker::{assign_types, display_ty, DiagnosticUnion, TypeMap};

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
    /// The bare name of the top-level declaration this diagnostic belongs to,
    /// when one is known: the name the checker put on the error if it had the
    /// declaration in hand (`TypeError::decl_name`), otherwise the declaration
    /// whose span contains the diagnostic (see `enclosing_decl_name`). `None`
    /// for a diagnostic with no declaration to name: a parse failure (no AST
    /// exists yet), or a span on the `module` line or an import. This tool has
    /// no project index to qualify the name with a
    /// module path — the caller (`glyph_diagnostics`), which does know the
    /// file's own path, assembles the qualified `module::name` form the same
    /// way `glyph_variants` addresses a declaration site.
    pub decl_name: Option<String>,
    /// The union this diagnostic is about, when it is about one: the
    /// exhaustiveness errors, which name a union and a set of its variants in
    /// their message and nowhere else, so an agent repairing the match had to
    /// regex the sentence to make its next query (G195).
    ///
    /// Carried unqualified for the same reason `decl_name` is: a union
    /// declared in this file has no module half here, because the module half
    /// is counted from a root only the caller knows. The caller supplies it to
    /// `DiagnosticUnion::declaration`, which assembles the `module::name`
    /// form.
    ///
    /// `analysis` is a private module, so a field with no in-crate reader is
    /// dead code, and the reader these two are for is `tool_diagnostics`,
    /// which renders a `GlyphDiagnostic` into the `glyph_diagnostics` reply
    /// the way it already renders `decl_name`. Delete both attributes in the
    /// change that adds those two keys; they are the marker for it, not a
    /// judgement that the fields can stay unread.
    #[allow(dead_code)]
    pub union: Option<DiagnosticUnion>,
    /// The variants the match leaves unmentioned, in declaration order and
    /// unquoted. `None`, never an empty list, on a diagnostic that reports no
    /// such set.
    #[allow(dead_code)]
    pub missing_variants: Option<Vec<String>>,
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
            // No `decl_name`: a file that failed to parse has no AST to look
            // an enclosing declaration up in.
            return vec![GlyphDiagnostic {
                start: e.span().start,
                end: e.span().end,
                message: with_help(format!("{e}"), e.help().as_deref()),
                code: e.code().to_string(),
                decl_name: None,
                union: None,
                missing_variants: None,
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
                out.push(resolve_diag(&e, &module));
            }
            return out;
        }
    };

    let stdlib = StdlibStubs::new();
    for e in verify_imports(&module, &stdlib) {
        out.push(resolve_diag(&e, &module));
    }

    let prelude = build_prelude();
    let (resolved, resolve_errors) = resolve_module(&module, symbols, &prelude);
    for e in &resolve_errors {
        out.push(resolve_diag(e, &module));
    }

    let (_types, type_errors) = assign_types(&module, &resolved, &prelude);
    for e in &type_errors {
        out.push(GlyphDiagnostic {
            start: e.span().start,
            end: e.span().end,
            message: with_help(format!("{e}"), e.help()),
            code: e.code().to_string(),
            // An error the checker raised while holding a declaration names it
            // itself; an annotation's span sits before the keyword the
            // declaration's span starts at, so the walk cannot find it.
            decl_name: e
                .decl_name()
                .map(str::to_string)
                .or_else(|| enclosing_decl_name(&module, e.span().start)),
            // The other entity the error concerns. An exhaustiveness error
            // names a union and a set of its variants; every other error names
            // neither, and answers nothing rather than guessing.
            union: e.union().cloned(),
            missing_variants: e.missing_variants().map(<[String]>::to_vec),
        });
    }

    // The warning-tier lints (unused import E0106, unused let E0107, unreachable
    // E0108) — the same ones `glyph build` surfaces — so the editor shows them
    // too, and the unused-import quick-fix has something to act on. They run only
    // on an otherwise error-free module, so they never mask a real error.
    if out.is_empty() {
        for e in module_lints(&module, &resolved) {
            out.push(resolve_diag(&e, &module));
        }
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

/// The symbol under the cursor, classified for cross-file operations.
#[derive(Debug, PartialEq, Eq)]
pub enum SymbolTarget {
    /// A file-private binding (a `let`, parameter, `match`/`for`/lambda binding).
    /// It cannot be referenced from another file.
    Local,
    /// A module-level symbol identified globally by `(module path, name)` — where
    /// it is declared, so every file's references agree on one identity.
    Global { module: String, name: String },
}

/// Validate a proposed rename target name: a legal Glyph identifier that is not a
/// reserved keyword. Shared by the local and workspace rename paths.
pub fn validate_rename_name(new_name: &str) -> Result<(), RenameError> {
    if !is_valid_ident(new_name) {
        return Err(RenameError::InvalidIdentifier);
    }
    if KEYWORDS.contains(&new_name) {
        return Err(RenameError::ReservedKeyword);
    }
    Ok(())
}

/// Join module-path segments into the `a/b` form used in imports and file paths.
fn join_segments(segments: &[glyph_ast::Ident]) -> String {
    segments
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every namespace-qualified read of `(sym_module, name)` in this file, as the
/// span of the *name* half of each `ns.name` path.
///
/// The resolver already collects these: `qualified_type_refs` holds every
/// `ns.name` reached through `import ns` or `import ns as n`, in value and in
/// type position, and it is the list the export check runs on, which is what
/// makes `import render` + `render.secret` report the same E0105 the named
/// spelling reports. Reading references off the same list is what keeps the two
/// spellings from drifting apart again (G186).
///
/// The span recorded is the whole path, and what comes back is the last
/// segment. Both producers end the path span at the end of its final
/// identifier (`Expr::Member` is built as `object.start..field_span.end`, and a
/// `TypeExpr::Path` walks `end` forward to its last segment), so the name sits
/// at the tail and is read there rather than searched for: searching would
/// point `label.label` at the namespace. The narrow span is also what workspace
/// rename writes over, and a site reported as the whole `render.label` would
/// take the namespace with it.
fn qualified_occurrences_in(
    resolved: &ResolvedModule,
    sym_module: &str,
    name: &str,
    text: &str,
) -> Vec<(u32, u32)> {
    resolved
        .qualified_type_refs
        .iter()
        .filter(|q| q.name.as_ref() == name && join_segments(&q.module.segments) == sym_module)
        .filter_map(|q| qualified_name_span(q, text))
        .collect()
}

/// The span of the name half of one recorded `ns.name` path. See
/// `qualified_occurrences_in` for why it is read off the tail of the path span
/// rather than searched for.
fn qualified_name_span(q: &QualifiedTypeRef, text: &str) -> Option<(u32, u32)> {
    let end = q.span.end as usize;
    let start = end.checked_sub(q.name.as_ref().len())?;
    if start < q.span.start as usize || text.get(start..end)? != q.name.as_ref() {
        return None;
    }
    let leads_a_longer_word = text[..start].chars().next_back().is_some_and(is_ident_char);
    (!leads_a_longer_word).then_some((start as u32, end as u32))
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
        Decl::Interface(i) => Some((i.name.as_ref(), (i.span.start, i.span.end))),
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

// ---------------------------------------------------------------------------
// The shared query layer.
//
// Every position query below is a free function over the *pieces* of an
// analysis rather than a method on a bundle, because the two callers hold
// those pieces differently: the language server owns an `Analysis` built from
// the editor's in-memory buffer, while the MCP server reads them out of the
// salsa database (`glyph_db::parse_module`, `glyph_db::resolve`) and never
// materializes an `Analysis` at all. Splitting them also keeps the type map
// out of the signatures that do not need it — only `hover_at` reads it, which
// is what lets the MCP references path skip `assign_types` entirely.
//
// `Analysis` below is a convenience wrapper over exactly these functions.
// ---------------------------------------------------------------------------

/// The rendered type of the innermost typed expression covering `offset`, for
/// hover. `None` when no typed expression is there or its type is the
/// not-yet-inferred placeholder.
pub fn hover_at(types: &TypeMap, offset: usize) -> Option<String> {
    let mut best: Option<(u32, String)> = None;
    for (span, ty) in types.iter() {
        if (span.start as usize) <= offset && offset < (span.end as usize) {
            let width = span.end - span.start;
            if best.as_ref().is_none_or(|(w, _)| width < *w) {
                best = Some((width, display_ty(ty)));
            }
        }
    }
    best.map(|(_, rendered)| rendered).filter(|s| s != "?")
}

/// Where the name reference covering `offset` is defined, for go-to-definition:
/// within this file (`Here`), or in another module (an imported name —
/// `InModule`, which the server resolves to a file). A prelude built-in or no
/// reference yields `None`.
///
/// Takes only the resolution table: unlike its siblings this query never reads
/// the AST, so it does not ask for a `Module` it would ignore.
pub fn definition_at(resolved: &ResolvedModule, offset: usize) -> Option<Definition> {
    match innermost_ref(resolved, offset)? {
        ResolvedRef::Local(def_start) => Some(Definition::Here(def_start, def_start)),
        ResolvedRef::Module(id) => {
            let sym = resolved.symbols.table.get(id)?;
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

/// The innermost resolution covering `offset`, or `None` when the position is
/// not on a resolved name. "Innermost" is the narrowest span, so a name inside
/// a larger resolved expression wins.
fn innermost_ref(resolved: &ResolvedModule, offset: usize) -> Option<ResolvedRef> {
    innermost_ref_span(resolved, offset).map(|(_, r)| r)
}

/// As `innermost_ref`, but also reporting the covering span.
fn innermost_ref_span(resolved: &ResolvedModule, offset: usize) -> Option<((u32, u32), ResolvedRef)> {
    let mut best: Option<(u32, (u32, u32), ResolvedRef)> = None;
    for (span, r) in resolved.resolutions.iter() {
        if (span.start as usize) <= offset && offset < (span.end as usize) {
            let width = span.end - span.start;
            if best.as_ref().is_none_or(|(w, _, _)| width < *w) {
                best = Some((width, (span.start, span.end), r));
            }
        }
    }
    best.map(|(_, span, r)| (span, r))
}

/// The canonical definition offset identifying the binding a `ResolvedRef`
/// points at — a local's def-site, or a module symbol's declaration start — so
/// two references to the same binding share one identity. `None` for a prelude
/// built-in (nothing in this document defines it).
fn ref_identity(resolved: &ResolvedModule, r: ResolvedRef) -> Option<u32> {
    match r {
        ResolvedRef::Local(def_start) => Some(def_start),
        ResolvedRef::Module(id) => Some(resolved.symbols.table.get(id)?.span.start),
        ResolvedRef::Prelude(_) => None,
    }
}

/// The binding at `offset` as `(identity, name, is_local)`, whether the cursor
/// sits on a *reference* or on the *definition's* name. `None` for a prelude
/// built-in, whitespace, or a position with no resolvable name. `is_local`
/// distinguishes a function-body binding (safe to rename in isolation) from a
/// module-level declaration (whose renames would need the workspace index).
fn binding_at(
    module: &Module,
    resolved: &ResolvedModule,
    offset: usize,
    text: &str,
) -> Option<(u32, String, bool)> {
    // 1. A reference position: the innermost resolution covering `offset`.
    if let Some(((s, e), r)) = innermost_ref_span(resolved, offset) {
        let id = ref_identity(resolved, r)?;
        let name = text.get(s as usize..e as usize)?.to_string();
        let is_local = matches!(r, ResolvedRef::Local(_));
        return Some((id, name, is_local));
    }
    // 2. A definition position: a top-level declaration's name.
    for decl in &module.items {
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
    for (span, r) in resolved.resolutions.iter() {
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
/// `identity`/`name`/`is_local`, sorted and de-duplicated. Reference sites come
/// from the resolution table; the declaration's own name span is added when
/// `include_decl`.
fn occurrences_of(
    module: &Module,
    resolved: &ResolvedModule,
    identity: u32,
    name: &str,
    is_local: bool,
    text: &str,
    include_decl: bool,
) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (span, r) in resolved.resolutions.iter() {
        if ref_identity(resolved, r) == Some(identity) {
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
            module
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
/// declaration site is included when `include_decl`. File-scoped: it sees one
/// document, so a symbol used from another module is under-reported — the
/// workspace-wide answer is `symbol_target_at` plus `global_occurrences_in`
/// over every file. Empty when `offset` is not on a name.
pub fn references_at(
    module: &Module,
    resolved: &ResolvedModule,
    offset: usize,
    text: &str,
    include_decl: bool,
) -> Vec<(u32, u32)> {
    match binding_at(module, resolved, offset, text) {
        Some((identity, name, is_local)) => {
            occurrences_of(module, resolved, identity, &name, is_local, text, include_decl)
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
pub fn rename_edits_at(
    module: &Module,
    resolved: &ResolvedModule,
    offset: usize,
    text: &str,
    new_name: &str,
) -> Result<Vec<(u32, u32)>, RenameError> {
    validate_rename_name(new_name)?;
    let (identity, name, is_local) =
        binding_at(module, resolved, offset, text).ok_or(RenameError::NoBinding)?;
    if !is_local {
        return Err(RenameError::ModuleLevelUnsupported);
    }
    Ok(occurrences_of(module, resolved, identity, &name, is_local, text, true))
}

/// The global identity `(module_path, name)` of a module-level symbol id, or
/// `None` for a namespace import, alias, or prelude built-in. An imported name
/// reports the module it came from; any other module symbol reports
/// `this_module` (the file it is declared in).
fn module_global_of(
    resolved: &ResolvedModule,
    id: SymbolId,
    this_module: &str,
) -> Option<(String, String)> {
    let sym = resolved.symbols.table.get(id)?;
    match &sym.kind {
        SymbolKind::ImportNamed { path, original } => {
            Some((join_segments(&path.segments), original.to_string()))
        }
        SymbolKind::ImportNamespace { .. }
        | SymbolKind::ImportAlias { .. }
        | SymbolKind::Prelude { .. } => None,
        _ => Some((this_module.to_string(), sym.name.to_string())),
    }
}

/// Resolve the symbol at `offset` for cross-file operations, given the file's
/// own module path `this_module`. `Local` is file-private and never crosses
/// files; `Global { module, name }` is a module-level symbol keyed by where it
/// is declared (its own module for a same-file declaration, the import's module
/// for an imported name). `None` when `offset` is not on a resolvable name.
pub fn symbol_target_at(
    module: &Module,
    resolved: &ResolvedModule,
    offset: usize,
    text: &str,
    this_module: &str,
) -> Option<SymbolTarget> {
    // 1. A reference position.
    if let Some(r) = innermost_ref(resolved, offset) {
        return match r {
            ResolvedRef::Local(_) => Some(SymbolTarget::Local),
            ResolvedRef::Module(id) => module_global_of(resolved, id, this_module)
                .map(|(module, name)| SymbolTarget::Global { module, name }),
            ResolvedRef::Prelude(_) => None,
        };
    }
    // 2. A top-level declaration name.
    for decl in &module.items {
        if let Some((name, span)) = top_decl_name_and_span(decl) {
            if let Some((ns, ne)) = whole_word_span(text, span.0, span.1, name) {
                if (ns as usize) <= offset && offset < (ne as usize) {
                    return Some(SymbolTarget::Global {
                        module: this_module.to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
    }
    // 2b. A tagged-union variant name (a module-scope constructor).
    for top in module_outline(module) {
        for child in &top.children {
            if let Some((ns, ne)) = whole_word_span(text, child.span.0, child.span.1, &child.name) {
                if (ns as usize) <= offset && offset < (ne as usize) {
                    return Some(SymbolTarget::Global {
                        module: this_module.to_string(),
                        name: child.name.clone(),
                    });
                }
            }
        }
    }
    // 3. An `import M { name }` binding.
    for decl in &module.items {
        if let Decl::Import(im) = decl {
            if let ImportKind::Named(names) = &im.kind {
                let module = join_segments(&im.path.segments);
                for n in names {
                    if let Some((ns, ne)) =
                        whole_word_span(text, im.span.start, im.span.end, n.as_ref())
                    {
                        if (ns as usize) <= offset && offset < (ne as usize) {
                            return Some(SymbolTarget::Global {
                                module,
                                name: n.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    // 4. A local binding name.
    for (span, r) in resolved.resolutions.iter() {
        if let ResolvedRef::Local(def_start) = r {
            let Some(name) = text.get(span.start as usize..span.end as usize) else {
                continue;
            };
            if let Some((ns, ne)) = local_name_span(text, def_start, name) {
                if (ns as usize) <= offset && offset < (ne as usize) {
                    return Some(SymbolTarget::Local);
                }
            }
        }
    }
    // 5. The name half of a namespace-qualified `ns.name`.
    //
    // Step 1 cannot reach it: the resolution recorded for that expression sits
    // on `ns` and names the module. Without this the cursor on `label` in
    // `render.label` addressed nothing, and `glyph_references` answered `[]`
    // for a symbol with at least the reference under the cursor. The set of
    // references has to be the same whichever site of it the caller points at,
    // for the same reason it has to be the same whichever way the import is
    // spelled.
    for q in &resolved.qualified_type_refs {
        if let Some((ns, ne)) = qualified_name_span(q, text) {
            if (ns as usize) <= offset && offset < (ne as usize) {
                return Some(SymbolTarget::Global {
                    module: join_segments(&q.module.segments),
                    name: q.name.to_string(),
                });
            }
        }
    }
    None
}

/// Every occurrence of the globally-identified symbol `(sym_module, name)` in
/// THIS file, whose own module path is `this_module`. This is the per-file half
/// of a workspace-wide references/rename: the server runs it over every file.
///
/// Reference sites come from two places, and it takes both to cover the ways a
/// consumer can spell its import. A name brought in by `import sym_module
/// { name }` resolves to a symbol and is in the resolution table. A
/// namespace-qualified `ns.name` is not: the table holds one entry for that
/// expression, on `ns`, and it points at the module. Those come from
/// `qualified_occurrences_in`.
///
/// The declaration or import-binding site is added when `include_decl`: the
/// declaration itself in the defining module, or the `import sym_module
/// { name }` token in an importing module. A namespace import has no such
/// token: `import render` writes `render`, and reporting it as a reference to
/// `label` would claim a site whose text is a different name, and would hand
/// rename the module to rewrite. So the two spellings agree on every use and
/// differ by the one token that exists in only one of them.
pub fn global_occurrences_in(
    module: &Module,
    resolved: &ResolvedModule,
    this_module: &str,
    sym_module: &str,
    name: &str,
    text: &str,
    include_decl: bool,
) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (span, r) in resolved.resolutions.iter() {
        if let ResolvedRef::Module(id) = r {
            if module_global_of(resolved, id, this_module)
                .as_ref()
                .map(|(m, n)| (m.as_str(), n.as_str()))
                == Some((sym_module, name))
            {
                out.push((span.start, span.end));
            }
        }
    }
    // A namespace-qualified read is a reference to the same symbol a named
    // import reads, so it belongs in the same answer. The resolution table
    // alone cannot see it: `render.label` records one resolution, for `render`,
    // and it points at the module.
    out.extend(qualified_occurrences_in(resolved, sym_module, name, text));
    if include_decl {
        if this_module == sym_module {
            // The declaration in the defining module: a top-level decl name,
            // or a tagged-union variant name.
            for decl in &module.items {
                if let Some((n, span)) = top_decl_name_and_span(decl) {
                    if n == name {
                        if let Some(ws) = whole_word_span(text, span.0, span.1, n) {
                            out.push(ws);
                        }
                    }
                }
            }
            for top in module_outline(module) {
                for child in &top.children {
                    if child.name == name {
                        if let Some(ws) = whole_word_span(text, child.span.0, child.span.1, name) {
                            out.push(ws);
                        }
                    }
                }
            }
        } else {
            // The `import sym_module { name }` binding token in an importer.
            for decl in &module.items {
                if let Decl::Import(im) = decl {
                    if join_segments(&im.path.segments) == sym_module {
                        if let ImportKind::Named(names) = &im.kind {
                            if names.iter().any(|nm| nm.as_ref() == name) {
                                if let Some(ws) =
                                    whole_word_span(text, im.span.start, im.span.end, name)
                                {
                                    out.push(ws);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

// ============================================================================
// Relations
// ============================================================================

/// A relationship the compiler computed between one entity and one site.
///
/// **One set, one spelling.** Every surface that names a relation names it from
/// here, and the name is identical in a request, in a reply, and in a coverage
/// statement. Before this existed the tree held two sets that never overlapped:
/// `glyph_references` spelled `CALLS` and `REFERENCES` on the wire, and
/// `glyph_variants` named no relation at all, carrying its site kinds as the
/// positional keys `sites`, `nested` and `unkeyed` so that position was the
/// only thing telling them apart. Two sets that never meet cannot be selected
/// from and cannot be named in a reply (G193).
///
/// The set is closed. A member is here because the checker already computes
/// it, and a name outside it is a question no surface can answer rather than a
/// relation nobody has got round to.
///
/// Membership is not the same as being answerable today. A relation the tree
/// holds a name for and no surface computes is refused when it is asked for,
/// because an empty edge list for it would say the relationship does not hold.
/// Which surface answers which relation is the MCP layer's own knowledge and
/// lives there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relation {
    /// The occurrence is the callee name of a call or a `new`: the site applies
    /// the symbol to an argument list. This is the set that stops compiling
    /// when the symbol's parameters, arity, or variant payload change.
    ///
    /// The callee has to be the name itself. `io.println()` applies a member of
    /// `io` rather than `io`; `let g = foo` then `g()` applies `g`; `fs[k]()`
    /// applies whatever the index yields. In each of those the named symbol is
    /// read and something else is applied, so the call is not a call *of* it,
    /// and the site is still reported under [`Relation::References`].
    Calls,
    /// The site names the entity without applying it: the declaration's own
    /// name, an `import` binding, a type annotation naming the symbol, a value
    /// read, the symbol passed as an argument rather than applied to one, a
    /// `match` pattern naming a variant, a JSX element naming a component.
    ///
    /// [`Relation::Calls`] and this one partition the occurrences of a symbol,
    /// so the two lists together are the flat occurrence list
    /// [`global_occurrences_in`] returns and asking for one loses no site.
    References,
    /// A `match` whose scrutinee is the entity, and which variants each of its
    /// arms names. This is the relation the match-coverage check writes while
    /// it types each file, and it is what a variant being added or removed
    /// travels along.
    MatchSites,
    /// A read or a write of a record field, keyed to the record the checker
    /// resolved the access against. Written by the member-access check as it
    /// types each file, which is why a site is here because a field set
    /// resolved rather than because a spelling matched.
    FieldAccess,
    /// A declaration a `glyph gen` run wrote, and the source artifact it was
    /// written from. Read out of the generation record the generator writes
    /// into the file's header rather than computed from the program, which is
    /// why every edge here is `ASSERTED`: it is the generator's claim, and the
    /// content hash beside it is what a caller checks the claim against.
    GeneratedFrom,
}

impl Relation {
    /// The name this relation is called on the wire. The set is closed, so the
    /// spellings exist here and nowhere else: a surface that wants to name a
    /// relation asks for it rather than writing the word out.
    pub fn wire(self) -> &'static str {
        match self {
            Relation::Calls => "CALLS",
            Relation::References => "REFERENCES",
            Relation::MatchSites => "MATCH_SITES",
            Relation::FieldAccess => "FIELD_ACCESS",
            Relation::GeneratedFrom => "GENERATED_FROM",
        }
    }

    /// Every relation, in the order an answer lists them.
    pub fn all() -> [Relation; 5] {
        [
            Relation::Calls,
            Relation::References,
            Relation::MatchSites,
            Relation::FieldAccess,
            Relation::GeneratedFrom,
        ]
    }

    /// The relation `wire` spells, or `None` when it names none of them. A
    /// caller that asks for a relation outside the vocabulary has to be told
    /// so: silently widening the request to everything would answer a
    /// different question, and silently narrowing it to nothing would return an
    /// empty list that reads as "no such edges exist".
    pub fn from_wire(wire: &str) -> Option<Relation> {
        Relation::all().into_iter().find(|r| r.wire() == wire)
    }

    /// The vocabulary as one comma-separated list, for a message that has to
    /// state what the closed set holds.
    pub fn vocabulary() -> String {
        Relation::all().map(|r| r.wire()).join(", ")
    }
}

/// One occurrence of a symbol, `[start, end)`, and the relation it stands in.
pub struct RelatedSpan {
    pub start: u32,
    pub end: u32,
    pub relation: Relation,
}

/// Every span in `module` that names the callee of a call or a `new`.
///
/// The span recorded is the callee identifier's own span, which is the span the
/// resolver keys a name reference by (`Expr::Ident { span }`), so an entry here
/// can be compared against an occurrence span directly rather than by
/// containment.
///
/// Only a bare name counts. A member callee (`io.println()`), an indexed one
/// (`handlers[k]()`) and a call through a local alias each apply something
/// other than the named symbol. A JSX element naming a component
/// (`<UserSearch />`) is not a call expression, and the resolver keys it by the
/// whole element's span rather than by the name's, so it is not in this set
/// either.
///
/// Applying a tagged-union variant (`Ok(3)`) *is* in it. The site applies the
/// constructor to an argument list and stops compiling when the payload
/// changes, which is the property this relation is for; that the emitter writes
/// an object literal rather than a function call is a representation detail.
pub fn callee_name_spans(module: &Module) -> BTreeSet<(u32, u32)> {
    let mut out = BTreeSet::new();
    for decl in &module.items {
        match decl {
            Decl::Fn(f) => block_callees(&f.body, &mut out),
            Decl::Component(c) => block_callees(&c.body, &mut out),
            Decl::Const(c) => expr_callees(&c.value, &mut out),
            // A `where` refinement is an expression, and can hold a call (D39).
            Decl::Type(t) => {
                if let Some(pred) = &t.refinement {
                    expr_callees(pred, &mut out);
                }
            }
            // Neither holds an expression: an interface is member signatures, an
            // import is a module path and a name list.
            Decl::Interface(_) | Decl::Import(_) => {}
        }
    }
    out
}

fn block_callees(block: &Block, out: &mut BTreeSet<(u32, u32)>) {
    for stmt in &block.stmts {
        glyph_ast::visit::stmt_exprs(stmt, &mut |e| expr_callees(e, out));
        glyph_ast::visit::stmt_blocks(stmt, &mut |b| block_callees(b, out));
    }
}

fn expr_callees(e: &Expr, out: &mut BTreeSet<(u32, u32)>) {
    if let Expr::Call { callee, .. } | Expr::New { callee, .. } = e {
        if let Expr::Ident { span, .. } = &**callee {
            out.insert((span.start, span.end));
        }
    }
    glyph_ast::visit::child_exprs(e, &mut |c| expr_callees(c, out));
    glyph_ast::visit::child_blocks(e, &mut |b| block_callees(b, out));
    // `child_exprs` does not descend into a JSX element, so the expressions
    // inside one are reached here or not at all.
    if let Expr::Jsx(el) = e {
        jsx_callees(el, out);
    }
}

fn jsx_callees(el: &JsxElement, out: &mut BTreeSet<(u32, u32)>) {
    for attr in &el.attrs {
        match attr {
            JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } => expr_callees(value, out),
            JsxAttr::String { .. } | JsxAttr::Positional { .. } => {}
        }
    }
    for child in &el.children {
        match child {
            JsxChild::Element(inner) => jsx_callees(inner, out),
            JsxChild::Expr(value) => expr_callees(value, out),
            JsxChild::Text { .. } => {}
        }
    }
}

fn relation_of(callees: &BTreeSet<(u32, u32)>, start: u32, end: u32) -> Relation {
    if callees.contains(&(start, end)) {
        Relation::Calls
    } else {
        Relation::References
    }
}

/// [`global_occurrences_in`] with each occurrence's relation beside it.
///
/// The classification is a set membership against this file's own callee spans,
/// so it costs one walk of the file and answers from the same parse the
/// occurrences came from. There is no third outcome: an occurrence is in the
/// callee set or it is not, and both answers are facts about a tree the
/// compiler built.
pub fn global_relations_in(
    module: &Module,
    resolved: &ResolvedModule,
    this_module: &str,
    sym_module: &str,
    name: &str,
    text: &str,
    include_decl: bool,
) -> Vec<RelatedSpan> {
    let callees = callee_name_spans(module);
    global_occurrences_in(
        module,
        resolved,
        this_module,
        sym_module,
        name,
        text,
        include_decl,
    )
    .into_iter()
    .map(|(start, end)| RelatedSpan {
        start,
        end,
        relation: relation_of(&callees, start, end),
    })
    .collect()
}

/// [`references_at`] with each occurrence's relation beside it.
pub fn relations_at(
    module: &Module,
    resolved: &ResolvedModule,
    offset: usize,
    text: &str,
    include_decl: bool,
) -> Vec<RelatedSpan> {
    let callees = callee_name_spans(module);
    references_at(module, resolved, offset, text, include_decl)
        .into_iter()
        .map(|(start, end)| RelatedSpan {
            start,
            end,
            relation: relation_of(&callees, start, end),
        })
        .collect()
}

impl Analysis {
    /// See [`hover_at`].
    pub fn hover(&self, offset: usize) -> Option<String> {
        hover_at(&self.types, offset)
    }

    /// Inlay type hints: for each `let` with no written type annotation, the
    /// inferred type of its initializer, positioned just after the binding name
    /// (`let x‹: number› = 1`). Returns `(byte offset, label)` pairs. Only
    /// concrete inferences are shown; an un-inferred (`?`) or `unknown` type is
    /// skipped rather than shown as noise.
    pub fn inlay_type_hints(&self, text: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        for decl in &self.module.items {
            let body = match decl {
                glyph_ast::Decl::Fn(f) => Some(&f.body),
                glyph_ast::Decl::Component(c) => Some(&c.body),
                _ => None,
            };
            if let Some(b) = body {
                self.block_inlay_hints(b, text, &mut out);
            }
        }
        out
    }

    fn block_inlay_hints(&self, block: &glyph_ast::Block, text: &str, out: &mut Vec<(usize, String)>) {
        for stmt in &block.stmts {
            match stmt {
                glyph_ast::Stmt::Let(l) if l.ty.is_none() => {
                    let vs = l.value.span();
                    if let Some(ty) = self.type_of_exact(vs) {
                        if let Some(pos) = let_name_end(text, l) {
                            out.push((pos, format!(": {ty}")));
                        }
                    }
                }
                glyph_ast::Stmt::For(f) => self.block_inlay_hints(&f.body, text, out),
                glyph_ast::Stmt::Loop(l) => self.block_inlay_hints(&l.body, text, out),
                _ => {}
            }
        }
    }

    /// The rendered type recorded exactly for `span` (the initializer), skipping
    /// the not-yet-inferred `?` placeholder and the uninformative `unknown`.
    fn type_of_exact(&self, span: glyph_ast::Span) -> Option<String> {
        for (sp, ty) in self.types.iter() {
            if sp.start == span.start && sp.end == span.end {
                let d = display_ty(ty);
                if d != "?" && d != "unknown" && !d.is_empty() {
                    return Some(d);
                }
            }
        }
        None
    }

    /// See [`definition_at`].
    pub fn definition(&self, offset: usize) -> Option<Definition> {
        definition_at(&self.resolved, offset)
    }

    /// See [`references_at`].
    pub fn references(&self, offset: usize, text: &str, include_decl: bool) -> Vec<(u32, u32)> {
        references_at(&self.module, &self.resolved, offset, text, include_decl)
    }

    /// See [`rename_edits_at`].
    pub fn rename_edits(
        &self,
        offset: usize,
        text: &str,
        new_name: &str,
    ) -> Result<Vec<(u32, u32)>, RenameError> {
        rename_edits_at(&self.module, &self.resolved, offset, text, new_name)
    }

    /// See [`symbol_target_at`].
    pub fn symbol_target(
        &self,
        offset: usize,
        text: &str,
        this_module: &str,
    ) -> Option<SymbolTarget> {
        symbol_target_at(&self.module, &self.resolved, offset, text, this_module)
    }

    /// See [`global_occurrences_in`].
    pub fn global_occurrences(
        &self,
        this_module: &str,
        sym_module: &str,
        name: &str,
        text: &str,
        include_decl: bool,
    ) -> Vec<(u32, u32)> {
        global_occurrences_in(
            &self.module,
            &self.resolved,
            this_module,
            sym_module,
            name,
            text,
            include_decl,
        )
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
                Decl::Interface(i) => out.push(Completion {
                    label: i.name.to_string(),
                    tag: CompletionTag::Type,
                }),
                Decl::Import(_) => {}
            }
        }

        out
    }
}

/// The top-level outline of a parsed module (used for both per-file document
/// symbols and the workspace symbol index). A tagged union's variants nest as
/// children.
/// The byte offset just after a `let` binding's name, where an inlay type hint
/// sits. Locates the name in the source between `let` and the initializer.
fn let_name_end(text: &str, l: &glyph_ast::LetStmt) -> Option<usize> {
    let start = l.span.start as usize;
    let vs = l.value.span().start as usize;
    let head = text.get(start..vs)?;
    let name = l.name.as_ref();
    let rel = head.rfind(name)?;
    Some(start + rel + name.len())
}

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
            Decl::Interface(i) => OutlineSymbol {
                name: i.name.to_string(),
                kind: OutlineKind::Type,
                span: (i.span.start, i.span.end),
                children: Vec::new(),
            },
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

fn resolve_diag(e: &glyph_resolver::ResolveError, module: &Module) -> GlyphDiagnostic {
    GlyphDiagnostic {
        start: e.span().start,
        end: e.span().end,
        message: with_help(format!("{e}"), e.help()),
        code: e.code().to_string(),
        decl_name: enclosing_decl_name(module, e.span().start),
        // A resolve error is about a name, not about a union's variant set.
        union: None,
        missing_variants: None,
    }
}

fn with_help(message: String, help: Option<&str>) -> String {
    match help {
        Some(h) => format!("{message}\n{h}"),
        None => message,
    }
}

/// The bare name of the top-level declaration whose span contains `offset`,
/// or `None` when no top-level declaration contains it (an offset on the
/// `module` line itself, or inside an import, which re-binds another
/// module's name rather than declaring one of its own).
///
/// This is the only copy of the attribution rule. `glyph check --json` walks
/// the same AST for the same answer (`glyph-cli`'s `diagnostic::entity_id`),
/// and when the walk lived in two places the two surfaces drifted (G180). The
/// name comes back bare: the module half is the caller's, because each caller
/// counts it from a root only it knows.
/// Whether the top-level declaration named `name` in `module` is an `extern_ts`
/// escape, and which position it is written in.
///
/// The question is not what the declaration is called or where it lives. It is
/// whether the compiler holds a shape for it. `type Row =
/// extern_ts("z.infer<typeof s>")` is a declaration this project owns, keyed
/// under its own module and resolved like any other, and behind the name there
/// is a raw TypeScript string that no Glyph pass reads: `tsc` checks every use
/// of it and Glyph's own checker sees `unknown` with no descriptor. The same
/// holds for the expression form, `const handle =
/// extern_ts("globalThis.user")`, which is the identical escape in the other
/// position.
///
/// `None` means the declaration is ordinary, or that `module` declares no
/// top-level `name` at all. The caller distinguishes those two, because it is
/// the one that knows whether the name was supposed to be there.
pub fn extern_ts_escape(module: &Module, name: &str) -> Option<ExternEscape> {
    module.items.iter().find_map(|item| {
        if item.name().map(|n| n.as_ref() != name).unwrap_or(true) {
            return None;
        }
        match item {
            Decl::Type(t) => matches!(t.body, TypeExpr::Extern { .. }).then_some(ExternEscape::Type),
            Decl::Const(c) => {
                matches!(c.value, Expr::Extern { .. }).then_some(ExternEscape::Value)
            }
            _ => None,
        }
    })
}

/// Which position an `extern_ts` escape is written in. Both are the same hole
/// in the same place; they read differently in an answer, so the answer says
/// which one it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternEscape {
    /// `type X = extern_ts("<raw TypeScript type>")`, emitted verbatim as the
    /// TypeScript type and opaque to Glyph's checker.
    Type,
    /// `const x = extern_ts("<raw TypeScript expression>")`, typed `unknown` at
    /// the Glyph seam.
    Value,
}

impl ExternEscape {
    /// How the escape reads in an answer, as a noun phrase a sentence can
    /// carry.
    pub fn describe(self) -> &'static str {
        match self {
            ExternEscape::Type => "its definition is an `extern_ts` type escape, so the raw \
                                   TypeScript inside it is what `tsc` checks and Glyph's own \
                                   checker holds no shape for it",
            ExternEscape::Value => "its value is an `extern_ts` expression escape, typed \
                                    `unknown` at the Glyph seam, so the raw TypeScript inside \
                                    it is what `tsc` checks",
        }
    }
}

pub fn enclosing_decl_name(module: &Module, offset: u32) -> Option<String> {
    module.items.iter().find_map(|item| {
        let span = item.span();
        if span.start <= offset && offset < span.end {
            item.name().map(|name| name.to_string())
        } else {
            None
        }
    })
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
    fn global_occurrences_in_the_defining_module() {
        let text = "module a\nfn foo() -> number {\n  return 1\n}\nfn bar() -> number {\n  return foo() + foo()\n}\n";
        let an = analyze_full(text).expect("parses");
        // Declaration + two calls, all named `foo`.
        let occ = an.global_occurrences("a", "a", "foo", text, true);
        assert_eq!(occ.len(), 3, "{:?}", spans_text(text, &occ));
        assert!(spans_text(text, &occ).iter().all(|s| *s == "foo"));
        // The declaration itself resolves to a global target.
        let def = text.find("foo").unwrap();
        assert_eq!(
            an.symbol_target(def, text, "a"),
            Some(SymbolTarget::Global {
                module: "a".to_string(),
                name: "foo".to_string()
            })
        );
    }

    #[test]
    fn global_occurrences_in_an_importing_module() {
        let text =
            "module b\nimport a { foo }\nfn use_it() -> number {\n  return foo() + foo()\n}\n";
        let an = analyze_full(text).expect("parses");
        // The import binding token + two uses = 3, keyed to a's `foo`.
        let occ = an.global_occurrences("b", "a", "foo", text, true);
        assert_eq!(occ.len(), 3, "{:?}", spans_text(text, &occ));
        assert!(spans_text(text, &occ).iter().all(|s| *s == "foo"));
        // A use resolves to the same global identity as the declaration in `a`.
        let use1 = text.rfind("foo").unwrap();
        assert_eq!(
            an.symbol_target(use1, text, "b"),
            Some(SymbolTarget::Global {
                module: "a".to_string(),
                name: "foo".to_string()
            })
        );
    }

    #[test]
    fn workspace_rename_spans_defining_and_importing_modules() {
        // The composition the server performs: resolve the target in one module,
        // then collect occurrences from every module.
        let a_text = "module a\nfn foo() -> number {\n  return 1\n}\n";
        let b_text = "module b\nimport a { foo }\nfn use_it() -> number {\n  return foo()\n}\n";
        let a = analyze_full(a_text).expect("a parses");
        let b = analyze_full(b_text).expect("b parses");

        let def = a_text.find("foo").unwrap();
        let SymbolTarget::Global { module, name } =
            a.symbol_target(def, a_text, "a").expect("a global symbol")
        else {
            panic!("declaration should be a global symbol");
        };

        let in_a = a.global_occurrences("a", &module, &name, a_text, true);
        let in_b = b.global_occurrences("b", &module, &name, b_text, true);
        // a: the declaration only. b: the import binding + one use.
        assert_eq!(in_a.len(), 1, "{:?}", spans_text(a_text, &in_a));
        assert_eq!(in_b.len(), 2, "{:?}", spans_text(b_text, &in_b));
        assert!(spans_text(a_text, &in_a)
            .iter()
            .chain(spans_text(b_text, &in_b).iter())
            .all(|s| *s == "foo"));
    }

    /// The rename half of the same relation, under the other import spelling.
    ///
    /// The server builds a workspace edit out of exactly these spans. A
    /// qualified use it does not report is a rename that leaves the project
    /// uncompilable, and a qualified use reported as the whole `a.foo` is one
    /// that renames the namespace along with the name.
    ///
    /// Both positions are here because the resolver records them through two
    /// different walks: `a.Row` in an annotation and `a.foo` in an expression.
    #[test]
    fn workspace_rename_reaches_namespace_qualified_uses() {
        let a_text =
            "module a\npub type Row = { n: number, }\npub fn foo() -> number {\n  return 1\n}\n";
        let b_text =
            "module b\nimport a\nfn use_it(r: a.Row) -> number {\n  return a.foo() + r.n\n}\n";
        let a = analyze_full(a_text).expect("a parses");
        let b = analyze_full(b_text).expect("b parses");

        let in_b = b.global_occurrences("b", "a", "foo", b_text, true);
        assert_eq!(spans_text(b_text, &in_b), vec!["foo"], "the qualified call");
        let rows = b.global_occurrences("b", "a", "Row", b_text, true);
        assert_eq!(spans_text(b_text, &rows), vec!["Row"], "the qualified annotation");

        // The declaring module is unaffected by how anyone imports it.
        let in_a = a.global_occurrences("a", "a", "foo", a_text, true);
        assert_eq!(spans_text(a_text, &in_a), vec!["foo"]);

        // `import a` names a module, not `foo`, so it is not one of the sites.
        // Reporting it would put the namespace into a rename of the name.
        assert_eq!(in_b.len(), 1, "{:?}", spans_text(b_text, &in_b));

        // And the use addresses the declaration, so asking from either end of
        // the edge gives one answer.
        let at_call = b_text.find("a.foo").unwrap() + 2;
        assert_eq!(
            b.symbol_target(at_call, b_text, "b"),
            Some(SymbolTarget::Global {
                module: "a".to_string(),
                name: "foo".to_string()
            })
        );
    }

    #[test]
    fn global_occurrences_covers_union_variants() {
        let text = "module a\ntype Color = Red | Blue\nfn pick() -> Color {\n  return Red\n}\n";
        let an = analyze_full(text).expect("parses");
        // The variant definition in the union + one use.
        let occ = an.global_occurrences("a", "a", "Red", text, true);
        assert_eq!(occ.len(), 2, "{:?}", spans_text(text, &occ));
        assert!(spans_text(text, &occ).iter().all(|s| *s == "Red"));
    }

    #[test]
    fn line_index_counts_utf16() {
        // A non-BMP char (😀) is two UTF-16 code units.
        let text = "a😀b";
        let idx = LineIndex::new(text);
        // byte offset of 'b' is 1 (a) + 4 (😀) = 5; expect character 3 (1 + 2).
        assert_eq!(idx.position(text, 5), (0, 3));
    }

    /// G181: the annotation sits before the keyword the declaration's span
    /// starts at, so the containment walk finds nothing for it. The checker
    /// knew the declaration when it raised the error; the LSP diagnostic must
    /// carry that name rather than report the site as belonging to nothing.
    #[test]
    fn decl_name_names_the_declaration_an_annotation_decorates() {
        let text = "module x\n\n@puer\nfn f() -> number {\n  return 1\n}\n";
        let diags = analyze(text);
        let d = diags
            .iter()
            .find(|d| d.code == "E0221")
            .expect("E0221 emitted");
        assert_eq!(d.decl_name.as_deref(), Some("f"));
    }

    /// G195: the same fields `glyph check --json` carries, on the surface the
    /// editor and the MCP tools read. An agent that gets E0200 from
    /// `glyph_diagnostics` needs the union's name to ask `glyph_variants`
    /// which other sites match on it, and until now the only way to it was a
    /// regex over the message.
    #[test]
    fn a_non_exhaustive_match_carries_its_union_and_its_missing_variants() {
        let text = "module billing\n\n\
            type PaymentResult =\n  | Settled\n  | Failed\n  | Pending\n\n\
            fn settle(r: PaymentResult) -> string {\n\
            \x20 return match r {\n    Settled => \"s\",\n    Failed => \"f\",\n  }\n\
            }\n";
        let diags = analyze(text);
        let d = diags
            .iter()
            .find(|d| d.code == "E0200")
            .expect("E0200 emitted");
        assert_eq!(
            d.union,
            Some(DiagnosticUnion::Local {
                name: "PaymentResult".to_string()
            })
        );
        assert_eq!(
            d.missing_variants.as_deref(),
            Some(["Pending".to_string()].as_slice())
        );
        // The caller supplies the module half, exactly as it does for
        // `decl_name`, and the two then agree.
        assert_eq!(d.decl_name.as_deref(), Some("settle"));
        assert_eq!(
            d.union.as_ref().unwrap().declaration("app/billing"),
            Some("app/billing::PaymentResult".to_string())
        );
    }

    /// A prelude union is declared in no project module, so it has a name and
    /// no address. Absent, not invented.
    #[test]
    fn a_prelude_unions_gap_has_a_name_and_no_declaration() {
        let text = "module x\n\n\
            import std/result { Ok, Err }\n\n\
            fn f(r: Result<number, string>) -> number {\n\
            \x20 return match r {\n    Ok(n) => n,\n  }\n\
            }\n";
        let diags = analyze(text);
        let d = diags
            .iter()
            .find(|d| d.code == "E0200")
            .expect("E0200 emitted");
        let union = d.union.as_ref().expect("E0200 names a union");
        assert_eq!(union.name(), "Result");
        assert_eq!(union.declaration("x"), None);
    }

    /// A diagnostic about no union answers nothing for either field. An
    /// invented attribution is worse than an absent one.
    #[test]
    fn a_diagnostic_about_no_union_carries_neither_field() {
        let text = "module x\n\ntype Account = { email: string }\n\n\
            fn f(a: Account) -> string {\n  return a.emial\n}\n";
        let diags = analyze(text);
        let d = diags
            .iter()
            .find(|d| d.code == "E0210")
            .expect("E0210 emitted");
        assert_eq!(d.union, None);
        assert_eq!(d.missing_variants, None);
    }
}
