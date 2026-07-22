//! The Glyph language server (step 7).
//!
//! v1 increment 1: live **diagnostics** (parse/resolve/typecheck errors with
//! their stable codes, published on open/change) and **document formatting**
//! (the canonical `glyph fmt` layout). Hover, go-to-definition, and completion
//! follow. Rename and find-references are deferred to v1.1 per the roadmap.
//!
//! The server reuses the compiler front end directly (see `analysis`); the
//! diagnostic work happens in a synchronous call that never holds a lock or a
//! non-`Send` value across an `await`.

#![forbid(unsafe_code)]

mod analysis;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use analysis::{
    analyze, analyze_full, base_completions, find_symbol_span, outline_of, validate_rename_name,
    CompletionTag, Definition, LineIndex, OutlineKind, OutlineSymbol, RenameError, SymbolTarget,
};

struct Backend {
    client: Client,
    /// Open documents by URI (full text; the server uses FULL text sync).
    docs: Mutex<HashMap<Url, String>>,
    /// The workspace root, captured at `initialize` — the tree the workspace
    /// symbol index walks for `.glyph` files.
    root: Mutex<Option<PathBuf>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Backend {
            client,
            docs: Mutex::new(HashMap::new()),
            root: Mutex::new(None),
        }
    }

    /// Store the document text and publish its diagnostics. The diagnostics are
    /// computed and the lock released before the `await`, so no guard or
    /// non-`Send` value crosses the suspension point.
    async fn refresh(&self, uri: Url, text: String, version: Option<i32>) {
        let diagnostics = to_lsp_diagnostics(&text, analyze(&text));
        {
            let mut docs = self.docs.lock().expect("docs mutex");
            docs.insert(uri.clone(), text);
        }
        self.client
            .publish_diagnostics(uri, diagnostics, version)
            .await;
    }
}

/// Convert byte-coordinate diagnostics to the protocol type, mapping spans to
/// UTF-16 line/character ranges via a one-shot line index over `text`.
fn to_lsp_diagnostics(text: &str, glyphs: Vec<analysis::GlyphDiagnostic>) -> Vec<Diagnostic> {
    let index = LineIndex::new(text);
    let pos = |offset: u32| {
        let (line, character) = index.position(text, offset as usize);
        Position::new(line, character)
    };
    glyphs
        .into_iter()
        .map(|d| Diagnostic {
            range: Range::new(pos(d.start), pos(d.end)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(d.code)),
            source: Some("glyph".to_string()),
            message: d.message,
            ..Default::default()
        })
        .collect()
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture the workspace root for the symbol index: the first workspace
        // folder, else the (deprecated) rootUri.
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|fs| fs.first())
            .map(|f| f.uri.clone())
            .or(params.root_uri)
            .and_then(|uri| uri.to_file_path().ok());
        if let Some(root) = root {
            *self.root.lock().expect("root mutex") = Some(root);
        }
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "glyph-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions::default()),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "glyph-lsp ready")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.refresh(doc.uri, doc.text, Some(doc.version)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync: the final content change carries the whole document.
        if let Some(change) = params.content_changes.into_iter().next_back() {
            self.refresh(
                params.text_document.uri,
                change.text,
                Some(params.text_document.version),
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        {
            let mut docs = self.docs.lock().expect("docs mutex");
            docs.remove(&uri);
        }
        // Clear the squiggles for a closed file.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = {
            let docs = self.docs.lock().expect("docs mutex");
            docs.get(&uri).cloned()
        };
        let Some(text) = text else {
            return Ok(None);
        };
        // Mirror `glyph fmt`: never format unparseable source.
        let Ok(module) = glyph_parser::parse(&text) else {
            return Ok(None);
        };
        let comments = glyph_lexer::comments(&text);
        let formatted = glyph_formatter::format_module(&module, &comments, &text);
        if formatted == text {
            return Ok(Some(Vec::new()));
        }
        // Replace the whole document with the canonical layout.
        let index = LineIndex::new(&text);
        let (end_line, end_char) = index.position(&text, text.len());
        Ok(Some(vec![TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(end_line, end_char)),
            new_text: formatted,
        }]))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params;
        let Some(text) = self.doc_text(&pos.text_document.uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyze_full(&text) else {
            return Ok(None);
        };
        let index = LineIndex::new(&text);
        let offset = index.offset(&text, pos.position.line, pos.position.character);
        Ok(analysis.hover(offset).map(|ty| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```glyph\n{ty}\n```"),
            }),
            range: None,
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri;
        let Some(text) = self.doc_text(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyze_full(&text) else {
            return Ok(None);
        };
        let index = LineIndex::new(&text);
        let offset = index.offset(&text, pos.position.line, pos.position.character);
        let location = match analysis.definition(offset) {
            None => None,
            Some(Definition::Here(start, end)) => Some(location_in(&uri, &text, start, end)),
            Some(Definition::InModule { module_path, name }) => {
                self.resolve_cross_module(&module_path, &name)
            }
        };
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        // Use the parsed document's full candidate set; fall back to keywords +
        // prelude when the file is unknown or does not parse (mid-edit), which
        // is exactly when completion matters most.
        let completions = self
            .doc_text(&uri)
            .and_then(|text| analyze_full(&text).map(|a| a.completions()))
            .unwrap_or_else(base_completions);
        let items = completions
            .into_iter()
            .map(|c| CompletionItem {
                kind: Some(match c.tag {
                    CompletionTag::Keyword => CompletionItemKind::KEYWORD,
                    CompletionTag::Function => CompletionItemKind::FUNCTION,
                    CompletionTag::Type => CompletionItemKind::CLASS,
                    CompletionTag::Variant => CompletionItemKind::ENUM_MEMBER,
                    CompletionTag::Value => CompletionItemKind::VALUE,
                }),
                label: c.label,
                ..Default::default()
            })
            .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some(text) = self.doc_text(&params.text_document.uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyze_full(&text) else {
            return Ok(None);
        };
        let index = LineIndex::new(&text);
        let symbols = analysis
            .document_symbols()
            .iter()
            .map(|s| outline_to_document_symbol(&index, &text, s))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    #[allow(deprecated)] // `SymbolInformation` is the still-widely-supported response shape.
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let Some(root) = self.root.lock().expect("root mutex").clone() else {
            return Ok(None);
        };
        let query = params.query.to_lowercase();
        let mut files = Vec::new();
        collect_glyph_files(&root, &mut files);

        let mut out = Vec::new();
        for path in files {
            let Ok(uri) = Url::from_file_path(&path) else {
                continue;
            };
            // Prefer the open buffer (unsaved edits) over the on-disk text.
            let Some(text) = self
                .doc_text(&uri)
                .or_else(|| std::fs::read_to_string(&path).ok())
            else {
                continue;
            };
            let index = LineIndex::new(&text);
            for top in outline_of(&text) {
                push_workspace_symbol(&mut out, &query, &uri, &index, &text, &top, None);
                for child in &top.children {
                    push_workspace_symbol(
                        &mut out,
                        &query,
                        &uri,
                        &index,
                        &text,
                        child,
                        Some(top.name.clone()),
                    );
                }
            }
        }
        Ok(Some(out))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let pos = params.text_document_position;
        let uri = pos.text_document.uri;
        let Some(text) = self.doc_text(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyze_full(&text) else {
            return Ok(None);
        };
        let index = LineIndex::new(&text);
        let offset = index.offset(&text, pos.position.line, pos.position.character);
        let include_decl = params.context.include_declaration;

        // With a workspace, a module-level symbol's references span every file.
        if let Some((root, this_module)) = self.file_module(&uri) {
            if let Some(SymbolTarget::Global { module, name }) =
                analysis.symbol_target(offset, &text, &this_module)
            {
                let mut locations = Vec::new();
                for (u2, t2) in self.workspace_docs(&root) {
                    let Some(fm) = u2
                        .to_file_path()
                        .ok()
                        .and_then(|p| module_path_of(&root, &p))
                    else {
                        continue;
                    };
                    let Some(a2) = analyze_full(&t2) else {
                        continue;
                    };
                    for (s, e) in a2.global_occurrences(&fm, &module, &name, &t2, include_decl) {
                        locations.push(location_in(&u2, &t2, s, e));
                    }
                }
                return Ok((!locations.is_empty()).then_some(locations));
            }
        }
        // A local binding, or no workspace: the open document only.
        let spans = analysis.references(offset, &text, include_decl);
        Ok((!spans.is_empty()).then(|| {
            spans
                .into_iter()
                .map(|(s, e)| location_in(&uri, &text, s, e))
                .collect()
        }))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let pos = params.text_document_position;
        let uri = pos.text_document.uri;
        let Some(text) = self.doc_text(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyze_full(&text) else {
            return Ok(None);
        };
        let index = LineIndex::new(&text);
        let offset = index.offset(&text, pos.position.line, pos.position.character);

        // With a workspace, a module-level rename edits every file that names the
        // symbol — the declaration, its references, and each importing module's
        // import binding — so it is complete and safe.
        if let Some((root, this_module)) = self.file_module(&uri) {
            if let Some(SymbolTarget::Global { module, name }) =
                analysis.symbol_target(offset, &text, &this_module)
            {
                validate_rename_name(&params.new_name).map_err(rename_error)?;
                let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
                for (u2, t2) in self.workspace_docs(&root) {
                    let Some(fm) = u2
                        .to_file_path()
                        .ok()
                        .and_then(|p| module_path_of(&root, &p))
                    else {
                        continue;
                    };
                    let Some(a2) = analyze_full(&t2) else {
                        continue;
                    };
                    let idx2 = LineIndex::new(&t2);
                    let edits: Vec<TextEdit> = a2
                        .global_occurrences(&fm, &module, &name, &t2, true)
                        .into_iter()
                        .map(|(s, e)| text_edit(&idx2, &t2, s, e, &params.new_name))
                        .collect();
                    if !edits.is_empty() {
                        changes.insert(u2, edits);
                    }
                }
                return Ok(Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }));
            }
        }
        // A local binding (or no workspace, where a module-level rename is refused
        // because its cross-file references cannot be found): edit this file only.
        let spans = analysis
            .rename_edits(offset, &text, &params.new_name)
            .map_err(rename_error)?;
        let edits: Vec<TextEdit> = spans
            .into_iter()
            .map(|(s, e)| text_edit(&index, &text, s, e, &params.new_name))
            .collect();
        let mut changes = HashMap::new();
        changes.insert(uri, edits);
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Surface a refused rename to the editor as a JSON-RPC error, so the user sees
/// why (an illegal name, a keyword, or a not-yet-supported module-level rename)
/// rather than a silent no-op.
fn rename_error(e: RenameError) -> tower_lsp::jsonrpc::Error {
    let message = match e {
        RenameError::InvalidIdentifier => "not a valid Glyph identifier",
        RenameError::ReservedKeyword => "that name is a reserved Glyph keyword",
        RenameError::NoBinding => "there is no renameable symbol at the cursor",
        RenameError::ModuleLevelUnsupported => {
            "renaming a module-level declaration is not supported yet (it may be \
             referenced from other files); a local binding can be renamed"
        }
    };
    tower_lsp::jsonrpc::Error {
        code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
        message: message.into(),
        data: None,
    }
}

/// The LSP `SymbolKind` for an outline node.
fn outline_symbol_kind(kind: OutlineKind) -> SymbolKind {
    match kind {
        OutlineKind::Function => SymbolKind::FUNCTION,
        OutlineKind::Type => SymbolKind::STRUCT,
        OutlineKind::Constant => SymbolKind::CONSTANT,
        OutlineKind::Variant => SymbolKind::ENUM_MEMBER,
    }
}

/// Append a workspace symbol for `sym` if its name matches `query` (empty query
/// matches all). `container` is the enclosing type for a union variant.
#[allow(deprecated)]
fn push_workspace_symbol(
    out: &mut Vec<SymbolInformation>,
    query: &str,
    uri: &Url,
    index: &LineIndex,
    text: &str,
    sym: &OutlineSymbol,
    container: Option<String>,
) {
    if !query.is_empty() && !sym.name.to_lowercase().contains(query) {
        return;
    }
    let (sl, sc) = index.position(text, sym.span.0 as usize);
    let (el, ec) = index.position(text, sym.span.1 as usize);
    out.push(SymbolInformation {
        name: sym.name.clone(),
        kind: outline_symbol_kind(sym.kind),
        tags: None,
        deprecated: None,
        location: Location {
            uri: uri.clone(),
            range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
        },
        container_name: container,
    });
}

/// Collect every `.glyph` file under `dir`, skipping dot-directories and the
/// conventional `target/` build output. Errors (unreadable dirs) are ignored —
/// the index is best-effort.
fn collect_glyph_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_glyph_files(&path, out);
        } else if meta.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("glyph")
        {
            out.push(path);
        }
    }
}

/// The Glyph module path of a `.glyph` file under `root` — its path relative to
/// the root, minus the extension, with `/` separators (`src/foo.glyph` →
/// `src/foo`). This is the name an `import` uses and the key a cross-file
/// symbol is identified by. `None` when the file is not under the root.
fn module_path_of(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("/"))
}

/// Convert an outline node to the protocol `DocumentSymbol`, recursively. The
/// declaration's byte span supplies both the full range and the selection range
/// (the latter must be contained in the former, which an equal range satisfies).
#[allow(deprecated)] // `DocumentSymbol::deprecated` is a deprecated protocol field.
fn outline_to_document_symbol(index: &LineIndex, text: &str, s: &OutlineSymbol) -> DocumentSymbol {
    let (sl, sc) = index.position(text, s.span.0 as usize);
    let (el, ec) = index.position(text, s.span.1 as usize);
    let range = Range::new(Position::new(sl, sc), Position::new(el, ec));
    let kind = outline_symbol_kind(s.kind);
    let children: Vec<DocumentSymbol> = s
        .children
        .iter()
        .map(|c| outline_to_document_symbol(index, text, c))
        .collect();
    DocumentSymbol {
        name: s.name.clone(),
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

/// Response to the custom `glyph/canonicalView` request (Q32). Exactly one of
/// the two fields is set: `content` when the open document parsed (the canonical
/// agent view), `error` when it did not (the parse-error message) or the
/// document is not open.
#[derive(Debug, serde::Serialize)]
struct CanonicalViewResponse {
    content: Option<String>,
    error: Option<String>,
}

/// Parameters of the custom `glyph/applyEdit` request (Q29). A set of standard
/// LSP text edits to apply atomically to an open document, gated on the result
/// type-checking clean.
#[derive(Debug, serde::Deserialize)]
struct ApplyEditParams {
    uri: Url,
    edits: Vec<TextEdit>,
}

/// Response to `glyph/applyEdit`. On success `content` is the verified new
/// document text and `diagnostics` is empty; on rejection `content` is absent,
/// `rejected` names the reason, and `diagnostics` carries the errors the edit
/// would have introduced (empty for a structural rejection like overlapping
/// edits).
#[derive(Debug, serde::Serialize)]
struct ApplyEditResponse {
    ok: bool,
    content: Option<String>,
    rejected: Option<String>,
    diagnostics: Vec<Diagnostic>,
}

impl Backend {
    /// The current text of an open document, if any.
    fn doc_text(&self, uri: &Url) -> Option<String> {
        self.docs.lock().expect("docs mutex").get(uri).cloned()
    }

    /// The workspace root and the open file's own module path, when both exist
    /// (there is a workspace and the file lives under it). `None` falls back to
    /// file-scoped behaviour.
    fn file_module(&self, uri: &Url) -> Option<(PathBuf, String)> {
        let root = self.root.lock().expect("root mutex").clone()?;
        let path = uri.to_file_path().ok()?;
        let module = module_path_of(&root, &path)?;
        Some((root, module))
    }

    /// Every `.glyph` document in the workspace as `(uri, text)`, preferring an
    /// open buffer (unsaved edits) over the on-disk text, and including any open
    /// document not on disk (a new, unsaved file). The per-file input to a
    /// workspace-wide references/rename.
    fn workspace_docs(&self, root: &Path) -> Vec<(Url, String)> {
        let mut files = Vec::new();
        collect_glyph_files(root, &mut files);
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for path in files {
            let Ok(uri) = Url::from_file_path(&path) else {
                continue;
            };
            let Some(text) = self
                .doc_text(&uri)
                .or_else(|| std::fs::read_to_string(&path).ok())
            else {
                continue;
            };
            seen.insert(uri.clone());
            out.push((uri, text));
        }
        for (uri, text) in self.docs.lock().expect("docs mutex").iter() {
            if !seen.contains(uri) {
                out.push((uri.clone(), text.clone()));
            }
        }
        out
    }

    /// Custom request `glyph/canonicalView`: return the canonical agent view
    /// (Q32) of an open document — the `glyph fmt` layout with stable `Lddd`
    /// line numbers and per-declaration content fingerprints. An agent reads
    /// this instead of the raw buffer to get position- and reformat-stable
    /// references. The view is computed by the same pure function the `glyph
    /// canonical` CLI uses.
    async fn canonical_view_request(
        &self,
        params: TextDocumentIdentifier,
    ) -> Result<CanonicalViewResponse> {
        let Some(text) = self.doc_text(&params.uri) else {
            return Ok(CanonicalViewResponse {
                content: None,
                error: Some("document not open".to_string()),
            });
        };
        Ok(match glyph_formatter::canonical_view(&text) {
            Ok(view) => CanonicalViewResponse {
                content: Some(view),
                error: None,
            },
            Err(e) => CanonicalViewResponse {
                content: None,
                error: Some(e.to_string()),
            },
        })
    }

    /// Custom request `glyph/applyEdit` (Q29): apply structured text edits to an
    /// open document and accept them only if the result type-checks clean. This
    /// is the TS-family reconciliation of the abandoned `edit { … } @verify { … }`
    /// source syntax: the edit is plain LSP `TextEdit`s and the verification is
    /// the compiler's own front-end gate (parse → resolve → typecheck), not a
    /// new language construct. A successful call returns the verified new text
    /// (the caller applies it and syncs via the normal `didChange`); a rejected
    /// call changes nothing and returns the errors the edit would have caused,
    /// making "the agent broke the file" a rejection rather than a saved edit.
    ///
    /// v1 gate: the result must have *no* errors (a crisp "lands a clean
    /// change" guarantee). Running the `@example`/property tests as part of the
    /// gate is a v1.1 enhancement (it needs the build pipeline factored into a
    /// library the server can call without the current cli→lsp dependency cycle).
    async fn apply_edit_request(&self, params: ApplyEditParams) -> Result<ApplyEditResponse> {
        let Some(text) = self.doc_text(&params.uri) else {
            return Ok(reject("document_not_open", Vec::new()));
        };
        let candidate = match apply_text_edits(&text, &params.edits) {
            Ok(c) => c,
            Err(reason) => return Ok(reject(&reason, Vec::new())),
        };
        let diags = analyze(&candidate);
        if diags.is_empty() {
            Ok(ApplyEditResponse {
                ok: true,
                content: Some(candidate),
                rejected: None,
                diagnostics: Vec::new(),
            })
        } else {
            let lsp = to_lsp_diagnostics(&candidate, diags);
            Ok(reject("verification_failed", lsp))
        }
    }

    /// Resolve an imported `module_path` to its file under the workspace root and
    /// locate the declaration named `name` in it (a `.glyph` whose path mirrors
    /// the module path, e.g. `sub/b` → `<root>/sub/b.glyph`). `None` if there is
    /// no workspace root, no such file (a `std/*` import has no project source),
    /// or the declaration is not found.
    fn resolve_cross_module(&self, module_path: &str, name: &str) -> Option<Location> {
        let root = self.root.lock().expect("root mutex").clone()?;
        let file = root.join(module_path).with_extension("glyph");
        let uri = Url::from_file_path(&file).ok()?;
        let text = self
            .doc_text(&uri)
            .or_else(|| std::fs::read_to_string(&file).ok())?;
        let (start, end) = find_symbol_span(&outline_of(&text), name)?;
        Some(location_in(&uri, &text, start, end))
    }
}

/// A rejected `glyph/applyEdit` response carrying a reason and any diagnostics.
fn reject(reason: &str, diagnostics: Vec<Diagnostic>) -> ApplyEditResponse {
    ApplyEditResponse {
        ok: false,
        content: None,
        rejected: Some(reason.to_string()),
        diagnostics,
    }
}

/// Apply LSP `TextEdit`s to `text`, producing the candidate document. Edits are
/// resolved to byte offsets, checked for overlap, and spliced from the end
/// backwards so earlier offsets stay valid. Returns `Err(reason)` for an
/// out-of-bounds range or overlapping edits (the edit set is then applied
/// atomically: all or nothing).
fn apply_text_edits(text: &str, edits: &[TextEdit]) -> std::result::Result<String, String> {
    let index = LineIndex::new(text);
    // The highest valid line index. `LineIndex::offset` silently clamps an
    // out-of-range line to EOF, which would mis-apply an edit instead of failing;
    // reject such ranges explicitly so a bogus position is a rejection, not a
    // corrupt splice.
    let max_line = text.bytes().filter(|&b| b == b'\n').count();
    let mut spans: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for e in edits {
        if e.range.start.line as usize > max_line || e.range.end.line as usize > max_line {
            return Err("edit_out_of_bounds".to_string());
        }
        let start = index.offset(text, e.range.start.line, e.range.start.character);
        let end = index.offset(text, e.range.end.line, e.range.end.character);
        if start > end || end > text.len() {
            return Err("edit_out_of_bounds".to_string());
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err("edit_not_on_char_boundary".to_string());
        }
        spans.push((start, end, e.new_text.as_str()));
    }
    spans.sort_by_key(|s| s.0);
    for pair in spans.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("overlapping_edits".to_string());
        }
    }
    let mut out = text.to_string();
    for (start, end, new_text) in spans.into_iter().rev() {
        out.replace_range(start..end, new_text);
    }
    Ok(out)
}

/// A `Location` for byte range `[start, end)` in `text` at `uri`.
fn location_in(uri: &Url, text: &str, start: u32, end: u32) -> Location {
    let index = LineIndex::new(text);
    let (sl, sc) = index.position(text, start as usize);
    let (el, ec) = index.position(text, end as usize);
    Location {
        uri: uri.clone(),
        range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
    }
}

/// A `TextEdit` replacing byte range `[start, end)` of `text` with `new_text`.
fn text_edit(index: &LineIndex, text: &str, start: u32, end: u32, new_text: &str) -> TextEdit {
    let (sl, sc) = index.position(text, start as usize);
    let (el, ec) = index.position(text, end as usize);
    TextEdit {
        range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
        new_text: new_text.to_string(),
    }
}

/// Run the language server over stdio (the transport an editor extension
/// spawns: `glyph lsp`). Blocks until the client closes the connection.
pub fn run_stdio() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::build(Backend::new)
            .custom_method("glyph/canonicalView", Backend::canonical_view_request)
            .custom_method("glyph/applyEdit", Backend::apply_edit_request)
            .finish();
        Server::new(stdin, stdout, socket).serve(service).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_path_of_maps_files_under_root() {
        let root = Path::new("/proj");
        assert_eq!(
            module_path_of(root, Path::new("/proj/foo.glyph")).as_deref(),
            Some("foo")
        );
        assert_eq!(
            module_path_of(root, Path::new("/proj/a/b.glyph")).as_deref(),
            Some("a/b")
        );
        // Outside the root: no module path.
        assert_eq!(module_path_of(root, Path::new("/other/x.glyph")), None);
    }

    fn edit(sl: u32, sc: u32, el: u32, ec: u32, new_text: &str) -> TextEdit {
        TextEdit {
            range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
            new_text: new_text.to_string(),
        }
    }

    #[test]
    fn applies_a_single_edit() {
        let text = "fn a() -> number {\n  return 1\n}\n";
        let out = apply_text_edits(text, &[edit(1, 9, 1, 10, "2")]).unwrap();
        assert_eq!(out, "fn a() -> number {\n  return 2\n}\n");
    }

    #[test]
    fn applies_multiple_edits_back_to_front() {
        let text = "ab\ncd\n";
        // Replace `a`→`X` (0,0-0,1) and `d`→`Y` (1,1-1,2) in one call.
        let out = apply_text_edits(text, &[edit(0, 0, 0, 1, "X"), edit(1, 1, 1, 2, "Y")]).unwrap();
        assert_eq!(out, "Xb\ncY\n");
    }

    #[test]
    fn rejects_overlapping_edits() {
        let text = "abcdef";
        let err = apply_text_edits(text, &[edit(0, 0, 0, 3, "X"), edit(0, 2, 0, 5, "Y")]).unwrap_err();
        assert_eq!(err, "overlapping_edits");
    }

    #[test]
    fn rejects_out_of_bounds_edit() {
        let text = "abc";
        let err = apply_text_edits(text, &[edit(9, 0, 9, 1, "X")]).unwrap_err();
        assert_eq!(err, "edit_out_of_bounds");
    }

    #[test]
    fn an_edit_that_typechecks_clean_has_no_diagnostics() {
        // The gate the RPC applies: a clean result yields no diagnostics.
        let candidate = apply_text_edits(
            "fn a() -> number {\n  return 1\n}\n",
            &[edit(1, 9, 1, 10, "42")],
        )
        .unwrap();
        assert!(analyze(&candidate).is_empty());
    }

    #[test]
    fn an_edit_that_breaks_types_produces_diagnostics() {
        // Replacing the numeric body with a string must fail the gate.
        let candidate = apply_text_edits(
            "fn a() -> number {\n  return 1\n}\n",
            &[edit(1, 9, 1, 10, "\"oops\"")],
        )
        .unwrap();
        assert!(!analyze(&candidate).is_empty());
    }
}
