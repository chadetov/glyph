//! A minimal Model Context Protocol (MCP) server exposing Glyph's language
//! analysis to a coding agent as tools. It speaks JSON-RPC 2.0 over stdio with
//! newline-delimited messages (the MCP stdio transport), and reuses the pure
//! `crate::analysis` queries (hover, go-to-definition, workspace references,
//! symbol search, diagnostics) so the agent surface is a thin adapter over the
//! same semantics the editor path uses, not a second implementation. One tool,
//! `glyph_variants`, has no editor counterpart: it answers from the salsa
//! match-coverage relation, which is a project-wide query rather than a
//! position in a buffer.
//!
//! Unlike the language server, this one keeps a salsa database per project
//! (see `Server`), so a repeated whole-project query costs a directory walk
//! instead of re-parsing every file. The two servers share the analysis
//! *functions* but not the database, because they disagree about what is
//! authoritative: the editor's truth is the unsaved buffer it sent in
//! `didChange`, this server's is what is on disk, and a `SourceFile` holds one
//! text. Handing disk to the language server would overwrite a dirty buffer
//! with stale bytes.
//!
//! Positions are LSP-style: 0-based `line` and a 0-based UTF-16 `character`.
//! Paths in tool arguments are relative to the project root (or absolute); paths
//! in results are reported relative to the root when possible.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use glyph_db::{
    project_match_coverage, CompilerDb, CoverageTypeRef, DeclIndex, EventSink,
    ProjectCoverageSite, Setter, SourceFile,
};
use glyph_resolver::{
    build_prelude, collect_module_symbols, resolve_module, DeclKey, Prelude, ResolvedModule,
    StdlibStubs, SymbolKind,
};
use glyph_typechecker::{CoverageSiteRef, CoverageState, CoverageTypeName};

use crate::analysis::{
    analyze, analyze_full, global_occurrences_in, module_outline, outline_of, references_at,
    symbol_target_at, Definition, LineIndex, OutlineKind, OutlineSymbol, SymbolTarget,
};
use crate::{collect_glyph_files, module_path_of};

/// The MCP protocol revision this server implements.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// How many project databases stay live at once, least-recently-used evicted.
///
/// A database retains roughly 35x the source it analyzed (39.5 MB of memo for
/// 1.1 MB of `.glyph` on the examples tree) and `glyph-db` evicts nothing on
/// its own, so an unbounded map is a leak in a server that runs for as long as
/// the agent session does.
const MAX_LIVE_DATABASES: usize = 4;

/// One file the database knows about: its salsa input and the module path that
/// names it inside its project.
struct ProjectFile {
    module_path: String,
    file: SourceFile,
}

/// One project's incremental database, plus the disk state it was built from.
///
/// Keyed by *project* root rather than by the server's root because module
/// paths are counted per project (D41): merging two projects into one
/// `ProjectFiles` would merge their module namespaces, so a sibling `lib` in
/// one project would answer an `import lib` in the other.
struct Project {
    root: PathBuf,
    db: CompilerDb,
    /// The project's members: absolute path → the database's handle, for every
    /// `.glyph` file the directory walk reaches. The walk is what defines
    /// membership, and this set is what every query answers from.
    files: BTreeMap<PathBuf, ProjectFile>,
    /// The file the current call asked about, when the walk does not reach it.
    ///
    /// A `.glyph` file under a dot directory, `target/`, or `node_modules/` is
    /// not a member of the project. Answering a question asked *about* one of
    /// them from its own contents is fine, since the caller named it; making it
    /// a member is not. It used to be force-inserted into `files`, where it
    /// stayed for the life of the database, so a later question about a
    /// *different* file answered differently for having been asked — and
    /// answered differently again once the LRU evicted the database and the
    /// walk rebuilt it without the intruder. This slot holds one file, is
    /// rewritten by every refresh, and never reaches `files` or `entries`.
    outsider: Option<(PathBuf, ProjectFile)>,
    /// The entry list last pushed to the database, so `set_project` fires only
    /// when the set actually changed (see `refresh`).
    entries: Vec<(String, SourceFile)>,
}

impl Project {
    fn new(root: PathBuf, sink: Option<&EventSink>) -> Self {
        let db = match sink {
            Some(sink) => CompilerDb::with_event_sink(
                build_prelude(),
                Arc::new(StdlibStubs::new()),
                Arc::clone(sink),
            ),
            None => CompilerDb::with_default_stdlib(),
        };
        Self {
            root,
            db,
            files: BTreeMap::new(),
            outsider: None,
            entries: Vec::new(),
        }
    }

    /// Bring the database back in line with what is on disk, then hand back a
    /// read-only view of it.
    ///
    /// Every `.glyph` file under the project root is re-read and compared, on
    /// every call. There is no watcher and no mtime heuristic: the walk costs
    /// about 4 ms warm and the reads about 5 ms against a query that used to
    /// cost 169 ms, and the walk is also what notices a file that did not exist
    /// when the server started.
    ///
    /// The candidate set is the walk, plus the members the database already
    /// holds so a file that has been deleted is noticed and dropped. `target`
    /// is not in it: when the walk does not reach `target` it is loaded into
    /// `outsider` instead, for this call only.
    ///
    /// **The comparison is the whole point.** salsa 0.28 does not backdate an
    /// input write: `set_text` with byte-identical text still opens a new
    /// revision and forces every dependent query to re-execute. A refresh that
    /// wrote unconditionally would miss on every call forever while looking
    /// exactly like a cache, and would be slower than the code it replaced,
    /// because it would pay the reads on top of the full analysis. The same
    /// applies to `set_project`, which is written only when the entry set
    /// changes.
    fn refresh(&mut self, target: &Path) {
        let mut walked = Vec::new();
        collect_glyph_files(&self.root, &mut walked);
        let mut candidates: BTreeSet<PathBuf> = walked.into_iter().collect();
        // Members the database already holds are re-read too, so a file the
        // walk no longer reaches is re-checked rather than left frozen.
        candidates.extend(self.files.keys().cloned());

        let mut next: BTreeMap<PathBuf, ProjectFile> = BTreeMap::new();
        for path in candidates {
            let Some(module_path) = module_path_of(&self.root, &path) else {
                continue;
            };
            // An unreadable path is a deleted (or never-present) file: leaving
            // it out of `next` is what drops it from the entry list.
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let existing = self.files.remove(&path);
            next.insert(path, load(&mut self.db, existing, module_path, text));
        }
        self.files = next;
        self.load_outsider(target);

        let entries: Vec<(String, SourceFile)> = self
            .files
            .values()
            .map(|f| (f.module_path.clone(), f.file))
            .collect();
        if entries != self.entries {
            self.db.set_project(entries.clone());
            self.entries = entries;
        }
    }

    /// Load `target` into the `outsider` slot when the walk does not reach it,
    /// so the question asked about it can be answered from its own contents.
    ///
    /// The slot is rewritten on every refresh, so a non-member file is readable
    /// exactly while it is the file being asked about. It is deliberately kept
    /// out of `entries`: a non-member must not answer another module's
    /// `import`, any more than it should turn up in another file's references.
    fn load_outsider(&mut self, target: &Path) {
        if self.files.contains_key(target) {
            // A member. The slot must not hold a second handle on the same
            // path, or every occurrence in it would be reported twice.
            self.outsider = None;
            return;
        }
        let Some(module_path) = module_path_of(&self.root, target) else {
            self.outsider = None;
            return;
        };
        let Ok(text) = std::fs::read_to_string(target) else {
            self.outsider = None;
            return;
        };
        // Reuse the handle when the same file is asked about again, so a repeat
        // call still writes nothing and executes no queries.
        let existing = match self.outsider.take() {
            Some((p, pf)) if p == target => Some(pf),
            _ => None,
        };
        let file = load(&mut self.db, existing, module_path, text);
        self.outsider = Some((target.to_path_buf(), file));
    }

    /// Every file this call may read, in path order: the project's members,
    /// plus the queried file when the walk does not reach it.
    fn searched(&self) -> Vec<(&Path, &ProjectFile)> {
        let mut files: Vec<(&Path, &ProjectFile)> = self
            .files
            .iter()
            .map(|(p, f)| (p.as_path(), f))
            .collect();
        if let Some((p, f)) = &self.outsider {
            files.push((p.as_path(), f));
            files.sort_by(|a, b| a.0.cmp(b.0));
        }
        files
    }
}

/// Put `text` into the database under `module_path`, reusing `existing`'s salsa
/// input when the file already had one.
///
/// The inequality is load-bearing: salsa 0.28 does not backdate an input write,
/// so `set_text` with byte-identical text still opens a new revision and forces
/// every dependent query to re-execute.
fn load(
    db: &mut CompilerDb,
    existing: Option<ProjectFile>,
    module_path: String,
    text: String,
) -> ProjectFile {
    match existing {
        Some(existing) => {
            if existing.file.text(&*db) != &text {
                existing.file.set_text(db).to(text);
            }
            existing
        }
        None => {
            let file = SourceFile::new(&*db, module_path.clone(), text);
            ProjectFile { module_path, file }
        }
    }
}

/// The MCP server's state across calls: the root it was started on and the
/// live project databases, most-recently-used first.
pub struct Server {
    root: PathBuf,
    projects: Vec<Project>,
    /// Set only by tests, which use it to prove a repeat call executed no
    /// queries. `None` in the shipped server, where salsa installs no callback.
    sink: Option<EventSink>,
}

/// The server root, canonicalized once so every later comparison has both sides
/// in the same spelling.
///
/// `read_file` canonicalizes the path it is handed, and a project root is found
/// by walking up from that path, so a root left in its original spelling
/// compares against something it can never match. On macOS this is not an edge
/// case: a temporary directory under `/var` canonicalizes to `/private/var`, so
/// every membership test fails and every file becomes a non-member.
///
/// Falls back to the given path when it cannot be resolved, because a root that
/// does not exist yet is the caller's problem to report, not this function's to
/// panic on.
fn canonical_root(root: PathBuf) -> PathBuf {
    std::fs::canonicalize(&root).unwrap_or(root)
}

impl Server {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: canonical_root(root),
            projects: Vec::new(),
            sink: None,
        }
    }

    /// A server whose databases report every salsa event to `sink`.
    #[cfg(test)]
    fn with_event_sink(root: PathBuf, sink: EventSink) -> Self {
        Self {
            root: canonical_root(root),
            projects: Vec::new(),
            sink: Some(sink),
        }
    }

    /// The database for the project rooted at `project_root`, refreshed from
    /// disk and moved to the front of the LRU. `target` is the file the caller
    /// is asking about; it is guaranteed to be in the returned database when it
    /// is readable and under the project root.
    fn project(&mut self, project_root: &Path, target: &Path) -> &Project {
        match self.projects.iter().position(|p| p.root == project_root) {
            Some(i) => {
                let project = self.projects.remove(i);
                self.projects.insert(0, project);
            }
            None => {
                let project = Project::new(project_root.to_path_buf(), self.sink.as_ref());
                self.projects.insert(0, project);
                self.projects.truncate(MAX_LIVE_DATABASES);
            }
        }
        let project = &mut self.projects[0];
        project.refresh(target);
        project
    }
}

/// Run the MCP server over stdio until stdin closes. `root` is the project root
/// used for workspace queries (references, symbols) and to resolve relative file
/// paths in tool arguments.
pub fn run_stdio(root: PathBuf) {
    let mut server = Server::new(root);
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    // The stdio transport frames each JSON-RPC message as one line.
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(resp) = handle(&req, &mut server) {
            // `to_string` escapes any newline inside a string, so the message is
            // a single line as the transport requires.
            let s = serde_json::to_string(&resp).unwrap_or_default();
            if writeln!(out, "{s}").is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}

/// Dispatch one JSON-RPC message. Returns the response for a request, or `None`
/// for a notification (no `id`) or a message we do not answer.
fn handle(req: &Value, server: &mut Server) -> Option<Value> {
    let method = req.get("method")?.as_str()?;
    let id = req.get("id").cloned();
    match method {
        "initialize" => Some(ok(id?, initialize_result())),
        "tools/list" => Some(ok(id?, json!({ "tools": tool_specs() }))),
        "tools/call" => {
            let id = id?;
            let params = req.get("params").cloned().unwrap_or(Value::Null);
            let (text, is_error) = match call_tool(&params, server) {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            Some(ok(
                id,
                json!({ "content": [text_content(&text)], "isError": is_error }),
            ))
        }
        // `notifications/initialized` and any other notification: no response.
        _ => id.map(|id| err(id, -32601, "method not found")),
    }
}

/// What the client is told about this server before it calls anything.
///
/// The `instructions` string is the only channel that reaches an agent without
/// it choosing to read something first: clients put it in the model's context
/// at connect time. That matters more than it sounds. `glyph --update` has been
/// in the bootstrap since it shipped, and the next agent that needed to update
/// Glyph still reached for npm, because nothing put the command in front of it
/// at the moment it was deciding. Documentation that has to be found does not
/// carry; this does.
///
/// Keep it short. It is spent from every conversation's budget whether or not
/// the agent uses a tool.
fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "glyph-mcp", "version": env!("CARGO_PKG_VERSION") },
        "instructions": concat!(
            "Glyph is a statically typed language that compiles to TypeScript. ",
            "This server answers from the compiler's own analysis, so prefer it ",
            "over grep for any semantic question: grep finds text that matches, ",
            "the compiler resolved every name to emit the program.\n\n",
            "Before adding or removing a variant of a tagged union, call ",
            "`glyph_variants`. It lists every match site over that type and what ",
            "each one does. Sites it reports as `has_catch_all` keep compiling ",
            "when you add a variant and silently route the new one into `else`, ",
            "which is the dangerous case precisely because nothing fails.\n\n",
            "Run `glyph llms` for the full language reference. It works offline ",
            "and is the same document the compiler ships. Read it before writing ",
            "Glyph rather than inferring the syntax from the TypeScript it ",
            "resembles.\n\n",
            "To update: `glyph --update` moves this installed compiler to the ",
            "newest release. `glyph upgrade` is the different one, moving a ",
            "project's pinned version in package.json. Use these rather than ",
            "installing from npm by hand."
        ),
    })
}

fn tool_specs() -> Value {
    let file = json!({ "type": "string", "description": "Path to a .glyph file, relative to the project root or absolute." });
    let line = json!({ "type": "integer", "description": "0-based line number." });
    let character = json!({ "type": "integer", "description": "0-based character offset (UTF-16 code units)." });
    let name = json!({ "type": "string", "description": "Name of a top-level declaration, a tagged-union variant, or an imported binding in that file. Addresses the symbol itself, so the answer stays about the same symbol when declarations above it are added or removed. A local binding has no name; address one by position." });
    let type_name = json!({ "type": "string", "description": "Name of a tagged union, as the file at `path` names it: one it declares, one it imports, or a prelude or stdlib union (`Result`, `Option`, `fs.ErrorKind`). The module the name resolves to is what picks out one declaration when several modules declare the same name." });
    json!([
        {
            "name": "glyph_diagnostics",
            "description": "Type-check one Glyph file and return its diagnostics (compiler errors and warnings) with stable codes (E0xxx) and source ranges.",
            "inputSchema": { "type": "object", "properties": { "path": file }, "required": ["path"] }
        },
        {
            "name": "glyph_hover",
            "description": "The inferred type of the expression at a position in a Glyph file.",
            "inputSchema": { "type": "object", "properties": { "path": file, "line": line, "character": character }, "required": ["path", "line", "character"] }
        },
        {
            "name": "glyph_definition",
            "description": "Where the name at a position is defined (a file path and range), following imports across modules.",
            "inputSchema": { "type": "object", "properties": { "path": file, "line": line, "character": character }, "required": ["path", "line", "character"] }
        },
        {
            "name": "glyph_references",
            "description": "Every reference to a symbol across the whole project: the declaration, all uses, and each importing module's import binding. Address the symbol by position (`line` and `character`, what an editor has under its cursor) or by `name` (a declaration in that file, which still means the same symbol after the lines above it move). Sending both checks one against the other, and a call whose position and name are different symbols is an error rather than a guess. A local binding is file-scoped and can only be addressed by position.",
            "inputSchema": { "type": "object", "properties": { "path": file, "line": line, "character": character, "name": name }, "required": ["path"] }
        },
        {
            "name": "glyph_variants",
            "description": "Every match site in the project over one tagged union, and which variants each site's arms name. Use it before adding or removing a variant: it is the list of places that have to change. Each site carries the declaration it sits in (as `module::name`), the scrutinee as written in the source, its line, and the arm ordinals with the variant each one names, so you can go to it after the lines around it have moved. `state` says what the compiler concluded, and the four states are not equally safe. `exhaustive`: every variant is named and no arm was skipped, so adding a variant breaks this site and the compiler will point you at it. `has_catch_all`: one of the arms absorbs everything the earlier arms did not name, so adding a variant leaves this site compiling and silently routes the new variant to the catch-all, which is more dangerous than a site that fails to compile because nothing tells you it is now wrong. `declined`: the checker either read an arm it does not model or found variants no arm names, and `missing` lists those. `scrutinee_unresolved`: the scrutinee's type never resolved, so nothing about the site is checked today. A site that reaches this type through a payload rather than as its own scrutinee (`Ok(Some(n))` reaching `Option` inside a match on `Result`) is filed under the type it matches on, so it is listed under `nested` instead, with the depth on each arm and the type it does match on. Those sites break the same way when a variant is added, so read both lists. A union with no declaration in this project (a prelude or stdlib one) is reported under its name with no declaration to go to, and a site whose type this project cannot key is listed under `unkeyed` rather than left out of the answer.",
            "inputSchema": { "type": "object", "properties": { "path": file, "name": type_name }, "required": ["path", "name"] }
        },
        {
            "name": "glyph_symbols",
            "description": "Search the project's top-level declarations (and tagged-union variants) by name substring; an empty query lists them all.",
            "inputSchema": { "type": "object", "properties": { "query": { "type": "string", "description": "Case-insensitive name substring; empty matches everything." } } }
        }
    ])
}

/// Run a `tools/call`. `Ok` is the tool's textual result (JSON we serialize for
/// the agent to parse); `Err` is a human-readable failure that becomes an
/// `isError` result rather than a protocol error.
fn call_tool(params: &Value, server: &mut Server) -> Result<String, String> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("missing tool `name`")?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let root = server.root.clone();
    match name {
        // The three single-file tools answer in well under a millisecond from a
        // fresh parse, so they stay off the database: routing them through it
        // would buy nothing and charge them the project-wide directory walk.
        "glyph_diagnostics" => tool_diagnostics(&args, &root),
        "glyph_hover" => tool_hover(&args, &root),
        "glyph_definition" => tool_definition(&args, &root),
        "glyph_references" => tool_references(&args, server),
        "glyph_variants" => tool_variants(&args, server),
        "glyph_symbols" => tool_symbols(&args, &root),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn tool_diagnostics(args: &Value, root: &Path) -> Result<String, String> {
    let (path, text) = read_file(args, root)?;
    let index = LineIndex::new(&text);
    // The same `module::name` identity `glyph_variants` reports for a match
    // site over a declaration (0.1.107), so an agent can act on a batch of
    // diagnostics by entity instead of re-deriving "which function is this"
    // from a line number that shifts under every unrelated edit above the
    // site. This tool stays off the project database (see the module docs),
    // so the module half is this file's own path rather than a minted
    // `ModuleId` — a pure transform of the path already given to the tool,
    // not a database lookup.
    let file_key = display_path(root, &path);
    let module_str = file_key.strip_suffix(".glyph").unwrap_or(&file_key);
    let items: Vec<Value> = analyze(&text)
        .into_iter()
        .map(|d| {
            let entity = d.decl_name.map(|name| format!("{module_str}::{name}"));
            json!({
                "code": d.code,
                "message": d.message,
                "range": range_json(&index, &text, d.start, d.end),
                "entity": entity,
            })
        })
        .collect();
    Ok(to_json(&items))
}

fn tool_hover(args: &Value, root: &Path) -> Result<String, String> {
    let (_, text) = read_file(args, root)?;
    let (line, character) = position(args)?;
    let Some(a) = analyze_full(&text) else {
        return Ok("null".to_string());
    };
    let offset = LineIndex::new(&text).offset(&text, line, character);
    Ok(to_json(&a.hover(offset)))
}

fn tool_definition(args: &Value, root: &Path) -> Result<String, String> {
    let (path, text) = read_file(args, root)?;
    let (line, character) = position(args)?;
    let Some(a) = analyze_full(&text) else {
        return Ok("null".to_string());
    };
    let offset = LineIndex::new(&text).offset(&text, line, character);
    let value = match a.definition(offset) {
        None => Value::Null,
        Some(Definition::Here(start, _)) => {
            let index = LineIndex::new(&text);
            location_value(FileCtx { path: &path, root, text: &text }, &index, start, start)
        }
        Some(Definition::InModule { module_path, name }) => {
            // The import path is counted from the *importing* file's project
            // root (D41), which is the marked ancestor when there is one and the
            // server's root otherwise.
            let file = crate::project_root_for(&path, root)
                .join(&module_path)
                .with_extension("glyph");
            let ftext = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            let (start, end) = crate::analysis::find_symbol_span(&outline_of(&ftext), &name)
                .ok_or_else(|| format!("`{name}` is not defined in module `{module_path}`"))?;
            let index = LineIndex::new(&ftext);
            location_value(FileCtx { path: &file, root, text: &ftext }, &index, start, end)
        }
    };
    Ok(to_json(&value))
}

/// The global identity of the top-level name `name` in this module, or `None`
/// when the module has no such name.
///
/// This is the by-name half of `symbol_target_at`, and it answers by the same
/// rule: an imported name reports the module that *declares* it, anything else
/// reports the file's own module. `module::name` is the identity every
/// cross-file query here is already keyed by, for the reason `Ty::Imported`
/// gives: a foreign module's symbol ids index an unrelated symbol in the
/// consumer's table.
///
/// The name alone is enough, with no kind beside it, because Glyph's module
/// namespace is flat and single. `fn Foo` beside `type Foo`, or a hoisted
/// variant beside a function of that name, is already rejected as a duplicate
/// declaration, so `module::name` picks out one symbol. `resolved.symbols` is
/// that namespace, which is why a hoisted union variant and an import binding
/// are both nameable and a `let` is not.
///
/// A namespace or aliased import names a module rather than a symbol, so there
/// is no identity to return for one.
fn named_target(resolved: &ResolvedModule, name: &str, this_module: &str) -> Option<SymbolTarget> {
    let sym = resolved.symbols.table.get(resolved.symbols.lookup(name)?)?;
    match &sym.kind {
        SymbolKind::ImportNamed { path, original } => Some(SymbolTarget::Global {
            module: path
                .segments
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<&str>>()
                .join("/"),
            name: original.to_string(),
        }),
        SymbolKind::ImportNamespace { .. }
        | SymbolKind::ImportAlias { .. }
        | SymbolKind::Prelude { .. } => None,
        _ => Some(SymbolTarget::Global {
            module: this_module.to_string(),
            name: sym.name.to_string(),
        }),
    }
}

/// The error for a `name` the module does not declare.
///
/// It lists what the module does declare, because a caller reaching this is
/// holding a name that has since been renamed or moved, and the current list is
/// the useful next step. This is the answer a renamed symbol has to produce:
/// resolving to whatever is nearby instead is the bug a name exists to fix.
fn not_declared(name: &str, this_module: &str, resolved: &ResolvedModule) -> String {
    /// How many names the error lists before it stops. A module with a hundred
    /// declarations should not spend the caller's context on all of them.
    const LISTED: usize = 20;

    // Only names this tool could actually answer about. `by_name` also holds
    // namespace imports, aliased imports and prelude entries, and `named_target`
    // returns nothing for those because they name a module rather than a symbol
    // with an identity. Listing them made the message deny a name and then
    // advertise it: asking for `a` in a module that does `import a` answered
    // "declares no top-level name `a`. It declares: a, also", which sends the
    // caller back to the name that just failed. The list is only useful if
    // everything in it is a name that would work.
    let mut declared: Vec<&str> = resolved
        .symbols
        .by_name
        .keys()
        .filter(|n| named_target(resolved, n.as_ref(), this_module).is_some())
        .map(|n| n.as_ref())
        .collect();
    // `by_name` is a hash map, and one question must get one answer every time
    // it is asked.
    declared.sort_unstable();
    let listed = match declared.len() {
        0 => "nothing".to_string(),
        n if n <= LISTED => declared.join(", "),
        n => format!("{}, and {} more", declared[..LISTED].join(", "), n - LISTED),
    };
    format!("module `{this_module}` declares no top-level name `{name}`. It declares: {listed}")
}

/// The symbol one call is asking about, from whichever address it gave.
///
/// A call that gives both has to have them agree. They disagree exactly when
/// the coordinate has gone stale, which is what a name is there to catch, so
/// the answer is an error naming both sides. Preferring one would hand back a
/// well-formed answer about the caller's other address and leave them nothing
/// to notice, which is the whole failure.
fn resolve_address(
    address: &Address,
    module: &glyph_ast::Module,
    resolved: &ResolvedModule,
    index: &LineIndex,
    text: &str,
    this_module: &str,
) -> Result<Option<SymbolTarget>, String> {
    let at = |line: u32, character: u32| {
        let offset = index.offset(text, line, character);
        symbol_target_at(module, resolved, offset, text, this_module)
    };
    match address {
        Address::Position { line, character } => Ok(at(*line, *character)),
        Address::Name(name) => match named_target(resolved, name, this_module) {
            Some(target) => Ok(Some(target)),
            None => Err(not_declared(name, this_module, resolved)),
        },
        Address::Both {
            line,
            character,
            name,
        } => {
            let by_position = at(*line, *character);
            let by_name = named_target(resolved, name, this_module);
            if by_position == by_name {
                return Ok(by_name);
            }
            let found = |target: &Option<SymbolTarget>| match target {
                None => None,
                Some(SymbolTarget::Local) => Some("a local binding".to_string()),
                Some(SymbolTarget::Global { module, name }) => {
                    Some(format!("`{name}` from module `{module}`"))
                }
            };
            let here = found(&by_position).unwrap_or_else(|| "no name".to_string());
            let there = found(&by_name)
                .unwrap_or_else(|| format!("not a top-level name in module `{this_module}`"));
            Err(format!(
                "the two addresses disagree: line {line}, character {character} is on {here}, \
                 but `name` \"{name}\" is {there}. Send one address, not two that point at \
                 different symbols."
            ))
        }
    }
}

/// The answer when the file cannot be analysed far enough to resolve an
/// address.
///
/// A position addresses nothing when there is no tree to point into, so an
/// empty answer is honest and is what this tool has always given. A name is a
/// different claim: the caller named a symbol, and `[]` would read as "that
/// symbol has no references" when the truth is that we could not look.
fn unresolvable(address: &Address, this_module: &str, why: &str) -> Result<String, String> {
    match address.name() {
        None => Ok("[]".to_string()),
        Some(name) => Err(format!(
            "cannot look up `{name}` in module `{this_module}`: {why}. \
             Run glyph_diagnostics on the file."
        )),
    }
}

/// Every reference to one symbol, across the file's own project.
///
/// The symbol is addressed by a position or by name (see `Address`). Either way
/// the first thing that happens is that the address becomes a
/// `SymbolTarget::Global { module, name }` identity, and nothing downstream
/// sees the address again: the position argument was already only a way to name
/// a symbol, which is why a name can stand in its place without changing
/// anything else here.
///
/// This is the one tool on the incremental database, and it takes the database
/// only when it has to. The address is resolved from the queried file alone
/// first; a local binding, or a position that names nothing, is answered right
/// there. Only a module-level symbol reaches the project.
///
/// The sweep used to run first, so a question whose answer never left the file
/// still walked and re-read every file in the project: 11 ms on a 175-file tree
/// and 77 ms on a 340-file one, all of it discarded. That cost scales with the
/// project where the single-file answer does not.
///
/// On the project path it reads `parse_module` and `resolve` and never touches
/// `type_map`: the occurrence scan reads the resolution table only, so computing
/// types for the whole project was 47 ms of work whose 68,425 entries were
/// dropped unread.
fn tool_references(args: &Value, server: &mut Server) -> Result<String, String> {
    let root = server.root.clone();
    let (path, text) = read_file(args, &root)?;
    let address = read_address(args)?;
    // Module paths, and so the search for other files naming this symbol, are
    // scoped to the file's own project (D41): another project's `import lib`
    // names its own `lib`, not this one. That is also why each project gets its
    // own database rather than one database over the server's root.
    let project_root = crate::project_root_for(&path, &root);
    let this_module =
        module_path_of(&project_root, &path).ok_or("the file is not under the project root")?;
    let index = LineIndex::new(&text);

    // Deliberately not `analyze_full`: that also assigns types, and every query
    // below reads the resolution table only. This is the same front end the
    // database runs (`parse_module` → `module_symbols` → `resolve`), on one
    // file, and it costs a fraction of a millisecond.
    let Ok(module) = glyph_parser::parse(&text) else {
        return unresolvable(&address, &this_module, "the file does not parse");
    };
    let Ok(symbols) = collect_module_symbols(&module) else {
        return unresolvable(&address, &this_module, "the file does not resolve");
    };
    let (resolved, _errs) = resolve_module(&module, symbols, &build_prelude());

    let target = resolve_address(&address, &module, &resolved, &index, &text, &this_module)?;
    let (sym_module, name) = match target {
        Some(SymbolTarget::Global { module, name }) => (module, name),
        // A local binding cannot be named from another file, so the project has
        // nothing to add and is never built.
        //
        // Only a position reaches this arm: `named_target` reads the module's
        // top-level table, and a `let` has no entry there, so a name never
        // resolves to a local.
        Some(SymbolTarget::Local) => {
            let Some((line, character)) = address.position() else {
                return Ok("[]".to_string());
            };
            let offset = index.offset(&text, line, character);
            let file = FileCtx { path: &path, root: &root, text: &text };
            let out: Vec<Value> = references_at(&module, &resolved, offset, &text, true)
                .into_iter()
                .map(|(s, e)| location_value(file, &index, s, e))
                .collect();
            return Ok(to_json(&out));
        }
        None => return Ok("[]".to_string()),
    };

    let project = server.project(&project_root, &path);
    let db = &project.db;
    let mut out: Vec<Value> = Vec::new();
    for (fpath, entry) in project.searched() {
        let ftext = entry.file.text(db);
        let fparsed = glyph_db::parse_module(db, entry.file);
        let fresolved = glyph_db::resolve(db, entry.file);
        let (Some(fmodule), Some(fresolved)) = (fparsed.module(), fresolved.resolved()) else {
            continue;
        };
        let spans = global_occurrences_in(
            fmodule,
            fresolved,
            &entry.module_path,
            &sym_module,
            &name,
            ftext,
            true,
        );
        if spans.is_empty() {
            continue;
        }
        let index = LineIndex::new(ftext);
        let file = FileCtx { path: fpath, root: &root, text: ftext };
        for (s, e) in spans {
            out.push(location_value(file, &index, s, e));
        }
    }
    Ok(to_json(&out))
}

/// Every match site in the project over one type, and which variants each
/// site's arms name.
///
/// The answer is descriptors, never ids. A site's identity inside the relation
/// is a cursor in one computation: it is not published, not compared across
/// revisions, and an answer carrying one would hand back a handle that the
/// next edit silently repoints. What crosses the boundary is what an agent
/// relocates the site from, which is the declaration it sits in, the scrutinee
/// as written, and the line.
///
/// The type is addressed by `name`, resolved in the module namespace of the
/// file the call names. That is the same rule `glyph_references` uses, and it
/// is the half that makes the answer about one declaration: eleven unrelated
/// declarations in the dogfood corpus are named `Command`, and the module the
/// name resolves to is what tells them apart. A name that resolves to nothing
/// is matched against the relation's own type ends, which is how a prelude or
/// stdlib union is asked for, and two matches is an error rather than a pick.
fn tool_variants(args: &Value, server: &mut Server) -> Result<String, String> {
    let root = server.root.clone();
    let (path, text) = read_file(args, &root)?;
    let name = type_name_arg(args)?;
    let project_root = crate::project_root_for(&path, &root);
    let this_module =
        module_path_of(&project_root, &path).ok_or("the file is not under the project root")?;

    // The same single-file front end `tool_references` runs, for the same
    // reason: the address is resolved from the queried file, and only the
    // relation itself needs the project.
    let Ok(module) = glyph_parser::parse(&text) else {
        return Err(unreadable(&name, &this_module, "the file does not parse"));
    };
    let Ok(symbols) = collect_module_symbols(&module) else {
        return Err(unreadable(&name, &this_module, "the file does not resolve"));
    };
    // Held rather than passed inline: it is also what says whether a name the
    // module does not declare is nonetheless a name the file can use.
    let prelude = build_prelude();
    let (resolved, _errs) = resolve_module(&module, symbols, &prelude);
    refuse_non_type(&module, &resolved, &name)?;
    let target = named_target(&resolved, &name, &this_module);

    let project = server.project(&project_root, &path);
    let db = &project.db;
    // A file the walk skips is not a member, so the relation holds none of its
    // sites. Answering from its namespace would report a complete set that is
    // missing every site in the file the caller named.
    if !project.files.contains_key(&path) {
        return Err(format!(
            "{} is not a member of the project at {}, so the match-coverage relation \
             holds none of its sites and an answer keyed from it would be missing them. \
             Ask about a file the project walk reaches.",
            display_path(&root, &path),
            display_path(&root, &project_root),
        ));
    }
    let cov = project_match_coverage(db, db.project_files_input());
    let decls = cov.decls();

    let want = wanted_type(decls, target, &name);
    let mut matched: Vec<&CoverageTypeRef> = cov
        .types()
        .filter(|end| end_matches(decls, end, &want))
        .collect();
    if matched.len() > 1 {
        let listed: Vec<String> = matched.iter().map(|e| render_type_end(decls, e)).collect();
        return Err(format!(
            "`{name}` names more than one type this project matches on: {}. A display name \
             is not an address. Ask from a file that declares or imports the one you mean, \
             so the module it resolves to decides which.",
            listed.join(", ")
        ));
    }
    let end = match matched.pop() {
        Some(end) => end.clone(),
        None => derive_type_end(decls, &want, &this_module, &resolved, &prelude)?,
    };

    let by_module: BTreeMap<&str, (&Path, SourceFile)> = project
        .files
        .iter()
        .map(|(p, f)| (f.module_path.as_str(), (p.as_path(), f.file)))
        .collect();
    let mut render = SiteRender {
        db,
        root: &root,
        decls,
        by_module,
        files: BTreeMap::new(),
    };

    let sites: Vec<Value> = cov
        .sites_over(&end)
        .map(|site| render.value(site))
        .collect();
    // Sites that name a variant of this type through a payload rather than as
    // their own scrutinee. `match r { Ok(Some(n)) => ... }` over a
    // `Result<Option<T>, E>` names `Some` of `Option` a level down, and the
    // relation files that site under `Result`, so the list above cannot hold
    // it. It is still a site that has to change when `Option` gains a
    // variant, which is the question this tool is asked, so it is named here
    // rather than left out. A union that appears both as a scrutinee and
    // inside its own payload is in both lists, because it is both.
    let nested: Vec<Value> = cov
        .sites()
        .iter()
        .filter_map(|site| render.nested_value(site, &end, &name))
        .collect();
    // Sites over a same-named type this project could not key. They are not
    // the answer and they are not absent from it either: absence in this
    // relation means no relation exists, and one of these may well be over the
    // type that was asked about, with the module string that would have joined
    // them naming nothing this project has.
    let unkeyed: Vec<Value> = cov
        .sites()
        .iter()
        .filter(|site| site.scrutinee_type != end && unkeyed_namesake(&site.scrutinee_type, &name))
        .map(|site| {
            let mut value = render.value(site);
            value["type"] = type_end_value(decls, &site.scrutinee_type);
            value
        })
        .collect();

    let mut answer = json!({
        "type": type_end_value(decls, &end),
        "sites": sites,
    });
    if !nested.is_empty() {
        answer["nested"] = json!(nested);
    }
    if !unkeyed.is_empty() {
        answer["unkeyed"] = json!(unkeyed);
    }
    Ok(to_json(&answer))
}

/// The required `name` argument: the type one call is asking about.
///
/// A malformed one is a malformed call, for the reason `read_address` gives:
/// answering something adjacent to what was asked is how a typo becomes a
/// confident answer about a different type.
fn type_name_arg(args: &Value) -> Result<String, String> {
    match args.get("name") {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err("`name` is empty".to_string()),
        None | Some(Value::Null) => {
            Err("missing `name`: the type to report match sites over".to_string())
        }
        Some(other) => Err(format!("`name` must be a string, got `{other}`")),
    }
}

/// The error for a file that cannot be analysed far enough to resolve a name.
///
/// An empty list is not available here the way it is for a stale position:
/// the caller named a type, and `[]` would read as "nothing matches on it"
/// when the truth is that we could not look.
fn unreadable(name: &str, this_module: &str, why: &str) -> String {
    format!(
        "cannot look up `{name}` in module `{this_module}`: {why}. \
         Run glyph_diagnostics on the file."
    )
}

/// Refuse a `name` the queried file says is not a type.
///
/// Every top-level declaration has a key, a function's included, so asking
/// about `run` would find no site over it and answer `sites: []`. That reads
/// as "nothing matches on this type", which is a wrong answer rather than a
/// missing one. The kind is in the file's own table, so the call fails saying
/// what the name actually is.
///
/// A variant is the case worth a real message: `Up` is not `Command`, and the
/// union it belongs to is one hop away in the same file.
fn refuse_non_type(
    module: &glyph_ast::Module,
    resolved: &ResolvedModule,
    name: &str,
) -> Result<(), String> {
    let Some(id) = resolved.symbols.lookup(name) else {
        return Ok(());
    };
    let Some(sym) = resolved.symbols.table.get(id) else {
        return Ok(());
    };
    let what = match &sym.kind {
        // A type, an imported binding (whose kind lives in the module that
        // declares it, not here), or a prelude name such as `Result`.
        SymbolKind::Type { .. } | SymbolKind::ImportNamed { .. } | SymbolKind::Prelude { .. } => {
            return Ok(())
        }
        SymbolKind::Function { .. } => "a function",
        SymbolKind::Const { .. } => "a constant",
        SymbolKind::Component { .. } => "a component",
        SymbolKind::ImportNamespace { .. }
        | SymbolKind::ImportAlias { .. }
        | SymbolKind::ImportDefault { .. } => "an imported module binding",
        SymbolKind::Variant { decl_idx } => {
            let union = match module.items.get(*decl_idx as usize) {
                Some(glyph_ast::Decl::Type(t)) => Some(t.name.to_string()),
                _ => None,
            };
            return Err(match union {
                Some(union) => format!(
                    "`{name}` is a variant of `{union}`, not a type. Ask about `{union}`: \
                     this tool reports the sites that match on a union, and `{name}` is one \
                     of that union's arms."
                ),
                None => format!("`{name}` is a union variant, not a type."),
            });
        }
    };
    Err(format!(
        "`{name}` is {what}, not a type. glyph_variants reports the match sites over a \
         tagged union, so it needs the union's name."
    ))
}

/// Which type end one call is asking about.
enum WantedType {
    /// The name resolved to a module this project keys, so only a declaration
    /// under that exact module can be it.
    Declared { module: String, name: String },
    /// The name resolved to a module this project does not have, which is what
    /// `import std/result { Result }` looks like from the consumer's side. No
    /// declaration of this project is under that module, so only a named
    /// non-declaration answers.
    Foreign { module: String, name: String },
    /// The file's namespace does not hold the name: a prelude union with no
    /// import (`Result`), the dotted spelling of a stdlib one
    /// (`fs.ErrorKind`), or a name that is simply not there. Matched on the
    /// name alone, and two matches is an error rather than a pick.
    Bare { name: String },
}

/// Turn the address into the type end to look for.
fn wanted_type(decls: &DeclIndex, target: Option<SymbolTarget>, name: &str) -> WantedType {
    match target {
        Some(SymbolTarget::Global { module, name }) => match decls.module_id(&module) {
            Some(_) => WantedType::Declared { module, name },
            None => WantedType::Foreign { module, name },
        },
        // `named_target` never answers `Local`: a `let` has no entry in the
        // module's top-level table. `None` is a prelude entry, a namespace
        // import, or a name the module does not hold.
        Some(SymbolTarget::Local) | None => WantedType::Bare {
            name: name.to_string(),
        },
    }
}

/// Whether one of the relation's type ends is the one being asked about.
fn end_matches(decls: &DeclIndex, end: &CoverageTypeRef, want: &WantedType) -> bool {
    match want {
        WantedType::Declared { module, name } => match end {
            CoverageTypeRef::Decl(key) => {
                key.name() == name && decls.module_path(key.module()) == Some(module.as_str())
            }
            // The project keys the module and not this name under it, which is
            // what an import of a name its module does not declare looks like.
            CoverageTypeRef::Unkeyed { module: m, name: n } => m == module && n == name,
            // A builtin has a name and no module, and no prelude union is
            // declared in a module this project keys.
            CoverageTypeRef::Builtin { .. } => false,
        },
        WantedType::Foreign { module, name } => match end {
            // The project has no such module, so it holds no key under one.
            CoverageTypeRef::Decl(_) => false,
            CoverageTypeRef::Unkeyed { module: m, name: n } => m == module && n == name,
            CoverageTypeRef::Builtin { name: n } => n == name,
        },
        WantedType::Bare { name } => match end {
            CoverageTypeRef::Decl(key) => key.name() == name,
            CoverageTypeRef::Unkeyed { name: n, .. } | CoverageTypeRef::Builtin { name: n } => {
                n == name
            }
        },
    }
}

/// The type end for a call the relation holds no site for.
///
/// No site is a real answer for a declaration nothing matches on yet, and it
/// still has to say which type it is about, so the end is derived from the
/// address rather than found among the sites.
fn derive_type_end(
    decls: &DeclIndex,
    want: &WantedType,
    this_module: &str,
    resolved: &ResolvedModule,
    prelude: &Prelude,
) -> Result<CoverageTypeRef, String> {
    match want {
        WantedType::Declared { module, name } => Ok(match decls.key_of(module, name) {
            Some(key) => CoverageTypeRef::Decl(key),
            None => CoverageTypeRef::Unkeyed {
                module: module.clone(),
                name: name.clone(),
            },
        }),
        WantedType::Foreign { module, name } => Ok(CoverageTypeRef::Unkeyed {
            module: module.clone(),
            name: name.clone(),
        }),
        // The prelude and not the module's table: a module's `by_name` holds
        // only what the file itself declares and imports, so `Option` is
        // absent from it in a file that uses `Option` throughout. Asking the
        // wrong table here reported a name every Glyph file can use as a name
        // the module does not have.
        WantedType::Bare { name } => match prelude.lookup(name) {
            // A prelude name with no site: `Option` in a project that only
            // ever reaches it through a payload. It has a name and no
            // declaration anywhere, and the honest answer is that nothing
            // matches on it directly, which is not the same as not existing.
            Some(_) => Ok(CoverageTypeRef::Builtin { name: name.clone() }),
            // Neither a prelude name, nor one the file declares or imports,
            // nor a type any site in the project matches on. An empty answer
            // would read as "this type has no match site", so the call fails
            // the way a renamed symbol does.
            None => Err(not_declared(name, this_module, resolved)),
        },
    }
}

/// Whether a type end is one this project could not key, under the name being
/// asked about.
fn unkeyed_namesake(end: &CoverageTypeRef, name: &str) -> bool {
    matches!(end, CoverageTypeRef::Unkeyed { name: n, .. } if n == name)
}

/// The same question `unkeyed_namesake` answers, one level down: whether a
/// coverage edge's own union (still the checker's raw `CoverageTypeName`,
/// unminted) is a namesake of the type being asked about that this project's
/// `DeclIndex` cannot key.
///
/// `edge_is` alone is not enough here. It compares a `Declared` union against
/// `end` by asking whether `decls` maps the *union's* module string to the
/// *same declaration key* `end` names, and a G172 module-line/path mismatch
/// means it never does: the union's module is the file's own `module` line,
/// `end` (when derived, rather than found among the relation's own type ends)
/// is keyed from the path-derived module `decls` actually holds the
/// declaration under, and those two strings disagree by construction. Without
/// this, a payload mention or gap that reaches such a type is `edge_is`-false
/// against every candidate `end` and is dropped from `nested` entirely, the
/// same silent omission `unkeyed_namesake` exists to prevent for a direct
/// site. Matched by name alone, same as `unkeyed_namesake`: an edge this
/// project cannot key has no module key left to compare by.
fn edge_is_unkeyed_namesake(decls: &DeclIndex, union: &CoverageTypeName, name: &str) -> bool {
    match union {
        CoverageTypeName::Declared { module, name: n } => {
            n == name && decls.key_of(module, name).is_none()
        }
        CoverageTypeName::Builtin { .. } => false,
    }
}

/// The type end as an answer names it.
///
/// Three shapes because the relation has three, and a consumer acts on the
/// difference: only the first has a declaration to go to.
fn type_end_value(decls: &DeclIndex, end: &CoverageTypeRef) -> Value {
    match end {
        CoverageTypeRef::Decl(key) => json!({
            "kind": "declaration",
            "module": decls.module_path(key.module()),
            "name": key.name(),
            "declaration": render_key(decls, key),
        }),
        // A name with a fixed variant table and no declaration anywhere in the
        // project. There is nothing to key: a declaration key invented for
        // `Result` would name a module no project has.
        CoverageTypeRef::Builtin { name } => json!({
            "kind": "builtin",
            "name": name,
            "declaration": Value::Null,
        }),
        CoverageTypeRef::Unkeyed { module, name } => json!({
            "kind": "unkeyed",
            "module": module,
            "name": name,
            "declaration": Value::Null,
        }),
    }
}

/// A type end as one line of prose.
fn render_type_end(decls: &DeclIndex, end: &CoverageTypeRef) -> String {
    match end {
        CoverageTypeRef::Decl(key) => render_key(decls, key),
        CoverageTypeRef::Builtin { name } => name.clone(),
        CoverageTypeRef::Unkeyed { module, name } => format!("{module}::{name}"),
    }
}

/// A declaration key as `module::name`, which is the identity the rest of the
/// graph names a declaration by.
///
/// Rendered through the index that minted the key and never through another
/// one. A `ModuleId` is an interner index: an in-range id from a different
/// interner names the wrong module rather than answering nothing.
fn render_key(decls: &DeclIndex, key: &DeclKey) -> String {
    match decls.module_path(key.module()) {
        Some(module) => format!("{module}::{}", key.name()),
        // Only reachable for a key from another interner, which cannot happen
        // here: this index is the one the relation minted its keys from.
        None => key.name().to_string(),
    }
}

/// What the checker concluded about a site, as the answer spells it.
fn state_str(state: CoverageState) -> &'static str {
    match state {
        CoverageState::Exhaustive => "exhaustive",
        CoverageState::HasCatchAll => "has_catch_all",
        CoverageState::Declined => "declined",
        CoverageState::ScrutineeUnresolved => "scrutinee_unresolved",
    }
}

/// One project file, as rendering a site in it needs it.
struct FileRender {
    path: PathBuf,
    file: SourceFile,
    /// The file's top-level declarations with their spans, which is how a site
    /// finds the declaration it sits in.
    outline: Vec<OutlineSymbol>,
    index: LineIndex,
}

/// Where one site is, in the terms the answer carries it.
struct SiteWhere {
    path: PathBuf,
    line: u32,
    scrutinee: String,
    declaration: Option<DeclKey>,
}

/// Turns the relation's sites into answers.
///
/// The per-file work (parsing out an outline, indexing lines) is cached across
/// the sites of one call, because several sites in one file is the common
/// shape and the parse is a memo hit rather than a re-parse either way.
struct SiteRender<'a> {
    db: &'a CompilerDb,
    root: &'a Path,
    decls: &'a DeclIndex,
    /// The project's members by the module key the relation files sites under.
    by_module: BTreeMap<&'a str, (&'a Path, SourceFile)>,
    files: BTreeMap<String, FileRender>,
}

impl SiteRender<'_> {
    /// One site as the answer carries it.
    ///
    /// No site index appears anywhere in here. The index that routed the
    /// checkers' writes is a cursor inside one computation, it is never
    /// compared across revisions, and publishing one would be handing back an
    /// identity the next edit silently repoints.
    fn value(&mut self, site: &ProjectCoverageSite) -> Value {
        let d = &site.site;
        let mut out = self.descriptor(site);
        out.insert("arms".to_string(), arms_value(d));
        for (field, value) in [
            ("missing", missing_value(d)),
            ("declined", declined_value(d)),
            ("catch_all", catch_all_value(d)),
            ("payload_unions", payload_unions_value(d)),
        ] {
            if let Some(value) = value {
                out.insert(field.to_string(), value);
            }
        }
        Value::Object(out)
    }

    /// One site that names a variant of `end` through a payload, or `None`
    /// when it names none.
    ///
    /// The arms here carry their depth, because a depth is what makes them not
    /// this site's own accounting: the site was counted against its scrutinee's
    /// variant set, and these arms reach a level below that.
    fn nested_value(&mut self, site: &ProjectCoverageSite, end: &CoverageTypeRef, name: &str) -> Option<Value> {
        let decls = self.decls;
        let d = &site.site;
        // `edge_is` alone misses a mention or gap whose union is a G172
        // namesake of `end`: the union's module string is the file's own
        // `module` line, and when `end` was derived (rather than found among
        // the relation's own type ends) it is keyed from the path-derived
        // module `decls` actually holds the declaration under, so the two
        // never compare equal. `edge_is_unkeyed_namesake` is the same
        // name-only fallback `unkeyed_namesake` already uses for a direct
        // site, one level down, so the payload site is reported rather than
        // silently dropped.
        let arms: Vec<Value> = d
            .mentions
            .iter()
            .filter(|m| {
                m.depth > 0
                    && (edge_is(decls, &m.union, end)
                        || edge_is_unkeyed_namesake(decls, &m.union, name))
            })
            .map(|m| json!({ "arm": m.arm, "depth": m.depth, "variant": m.variant }))
            .collect();
        let missing: Vec<&String> = d
            .gaps
            .iter()
            .filter(|g| {
                g.depth > 0
                    && (edge_is(decls, &g.union, end)
                        || edge_is_unkeyed_namesake(decls, &g.union, name))
            })
            .flat_map(|g| g.missing.iter())
            .collect();
        if arms.is_empty() && missing.is_empty() {
            return None;
        }
        let scrutinee_type = type_end_value(decls, &site.scrutinee_type);
        let mut out = self.descriptor(site);
        // What the site itself matches on, which is not the type that was
        // asked about. Without it the entry reads as a site over this type.
        out.insert("type".to_string(), scrutinee_type);
        out.insert("arms".to_string(), json!(arms));
        if !missing.is_empty() {
            out.insert("missing".to_string(), json!(missing));
        }
        Some(Value::Object(out))
    }

    /// Where a site is and what the checker concluded about it: the half of
    /// the descriptor that is the same whichever list the site is in.
    fn descriptor(&mut self, site: &ProjectCoverageSite) -> serde_json::Map<String, Value> {
        let d = &site.site;
        let mut out = serde_json::Map::new();
        out.insert("module".to_string(), json!(d.module));
        match self.locate(&d.module, d.scrutinee_span.start, d.scrutinee_span.end) {
            Some(w) => {
                out.insert(
                    "path".to_string(),
                    json!(display_path(self.root, &w.path)),
                );
                out.insert("line".to_string(), json!(w.line));
                out.insert("scrutinee".to_string(), json!(w.scrutinee));
                out.insert(
                    "declaration".to_string(),
                    match &w.declaration {
                        Some(key) => json!(render_key(self.decls, key)),
                        None => Value::Null,
                    },
                );
            }
            // The relation files the site under a module the project's file
            // list no longer holds. Name it rather than drop it: the site is
            // real, and absence here means no relation exists.
            None => {
                for field in ["path", "line", "scrutinee", "declaration"] {
                    out.insert(field.to_string(), Value::Null);
                }
            }
        }
        out.insert("state".to_string(), json!(state_str(d.state)));
        out
    }

    /// Where a site's scrutinee is, and which declaration it sits in.
    fn locate(&mut self, module: &str, start: u32, end: u32) -> Option<SiteWhere> {
        // Copied out before the cache borrow, which is a borrow of `self`.
        let db = self.db;
        let decls = self.decls;
        let f = self.cached(module)?;
        let text = f.file.text(db);
        let (line, _character) = f.index.position(text, start as usize);
        Some(SiteWhere {
            path: f.path.clone(),
            line,
            scrutinee: text
                .get(start as usize..end as usize)
                .unwrap_or_default()
                .to_string(),
            // The enclosing top-level declaration, keyed through the index the
            // relation's own keys came from so the two are comparable.
            declaration: f
                .outline
                .iter()
                .find(|s| s.span.0 <= start && start < s.span.1)
                .and_then(|s| decls.key_of(module, &s.name)),
        })
    }

    fn cached(&mut self, module: &str) -> Option<&FileRender> {
        if !self.files.contains_key(module) {
            let (path, file) = *self.by_module.get(module)?;
            // A memo hit: the relation already parsed every project file.
            let parsed = glyph_db::parse_module(self.db, file);
            let outline = parsed.module().map(module_outline).unwrap_or_default();
            let index = LineIndex::new(file.text(self.db));
            self.files.insert(
                module.to_string(),
                FileRender {
                    path: path.to_path_buf(),
                    file,
                    outline,
                    index,
                },
            );
        }
        self.files.get(module)
    }
}

/// The arms naming a variant of the site's own scrutinee type, with the
/// ordinal each one sits at.
///
/// Depth 0 only. A deeper mention (`Some` inside `Ok(Some(x))`) names a
/// variant of a payload union, which is a different declaration, and listing
/// it here would report a variant this type does not have.
fn arms_value(d: &CoverageSiteRef) -> Value {
    let arms: Vec<Value> = d
        .mentions
        .iter()
        .filter(|m| m.depth == 0)
        .map(|m| json!({ "arm": m.arm, "variant": m.variant }))
        .collect();
    json!(arms)
}

/// The variants of this type no arm names: the same list E0200 reports.
fn missing_value(d: &CoverageSiteRef) -> Option<Value> {
    let missing: Vec<&String> = d
        .gaps
        .iter()
        .filter(|g| g.depth == 0)
        .flat_map(|g| g.missing.iter())
        .collect();
    if missing.is_empty() {
        None
    } else {
        Some(json!(missing))
    }
}

/// The arms the checker read nothing from, so the list above is not a complete
/// accounting of what this site handles.
fn declined_value(d: &CoverageSiteRef) -> Option<Value> {
    let declined: Vec<Value> = d
        .declines
        .iter()
        .filter(|x| x.depth == 0)
        .map(|x| json!({ "arm": x.arm, "variant": x.variant }))
        .collect();
    if declined.is_empty() {
        None
    } else {
        Some(json!(declined))
    }
}

/// The arms that absorb every value the scrutinee can still take.
fn catch_all_value(d: &CoverageSiteRef) -> Option<Value> {
    let arms: Vec<u16> = d
        .catch_alls
        .iter()
        .filter(|c| c.depth == 0)
        .map(|c| c.arm)
        .collect();
    if arms.is_empty() {
        None
    } else {
        Some(json!(arms))
    }
}

/// Whether a union named inside a coverage edge is the type being asked about.
///
/// A comparison rather than a second minting of the key. `project_match_coverage`
/// mints one key per site from the interner its declaration index owns, and a
/// key minted anywhere else is an in-range index for some other module, so it
/// would answer about the wrong declaration rather than fail. This only ever
/// answers yes or no, and its declared case goes through that same index.
fn edge_is(decls: &DeclIndex, union: &CoverageTypeName, end: &CoverageTypeRef) -> bool {
    match (union, end) {
        (CoverageTypeName::Builtin { name: a }, CoverageTypeRef::Builtin { name: b }) => a == b,
        (CoverageTypeName::Declared { module, name }, CoverageTypeRef::Decl(key)) => {
            key.name() == name && decls.module_path(key.module()) == Some(module.as_str())
        }
        (
            CoverageTypeName::Declared { module: m, name: n },
            CoverageTypeRef::Unkeyed { module, name },
        ) => m == module && n == name,
        _ => false,
    }
}

/// The unions this site's arms reach into through a payload.
///
/// A site's state covers its payload recursions, so it can read short of
/// exhaustive with nothing missing at depth 0. Naming the unions underneath is
/// what makes that answer followable: each is a declaration of its own, and
/// asking this tool about one lists this site under `nested`.
fn payload_unions_value(d: &CoverageSiteRef) -> Option<Value> {
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for m in d.mentions.iter().filter(|m| m.depth > 0) {
        names.insert(union_name(&m.union));
    }
    for g in d.gaps.iter().filter(|g| g.depth > 0) {
        names.insert(union_name(&g.union));
    }
    if names.is_empty() {
        None
    } else {
        Some(json!(names))
    }
}

fn union_name(union: &CoverageTypeName) -> &str {
    match union {
        CoverageTypeName::Declared { name, .. } | CoverageTypeName::Builtin { name } => name,
    }
}

/// Search the whole server root's top-level declarations.
///
/// Deliberately still an uncached walk. This tool spans project boundaries by
/// design — an agent asking "where is `parse_row`" wants the answer from every
/// project under the root, not just the one holding some file it happened to
/// name — and the per-project databases cannot answer a question that crosses
/// them. Changing what a tool returns as a side effect of a caching change is
/// the wrong way to decide that, so its behaviour is untouched.
fn tool_symbols(args: &Value, root: &Path) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let mut out: Vec<Value> = Vec::new();
    for (fpath, ftext) in workspace_files(root) {
        let index = LineIndex::new(&ftext);
        let file = FileCtx { path: &fpath, root, text: &ftext };
        for top in outline_of(&ftext) {
            push_symbol(&mut out, &query, file, &index, &top, None);
            for child in &top.children {
                push_symbol(&mut out, &query, file, &index, child, Some(top.name.as_str()));
            }
        }
    }
    Ok(to_json(&out))
}

fn push_symbol(
    out: &mut Vec<Value>,
    query: &str,
    file: FileCtx<'_>,
    index: &LineIndex,
    sym: &OutlineSymbol,
    container: Option<&str>,
) {
    if !query.is_empty() && !sym.name.to_lowercase().contains(query) {
        return;
    }
    let mut value = json!({
        "name": sym.name,
        "kind": outline_kind_str(sym.kind),
        "location": location_value(file, index, sym.span.0, sym.span.1),
    });
    if let Some(c) = container {
        value["container"] = json!(c);
    }
    out.push(value);
}

/// Every `.glyph` file under `root` as `(path, text)`, skipping unreadable ones.
fn workspace_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    collect_glyph_files(root, &mut files);
    files
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
        .collect()
}

// ----- argument + result helpers -----

/// Resolve the `path` argument to a real, canonical `.glyph` file.
///
/// Both halves of this are load-bearing, and each one was a wrong answer before
/// it was here.
///
/// **Canonicalize**, because a path is an identity and one file must have one.
/// Symlinks are the clearest case: `collect_glyph_files` reads directory
/// entries without traversing links, so an aliased spelling is never a walk
/// member and the same physical file was reported twice, once under each name.
/// A `..` segment was worse than cosmetic: `project_root_for` resolved
/// `p/sub/..` to a root distinct from `p`, building a second copy of a
/// project's memo and spending a second cache slot on it, while
/// `p/../outside.glyph` was captured by `p`'s marker because the prefix test
/// succeeds on an unnormalized path. Case-only spellings forked a project the
/// same way on a case-insensitive filesystem.
///
/// **Check the extension**, because the module path is derived by dropping it.
/// Without the check `a.txt` becomes module `a`, collides with the real `a.glyph`,
/// and a declaration gets reported in two files. A rename driven off that answer
/// would be handed a text file as a site to edit.
fn read_file(args: &Value, root: &Path) -> Result<(PathBuf, String), String> {
    let raw = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing `path`")?;
    let joined = {
        let p = Path::new(raw);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    };
    // Canonicalize before anything reads the path, so every later comparison
    // (project root, walk membership, the emitted location) sees one spelling.
    // It also fails cleanly for a path that does not exist, which is the same
    // error the read would have produced.
    let path = std::fs::canonicalize(&joined)
        .map_err(|e| format!("cannot read {}: {e}", joined.display()))?;
    if path.extension().and_then(|e| e.to_str()) != Some("glyph") {
        return Err(format!("not a Glyph source file: {}", path.display()));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok((path, text))
}

fn position(args: &Value) -> Result<(u32, u32), String> {
    let line = args
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or("missing `line`")? as u32;
    let character = args
        .get("character")
        .and_then(|v| v.as_u64())
        .ok_or("missing `character`")? as u32;
    Ok((line, character))
}

/// How one call addressed the thing it is asking about.
///
/// A position is what an editor has. It is the only way to point at an
/// expression or at a local binding, and it is the right question for a cursor.
/// A name is what an agent has, and it is the only address that survives an
/// edit elsewhere in the file: inserting one declaration above `charge` moves
/// every line below it, so a line and character recorded a few edits ago now
/// covers the neighbour, and the answer about the neighbour is well formed.
enum Address {
    Position {
        line: u32,
        character: u32,
    },
    Name(String),
    /// Both, which is a cross-check rather than a choice. See `resolve_address`.
    Both {
        line: u32,
        character: u32,
        name: String,
    },
}

impl Address {
    fn position(&self) -> Option<(u32, u32)> {
        match self {
            Address::Position { line, character } | Address::Both { line, character, .. } => {
                Some((*line, *character))
            }
            Address::Name(_) => None,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            Address::Name(name) | Address::Both { name, .. } => Some(name),
            Address::Position { .. } => None,
        }
    }
}

/// Read the `line`, `character`, and `name` arguments as one address.
///
/// A malformed `name` is an error rather than an ignored key. Falling back to a
/// position the caller also sent would answer a different question from the one
/// they asked, and answer it confidently, which is the failure the name is here
/// to remove.
fn read_address(args: &Value) -> Result<Address, String> {
    let name = match args.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => return Err("`name` is empty".to_string()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => return Err(format!("`name` must be a string, got `{other}`")),
    };
    let given = |key: &str| args.get(key).is_some_and(|v| !v.is_null());
    match (name, given("line") || given("character")) {
        (Some(name), false) => Ok(Address::Name(name)),
        (Some(name), true) => {
            let (line, character) = position(args)?;
            Ok(Address::Both {
                line,
                character,
                name,
            })
        }
        (None, true) => {
            let (line, character) = position(args)?;
            Ok(Address::Position { line, character })
        }
        (None, false) => {
            Err("no address: give either `line` and `character`, or `name`".to_string())
        }
    }
}

/// One file, as every location-producing helper here needs it: where it is,
/// what project it belongs to, and its text. The three always travel together
/// and were threaded separately through five call sites, which is what pushed
/// two of them past clippy's argument limit.
#[derive(Clone, Copy)]
struct FileCtx<'a> {
    path: &'a Path,
    root: &'a Path,
    text: &'a str,
}

fn location_value(
    file: FileCtx<'_>,
    index: &LineIndex,
    start: u32,
    end: u32,
) -> Value {
    json!({
        "path": display_path(file.root, file.path),
        "range": range_json(index, file.text, start, end),
    })
}

fn range_json(index: &LineIndex, text: &str, start: u32, end: u32) -> Value {
    let (sl, sc) = index.position(text, start as usize);
    let (el, ec) = index.position(text, end as usize);
    json!({
        "start": { "line": sl, "character": sc },
        "end": { "line": el, "character": ec },
    })
}

/// A file path reported to the agent: relative to `root` with `/` separators
/// when the file is under it, else the absolute path.
fn display_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .ok()
        .map(|r| {
            r.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|| file.to_string_lossy().into_owned())
}

fn outline_kind_str(kind: OutlineKind) -> &'static str {
    match kind {
        OutlineKind::Function => "function",
        OutlineKind::Type => "type",
        OutlineKind::Constant => "constant",
        OutlineKind::Variant => "variant",
    }
}

fn to_json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn text_content(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp_root() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "glyph_mcp_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, name: &str, text: &str) {
        std::fs::write(root.join(name), text).unwrap();
    }

    /// Invoke a tool on `server` and return its raw text content plus the error
    /// flag. A tool error is prose, not JSON, so a test that asserts on the
    /// message has to read the text before anything tries to parse it.
    fn call_raw(server: &mut Server, name: &str, args: Value) -> (String, bool) {
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        let resp = handle(&req, server).expect("response");
        let result = &resp["result"];
        let is_error = result["isError"].as_bool().unwrap();
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        (text, is_error)
    }

    /// Invoke a tool on `server` and return the parsed JSON of its text content.
    fn call_on(server: &mut Server, name: &str, args: Value) -> (Value, bool) {
        let (text, is_error) = call_raw(server, name, args);
        (serde_json::from_str(&text).unwrap_or(Value::Null), is_error)
    }

    /// Invoke a tool on a fresh server rooted at `root`.
    fn call(root: &Path, name: &str, args: Value) -> (Value, bool) {
        call_on(&mut Server::new(root.to_path_buf()), name, args)
    }

    /// The reference locations for a position, as `(path, start line)` pairs.
    fn refs_at(server: &mut Server, path: &str, line: u32, character: u32) -> Vec<(String, u64)> {
        let (value, is_error) = call_on(
            server,
            "glyph_references",
            json!({ "path": path, "line": line, "character": character }),
        );
        assert!(!is_error, "{value}");
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["path"].as_str().unwrap().to_string(),
                    l["range"]["start"]["line"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    /// The source text each reported reference covers, so a test can assert
    /// *which entity* an answer is about rather than which line it landed on.
    /// Inserting a declaration above one moves every line below it, so the line
    /// numbers are the one part of the answer that legitimately changes.
    fn ref_names(server: &mut Server, root: &Path, args: Value) -> Vec<String> {
        let (value, is_error) = call_on(server, "glyph_references", args);
        assert!(!is_error, "{value}");
        let root = std::fs::canonicalize(root).unwrap();
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|loc| {
                let file = root.join(loc["path"].as_str().unwrap());
                let text = std::fs::read_to_string(&file).unwrap();
                let index = LineIndex::new(&text);
                let at = |end: &str| {
                    let p = &loc["range"][end];
                    index.offset(
                        &text,
                        p["line"].as_u64().unwrap() as u32,
                        p["character"].as_u64().unwrap() as u32,
                    )
                };
                text[at("start")..at("end")].to_string()
            })
            .collect()
    }

    const DECL: &str = "module a\npub fn foo() -> number {\n  return 1\n}\n";
    const IMPORTER: &str =
        "module b\nimport a { foo }\npub fn use_it() -> number {\n  return foo()\n}\n";
    const NO_IMPORT: &str = "module b\npub fn use_it() -> number {\n  return 1\n}\n";
    /// The insertion demonstration: a module, the same module with one
    /// unrelated declaration inserted above `charge`, and a module importing
    /// it. `charge` sits at line 1, character 7 in the first and at line 4,
    /// character 7 in the second, which is the whole problem with recording a
    /// coordinate.
    const CHARGE: &str = "module a\npub fn charge(m: number) -> number {\n  return m\n}\n";
    const AUDIT_ABOVE_CHARGE: &str = "module a\npub fn audit() -> number {\n  return 0\n}\npub fn charge(m: number) -> number {\n  return m\n}\n";
    const CHARGE_IMPORTER: &str =
        "module b\nimport a { charge }\npub fn bill() -> number {\n  return charge(1)\n}\n";

    /// A third importer of `a`, used from a directory the walk skips.
    const OUTSIDE_IMPORTER: &str =
        "module c\nimport a { foo }\npub fn also() -> number {\n  return foo()\n}\n";

    #[test]
    fn initialize_and_tools_list() {
        let root = tmp_root();
        let mut server = Server::new(root);
        let init = handle(
            &json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {} }),
            &mut server,
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], "glyph-mcp");

        let list = handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            &mut server,
        )
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for want in [
            "glyph_diagnostics",
            "glyph_hover",
            "glyph_definition",
            "glyph_references",
            "glyph_symbols",
            "glyph_variants",
        ] {
            assert!(names.contains(&want), "missing {want} in {names:?}");
        }
    }

    #[test]
    fn a_notification_gets_no_response() {
        let mut server = Server::new(tmp_root());
        assert!(handle(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            &mut server
        )
        .is_none());
    }

    #[test]
    fn diagnostics_tool_reports_codes() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\ntype U = { name: string }\nfn f(u: U) -> string {\n  return u.naem\n}\n",
        );
        let (value, is_error) = call(&root, "glyph_diagnostics", json!({ "path": "a.glyph" }));
        assert!(!is_error);
        let codes: Vec<&str> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap())
            .collect();
        assert!(codes.contains(&"E0210"), "{codes:?}");
    }

    /// An agent acting on a batch of `--json`-shaped diagnostics needs to get
    /// from an error straight into the declaration it lives in, without
    /// re-deriving "which function is this" from a line number — a line
    /// number shifts under every unrelated edit above the site, so a diff
    /// that touches nothing near the error can still stale a saved mapping.
    /// `glyph_diagnostics` names the entity the same way `glyph_variants`
    /// addresses a declaration site (`module::name`), so the two answers can
    /// be cross-referenced.
    #[test]
    fn diagnostics_tool_names_the_enclosing_declaration() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\ntype U = { name: string }\nfn f(u: U) -> string {\n  return u.naem\n}\n",
        );
        let (value, is_error) = call(&root, "glyph_diagnostics", json!({ "path": "a.glyph" }));
        assert!(!is_error);
        let items = value.as_array().unwrap();
        assert_eq!(items.len(), 1, "{value}");
        assert_eq!(items[0]["entity"], "a::f", "{value}");
    }

    /// A diagnostic that has no enclosing declaration to name — here, a parse
    /// failure, which has no AST to look one up in — must say so with an
    /// absent field, not a guessed value. `glyph_diagnostics` is a
    /// single-file, off-the-database tool, so the entity is derived only from
    /// this file's own parsed module; it is never wrong, only sometimes
    /// unavailable.
    #[test]
    fn diagnostics_tool_names_no_entity_for_a_parse_failure() {
        let root = tmp_root();
        write(&root, "a.glyph", "module a\nfn f(\n");
        let (value, is_error) = call(&root, "glyph_diagnostics", json!({ "path": "a.glyph" }));
        assert!(!is_error);
        let items = value.as_array().unwrap();
        assert!(!items.is_empty(), "{value}");
        assert!(items[0]["entity"].is_null(), "{value}");
    }

    #[test]
    fn references_tool_spans_files() {
        let root = tmp_root();
        write(&root, "a.glyph", "module a\nfn foo() -> number {\n  return 1\n}\n");
        write(
            &root,
            "b.glyph",
            "module b\nimport a { foo }\nfn use_it() -> number {\n  return foo()\n}\n",
        );
        // Position of `foo` in the declaration in a.glyph (line 1, char 3).
        let (value, is_error) = call(
            &root,
            "glyph_references",
            json!({ "path": "a.glyph", "line": 1, "character": 3 }),
        );
        assert!(!is_error);
        let locs = value.as_array().unwrap();
        // Declaration in a, import binding + one use in b = 3 across two files.
        assert_eq!(locs.len(), 3, "{value}");
        let paths: Vec<&str> = locs.iter().map(|l| l["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"a.glyph") && paths.contains(&"b.glyph"), "{paths:?}");
    }

    #[test]
    fn symbols_tool_searches_the_workspace() {
        let root = tmp_root();
        write(&root, "a.glyph", "module a\ntype Color = Red | Blue\nfn paint() -> number {\n  return 1\n}\n");
        let (value, is_error) = call(&root, "glyph_symbols", json!({ "query": "col" }));
        assert!(!is_error);
        let names: Vec<&str> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Color"), "{names:?}");
    }

    #[test]
    fn a_missing_file_is_a_tool_error_not_a_crash() {
        let root = tmp_root();
        let (_v, is_error) = call(&root, "glyph_diagnostics", json!({ "path": "nope.glyph" }));
        assert!(is_error);
    }
    // ---- the database-backed references path ----
    //
    // Three properties, and no one of them substitutes for another: the answer
    // must follow disk with no notification (`not stale`), a repeat must cost
    // nothing (`actually a cache`), and a file that did not exist when the
    // server started must be found.

    #[test]
    fn a_file_rewritten_behind_the_server_is_seen() {
        let root = tmp_root();
        write(&root, "a.glyph", DECL);
        write(&root, "b.glyph", IMPORTER);
        let mut server = Server::new(root.clone());

        // Declaration in a, import binding + one use in b.
        let before = refs_at(&mut server, "a.glyph", 1, 7);
        assert_eq!(before.len(), 3, "{before:?}");

        // Rewrite b on disk behind the server's back: no notification, no
        // restart, nothing told the server anything happened.
        write(&root, "b.glyph", NO_IMPORT);

        let after = refs_at(&mut server, "a.glyph", 1, 7);
        assert_eq!(after.len(), 1, "stale answer: {after:?}");
        assert_eq!(after[0].0, "a.glyph");
    }

    #[test]
    fn a_file_created_after_the_server_started_is_found() {
        let root = tmp_root();
        write(&root, "a.glyph", DECL);
        let mut server = Server::new(root.clone());
        assert_eq!(refs_at(&mut server, "a.glyph", 1, 7).len(), 1);

        // The directory walk, not a watcher, is what notices this.
        write(&root, "b.glyph", IMPORTER);

        let after = refs_at(&mut server, "a.glyph", 1, 7);
        assert_eq!(after.len(), 3, "{after:?}");
        assert!(after.iter().any(|(p, _)| p == "b.glyph"), "{after:?}");
    }

    #[test]
    fn a_deleted_file_leaves_the_database() {
        let root = tmp_root();
        write(&root, "a.glyph", DECL);
        write(&root, "b.glyph", IMPORTER);
        let mut server = Server::new(root.clone());
        assert_eq!(refs_at(&mut server, "a.glyph", 1, 7).len(), 3);

        std::fs::remove_file(root.join("b.glyph")).unwrap();

        let after = refs_at(&mut server, "a.glyph", 1, 7);
        assert_eq!(after.len(), 1, "{after:?}");
    }


    /// A server whose databases log the name of every query they execute, and
    /// the log. `WillExecute` is the only event that means work happened.
    ///
    /// salsa renders a `DatabaseKeyIndex` as its query name only while the
    /// database is attached, which it is inside the event callback.
    fn recording_server(root: &Path) -> (Server, Arc<std::sync::Mutex<Vec<String>>>) {
        let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Arc::clone(&log);
        let sink: EventSink = Arc::new(move |event: &glyph_db::Event| {
            if let glyph_db::EventKind::WillExecute { database_key } = &event.kind {
                recorder.lock().unwrap().push(format!("{database_key:?}"));
            }
        });
        (Server::with_event_sink(root.to_path_buf(), sink), log)
    }

    /// A repeat call with nothing changed must execute **zero** salsa queries.
    ///
    /// This is the test that catches the trap the whole refresh is written
    /// around: salsa 0.28 does not backdate an input write, so a refresh that
    /// called `set_text` unconditionally would re-execute `parse_module` for
    /// every file on every call — a cache with a permanent zero hit rate, and
    /// slower than no cache at all, while every other test here still passed.
    #[test]
    fn a_repeat_call_executes_no_queries() {
        let root = tmp_root();
        write(&root, "a.glyph", DECL);
        write(&root, "b.glyph", IMPORTER);

        let (mut server, log) = recording_server(&root);

        let first_answer = refs_at(&mut server, "a.glyph", 1, 7);
        let first: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(first_answer.len(), 3, "{first_answer:?}");
        assert_eq!(
            first.iter().filter(|k| k.starts_with("parse_module")).count(),
            2,
            "the cold call must parse both files: {first:?}"
        );

        let second_answer = refs_at(&mut server, "a.glyph", 1, 7);
        let second: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(second_answer, first_answer);
        assert_eq!(
            second.iter().filter(|k| k.starts_with("parse_module")).count(),
            0,
            "parse_module re-executed on an unchanged repeat call: {second:?}"
        );
        assert!(
            second.is_empty(),
            "an unchanged repeat call executed queries: {second:?}"
        );

        // And an edit still gets through: only the edited file re-parses.
        write(&root, "b.glyph", NO_IMPORT);
        let third_answer = refs_at(&mut server, "a.glyph", 1, 7);
        let third: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(third_answer.len(), 1, "{third_answer:?}");
        assert_eq!(
            third.iter().filter(|k| k.starts_with("parse_module")).count(),
            1,
            "only the edited file should re-parse: {third:?}"
        );
    }

    #[test]
    fn each_project_under_the_root_gets_its_own_database() {
        // Two sibling projects, each marked by a package.json "glyph" key, each
        // with its own `a` module. A reference query in one must not reach into
        // the other (D41).
        let root = tmp_root();
        for project in ["one", "two"] {
            let dir = root.join(project);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
            write(&dir, "a.glyph", DECL);
            write(&dir, "b.glyph", IMPORTER);
        }
        let mut server = Server::new(root.clone());
        let one = refs_at(&mut server, "one/a.glyph", 1, 7);
        assert_eq!(one.len(), 3, "{one:?}");
        assert!(one.iter().all(|(p, _)| p.starts_with("one/")), "{one:?}");
        let two = refs_at(&mut server, "two/a.glyph", 1, 7);
        assert_eq!(two.len(), 3, "{two:?}");
        assert!(two.iter().all(|(p, _)| p.starts_with("two/")), "{two:?}");
        assert_eq!(server.projects.len(), 2);
    }

    #[test]
    fn only_four_databases_stay_live() {
        let root = tmp_root();
        for n in 0..6 {
            let dir = root.join(format!("p{n}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
            write(&dir, "a.glyph", DECL);
        }
        let mut server = Server::new(root.clone());
        for n in 0..6 {
            refs_at(&mut server, &format!("p{n}/a.glyph"), 1, 7);
        }
        assert_eq!(server.projects.len(), MAX_LIVE_DATABASES);
        // Most-recently-used first, so the last project queried is at the front
        // and the first two were evicted. The expectation is canonicalized
        // because the server canonicalizes its root: on macOS a temporary
        // directory under `/var` resolves to `/private/var`, and comparing the
        // two spellings tests the test rather than the code.
        assert_eq!(
            server.projects[0].root,
            std::fs::canonicalize(root.join("p5")).unwrap()
        );
    }

    /// The not-found message must not list a name it would refuse.
    ///
    /// `by_name` holds namespace imports, aliased imports and prelude entries
    /// beside real declarations, and none of those has an identity to return.
    /// Listing them made the error deny a name and then offer it back, which is
    /// a loop rather than a next step.
    #[test]
    fn the_not_found_list_only_holds_names_that_would_answer() {
        let root = tmp_root();
        std::fs::write(root.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
        write(&root, "a.glyph", DECL);
        write(
            &root,
            "c.glyph",
            "module c\nimport a\nimport a as alias_a\npub fn also() -> number {\n  return 1\n}\n",
        );

        let mut server = Server::new(root.clone());
        let (text, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "c.glyph", "name": "a" }),
        );
        assert!(is_error, "a namespace import has no identity to return: {text}");

        let listed = text.split("It declares: ").nth(1).unwrap_or("").to_string();
        // The failing name and the alias both name modules, so neither may be
        // offered as a name that would have worked.
        for refused in ["a", "alias_a"] {
            assert!(
                !listed
                    .split(", ")
                    .any(|n| n.trim().trim_end_matches('.') == refused),
                "the list offers `{refused}`, which this tool refuses: {text}"
            );
        }
        assert!(
            listed.contains("also"),
            "a real declaration should still be listed: {text}"
        );
    }

    /// A `.glyph` file the directory walk skips is not part of the project.
    /// Asking about it answers from its own contents, because the caller asked
    /// about that file; what it must not do is change the answer to a question
    /// about a different file, then or later.
    ///
    /// The second half is the one that matters most. Before this was fixed the
    /// answer to an unchanged question moved from 3 to 5 and back to 3, and the
    /// move back was the LRU evicting the database. An answer that depends on
    /// what else was asked, and reverts when a cache forgets, is worse than a
    /// slow answer.
    #[test]
    fn asking_about_a_file_outside_the_walk_leaves_other_answers_alone() {
        let root = tmp_root();
        for n in 0..5 {
            let dir = root.join(format!("p{n}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
            write(&dir, "a.glyph", DECL);
            write(&dir, "b.glyph", IMPORTER);
        }
        // Under a dot directory, so `collect_glyph_files` never reaches it.
        let hidden = root.join("p0").join(".hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        write(&hidden, "c.glyph", OUTSIDE_IMPORTER);

        let mut server = Server::new(root.clone());

        // Declaration in a, import binding + one use in b.
        let before = refs_at(&mut server, "p0/a.glyph", 1, 7);
        assert_eq!(before.len(), 3, "{before:?}");

        // The question asked *about* the skipped file still reads it: the three
        // above plus its own import binding and use.
        let outside = refs_at(&mut server, "p0/.hidden/c.glyph", 1, 11);
        assert_eq!(outside.len(), 5, "{outside:?}");

        // Nothing changed on disk, so the first question answers the same.
        let after = refs_at(&mut server, "p0/a.glyph", 1, 7);
        assert_eq!(
            after, before,
            "asking about p0/.hidden/c.glyph changed a different file's answer: {after:?}"
        );

        // And it still answers the same once the LRU has evicted p0's database
        // and rebuilt it from disk: eviction must not be what decides.
        for n in 1..5 {
            refs_at(&mut server, &format!("p{n}/a.glyph"), 1, 7);
        }
        let rebuilt = refs_at(&mut server, "p0/a.glyph", 1, 7);
        assert_eq!(
            rebuilt, before,
            "the answer moved when the cache evicted the database: {rebuilt:?}"
        );
    }

    /// A local binding is a single-file question. Answering it must not build
    /// the project: no directory walk, no re-reading every file, no database.
    ///
    /// The sweep used to run before the position was resolved, so a question
    /// whose answer never left the file still paid for the whole project, and
    /// paid more the bigger the project got. On a 340-file tree that was 77 ms
    /// of work thrown away.
    #[test]
    fn a_local_binding_is_answered_without_building_the_project() {
        let root = tmp_root();
        write(&root, "a.glyph", DECL);
        write(&root, "b.glyph", IMPORTER);
        write(
            &root,
            "c.glyph",
            "module c\nfn f() -> number {\n  let total = 1\n  return total\n}\n",
        );
        let mut server = Server::new(root.clone());

        // `total`, declared on line 2 and used on line 3.
        let local = refs_at(&mut server, "c.glyph", 2, 6);
        assert_eq!(local.len(), 2, "{local:?}");
        assert!(local.iter().all(|(p, _)| p == "c.glyph"), "{local:?}");
        assert!(
            server.projects.is_empty(),
            "a local binding walked the project"
        );

        // A position that names nothing is the same question, cheaper.
        let nothing = refs_at(&mut server, "c.glyph", 1, 0);
        assert!(nothing.is_empty(), "{nothing:?}");
        assert!(
            server.projects.is_empty(),
            "an unresolved position walked the project"
        );

        // A global symbol still does build it, and still answers across files.
        let global = refs_at(&mut server, "a.glyph", 1, 7);
        assert_eq!(global.len(), 3, "{global:?}");
        assert_eq!(server.projects.len(), 1);
    }

    /// The same holds for a file the walk does not reach: asking about it twice
    /// must not re-read it into a second salsa input.
    ///
    /// It lives in a slot that every refresh rewrites, and the cheap way to
    /// write that is to build a fresh `SourceFile` each time. That would
    /// re-parse the file on every call and leave the old input behind in a
    /// database that frees nothing.
    #[test]
    fn a_repeat_call_about_a_file_outside_the_walk_executes_no_queries() {
        let root = tmp_root();
        write(&root, "a.glyph", DECL);
        let hidden = root.join(".hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        write(&hidden, "c.glyph", OUTSIDE_IMPORTER);
        let (mut server, log) = recording_server(&root);

        // The declaration in a, plus the import binding and use in the file the
        // walk skips.
        let first = refs_at(&mut server, ".hidden/c.glyph", 1, 11);
        assert_eq!(first.len(), 3, "{first:?}");
        let cold: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(
            cold.iter().filter(|k| k.starts_with("parse_module")).count(),
            2,
            "the cold call must parse both files: {cold:?}"
        );

        let second = refs_at(&mut server, ".hidden/c.glyph", 1, 11);
        let events: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(second, first);
        assert!(
            events.is_empty(),
            "an unchanged repeat about a non-member executed queries: {events:?}"
        );
    }

    // ---- addressing an entity by name ----
    //
    // A position is what an editor has. An agent has a name, and a coordinate
    // it recorded a few edits ago is not the same address it was.

    /// The demonstration the `name` argument exists for.
    ///
    /// Against published 0.1.103 the position was the only address, so an agent
    /// that recorded line 1 and asked again after a declaration was inserted
    /// above got a well-formed answer about the inserted declaration instead.
    /// No error, nothing in the result to notice.
    #[test]
    fn a_name_survives_a_declaration_inserted_above_it() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let mut server = Server::new(root.clone());

        // The declaration in a, the import binding in b, and b's one call.
        let named = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        assert_eq!(named, ["charge", "charge", "charge"], "{named:?}");
        let positioned = ref_names(
            &mut server,
            &root,
            json!({ "path": "a.glyph", "line": 1, "character": 7 }),
        );
        assert_eq!(positioned, named, "the two addresses must start out equal");

        // One unrelated declaration inserted above. Nothing about `charge`
        // changed; every line below it moved.
        write(&root, "a.glyph", AUDIT_ABOVE_CHARGE);

        let named_again =
            ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        assert_eq!(
            named_again,
            ["charge", "charge", "charge"],
            "the name stopped addressing `charge`: {named_again:?}"
        );

        // And the recorded coordinate now answers about the neighbour.
        let positioned_again = ref_names(
            &mut server,
            &root,
            json!({ "path": "a.glyph", "line": 1, "character": 7 }),
        );
        assert_eq!(positioned_again, ["audit"], "{positioned_again:?}");
    }

    /// The inverse, so the guarantee above is not vacuous: a name that no
    /// longer names anything must come back not-found rather than quietly
    /// resolving to whatever is nearby.
    #[test]
    fn a_renamed_entity_is_not_found_under_its_old_name() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let mut server = Server::new(root.clone());
        let before = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        assert_eq!(before, ["charge", "charge", "charge"], "{before:?}");

        // A real rename, both sides of the import, so the project stays valid.
        write(&root, "a.glyph", &CHARGE.replace("charge", "settle"));
        write(&root, "b.glyph", &CHARGE_IMPORTER.replace("charge", "settle"));

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "name": "charge" }),
        );
        assert!(is_error, "the old name still answered: {message}");
        assert!(message.contains("charge"), "{message}");
        assert!(
            message.contains("settle"),
            "the message should say what the module does declare: {message}"
        );

        // The new name answers, so the error above is a missing name and not a
        // broken lookup.
        let after = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "settle" }));
        assert_eq!(after, ["settle", "settle", "settle"], "{after:?}");
    }

    /// Both addresses at once is a cross-check, not a preference: they agree
    /// and the call answers, or they disagree and the call fails naming both.
    ///
    /// This is also the guard that keeps the two lookups honest. The position
    /// path resolves through `symbol_target_at` and the name path through
    /// `named_target`, and the agreeing half of this test fails the moment the
    /// two identity rules drift apart.
    #[test]
    fn a_position_and_a_name_that_disagree_are_an_error() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let mut server = Server::new(root.clone());

        let both = ref_names(
            &mut server,
            &root,
            json!({ "path": "a.glyph", "line": 1, "character": 7, "name": "charge" }),
        );
        assert_eq!(both, ["charge", "charge", "charge"], "{both:?}");

        write(&root, "a.glyph", AUDIT_ABOVE_CHARGE);

        // The stale coordinate covers `audit` now. Answering either side would
        // be a guess, so the caller is told instead.
        let (message, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "line": 1, "character": 7, "name": "charge" }),
        );
        assert!(is_error, "a disagreeing address answered: {message}");
        assert!(
            message.contains("audit") && message.contains("charge"),
            "the error must name both sides: {message}"
        );

        // The moved coordinate agrees with the name again.
        let moved = ref_names(
            &mut server,
            &root,
            json!({ "path": "a.glyph", "line": 4, "character": 7, "name": "charge" }),
        );
        assert_eq!(moved, ["charge", "charge", "charge"], "{moved:?}");
    }

    /// One of the two addresses is required, and a malformed `name` is a
    /// malformed call. Ignoring it and falling back to the position is how a
    /// typo becomes a confident answer about something else.
    #[test]
    fn a_call_needs_exactly_one_kind_of_address() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let mut server = Server::new(root.clone());

        let (message, is_error) =
            call_raw(&mut server, "glyph_references", json!({ "path": "a.glyph" }));
        assert!(is_error, "a call with no address answered: {message}");
        assert!(message.contains("name"), "{message}");

        // A position that would otherwise answer, alongside a `name` that is
        // not a string.
        let (message, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "line": 1, "character": 7, "name": 7 }),
        );
        assert!(is_error, "a non-string `name` was ignored: {message}");
        assert!(message.contains("name"), "{message}");
    }

    /// A name is looked up in the module namespace of the file it is asked
    /// about, and that namespace includes imports, so naming `charge` from the
    /// importing module answers about the module that declares it. That is the
    /// identity the position form already reports for an import binding.
    #[test]
    fn a_name_asked_of_an_importing_module_resolves_to_the_declaring_one() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let mut server = Server::new(root.clone());

        let declaring = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        assert_eq!(declaring, ["charge", "charge", "charge"], "{declaring:?}");
        let importing = ref_names(&mut server, &root, json!({ "path": "b.glyph", "name": "charge" }));
        assert_eq!(importing, declaring, "{importing:?}");

        // `charge` in `import a { charge }`, the position form of the same
        // question.
        let at_binding = ref_names(
            &mut server,
            &root,
            json!({ "path": "b.glyph", "line": 1, "character": 11 }),
        );
        assert_eq!(at_binding, declaring, "{at_binding:?}");
    }

    /// The module namespace is flat, so a hoisted union variant is a top-level
    /// name like any other and can be addressed as one.
    #[test]
    fn a_union_variant_can_be_addressed_by_name() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub type Color = Red | Blue\npub fn pick() -> Color {\n  return Red\n}\n",
        );
        let mut server = Server::new(root.clone());
        let red = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "Red" }));
        assert_eq!(red, ["Red", "Red"], "{red:?}");
    }

    /// A `let` has no name-address: it is not in the module's top-level table,
    /// so naming it is not-found rather than a match on something else. The
    /// position form still answers it, file-scoped, without building the
    /// project.
    #[test]
    fn a_local_binding_cannot_be_addressed_by_name() {
        let root = tmp_root();
        write(
            &root,
            "c.glyph",
            "module c\npub fn f() -> number {\n  let total = 1\n  return total\n}\n",
        );
        let mut server = Server::new(root.clone());

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "c.glyph", "name": "total" }),
        );
        assert!(is_error, "a local answered a name address: {message}");
        assert!(message.contains("total"), "{message}");

        let local = ref_names(
            &mut server,
            &root,
            json!({ "path": "c.glyph", "line": 2, "character": 6 }),
        );
        assert_eq!(local, ["total", "total"], "{local:?}");
        assert!(
            server.projects.is_empty(),
            "a file-scoped question walked the project"
        );
    }

    /// The name form keeps what the position form established: a repeat call
    /// with nothing changed executes no salsa queries, and an edit still gets
    /// through.
    #[test]
    fn a_repeat_call_addressed_by_name_executes_no_queries() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let (mut server, log) = recording_server(&root);

        let first = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        assert_eq!(first, ["charge", "charge", "charge"], "{first:?}");
        let cold: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(
            cold.iter().filter(|k| k.starts_with("parse_module")).count(),
            2,
            "the cold call must parse both files: {cold:?}"
        );

        let second = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        let events: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(second, first);
        assert!(
            events.is_empty(),
            "an unchanged repeat addressed by name executed queries: {events:?}"
        );

        // A declaration inserted above is a real edit: the file re-parses, and
        // the answer is still about `charge`.
        write(&root, "a.glyph", AUDIT_ABOVE_CHARGE);
        let third = ref_names(&mut server, &root, json!({ "path": "a.glyph", "name": "charge" }));
        let after: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(third, ["charge", "charge", "charge"], "{third:?}");
        assert_eq!(
            after.iter().filter(|k| k.starts_with("parse_module")).count(),
            1,
            "only the edited file should re-parse: {after:?}"
        );
    }

    /// The schema says what each tool takes. `path` is the only required
    /// argument on `glyph_references`, where `name` sits beside the position;
    /// `glyph_variants` answers about a type and takes the name and nothing
    /// else. The two tools that answer about an expression or a reference
    /// occurrence rather than about an entity are deliberately position-only.
    #[test]
    fn a_name_addresses_an_entity_and_a_position_addresses_a_cursor() {
        let mut server = Server::new(tmp_root());
        let list = handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            &mut server,
        )
        .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap().clone();
        let spec = |want: &str| {
            tools
                .iter()
                .find(|t| t["name"] == want)
                .unwrap_or_else(|| panic!("missing {want}"))
                .clone()
        };

        let refs = spec("glyph_references");
        assert!(
            refs["inputSchema"]["properties"]["name"].is_object(),
            "{refs}"
        );
        assert_eq!(refs["inputSchema"]["required"], json!(["path"]), "{refs}");

        // A type has no cursor: the relation is keyed by declaration, so the
        // only address is the name.
        let vars = spec("glyph_variants");
        assert!(
            vars["inputSchema"]["properties"]["name"].is_object(),
            "{vars}"
        );
        assert!(
            vars["inputSchema"]["properties"]["line"].is_null(),
            "glyph_variants grew a position argument: {vars}"
        );

        for name in ["glyph_hover", "glyph_definition"] {
            let tool = spec(name);
            assert_eq!(
                tool["inputSchema"]["required"],
                json!(["path", "line", "character"]),
                "{name} changed its required arguments"
            );
            assert!(
                tool["inputSchema"]["properties"]["name"].is_null(),
                "{name} grew a `name` argument"
            );
        }
    }

    // ---- glyph_variants: the match-coverage relation, one type at a time ----
    //
    // An agent about to add a variant to a union needs the sites that will
    // have to change, and it needs them as something it can go to after the
    // lines around them have moved. So the answer is descriptors: the
    // declaration each site sits in, the scrutinee as written, the line, and
    // the arms with the variant each one names.

    /// A union declared in `a` and matched there over every variant.
    const COMMAND_A: &str = "module a\npub type Command =\n  | Up\n  | Down\npub fn run(c: Command) -> number {\n  return match c {\n    Up => 1,\n    Down => 0,\n  }\n}\n";
    /// The same declaration matched in another module through an import, with
    /// an `else` arm. This is the site that keeps compiling when a variant is
    /// added, which is the state worth telling a caller about.
    const COMMAND_B: &str = "module b\nimport a { Command, Up }\npub fn label(c: Command) -> string {\n  return match c {\n    Up => \"up\",\n    else => \"other\",\n  }\n}\n";
    /// `COMMAND_A` with one unrelated declaration inserted above `run`, which
    /// moves the match down three lines and changes nothing else about it.
    const AUDIT_ABOVE_RUN: &str = "module a\npub type Command =\n  | Up\n  | Down\npub fn audit() -> number {\n  return 0\n}\npub fn run(c: Command) -> number {\n  return match c {\n    Up => 1,\n    Down => 0,\n  }\n}\n";
    /// A match on the prelude `Result`, which has a variant table and no
    /// declaration in any project module.
    const RESULT_MATCH: &str = "module a\npub fn f(r: Result<number, string>) -> number {\n  return match r {\n    Ok(n) => n,\n    Err(_e) => 0,\n  }\n}\n";

    /// Call `glyph_variants` and return the parsed answer. A tool error is
    /// prose rather than JSON, so the failure prints the text: parsing it
    /// first turns every unexpected error into the word `null`.
    fn variants(server: &mut Server, path: &str, name: &str) -> Value {
        let (text, is_error) = call_raw(
            server,
            "glyph_variants",
            json!({ "path": path, "name": name }),
        );
        assert!(!is_error, "{text}");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    /// The core answer: every site over one declaration, each as a descriptor.
    #[test]
    fn the_variants_tool_lists_every_site_over_one_type() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "b.glyph", COMMAND_B);
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Command");
        assert_eq!(answer["type"]["kind"], "declaration", "{answer}");
        assert_eq!(answer["type"]["declaration"], "a::Command", "{answer}");

        let sites = answer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 2, "{answer}");

        // Module-path order, so `a`'s site comes first.
        assert_eq!(sites[0]["declaration"], "a::run", "{answer}");
        assert_eq!(sites[0]["path"], "a.glyph", "{answer}");
        assert_eq!(sites[0]["scrutinee"], "c", "{answer}");
        assert_eq!(sites[0]["line"], 5, "{answer}");
        assert_eq!(sites[0]["state"], "exhaustive", "{answer}");
        assert_eq!(
            sites[0]["arms"],
            json!([{ "arm": 0, "variant": "Up" }, { "arm": 1, "variant": "Down" }]),
            "{answer}"
        );

        assert_eq!(sites[1]["declaration"], "b::label", "{answer}");
        assert_eq!(sites[1]["path"], "b.glyph", "{answer}");
        assert_eq!(sites[1]["state"], "has_catch_all", "{answer}");
        assert_eq!(sites[1]["arms"], json!([{ "arm": 0, "variant": "Up" }]), "{answer}");
        assert_eq!(sites[1]["catch_all"], json!([1]), "{answer}");

        // The relation is a project-wide query, so it goes through the
        // per-project database rather than re-reading the tree.
        assert_eq!(server.projects.len(), 1);
    }

    /// Adding a variant is the edit this tool exists for, and the two sites
    /// answer differently: the exhaustive one is now short a variant and the
    /// compiler says so, the catch-all one still compiles and swallows it.
    ///
    /// Also the no-stale-answers property: the union is rewritten on disk
    /// behind the server's back, with no notification and no restart.
    #[test]
    fn adding_a_variant_names_the_gap_and_leaves_the_catch_all_silent() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "b.glyph", COMMAND_B);
        let mut server = Server::new(root.clone());

        let before = variants(&mut server, "a.glyph", "Command");
        assert_eq!(before["sites"][0]["state"], "exhaustive", "{before}");

        write(&root, "a.glyph", &COMMAND_A.replace("  | Down\n", "  | Down\n  | Left\n"));

        let after = variants(&mut server, "a.glyph", "Command");
        let sites = after["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 2, "{after}");
        assert_eq!(sites[0]["state"], "declined", "{after}");
        assert_eq!(sites[0]["missing"], json!(["Left"]), "{after}");
        // The dangerous one: nothing is missing here, because `else` absorbs
        // the variant that was just added.
        assert_eq!(sites[1]["state"], "has_catch_all", "{after}");
        assert!(sites[1]["missing"].is_null(), "{after}");
    }

    /// A prelude union has a name and no declaration. Reporting it needs no
    /// key, and inventing one would name a module no project has.
    #[test]
    fn a_builtin_union_is_named_with_no_declaration_to_key() {
        let root = tmp_root();
        write(&root, "a.glyph", RESULT_MATCH);
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Result");
        assert_eq!(answer["type"]["kind"], "builtin", "{answer}");
        assert_eq!(answer["type"]["name"], "Result", "{answer}");
        assert!(answer["type"]["declaration"].is_null(), "{answer}");

        let sites = answer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 1, "{answer}");
        assert_eq!(sites[0]["declaration"], "a::f", "{answer}");
        assert_eq!(sites[0]["scrutinee"], "r", "{answer}");
        assert_eq!(
            sites[0]["arms"],
            json!([{ "arm": 0, "variant": "Ok" }, { "arm": 1, "variant": "Err" }]),
            "{answer}"
        );
    }

    /// A payload union's variants are not this type's variants.
    ///
    /// `Ok(Some(n))` names `Ok` of the scrutinee and `Some` of a different
    /// declaration one level down. Reporting both in `arms` would tell a
    /// caller that `Result` has a variant called `Some`. The unions
    /// underneath are named instead, because the state covers them: a site
    /// can read short of exhaustive with nothing missing at depth 0.
    #[test]
    fn a_payload_unions_variants_are_not_reported_as_this_types_arms() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub fn f(r: Result<Option<number>, string>) -> number {\n  return match r {\n    Ok(Some(n)) => n,\n    Ok(None) => 0,\n    Err(_e) => 1,\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Result");
        let sites = answer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 1, "{answer}");
        assert_eq!(
            sites[0]["arms"],
            json!([
                { "arm": 0, "variant": "Ok" },
                { "arm": 1, "variant": "Ok" },
                { "arm": 2, "variant": "Err" },
            ]),
            "{answer}"
        );
        assert_eq!(sites[0]["payload_unions"], json!(["Option"]), "{answer}");

        // And asking about the payload union finds the same site, which is
        // the half that matters: adding a variant to `Option` breaks this
        // match, and `sites_over` cannot reach it because the relation files
        // the site under `Result`. Named, not left out.
        let option = variants(&mut server, "a.glyph", "Option");
        assert_eq!(option["type"]["kind"], "builtin", "{option}");
        assert_eq!(option["sites"], json!([]), "{option}");
        let nested = option["nested"].as_array().unwrap();
        assert_eq!(nested.len(), 1, "{option}");
        assert_eq!(nested[0]["declaration"], "a::f", "{option}");
        assert_eq!(nested[0]["scrutinee"], "r", "{option}");
        // What the site itself matches on, so the entry cannot be misread as a
        // site over `Option`.
        assert_eq!(nested[0]["type"]["name"], "Result", "{option}");
        assert_eq!(
            nested[0]["arms"],
            json!([
                { "arm": 0, "depth": 1, "variant": "Some" },
                { "arm": 1, "depth": 1, "variant": "None" },
            ]),
            "{option}"
        );
    }

    /// The same story for two declarations of this project, and with a gap:
    /// `A(X)` names `X` of `Inner` one level down and leaves `Y` unmentioned,
    /// which is the E0200 the checker reports against `Inner` inside a site
    /// filed under `Outer`.
    #[test]
    fn a_payload_gap_is_reported_against_the_union_that_is_short_a_variant() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub type Inner =\n  | X\n  | Y\npub type Outer =\n  | A(Inner)\n  | B\npub fn f(o: Outer) -> number {\n  return match o {\n    A(X) => 1,\n    B => 2,\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let outer = variants(&mut server, "a.glyph", "Outer");
        let sites = outer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 1, "{outer}");
        assert_eq!(
            sites[0]["arms"],
            json!([{ "arm": 0, "variant": "A" }, { "arm": 1, "variant": "B" }]),
            "{outer}"
        );
        // The union underneath is named, so the state is followable.
        assert_eq!(sites[0]["payload_unions"], json!(["Inner"]), "{outer}");

        let inner = variants(&mut server, "a.glyph", "Inner");
        assert_eq!(inner["type"]["declaration"], "a::Inner", "{inner}");
        assert_eq!(inner["sites"], json!([]), "{inner}");
        let nested = inner["nested"].as_array().unwrap();
        assert_eq!(nested.len(), 1, "{inner}");
        assert_eq!(nested[0]["type"]["declaration"], "a::Outer", "{inner}");
        assert_eq!(
            nested[0]["arms"],
            json!([{ "arm": 0, "depth": 1, "variant": "X" }]),
            "{inner}"
        );
        assert_eq!(nested[0]["missing"], json!(["Y"]), "{inner}");
    }

    /// A site whose type end this project cannot key is named, not counted,
    /// and not dropped.
    ///
    /// The file's own `module` line disagrees with where the file sits, so the
    /// declaration the typechecker names is under a module the project has
    /// never heard of. Asked the sensible way, by the name the file declares,
    /// the tool reports the declaration it *can* key with no sites, and names
    /// the site it cannot join to it. Answering `[]` alone would assert that
    /// no match site is over this type, which is false.
    #[test]
    fn a_site_the_project_cannot_key_is_named_rather_than_absent() {
        let root = tmp_root();
        write(&root, "models.glyph", &COMMAND_A.replace("module a", "module app/models"));
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "models.glyph", "Command");
        assert_eq!(answer["sites"], json!([]), "{answer}");

        let unkeyed = answer["unkeyed"].as_array().unwrap();
        assert_eq!(unkeyed.len(), 1, "{answer}");
        assert_eq!(unkeyed[0]["type"]["kind"], "unkeyed", "{answer}");
        assert_eq!(unkeyed[0]["type"]["module"], "app/models", "{answer}");
        assert!(unkeyed[0]["type"]["declaration"].is_null(), "{answer}");
        // The whole descriptor, not a count.
        assert_eq!(unkeyed[0]["declaration"], "models::run", "{answer}");
        assert_eq!(unkeyed[0]["scrutinee"], "c", "{answer}");
        assert_eq!(unkeyed[0]["line"], 5, "{answer}");
        assert_eq!(unkeyed[0]["state"], "exhaustive", "{answer}");
        assert_eq!(
            unkeyed[0]["arms"],
            json!([{ "arm": 0, "variant": "Up" }, { "arm": 1, "variant": "Down" }]),
            "{answer}"
        );
    }

    /// A site the project cannot key is still not absent when it reaches the
    /// mismatched type through a payload rather than as its own scrutinee.
    ///
    /// `a_site_the_project_cannot_key_is_named_rather_than_absent` covers the
    /// direct case; this is the same module-line/path mismatch (G172) with a
    /// payload site layered on top, which is exactly the shape
    /// `glyph_variants` silently dropped: neither a `sites` entry (the site's
    /// own scrutinee is `Result`, not `Command`), nor an `unkeyed` entry
    /// (`unkeyed` is keyed off the site's own scrutinee type, which is a
    /// `Builtin`, not an `Unkeyed` namesake), nor a `nested` entry (`edge_is`
    /// compares the mention's module string, taken from the file's own
    /// `module` line, against the declaration's *path-derived* module, and a
    /// mismatched file never has those agree). An agent asking "which sites
    /// change if I add a variant to `Command`" would miss this one, which is
    /// the one case where the compiler itself already flags the omission as
    /// E0200.
    #[test]
    fn a_payload_site_the_project_cannot_key_is_named_rather_than_absent() {
        let root = tmp_root();
        write(
            &root,
            "models.glyph",
            "module app/models\npub type Command =\n  | Up\n  | Down\npub fn run(c: Command) -> number {\n  return match c {\n    Up => 1,\n    Down => 0,\n  }\n}\npub fn from_result(r: Result<Command, string>) -> string {\n  return match r {\n    Ok(Up) => \"ok-up\",\n    Err(e) => e,\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "models.glyph", "Command");
        assert_eq!(answer["sites"], json!([]), "{answer}");

        // The direct site still reports under `unkeyed`, same as the sibling
        // test with no payload involved.
        let unkeyed = answer["unkeyed"].as_array().unwrap();
        assert_eq!(unkeyed.len(), 1, "{answer}");
        assert_eq!(unkeyed[0]["declaration"], "models::run", "{answer}");

        // The payload site must show up somewhere rather than vanish. It
        // reaches `Command` one level into `Result`, so `nested` is where the
        // clean-module sibling test (`a_payload_gap_is_reported_against_the_
        // union_that_is_short_a_variant`) puts the same shape of site.
        let nested = answer["nested"]
            .as_array()
            .unwrap_or_else(|| panic!("from_result must appear somewhere, not be absent: {answer}"));
        assert_eq!(nested.len(), 1, "{answer}");
        assert_eq!(nested[0]["declaration"], "models::from_result", "{answer}");
        assert_eq!(nested[0]["type"]["kind"], "builtin", "{answer}");
        assert_eq!(nested[0]["type"]["name"], "Result", "{answer}");
        assert_eq!(
            nested[0]["arms"],
            json!([{ "arm": 0, "depth": 1, "variant": "Up" }]),
            "{answer}"
        );
        // The genuine E0200 gap: `Down` never appears under the payload.
        assert_eq!(nested[0]["missing"], json!(["Down"]), "{answer}");
    }

    /// A display name is not an address. Asked from a file that names neither
    /// of two same-named declarations, the tool says so instead of picking.
    #[test]
    fn a_name_two_modules_declare_is_an_error_rather_than_a_pick() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "c.glyph", &COMMAND_A.replace("module a", "module c"));
        write(&root, "z.glyph", "module z\npub fn zed() -> number {\n  return 1\n}\n");
        let mut server = Server::new(root.clone());

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_variants",
            json!({ "path": "z.glyph", "name": "Command" }),
        );
        assert!(is_error, "an ambiguous name answered: {message}");
        assert!(
            message.contains("a::Command") && message.contains("c::Command"),
            "the error must name both declarations: {message}"
        );

        // And from a file that does name one, the same question answers.
        let answer = variants(&mut server, "a.glyph", "Command");
        assert_eq!(answer["type"]["declaration"], "a::Command", "{answer}");
        assert_eq!(answer["sites"].as_array().unwrap().len(), 1, "{answer}");
    }

    /// The relation is keyed by declaration, and a function has a key too, so
    /// asking about one would answer `sites: []` and read as "nothing matches
    /// on this type". The kind is in the file's own table; the answer is an
    /// error that says what the name is.
    #[test]
    fn a_name_that_does_not_name_a_type_is_refused() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        let mut server = Server::new(root.clone());

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_variants",
            json!({ "path": "a.glyph", "name": "run" }),
        );
        assert!(is_error, "a function answered: {message}");
        assert!(message.contains("run"), "{message}");

        // A variant is not its union, and the union is one hop away.
        let (message, is_error) = call_raw(
            &mut server,
            "glyph_variants",
            json!({ "path": "a.glyph", "name": "Up" }),
        );
        assert!(is_error, "a variant answered: {message}");
        assert!(
            message.contains("Up") && message.contains("Command"),
            "the error must point at the union: {message}"
        );
    }

    /// Zero sites is a real answer, and it still has to say which type it is
    /// about.
    #[test]
    fn a_type_nothing_matches_on_answers_no_sites() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub type Command =\n  | Up\n  | Down\npub fn run(c: Command) -> Command {\n  return c\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Command");
        assert_eq!(answer["type"]["declaration"], "a::Command", "{answer}");
        assert_eq!(answer["sites"], json!([]), "{answer}");
        assert!(answer["unkeyed"].is_null(), "{answer}");
    }

    /// A name the module does not hold is not-found, listing what it does
    /// hold. `[]` would read as "that type has no match site".
    #[test]
    fn a_name_nothing_declares_is_not_found() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        let mut server = Server::new(root.clone());

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_variants",
            json!({ "path": "a.glyph", "name": "Kommand" }),
        );
        assert!(is_error, "a name nothing declares answered: {message}");
        assert!(message.contains("Kommand") && message.contains("Command"), "{message}");
    }

    /// Two properties in one: a repeat call with nothing changed executes no
    /// salsa query, and an edit still gets through. The second half is also
    /// what the descriptor is for: the match moves down three lines and the
    /// declaration it sits in is still `a::run`.
    #[test]
    fn a_repeat_variants_call_executes_no_queries() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "b.glyph", COMMAND_B);
        let (mut server, log) = recording_server(&root);

        let first = variants(&mut server, "a.glyph", "Command");
        let cold: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(first["sites"].as_array().unwrap().len(), 2, "{first}");
        assert!(
            cold.iter().any(|k| k.starts_with("project_match_coverage")),
            "the cold call must run the fold: {cold:?}"
        );

        let second = variants(&mut server, "a.glyph", "Command");
        let warm: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(second, first);
        assert!(
            warm.is_empty(),
            "an unchanged repeat call executed queries: {warm:?}"
        );

        write(&root, "a.glyph", AUDIT_ABOVE_RUN);
        let third = variants(&mut server, "a.glyph", "Command");
        let after: Vec<String> = std::mem::take(&mut log.lock().unwrap());
        assert_eq!(third["sites"][0]["declaration"], "a::run", "{third}");
        assert_eq!(third["sites"][0]["line"], 8, "{third}");
        assert_eq!(
            after.iter().filter(|k| k.starts_with("parse_module")).count(),
            1,
            "only the edited file should re-parse: {after:?}"
        );
    }

    /// Module paths are counted per project (D41), and each project's keys
    /// come from its own interner, so one project's answer must hold none of
    /// the other's sites.
    #[test]
    fn each_projects_variants_answer_holds_only_its_own_sites() {
        let root = tmp_root();
        for project in ["one", "two"] {
            let dir = root.join(project);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
            write(&dir, "a.glyph", COMMAND_A);
            write(&dir, "b.glyph", COMMAND_B);
        }
        let mut server = Server::new(root.clone());

        let one = variants(&mut server, "one/a.glyph", "Command");
        let one_paths: Vec<&str> = one["sites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["path"].as_str().unwrap())
            .collect();
        assert_eq!(one_paths, ["one/a.glyph", "one/b.glyph"], "{one}");

        let two = variants(&mut server, "two/a.glyph", "Command");
        let two_paths: Vec<&str> = two["sites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["path"].as_str().unwrap())
            .collect();
        assert_eq!(two_paths, ["two/a.glyph", "two/b.glyph"], "{two}");
        assert_eq!(server.projects.len(), 2);
    }

    /// The description is what a consumer reads before it acts on a state, so
    /// it has to say what each state means, and in particular that a
    /// catch-all site keeps compiling and takes the new variant silently.
    #[test]
    fn the_variants_tool_says_what_its_states_mean() {
        let mut server = Server::new(tmp_root());
        let list = handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            &mut server,
        )
        .unwrap();
        let spec = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "glyph_variants")
            .expect("missing glyph_variants")
            .clone();

        assert_eq!(
            spec["inputSchema"]["required"],
            json!(["path", "name"]),
            "{spec}"
        );
        let described = spec["description"].as_str().unwrap();
        for state in [
            "exhaustive",
            "has_catch_all",
            "declined",
            "scrutinee_unresolved",
        ] {
            assert!(described.contains(state), "`{state}` is undescribed: {described}");
        }
        for phrase in ["catch-all", "silently", "compil"] {
            assert!(
                described.contains(phrase),
                "the description must say why a catch-all is the dangerous state: {described}"
            );
        }
        // The two lists that are not `sites` exist because an answer must not
        // report a site absent, so the description has to say what is in them.
        for phrase in ["nested", "unkeyed"] {
            assert!(
                described.contains(phrase),
                "the description must say what `{phrase}` holds: {described}"
            );
        }
    }
}
