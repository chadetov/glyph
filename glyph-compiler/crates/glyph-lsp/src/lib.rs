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
    analyze, analyze_full, base_completions, find_symbol_span, outline_of, CompletionTag,
    Definition, LineIndex, OutlineKind, OutlineSymbol,
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

    async fn shutdown(&self) -> Result<()> {
        Ok(())
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

impl Backend {
    /// The current text of an open document, if any.
    fn doc_text(&self, uri: &Url) -> Option<String> {
        self.docs.lock().expect("docs mutex").get(uri).cloned()
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
            .finish();
        Server::new(stdin, stdout, socket).serve(service).await;
    });
}
