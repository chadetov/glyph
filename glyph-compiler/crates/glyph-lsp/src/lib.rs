//! Glyph's language services.
//!
//! Two servers, both over stdio, sharing one pure analysis layer (see
//! `analysis`, which holds no protocol types): the **language server** (`run_stdio`)
//! for editors — diagnostics, hover, go-to-definition, completion, document and
//! workspace symbols, workspace-wide find-references and rename, and formatting;
//! and the **MCP server** (`run_mcp_stdio`, see `mcp`) exposing the same queries
//! to a coding agent as tools. Reusing the analysis layer keeps the two surfaces
//! a thin adapter apiece rather than a second implementation of the semantics.
//!
//! What they do not share is a database. The editor's truth is the buffer it
//! last sent and the agent's is what is on disk, so each server holds its own
//! (see `overlay` for the editor's, `mcp` for the agent's). They are peers over
//! one compiler model, not one built on the other.
//!
//! The language server's diagnostic work happens in a synchronous call that
//! never holds a lock or a non-`Send` value across an `await`.

#![forbid(unsafe_code)]

mod analysis;
mod mcp;
mod overlay;

/// The structured diagnostic `glyph check --json` and the `glyph_diagnostics`
/// MCP tool both answer with, and the per-file assembly the MCP tool runs
/// against a project database. Public because `glyph-cli` serializes this same
/// type; it lives here because `glyph-cli` depends on this crate and not the
/// other way round.
pub mod diagnostic;

/// The span-containment walk that decides which top-level declaration a byte
/// offset belongs to. Re-exported because `glyph-cli` attributes a `--json`
/// diagnostic by the same rule, and two copies of it drifted apart once
/// already (G180).
pub use analysis::enclosing_decl_name;

/// The generation record `glyph gen` writes into every file it generates, and
/// the reader that turns it back into `GENERATED_FROM` edges. Re-exported for
/// the same reason as `enclosing_decl_name`: `glyph-cli` writes the format and
/// this crate reads it, and a format with two implementations is two formats
/// the first time one of them changes.
pub use mcp::generated;
/// The tool catalogue, its full text and the server's `instructions`, so
/// `glyph llms --json` publishes the object the server serves rather than a
/// second description of it.
pub use mcp::{instructions, tool_manual, tool_specs};

/// Run the Model Context Protocol server over stdio, exposing Glyph's language
/// analysis to a coding agent as tools (see `mcp`). `root` is the project the
/// workspace queries operate over.
pub fn run_mcp_stdio(root: std::path::PathBuf) {
    mcp::run_stdio(root);
}

/// Answer one MCP tool call and return its JSON, without a transport.
///
/// The CLI's `glyph query` verbs route through here, so a tool asked from the
/// command line and the same tool asked over stdio run the same code and
/// return the same bytes. `Err` is the refusal an MCP client would receive as
/// an `isError` result.
pub fn call_mcp_tool(
    root: std::path::PathBuf,
    tool: &str,
    arguments: serde_json::Value,
) -> std::result::Result<String, String> {
    mcp::call_tool_once(root, tool, arguments)
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use analysis::{
    base_completions, find_symbol_span, validate_rename_name, CompletionTag, Definition,
    LineIndex, OutlineKind, OutlineSymbol, RenameError, SymbolTarget,
};
use diagnostic::Stages;
use overlay::Workspace;

/// How long typing has to pause before the emitter's refusals are published.
///
/// The front end's diagnostics go out on every keystroke; the full list, which
/// adds the emitter's E0302 to E0305, waits for this much quiet (G295). The
/// emitter is the one stage whose cost on a large file is the same order as
/// the whole front end's: on the 2,205-line `minilang/main.glyph`, in-process
/// in the dev profile, the front end took 19.3 ms per keystroke and the emitter
/// 14.9 ms more. Running it once per pause rather than once per keystroke keeps
/// the squiggle for a type error as fast as it was before the emitter was
/// published at all.
///
/// 300 ms because it has to sit above the gap between two keystrokes of
/// someone typing a word, which is under 200 ms for most typists, or a burst
/// would run the emitter after every character anyway; and it has to stay
/// short enough that the refusal is there by the time the eye comes back to
/// the line. The emitter's own run is an order of magnitude shorter than the
/// wait, so the wait is what a person sees. A save and an open do not wait.
const EMIT_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

/// What the server knows about one open document's diagnostics.
struct DocState {
    /// Bumped by every change, open, save and close, while the workspace lock
    /// is held, so it names one buffer. A debounced emitter run carries the
    /// generation it was started for and publishes only while that is still
    /// the current one: a stale result never overwrites fresher diagnostics.
    ///
    /// A counter rather than the editor's document version, because a save
    /// carries no version and still has to supersede a pending run. Drawn
    /// from one counter for the whole session, so a document closed and
    /// opened again never reuses a generation a run from before the close
    /// still carries.
    generation: u64,
    /// The editor's version of the buffer, published with the diagnostics.
    version: Option<i32>,
    /// The emitter's part of the last full list, in the coordinates of the
    /// current buffer: `full[front.len()..]`, which is exact because the
    /// front-end list is a prefix of the full one (`diagnostic::Stages`).
    ///
    /// Set by every full run and mapped through every keystroke's edit
    /// ([`carry_through_edit`]), so a keystroke's front-end publish can carry
    /// the emitter's refusals forward instead of dropping them until typing
    /// pauses. The next full run replaces them (review 78, item 8a).
    carried: Vec<Diagnostic>,
    /// The debounced emitter run waiting for typing to pause, cancelled by
    /// the next bump (review 78, item 8c). The generation check is what keeps
    /// a stale run from publishing; cancelling only spares the wake-up.
    pending: Option<tokio::task::AbortHandle>,
}

/// A unit test's record of what was published: each publish as its URI, its
/// version and its diagnostics' codes.
#[cfg(test)]
type PublishLog = Arc<Mutex<Vec<(Url, Option<i32>, Vec<String>)>>>;

/// Where published diagnostics go: the editor, or in a unit test a list.
#[derive(Clone)]
enum Sink {
    Client(Client),
    #[cfg(test)]
    Record(PublishLog),
}

/// The state a debounced emitter run needs after the request that started it
/// has returned. Every field is shared with the [`Backend`] that owns it.
#[derive(Clone)]
struct Publisher {
    sink: Sink,
    ws: Arc<Mutex<Workspace>>,
    docs: Arc<Mutex<HashMap<Url, DocState>>>,
    /// The editor's workspace folder, shared with the [`Backend`], which sets
    /// it at `initialize`.
    workspace: Arc<Mutex<Option<PathBuf>>>,
    /// The last generation handed out, for any document.
    generations: Arc<std::sync::atomic::AtomicU64>,
    /// Held across a check of the generation and the publish that follows it,
    /// so two publishes for one document cannot pass each other on the way
    /// out. Without it a run that checked as current could be overtaken by a
    /// newer keystroke's publish and land after it.
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl Publisher {
    /// The current generation of `uri`, or `None` when it is not open.
    fn current(&self, uri: &Url) -> Option<u64> {
        self.docs.lock().expect("docs mutex").get(uri).map(|d| d.generation)
    }

    /// Start a new generation for `uri`. `version` replaces the stored one when
    /// the event carried a version; a save does not, and keeps the last.
    ///
    /// Called with the workspace lock held, which is what makes a generation
    /// name one buffer.
    fn bump(&self, uri: &Url, version: Option<i32>) -> u64 {
        let mut docs = self.docs.lock().expect("docs mutex");
        let state = docs.entry(uri.clone()).or_insert(DocState {
            generation: 0,
            version: None,
            carried: Vec::new(),
            pending: None,
        });
        state.generation = self
            .generations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if let Some(run) = state.pending.take() {
            run.abort();
        }
        if version.is_some() {
            state.version = version;
        }
        state.generation
    }

    /// Forget a closed document, cancelling its pending run, and return its
    /// last version. `None` when it was not open (review 78, item 8b).
    ///
    /// Called with the workspace lock held. A run that was already past its
    /// generation check finds no document at its publish and drops itself.
    fn forget(&self, uri: &Url) -> Option<Option<i32>> {
        let state = self.docs.lock().expect("docs mutex").remove(uri)?;
        if let Some(run) = state.pending {
            run.abort();
        }
        Some(state.version)
    }

    /// Clear a closed document's diagnostics, unless it was opened again
    /// before this got through the gate, whose publish this would wipe.
    async fn clear(&self, uri: Url, version: Option<i32>) {
        let _gate = self.gate.lock().await;
        if self.docs.lock().expect("docs mutex").contains_key(&uri) {
            return;
        }
        self.send(uri, version, Vec::new()).await;
    }

    /// The diagnostics for `uri` through `stages`, as the protocol type.
    fn compute(&self, ws: &mut Workspace, root: &Path, uri: &Url, stages: Stages) -> Vec<Diagnostic> {
        match ws.diagnostics(root, uri, stages) {
            Some((text, diagnostics)) => to_lsp_diagnostics(text, diagnostics),
            None => Vec::new(),
        }
    }

    /// The full list for `uri`, emitter included, and the emitter's part of
    /// it, which is everything after the front end's list.
    fn compute_full(
        &self,
        ws: &mut Workspace,
        root: &Path,
        uri: &Url,
    ) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
        let front = self.compute(ws, root, uri, Stages::FrontEnd).len();
        let full = self.compute(ws, root, uri, Stages::All);
        let emitter = full[front.min(full.len())..].to_vec();
        (full, emitter)
    }

    /// Record `carried` as `uri`'s emitter diagnostics, if `generation` is
    /// still its current one. Called with the workspace lock held, so the
    /// buffer they are in coordinates of is the one that generation names.
    fn set_carried(&self, uri: &Url, generation: u64, carried: Vec<Diagnostic>) {
        let mut docs = self.docs.lock().expect("docs mutex");
        if let Some(d) = docs.get_mut(uri).filter(|d| d.generation == generation) {
            d.carried = carried;
        }
    }

    /// Map `uri`'s carried emitter diagnostics from `old` to `new` and return
    /// them, or clear them when `front` has an error: the emitter does not run
    /// on a file with one, so its part of the full list is known to be empty.
    /// Called with the workspace lock held, after the bump for `new`.
    fn carry(&self, uri: &Url, old: Option<&str>, new: &str, front: &[Diagnostic]) -> Vec<Diagnostic> {
        let mut docs = self.docs.lock().expect("docs mutex");
        let Some(d) = docs.get_mut(uri) else {
            return Vec::new();
        };
        let has_error = front
            .iter()
            .any(|f| f.severity == Some(DiagnosticSeverity::ERROR));
        d.carried = match old {
            Some(old) if !has_error => carry_through_edit(old, new, &d.carried),
            _ => Vec::new(),
        };
        d.carried.clone()
    }

    /// Publish `diagnostics` for `uri` if `generation` is still its current
    /// one, and drop them otherwise.
    async fn publish(&self, uri: Url, generation: u64, diagnostics: Vec<Diagnostic>) {
        self.publish_if(uri, generation, diagnostics, None).await;
    }

    /// [`Publisher::publish`], also dropped when `cause`, the document whose
    /// change the list was computed for, has moved on since: that change's
    /// own run republishes it.
    async fn publish_if(
        &self,
        uri: Url,
        generation: u64,
        diagnostics: Vec<Diagnostic>,
        cause: Option<&(Url, u64)>,
    ) {
        let _gate = self.gate.lock().await;
        let version = {
            let docs = self.docs.lock().expect("docs mutex");
            if let Some((cause, at)) = cause {
                if docs.get(cause).map(|d| d.generation) != Some(*at) {
                    return;
                }
            }
            match docs.get(&uri) {
                Some(d) if d.generation == generation => d.version,
                _ => return,
            }
        };
        self.send(uri, version, diagnostics).await;
    }

    /// The full list of every open document but `except`, each with its
    /// current generation (review 78, item 8d).
    ///
    /// A document's diagnostics depend on its siblings: an import names their
    /// exports, and E0304 is the declaring module's verdict (G210). So a
    /// change to one document, its buffer or its file on disk, can move
    /// another's list without touching it. Every full publish of a document
    /// is followed by this, and the other documents' generations are read,
    /// not bumped: an edit to one of them still supersedes the list computed
    /// here, and its own carried diagnostics are replaced along with the
    /// list. Called with the workspace lock held; `workspace` is the editor's
    /// workspace folder, read before it.
    fn others_full(
        &self,
        ws: &mut Workspace,
        workspace: Option<&Path>,
        except: Option<&Url>,
    ) -> Vec<(Url, u64, Vec<Diagnostic>)> {
        let open: Vec<(Url, u64)> = self
            .docs
            .lock()
            .expect("docs mutex")
            .iter()
            .filter(|(u, _)| Some(*u) != except)
            .map(|(u, d)| (u.clone(), d.generation))
            .collect();
        open.into_iter()
            .map(|(uri, generation)| {
                let root = analysis_root_in(workspace, &uri);
                let (full, emitter) = self.compute_full(ws, &root, &uri);
                self.set_carried(&uri, generation, emitter);
                (uri, generation, full)
            })
            .collect()
    }

    /// Publish what [`Publisher::others_full`] computed.
    async fn publish_others(&self, others: Vec<(Url, u64, Vec<Diagnostic>)>, cause: Option<(Url, u64)>) {
        for (uri, generation, diagnostics) in others {
            self.publish_if(uri, generation, diagnostics, cause.as_ref()).await;
        }
    }

    /// Hand one publish to the sink. Callers hold the gate.
    async fn send(&self, uri: Url, version: Option<i32>, diagnostics: Vec<Diagnostic>) {
        match &self.sink {
            Sink::Client(client) => client.publish_diagnostics(uri, diagnostics, version).await,
            #[cfg(test)]
            Sink::Record(log) => {
                let codes = diagnostics
                    .iter()
                    .map(|d| match &d.code {
                        Some(NumberOrString::String(c)) => c.clone(),
                        _ => String::new(),
                    })
                    .collect();
                log.lock().expect("log mutex").push((uri, version, codes));
            }
        }
    }

    /// Run the emitter for `uri` once typing has paused for [`EMIT_DEBOUNCE`],
    /// unless a newer change, save or close superseded `generation` first.
    fn schedule_full(&self, root: PathBuf, uri: Url, generation: u64) {
        let this = self.clone();
        let task_uri = uri.clone();
        let run = tokio::spawn(async move {
            let uri = task_uri;
            tokio::time::sleep(EMIT_DEBOUNCE).await;
            let workspace = this.workspace.lock().expect("root mutex").clone();
            let (diagnostics, others) = {
                let mut ws = this.ws.lock().expect("workspace mutex");
                // Checked under the workspace lock, where generations are
                // bumped, so a run that passes here is computing the buffer
                // it was started for.
                if this.current(&uri) != Some(generation) {
                    return;
                }
                let (full, emitter) = this.compute_full(&mut ws, &root, &uri);
                this.set_carried(&uri, generation, emitter);
                let others = this.others_full(&mut ws, workspace.as_deref(), Some(&uri));
                (full, others)
            };
            this.publish(uri.clone(), generation, diagnostics).await;
            this.publish_others(others, Some((uri, generation))).await;
        });
        // Held so the next bump can cancel it. A bump that got in between the
        // spawn and here has already superseded it.
        let mut docs = self.docs.lock().expect("docs mutex");
        match docs.get_mut(&uri).filter(|d| d.generation == generation) {
            Some(d) => d.pending = Some(run.abort_handle()),
            None => run.abort(),
        }
    }
}

struct Backend {
    client: Client,
    /// The editor's open buffers and the incremental model they feed (see
    /// `overlay`). Every analysis the server does reads through this, so the
    /// answer is always about the text the editor currently has.
    ws: Arc<Mutex<Workspace>>,
    /// The workspace root, captured at `initialize` — the tree the workspace
    /// symbol index walks for `.glyph` files.
    root: Arc<Mutex<Option<PathBuf>>>,
    /// Publishes diagnostics, now or after typing pauses.
    publisher: Publisher,
}

impl Backend {
    fn new(client: Client) -> Self {
        let ws = Arc::new(Mutex::new(Workspace::new()));
        let root = Arc::new(Mutex::new(None));
        let publisher = Publisher {
            workspace: Arc::clone(&root),
            sink: Sink::Client(client.clone()),
            ws: Arc::clone(&ws),
            docs: Arc::new(Mutex::new(HashMap::new())),
            generations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            gate: Arc::new(tokio::sync::Mutex::new(())),
        };
        Backend {
            client,
            ws,
            root,
            publisher,
        }
    }

    /// Store the document text and publish its diagnostics, in two phases.
    ///
    /// The buffer is written before anything is analysed, so a request racing
    /// this one cannot be answered from the text the change replaced. The
    /// diagnostics are computed and the lock released before the `await`, so no
    /// guard or non-`Send` value crosses the suspension point.
    ///
    /// `rescan` re-reads the project from disk first. It is set for a document
    /// being opened and clear for a keystroke: the walk and the reads are the
    /// one cost that scales with the project rather than with the edit, and
    /// nothing about typing can have changed a file the editor did not send.
    ///
    /// `full_now` publishes the whole list, emitter included, straight away:
    /// an open. A keystroke publishes the front end at once and leaves the
    /// emitter to a run [`EMIT_DEBOUNCE`] later that the next keystroke
    /// supersedes (G295).
    async fn refresh(
        &self,
        uri: Url,
        text: String,
        version: Option<i32>,
        rescan: bool,
        full_now: bool,
    ) {
        let root = self.analysis_root(&uri);
        let (generation, diagnostics) = {
            let mut ws = self.ws.lock().expect("workspace mutex");
            let old = ws.buffer(&uri);
            ws.set_buffer(&uri, text.clone());
            if rescan {
                ws.rescan(&root);
            }
            let generation = self.publisher.bump(&uri, version);
            let diagnostics = if full_now {
                let (full, emitter) = self.publisher.compute_full(&mut ws, &root, &uri);
                self.publisher.set_carried(&uri, generation, emitter);
                full
            } else {
                // The front end now, and the emitter's last refusals carried
                // through the edit until the debounced run replaces them.
                let mut front = self.publisher.compute(&mut ws, &root, &uri, Stages::FrontEnd);
                let carried = self.publisher.carry(&uri, old.as_deref(), &text, &front);
                front.extend(carried);
                front
            };
            (generation, diagnostics)
        };
        self.publisher
            .publish(uri.clone(), generation, diagnostics)
            .await;
        if full_now {
            self.republish_others(Some(&uri), Some((uri.clone(), generation))).await;
        } else {
            self.publisher.schedule_full(root, uri, generation);
        }
    }

    /// The project a document is analysed inside: its marked project, else its
    /// own directory (D41/G180).
    ///
    /// This is the root `glyph query hover` counts the same file's imports
    /// from, and the two surfaces answering one question alike is the whole
    /// point of asking inside a project at all (G219). It is deliberately not
    /// `project_root_for`, whose workspace-folder fallback is about which
    /// files a workspace-wide rename ranges over rather than about where a
    /// module's imports resolve.
    ///
    /// A document that is not a file on disk (an unsaved `untitled:` buffer)
    /// has no project. Its root is the empty path, which walks nothing, so it
    /// is analysed on its own exactly as it was before.
    fn analysis_root(&self, uri: &Url) -> PathBuf {
        let workspace = self.root.lock().expect("root mutex").clone();
        analysis_root_in(workspace.as_deref(), uri)
    }

    /// Publish the full list of every open document but `except`, after a
    /// change that can have moved them (see [`Publisher::others_full`]).
    async fn republish_others(&self, except: Option<&Url>, cause: Option<(Url, u64)>) {
        let workspace = self.root.lock().expect("root mutex").clone();
        let others = {
            let mut ws = self.ws.lock().expect("workspace mutex");
            self.publisher.others_full(&mut ws, workspace.as_deref(), except)
        };
        self.publisher.publish_others(others, cause).await;
    }
}

/// [`Backend::analysis_root`] for the editor's workspace folder `workspace`.
fn analysis_root_in(workspace: Option<&Path>, uri: &Url) -> PathBuf {
    let Ok(path) = uri.to_file_path() else {
        return PathBuf::new();
    };
    let workspace = workspace.map(Path::to_path_buf).unwrap_or_else(|| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    });
    module_root_for(&path, &workspace)
}

/// The region one change replaced: `old[start..old_end]` became
/// `new[start..new_end]`. The common prefix and the common suffix of the two
/// texts, trimmed to character boundaries, which with FULL sync is the one
/// region a change event can be read as. `None` when the texts are equal.
fn edit_region(old: &str, new: &str) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(start) {
        start -= 1;
    }
    let room = a.len().min(b.len()) - start;
    let mut suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    while !old.is_char_boundary(a.len() - suffix) {
        suffix -= 1;
    }
    Some((start, a.len() - suffix, b.len() - suffix))
}

/// Map diagnostics in `old`'s coordinates into `new`'s, through the one edit
/// between them (review 78, item 8a).
///
/// A diagnostic wholly before the edited region keeps its position, one
/// wholly after it moves by the edit's delta, and one whose range touches or
/// overlaps the region is dropped: the text it was about has changed, and a
/// diagnostic shown at a place it is no longer true of is a false statement.
/// Touching counts, so typing right against a refused expression drops its
/// refusal too.
fn carry_through_edit(old: &str, new: &str, carried: &[Diagnostic]) -> Vec<Diagnostic> {
    let Some((start, old_end, new_end)) = edit_region(old, new) else {
        return carried.to_vec();
    };
    let (old_index, new_index) = (LineIndex::new(old), LineIndex::new(new));
    let at = |p: Position| old_index.offset(old, p.line, p.character);
    let moved = |offset: usize| {
        let (line, character) = new_index.position(new, offset - old_end + new_end);
        Position::new(line, character)
    };
    carried
        .iter()
        .filter_map(|d| {
            let (s, e) = (at(d.range.start), at(d.range.end));
            if e < start {
                Some(d.clone())
            } else if s > old_end {
                Some(Diagnostic {
                    range: Range::new(moved(s), moved(e)),
                    ..d.clone()
                })
            } else {
                None
            }
        })
        .collect()
}

/// Convert the compiler's diagnostics to the protocol type, mapping each span's
/// byte offsets to UTF-16 line/character ranges via a one-shot line index over
/// `text`.
///
/// The message carries the help on a second line, because the protocol has no
/// field for it and an editor shows the message whole. A warning (the lint
/// tier) is published as a warning, which is what `glyph check` calls it.
fn to_lsp_diagnostics(text: &str, diagnostics: Vec<diagnostic::Diagnostic>) -> Vec<Diagnostic> {
    let index = LineIndex::new(text);
    let pos = |offset: u32| {
        let (line, character) = index.position(text, offset as usize);
        Position::new(line, character)
    };
    diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            range: Range::new(pos(d.range.start.offset), pos(d.range.end.offset)),
            severity: Some(if d.severity == "warning" {
                DiagnosticSeverity::WARNING
            } else {
                DiagnosticSeverity::ERROR
            }),
            code: Some(NumberOrString::String(d.code)),
            source: Some("glyph".to_string()),
            message: match d.help {
                Some(help) => format!("{}\n{help}", d.message),
                None => d.message,
            },
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
                // Full sync, plus the save notification. The save is not for
                // the file being saved, whose buffer the server already has;
                // it is one of the two moments the project is re-read from
                // disk, because a formatter or a generator may have written a
                // sibling alongside it.
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions::default()),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Ask to hear about `.glyph` files changed by something that is not
        // this editor: a branch switch, a generator, a second editor. Without
        // it the server's view of the files it did not open goes stale until
        // the next open or save. A client with no dynamic registration refuses
        // this, which is why the failure is logged and not acted on.
        let watcher = Registration {
            id: "glyph-watched-files".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![FileSystemWatcher {
                    glob_pattern: GlobPattern::String("**/*.glyph".to_string()),
                    kind: None,
                }],
            })
            .ok(),
        };
        if self.client.register_capability(vec![watcher]).await.is_err() {
            self.client
                .log_message(
                    MessageType::INFO,
                    "glyph-lsp: no file watching (the client does not register capabilities); \
                     the project is re-read on open and on save",
                )
                .await;
        }
        self.client
            .log_message(MessageType::INFO, "glyph-lsp ready")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        // Opening a document is when the project is (re-)read from disk: it is
        // the moment a file can have changed behind the editor's back, and it
        // is not on the typing path.
        self.refresh(doc.uri, doc.text, Some(doc.version), true, true).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // A save is the other moment worth re-reading on: a formatter, a
        // generator or a build may have written a sibling alongside it.
        //
        // It also publishes the whole list at once, emitter included, and
        // supersedes a pending debounced run: whoever saves is asking whether
        // the file is done, and the answer should not wait for a timer (G295).
        let uri = params.text_document.uri;
        let root = self.analysis_root(&uri);
        let full = {
            let mut ws = self.ws.lock().expect("workspace mutex");
            ws.rescan(&root);
            if ws.is_open(&uri) {
                let generation = self.publisher.bump(&uri, None);
                let (full, emitter) = self.publisher.compute_full(&mut ws, &root, &uri);
                self.publisher.set_carried(&uri, generation, emitter);
                Some((generation, full))
            } else {
                None
            }
        };
        let cause = full.as_ref().map(|(generation, _)| (uri.clone(), *generation));
        if let Some((generation, diagnostics)) = full {
            self.publisher.publish(uri.clone(), generation, diagnostics).await;
        }
        // The save re-read the project, which may have changed what the other
        // open documents import.
        self.republish_others(Some(&uri), cause).await;
    }

    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        // Files changed by something that is not this editor: a branch switch,
        // a generator, another editor. Registered in `initialized`, so a client
        // with no dynamic registration simply never sends this and the open and
        // the save are what keep the project current.
        //
        // Every live project is re-read rather than the ones the event names.
        // A changed file can be a member of a project rooted above it as well
        // as the nearest one, and a deleted file names a root that may never
        // have been a project at all.
        self.ws.lock().expect("workspace mutex").rescan_live();
        // What changed can be what an open document imports.
        self.republish_others(None, None).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync: the final content change carries the whole document.
        if let Some(change) = params.content_changes.into_iter().next_back() {
            self.refresh(
                params.text_document.uri,
                change.text,
                Some(params.text_document.version),
                false,
                false,
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = {
            // Disk is the truth for this file again from here on.
            let mut ws = self.ws.lock().expect("workspace mutex");
            ws.close(&uri);
            // Forgetting the document cancels its pending emitter run, and a
            // run already past its generation check finds no document at its
            // publish, so neither can put the squiggles back after they are
            // cleared below.
            self.publisher.forget(&uri)
        };
        // Clear the squiggles for a closed file.
        if let Some(version) = version {
            self.publisher.clear(uri.clone(), version).await;
        }
        // The closed file reads from disk again, and its siblings import
        // that, not the buffer they were last checked against.
        self.republish_others(Some(&uri), None).await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let Some(text) = self.buffer(&uri) else {
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
        let uri = pos.text_document.uri;
        // Read before the workspace lock: `analysis_root` takes the root lock.
        let root = self.analysis_root(&uri);
        let mut ws = self.ws.lock().expect("workspace mutex");
        let Some(text) = ws.buffer(&uri) else {
            return Ok(None);
        };
        let index = LineIndex::new(&text);
        let offset = index.offset(&text, pos.position.line, pos.position.character);
        Ok(ws.hover(&root, &uri, offset).map(|ty| Hover {
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
        // Read before the workspace lock: `analysis_root` takes the root lock.
        let root = self.analysis_root(&uri);
        let here = {
            let mut ws = self.ws.lock().expect("workspace mutex");
            let Some((text, analysis)) = ws.open_view(&root, &uri) else {
                return Ok(None);
            };
            let index = LineIndex::new(text);
            let offset = index.offset(text, pos.position.line, pos.position.character);
            match analysis.definition(offset, text) {
                None => None,
                Some(Definition::Here(start, end)) => {
                    Some(Ok(location_in(&uri, text, start, end)))
                }
                // Resolved outside the lock: the other module is read through
                // the overlay too, which needs the same lock this holds.
                Some(Definition::InModule { module_path, name }) => Some(Err((module_path, name))),
            }
        };
        let location = match here {
            None => None,
            Some(Ok(location)) => Some(location),
            Some(Err((module_path, name))) => self.resolve_cross_module(&uri, &module_path, &name),
        };
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let root = self.analysis_root(&uri);
        let mut ws = self.ws.lock().expect("workspace mutex");
        let Some((text, analysis)) = ws.open_view(&root, &uri) else {
            return Ok(None);
        };
        let index = LineIndex::new(text);
        let hints = analysis
            .inlay_type_hints(text)
            .into_iter()
            .map(|(offset, label)| {
                let (line, character) = index.position(text, offset);
                InlayHint {
                    position: Position { line, character },
                    label: InlayHintLabel::String(label),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right: None,
                    data: None,
                }
            })
            .collect();
        Ok(Some(hints))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let root = self.analysis_root(&uri);
        let mut ws = self.ws.lock().expect("workspace mutex");
        if !ws.is_open(&uri) {
            return Ok(None);
        }
        // The quick fix acts on the lint tier, which the front end produces;
        // the emitter has nothing for it.
        let Some((text, diagnostics)) = ws.diagnostics(&root, &uri, Stages::FrontEnd) else {
            return Ok(None);
        };
        let index = LineIndex::new(text);
        let req_start = index.offset(text, params.range.start.line, params.range.start.character);
        let req_end = index.offset(text, params.range.end.line, params.range.end.character);

        let mut actions = Vec::new();
        for d in diagnostics {
            // Quick-fix: remove a fully-unused import (E0106). Deletes the whole
            // import line, the same safe edit `glyph fix` applies at the CLI.
            if d.code != "E0106" {
                continue;
            }
            let (start, end) = (d.range.start.offset as usize, d.range.end.offset as usize);
            if start > req_end || end < req_start {
                continue;
            }
            let (ls, le) = line_span(text, start);
            let (sl, sc) = index.position(text, ls);
            let (el, ec) = index.position(text, le);
            let range = Range {
                start: Position { line: sl, character: sc },
                end: Position { line: el, character: ec },
            };
            let mut changes = HashMap::new();
            changes.insert(uri.clone(), vec![TextEdit { range, new_text: String::new() }]);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Remove unused import".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        // Use the parsed document's full candidate set; fall back to keywords +
        // prelude when the file is unknown or does not parse (mid-edit), which
        // is exactly when completion matters most.
        let root = self.analysis_root(&uri);
        let completions = {
            let mut ws = self.ws.lock().expect("workspace mutex");
            ws.open_view(&root, &uri)
                .map(|(_, a)| a.completions())
                .unwrap_or_else(base_completions)
        };
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
        let uri = params.text_document.uri;
        let root = self.analysis_root(&uri);
        let mut ws = self.ws.lock().expect("workspace mutex");
        let Some((text, analysis)) = ws.open_view(&root, &uri) else {
            return Ok(None);
        };
        let index = LineIndex::new(text);
        let symbols = analysis
            .document_symbols()
            .iter()
            .map(|s| outline_to_document_symbol(&index, text, s))
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
        let mut ws = self.ws.lock().expect("workspace mutex");
        // The overlay decides each file's text: the editor's buffer for the
        // ones it has open, disk for the rest.
        let uris = ws.workspace_docs(&root);

        // One model for the whole sweep, rooted at the folder being swept. A
        // per-file project would be right and unaffordable: every root the
        // loop had not seen would index itself, and a workspace of more
        // projects than the cache holds would re-index one on every file. An
        // outline is a parse, which no project graph changes.
        let mut out = Vec::new();
        for uri in uris {
            let Some((text, outline)) = ws.outline(&root, &uri) else {
                continue;
            };
            let index = LineIndex::new(text);
            for top in &outline {
                push_workspace_symbol(&mut out, &query, &uri, &index, text, top, None);
                for child in &top.children {
                    push_workspace_symbol(
                        &mut out,
                        &query,
                        &uri,
                        &index,
                        text,
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
        let include_decl = params.context.include_declaration;
        // Read before the workspace lock: both take the root lock.
        let file_module = self.file_module(&uri);
        let here = self.analysis_root(&uri);

        let mut ws = self.ws.lock().expect("workspace mutex");
        let Some((text, analysis)) = ws.open_view(&here, &uri) else {
            return Ok(None);
        };
        let index = LineIndex::new(text);
        let offset = index.offset(text, pos.position.line, pos.position.character);

        // With a workspace, a module-level symbol's references span every file.
        if let Some((root, this_module)) = file_module {
            if let Some(SymbolTarget::Global { module, name }) =
                analysis.symbol_target(offset, text, &this_module)
            {
                let mut locations = Vec::new();
                for u2 in ws.workspace_docs(&root) {
                    let Some(fm) = u2
                        .to_file_path()
                        .ok()
                        .and_then(|p| module_path_of(&root, &p))
                    else {
                        continue;
                    };
                    // Read through the model rooted where the sweep is: an
                    // occurrence is a name in a resolution table, which is a
                    // reading of one file, so the root here scopes the sweep
                    // rather than deciding an answer.
                    let Some((t2, a2)) = ws.view(&root, &u2) else {
                        continue;
                    };
                    for (s, e) in a2.global_occurrences(&fm, &module, &name, t2, include_decl) {
                        locations.push(location_in(&u2, t2, s, e));
                    }
                }
                return Ok((!locations.is_empty()).then_some(locations));
            }
        }
        // A local binding, or no workspace: the open document only.
        let spans = analysis.references(offset, text, include_decl);
        Ok((!spans.is_empty()).then(|| {
            spans
                .into_iter()
                .map(|(s, e)| location_in(&uri, text, s, e))
                .collect()
        }))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let pos = params.text_document_position;
        let uri = pos.text_document.uri;
        // Read before the workspace lock: both take the root lock.
        let file_module = self.file_module(&uri);
        let here = self.analysis_root(&uri);

        let mut ws = self.ws.lock().expect("workspace mutex");
        let Some((text, analysis)) = ws.open_view(&here, &uri) else {
            return Ok(None);
        };
        let index = LineIndex::new(text);
        let offset = index.offset(text, pos.position.line, pos.position.character);

        // With a workspace, a module-level rename edits every file that names the
        // symbol — the declaration, its references, and each importing module's
        // import binding — so it is complete and safe.
        if let Some((root, this_module)) = file_module {
            if let Some(SymbolTarget::Global { module, name }) =
                analysis.symbol_target(offset, text, &this_module)
            {
                validate_rename_name(&params.new_name).map_err(rename_error)?;
                let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
                for u2 in ws.workspace_docs(&root) {
                    let Some(fm) = u2
                        .to_file_path()
                        .ok()
                        .and_then(|p| module_path_of(&root, &p))
                    else {
                        continue;
                    };
                    let Some((t2, a2)) = ws.view(&root, &u2) else {
                        continue;
                    };
                    let idx2 = LineIndex::new(t2);
                    let edits: Vec<TextEdit> = a2
                        .global_occurrences(&fm, &module, &name, t2, true)
                        .into_iter()
                        .map(|(s, e)| text_edit(&idx2, t2, s, e, &params.new_name))
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
            .rename_edits(offset, text, &params.new_name)
            .map_err(rename_error)?;
        let edits: Vec<TextEdit> = spans
            .into_iter()
            .map(|(s, e)| text_edit(&index, text, s, e, &params.new_name))
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
pub(crate) fn collect_glyph_files(dir: &Path, out: &mut Vec<PathBuf>) {
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

/// The nearest marked project's resolution root at or above `file` (D41): the
/// nearest ancestor holding a `package.json` with a top-level `"glyph"` key,
/// with that key's `"src"` applied (else `src/` when it exists, else the marked
/// directory). `None` when the climb finds no marker.
///
/// The climb stops at the workspace folder or at a directory holding `.git`,
/// whichever comes first, so an unrelated marker above the workspace cannot
/// capture a file.
///
/// The two roots below differ only in what they do when this answers `None`.
fn marked_root_for(file: &Path, workspace: &Path) -> Option<PathBuf> {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if let Some(src) = marked_project_src(d) {
            if file.starts_with(&src) {
                return Some(src);
            }
        }
        if d == workspace || d.join(".git").exists() {
            break;
        }
        dir = d.parent();
    }
    None
}

/// The resolution root the *editor* server counts a file's imports from: its
/// marked project, else `workspace`, the editor's workspace folder, which is
/// what the server used before D41.
///
/// This mirrors `glyph-cli`'s `config::project_for_file`, and the two have to
/// agree: if they disagree, go-to-definition finds nothing in a tree `glyph
/// build` compiles without complaint. They cannot share code today because
/// `glyph-cli` depends on `glyph-lsp`, so the dependency cannot point back;
/// hoisting both onto a shared crate is the follow-up.
pub(crate) fn project_root_for(file: &Path, workspace: &Path) -> PathBuf {
    marked_root_for(file, workspace).unwrap_or_else(|| workspace.to_path_buf())
}

/// The nearest ancestor `src/` directory at or above `file`, which is the
/// resolution root of an unmarked tree.
///
/// `glyph build` and `glyph check` over an unmarked directory already give it
/// the source-directory rule a marked one gets, minus the marker: `src/` when
/// it exists, else the directory (`discover_projects`). A file handed to a tool
/// has no build target to apply that to, so the same rule is spelled by
/// climbing: the first ancestor named `src` is the directory the build would
/// have counted from.
///
/// Without it a nested file had two identities. On an unmarked tree holding
/// `src/sub/deep.glyph`, `glyph check src` reported the module `sub/deep` and
/// every tool reported `deep`, because the tools fell back to the file's own
/// parent. One declaration with two names is the thing G180 was fixed to
/// remove, and a nested directory brought it back.
///
/// The climb stops at `stop_at` (the editor's workspace folder) or at a
/// directory holding `.git`, whichever comes first, so a `src/` far above an
/// unrelated tree cannot capture a file. `None` when the climb finds none,
/// which leaves each caller its own last resort.
///
/// `glyph-cli`'s `config::project_for_file` calls this rather than keeping a
/// second copy: two implementations of a module key is what G180 was.
pub fn nearest_src_root(file: &Path, stop_at: Option<&Path>) -> Option<PathBuf> {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d.file_name().and_then(|n| n.to_str()) == Some("src") && d.is_dir() {
            return Some(d.to_path_buf());
        }
        if stop_at == Some(d) || d.join(".git").exists() {
            break;
        }
        dir = d.parent();
    }
    None
}

/// The root a declaration's `module::name` identity is counted from: its marked
/// project, else the nearest ancestor `src/`, else the file's **own parent
/// directory**.
///
/// A declaration's identity must not change with where a tool was invoked or
/// what the cwd was, and a workspace-root fallback makes it do exactly that.
/// The same function answered to `tests/negative/non_exhaustive_union::handle`
/// from an MCP server started at the repository root and to
/// `non_exhaustive_union::handle` from `glyph check --json`, which places a
/// file in its project with `glyph-cli`'s `config::project_for_file` and falls
/// back to the file's own directory. The file's parent is the one root that
/// does not vary with the invocation, so it is the one the two agree on
/// (G180).
///
/// The parent alone was not enough once a project had subdirectories. An
/// unmarked tree's resolution root is its `src/`, which is what `glyph check`
/// counts a nested module from, and the parent of `src/sub/deep.glyph` is
/// `src/sub`. `nearest_src_root` is the rule the build already applies at its
/// target, spelled for a file, and the parent stays the last resort for a tree
/// that has no `src/` at all.
///
/// This is the root every identity-publishing surface uses. It is separate
/// from `project_root_for` because the editor's workspace folder is a root the
/// user opened, and narrowing it is a change to which files a workspace-wide
/// query ranges over rather than to how a declaration is named.
pub(crate) fn module_root_for(file: &Path, workspace: &Path) -> PathBuf {
    marked_root_for(file, workspace)
        .or_else(|| nearest_src_root(file, Some(workspace)))
        .unwrap_or_else(|| {
            file.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        })
}

/// The resolution root declared by `dir/package.json`, or `None` when there is
/// no manifest, no `"glyph"` key, or the manifest will not parse (D41: a
/// malformed manifest is not a marker).
pub(crate) fn marked_project_src(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    let glyph = manifest.get("glyph")?;
    if let Some(src) = glyph.get("src").and_then(|s| s.as_str()) {
        return Some(dir.join(src));
    }
    let conventional = dir.join("src");
    Some(if conventional.is_dir() {
        conventional
    } else {
        dir.to_path_buf()
    })
}

/// The Glyph module path of a `.glyph` file under `root` — its path relative to
/// the root, minus the extension, with `/` separators (`src/foo.glyph` →
/// `src/foo`). This is the name an `import` uses and the key a cross-file
/// symbol is identified by. `None` when the file is not under the root.
pub(crate) fn module_path_of(root: &Path, file: &Path) -> Option<String> {
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

/// Response to `glyph/applyEdit` (review 81, decision 2). `diagnostics` holds
/// only the diagnostics the edit introduced, warnings included, and `ok` is
/// true exactly when it is empty. `preexisting` holds the candidate's other
/// diagnostics, the ones the file already had, on acceptance and on refusal
/// alike, so `ok: true` means "this edit introduced nothing" and `ok: true`
/// with an empty `preexisting` means the file is clean. On success `content`
/// is the verified new text; on rejection it is absent and `rejected` names
/// the reason (both lists are empty for a structural rejection like
/// overlapping edits). Both lists are in the candidate's coordinates.
#[derive(Debug, serde::Serialize)]
struct ApplyEditResponse {
    ok: bool,
    content: Option<String>,
    rejected: Option<String>,
    diagnostics: Vec<Diagnostic>,
    preexisting: Vec<Diagnostic>,
}

impl Backend {
    /// The editor's buffer for an open document, if any. A file the editor
    /// does not have open answers `None` here even when it exists on disk:
    /// the per-document requests are about the buffer, and the overlay's disk
    /// half belongs to the workspace-wide ones.
    fn buffer(&self, uri: &Url) -> Option<String> {
        self.ws.lock().expect("workspace mutex").buffer(uri)
    }

    /// The open file's resolution root and its own module path, when both exist
    /// (there is a workspace and the file lives under it). `None` falls back to
    /// file-scoped behaviour.
    ///
    /// The root is the file's *project* (D41), not the workspace folder, so a
    /// monorepo of marked apps answers the same way `glyph build` does. Every
    /// workspace-wide query is scoped to it, which is right: another project's
    /// `import lib` names its own `lib`, not this one.
    fn file_module(&self, uri: &Url) -> Option<(PathBuf, String)> {
        let workspace = self.root.lock().expect("root mutex").clone()?;
        let path = uri.to_file_path().ok()?;
        let root = project_root_for(&path, &workspace);
        let module = module_path_of(&root, &path)?;
        Some((root, module))
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
        let Some(text) = self.buffer(&params.uri) else {
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
    /// the compiler's own check of the file, not a new language construct. A
    /// successful call returns the verified new text
    /// (the caller applies it and syncs via the normal `didChange`); a rejected
    /// call changes nothing and returns the errors the edit would have caused,
    /// making "the agent broke the file" a rejection rather than a saved edit.
    ///
    /// The gate: the edit may introduce no diagnostic, the warning tier
    /// included; diagnostics the file already had are returned under
    /// `preexisting` and do not refuse it (review 81, decision 2). "Already
    /// had" is decided by mapping the file's list before the edit through the
    /// edit (`split_introduced`) and matching the candidate's list against it
    /// as a multiset. A diagnostic whose text the edit wrote or touched is the
    /// edit's own, and so is one a removed error used to hide behind it. One
    /// that strictly encloses the edit (an E0200 over a `match` whose arm
    /// changed) keeps its start, stretches its end, and is preexisting when
    /// its message is unchanged (review 82, item 3(b)).
    ///
    /// "The warning tier included" holds only while the file has no error.
    /// The file's list stops after the type checker when it has one
    /// (`project_file_diagnostics`), so the lint tier and the emitter never
    /// run on either side of the comparison: an edit to a file that already
    /// has an error can add a warning or an E0302 to E0305 refusal and is
    /// still answered `ok: true`. The diagnostic surfaces on the edit that
    /// fixes the last error, which is then refused for it (G313).
    /// Running the `@example`/property tests as part of the gate is a v1.1
    /// enhancement (it needs the build pipeline factored into a library the
    /// server can call without the current cli→lsp dependency cycle).
    ///
    /// The candidate is checked inside the document's project, through the
    /// emitter, with the list the server publishes for the file and `glyph
    /// check --no-tsc` reports for it. Until 0.1.132 it was checked as text
    /// alone and the check stopped after the checker, so an edit introducing
    /// an emitter refusal or an error against a sibling module was accepted
    /// (G306). The buffer is swapped for the candidate and back under one
    /// hold of the workspace lock, so nothing else ever reads the candidate,
    /// and the list before the edit is read under the same hold.
    ///
    /// The gate compares the edited file's list only; an edit that breaks a
    /// module importing it is accepted. `glyph check` over the project is the
    /// authority until 0.1.133 (G310).
    async fn apply_edit_request(&self, params: ApplyEditParams) -> Result<ApplyEditResponse> {
        let uri = &params.uri;
        let root = self.analysis_root(uri);
        let checked = {
            let mut ws = self.ws.lock().expect("workspace mutex");
            match ws.buffer(uri) {
                None => Err("document_not_open".to_string()),
                Some(text) => match resolve_text_edits(&text, &params.edits) {
                    Err(reason) => Err(reason),
                    Ok(spans) => {
                        let candidate = splice_text_edits(&text, &spans);
                        let regions = edit_regions(&text, &spans);
                        let before = ws
                            .diagnostics(&root, uri, Stages::All)
                            .map(|(_, d)| d)
                            .unwrap_or_default();
                        ws.set_buffer(uri, candidate.clone());
                        let after = ws
                            .diagnostics(&root, uri, Stages::All)
                            .map(|(_, d)| d)
                            .unwrap_or_default();
                        ws.set_buffer(uri, text);
                        Ok((candidate, split_introduced(&before, &regions, after)))
                    }
                },
            }
        };
        let (candidate, (introduced, preexisting)) = match checked {
            Ok(c) => c,
            Err(reason) => return Ok(reject(&reason, Vec::new(), Vec::new())),
        };
        let diagnostics = to_lsp_diagnostics(&candidate, introduced);
        let preexisting = to_lsp_diagnostics(&candidate, preexisting);
        if diagnostics.is_empty() {
            Ok(ApplyEditResponse {
                ok: true,
                content: Some(candidate),
                rejected: None,
                diagnostics,
                preexisting,
            })
        } else {
            Ok(reject("verification_failed", diagnostics, preexisting))
        }
    }

    /// Resolve an imported `module_path` to its file and locate the declaration
    /// named `name` in it (a `.glyph` whose path mirrors the module path, e.g.
    /// `sub/b` → `<root>/sub/b.glyph`). `None` if there is no workspace root, no
    /// such file (a `std/*` import has no project source), or the declaration is
    /// not found.
    ///
    /// The root is the *importing* file's project (D41), which is the root that
    /// file's import paths are counted from.
    fn resolve_cross_module(&self, from: &Url, module_path: &str, name: &str) -> Option<Location> {
        let workspace = self.root.lock().expect("root mutex").clone()?;
        let root = match from.to_file_path() {
            Ok(path) => project_root_for(&path, &workspace),
            Err(_) => workspace,
        };
        let file = root.join(module_path).with_extension("glyph");
        let uri = Url::from_file_path(&file).ok()?;
        // Through the overlay: the buffer when the editor has the other file
        // open, disk otherwise.
        let mut ws = self.ws.lock().expect("workspace mutex");
        let (text, outline) = ws.outline(&root, &uri)?;
        let (start, end) = find_symbol_span(&outline, name)?;
        Some(location_in(&uri, text, start, end))
    }
}

/// A rejected `glyph/applyEdit` response carrying a reason, the diagnostics
/// the edit introduced and the ones the file already had.
fn reject(
    reason: &str,
    diagnostics: Vec<Diagnostic>,
    preexisting: Vec<Diagnostic>,
) -> ApplyEditResponse {
    ApplyEditResponse {
        ok: false,
        content: None,
        rejected: Some(reason.to_string()),
        diagnostics,
        preexisting,
    }
}

/// One `TextEdit`'s edited region in the old text: `old[start..old_end]`
/// became `new_len` bytes. The edit's range shrunk by the common prefix and
/// the common suffix of the text it replaced and its `newText`, so a
/// whole-buffer replacement that changes one line edits that line, not the
/// file (review 81, decision 2).
struct EditRegion {
    start: usize,
    old_end: usize,
    new_len: usize,
}

/// The edited regions of resolved, sorted `spans` over `text`, in order. An
/// edit whose `newText` equals what it replaces edits nothing and has none.
fn edit_regions(text: &str, spans: &[(usize, usize, &str)]) -> Vec<EditRegion> {
    spans
        .iter()
        .filter_map(|&(start, end, new_text)| {
            let (prefix, old_end, new_end) = edit_region(&text[start..end], new_text)?;
            Some(EditRegion {
                start: start + prefix,
                old_end: start + old_end,
                new_len: new_end - prefix,
            })
        })
        .collect()
}

/// The byte range `[start, end)` of the old text in the new text's
/// coordinates, through `regions` (sorted): unmoved when wholly before every
/// region, shifted by the deltas of the regions before it otherwise, and
/// `None` when it overlaps or touches one, the rule `carry_through_edit` uses
/// for a single region.
///
/// A range that strictly encloses a region (starts before it and ends after
/// it) is about the text around the edit, not the text the edit wrote: an
/// E0200 spans the whole `match` whose arm was changed. It keeps its start
/// and its end moves by the region's delta (review 82, item 3(b)). Whether
/// it is the same diagnostic is still decided by its message, so an E0200
/// that now misses one more variant is the edit's own.
fn map_through_regions(regions: &[EditRegion], start: usize, end: usize) -> Option<(usize, usize)> {
    let mut start_shift = 0isize;
    let mut end_shift = 0isize;
    for r in regions {
        if end < r.start {
            break;
        }
        let delta = r.new_len as isize - (r.old_end - r.start) as isize;
        if start > r.old_end {
            start_shift += delta;
            end_shift += delta;
            continue;
        }
        if start < r.start && end > r.old_end {
            end_shift += delta;
            continue;
        }
        return None;
    }
    let moved = |o: usize, shift: isize| (o as isize + shift) as usize;
    Some((moved(start, start_shift), moved(end, end_shift)))
}

/// The lexer's message embeds the error's byte offset (`lex error: ... at
/// offset N`, `glyph_lexer::LexError`), the one message in the catalogue that
/// carries a source position. When `N` is the diagnostic's own start, the
/// mapped message says the mapped start; an edit that moved the error would
/// otherwise make it look new. An offset counted from somewhere else (an
/// interpolation's interior) is left as written.
fn map_message_offset(d: &diagnostic::Diagnostic, mapped_start: usize) -> String {
    const AT: &str = " at offset ";
    if d.code != "E0001" {
        return d.message.clone();
    }
    let Some(at) = d.message.rfind(AT) else {
        return d.message.clone();
    };
    let digits_start = at + AT.len();
    let digits_len = d.message[digits_start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    let digits = &d.message[digits_start..digits_start + digits_len];
    if digits.parse::<u32>().ok() != Some(d.range.start.offset) {
        return d.message.clone();
    }
    format!(
        "{}{mapped_start}{}",
        &d.message[..digits_start],
        &d.message[digits_start + digits_len..]
    )
}

/// Split the candidate's diagnostics `after` into the ones the edit
/// introduced and the ones the file already had (review 81, decision 2).
///
/// Each of `before` is mapped into the candidate's coordinates through the
/// edit's `regions`; one the edit overlaps or touches is dropped, because the
/// text it was about is now the edit's, and one that strictly encloses a
/// region keeps its start and stretches its end. A candidate diagnostic is already
/// there when an unused mapped one has the same code, severity, message, help
/// and range: a multiset match, so two identical diagnostics need two
/// before. Everything else, a changed message included, is introduced.
fn split_introduced(
    before: &[diagnostic::Diagnostic],
    regions: &[EditRegion],
    after: Vec<diagnostic::Diagnostic>,
) -> (Vec<diagnostic::Diagnostic>, Vec<diagnostic::Diagnostic>) {
    type Key<'a> = (&'a str, &'a str, String, Option<&'a str>, usize, usize);
    let mut mapped: Vec<Option<Key>> = before
        .iter()
        .filter_map(|d| {
            let (s, e) = map_through_regions(
                regions,
                d.range.start.offset as usize,
                d.range.end.offset as usize,
            )?;
            Some(Some((
                d.code.as_str(),
                d.severity.as_str(),
                map_message_offset(d, s),
                d.help.as_deref(),
                s,
                e,
            )))
        })
        .collect();
    let mut introduced = Vec::new();
    let mut preexisting = Vec::new();
    for d in after {
        let key: Key = (
            d.code.as_str(),
            d.severity.as_str(),
            d.message.clone(),
            d.help.as_deref(),
            d.range.start.offset as usize,
            d.range.end.offset as usize,
        );
        match mapped.iter_mut().find(|m| m.as_ref() == Some(&key)) {
            Some(slot) => {
                *slot = None;
                preexisting.push(d);
            }
            None => introduced.push(d),
        }
    }
    (introduced, preexisting)
}

/// Apply LSP `TextEdit`s to `text`, producing the candidate document. Edits are
/// resolved to byte offsets, checked for overlap, and spliced from the end
/// backwards so earlier offsets stay valid. Returns `Err(reason)` for an
/// out-of-bounds range or overlapping edits (the edit set is then applied
/// atomically: all or nothing).
#[cfg(test)]
fn apply_text_edits(text: &str, edits: &[TextEdit]) -> std::result::Result<String, String> {
    resolve_text_edits(text, edits).map(|spans| splice_text_edits(text, &spans))
}

/// Splice resolved, sorted, non-overlapping `spans` into `text`, from the end
/// backwards so earlier offsets stay valid.
fn splice_text_edits(text: &str, spans: &[(usize, usize, &str)]) -> String {
    let mut out = text.to_string();
    for &(start, end, new_text) in spans.iter().rev() {
        out.replace_range(start..end, new_text);
    }
    out
}

/// Resolve LSP `TextEdit`s over `text` to byte spans `(start, end, newText)`,
/// sorted by start. Returns `Err(reason)` for an out-of-bounds range, a
/// position off a character boundary, or overlapping edits, so the set is
/// applied all or nothing.
fn resolve_text_edits<'e>(
    text: &str,
    edits: &'e [TextEdit],
) -> std::result::Result<Vec<(usize, usize, &'e str)>, String> {
    let index = LineIndex::new(text);
    // The highest valid line index. `LineIndex::offset` silently clamps an
    // out-of-range line to EOF, which would mis-apply an edit instead of failing;
    // reject such ranges explicitly so a bogus position is a rejection, not a
    // corrupt splice.
    let max_line = text.bytes().filter(|&b| b == b'\n').count();
    let mut spans: Vec<(usize, usize, &'e str)> = Vec::with_capacity(edits.len());
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
    Ok(spans)
}

/// A `Location` for byte range `[start, end)` in `text` at `uri`.
/// The byte range of the whole line containing `offset`, including its trailing
/// newline, so deleting it removes the line entirely. Used by the unused-import
/// quick-fix.
fn line_span(text: &str, offset: usize) -> (usize, usize) {
    let start = text[..offset.min(text.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = text[offset.min(text.len())..]
        .find('\n')
        .map(|i| offset + i + 1)
        .unwrap_or(text.len());
    (start, end)
}

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
    use analysis::analyze;

    /// A publisher whose publishes land in the returned list.
    fn recording() -> (Publisher, PublishLog) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let publisher = Publisher {
            sink: Sink::Record(Arc::clone(&log)),
            workspace: Arc::new(Mutex::new(None)),
            ws: Arc::new(Mutex::new(Workspace::new())),
            docs: Arc::new(Mutex::new(HashMap::new())),
            generations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            gate: Arc::new(tokio::sync::Mutex::new(())),
        };
        (publisher, log)
    }

    /// G295: a result computed for a buffer the editor has since replaced is
    /// dropped, never published over the newer one's diagnostics. This is the
    /// race the generation exists for: an emitter run that finished computing
    /// just before a keystroke landed and reaches the publish just after it.
    #[tokio::test]
    async fn a_superseded_generation_is_never_published() {
        let (publisher, log) = recording();
        let uri = Url::parse("file:///tmp/g295/main.glyph").expect("uri");
        let stale = publisher.bump(&uri, Some(1));
        let fresh = publisher.bump(&uri, Some(2));
        let e0302 = Diagnostic {
            code: Some(NumberOrString::String("E0302".to_string())),
            ..Default::default()
        };
        publisher.publish(uri.clone(), fresh, Vec::new()).await;
        publisher.publish(uri.clone(), stale, vec![e0302]).await;
        assert_eq!(
            *log.lock().unwrap(),
            vec![(uri, Some(2), Vec::new())],
            "only the current generation is published"
        );
    }

    /// G295: a debounced emitter run publishes the whole list, emitter
    /// included, for the buffer it was started for; and a run a newer change
    /// superseded before it fired publishes nothing at all.
    #[tokio::test]
    async fn a_debounced_run_publishes_the_emitter_unless_superseded() {
        let dir = tmp_dir("debounce");
        let path = dir.join("main.glyph");
        let text = "module main\npub type Id =\n  | None\n  | Some(number)\n\n\
            fn f(b: string) -> Result<string, string> {\n  return Ok(b)\n}\n\n\
            fn shout(s: string) -> string {\n  return s\n}\n\n\
            pub fn pick(id: Id, b: string) -> Result<string, string> {\n  \
            return Ok(shout(match id {\n    None => f(b)?,\n    Some(n) => b,\n  }))\n}\n";
        std::fs::write(&path, text).expect("write main");
        let uri = Url::from_file_path(&path).expect("uri");
        let (publisher, log) = recording();
        publisher
            .ws
            .lock()
            .unwrap()
            .set_buffer(&uri, text.to_string());

        let superseded = publisher.bump(&uri, Some(1));
        publisher.schedule_full(dir.clone(), uri.clone(), superseded);
        let current = publisher.bump(&uri, Some(2));
        tokio::time::sleep(EMIT_DEBOUNCE * 3).await;
        assert!(log.lock().unwrap().is_empty(), "the superseded run published: {:?}", log.lock().unwrap());

        publisher.schedule_full(dir.clone(), uri.clone(), current);
        tokio::time::sleep(EMIT_DEBOUNCE * 3).await;
        assert_eq!(
            *log.lock().unwrap(),
            vec![(uri, Some(2), vec!["E0302".to_string()])],
            "the current run publishes the emitter's refusal"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review 78, item 8b: closing a document forgets it, and a reopen does
    /// not start its generations again from the ones the closed document
    /// used, so a run started before the close cannot publish after the
    /// reopen.
    #[tokio::test]
    async fn a_closed_document_is_forgotten_and_its_generations_stay_dead() {
        let (publisher, log) = recording();
        let uri = Url::parse("file:///tmp/g295/closed.glyph").expect("uri");
        let before = publisher.bump(&uri, Some(1));
        assert_eq!(publisher.forget(&uri), Some(Some(1)));
        assert!(publisher.docs.lock().unwrap().is_empty(), "the closed document is still held");
        publisher.clear(uri.clone(), Some(1)).await;

        let reopened = publisher.bump(&uri, Some(1));
        assert_ne!(reopened, before, "the reopen reused a closed generation");
        let e0302 = Diagnostic {
            code: Some(NumberOrString::String("E0302".to_string())),
            ..Default::default()
        };
        publisher.publish(uri.clone(), before, vec![e0302]).await;
        // A clear for a document that was reopened in the meantime would
        // wipe the reopen's diagnostics, so it does not go out.
        publisher.clear(uri.clone(), Some(1)).await;
        assert_eq!(*log.lock().unwrap(), vec![(uri, Some(1), Vec::new())]);
    }

    /// Review 78, item 8c: a debounced run a newer event superseded is
    /// cancelled, not left to wake up and find itself stale.
    #[tokio::test]
    async fn a_superseded_debounced_run_is_cancelled() {
        let (publisher, _log) = recording();
        let uri = Url::parse("file:///tmp/g295/cancel.glyph").expect("uri");
        let first = publisher.bump(&uri, Some(1));
        publisher.schedule_full(PathBuf::new(), uri.clone(), first);
        let pending = publisher.docs.lock().unwrap()[&uri]
            .pending
            .clone()
            .expect("the run is held");
        publisher.bump(&uri, Some(2));
        tokio::time::sleep(EMIT_DEBOUNCE / 10).await;
        assert!(pending.is_finished(), "the superseded run is still waiting");
    }

    /// A diagnostic over the first occurrence of `needle` in `text`.
    fn diag_at(text: &str, needle: &str) -> Diagnostic {
        let index = LineIndex::new(text);
        let s = text.find(needle).expect("needle");
        let pos = |o: usize| {
            let (l, c) = index.position(text, o);
            Position::new(l, c)
        };
        Diagnostic {
            range: Range::new(pos(s), pos(s + needle.len())),
            code: Some(NumberOrString::String("E0302".to_string())),
            ..Default::default()
        }
    }

    /// Review 78, item 8a: a line typed above a carried diagnostic moves it
    /// down a line, at the same column.
    #[test]
    fn a_carried_diagnostic_moves_down_with_a_line_typed_above_it() {
        let old = "module m\nfn f() -> void {\n  g(x)?\n}\n";
        let new = "module m\n// note\nfn f() -> void {\n  g(x)?\n}\n";
        let carried = carry_through_edit(old, new, &[diag_at(old, "g(x)?")]);
        assert_eq!(carried.len(), 1);
        assert_eq!(carried[0].range, diag_at(new, "g(x)?").range);
    }

    /// An edit earlier on the same line moves the column, and an edit after a
    /// diagnostic leaves it where it was.
    #[test]
    fn a_carried_diagnostic_shifts_by_column_and_stays_before_an_edit() {
        let old = "let a = g(x)?\nlet b = 1\n";
        let new = "let aaa = g(x)?\nlet b = 1\n";
        let carried = carry_through_edit(old, new, &[diag_at(old, "g(x)?")]);
        assert_eq!(carried.iter().map(|d| d.range).collect::<Vec<_>>(), vec![diag_at(new, "g(x)?").range]);

        let new = "let a = g(x)?\nlet b = 22\n";
        let carried = carry_through_edit(old, new, &[diag_at(old, "g(x)?")]);
        assert_eq!(carried.iter().map(|d| d.range).collect::<Vec<_>>(), vec![diag_at(old, "g(x)?").range]);
    }

    /// An edit inside a diagnostic's range, or right against it, drops it.
    #[test]
    fn a_carried_diagnostic_the_edit_touches_is_dropped() {
        let old = "let a = g(x)?\n";
        for new in ["let a = g(y)?\n", "let a = zg(x)?\n", "let a = g(x)?z\n", "let a = r\n"] {
            assert!(
                carry_through_edit(old, new, &[diag_at(old, "g(x)?")]).is_empty(),
                "kept across {new:?}"
            );
        }
    }

    /// A uniquely named temp directory for the project-root tests.
    fn tmp_dir(prefix: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("glyph_lsp_{prefix}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// The server resolves module paths the way `glyph build` does (D41): from
    /// the file's marked project, not from the editor's workspace folder. If it
    /// did not, go-to-definition would find nothing in a tree the compiler
    /// builds without complaint.
    #[test]
    fn the_project_root_is_the_nearest_marked_ancestor() {
        let workspace = tmp_dir("marked");
        let app = workspace.join("apps").join("csvql");
        std::fs::create_dir_all(app.join("src")).expect("app src");
        std::fs::write(
            app.join("package.json"),
            "{ \"name\": \"csvql\", \"private\": true, \"glyph\": { \"src\": \"src\" } }\n",
        )
        .expect("marker");
        let file = app.join("src").join("main.glyph");

        let root = project_root_for(&file, &workspace);
        assert_eq!(root, app.join("src"));
        assert_eq!(module_path_of(&root, &file).as_deref(), Some("main"));
        let _ = std::fs::remove_dir_all(&workspace);
    }

    /// With no marker anywhere, the workspace folder stays the root, which is
    /// what every project had before D41.
    #[test]
    fn an_unmarked_tree_still_roots_at_the_workspace() {
        let workspace = tmp_dir("unmarked");
        let sub = workspace.join("sub");
        std::fs::create_dir_all(&sub).expect("sub");
        // A plain npm package is not a Glyph project root.
        std::fs::write(sub.join("package.json"), "{ \"name\": \"plain\" }\n").expect("manifest");
        let file = sub.join("b.glyph");

        assert_eq!(project_root_for(&file, &workspace), workspace);
        assert_eq!(module_path_of(&workspace, &file).as_deref(), Some("sub/b"));
        let _ = std::fs::remove_dir_all(&workspace);
    }

    /// The identity root on an unmarked tree is its `src/`, not the parent of
    /// whatever subdirectory a file sits in. `glyph check` counts a nested
    /// module from `src`, so `src/sub/deep.glyph` is `sub/deep`; the tools
    /// answered `deep` until `nearest_src_root` gave them the same rule.
    #[test]
    fn a_nested_unmarked_tree_keys_its_modules_from_src() {
        let workspace = tmp_dir("unmarked_nested");
        let sub = workspace.join("src").join("sub");
        std::fs::create_dir_all(&sub).expect("src/sub");
        let file = sub.join("deep.glyph");
        std::fs::write(&file, "module deep\n").expect("write deep");

        let root = module_root_for(&file, &workspace);
        assert_eq!(root, workspace.join("src"));
        assert_eq!(module_path_of(&root, &file).as_deref(), Some("sub/deep"));
        let _ = std::fs::remove_dir_all(&workspace);
    }

    /// A tree with no `src/` keeps the parent fallback, which is the root that
    /// does not vary with the invocation (G180). The climb is an addition to
    /// that rule, not a replacement for it.
    #[test]
    fn a_tree_with_no_src_keeps_the_parent_fallback() {
        let workspace = tmp_dir("no_src");
        let sub = workspace.join("lib");
        std::fs::create_dir_all(&sub).expect("lib");
        let file = sub.join("b.glyph");
        std::fs::write(&file, "module b\n").expect("write b");

        assert_eq!(module_root_for(&file, &workspace), sub);
        assert_eq!(nearest_src_root(&file, Some(&workspace)), None);
        let _ = std::fs::remove_dir_all(&workspace);
    }

    /// The climb stops at the workspace folder, so a `src/` above a tree the
    /// editor did not open cannot capture a file inside it.
    #[test]
    fn the_src_climb_stops_at_the_workspace() {
        let outer = tmp_dir("src_above");
        let workspace = outer.join("src").join("inner");
        let sub = workspace.join("pkg");
        std::fs::create_dir_all(&sub).expect("pkg");
        let file = sub.join("c.glyph");
        std::fs::write(&file, "module c\n").expect("write c");

        assert_eq!(nearest_src_root(&file, Some(&workspace)), None);
        assert_eq!(module_root_for(&file, &workspace), sub);
        let _ = std::fs::remove_dir_all(&outer);
    }

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

    /// A compiler diagnostic over `[start, end)` of `source`, for the gate's
    /// matching tests.
    fn gate_diag(source: &str, start: u32, end: u32, message: &str) -> diagnostic::Diagnostic {
        diagnostic::Diagnostic::new(
            "main",
            source,
            glyph_ast::Span::new(start, end),
            "E0213",
            "error",
            "typecheck",
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Review 81, decision 2: "already there" is a multiset match. One
    /// diagnostic before the edit accounts for one identical diagnostic after
    /// it, not for two, so an edit that duplicates an error is refused for
    /// the copy.
    #[test]
    fn the_gate_matches_preexisting_diagnostics_as_a_multiset() {
        let source = "abcdefghij";
        let before = [gate_diag(source, 2, 4, "m")];
        let after = vec![gate_diag(source, 2, 4, "m"), gate_diag(source, 2, 4, "m")];
        let (introduced, preexisting) = split_introduced(&before, &[], after);
        assert_eq!((introduced.len(), preexisting.len()), (1, 1));
    }

    /// The regions of several edits shift a diagnostic by the deltas of the
    /// regions before it, and drop one a region touches.
    #[test]
    fn the_gate_maps_a_range_through_every_region_before_it() {
        let regions = [
            EditRegion { start: 2, old_end: 3, new_len: 4 },
            EditRegion { start: 10, old_end: 10, new_len: 2 },
        ];
        assert_eq!(map_through_regions(&regions, 0, 1), Some((0, 1)));
        assert_eq!(map_through_regions(&regions, 5, 7), Some((8, 10)));
        assert_eq!(map_through_regions(&regions, 12, 14), Some((17, 19)));
        assert_eq!(map_through_regions(&regions, 1, 2), None);
        assert_eq!(map_through_regions(&regions, 8, 10), None);
    }

    /// Review 82, item 3(b): a diagnostic that strictly encloses a region
    /// (an E0200 over a whole `match` whose arm the edit changed) keeps its
    /// start and moves its end by the region's delta. One that starts or
    /// ends at a region's edge still touches it and is dropped, and one
    /// that encloses a region but ends inside the next is dropped too.
    #[test]
    fn the_gate_keeps_a_diagnostic_that_encloses_a_region() {
        let one = [EditRegion { start: 5, old_end: 6, new_len: 3 }];
        assert_eq!(map_through_regions(&one, 2, 9), Some((2, 11)));
        assert_eq!(map_through_regions(&one, 5, 9), None);
        assert_eq!(map_through_regions(&one, 2, 6), None);
        let insert = [EditRegion { start: 5, old_end: 5, new_len: 1 }];
        assert_eq!(map_through_regions(&insert, 2, 9), Some((2, 10)));
        let two = [
            EditRegion { start: 2, old_end: 3, new_len: 4 },
            EditRegion { start: 6, old_end: 7, new_len: 0 },
            EditRegion { start: 12, old_end: 13, new_len: 2 },
        ];
        assert_eq!(map_through_regions(&two, 1, 9), Some((1, 11)));
        assert_eq!(map_through_regions(&two, 4, 10), Some((7, 12)));
        assert_eq!(map_through_regions(&two, 1, 12), None);
        assert_eq!(map_through_regions(&two, 1, 14), Some((1, 17)));
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
        // The text path over a spliced candidate: a clean result yields no
        // diagnostics. The gate itself is tested over the protocol
        // (`apply_edit_refuses_what_glyph_check_refuses`).
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

    #[test]
    fn let_annotation_mismatch_produces_a_diagnostic() {
        // G149: the LSP has no tsc pass of its own (`analyze` runs parse ->
        // resolve -> assign_types only), so this class of error is invisible
        // here unless the typechecker itself raises it. A `let` annotation
        // that disagrees with its initializer must show up over stdio, not
        // just from `glyph check`'s tsc-backed TS2322.
        let src = "fn a() -> void {\n  let x: string = \"a\"\n  print(x)\n}\n";
        assert!(analyze(src).is_empty(), "sanity: matching annotation is clean");
        let src = "fn a() -> void {\n  let x: string = 42\n  print(x)\n}\n";
        assert!(!analyze(src).is_empty(), "mismatched let annotation must be flagged");
    }

    #[test]
    fn inlay_hints_show_inferred_let_types() {
        let text = "module m\nfn f() -> number {\n  let x = 1\n  let y: number = 2\n  return x + y\n}\n";
        let a = analysis::analyze_full(text).expect("analyze");
        let hints = a.inlay_type_hints(text);
        // `x` (untyped) gets a hint; `y` (annotated) does not.
        assert_eq!(hints.len(), 1, "one untyped let: {hints:?}");
        let (offset, label) = &hints[0];
        assert_eq!(label, ": number");
        // The hint sits right after the `x` name.
        assert_eq!(&text[..*offset], "module m\nfn f() -> number {\n  let x");
    }

    #[test]
    fn inlay_hints_show_each_destructured_name_its_element_type() {
        // D48: one hint per name, after that name, with its element's type. A
        // name over an element the checker cannot see gets none.
        let text = "module m\nfn f() -> number {\n  let [n, s, _] = [1, \"a\", true]\n  print(s)\n  return n\n}\n";
        let a = analysis::analyze_full(text).expect("analyze");
        let hints = a.inlay_type_hints(text);
        let at: Vec<(&str, &str)> = hints
            .iter()
            .map(|(o, l)| (&text[o - 1..*o], l.as_str()))
            .collect();
        assert_eq!(at, [("n", ": number"), ("s", ": string")], "{hints:?}");
    }

    #[test]
    fn hover_rename_and_references_reach_a_destructured_name() {
        // Each name in a pattern is a real `let` binding: hover answers its
        // element type, and rename edits the def site and every use.
        let text = "module m\nfn f() -> number {\n  let [count, label] = [1, \"a\"]\n  print(label)\n  return count + count\n}\n";
        let a = analysis::analyze_full(text).expect("analyze");
        let def = text.find("count").unwrap();
        assert_eq!(a.hover(text, def).as_deref(), Some("number"));
        let edits = a.rename_edits(def, text, "total").expect("renameable");
        assert_eq!(edits.len(), 3, "def and two uses: {edits:?}");
        let refs = a.references(text.rfind("count").unwrap(), text, true);
        assert_eq!(refs.len(), 3, "{refs:?}");
        assert!(refs.contains(&(def as u32, (def + "count".len()) as u32)), "{refs:?}");
    }

    #[test]
    fn unused_import_drives_a_line_deletion_quick_fix() {
        // The unused-import diagnostic (E0106) plus line_span is what the code
        // action turns into a "delete this line" edit.
        let text = "module m\nimport std/io\nimport std/string\nfn main() -> void {\n  io.println(\"x\")\n}\n";
        let diags = analyze(text);
        let unused: Vec<_> = diags.iter().filter(|d| d.code == "E0106").collect();
        assert!(!unused.is_empty(), "expected an E0106 unused-import diagnostic");
        // The unused one is `std/string`; deleting its line drops exactly it.
        let d = unused.iter().find(|d| text[d.start as usize..d.end as usize].contains("string")).expect("string import diag");
        let (ls, le) = line_span(text, d.start as usize);
        assert_eq!(&text[ls..le], "import std/string\n", "got {:?} (span {}..{})", &text[ls..le], d.start, d.end);
    }
}
