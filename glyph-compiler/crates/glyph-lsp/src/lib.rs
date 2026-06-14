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
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use analysis::{analyze, analyze_full, LineIndex};

struct Backend {
    client: Client,
    /// Open documents by URI (full text; the server uses FULL text sync).
    docs: Mutex<HashMap<Url, String>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Backend {
            client,
            docs: Mutex::new(HashMap::new()),
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
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
        Ok(analysis.definition(offset).map(|(start, end)| {
            let (sl, sc) = index.position(&text, start as usize);
            let (el, ec) = index.position(&text, end as usize);
            GotoDefinitionResponse::Scalar(Location {
                uri,
                range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
            })
        }))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

impl Backend {
    /// The current text of an open document, if any.
    fn doc_text(&self, uri: &Url) -> Option<String> {
        self.docs.lock().expect("docs mutex").get(uri).cloned()
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
        let (service, socket) = LspService::new(Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
}
