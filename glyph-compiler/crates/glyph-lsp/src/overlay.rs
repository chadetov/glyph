//! The overlay: what text the server analyses, and where that text comes from.
//!
//! A language server answers about a file the user is editing, which is not the
//! file on disk. The rule here is one sentence: **the editor's buffer for a
//! file it has open, the file on disk for everything else.** `didOpen` and
//! `didChange` write the buffer, `didClose` drops it and disk becomes the truth
//! again, and every read goes through [`Workspace::overlay_text`] so no query
//! can pick the other one by accident.
//!
//! Underneath the overlay is the compiler's own incremental model, one
//! [`CompilerDb`] for the server's lifetime. Each file the server has looked at
//! has a `SourceFile` input in it; putting the overlay's text into that input
//! is the only write, and salsa decides from there what has to run again. That
//! is the whole of the change: the server used to hand a bare `&str` to the
//! front end on every request, so a hover after a keystroke re-parsed,
//! re-resolved and re-typechecked text it had just finished analysing.
//!
//! **What it costs, measured rather than assumed.** A second request over an
//! unchanged buffer stops analysing anything: on a 2,205-line file, hover after
//! a keystroke goes from 15.6 ms to about 1 ms of protocol work, a repeated
//! workspace-wide references over an 11-file project from 13.5 ms to 1.3 ms,
//! and an editor burst (a keystroke, then hover, definition and document
//! symbols) from 61.8 ms to 37.1 ms.
//!
//! The keystroke itself got slower, from 15.6 ms to 33.4 ms on the same file,
//! and the cause is one query rather than the design: `glyph_db::type_map`
//! takes 21.2 ms where `assign_types` over the same text takes 4.2 ms, and
//! 16.2 ms of that gap is the per-declaration layer `typed_file` drives
//! (`decl_ast` clones every declaration and copies its source bytes,
//! `resolved_decl` clones a sliced `ResolvedModule` for each). That layer earns
//! its keep in a build, where its memos are read across files and revisions;
//! on a keystroke it is paid in full and saves at most the 4.2 ms of assignment
//! it wraps. Fixing it belongs in `glyph-db`, not here.
//!
//! Two things this deliberately is not.
//!
//! It is not the MCP server's database. That one is keyed on disk truth and
//! answers an agent; this one is keyed on buffer truth and answers an editor.
//! They are peers over one compiler model, and neither is built on the other:
//! a `SourceFile` holds one text, and an unsaved keystroke is not a question a
//! disk-backed store can answer.
//!
//! It is not a project graph. The server's analysis sees exactly what it saw
//! before, one file at a time, because `ProjectFiles` is left empty: with no
//! entries the cross-module resolvers answer `None`, which is what the
//! text path did too. Making the editor's diagnostics project-aware is a
//! change to what the server *knows*, and this one is about what it *costs*.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glyph_db::{CompilerDb, SourceFile};
use glyph_resolver::{build_prelude, ModuleGraph, StdlibStubs};
use tower_lsp::lsp_types::Url;

use crate::analysis::{analysis_in, analyze_in, module_outline, Analysis, GlyphDiagnostic};
use crate::collect_glyph_files;

/// The editor's buffers and the incremental model they feed.
///
/// Memory: the database keeps every memo it has computed, so the cost scales
/// with the source the server has actually analysed, at roughly 35x its size
/// (the figure the MCP server measured: 39.5 MB of memo for 1.1 MB of
/// `.glyph`). Nothing is evicted. A workspace-wide rename is what loads the
/// whole tree; a session that only edits a handful of files only ever holds
/// those.
pub(crate) struct Workspace {
    db: CompilerDb,
    /// The stdlib stub graph, shared between the database's module graph and
    /// the import verification each analysis runs. Built once, because it is
    /// the same graph on every keystroke.
    stdlib: Arc<StdlibStubs>,
    /// The editor's buffers, by document URI. Authoritative over disk for as
    /// long as the editor says the file is open.
    open: HashMap<Url, String>,
    /// One salsa input per file the server has read, reused across edits so a
    /// write of unchanged bytes stays free and a memo survives.
    files: HashMap<Url, SourceFile>,
}

impl Workspace {
    pub(crate) fn new() -> Self {
        let stdlib = Arc::new(StdlibStubs::new());
        let graph: Arc<dyn ModuleGraph + Send + Sync> = stdlib.clone();
        Workspace {
            db: CompilerDb::new(build_prelude(), graph),
            stdlib,
            open: HashMap::new(),
            files: HashMap::new(),
        }
    }

    /// The same workspace, over a database that reports every salsa event to
    /// `sink`. Tests use it to prove a second request over an unchanged buffer
    /// executes nothing; there is no other way to observe the runtime from
    /// outside `glyph-db`.
    #[cfg(test)]
    fn with_event_sink(sink: glyph_db::EventSink) -> Self {
        let stdlib = Arc::new(StdlibStubs::new());
        let graph: Arc<dyn ModuleGraph + Send + Sync> = stdlib.clone();
        Workspace {
            db: CompilerDb::with_event_sink(build_prelude(), graph, sink),
            stdlib,
            open: HashMap::new(),
            files: HashMap::new(),
        }
    }

    /// Record the editor's buffer for `uri`.
    ///
    /// This and [`Workspace::close`] are the only writers of the overlay, and
    /// they write nothing but the map. Putting text into the model is
    /// [`Workspace::sync`]'s job, and every read goes through it first, so
    /// there is one place where the model can disagree with the editor and it
    /// is the place every answer passes through.
    pub(crate) fn set_buffer(&mut self, uri: &Url, text: String) {
        self.open.insert(uri.clone(), text);
    }

    /// Forget the editor's buffer for `uri`: the file on disk is the truth
    /// again, and the next read picks it up.
    pub(crate) fn close(&mut self, uri: &Url) {
        self.open.remove(uri);
    }

    /// The open buffer's text, or `None` for a file the editor does not have
    /// open. The per-document requests answer only for open documents, which
    /// is what this returning `None` means at their call sites.
    pub(crate) fn buffer(&self, uri: &Url) -> Option<String> {
        self.open.get(uri).cloned()
    }

    /// Whether the editor has this document open.
    pub(crate) fn is_open(&self, uri: &Url) -> bool {
        self.open.contains_key(uri)
    }

    /// [`Workspace::view`], restricted to a document the editor has open.
    ///
    /// Every per-document request goes through this rather than through `view`,
    /// because answering a hover about a file nobody opened out of its bytes on
    /// disk is not what the request asked. The disk half of the overlay is for
    /// the workspace-wide queries, which do range over files the editor never
    /// opened.
    pub(crate) fn open_view(&mut self, uri: &Url) -> Option<(&str, Analysis)> {
        if !self.is_open(uri) {
            return None;
        }
        self.view(uri)
    }

    /// The text the overlay says `uri` holds: the editor's buffer when it has
    /// the file open, the bytes on disk otherwise. `None` when the editor does
    /// not have it open and it cannot be read (it was deleted, or the URI does
    /// not name a file at all).
    fn overlay_text(&self, uri: &Url) -> Option<String> {
        if let Some(text) = self.open.get(uri) {
            return Some(text.clone());
        }
        let path = uri.to_file_path().ok()?;
        std::fs::read_to_string(path).ok()
    }

    /// Bring `uri`'s input in line with the overlay and hand back its handle.
    ///
    /// Writing text that is byte-identical to what the input already holds is
    /// free (`set_file_text` compares first), so a request that changes nothing
    /// invalidates nothing. A file that has neither a buffer nor readable bytes
    /// is dropped rather than left holding what it used to say.
    fn sync(&mut self, uri: &Url) -> Option<SourceFile> {
        let Some(text) = self.overlay_text(uri) else {
            self.files.remove(uri);
            return None;
        };
        match self.files.get(uri) {
            Some(&file) => {
                self.db.set_file_text(file, text);
                Some(file)
            }
            None => {
                let name = uri
                    .to_file_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| uri.to_string());
                let file = SourceFile::new(&self.db, name, text);
                self.files.insert(uri.clone(), file);
                Some(file)
            }
        }
    }

    /// The diagnostics for `uri` and the text they are in coordinates of.
    pub(crate) fn diagnostics(&mut self, uri: &Url) -> Option<(&str, Vec<GlyphDiagnostic>)> {
        let file = self.sync(uri)?;
        let diagnostics = analyze_in(&self.db, file, &self.stdlib);
        Some((file.source_text(&self.db).as_str(), diagnostics))
    }

    /// The analysis of `uri` and the text its spans index into. `None` when the
    /// document does not parse or its symbols do not collect.
    pub(crate) fn view(&mut self, uri: &Url) -> Option<(&str, Analysis)> {
        let file = self.sync(uri)?;
        let analysis = analysis_in(&self.db, file)?;
        Some((file.source_text(&self.db).as_str(), analysis))
    }

    /// The document outline of `uri`, from the memoized parse.
    pub(crate) fn outline(&mut self, uri: &Url) -> Option<(&str, Vec<crate::analysis::OutlineSymbol>)> {
        let file = self.sync(uri)?;
        let parsed = glyph_db::parse_module(&self.db, file);
        let outline = module_outline(parsed.module()?);
        Some((file.source_text(&self.db).as_str(), outline))
    }

    /// Every `.glyph` document in the workspace, in the order a walk of `root`
    /// finds them, followed by any open document the walk did not reach (a new
    /// file that has never been saved). The per-file input to a workspace-wide
    /// references or rename.
    ///
    /// Only the URIs: each one is read through the overlay when the caller asks
    /// for it, so a file the editor has open answers with the unsaved buffer
    /// and every other file answers with what is on disk.
    pub(crate) fn workspace_docs(&self, root: &Path) -> Vec<Url> {
        let mut files: Vec<PathBuf> = Vec::new();
        collect_glyph_files(root, &mut files);
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for path in files {
            let Ok(uri) = Url::from_file_path(&path) else {
                continue;
            };
            seen.insert(uri.clone());
            out.push(uri);
        }
        for uri in self.open.keys() {
            if !seen.contains(uri) {
                out.push(uri.clone());
            }
        }
        out
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A clean two-declaration module, and the same module with the annotation
    /// on `x` broken. The second is what a keystroke turns the first into.
    const CLEAN: &str = "module m\nfn f() -> void {\n  let x: string = \"a\"\n  print(x)\n}\n";
    const BROKEN: &str = "module m\nfn f() -> void {\n  let x: string = 42\n  print(x)\n}\n";

    fn tmp_dir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("glyph_overlay_{name}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Write `text` to `dir/name.glyph` and return its URI.
    fn on_disk(dir: &Path, name: &str, text: &str) -> Url {
        let path = dir.join(format!("{name}.glyph"));
        std::fs::write(&path, text).expect("write");
        Url::from_file_path(&path).expect("uri")
    }

    /// The text the model holds for `uri`, which is what the overlay put
    /// there. `None` when the overlay has no text for it at all.
    fn model_text(ws: &mut Workspace, uri: &Url) -> Option<String> {
        let file = ws.sync(uri)?;
        Some(file.source_text(&ws.db).clone())
    }

    /// The diagnostic codes for `uri`, in order.
    fn codes(ws: &mut Workspace, uri: &Url) -> Vec<String> {
        ws.diagnostics(uri)
            .map(|(_, ds)| ds.into_iter().map(|d| d.code).collect())
            .unwrap_or_default()
    }

    /// The editor's truth is the buffer it just sent, not the one before it.
    ///
    /// This is the failure the overlay exists to make impossible: a request
    /// that lands after a `didChange` must be answered from the text that
    /// change carried. A server that analysed a stale buffer would still pass
    /// every other test here, because every other test only ever writes once.
    #[test]
    fn a_request_after_a_change_sees_the_new_buffer() {
        let dir = tmp_dir("change");
        let uri = on_disk(&dir, "m", CLEAN);
        let mut ws = Workspace::new();

        ws.set_buffer(&uri, CLEAN.to_string());
        assert!(codes(&mut ws, &uri).is_empty(), "the clean buffer is clean");

        ws.set_buffer(&uri, BROKEN.to_string());
        let after = codes(&mut ws, &uri);
        assert!(!after.is_empty(), "the changed buffer must be re-analysed");
        let (text, _) = ws.view(&uri).expect("analysis");
        assert_eq!(text, BROKEN, "the analysis is of the text the editor sent");

        // And back: an undo is a change like any other.
        ws.set_buffer(&uri, CLEAN.to_string());
        assert!(codes(&mut ws, &uri).is_empty(), "the undo is re-analysed too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file the editor never opened is read from disk.
    ///
    /// The workspace-wide queries range over files nobody has open, and this is
    /// where the overlay's other half lives.
    #[test]
    fn a_file_the_editor_never_opened_is_read_from_disk() {
        let dir = tmp_dir("disk");
        let uri = on_disk(&dir, "m", BROKEN);
        let mut ws = Workspace::new();

        assert!(!ws.is_open(&uri), "nobody opened it");
        assert_eq!(model_text(&mut ws, &uri).as_deref(), Some(BROKEN), "read from disk");
        assert!(
            !codes(&mut ws, &uri).is_empty(),
            "the bytes on disk are what gets analysed"
        );
        assert!(
            ws.open_view(&uri).is_none(),
            "a per-document request still answers only for an open document"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// While the editor holds a file open, its buffer outranks the bytes on
    /// disk; when it closes it, disk is the truth again.
    #[test]
    fn the_open_buffer_outranks_the_file_on_disk() {
        let dir = tmp_dir("both");
        let uri = on_disk(&dir, "m", BROKEN);
        let mut ws = Workspace::new();

        // Unsaved edits fix the file: the buffer is clean, disk is not.
        ws.set_buffer(&uri, CLEAN.to_string());
        assert_eq!(model_text(&mut ws, &uri).as_deref(), Some(CLEAN));
        assert!(codes(&mut ws, &uri).is_empty(), "the buffer is what counts");

        ws.close(&uri);
        assert_eq!(model_text(&mut ws, &uri).as_deref(), Some(BROKEN), "disk again");
        assert!(!codes(&mut ws, &uri).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is neither open nor readable answers nothing, rather than
    /// answering with what it used to say.
    #[test]
    fn a_deleted_file_stops_answering() {
        let dir = tmp_dir("gone");
        let uri = on_disk(&dir, "m", CLEAN);
        let mut ws = Workspace::new();
        assert_eq!(model_text(&mut ws, &uri).as_deref(), Some(CLEAN));

        std::fs::remove_file(uri.to_file_path().unwrap()).expect("delete");
        assert_eq!(model_text(&mut ws, &uri), None);
        assert!(ws.view(&uri).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The claim the release is about: a second request over an unchanged
    /// buffer runs no compiler work at all.
    ///
    /// `WillExecute` is the only salsa event that means a query body ran. The
    /// first analysis executes plenty; the second must execute none. Without
    /// this the overlay could be re-running everything and every other test
    /// here would still pass, which is exactly how a cache with a permanent
    /// zero hit rate ships.
    #[test]
    fn a_second_request_over_an_unchanged_buffer_executes_nothing() {
        let dir = tmp_dir("memo");
        let uri = on_disk(&dir, "m", CLEAN);
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&log);
        let sink: glyph_db::EventSink = Arc::new(move |event: &glyph_db::Event| {
            if let glyph_db::EventKind::WillExecute { database_key } = &event.kind {
                recorder.lock().unwrap().push(format!("{database_key:?}"));
            }
        });
        let mut ws = Workspace::with_event_sink(sink);

        ws.set_buffer(&uri, CLEAN.to_string());
        let _ = ws.diagnostics(&uri);
        let first: Vec<String> = std::mem::take(&mut *log.lock().unwrap());
        assert!(
            first.iter().any(|q| q.contains("parse_module")),
            "the first analysis parses: {first:?}"
        );

        // Everything an editor fires after a keystroke, over the same buffer.
        let _ = ws.view(&uri);
        let _ = ws.outline(&uri);
        let _ = ws.diagnostics(&uri);
        let second: Vec<String> = std::mem::take(&mut *log.lock().unwrap());
        assert!(
            second.is_empty(),
            "a repeat request must execute no query, ran: {second:?}"
        );

        // A real edit does execute again, so the assertion above is a live
        // instrument rather than a claim about an inert database.
        ws.set_buffer(&uri, BROKEN.to_string());
        let _ = ws.diagnostics(&uri);
        let third: Vec<String> = std::mem::take(&mut *log.lock().unwrap());
        assert!(!third.is_empty(), "an edit re-executes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writing the buffer the editor already sent changes nothing, so it
    /// invalidates nothing. `didChange` can arrive with identical text (an
    /// editor re-sending on save, a formatter that changed nothing), and a
    /// write of unchanged bytes opens a salsa revision that would re-execute
    /// every query in the file.
    #[test]
    fn re_sending_the_same_buffer_executes_nothing() {
        let dir = tmp_dir("same");
        let uri = on_disk(&dir, "m", CLEAN);
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&log);
        let sink: glyph_db::EventSink = Arc::new(move |event: &glyph_db::Event| {
            if let glyph_db::EventKind::WillExecute { database_key } = &event.kind {
                recorder.lock().unwrap().push(format!("{database_key:?}"));
            }
        });
        let mut ws = Workspace::with_event_sink(sink);

        ws.set_buffer(&uri, CLEAN.to_string());
        let _ = ws.diagnostics(&uri);
        log.lock().unwrap().clear();

        ws.set_buffer(&uri, CLEAN.to_string());
        let _ = ws.diagnostics(&uri);
        let again: Vec<String> = std::mem::take(&mut *log.lock().unwrap());
        assert!(again.is_empty(), "identical text re-executed: {again:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The overlay's file list is the walk plus the buffers the walk missed.
    #[test]
    fn an_unsaved_document_is_still_a_workspace_document() {
        let dir = tmp_dir("docs");
        let saved = on_disk(&dir, "saved", CLEAN);
        let unsaved = Url::from_file_path(dir.join("unsaved.glyph")).expect("uri");
        let mut ws = Workspace::new();
        ws.set_buffer(&unsaved, CLEAN.to_string());

        let docs = ws.workspace_docs(&dir);
        assert!(docs.contains(&saved), "the file on disk: {docs:?}");
        assert!(docs.contains(&unsaved), "the buffer with no file: {docs:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The two ways into the front end must not drift apart.
    ///
    /// `analyze` runs it over a bare string for a caller with no database;
    /// `analyze_in` reads the same stages out of one. They assemble the list
    /// through a single function, and this is what holds them to that: an
    /// editor and an agent looking at identical text see identical
    /// diagnostics, code for code and span for span.
    #[test]
    fn the_text_path_and_the_incremental_path_agree() {
        let sources: [&str; 6] = [
            CLEAN,
            BROKEN,
            // A parse failure.
            "module m\nfn f( -> void {\n}\n",
            // A duplicate declaration: symbol collection fails.
            "module m\nfn f() -> void {\n}\nfn f() -> void {\n}\n",
            // An unused import: the warning tier, which only runs when nothing
            // else fired.
            "module m\nimport std/io\nimport std/string\nfn main() -> void {\n  io.println(\"x\")\n}\n",
            // An unknown name: a resolution error.
            "module m\nfn f() -> void {\n  print(nope)\n}\n",
        ];
        let dir = tmp_dir("agree");
        let mut ws = Workspace::new();
        for (i, src) in sources.iter().enumerate() {
            let uri = on_disk(&dir, &format!("s{i}"), src);
            ws.set_buffer(&uri, (*src).to_string());
            let (_, incremental) = ws.diagnostics(&uri).expect("diagnostics");
            let text_path = crate::analysis::analyze(src);
            let render = |ds: &[GlyphDiagnostic]| -> Vec<(String, u32, u32, String)> {
                ds.iter()
                    .map(|d| (d.code.clone(), d.start, d.end, d.message.clone()))
                    .collect()
            };
            assert_eq!(
                render(&incremental),
                render(&text_path),
                "source {i} disagrees between the two paths"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
