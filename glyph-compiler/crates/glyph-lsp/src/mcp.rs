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
    build_prelude, collect_module_symbols, resolve_module, DeclKey, ModuleGraph, Prelude,
    ResolvedModule, StdlibStubs, SymbolKind,
};
use glyph_typechecker::{
    CoverageSiteRef, CoverageState, CoverageTypeName, FieldAccess, FieldOwner, FieldSite,
};

use crate::analysis::{
    analyze, analyze_full, enclosing_decl_name, global_relations_in, module_outline, outline_of,
    relations_at, symbol_target_at, Definition, LineIndex, OutlineKind, OutlineSymbol,
    RelatedSpan, Relation, SymbolTarget,
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
            "Before adding a variant to a tagged union, call `glyph_variants` ",
            "with `proposed_variant` set to the name you are adding. It answers ",
            "per match site: `WILL_FAIL` (the site stops compiling and the ",
            "compiler points at it), `ABSORBS` (a catch-all takes the new ",
            "variant silently, which is the dangerous one because nothing ",
            "fails), `UNDETERMINED`, `NOT_INDEXED`. Without `proposed_variant` ",
            "it lists the sites and what each one does today.\n\n",
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
    let name = json!({ "type": "string", "description": "Name of a top-level declaration, a tagged-union variant, or an imported binding in that file. Addresses the symbol itself, so the answer stays about the same symbol when declarations above it are added or removed. A local binding has no name; address one by position. A record field is addressed as `Record.field` (`User.email`), with the record named the way the file at `path` names it: a bare field name is not an address, since two records in one module can each declare a field of that name. The field form answers `{ entity, sites, unkeyed, unindexed, not_indexed }` rather than the relation split, because it reads a different relation: `sites` are the field's own declaration and every member access the checker resolved onto that record, each with `access` (`declaration`, `read`, `write`, `redact`) and a range covering the field's name alone, so a rename can write from it; `unkeyed` holds sites that spell the field on an object whose type never resolved to a field set, which the compiler never joined to any record and which are named rather than dropped; `unindexed` names the project files the sweep could not read, one by one, since a file that does not parse holds field sites this answer cannot see; and `not_indexed` names the classes of site the relation does not hold at all, of which a record literal constructing the record is one." });
    let type_name = json!({ "type": "string", "description": "Name of a tagged union, as the file at `path` names it: one it declares, one it imports, or a prelude or stdlib union (`Result`, `Option`, `fs.ErrorKind`). The module the name resolves to is what picks out one declaration when several modules declare the same name." });
    let relation = json!({
        "type": ["string", "array"],
        "items": { "type": "string", "enum": ["CALLS", "REFERENCES"] },
        "description": "Optional. Which relations to answer, as a name or an array of names. The vocabulary is closed and holds exactly two: `CALLS`, the sites that apply the symbol to an argument list, and `REFERENCES`, every other occurrence. Leave it out to get both. A name outside the vocabulary is an error rather than an ignored key."
    });
    let proposed_variant = json!({ "type": "string", "description": "Optional. The name of a variant you are about to add to this union. Sending it makes the answer about that edit: every site carries a `consequence` beside its state, and a name the union already has is refused rather than answered." });
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
            "description": "Every edge into a symbol across the whole project, split by relation. Address the symbol by position (`line` and `character`, what an editor has under its cursor) or by `name` (a declaration in that file, which still means the same symbol after the lines above it move). Sending both checks one against the other, and a call whose position and name are different symbols is an error rather than a guess. A local binding is file-scoped and can only be addressed by position. The answer is `{ entity, provenance, relations }`, and `relations` holds one entry per relation asked for. The vocabulary is closed at two. `CALLS`: the site applies the symbol to an argument list, so it stops compiling when the parameters, the arity, or a variant payload change. The callee has to be the name itself, so `io.println()` calls a member of `io` and not `io`, and a call through a local alias calls the alias; applying a tagged-union variant (`Ok(3)`) is a call, since the site breaks the same way when the payload changes. `REFERENCES`: every other occurrence, which is the declaration's own name, an import binding, a type annotation naming the symbol, a value read, the symbol passed as an argument rather than applied to one, a match pattern naming a variant, and a JSX element naming a component. Together the two lists are every occurrence, so asking for one loses no site. Each edge carries `relation`, `from` (the declaration it sits in, as `module::name`, or null with `from_absent` when it is at module level), `to`, `provenance`, and where it is, so an entry read on its own still says what it is about. `provenance` says whether the far end is a fact or a claim, and the three values mean one thing each. `PROVED`: the symbol is declared by a `.glyph` module this project holds, or by a stdlib module the compiler carries and checks the export list of. `ASSERTED`: no Glyph module declares it and a TypeScript declaration does, either a `declare module` in a `.d.ts` this project carries or an installed package of that name, so `tsc` checks the far end and Glyph's resolver never read it; `provenance_detail` names the evidence. `UNDETERMINED`: neither, and the detail says what was checked. Each relation also carries `unindexed`, the files the sweep could not read, named one by one rather than counted: a project file that does not parse holds occurrences this answer cannot see, and leaving it out would make a partial list look like a complete one. Coverage is stated per relation and never per answer.",
            "inputSchema": { "type": "object", "properties": { "path": file, "line": line, "character": character, "name": name, "relation": relation }, "required": ["path"] }
        },
        {
            "name": "glyph_variants",
            "description": "Every match site in the project over one tagged union, and which variants each site's arms name. Use it before adding or removing a variant: it is the list of places that have to change. Each site carries the declaration it sits in (as `module::name`), the scrutinee as written in the source, its line, and the arm ordinals with the variant each one names, so you can go to it after the lines around it have moved. `state` says what the compiler concluded, and the four states are not equally safe. `exhaustive`: every variant is named and no arm was skipped, so adding a variant breaks this site and the compiler will point you at it. `has_catch_all`: one of the arms absorbs everything the earlier arms did not name, so adding a variant leaves this site compiling and silently routes the new variant to the catch-all, which is more dangerous than a site that fails to compile because nothing tells you it is now wrong. `declined`: the checker either read an arm it does not model or found variants no arm names, and `missing` lists those. `scrutinee_unresolved`: the scrutinee's type never resolved, so nothing about the site is checked today. A site that reaches this type through a payload rather than as its own scrutinee (`Ok(Some(n))` reaching `Option` inside a match on `Result`) is filed under the type it matches on, so it is listed under `nested` instead, with the depth on each arm and the type it does match on. Those sites break the same way when a variant is added, so read both lists. A union with no declaration in this project (a prelude or stdlib one) is reported under its name with no declaration to go to, and a site whose type this project cannot key is listed under `unkeyed` rather than left out of the answer. The `type` block carries the union's own `variants` in declaration order, or an explicit `null` with `variants_unavailable` saying why they could not be read. A name that turns out not to be a tagged union at all, a record for instance, is refused rather than answered with an empty site list, because an empty list means a union nothing matches on. Send `proposed_variant` to ask what your edit does rather than what is there. Each site then carries a `consequence`: `WILL_FAIL`, the site stops compiling once the variant exists and the compiler points at it; `ABSORBS`, the site keeps compiling and an arm silently takes the new variant, which is the one nothing will tell you about; `UNDETERMINED`, the compiler concluded nothing about this site, either an arm it read nothing from or a scrutinee whose type never resolved, so it cannot say; `NOT_INDEXED`, a site under `unkeyed`, which this project never joined to the type. A proposed name the union already has is refused, since that is not the change it looks like. `summary` is the arithmetic over that list, so two callers reading one answer reach the same figures: `sites` and `files` are the totals, `consequences` (or `states`, without `proposed_variant`) is the breakdown, and `lines` renders them as the sentences a reader wants, counts aligned. Every total states what it could not count, in `not_counted` and in `lines` both, and `not_counted` is present and empty rather than absent when a total covers everything: sites that reach the type through a payload (`nested`), sites this project could not key to it (`unkeyed`), sites filed under a module the project's file list no longer holds, and files the sweep never read, whose site count is `null` because it is unknown rather than zero. A count reads as authoritative in a way a list does not, so a total that silently left any of those out would be a partial list with a figure in front of it. `unindexed` names those files one by one, since a project file that does not parse or does not resolve holds match sites this answer cannot see.",
            "inputSchema": { "type": "object", "properties": { "path": file, "name": type_name, "proposed_variant": proposed_variant }, "required": ["path", "name"] }
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
    // so the module half is a pure transform of the path already given to the
    // tool rather than a minted `ModuleId`. It is counted from the file's
    // project, never from the server's root: see `module_key`.
    let module_str = module_key(&path, root);
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
            // root (D41), which is the marked ancestor when there is one and
            // the file's own directory otherwise (see `module_root_for`).
            let file = crate::module_root_for(&path, root)
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
fn unresolvable(
    address: &Address,
    this_module: &str,
    why: &str,
    wanted: &[Relation],
) -> Result<String, String> {
    match address.name() {
        None => Ok(relations_answer(
            None,
            Some(&format!("{why}, so the position addresses no symbol")),
            &Provenance::Undetermined(format!(
                "{why}, so there is no far end to have proved or asserted"
            )),
            wanted,
            BTreeMap::new(),
            &[],
        )),
        Some(name) => Err(format!(
            "cannot look up `{name}` in module `{this_module}`: {why}. \
             Run glyph_diagnostics on the file."
        )),
    }
}


/// The one reason an edge has no `from`: the occurrence sits at module level,
/// outside every declaration. An `import` binding is the case that happens in
/// practice; the `module` header is the other position that can hold one.
const MODULE_LEVEL: &str = "the occurrence is at module level, inside no declaration";

/// Which relations one call asked for, in answer order.
///
/// Absent means every relation, which is what a caller from before the argument
/// existed was asking for: the two lists together are the flat occurrence list
/// this tool used to return, so an old call keeps every location it had and
/// gains the label on each one.
///
/// A name outside the vocabulary is an error. The vocabulary is closed, so a
/// misspelled relation is a question this tool cannot answer, and a silently
/// dropped one would come back as an empty list that reads as "no such edges
/// exist".
fn read_relations(args: &Value) -> Result<Vec<Relation>, String> {
    let vocabulary = || Relation::all().map(|r| r.wire()).join(", ");
    let asked: Vec<String> = match args.get("relation") {
        None | Some(Value::Null) => return Ok(Relation::all().to_vec()),
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => {
            if items.is_empty() {
                return Err(format!(
                    "`relation` is an empty array, which asks for nothing. Leave it out to \
                     ask for every relation ({}).",
                    vocabulary()
                ));
            }
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(s) => out.push(s.to_string()),
                    None => {
                        return Err(format!(
                            "`relation` takes a relation name or an array of them; the array \
                             holds `{item}`."
                        ))
                    }
                }
            }
            out
        }
        Some(other) => {
            return Err(format!(
                "`relation` takes a relation name or an array of them, got `{other}`."
            ))
        }
    };
    let mut wanted = Vec::new();
    for wire in &asked {
        let Some(relation) = Relation::from_wire(wire) else {
            return Err(format!(
                "`{wire}` is not a relation. The vocabulary is closed and holds {}.",
                vocabulary()
            ));
        };
        wanted.push(relation);
    }
    // Answer order is the vocabulary's, not the request's, so one question gets
    // one answer however the caller happened to list it.
    Ok(Relation::all()
        .into_iter()
        .filter(|r| wanted.contains(r))
        .collect())
}

/// What stands behind the far end of an edge, and so whether the edge is
/// something the compiler established or something a TypeScript declaration
/// claimed.
///
/// The two are different facts wearing the same shape. An edge into a `.glyph`
/// declaration this project holds was checked by Glyph's own resolver against a
/// declaration it parsed. An edge into a name a `.d.ts` declares was checked by
/// `tsc` against a file no Glyph pass ever read, so the far end exists because
/// a declaration says it does. Merged into one list they are indistinguishable,
/// and a caller acting on the list cannot tell which half a rename is safe in.
///
/// The third member is not a hedge. It is the case where neither holds, and the
/// honest answer is to say what was checked instead of rounding to the nearer
/// of the two.
enum Provenance {
    Proved(String),
    Asserted(String),
    Undetermined(String),
}

impl Provenance {
    fn wire(&self) -> &'static str {
        match self {
            Provenance::Proved(_) => "PROVED",
            Provenance::Asserted(_) => "ASSERTED",
            Provenance::Undetermined(_) => "UNDETERMINED",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Provenance::Proved(d) | Provenance::Asserted(d) | Provenance::Undetermined(d) => d,
        }
    }
}

/// Whether the far end of an edge into `sym_module::name` was proved by the
/// compiler, asserted by a declaration file, or is neither.
///
/// The tests run in order of what they can establish. A `.glyph` member of this
/// project is the strongest: the module is a file on disk that the walk reached
/// and the pipeline parsed and resolved. The compiler's own stdlib surface is
/// next, and it is checked against the export list rather than assumed, so a
/// stdlib module asked for a name it does not export is reported as
/// undetermined rather than proved. Only then does a declaration file decide,
/// and the answer names the evidence.
fn symbol_provenance(
    project: &Project,
    root: &Path,
    sym_module: &str,
    name: &str,
) -> Provenance {
    if let Some((fpath, _)) = project
        .searched()
        .into_iter()
        .find(|(_, f)| f.module_path == sym_module)
    {
        return Provenance::Proved(format!(
            "`{sym_module}` is {}, a Glyph module this project holds; the compiler parsed and \
             resolved the declaration this edge points at",
            display_path(root, fpath)
        ));
    }
    // The stdlib's surface is the compiler's own, which is why it is not a
    // declaration-file claim: the resolver holds the export list and the
    // emitter writes the implementation.
    let stdlib = StdlibStubs::new();
    if let Some(exports) = stdlib.exports_of(&module_path_from_key(sym_module)) {
        return if exports.contains(name) {
            Provenance::Proved(format!(
                "`{sym_module}` is a stdlib module the compiler carries, and it exports `{name}`"
            ))
        } else {
            Provenance::Undetermined(format!(
                "`{sym_module}` is a stdlib module the compiler carries and it does not export \
                 `{name}`, so this import does not resolve and there is no declaration at the \
                 far end to have proved or asserted"
            ))
        };
    }
    match asserting_declaration(&project.root, sym_module) {
        Some(evidence) => Provenance::Asserted(format!(
            "no Glyph module in this project is named `{sym_module}`; {evidence} declares it, so \
             `{name}` exists because a TypeScript declaration says so and `tsc` is what checks \
             it. Glyph's resolver never read the declaration"
        )),
        None => Provenance::Undetermined(format!(
            "`{sym_module}` is not a Glyph module this project holds, not a stdlib module the \
             compiler carries, and nothing under {} declares it: no `declare module` in a \
             `.d.ts` and no installed package of that name. What `{name}` is cannot be \
             established from this project",
            display_path(root, &project.root)
        )),
    }
}

/// `sym_module` as a `ModulePath`, for asking the stdlib surface about it.
///
/// The span is a placeholder: `exports_of` keys on the segments alone, and this
/// path is a lookup key rather than something any diagnostic points at.
fn module_path_from_key(sym_module: &str) -> glyph_ast::ModulePath {
    glyph_ast::ModulePath {
        segments: sym_module
            .split('/')
            .map(|s| std::sync::Arc::from(s) as glyph_ast::Ident)
            .collect(),
        span: glyph_lexer::Span::new(0, 0),
    }
}

/// The TypeScript declaration that claims module `name`, named so an answer can
/// point at its evidence, or `None` when nothing under the project does.
///
/// Two forms count, and they are the two ways a Glyph import reaches something
/// no Glyph file declares: an ambient `declare module "name"` in a `.d.ts` the
/// project carries (the `.types` directory), and an installed package of that
/// name. `node_modules` is looked for from the project root upward, the way
/// node's own resolution walks, and stops at a `.git` so a stray install in a
/// home directory cannot answer for a project.
///
/// This runs only when the symbol's module is neither a `.glyph` member nor a
/// stdlib module, so the common question never pays for the walk.
fn asserting_declaration(project_root: &Path, name: &str) -> Option<String> {
    let mut declaration_files = Vec::new();
    collect_dts_files(project_root, 0, &mut declaration_files);
    for file in &declaration_files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if declares_module(&text, name) {
            return Some(display_path(project_root, file));
        }
    }
    let mut dir = project_root.to_path_buf();
    loop {
        let installed = dir.join("node_modules").join(name);
        if installed.is_dir() {
            return Some(format!("the installed package `{name}`"));
        }
        if dir.join(".git").exists() {
            break;
        }
        let Some(parent) = dir.parent() else { break };
        dir = parent.to_path_buf();
    }
    None
}

/// Every `*.d.ts` under `dir`, skipping the directories that are not the
/// project's own source: an installed dependency answers through
/// `node_modules` rather than through its files, and build output is a copy of
/// something already read.
///
/// Dot directories are descended into, unlike the `.glyph` walk, because
/// `.types` is where a project keeps the declarations it wrote by hand.
fn collect_dts_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    /// Deep enough for a project's own `.types` tree, shallow enough that a
    /// symlink loop cannot spend a call.
    const MAX_DEPTH: usize = 8;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            collect_dts_files(&path, depth + 1, out);
        } else if name.ends_with(".d.ts") {
            out.push(path);
        }
    }
}

/// Whether `text` carries an ambient `declare module "name"` block.
fn declares_module(text: &str, name: &str) -> bool {
    const KEYWORD: &str = "declare module";
    let mut rest = text;
    while let Some(at) = rest.find(KEYWORD) {
        rest = &rest[at + KEYWORD.len()..];
        let body = rest.trim_start();
        let Some(quote) = body.chars().next() else {
            return false;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let after = &body[1..];
        let Some(close) = after.find(quote) else {
            return false;
        };
        if &after[..close] == name {
            return true;
        }
        rest = &after[close + 1..];
    }
    false
}

/// One edge, complete enough to be read on its own.
///
/// `relation` and the two ends are on the entry rather than only in the
/// envelope, because an entry lifted out of its reply has to still say what it
/// is about. The moment two relations appear in one answer, entries from each
/// are the same shape and position is all that tells them apart.
#[allow(clippy::too_many_arguments)]
fn edge_value(
    file: FileCtx<'_>,
    index: &LineIndex,
    span: &RelatedSpan,
    from: Option<String>,
    to: Option<&str>,
    provenance: &Provenance,
) -> Value {
    json!({
        "relation": span.relation.wire(),
        "from": from,
        "from_absent": from.is_none().then_some(MODULE_LEVEL),
        "to": to,
        "provenance": provenance.wire(),
        "path": display_path(file.root, file.path),
        "range": range_json(index, file.text, span.start, span.end),
    })
}

/// The answer one `glyph_references` call returns.
///
/// The envelope exists because coverage binds per relation. A flat list of
/// locations cannot say what it could not index, so a project with one
/// unreadable file used to return a list shaped exactly like a complete one.
/// Each relation states its own edges and its own gaps, and a relation the
/// caller did not ask for is absent rather than empty.
fn relations_answer(
    entity: Option<&str>,
    entity_absent: Option<&str>,
    provenance: &Provenance,
    wanted: &[Relation],
    mut edges: BTreeMap<Relation, Vec<Value>>,
    unindexed: &[Value],
) -> String {
    let mut relations = serde_json::Map::new();
    for relation in wanted {
        relations.insert(
            relation.wire().to_string(),
            json!({
                "edges": edges.remove(relation).unwrap_or_default(),
                "unindexed": unindexed,
            }),
        );
    }
    to_json(&json!({
        "entity": entity,
        "entity_absent": entity_absent,
        "provenance": provenance.wire(),
        "provenance_detail": provenance.detail(),
        "relations": Value::Object(relations),
    }))
}

/// Group classified spans into the per-relation lists an answer carries.
fn group_by_relation(edges: Vec<(Relation, Value)>) -> BTreeMap<Relation, Vec<Value>> {
    let mut out: BTreeMap<Relation, Vec<Value>> = BTreeMap::new();
    for (relation, value) in edges {
        out.entry(relation).or_default().push(value);
    }
    out
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
    let wanted = read_relations(args)?;
    // Module paths, and so the search for other files naming this symbol, are
    // scoped to the file's own project (D41): another project's `import lib`
    // names its own `lib`, not this one. That is also why each project gets its
    // own database rather than one database over the server's root.
    let project_root = crate::module_root_for(&path, &root);
    let this_module =
        module_path_of(&project_root, &path).ok_or("the file is not under the project root")?;
    let index = LineIndex::new(&text);

    // Deliberately not `analyze_full`: that also assigns types, and every query
    // below reads the resolution table only. This is the same front end the
    // database runs (`parse_module` → `module_symbols` → `resolve`), on one
    // file, and it costs a fraction of a millisecond.
    let Ok(module) = glyph_parser::parse(&text) else {
        return unresolvable(&address, &this_module, "the file does not parse", &wanted);
    };
    let Ok(symbols) = collect_module_symbols(&module) else {
        return unresolvable(&address, &this_module, "the file does not resolve", &wanted);
    };
    let (resolved, _errs) = resolve_module(&module, symbols, &build_prelude());

    // A record field is addressed through its record, so the address resolves
    // in two steps and the answer is a different relation: the field-use
    // relation rather than the occurrence scan. Only a dotted name reaches
    // here, and a dotted name is never a top-level declaration.
    if let Some(field) = address.name().map(FieldAddress::parse).transpose()?.flatten() {
        let record = named_target(&resolved, &field.record, &this_module).ok_or_else(|| {
            format!(
                "`{}` names no record in module `{this_module}`, so there is no \
                 `{}` to address. {}",
                field.record,
                field.field,
                not_declared(&field.record, &this_module, &resolved)
            )
        })?;
        return field_references(&field, record, server, &path, &project_root);
    }

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
            // A local has no `module::name` identity, so the answer names no
            // entity. That is a fact about locals rather than a lookup that
            // failed, and `entity_absent` is what keeps the two apart.
            let absent = "a local binding has no `module::name` identity; it is file-scoped and \
                          addressable only by position";
            let provenance = Provenance::Proved(
                "the binding is a `let`, parameter, or pattern binding in this file, which the \
                 compiler resolved against its own tree"
                    .to_string(),
            );
            let Some((line, character)) = address.position() else {
                return Ok(relations_answer(
                    None,
                    Some(absent),
                    &provenance,
                    &wanted,
                    BTreeMap::new(),
                    &[],
                ));
            };
            let offset = index.offset(&text, line, character);
            let file = FileCtx { path: &path, root: &root, text: &text };
            let edges: Vec<(Relation, Value)> = relations_at(&module, &resolved, offset, &text, true)
                .into_iter()
                .map(|span| {
                    let from = enclosing_decl_name(&module, span.start)
                        .map(|d| format!("{this_module}::{d}"));
                    (
                        span.relation,
                        edge_value(file, &index, &span, from, None, &provenance),
                    )
                })
                .collect();
            // One file, and it parsed, or this arm was never reached: there is
            // nothing the answer could have failed to index.
            return Ok(relations_answer(
                None,
                Some(absent),
                &provenance,
                &wanted,
                group_by_relation(edges),
                &[],
            ));
        }
        // The position is on no resolvable name at all. There is no subject, so
        // there are no edges, and `entity_absent` says which of the two an
        // empty answer is: not "this symbol is unused".
        None => {
            let (line, character) = address.position().unwrap_or((0, 0));
            return Ok(relations_answer(
                None,
                Some(&format!(
                    "line {line}, character {character} is on no resolvable name, so there is no \
                     symbol to report relations for"
                )),
                &Provenance::Undetermined(
                    "the address names no symbol, so there is no far end".to_string(),
                ),
                &wanted,
                BTreeMap::new(),
                &[],
            ));
        }
    };

    let project = server.project(&project_root, &path);
    let entity = format!("{sym_module}::{name}");
    let provenance = symbol_provenance(project, &root, &sym_module, &name);
    let db = &project.db;
    let mut edges: Vec<(Relation, Value)> = Vec::new();
    // What the sweep could not read, named rather than skipped. A file that
    // does not parse holds occurrences this answer cannot see, and dropping it
    // silently is what makes a partial list look like a complete one.
    let mut unindexed: Vec<Value> = Vec::new();
    for (fpath, entry) in project.searched() {
        let ftext = entry.file.text(db);
        let fparsed = glyph_db::parse_module(db, entry.file);
        let fresolved = glyph_db::resolve(db, entry.file);
        let (Some(fmodule), Some(fresolved)) = (fparsed.module(), fresolved.resolved()) else {
            unindexed.push(json!({
                "path": display_path(&root, fpath),
                "why": match fparsed.module() {
                    None => "the file does not parse, so no occurrence in it was read",
                    Some(_) => "the file does not resolve, so no occurrence in it was read",
                },
            }));
            continue;
        };
        let spans = global_relations_in(
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
        for span in spans {
            let from = enclosing_decl_name(fmodule, span.start)
                .map(|d| format!("{}::{d}", entry.module_path));
            edges.push((
                span.relation,
                edge_value(file, &index, &span, from, Some(&entity), &provenance),
            ));
        }
    }
    // Both relations are read off the same parse and the same resolution table,
    // so a file that defeated one defeated the other. Stating it under each is
    // not duplication for its own sake: coverage is a property of a relation,
    // and a relation that later reads something else needs somewhere to say so.
    Ok(relations_answer(
        Some(&entity),
        None,
        &provenance,
        &wanted,
        group_by_relation(edges),
        &unindexed,
    ))
}

/// A record field, addressed as the record and then the field.
///
/// `User.email`, resolved in the module namespace of the file the call names,
/// the same rule `glyph_variants` uses for a type. A field is not a top-level
/// declaration, so a bare `email` is not an address: two records in one module
/// can each declare a field of that name, and answering about whichever one
/// came first would merge two entities under one spelling.
struct FieldAddress {
    /// The record as the querying file names it: one it declares or one it
    /// imports.
    record: String,
    field: String,
}

impl FieldAddress {
    /// Read a `Type.field` name. `Ok(None)` for a plain name, which is a
    /// declaration and goes down the occurrence path.
    ///
    /// Exactly one dot, and anything else is a malformed address rather than a
    /// name to try resolving. No declaration, variant or import binding this
    /// tool ever accepted has a dot in it, so a dotted name is always an
    /// attempt at a field, and falling through with one would answer "module
    /// `m` declares no top-level name `types.User.email`", which is true and
    /// tells the caller nothing about the form that would have worked.
    ///
    /// A namespaced record (`import types`, then `types.User`) is the two-dot
    /// case and is refused. The namespace names a module rather than a symbol,
    /// so `named_target` has no identity to return for it, which is the same
    /// limit `glyph_variants` has; the message says where to ask instead.
    fn parse(name: &str) -> Result<Option<FieldAddress>, String> {
        let Some((record, field)) = name.split_once('.') else {
            return Ok(None);
        };
        if record.is_empty() || field.is_empty() || field.contains('.') {
            return Err(format!(
                "`{name}` is not an address. A record field is `Record.field` \
                 (`User.email`), with the record named the way the file you are \
                 asking from names it. A record reached through a namespace \
                 import (`import types`, then `types.User`) cannot be addressed \
                 this way, because the namespace names a module and not a \
                 symbol: ask from the module that declares the record, or from \
                 one that imports it by name."
            ));
        }
        Ok(Some(FieldAddress {
            record: record.to_string(),
            field: field.to_string(),
        }))
    }
}

/// The classes of site this relation does not hold, named rather than left to
/// be discovered.
///
/// Absence in an impact answer means absence of a relation, so a class the
/// compiler never computes has to be said out loud. Both of these are places
/// where Glyph's own checker draws no conclusion and `tsc` catches the mistake
/// on the emitted TypeScript, which is exactly why a caller cannot infer them
/// from the sites that are here.
const FIELD_NOT_INDEXED: &[&str] = &[
    "a record literal that constructs this record. An object literal gets no \
     expected type from the checker, so no key in one is ever joined to a field \
     of a declaration; renaming the field breaks the literal, and `tsc` is what \
     reports it.",
    "an object pattern that destructures this record (`Loaded({ email })`). The \
     pattern binder resolves fields against a structural payload record only, so \
     a named record reached through one has no field resolved.",
];

/// Every site naming one record field, across the file's own project.
///
/// The relation is the one the member-access check writes while it types each
/// file: a site is here because the checker resolved the object's field set and
/// found this field on it. Nothing here re-walks the members, and nothing here
/// infers a site from a spelling.
///
/// Three lists, and the split is the whole point. `sites` are proven: the
/// compiler joined each one to this declaration. `unkeyed` are sites that spell
/// the field on an object whose type never resolved to a field set, so the
/// compiler never decided which record they name and one of them may well be
/// over this one. `not_indexed` names the classes of site the relation does not
/// model at all.
fn field_references(
    address: &FieldAddress,
    target: SymbolTarget,
    server: &mut Server,
    path: &Path,
    project_root: &Path,
) -> Result<String, String> {
    let SymbolTarget::Global { module, name } = target else {
        return Err(format!(
            "`{}` is a local binding, and a local has no fields to address: a \
             record field belongs to a type declaration.",
            address.record
        ));
    };
    let root = server.root.clone();
    let project = server.project(project_root, path);
    // A file the walk skips is not a member, so the relation holds none of its
    // sites, and an answer keyed from its namespace would be missing every one
    // of them. The same rule `glyph_variants` applies.
    if !project.files.contains_key(path) {
        return Err(format!(
            "{} is not a member of the project at {}, so the field-use relation \
             holds none of its sites and an answer keyed from it would be missing \
             them. Ask about a file the project walk reaches.",
            display_path(&root, path),
            display_path(&root, project_root),
        ));
    }
    let entity = format!("{module}::{name}.{}", address.field);
    let owner = FieldOwner::Declared {
        module: module.clone(),
        name: name.clone(),
    };

    let db = &project.db;
    let mut sites: Vec<Value> = Vec::new();
    let mut unkeyed: Vec<Value> = Vec::new();
    // What the sweep could not read, named rather than skipped. The relation
    // holds nothing for a file that does not parse or does not resolve, so its
    // field sites are invisible here, and dropping it silently is what makes a
    // partial list look like a complete one. Coverage is stated per relation.
    let mut unindexed: Vec<Value> = Vec::new();
    let mut declared = false;
    for (fpath, entry) in project.searched() {
        let parsed = glyph_db::parse_module(db, entry.file);
        if parsed.module().is_none() {
            unindexed.push(json!({
                "path": display_path(&root, fpath),
                "why": "the file does not parse, so no field site in it was read",
            }));
            continue;
        }
        if glyph_db::resolve(db, entry.file).resolved().is_none() {
            unindexed.push(json!({
                "path": display_path(&root, fpath),
                "why": "the file does not resolve, so no field site in it was read",
            }));
            continue;
        }
        let uses = glyph_db::field_uses(db, entry.file);
        // Built on the first matching site rather than per file. Most files in
        // a project name no site of any one field, and a line index over every
        // one of them is work whose answer is thrown away.
        let mut rendered: Option<(LineIndex, Vec<OutlineSymbol>)> = None;
        let ftext = entry.file.text(db);
        let file = FileCtx { path: fpath, root: &root, text: ftext };
        for site in uses.sites() {
            if site.field() != address.field {
                continue;
            }
            let unresolved = match site.owner() {
                FieldOwner::Declared { .. } if site.owner() != &owner => continue,
                FieldOwner::Declared { .. } => None,
                // A field set with no declaration behind it is a different
                // record, decided: an inline annotation or a stdlib type is
                // never this declaration.
                FieldOwner::Undeclared { .. } => continue,
                FieldOwner::Unresolved { display } => Some(display.clone()),
            };
            if site.access() == FieldAccess::Declaration && unresolved.is_none() {
                declared = true;
            }
            let (index, outline) = rendered.get_or_insert_with(|| {
                (
                    LineIndex::new(ftext),
                    parsed.module().map(module_outline).unwrap_or_default(),
                )
            });
            let value = field_site_value(
                file,
                index,
                outline,
                &entry.module_path,
                site,
                unresolved.as_deref(),
            );
            match unresolved {
                None => sites.push(value),
                Some(_) => unkeyed.push(value),
            }
        }
    }

    // The declaration is a site of its own, so its absence says the record does
    // not declare this field. Answering `[]` there would read as "nothing uses
    // it", which is a different and much more expensive claim.
    if !declared {
        return Err(field_not_declared(server, path, project_root, &module, &name, address));
    }

    Ok(to_json(&json!({
        "entity": entity,
        "sites": sites,
        "unkeyed": unkeyed,
        "unindexed": unindexed,
        "not_indexed": FIELD_NOT_INDEXED,
    })))
}

/// One field site as the answer carries it: where it is, which declaration it
/// sits in, and what it does with the field.
///
/// The range is narrowed to the field's own name inside the site's span,
/// because this relation is what a rename writes its edits from and a span
/// covering `u.email` would rewrite the object along with the field. The
/// narrowing is a search for the spelling inside the span the checker recorded,
/// so it is a relocation hint rather than a key; a span it cannot narrow is
/// reported whole rather than dropped.
fn field_site_value(
    file: FileCtx<'_>,
    index: &LineIndex,
    outline: &[OutlineSymbol],
    module: &str,
    site: &FieldSite,
    unresolved: Option<&str>,
) -> Value {
    let span = site.span();
    let (start, end) = narrow_to_name(file.text, span.start, span.end, site.field());
    // The declaration a site sits in. For a read or a write that is whichever
    // top-level declaration contains it; for the field's own declaration and
    // for a `@redact` name it is the record, and the record is what the
    // relation already keyed the site to.
    //
    // The record has to be named rather than looked up, because a `@redact`
    // annotation sits *before* the `type` keyword the declaration's span starts
    // at, so the containment search finds nothing and the site came back with
    // no declaration at all.
    let declaration = match (site.access(), site.owner()) {
        (FieldAccess::Declaration | FieldAccess::Redact, FieldOwner::Declared { module, name }) => {
            Some(format!("{module}::{name}"))
        }
        _ => outline
            .iter()
            .find(|s| s.span.0 <= span.start && span.start < s.span.1)
            .map(|s| format!("{module}::{}", s.name)),
    };
    let mut out = serde_json::Map::new();
    out.insert("path".to_string(), json!(display_path(file.root, file.path)));
    out.insert(
        "range".to_string(),
        range_json(index, file.text, start, end),
    );
    out.insert(
        "declaration".to_string(),
        match declaration {
            Some(d) => json!(d),
            None => Value::Null,
        },
    );
    out.insert("access".to_string(), json!(site.access().as_str()));
    match unresolved {
        None => {
            out.insert("indexed".to_string(), json!(true));
        }
        Some(display) => {
            out.insert("indexed".to_string(), json!(false));
            out.insert(
                "not_indexed".to_string(),
                json!(format!(
                    "the object's type is `{display}`, which never resolved to a \
                     field set, so the compiler never joined this site to a record. \
                     It may be over this one."
                )),
            );
        }
    }
    Value::Object(out)
}

/// The last occurrence of `name` inside `[start, end)`, or the whole span when
/// it holds no such text.
fn narrow_to_name(text: &str, start: u32, end: u32, name: &str) -> (u32, u32) {
    let Some(slice) = text.get(start as usize..end as usize) else {
        return (start, end);
    };
    match slice.rfind(name) {
        Some(at) => {
            let s = start + at as u32;
            (s, s + name.len() as u32)
        }
        None => (start, end),
    }
}

/// The error for a field the record does not declare.
///
/// It lists the record's own fields, for the same reason `not_declared` lists a
/// module's names: a caller reaching this holds a field name that has been
/// renamed or moved, and the current list is the useful next step. A record the
/// answer cannot read the fields of says that instead of listing nothing.
fn field_not_declared(
    server: &mut Server,
    path: &Path,
    project_root: &Path,
    module: &str,
    name: &str,
    address: &FieldAddress,
) -> String {
    let project = server.project(project_root, path);
    let db = &project.db;
    let owner = FieldOwner::Declared {
        module: module.to_string(),
        name: name.to_string(),
    };
    let mut fields: Vec<String> = Vec::new();
    for (_, entry) in project.searched() {
        for site in glyph_db::field_uses(db, entry.file).sites() {
            if site.access() == FieldAccess::Declaration && site.owner() == &owner {
                fields.push(site.field().to_string());
            }
        }
    }
    match fields.is_empty() {
        true => format!(
            "`{module}::{name}` declares no fields this project can read, so it has \
             no field `{}` to address. A `type` whose body is not written as a \
             record inline (an alias, a union, a generic application) declares no \
             field of its own; a field reached through one belongs to the record \
             the declaration names.",
            address.field
        ),
        false => format!(
            "`{module}::{name}` has no field `{}`. It declares: {}.",
            address.field,
            fields.join(", ")
        ),
    }
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
    // The name of a variant that does not exist yet. With it, the call is a
    // question about a change rather than about what is there.
    let proposed = proposed_variant_arg(args)?;
    let project_root = crate::module_root_for(&path, &root);
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

    // What the union itself declares. A caller asking about a change needs it
    // twice over: to see what it is changing, and because a proposed name the
    // union already has is not the edit it looks like.
    let shape = union_shape(db, decls, &project.files, &end);
    if let UnionShape::NotAUnion(what) = &shape {
        return Err(format!(
            "`{name}` is {what}, not a tagged union. `{}` has no variants, so no \
             match site is ever filed over it and there is nothing here to add a \
             variant to. That is a different answer from an empty site list, which \
             says a union exists and nothing matches on it.",
            render_type_end(decls, &end),
        ));
    }
    if let Some(proposed) = &proposed {
        match &shape {
            UnionShape::Variants(existing) if existing.iter().any(|v| v == proposed) => {
                return Err(format!(
                    "`{proposed}` is already a variant of `{}`, whose variants are {}. \
                     Adding a name the union already has is not the change this answers \
                     about.",
                    render_type_end(decls, &end),
                    existing.join(", "),
                ));
            }
            UnionShape::Unread(why) => {
                return Err(format!(
                    "cannot answer for a `proposed_variant` of `{name}`: {why}. Without \
                     the current variant list there is no way to tell whether \
                     `{proposed}` already exists, and a consequence stated for an edit \
                     that turns out not to be one would be a guess. Ask again without \
                     `proposed_variant` for the sites and what each does today."
                ));
            }
            _ => {}
        }
    }

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
        .map(|site| {
            let mut value = render.value(site);
            if proposed.is_some() {
                value["consequence"] = json!(consequence(&site.site));
            }
            value
        })
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
        .filter_map(|site| {
            let mut value = render.nested_value(site, &end, &name)?;
            if proposed.is_some() {
                value["consequence"] = json!(nested_consequence(&site.site));
            }
            Some(value)
        })
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
            // The relation never joined this site to the type being asked
            // about, so there is no consequence to state for it. Naming it is
            // the point: leaving it out would say no such site exists.
            if proposed.is_some() {
                value["consequence"] = json!("NOT_INDEXED");
            }
            value
        })
        .collect();

    // What the sweep could not read, named rather than skipped. The coverage
    // relation holds nothing for a file that does not parse or does not
    // resolve, so whatever that file matches on is invisible here. Stating it
    // matters more once a total exists than it did beside a bare list: a
    // number that counted the readable files and said nothing about the rest
    // is a partial list with an authoritative figure in front of it.
    let mut unindexed: Vec<Value> = Vec::new();
    for (fpath, entry) in project.searched() {
        let why = if glyph_db::parse_module(db, entry.file).module().is_none() {
            "the file does not parse, so no match site in it was read"
        } else if glyph_db::resolve(db, entry.file).resolved().is_none() {
            "the file does not resolve, so no match site in it was read"
        } else {
            continue;
        };
        unindexed.push(json!({ "path": display_path(&root, fpath), "why": why }));
    }

    let summary = variant_summary(
        &sites,
        &nested,
        &unkeyed,
        &unindexed,
        proposed.as_deref(),
        &render_type_end(decls, &end),
    );

    let mut answer = json!({
        "type": type_block(decls, &end, &shape),
        "summary": summary,
        "sites": sites,
        "unindexed": unindexed,
    });
    if let Some(proposed) = &proposed {
        answer["proposed_variant"] = json!(proposed);
    }
    if !nested.is_empty() {
        answer["nested"] = json!(nested);
    }
    if !unkeyed.is_empty() {
        answer["unkeyed"] = json!(unkeyed);
    }
    Ok(to_json(&answer))
}

/// The optional `proposed_variant` argument: the name a caller is about to add
/// to the union, which turns the lookup into a question about a change.
///
/// A malformed one is a malformed call, for the same reason `name` is checked:
/// a consequence reported for `Pendign` reads exactly like one reported for
/// the variant that was meant.
fn proposed_variant_arg(args: &Value) -> Result<Option<String>, String> {
    match args.get("proposed_variant") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if is_variant_name(s) => Ok(Some(s.clone())),
        Some(Value::String(s)) => Err(format!(
            "`proposed_variant` is `{s}`, which cannot be written as a variant name. \
             A variant is an identifier, so this is a malformed request rather than a \
             variant that does not exist yet."
        )),
        Some(other) => Err(format!(
            "`proposed_variant` must be a string, got `{other}`"
        )),
    }
}

/// Whether a string could be written as a variant name in a union declaration.
fn is_variant_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// What the declaration behind a type end turns out to be.
enum UnionShape {
    /// A tagged union, and these are its variants in declaration order.
    Variants(Vec<String>),
    /// A declaration that is not a tagged union, so the match-coverage
    /// relation does not reach it and the question does not apply. The string
    /// names what it is instead.
    NotAUnion(&'static str),
    /// A type end whose variants this answer cannot read, and why.
    Unread(String),
}

/// The variants of the type one call is about, read from the declaration.
///
/// Read from the declaration rather than gathered from the relation. The
/// relation holds the variants sites name, and a variant no site has ever
/// named is exactly the one somebody is most likely to be adding beside. The
/// declaring file is already parsed by the time this runs, so the parse is a
/// memo hit.
fn union_shape(
    db: &CompilerDb,
    decls: &DeclIndex,
    files: &BTreeMap<PathBuf, ProjectFile>,
    end: &CoverageTypeRef,
) -> UnionShape {
    let (module, name) = match end {
        CoverageTypeRef::Decl(key) => match decls.module_path(key.module()) {
            Some(module) => (module, key.name()),
            // Only reachable for a key from another interner, which this
            // index cannot produce; see `render_key`.
            None => {
                return UnionShape::Unread(
                    "the declaration's module is not one this project keys".to_string(),
                )
            }
        },
        CoverageTypeRef::Builtin { name } => {
            return UnionShape::Unread(format!(
                "`{name}` has a fixed variant table in the compiler and no declaration \
                 in this project, so there is nothing here to read it from"
            ))
        }
        CoverageTypeRef::Unkeyed { module, name } => {
            return UnionShape::Unread(format!(
                "no file of this project declares `{name}` under module `{module}`"
            ))
        }
    };
    let Some(file) = files.values().find(|f| f.module_path == module) else {
        return UnionShape::Unread(format!(
            "module `{module}` is not a file this project holds"
        ));
    };
    let parsed = glyph_db::parse_module(db, file.file);
    let Some(parsed) = parsed.module() else {
        return UnionShape::Unread(format!("module `{module}` does not parse"));
    };
    let decl = parsed.items.iter().find_map(|d| match d {
        glyph_ast::Decl::Type(t) if t.name.as_ref() == name => Some(t),
        _ => None,
    });
    let Some(decl) = decl else {
        return UnionShape::Unread(format!("module `{module}` declares no type `{name}`"));
    };
    match &decl.body {
        glyph_ast::TypeExpr::Union { variants, .. } => {
            UnionShape::Variants(variants.iter().map(|v| v.name.to_string()).collect())
        }
        glyph_ast::TypeExpr::Record { .. } => UnionShape::NotAUnion("a record"),
        glyph_ast::TypeExpr::Fn { .. } => UnionShape::NotAUnion("a function type"),
        // Its members are values rather than named constructors, and the
        // exhaustiveness checkers count tags, so no match site is ever filed
        // over one of these either.
        glyph_ast::TypeExpr::StringLiteralUnion { .. } => {
            UnionShape::NotAUnion("a union of string literals")
        }
        // An alias, a generic application, `extern_ts`, or `typeof`. Whatever
        // variants the type has belong to the declaration this one names, and
        // chasing that here would be a second resolution running beside the
        // one the checker already did.
        _ => UnionShape::Unread(format!(
            "`{name}` is declared as another type rather than as a variant list, so its \
             variants belong to whatever that names"
        )),
    }
}

/// The type end as an answer names it, with the union's own variants.
///
/// `variants` is a list, or an explicit `null` beside the reason it could not
/// be read. Leaving the field out for a type whose declaration this answer
/// cannot reach would spell "has no variants" and "not read" the same way,
/// which is the confusion an empty site list used to carry.
fn type_block(decls: &DeclIndex, end: &CoverageTypeRef, shape: &UnionShape) -> Value {
    let mut out = type_end_value(decls, end);
    match shape {
        UnionShape::Variants(names) => out["variants"] = json!(names),
        UnionShape::Unread(why) => {
            out["variants"] = Value::Null;
            out["variants_unavailable"] = json!(why);
        }
        // Refused before this runs.
        UnionShape::NotAUnion(_) => {}
    }
    out
}

/// What one site does once the proposed variant exists, read off the site's
/// own edges rather than off its summary state.
///
/// The state summarises the site as it stands and the two questions come
/// apart: a site already short a variant reads `declined`, and adding another
/// one still leaves it failing to compile, which is a decided answer rather
/// than an undetermined one.
fn consequence(d: &CoverageSiteRef) -> &'static str {
    // Nothing was ever counted here, so nothing follows from adding to the
    // set it was not counted against.
    if d.state == CoverageState::ScrutineeUnresolved {
        return "UNDETERMINED";
    }
    // A catch-all settles the compile question whatever else the site does:
    // the new variant reaches an arm and no diagnostic is raised, which is
    // the case nothing will tell you about.
    if d.catch_alls.iter().any(|c| c.depth == 0) {
        return "ABSORBS";
    }
    // An arm the checker read nothing from may or may not take the new
    // variant, and which it is decides whether this site still compiles.
    if d.declines.iter().any(|x| x.depth == 0) {
        return "UNDETERMINED";
    }
    // Every arm names a variant, none absorbs, so the new one is named by no
    // arm: E0200 against this site.
    "WILL_FAIL"
}

/// The same question for a site that reaches the type through a payload.
///
/// Stricter, because a coverage edge carries the depth it was written at and
/// not the union it belongs to. A catch-all one level down may sit in the
/// scope the new variant lands in or in a sibling payload, and the relation
/// cannot tell those apart, so it is reported undetermined rather than
/// guessed either way. A catch-all at depth 0 needs no guess: it takes every
/// value the scrutinee can hold, payloads included.
fn nested_consequence(d: &CoverageSiteRef) -> &'static str {
    if d.state == CoverageState::ScrutineeUnresolved {
        return "UNDETERMINED";
    }
    if d.catch_alls.iter().any(|c| c.depth == 0) {
        return "ABSORBS";
    }
    if !d.catch_alls.is_empty() || !d.declines.is_empty() {
        return "UNDETERMINED";
    }
    "WILL_FAIL"
}

/// The buckets a change answer splits its sites into, each with the prose one
/// line of the summary reads as: `(bucket, one site, several sites)`.
///
/// The count and the sentence live in one table because they are one fact. A
/// renderer kept beside the bucket list is a renderer that eventually reports
/// a count under the wrong sentence.
const CONSEQUENCE_BUCKETS: &[(&str, &str, &str)] = &[
    (
        "WILL_FAIL",
        "will fail compilation",
        "will fail compilation",
    ),
    (
        "ABSORBS",
        "contains a catch-all and will silently absorb it",
        "contain a catch-all and will silently absorb it",
    ),
    (
        "UNDETERMINED",
        "the compiler cannot decide either way",
        "the compiler cannot decide either way",
    ),
];

/// The same, for the lookup form, where a site has a state rather than a
/// consequence because no change was proposed to have one about.
const STATE_BUCKETS: &[(&str, &str, &str)] = &[
    ("exhaustive", "names every variant", "name every variant"),
    (
        "has_catch_all",
        "contains a catch-all",
        "contain a catch-all",
    ),
    (
        "declined",
        "the checker declined: an arm it does not model, or a variant no arm names",
        "the checker declined: an arm it does not model, or a variant no arm names",
    ),
    (
        "scrutinee_unresolved",
        "has a scrutinee whose type never resolved",
        "have a scrutinee whose type never resolved",
    ),
];

/// The arithmetic a caller would otherwise do over the site list, and, beside
/// it, everything that arithmetic does not cover.
///
/// The counts are read back off the rendered sites rather than recomputed from
/// the relation. That is what makes the summary checkable: it is arithmetic
/// over exactly the objects this answer carries, so it cannot come to a
/// different figure than a reader counting the list by hand.
///
/// **Every total states what it could not count.** A number reads as
/// authoritative in a way a list does not, so a total that quietly dropped the
/// sites this project could not key, the sites that reach the type through a
/// payload, or a file the sweep never opened would be a partial list wearing a
/// figure that says it is complete. `not_counted` is therefore always present,
/// explicitly empty when nothing was left out: an absent field spells "nothing
/// was excluded" and "exclusions were never worked out" the same way, which is
/// the ambiguity an empty site list used to carry.
///
/// The exclusions are in the same object as the totals and in `lines` as well,
/// because a caller who prints the three lines and acts on them must not have
/// to go looking for the caveat.
fn variant_summary(
    sites: &[Value],
    nested: &[Value],
    unkeyed: &[Value],
    unindexed: &[Value],
    proposed: Option<&str>,
    type_name: &str,
) -> Value {
    // Which question the breakdown is over. With a proposed variant the sites
    // carry what an edit does to them, without one they carry what they are
    // today, and the two are different columns rather than two names for one.
    let (field, buckets, group) = match proposed {
        Some(_) => ("consequence", CONSEQUENCE_BUCKETS, "consequences"),
        None => ("state", STATE_BUCKETS, "states"),
    };

    // `(count, the rest of the line)`. The count leads every line so they read
    // as a column once padded.
    let mut rows: Vec<(usize, String)> = Vec::new();
    let files: BTreeSet<&str> = sites
        .iter()
        .filter_map(|s| s.get("path").and_then(Value::as_str))
        .collect();
    rows.push((
        sites.len(),
        format!(
            "match {} across {} {}",
            plural(sites.len(), "site", "sites"),
            files.len(),
            plural(files.len(), "file", "files"),
        ),
    ));

    let mut counts = serde_json::Map::new();
    let mut bucketed = 0usize;
    for (bucket, one, many) in buckets {
        let n = sites
            .iter()
            .filter(|s| s.get(field).and_then(Value::as_str) == Some(*bucket))
            .count();
        bucketed += n;
        // Every bucket is stated, zero included, because a bucket the answer
        // omits and a bucket that came back empty are not the same claim. Only
        // a bucket with sites in it gets a line, since "0 will fail
        // compilation" is noise in prose and a fact in the object.
        counts.insert(bucket.to_string(), json!(n));
        if n > 0 {
            rows.push((n, plural(n, one, many).to_string()));
        }
    }

    let mut not_counted: Vec<Value> = Vec::new();

    // A site the breakdown has no bucket for. Unreachable as the tables stand,
    // and recorded rather than assumed away: this is the one arithmetic error
    // the summary could make on its own, and it would make the buckets read as
    // a partition of the total when they were short of it.
    if bucketed < sites.len() {
        let n = sites.len() - bucketed;
        not_counted.push(json!({
            "what": "unbucketed",
            "sites": n,
            "why": format!(
                "these sites carry a `{field}` this summary has no bucket for, so the \
                 breakdown is short of the site total by that many."
            ),
        }));
        rows.push((
            n,
            format!(
                "counted {} fall in no bucket above",
                plural(n, "site", "sites")
            ),
        ));
    }

    // Counted in the site total, absent from the file total: the relation
    // files these under a module the project's file list no longer holds, so
    // there is no path to attribute them to.
    let unlocated = sites
        .iter()
        .filter(|s| s.get("path").map(Value::is_null).unwrap_or(true))
        .count();
    if unlocated > 0 {
        not_counted.push(json!({
            "what": "unlocated",
            "sites": unlocated,
            "why": "the relation files these sites under a module the project's file list \
                    no longer holds, so they are in the site total and the file total \
                    cannot include them.",
        }));
        rows.push((
            unlocated,
            format!(
                "counted {} {} in no file this project's list holds, so the file count leaves {} out",
                plural(unlocated, "site", "sites"),
                plural(unlocated, "sits", "sit"),
                plural(unlocated, "it", "them"),
            ),
        ));
    }

    if !nested.is_empty() {
        not_counted.push(json!({
            "what": "nested",
            "sites": nested.len(),
            "why": format!(
                "these sites match on another type and name a variant of `{type_name}` \
                 through a payload, so they are counted on their own under `nested` \
                 rather than folded into a total over this type's own scrutinees. They \
                 break the same way when a variant is added, so read both."
            ),
        }));
        rows.push((
            nested.len(),
            format!(
                "further {} {} `{type_name}` through a payload, counted separately under `nested`",
                plural(nested.len(), "site", "sites"),
                plural(nested.len(), "reaches", "reach"),
            ),
        ));
    }

    if !unkeyed.is_empty() {
        not_counted.push(json!({
            "what": "unkeyed",
            "sites": unkeyed.len(),
            "why": format!(
                "this project never joined these sites to `{type_name}`: each matches on a \
                 same-named type whose module names nothing this project holds. One of them \
                 may well be over the type that was asked about, so folding them into the \
                 total would claim an edge that was never made and dropping them from it \
                 without a word would make the total read as complete."
            ),
        }));
        rows.push((
            unkeyed.len(),
            format!(
                "further {} this project could not key to `{type_name}`, in no count above, listed under `unkeyed`",
                plural(unkeyed.len(), "site", "sites"),
            ),
        ));
    }

    if !unindexed.is_empty() {
        not_counted.push(json!({
            "what": "unindexed",
            // Not zero. How many sites an unread file holds is not a number
            // this answer has, and writing one would be the manufactured
            // figure the whole rule is about.
            "sites": Value::Null,
            "files": unindexed.len(),
            "why": "the coverage sweep read no site in these files, so how many they hold \
                    is unknown and no count above includes them. They are named one by one \
                    under `unindexed`.",
        }));
        rows.push((
            unindexed.len(),
            format!(
                "project {} {} never read, so any match site in {} is in no count above",
                plural(unindexed.len(), "file", "files"),
                plural(unindexed.len(), "was", "were"),
                plural(unindexed.len(), "it", "them"),
            ),
        ));
    }

    let width = rows
        .iter()
        .map(|(n, _)| n.to_string().len())
        .max()
        .unwrap_or(1);
    let lines: Vec<String> = rows
        .iter()
        .map(|(n, text)| format!("{n:>width$} {text}"))
        .collect();

    let mut out = serde_json::Map::new();
    out.insert("sites".to_string(), json!(sites.len()));
    out.insert("files".to_string(), json!(files.len()));
    out.insert(group.to_string(), Value::Object(counts));
    out.insert("not_counted".to_string(), json!(not_counted));
    out.insert("lines".to_string(), json!(lines));
    Value::Object(out)
}

/// One of two words, by count.
///
/// Written as a pair rather than as a suffix because half the pairs here are
/// not plural-by-`s`: `was`/`were`, `sits`/`sit`, `it`/`them`.
fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 {
        one
    } else {
        many
    }
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
/// A `..` segment was worse than cosmetic: the project-root climb resolved
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

/// The module half of a declaration's `module::name` identity: the file's path
/// under its own project root (D41), extension dropped, `/`-separated.
///
/// Counted from the project and never from the server's root. The server root
/// is where the tool was started, so rooting an identity on it gives one
/// declaration a different name per session, and gave `glyph_diagnostics` a
/// different name from `glyph_variants` and `glyph check --json` for the same
/// declaration (G180). `module_root_for` is the shared rule, fallback and all.
fn module_key(file: &Path, workspace: &Path) -> String {
    let root = crate::module_root_for(file, workspace);
    module_path_of(&root, file).unwrap_or_else(|| {
        // Unreachable: `module_root_for` returns a marked root only when the
        // file lies under it, and the file's own parent otherwise.
        file.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
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

    /// Every edge of an answer, whatever relation it stands in, back in
    /// document order.
    ///
    /// The answer is split by relation, and a test about *where* a symbol is
    /// used is not about which relation each site stands in. Flattening here
    /// keeps those tests asking their own question, and sorting restores the
    /// order the flat list had before relations existed.
    fn flat_edges(value: &Value) -> Vec<Value> {
        let relations = value["relations"]
            .as_object()
            .unwrap_or_else(|| panic!("no `relations` in {value}"));
        let mut out: Vec<Value> = relations
            .values()
            .flat_map(|r| r["edges"].as_array().cloned().unwrap_or_default())
            .collect();
        out.sort_by_key(|e| {
            (
                e["path"].as_str().unwrap_or_default().to_string(),
                e["range"]["start"]["line"].as_u64().unwrap_or_default(),
                e["range"]["start"]["character"].as_u64().unwrap_or_default(),
            )
        });
        out
    }

    /// The reference locations for a position, as `(path, start line)` pairs.
    fn refs_at(server: &mut Server, path: &str, line: u32, character: u32) -> Vec<(String, u64)> {
        let (value, is_error) = call_on(
            server,
            "glyph_references",
            json!({ "path": path, "line": line, "character": character }),
        );
        assert!(!is_error, "{value}");
        flat_edges(&value)
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
        flat_edges(&value)
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
        let locs = flat_edges(&value);
        // Declaration in a, import binding + one use in b = 3 across two files.
        assert_eq!(locs.len(), 3, "{value}");
        let paths: Vec<&str> = locs.iter().map(|l| l["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"a.glyph") && paths.contains(&"b.glyph"), "{paths:?}");
    }

    /// The relations of one answer, as `(relation, from)` pairs in the order
    /// the answer lists them. `from` is the declaration the site sits in, and
    /// a site at module level (an `import` binding) has none.
    fn relation_pairs(value: &Value, relation: &str) -> Vec<(String, String)> {
        value["relations"][relation]["edges"]
            .as_array()
            .unwrap_or_else(|| panic!("no `{relation}` edges in {value}"))
            .iter()
            .map(|e| {
                (
                    e["relation"].as_str().unwrap_or("<missing>").to_string(),
                    match e["from"].as_str() {
                        Some(from) => from.to_string(),
                        None => "<module level>".to_string(),
                    },
                )
            })
            .collect()
    }

    /// `charge` is applied to an argument list once, and named three other
    /// times: its own declaration, the import binding, and the argument in
    /// `apply(charge)`. Only the first breaks when the signature changes, and
    /// one flat list of four says nothing about which.
    #[test]
    fn calls_are_a_separate_relation_from_references() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub fn charge(m: number) -> number {\n  return m\n}\n",
        );
        write(
            &root,
            "b.glyph",
            "module b\n\
             import a { charge }\n\
             fn apply(f: fn(number) -> number) -> number {\n  return f(1)\n}\n\
             pub fn bill() -> number {\n  return charge(1)\n}\n\
             pub fn handler() -> number {\n  return apply(charge)\n}\n",
        );
        let mut server = Server::new(root.clone());
        let (value, is_error) = call_on(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "name": "charge" }),
        );
        assert!(!is_error, "{value}");

        assert_eq!(
            relation_pairs(&value, "CALLS"),
            vec![("CALLS".to_string(), "b::bill".to_string())],
            "{value}"
        );
        assert_eq!(
            relation_pairs(&value, "REFERENCES"),
            vec![
                ("REFERENCES".to_string(), "a::charge".to_string()),
                ("REFERENCES".to_string(), "<module level>".to_string()),
                ("REFERENCES".to_string(), "b::handler".to_string()),
            ],
            "{value}"
        );
    }

    /// A file the sweep could not read is named, not skipped.
    ///
    /// The answer used to `continue` past a file that does not parse, so a
    /// project with one broken module returned a list shaped exactly like a
    /// complete one. Coverage binds per relation, so each relation says what it
    /// could not index and names the file rather than counting it.
    #[test]
    fn a_file_the_sweep_cannot_read_is_named_under_every_relation() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        write(&root, "broken.glyph", "module c\npub fn (\n");
        let mut server = Server::new(root.clone());
        let (value, is_error) = call_on(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "name": "charge" }),
        );
        assert!(!is_error, "{value}");

        for relation in ["CALLS", "REFERENCES"] {
            let unindexed = value["relations"][relation]["unindexed"]
                .as_array()
                .unwrap_or_else(|| panic!("no `unindexed` on {relation}: {value}"));
            let named: Vec<&str> = unindexed
                .iter()
                .map(|u| u["path"].as_str().unwrap_or("<no path>"))
                .collect();
            assert_eq!(named, ["broken.glyph"], "{relation}: {value}");
            assert!(
                unindexed[0]["why"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("does not parse"),
                "{relation}: {value}"
            );
        }
        // The rest of the project still answered, so the gap is a gap and not a
        // failure: the call site in `b` is there, under CALLS.
        assert_eq!(
            relation_pairs(&value, "CALLS"),
            vec![("CALLS".to_string(), "b::bill".to_string())],
            "{value}"
        );
    }

    /// An edge the compiler proved and an edge a declaration file claimed are
    /// different facts, and the answer says which.
    ///
    /// `charge` is declared in a `.glyph` module this project holds, so the
    /// resolver checked the far end against a declaration it parsed. `log`
    /// comes from a module no Glyph file declares and a `.d.ts` does, so `tsc`
    /// checks it and Glyph's resolver never read it. `thing` comes from a
    /// module nothing declares at all, and that is neither: saying `ASSERTED`
    /// there would claim a declaration file that does not exist.
    #[test]
    fn provenance_separates_what_the_compiler_proved_from_what_a_dts_asserts() {
        let root = tmp_root();
        std::fs::write(root.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
        std::fs::create_dir_all(root.join(".types")).unwrap();
        std::fs::write(
            root.join(".types").join("ext.d.ts"),
            "declare module \"tinylog\" { export function log(m: string): void; }\n",
        )
        .unwrap();
        write(&root, "a.glyph", CHARGE);
        write(
            &root,
            "b.glyph",
            "module b\n\
             import a { charge }\n\
             import tinylog { log }\n\
             import nowhere { thing }\n\
             import std/io { println }\n\
             pub fn bill() -> number {\n\
             \u{20} log(\"x\")\n\
             \u{20} println(\"y\")\n\
             \u{20} thing()\n\
             \u{20} return charge(1)\n\
             }\n",
        );
        let mut server = Server::new(root.clone());
        let ask = |server: &mut Server, name: &str| {
            let (value, is_error) = call_on(
                server,
                "glyph_references",
                json!({ "path": "b.glyph", "name": name }),
            );
            assert!(!is_error, "asked about `{name}`: {value}");
            value
        };

        let proved = ask(&mut server, "charge");
        assert_eq!(proved["provenance"], "PROVED", "{proved}");
        let stdlib = ask(&mut server, "println");
        assert_eq!(stdlib["provenance"], "PROVED", "{stdlib}");

        let asserted = ask(&mut server, "log");
        assert_eq!(asserted["provenance"], "ASSERTED", "{asserted}");
        assert!(
            asserted["provenance_detail"]
                .as_str()
                .unwrap_or_default()
                .contains(".types/ext.d.ts"),
            "the answer must name the declaration that asserts it: {asserted}"
        );

        let unknown = ask(&mut server, "thing");
        assert_eq!(unknown["provenance"], "UNDETERMINED", "{unknown}");

        // Every edge carries the value too, so an entry read on its own still
        // says whether it is a proof or a claim.
        for value in [&proved, &asserted, &unknown] {
            let want = value["provenance"].as_str().unwrap();
            for relation in ["CALLS", "REFERENCES"] {
                for edge in value["relations"][relation]["edges"].as_array().unwrap() {
                    assert_eq!(edge["provenance"], want, "{value}");
                }
            }
        }
    }

    /// The relation argument selects from a closed vocabulary. A relation the
    /// caller did not ask for is absent from the answer rather than empty, and
    /// a name outside the vocabulary is an error: answering `[]` to a
    /// misspelled relation would read as "no such edges exist".
    #[test]
    fn a_relation_outside_the_vocabulary_is_refused() {
        let root = tmp_root();
        write(&root, "a.glyph", CHARGE);
        write(&root, "b.glyph", CHARGE_IMPORTER);
        let mut server = Server::new(root.clone());

        let (only_calls, is_error) = call_on(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "name": "charge", "relation": "CALLS" }),
        );
        assert!(!is_error, "{only_calls}");
        assert!(only_calls["relations"]["CALLS"].is_object(), "{only_calls}");
        assert!(
            only_calls["relations"]["REFERENCES"].is_null(),
            "a relation nobody asked for must be absent, not empty: {only_calls}"
        );

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "a.glyph", "name": "charge", "relation": "MENTIONS" }),
        );
        assert!(is_error, "an unknown relation answered: {message}");
        assert!(
            message.contains("MENTIONS") && message.contains("CALLS"),
            "the error must name the vocabulary: {message}"
        );
    }

    /// One symbol, one project, two spellings of the consumer's import. The
    /// answer names the same use site either way.
    ///
    /// A namespace-qualified read is a reference to the same symbol as a
    /// named-import read. When the qualified one was dropped the answer was
    /// not "no references", which would be wrong but visible. It was the
    /// declaration alone, shaped exactly like a complete answer for a symbol
    /// nobody calls, with the calling module absent from it entirely (G186).
    ///
    /// Three things are asserted, and dropping any of them lets the defect
    /// back in a different shape. Both spellings report the use. The covered
    /// text is the name and not the whole `render.label`, because this
    /// relation is what workspace rename writes its edits from. And the set is
    /// the same asked from the use as asked from the declaration.
    #[test]
    fn a_namespace_qualified_call_is_the_same_reference_as_a_named_one() {
        const RENDER: &str = "module render\npub fn label(s: string) -> string {\n  return s\n}\n";
        const NAMED: &str = "module policy\nimport render { label }\npub fn run(s: string) -> string {\n  return label(s)\n}\n";
        const QUALIFIED: &str = "module policy\nimport render\npub fn run(s: string) -> string {\n  return render.label(s)\n}\n";

        let build = |consumer: &str| {
            let root = tmp_root();
            write(&root, "render.glyph", RENDER);
            write(&root, "policy.glyph", consumer);
            root
        };
        // Every reported site as `(file, line, the source text it covers)`.
        let ask = |root: &Path, path: &str, line: u32, character: u32| -> Vec<(String, u64, String)> {
            let mut server = Server::new(root.to_path_buf());
            let args = json!({ "path": path, "line": line, "character": character });
            let sites = refs_at(&mut server, path, line, character);
            let names = ref_names(&mut server, root, args);
            assert_eq!(sites.len(), names.len());
            sites.into_iter().zip(names).map(|((p, l), n)| (p, l, n)).collect()
        };

        // `label` in its declaration: render.glyph line 1, character 7.
        let named = ask(&build(NAMED), "render.glyph", 1, 7);
        let qualified_root = build(QUALIFIED);
        let qualified = ask(&qualified_root, "render.glyph", 1, 7);

        // The use in the consumer, which is the half that must not depend on
        // the spelling: same file, same line, and the covered text is the name
        // rather than the qualified path.
        let use_site = ("policy.glyph".to_string(), 3, "label".to_string());
        assert!(named.contains(&use_site), "named spelling lost the call: {named:?}");
        assert!(
            qualified.contains(&use_site),
            "the namespace spelling dropped the call: {qualified:?}"
        );

        // Every file that names the symbol, either way.
        let files = |v: &[(String, u64, String)]| {
            let mut f: Vec<String> = v.iter().map(|(p, _, _)| p.clone()).collect();
            f.sort();
            f.dedup();
            f
        };
        assert_eq!(files(&named), files(&qualified), "{named:?} vs {qualified:?}");

        // The one difference the two answers are allowed. `import render { label }`
        // has a `label` token to point at; `import render` has none, and the
        // `render` token is a reference to the module, not to `label`.
        assert_eq!(named.len(), 3, "{named:?}");
        assert_eq!(qualified.len(), 2, "{qualified:?}");

        // The same set asked from the qualified use itself: `label` in
        // `render.label` at policy.glyph line 3, character 16. A cursor there
        // addressed nothing, so the tool answered `[]` for a symbol with at
        // least the reference under the cursor.
        let from_the_use = ask(&qualified_root, "policy.glyph", 3, 16);
        assert_eq!(from_the_use, qualified, "{from_the_use:?} vs {qualified:?}");
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
        // Marked, so the skipped file's project is still this root: an
        // unmarked file's module half is counted from its own directory
        // (G180), which would put `.hidden/c.glyph` in a project of its own
        // and leave it nothing to be outside of.
        std::fs::write(root.join("package.json"), r#"{"name":"p","glyph":{}}"#).unwrap();
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

    // ---- addressing a record field ----

    /// A project whose `types` module declares two records that each have a
    /// `name` field, plus a union carrying one of them as a payload. Every
    /// field case this addressing has to survive is reachable from it.
    fn field_project() -> PathBuf {
        let root = tmp_root();
        write(
            &root,
            "types.glyph",
            "module types\n\
             pub type User = {\n  name: string,\n  email: string,\n}\n\
             pub type Company = {\n  name: string,\n}\n\
             pub type Load = Loaded(User) | Empty\n",
        );
        write(
            &root,
            "app.glyph",
            "module app\n\
             import types { User, Company, Load, Loaded, Empty }\n\
             pub fn greet(u: User) -> string {\n  return u.email\n}\n\
             pub fn label(u: User) -> string {\n  return u.name\n}\n\
             pub fn org(c: Company) -> string {\n  return c.name\n}\n\
             pub fn from_payload(l: Load) -> string {\n  return match l {\n\
             \u{20}   Loaded(u) => u.email,\n    Empty => \"none\",\n  }\n}\n",
        );
        root
    }

    /// Ask `glyph_references` about a field and return the parsed answer. The
    /// raw text is read first so a refusal shows its own words rather than the
    /// `null` a failed parse leaves behind.
    fn field_answer(server: &mut Server, path: &str, name: &str) -> Value {
        let (text, is_error) = call_raw(
            server,
            "glyph_references",
            json!({ "path": path, "name": name }),
        );
        assert!(!is_error, "asked about `{name}`: {text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
    }

    /// `email` on its own is not an address: the module declares no such
    /// top-level name, and two records in the same module could each declare a
    /// field of that name. The address is the record and the field.
    #[test]
    fn a_field_is_addressed_through_its_record() {
        let root = field_project();
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        assert_eq!(value["entity"], json!("types::User.email"), "{value}");
        let sites: Vec<String> = value["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .map(|s| format!("{} {}", s["declaration"], s["access"]))
            .collect();
        assert!(
            sites.iter().any(|s| s.contains("app::greet") && s.contains("read")),
            "the direct member access is missing: {sites:?}"
        );
        assert!(
            sites.iter().any(|s| s.contains("app::from_payload")),
            "the access through a payload binding is missing: {sites:?}"
        );
    }

    /// Two records in one module, each with a `name` field. They are two
    /// entities, and an answer about one must not carry the other's site.
    #[test]
    fn same_named_fields_on_two_records_are_two_entities() {
        let root = field_project();
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "Company.name");
        assert_eq!(value["entity"], json!("types::Company.name"), "{value}");
        let decls: Vec<String> = value["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .map(|s| s["declaration"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(
            decls.iter().any(|d| d == "app::org"),
            "the Company.name read is missing: {decls:?}"
        );
        // `app::label` reads `User.name`, the same spelling on the other
        // record, and `types::User` declares it. Both are sites of a `name`
        // field and neither is a site of this one, so a filter on the spelling
        // alone would carry them here.
        assert!(
            !decls.iter().any(|d| d == "app::label"),
            "the same-named field on the other record was merged in: {decls:?}"
        );
        assert!(
            !decls.iter().any(|d| d == "types::User"),
            "the other record's declaration of `name` was merged in: {decls:?}"
        );

        // And the other way round, so the answer is not simply narrow.
        let other = field_answer(&mut server, "types.glyph", "User.name");
        let decls: Vec<String> = other["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .map(|s| s["declaration"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(
            decls.iter().any(|d| d == "app::label") && !decls.iter().any(|d| d == "app::org"),
            "`User.name` did not resolve to its own record's site: {decls:?}"
        );
    }

    /// The site kinds, as `(declaration, access)` pairs.
    fn field_kinds(value: &Value, key: &str) -> Vec<(String, String)> {
        value[key]
            .as_array()
            .unwrap_or_else(|| panic!("no `{key}` in {value}"))
            .iter()
            .map(|s| {
                (
                    s["declaration"].as_str().unwrap_or_default().to_string(),
                    s["access"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    /// The declaration is a site, and so is a write. A rename has to edit both,
    /// so an impact set that held only the reads would be missing the two edits
    /// the caller is most certain to need.
    #[test]
    fn a_field_answer_holds_the_declaration_the_reads_and_the_writes() {
        let root = tmp_root();
        write(
            &root,
            "types.glyph",
            "module types\n\
             pub type User = {\n  name: string,\n  email: string,\n}\n\
             pub type Account = {\n  owner: User,\n}\n",
        );
        write(
            &root,
            "app.glyph",
            "module app\n\
             import types { User, Account }\n\
             pub fn rename(u: User, from: User) -> string {\n\
             \u{20} mut u.email = from.email\n  return u.email\n}\n\
             pub fn adopt(a: Account, from: User) -> string {\n\
             \u{20} mut a.owner.email = from.email\n  return a.owner.email\n}\n",
        );
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let kinds = field_kinds(&value, "sites");
        assert!(
            kinds.contains(&("types::User".to_string(), "declaration".to_string())),
            "the declaration is not a site: {kinds:?}"
        );
        assert!(
            kinds.contains(&("app::rename".to_string(), "write".to_string())),
            "the assignment target is not reported as a write: {kinds:?}"
        );
        assert!(
            kinds.contains(&("app::rename".to_string(), "read".to_string())),
            "the read after the write is missing: {kinds:?}"
        );
        // `mut u.email = from.email` reads the same field on the right of the
        // same statement, and `mut a.owner.email = from.email` writes it
        // through a chain. Two writes and four reads across the two functions,
        // and the reads are the ones a coarser answer gets wrong.
        assert_eq!(
            kinds.iter().filter(|(_, a)| a == "write").count(),
            2,
            "the value side of an assignment was called a write: {kinds:?}"
        );
        assert_eq!(
            kinds.iter().filter(|(_, a)| a == "read").count(),
            4,
            "the reads are not all here: {kinds:?}"
        );

        // The other half of the chained lvalue. `a.owner` inside
        // `mut a.owner.email = ...` is read to reach the field being written,
        // and it is not itself written: what travels into the member arm is the
        // target's own span, so only the outermost member of the chain matches
        // it. A flag set for the statement would call this a write too.
        let owner = field_answer(&mut server, "types.glyph", "Account.owner");
        let kinds = field_kinds(&owner, "sites");
        assert!(
            !kinds.contains(&("app::adopt".to_string(), "write".to_string())),
            "an inner member of a chained lvalue was called a write: {kinds:?}"
        );
        assert_eq!(
            kinds.iter().filter(|(_, a)| a == "read").count(),
            2,
            "the two reads of `a.owner` are not both here: {kinds:?}"
        );
    }

    /// A project file the sweep cannot read is named in the field answer.
    ///
    /// The relation holds nothing for a file that does not parse, so its field
    /// sites are invisible here. Leaving it out would make a list of the files
    /// that did parse read as the whole project, which is the partial list
    /// shaped like a complete one.
    #[test]
    fn a_file_the_sweep_cannot_read_is_named_in_a_field_answer() {
        let root = tmp_root();
        write(
            &root,
            "types.glyph",
            "module types\npub type User = {\n  name: string,\n  email: string,\n}\n",
        );
        write(
            &root,
            "app.glyph",
            "module app\n\
             import types { User }\n\
             pub fn greet(u: User) -> string {\n  return u.email\n}\n",
        );
        write(&root, "broken.glyph", "module broken\npub fn oops( {\n");
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let unindexed = value["unindexed"]
            .as_array()
            .unwrap_or_else(|| panic!("no `unindexed` in {value}"));
        let named: Vec<&str> = unindexed
            .iter()
            .filter_map(|u| u["path"].as_str())
            .collect();
        assert_eq!(named, vec!["broken.glyph"], "{value}");
        assert!(
            unindexed[0]["why"].as_str().unwrap_or_default().contains("field site"),
            "the file is named with no reason for it: {}",
            unindexed[0]
        );
        // The readable file's site is still there, so the coverage statement is
        // an addition to the answer rather than a replacement for it.
        let kinds = field_kinds(&value, "sites");
        assert!(
            kinds.contains(&("app::greet".to_string(), "read".to_string())),
            "the readable file's site was lost: {kinds:?}"
        );
    }

    /// A record reached only through a namespace import cannot have its fields
    /// addressed, and the refusal says which spelling would work.
    ///
    /// Falling through to the declaration path answered "module `ns` declares
    /// no top-level name `types.User.email`", which is true and says nothing
    /// about the form. The limit itself is the one `named_target` has for a
    /// namespace: it names a module, not a symbol, so there is no identity to
    /// key a field to.
    #[test]
    fn a_namespaced_record_is_refused_with_the_spelling_that_works() {
        let root = tmp_root();
        write(
            &root,
            "types.glyph",
            "module types\npub type User = {\n  name: string,\n  email: string,\n}\n",
        );
        write(
            &root,
            "ns.glyph",
            "module ns\n\
             import types\n\
             pub fn greet(u: types.User) -> string {\n  return u.email\n}\n",
        );
        let mut server = Server::new(root.clone());
        let (text, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "ns.glyph", "name": "types.User.email" }),
        );
        assert!(is_error, "answered a namespaced field address: {text}");
        assert!(text.contains("`Record.field`"), "{text}");
        assert!(text.contains("namespace"), "{text}");

        // The read in that file is still a site of the field; the answer is
        // asked for from a module that can name the record.
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let kinds = field_kinds(&value, "sites");
        assert!(
            kinds.contains(&("ns::greet".to_string(), "read".to_string())),
            "a read through a namespace-imported record is not a site: {kinds:?}"
        );
    }

    /// A `@redact fields: [...]` name is a site, keyed to the record it decorates.
    ///
    /// The annotation is validated against the same record body (E0219), so it
    /// is exact, and renaming the field means editing it or the redaction
    /// silently stops masking anything. Its span sits before the `type` keyword
    /// the declaration's span starts at, so the site's declaration comes from
    /// the record the relation keyed it to rather than from a containment
    /// search, which finds nothing there.
    #[test]
    fn a_redact_annotation_naming_the_field_is_a_site_on_the_record() {
        let root = tmp_root();
        write(
            &root,
            "types.glyph",
            "module types\n\
             @redact fields: [email]\n\
             pub type User = {\n  name: string,\n  email: string,\n}\n",
        );
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let kinds = field_kinds(&value, "sites");
        assert!(
            kinds.contains(&("types::User".to_string(), "redact".to_string())),
            "the redaction is not a site on the record: {kinds:?}"
        );
        // And the field's own declaration names the record too, not `null`.
        assert!(
            kinds.contains(&("types::User".to_string(), "declaration".to_string())),
            "the declaration site does not name its record: {kinds:?}"
        );
    }

    /// A range covers the field's name and nothing else, because this is what a
    /// rename writes its edits from: a span over `u.email` would rewrite the
    /// binding along with the field.
    #[test]
    fn a_field_site_covers_the_name_and_not_the_access() {
        let root = field_project();
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let root = std::fs::canonicalize(&root).unwrap();
        let covered: Vec<String> = value["sites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|loc| {
                let text = std::fs::read_to_string(root.join(loc["path"].as_str().unwrap())).unwrap();
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
            .collect();
        let wide: Vec<&String> = covered.iter().filter(|t| *t != "email").collect();
        assert!(wide.is_empty(), "a span wider than the name: {wide:?}");
    }

    /// A site the compiler could not join to any record is named, and it is not
    /// in `sites`.
    ///
    /// `extern_ts` is opaque to Glyph's checker, so `handle.email` resolves to
    /// no field set at all. The site may be over `User.email` and may not, and
    /// the answer has to say that rather than pick: promoting it would claim a
    /// proven edge, and dropping it would say the site does not exist.
    #[test]
    fn a_site_over_a_type_that_never_resolved_is_named_and_not_proven() {
        let root = tmp_root();
        write(
            &root,
            "types.glyph",
            "module types\npub type User = {\n  name: string,\n  email: string,\n}\n",
        );
        write(
            &root,
            "app.glyph",
            "module app\n\
             pub fn opaque() -> string {\n\
             \u{20} let handle = extern_ts(\"globalThis.user\")\n\
             \u{20} return handle.email\n}\n",
        );
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let proven = field_kinds(&value, "sites");
        assert!(
            !proven.iter().any(|(d, _)| d == "app::opaque"),
            "claimed a proven edge for a site it could not key: {proven:?}"
        );
        let unkeyed = field_kinds(&value, "unkeyed");
        assert!(
            unkeyed.iter().any(|(d, _)| d == "app::opaque"),
            "dropped the site instead of naming it: {unkeyed:?}"
        );
        let entry = &value["unkeyed"][0];
        assert_eq!(entry["indexed"], json!(false), "{entry}");
        assert!(
            entry["not_indexed"].as_str().unwrap_or_default().contains("never resolved"),
            "an unkeyed site with no reason for it: {entry}"
        );
    }

    /// The classes of site this relation does not hold are named in the answer.
    ///
    /// A record literal that constructs the record breaks when the field is
    /// renamed, and the checker gives an object literal no expected type, so no
    /// site is ever joined to a field of one. Leaving that unsaid would make a
    /// list of member accesses read as the whole impact set.
    #[test]
    fn a_field_answer_says_which_site_classes_it_does_not_hold() {
        let root = field_project();
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "types.glyph", "User.email");
        let classes: Vec<String> = value["not_indexed"]
            .as_array()
            .unwrap_or_else(|| panic!("no `not_indexed` in {value}"))
            .iter()
            .map(|c| c.as_str().unwrap_or_default().to_string())
            .collect();
        assert!(
            classes.iter().any(|c| c.contains("record literal")),
            "the construction class is unstated: {classes:?}"
        );
        assert!(
            classes.iter().any(|c| c.contains("object pattern")),
            "the destructure class is unstated: {classes:?}"
        );
    }

    /// A field the record does not declare is refused, with the record's own
    /// fields listed. An empty site list would read as a field nothing uses.
    #[test]
    fn a_field_the_record_does_not_declare_is_refused() {
        let root = field_project();
        let mut server = Server::new(root.clone());
        let (text, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "types.glyph", "name": "User.emial" }),
        );
        assert!(is_error, "answered about a field the record has not got: {text}");
        assert!(text.contains("has no field `emial`"), "{text}");
        assert!(text.contains("name, email"), "{text}");
    }

    /// A record reached through a cross-module alias keys to the record the
    /// chain ends at.
    ///
    /// `view::Row` is `store::Sheet`, which declares the field. Keying a read
    /// through `Row` under `view::Row.header` would put it in the impact set of
    /// a rename that cannot reach it, and leave it out of the one that can.
    #[test]
    fn a_field_reached_through_an_alias_keys_to_the_record_that_declares_it() {
        let root = tmp_root();
        write(
            &root,
            "store.glyph",
            "module store\npub type Sheet = {\n  header: string,\n}\n",
        );
        write(
            &root,
            "view.glyph",
            "module view\nimport store { Sheet }\npub type Row = Sheet\n",
        );
        write(
            &root,
            "app.glyph",
            "module app\n\
             import view { Row }\n\
             pub fn head(r: Row) -> string {\n  return r.header\n}\n",
        );
        let mut server = Server::new(root.clone());
        let value = field_answer(&mut server, "store.glyph", "Sheet.header");
        let kinds = field_kinds(&value, "sites");
        assert_eq!(value["entity"], json!("store::Sheet.header"), "{value}");
        assert!(
            kinds.contains(&("app::head".to_string(), "read".to_string())),
            "the read through the alias is not keyed to the declaring record: {kinds:?}"
        );

        // The alias itself declares no field, so it has none to address.
        let (text, is_error) = call_raw(
            &mut server,
            "glyph_references",
            json!({ "path": "view.glyph", "name": "Row.header" }),
        );
        assert!(is_error, "answered about a field an alias does not declare: {text}");
        assert!(text.contains("declares no fields"), "{text}");
    }

    /// The record can be addressed the way the querying file names it, so a
    /// consumer that imported it asks about the same entity as the module that
    /// declares it.
    #[test]
    fn an_imported_record_addresses_the_same_field() {
        let root = field_project();
        let mut server = Server::new(root.clone());
        let from_decl = field_answer(&mut server, "types.glyph", "User.email");
        let from_use = field_answer(&mut server, "app.glyph", "User.email");
        assert_eq!(from_decl["entity"], from_use["entity"], "{from_use}");
        assert_eq!(from_decl["sites"], from_use["sites"], "{from_use}");
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

    // ---- glyph_variants as a change, not a lookup ----
    //
    // The lookup form answers what is there. An agent adding `Pending` to
    // `PaymentResult` is asking what its edit does, and three things the
    // compiler already knows only reach it once the proposed name is in the
    // request: whether that name already exists, which sites the new variant
    // reaches, and what each of them then does.

    /// A union with three variants where one site is short of the third, so
    /// the checker declines the site over a gap rather than over an arm.
    const COMMAND_SHORT: &str = "module a\npub type Command =\n  | Up\n  | Down\n  | Left\npub fn run(c: Command) -> number {\n  return match c {\n    Up => 1,\n    Down => 0,\n  }\n}\n";
    /// The same union with an `is` guard that names no variant of it. The
    /// checker reads nothing from that arm, so the site's mentions are not a
    /// complete accounting of what it handles.
    const COMMAND_IS_GUARD: &str = "module a\npub type Command =\n  | Up\n  | Down\npub fn run(c: Command) -> number {\n  return match c {\n    is string => 0,\n    Up => 1,\n    Down => 2,\n  }\n}\n";

    /// Call `glyph_variants` with a proposed variant. A tool error is prose,
    /// so an unexpected refusal prints its text rather than the word `null`.
    fn proposing(server: &mut Server, path: &str, name: &str, proposed: &str) -> Value {
        let (text, is_error) = call_raw(
            server,
            "glyph_variants",
            json!({ "path": path, "name": name, "proposed_variant": proposed }),
        );
        assert!(!is_error, "{text}");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    /// The same call where the refusal is the answer under test.
    fn proposing_refused(server: &mut Server, path: &str, name: &str, proposed: &str) -> String {
        let (text, is_error) = call_raw(
            server,
            "glyph_variants",
            json!({ "path": path, "name": name, "proposed_variant": proposed }),
        );
        assert!(is_error, "the call answered instead of refusing: {text}");
        text
    }

    /// G187, G188: the answer states what happens to each site, rather than a
    /// state the caller has to map to a consequence out of knowledge it does
    /// not have.
    #[test]
    fn a_proposed_variant_answers_a_consequence_per_site() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "b.glyph", COMMAND_B);
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Command", "Left");
        assert_eq!(answer["proposed_variant"], "Left", "{answer}");

        let sites = answer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 2, "{answer}");
        assert_eq!(sites[0]["declaration"], "a::run", "{answer}");
        assert_eq!(sites[0]["consequence"], "WILL_FAIL", "{answer}");
        assert_eq!(sites[1]["declaration"], "b::label", "{answer}");
        assert_eq!(sites[1]["consequence"], "ABSORBS", "{answer}");

        // The state stays beside it. The consequence is the added half, not a
        // rename of what was already there.
        assert_eq!(sites[0]["state"], "exhaustive", "{answer}");
        assert_eq!(sites[1]["state"], "has_catch_all", "{answer}");
    }

    /// G188: the union's own variants, so a caller can render what it is
    /// changing without asking a second time.
    #[test]
    fn the_answer_lists_the_unions_own_variants() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Command");
        assert_eq!(answer["type"]["variants"], json!(["Up", "Down"]), "{answer}");
        assert!(answer["type"]["variants_unavailable"].is_null(), "{answer}");
    }

    /// G187: the name reaches the tool now, so the collision is checkable.
    /// A name the union already has is not a change, and the lookup form
    /// could never have caught it.
    #[test]
    fn a_proposed_variant_that_already_exists_is_refused() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        let mut server = Server::new(root.clone());

        let message = proposing_refused(&mut server, "a.glyph", "Command", "Up");
        assert!(message.contains("Up"), "{message}");
        assert!(
            message.contains("Down"),
            "the existing variants must be named: {message}"
        );
        assert!(message.contains("a::Command"), "{message}");
    }

    /// The consequence comes off the site's own edges, not off its summary
    /// state. A site already short a variant is `declined`, and adding
    /// another one still leaves it failing to compile, which is a decided
    /// answer rather than an undetermined one.
    #[test]
    fn a_site_declined_over_a_gap_still_fails() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_SHORT);
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Command", "Right");
        let sites = answer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 1, "{answer}");
        assert_eq!(sites[0]["state"], "declined", "{answer}");
        assert_eq!(sites[0]["missing"], json!(["Left"]), "{answer}");
        assert_eq!(sites[0]["consequence"], "WILL_FAIL", "{answer}");
    }

    /// A site with an arm the checker read nothing from has no consequence
    /// the compiler can state: the unread arm may or may not take the new
    /// variant. Reported as undetermined rather than folded into one of the
    /// two decided answers.
    #[test]
    fn a_site_with_an_unread_arm_is_undetermined() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_IS_GUARD);
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Command", "Left");
        let sites = answer["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 1, "{answer}");
        assert_eq!(sites[0]["state"], "declined", "{answer}");
        assert_eq!(sites[0]["consequence"], "UNDETERMINED", "{answer}");
    }

    /// A site the project cannot key is NOT_INDEXED. The compiler never
    /// joined it to this type, so it has no consequence to state, and
    /// leaving it out would say no such site exists.
    #[test]
    fn an_unkeyable_site_is_not_indexed() {
        let root = tmp_root();
        write(
            &root,
            "models.glyph",
            &COMMAND_A.replace("module a", "module app/models"),
        );
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "models.glyph", "Command", "Left");
        assert_eq!(answer["sites"], json!([]), "{answer}");
        let unkeyed = answer["unkeyed"].as_array().unwrap();
        assert_eq!(unkeyed.len(), 1, "{answer}");
        assert_eq!(unkeyed[0]["consequence"], "NOT_INDEXED", "{answer}");
    }

    /// A payload site breaks the same way a direct one does, so it carries
    /// the same consequence.
    #[test]
    fn a_nested_site_carries_a_consequence_too() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub type Inner =\n  | X\n  | Y\npub type Outer =\n  | A(Inner)\n  | B\npub fn f(o: Outer) -> number {\n  return match o {\n    A(X) => 1,\n    A(Y) => 3,\n    B => 2,\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Inner", "Z");
        assert_eq!(answer["type"]["variants"], json!(["X", "Y"]), "{answer}");
        let nested = answer["nested"].as_array().unwrap();
        assert_eq!(nested.len(), 1, "{answer}");
        assert_eq!(nested[0]["declaration"], "a::f", "{answer}");
        assert_eq!(nested[0]["consequence"], "WILL_FAIL", "{answer}");
    }

    // ---- the summary: the arithmetic, and what it could not count ----

    /// G198: the answer takes a position on its own totals.
    ///
    /// It used to hand back a list and stop, so the three lines a reader
    /// wants were arithmetic the caller did outside the compiler. Two callers
    /// could tally one reply differently and neither was wrong.
    ///
    /// The figures are checked against the list in the same answer, because
    /// that is the property: a summary that could disagree with the sites
    /// beside it is a second opinion rather than a total.
    #[test]
    fn the_answer_carries_the_arithmetic_over_its_own_site_list() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "b.glyph", COMMAND_B);
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Command", "Left");
        let summary = &answer["summary"];
        assert_eq!(summary["sites"], 2, "{answer}");
        assert_eq!(summary["files"], 2, "{answer}");
        assert_eq!(
            summary["consequences"],
            json!({ "WILL_FAIL": 1, "ABSORBS": 1, "UNDETERMINED": 0 }),
            "{answer}"
        );
        // Nothing was left out here, and the answer says that rather than
        // leaving the field off: an absent list spells "nothing was excluded"
        // and "exclusions were never worked out" the same way.
        assert_eq!(summary["not_counted"], json!([]), "{answer}");
        assert_eq!(
            summary["lines"],
            json!([
                "2 match sites across 2 files",
                "1 will fail compilation",
                "1 contains a catch-all and will silently absorb it",
            ]),
            "{answer}"
        );

        // Without a proposed variant there is no consequence to count, so the
        // breakdown is over what the sites are today.
        let lookup = variants(&mut server, "a.glyph", "Command");
        assert!(lookup["summary"]["consequences"].is_null(), "{lookup}");
        assert_eq!(
            lookup["summary"]["states"],
            json!({
                "exhaustive": 1,
                "has_catch_all": 1,
                "declined": 0,
                "scrutinee_unresolved": 0,
            }),
            "{lookup}"
        );
        assert_eq!(
            lookup["summary"]["lines"],
            json!([
                "2 match sites across 2 files",
                "1 names every variant",
                "1 contains a catch-all",
            ]),
            "{lookup}"
        );
    }

    /// The counts line up as a column, which is only visible once one of them
    /// is wider than the others.
    #[test]
    fn the_lines_pad_their_counts_to_one_width() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        // Ten more exhaustive sites, so the total is two digits and the
        // catch-all count is one.
        for i in 0..10 {
            write(
                &root,
                &format!("c{i}.glyph"),
                &format!("module c{i}\nimport a {{ Command, Up, Down }}\npub fn f(c: Command) -> number {{\n  return match c {{\n    Up => 1,\n    Down => 0,\n  }}\n}}\n"),
            );
        }
        write(&root, "b.glyph", COMMAND_B);
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Command", "Left");
        assert_eq!(
            answer["summary"]["lines"],
            json!([
                "12 match sites across 12 files",
                "11 will fail compilation",
                " 1 contains a catch-all and will silently absorb it",
            ]),
            "{answer}"
        );
    }

    /// A total states what it could not count, in the same object.
    ///
    /// `models.glyph` declares its module as `app/models` while sitting at the
    /// root, so nothing it declares can be keyed and its match site is filed
    /// under `unkeyed`. The site total is 0 and the answer has to say a site
    /// exists outside it: a number reads as authoritative in a way a list
    /// does not, so "0 match sites" alone is the partial-list-as-complete
    /// failure in its most persuasive form.
    #[test]
    fn a_total_names_the_sites_this_project_could_not_key() {
        let root = tmp_root();
        write(
            &root,
            "models.glyph",
            "module app/models\npub type Command =\n  | Up\n  | Down\npub fn label(c: Command) -> string {\n  return match c {\n    Up => \"u\",\n    Down => \"d\",\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "models.glyph", "Command", "Left");
        let summary = &answer["summary"];
        assert_eq!(summary["sites"], 0, "{answer}");
        assert_eq!(answer["unkeyed"].as_array().unwrap().len(), 1, "{answer}");

        let excluded = summary["not_counted"].as_array().unwrap();
        assert_eq!(excluded.len(), 1, "{answer}");
        assert_eq!(excluded[0]["what"], "unkeyed", "{answer}");
        assert_eq!(excluded[0]["sites"], 1, "{answer}");

        // And in the rendered lines too. A caller who prints those and acts on
        // them must not have to go looking for the caveat.
        let lines = summary["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 2, "{answer}");
        assert!(
            lines[1].as_str().unwrap().contains("could not key"),
            "the exclusion is in the object and not in the lines: {answer}"
        );
    }

    /// A file the sweep could not read holds match sites this answer never
    /// saw, so the count of files it did read is not a count of the project.
    /// The site count for such a file is `null` rather than 0, which is the
    /// distinction the whole rule turns on: unknown is not zero.
    #[test]
    fn a_total_names_the_files_it_never_read() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "broken.glyph", "module broken\npub fn (\n");
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Command", "Left");
        assert_eq!(
            answer["unindexed"],
            json!([{
                "path": "broken.glyph",
                "why": "the file does not parse, so no match site in it was read",
            }]),
            "{answer}"
        );

        let excluded = answer["summary"]["not_counted"].as_array().unwrap();
        assert_eq!(excluded.len(), 1, "{answer}");
        assert_eq!(excluded[0]["what"], "unindexed", "{answer}");
        assert_eq!(excluded[0]["files"], 1, "{answer}");
        assert!(excluded[0]["sites"].is_null(), "unknown is not zero: {answer}");
        assert!(
            answer["summary"]["lines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l.as_str().unwrap().contains("never read")),
            "{answer}"
        );
    }

    /// A payload site breaks when a variant is added and is not a site over
    /// this type's own scrutinees, so it is counted on its own rather than
    /// folded into the total. Both halves are said: the count exists, and the
    /// total says it is not in it.
    #[test]
    fn a_total_counts_payload_sites_separately_and_says_so() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub type Inner =\n  | X\n  | Y\npub type Outer =\n  | A(Inner)\n  | B\npub fn f(o: Outer) -> number {\n  return match o {\n    A(X) => 1,\n    A(Y) => 3,\n    B => 2,\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Inner", "Z");
        let summary = &answer["summary"];
        assert_eq!(summary["sites"], 0, "{answer}");
        assert_eq!(answer["nested"].as_array().unwrap().len(), 1, "{answer}");

        let excluded = summary["not_counted"].as_array().unwrap();
        assert_eq!(excluded.len(), 1, "{answer}");
        assert_eq!(excluded[0]["what"], "nested", "{answer}");
        assert_eq!(excluded[0]["sites"], 1, "{answer}");
        assert!(
            summary["lines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l.as_str().unwrap().contains("through a payload")),
            "{answer}"
        );
    }

    /// Without a proposed variant nothing changes: no consequence is stated,
    /// because with no proposed change there is no consequence to state.
    #[test]
    fn the_lookup_form_states_no_consequence() {
        let root = tmp_root();
        write(&root, "a.glyph", COMMAND_A);
        write(&root, "b.glyph", COMMAND_B);
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Command");
        assert!(answer["proposed_variant"].is_null(), "{answer}");
        for site in answer["sites"].as_array().unwrap() {
            assert!(site["consequence"].is_null(), "{answer}");
        }
    }

    /// A union with no declaration in this project has no variant list to
    /// read here, and the answer says so instead of leaving the field out.
    /// Asked as a change it refuses, because without the variants there is no
    /// way to tell whether the proposed name already exists.
    #[test]
    fn a_union_whose_variants_cannot_be_read_says_so_and_refuses_a_proposal() {
        let root = tmp_root();
        write(&root, "a.glyph", RESULT_MATCH);
        let mut server = Server::new(root.clone());

        let answer = variants(&mut server, "a.glyph", "Result");
        assert!(answer["type"]["variants"].is_null(), "{answer}");
        assert!(
            answer["type"]["variants_unavailable"].is_string(),
            "an unreadable variant list must say why: {answer}"
        );

        let message = proposing_refused(&mut server, "a.glyph", "Result", "Pending");
        assert!(message.contains("Result"), "{message}");
        assert!(
            message.contains("proposed_variant"),
            "the refusal must point back at the lookup form: {message}"
        );
    }

    /// G190: a record has no variants, so the question does not apply to it.
    /// `{"sites": []}` is the answer for a union nothing matches on, and
    /// spelling both the same way makes an empty list unreadable.
    #[test]
    fn a_record_is_refused_rather_than_answered_with_no_sites() {
        let root = tmp_root();
        write(
            &root,
            "m.glyph",
            "module m\npub type User = {\n  name: string,\n  email: string,\n}\npub fn greet(u: User) -> string {\n  return u.name\n}\n",
        );
        write(
            &root,
            "u.glyph",
            "module u\npub type Command =\n  | Up\n  | Down\npub fn keep(c: Command) -> Command {\n  return c\n}\n",
        );
        let mut server = Server::new(root.clone());

        let (message, is_error) = call_raw(
            &mut server,
            "glyph_variants",
            json!({ "path": "m.glyph", "name": "User" }),
        );
        assert!(is_error, "a record answered with a site list: {message}");
        assert!(
            message.contains("User") && message.contains("record"),
            "{message}"
        );

        // The other side of the distinction: a union nothing matches on is a
        // real answer and still answers.
        let answer = variants(&mut server, "u.glyph", "Command");
        assert_eq!(answer["sites"], json!([]), "{answer}");
        assert_eq!(answer["type"]["variants"], json!(["Up", "Down"]), "{answer}");
    }


    /// A payload site whose scope has a catch-all one level down is not the
    /// same question as a direct site with no catch-all at all.
    ///
    /// `A(rest)` absorbs whatever `A(X)` did not name, so adding `Z` to
    /// `Inner` may leave this site compiling. The relation records that
    /// catch-all with a depth and not with the union it belongs to, so it
    /// could equally sit in a sibling payload, and the answer says it does not
    /// know rather than picking. The site's own state is `exhaustive`, which
    /// is exactly what a rule reading the summary state would report
    /// `WILL_FAIL` from.
    #[test]
    fn a_payload_catch_all_leaves_a_nested_site_undetermined() {
        let root = tmp_root();
        write(
            &root,
            "a.glyph",
            "module a\npub type Inner =\n  | X\n  | Y\npub type Outer =\n  | A(Inner)\n  | B\npub fn f(o: Outer) -> number {\n  return match o {\n    A(X) => 1,\n    A(rest) => 3,\n    B => 2,\n  }\n}\n",
        );
        let mut server = Server::new(root.clone());

        let answer = proposing(&mut server, "a.glyph", "Inner", "Z");
        let nested = answer["nested"].as_array().unwrap();
        assert_eq!(nested.len(), 1, "{answer}");
        assert_eq!(nested[0]["state"], "exhaustive", "{answer}");
        assert_eq!(nested[0]["consequence"], "UNDETERMINED", "{answer}");
    }

    /// The description is the only thing a caller reads before it acts on a
    /// consequence, so every word the answer can carry has to be in it, and
    /// the optional argument has to be discoverable from the schema.
    #[test]
    fn the_variants_tool_says_what_a_proposed_variant_answers() {
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

        assert!(
            spec["inputSchema"]["properties"]["proposed_variant"].is_object(),
            "{spec}"
        );
        // Optional: the lookup form is still a whole question.
        assert_eq!(
            spec["inputSchema"]["required"],
            json!(["path", "name"]),
            "{spec}"
        );
        let described = spec["description"].as_str().unwrap();
        for word in ["WILL_FAIL", "ABSORBS", "UNDETERMINED", "NOT_INDEXED"] {
            assert!(described.contains(word), "`{word}` is undescribed: {described}");
        }
        assert!(described.contains("proposed_variant"), "{described}");
        // A total nobody knows the limits of is worse than no total, so the
        // description that sells the summary has to carry them too.
        for word in ["summary", "not_counted", "unindexed"] {
            assert!(described.contains(word), "`{word}` is undescribed: {described}");
        }
    }

    /// `COMMAND_A` with one declaration that does not type-check, so a single
    /// file drives both surfaces at once: `glyph_variants` keys the match site
    /// in `run`, `glyph_diagnostics` keys the error in `handle`, and the two
    /// have to spell the module half the same way.
    const COMMAND_AND_BAD: &str = "module a\npub type Command =\n  | Up\n  | Down\npub fn run(c: Command) -> number {\n  return match c {\n    Up => 1,\n    Down => 0,\n  }\n}\npub fn handle(n: number) -> string {\n  return n\n}\n";

    /// G180: one declaration, one identity. The module half of a
    /// `module::name` identity is counted from the file's *project* root, in
    /// the layout `glyph init` writes: a `package.json` carrying
    /// `"glyph": {"src": "src"}` with the sources under `src/`.
    ///
    /// `glyph_diagnostics` used to count it from the server's own root, which
    /// is where the tool was invoked rather than anything about the
    /// declaration, so the same function was `src/a::handle` here and
    /// `a::handle` from `glyph check --json` and `glyph_variants`.
    #[test]
    fn a_declarations_module_half_is_counted_from_its_project_root() {
        let root = tmp_root();
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"p","glyph":{"src":"src"}}"#,
        )
        .unwrap();
        write(&src, "a.glyph", COMMAND_AND_BAD);

        let mut server = Server::new(root.clone());
        let (diags, is_error) = call_on(
            &mut server,
            "glyph_diagnostics",
            json!({ "path": "src/a.glyph" }),
        );
        assert!(!is_error, "{diags}");
        assert_eq!(diags[0]["entity"], "a::handle", "{diags}");

        let answer = variants(&mut server, "src/a.glyph", "Command");
        assert_eq!(answer["sites"][0]["declaration"], "a::run", "{answer}");
        // The file path stays relative to the server root; only the module
        // half of the identity is counted from the project.
        assert_eq!(answer["sites"][0]["path"], "src/a.glyph", "{answer}");
    }

    /// G180, the other half: with no project marker anywhere above it, a
    /// file's module half is counted from its own parent directory, which is
    /// the root `glyph check <file>` uses and the only one that does not move
    /// when the tool is invoked from somewhere else.
    #[test]
    fn an_unmarked_files_module_half_is_counted_from_its_own_directory() {
        let root = tmp_root();
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        write(&sub, "a.glyph", COMMAND_AND_BAD);

        let mut server = Server::new(root.clone());
        let (diags, is_error) = call_on(
            &mut server,
            "glyph_diagnostics",
            json!({ "path": "sub/a.glyph" }),
        );
        assert!(!is_error, "{diags}");
        assert_eq!(diags[0]["entity"], "a::handle", "{diags}");

        let answer = variants(&mut server, "sub/a.glyph", "Command");
        assert_eq!(answer["sites"][0]["declaration"], "a::run", "{answer}");
        assert_eq!(answer["sites"][0]["path"], "sub/a.glyph", "{answer}");
    }

    /// G184: absence has one spelling. A diagnostic with no enclosing
    /// declaration carries `"entity": null` rather than dropping the key, so a
    /// consumer reads the same field whichever surface produced it.
    #[test]
    fn a_diagnostic_with_no_enclosing_declaration_has_an_explicit_null_entity() {
        let root = tmp_root();
        write(&root, "broken.glyph", "module a\npub fn (\n");
        let (diags, is_error) = call(&root, "glyph_diagnostics", json!({ "path": "broken.glyph" }));
        assert!(!is_error, "{diags}");
        let entity = diags[0]
            .get("entity")
            .unwrap_or_else(|| panic!("the `entity` key must be present: {diags}"));
        assert!(entity.is_null(), "{diags}");
    }

}
