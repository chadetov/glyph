//! A minimal Model Context Protocol (MCP) server exposing Glyph's language
//! analysis to a coding agent as tools. It speaks JSON-RPC 2.0 over stdio with
//! newline-delimited messages (the MCP stdio transport), and reuses the pure
//! `crate::analysis` queries — hover, go-to-definition, workspace references,
//! symbol search, and diagnostics — so the agent surface is a thin adapter over
//! the same semantics the editor path uses, not a second implementation.
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

use glyph_db::{CompilerDb, EventSink, Setter, SourceFile};
use glyph_resolver::{
    build_prelude, collect_module_symbols, resolve_module, ResolvedModule, StdlibStubs,
    SymbolKind,
};

use crate::analysis::{
    analyze, analyze_full, global_occurrences_in, outline_of, references_at, symbol_target_at,
    Definition, LineIndex, OutlineKind, OutlineSymbol, SymbolTarget,
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

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "glyph-mcp", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn tool_specs() -> Value {
    let file = json!({ "type": "string", "description": "Path to a .glyph file, relative to the project root or absolute." });
    let line = json!({ "type": "integer", "description": "0-based line number." });
    let character = json!({ "type": "integer", "description": "0-based character offset (UTF-16 code units)." });
    let name = json!({ "type": "string", "description": "Name of a top-level declaration, a tagged-union variant, or an imported binding in that file. Addresses the symbol itself, so the answer stays about the same symbol when declarations above it are added or removed. A local binding has no name; address one by position." });
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
        "glyph_symbols" => tool_symbols(&args, &root),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn tool_diagnostics(args: &Value, root: &Path) -> Result<String, String> {
    let (_, text) = read_file(args, root)?;
    let index = LineIndex::new(&text);
    let items: Vec<Value> = analyze(&text)
        .into_iter()
        .map(|d| {
            json!({
                "code": d.code,
                "message": d.message,
                "range": range_json(&index, &text, d.start, d.end),
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

    /// The schema says what the tool takes: `path` is the only required
    /// argument on `glyph_references`, and `name` sits beside the position. The
    /// two tools that answer about an expression or a reference occurrence
    /// rather than about an entity are deliberately left position-only.
    #[test]
    fn only_the_references_tool_takes_a_name() {
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
}
