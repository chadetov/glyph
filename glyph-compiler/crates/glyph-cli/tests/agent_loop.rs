//! The two surfaces an agent actually uses, exercised the way an agent uses
//! them.
//!
//! The first is transport. Every other test of the MCP server calls its
//! dispatcher in process, which covers request and response shapes and stops
//! at the dispatcher. A client never does that. It starts `glyph mcp`, writes
//! one line to its stdin and reads one line back, so newline framing, the
//! handshake as an ordered exchange, what comes back from a line that is not
//! JSON, and whether the process starts at all are unexercised by an in-process
//! call. The `instructions` string is the sharpest case: it exists so a client
//! puts Glyph's guidance in a model's context at connect time, it only ever
//! mattered on the wire, and until this file it appeared in no assertion.
//!
//! The second is the repair loop:
//!
//! ```text
//! agent edits -> glyph check -> diagnostic -> query -> exact sites -> fix -> check -> green
//! ```
//!
//! What the loop test asserts is narrower than "the loop works" and harder to
//! fake: at each hop, that what the next call needed was a **field** in the
//! previous answer. A test that closes the loop while the harness supplies the
//! union's name from its own fixture proves that the fixture agrees with
//! itself. So the fixture lives in a private module that hands the test paths
//! and counts and nothing else, and every name it uses (the union, the module
//! it is declared in, the variant being added, the files to edit, the lines to
//! edit them at) is read out of a parsed answer. The names also carry a
//! per-run suffix, which makes a hard-coded one fail rather than pass quietly.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

/// How long one answer is waited for. Generous because a debug-built binary
/// walking a project is slow, and because the alternative to a budget is a
/// hang that becomes the suite's hang instead of a failure.
const ANSWER_BUDGET: Duration = Duration::from_secs(120);

/// How long "nothing came back" is waited for before it counts as silence.
/// Short on purpose: it is only ever used after a later request has already
/// been answered, and stdio is ordered, so an answer to the earlier line would
/// have arrived first.
const SILENCE_BUDGET: Duration = Duration::from_millis(500);

/// Build a uniquely-named temp directory. Mirrors the helper in
/// `integration.rs`; the two are separate test binaries and cannot share it.
fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("glyph_agent_loop_{prefix}_{}_{n}", std::process::id());
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    // The server canonicalizes its root and reports site paths relative to the
    // canonical one, so the test joins them back onto the same spelling.
    std::fs::canonicalize(&dir).expect("canonicalize temp dir")
}

// ---------------------------------------------------------------------------
// `glyph mcp` as a process, spoken to over a pipe.
// ---------------------------------------------------------------------------

/// A running `glyph mcp`, with a pipe on each side.
struct McpProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    stderr: Arc<Mutex<String>>,
}

impl McpProcess {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_glyph"))
            .arg("mcp")
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn `glyph mcp`");
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");

        // Both pipes are drained on their own threads. A child that fills a
        // pipe buffer blocks on the write, which looks exactly like a server
        // that stopped answering.
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let collected = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&collected);
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            if let Ok(mut slot) = sink.lock() {
                *slot = text;
            }
        });

        McpProcess {
            child,
            stdin: Some(stdin),
            lines,
            stderr: collected,
        }
    }

    /// Whatever the server has written to stderr so far, for a panic message.
    fn stderr(&self) -> String {
        match self.stderr.lock() {
            Ok(text) => text.clone(),
            Err(_) => String::from("<stderr unavailable>"),
        }
    }

    /// One message, framed as the transport frames it: one line.
    fn send(&mut self, message: &Value) {
        let line = serde_json::to_string(message).expect("serialize a message");
        assert!(
            !line.contains('\n'),
            "a message has to fit on one line: {line}"
        );
        let pipe = self.stdin.as_mut().expect("stdin is still open");
        writeln!(pipe, "{line}").expect("write to `glyph mcp`");
        pipe.flush().expect("flush to `glyph mcp`");
    }

    /// A raw line, for the cases where the point is that it is not a message.
    fn send_raw(&mut self, line: &str) {
        let pipe = self.stdin.as_mut().expect("stdin is still open");
        writeln!(pipe, "{line}").expect("write to `glyph mcp`");
        pipe.flush().expect("flush to `glyph mcp`");
    }

    /// The next line the server writes, parsed.
    fn recv(&mut self) -> Value {
        match self.lines.recv_timeout(ANSWER_BUDGET) {
            Ok(line) => serde_json::from_str(&line).unwrap_or_else(|e| {
                panic!("`glyph mcp` wrote a line that is not JSON ({e}): {line}")
            }),
            Err(RecvTimeoutError::Timeout) => panic!(
                "`glyph mcp` wrote nothing within {ANSWER_BUDGET:?}. stderr: {}",
                self.stderr()
            ),
            Err(RecvTimeoutError::Disconnected) => panic!(
                "`glyph mcp` closed stdout without answering. stderr: {}",
                self.stderr()
            ),
        }
    }

    /// Assert the server writes nothing more.
    fn expect_silence(&mut self, about: &str) {
        match self.lines.recv_timeout(SILENCE_BUDGET) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(line) => panic!("{about}: expected no further line, got {line}"),
            Err(RecvTimeoutError::Disconnected) => {
                panic!("{about}: `glyph mcp` closed stdout. stderr: {}", self.stderr())
            }
        }
    }

    /// Send a request, read its answer. The id is checked against the request's
    /// own, which is what makes the exchange ordered rather than a bag of
    /// messages that happened to come back.
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let answer = self.recv();
        assert_eq!(answer["jsonrpc"], json!("2.0"), "{answer}");
        assert_eq!(
            answer["id"],
            json!(id),
            "the answer to `{method}` has to carry the id it was asked under: {answer}"
        );
        answer
    }

    /// A notification: no id, and by the protocol no answer.
    fn notify(&mut self, method: &str) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method }));
    }

    /// The exchange a client performs before it calls anything.
    fn handshake(&mut self, id: u64) -> Value {
        let answer = self.request(
            id,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "glyph-cli-test", "version": "0" },
            }),
        );
        self.notify("notifications/initialized");
        answer
    }

    /// `tools/call`, with the tool's own JSON parsed back out of the content
    /// block a client receives it in.
    fn call_tool(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        let answer = self.request(
            id,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let text = answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("`{name}` returned no text content: {answer}"))
            .to_string();
        assert_eq!(
            answer["result"]["isError"],
            json!(false),
            "`{name}` failed: {text}"
        );
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("`{name}` answered with non-JSON text ({e}): {text}"))
    }

    /// Close stdin and wait. `run_stdio` returns when stdin closes, so a
    /// process that exits here is one whose read loop was still running.
    fn finish(mut self) -> i32 {
        drop(self.stdin.take());
        let status = self.child.wait().expect("wait on `glyph mcp`");
        status.code().unwrap_or(-1)
    }
}

// ---------------------------------------------------------------------------
// G197: the process surface.
// ---------------------------------------------------------------------------

#[test]
fn the_initialize_handshake_is_an_ordered_exchange_over_stdio() {
    let root = unique_tmp("handshake");
    let mut mcp = McpProcess::start(&root);

    // Write, then read. The next line is not written until this answer is in
    // hand, so what comes back is this request's answer and not a later one.
    let init = mcp.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "glyph-cli-test", "version": "0" },
        }),
    );
    assert_eq!(init["result"]["protocolVersion"], json!("2024-11-05"), "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], json!("glyph-mcp"), "{init}");
    assert_eq!(
        init["result"]["capabilities"]["tools"]["listChanged"],
        json!(false),
        "{init}"
    );

    // A notification carries no id and gets no answer. Nothing here waits for
    // one: the next request's answer arriving with the next id is the proof,
    // because a spurious answer to the notification would be ahead of it in
    // the stream.
    mcp.notify("notifications/initialized");

    let list = mcp.request(2, "tools/list", json!({}));
    let tools = list["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tool list: {list}"));
    let names: BTreeSet<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        names.contains("glyph_variants") && names.contains("glyph_diagnostics"),
        "{names:?}"
    );

    mcp.expect_silence("after the handshake and one list");
    assert_eq!(mcp.finish(), 0, "`glyph mcp` exits 0 when stdin closes");
}

#[test]
fn initialize_carries_instructions_that_name_the_update_commands() {
    let root = unique_tmp("instructions");
    let mut mcp = McpProcess::start(&root);
    let init = mcp.handshake(1);

    // This field reaches a model's context at connect time without the model
    // choosing to read anything, which is the whole reason it exists. It is
    // also the field that is invisible to an in-process test of the
    // dispatcher's shape, so it is asserted here and only here.
    let instructions = init["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("initialize carries no `instructions` string: {init}"));
    assert!(
        instructions.len() > 200,
        "`instructions` is too short to be the guidance it is for: {instructions}"
    );
    for expected in [
        // The commands. `glyph --update` had been in the bootstrap for
        // releases and agents still reached for npm, because nothing put it in
        // front of them at the moment they were deciding.
        "glyph --update",
        "glyph upgrade",
        "glyph llms",
        // The one tool worth naming before a model has listed any.
        "glyph_variants",
        "proposed_variant",
    ] {
        assert!(
            instructions.contains(expected),
            "`instructions` never mentions `{expected}`: {instructions}"
        );
    }

    assert_eq!(mcp.finish(), 0);
}

#[test]
fn a_tools_call_over_stdio_answers_from_the_project_on_disk() {
    let root = unique_tmp("tools_call");
    let file = root.join("broken.glyph");
    std::fs::write(&file, "module broken\nfn f() -> number {\n  return \"nope\"\n}\n")
        .expect("write the fixture");

    let mut mcp = McpProcess::start(&root);
    mcp.handshake(1);
    let answer = mcp.call_tool(2, "glyph_diagnostics", json!({ "path": file }));

    // `glyph_diagnostics` answers with the list itself.
    let diagnostics = answer
        .as_array()
        .unwrap_or_else(|| panic!("`glyph_diagnostics` answered with no list: {answer}"));
    assert_eq!(diagnostics.len(), 1, "{answer}");
    assert_eq!(diagnostics[0]["code"], json!("E0204"), "{answer}");
    assert_eq!(diagnostics[0]["entity"], json!("broken::f"), "{answer}");

    mcp.expect_silence("after one tool call");
    assert_eq!(mcp.finish(), 0);
}

#[test]
fn a_malformed_line_gets_no_answer_and_does_not_end_the_server() {
    let root = unique_tmp("malformed");
    let mut mcp = McpProcess::start(&root);
    mcp.handshake(1);

    mcp.send_raw("{ this is not json");
    mcp.send_raw("");
    mcp.send_raw("[]");

    // The transport is ordered and the server reads one line at a time, so an
    // answer to any of the three would be ahead of this one in the stream.
    // Getting id 2 first is what says they were answered with nothing.
    let list = mcp.request(2, "tools/list", json!({}));
    assert!(list["result"]["tools"].is_array(), "{list}");
    mcp.expect_silence("after three lines that are not requests");

    assert_eq!(
        mcp.finish(),
        0,
        "a line the server could not read must not end it"
    );
}

// ---------------------------------------------------------------------------
// G196: the repair loop.
// ---------------------------------------------------------------------------

/// The project the loop runs over, and the only place its names exist.
///
/// Everything below is private on purpose. The tests are handed paths and
/// counts and nothing else: they cannot spell the union, the module it is
/// declared in, or the variant being added, because those strings are not in
/// scope. Every one of them a test uses has to have come out of an answer.
/// They also carry a per-run suffix, so a hard-coded one is wrong rather than
/// merely dishonest.
mod fixture {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Names for one run of one test.
    struct Names {
        /// The tagged union.
        union: String,
        /// The variant the agent adds.
        variant: String,
        /// The module the union is declared in.
        home: String,
        /// The modules that match on it. The last one holds a catch-all.
        callers: Vec<String>,
    }

    fn names() -> Names {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tag = format!("{}x{n}", std::process::id());
        Names {
            union: format!("Signal{tag}"),
            variant: format!("Quiesced{tag}"),
            home: format!("signal{tag}"),
            callers: vec![
                format!("render{tag}"),
                format!("ledger{tag}"),
                format!("audit{tag}"),
            ],
        }
    }

    pub struct Project {
        /// The project root. `glyph mcp` is started on it.
        pub root: PathBuf,
        /// The directory `glyph check` runs over.
        pub src: PathBuf,
        /// The file the agent edits, and the only path a test starts with.
        pub edited: PathBuf,
        /// How many match sites the project has over the union.
        pub sites: usize,
        /// How many of them the compiler reports once the variant is added.
        /// The rest are why the query exists.
        pub diagnosed: usize,
        names: Names,
    }

    fn put(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    /// A union, two modules that match on it exhaustively, and one that ends
    /// its match with a catch-all. The catch-all site keeps compiling when a
    /// variant is added, so no diagnostic will ever name it.
    pub fn write(root: PathBuf) -> Project {
        let names = names();
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");

        let edited = src.join(format!("{}.glyph", names.home));
        put(
            &edited,
            &format!(
                "module {home}\n\npub type {union} =\n  | Accepted\n  | Rejected\n",
                home = names.home,
                union = names.union,
            ),
        );

        let last = names.callers.len() - 1;
        for (i, caller) in names.callers.iter().enumerate() {
            let tail = if i == last {
                "    else => 9,\n".to_string()
            } else {
                format!("    {}.Rejected => 2,\n", names.home)
            };
            put(
                &src.join(format!("{caller}.glyph")),
                &format!(
                    "module {caller}\n\nimport {home}\n\npub fn probe(s: {home}.{union}) -> number {{\n  \
                     return match s {{\n    {home}.Accepted => 1,\n{tail}  }}\n}}\n",
                    home = names.home,
                    union = names.union,
                ),
            );
        }

        Project {
            root,
            src,
            edited,
            sites: names.callers.len(),
            diagnosed: last,
            names,
        }
    }

    /// The agent's edit: one more variant on the union. The declaration is the
    /// last thing in its file, so the new line goes on the end.
    pub fn add_the_variant(project: &Project) {
        let text = std::fs::read_to_string(&project.edited).expect("read the union's file");
        put(
            &project.edited,
            &format!("{text}  | {}\n", project.names.variant),
        );
    }
}

/// One string field of an answer, with the whole answer in the panic when it
/// is not there. Every value a loop test carries from one hop to the next goes
/// through this, so "it came out of the previous answer" is visible at the
/// call site rather than promised in a comment.
fn field<'a>(answer: &'a Value, path: &[&str]) -> &'a str {
    let mut cursor = answer;
    for key in path {
        cursor = &cursor[*key];
    }
    cursor
        .as_str()
        .unwrap_or_else(|| panic!("no string at `{}`: {answer}", path.join(".")))
}

/// `glyph check --json` over `src`, as an exit code and a parsed report.
///
/// `--no-tsc` keeps the check to the Glyph stages, which is what "the compiler
/// accepts this program" means here, and `--no-test` keeps it off node; the
/// fixture declares no examples, so it removes no check.
fn check_json(src: &Path) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .arg("check")
        .arg(src)
        .arg("--no-tsc")
        .arg("--no-test")
        .arg("--json")
        .output()
        .expect("spawn `glyph check`");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let report = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("`glyph check --json` wrote non-JSON ({e}): {stdout}{stderr}"));
    (out.status.code().unwrap_or(-1), report)
}

/// Insert one arm for `variant` at every site in `sites`, and nowhere else.
///
/// The files and the lines are the answer's `path` and `line`; the arm is
/// spelled from the union's own module. This function is given no other way to
/// find a match site, which is the point: it edits what the query named.
/// Returns the files it touched.
fn repair(root: &Path, sites: &[Value], union_home: &str, variant: &str) -> BTreeSet<PathBuf> {
    let mut by_file: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for site in sites {
        let path = root.join(field(site, &["path"]));
        let line = site["line"]
            .as_u64()
            .unwrap_or_else(|| panic!("a site with no line: {site}")) as usize;
        by_file.entry(path).or_default().push(line);
    }
    for (path, lines) in &mut by_file {
        // Latest line first: an insertion moves every line below it.
        lines.sort_unstable();
        lines.reverse();
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut out: Vec<String> = text.lines().map(str::to_string).collect();
        for &line in lines.iter() {
            // A site's `line` is its scrutinee's, which is the line the match
            // opens on, so the new arm goes directly after it.
            out.insert(line + 1, format!("    {union_home}.{variant} => 0,"));
        }
        std::fs::write(path, format!("{}\n", out.join("\n")))
            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }
    by_file.into_keys().collect()
}

/// The union a batch of diagnostics is about, and the variant they say is
/// missing, read as fields and cross-checked across the batch so that taking
/// them off the first diagnostic is not a choice about which one to believe.
struct FromTheDiagnostics {
    name: String,
    home: String,
    declaration: String,
    variant: String,
    entities: BTreeSet<String>,
}

fn read_the_diagnostics(report: &Value, expected: usize) -> FromTheDiagnostics {
    let diagnostics = report["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics array: {report}"));
    assert_eq!(
        diagnostics.len(),
        expected,
        "the sites the compiler reports: {report}"
    );

    let first = &diagnostics[0];
    let read = FromTheDiagnostics {
        name: field(first, &["union", "name"]).to_string(),
        home: field(first, &["union", "module"]).to_string(),
        declaration: field(first, &["union", "declaration"]).to_string(),
        variant: first["missing_variants"][0]
            .as_str()
            .unwrap_or_else(|| panic!("no missing variant: {first}"))
            .to_string(),
        entities: diagnostics
            .iter()
            .map(|d| field(d, &["entity"]).to_string())
            .collect(),
    };

    for d in diagnostics {
        assert_eq!(d["code"], json!("E0200"), "{d}");
        assert_eq!(field(d, &["union", "name"]), read.name, "{d}");
        assert_eq!(field(d, &["union", "declaration"]), read.declaration, "{d}");
        assert_eq!(
            d["missing_variants"],
            json!([read.variant.as_str()]),
            "every diagnostic in the batch names the same one missing variant: {d}"
        );
    }
    assert_eq!(
        read.entities.len(),
        expected,
        "each diagnostic sits in a declaration of its own: {report}"
    );
    read
}

#[test]
fn the_repair_loop_closes_with_every_hop_fed_from_the_previous_answer() {
    let project = fixture::write(unique_tmp("repair_loop"));

    // The tree is green before the edit, so what follows is caused by it.
    let (code, clean) = check_json(&project.src);
    assert_eq!(code, 0, "the fixture starts green: {clean}");

    // Hop 1. The agent edits.
    fixture::add_the_variant(&project);

    // Hop 2. `glyph check` turns the edit into diagnostics.
    let (code, broken) = check_json(&project.src);
    assert_eq!(code, 1, "the edit has to break the build: {broken}");
    let told = read_the_diagnostics(&broken, project.diagnosed);

    // Hop 3. The query, addressed with the union's own name out of the
    // diagnostic. The path is the file the agent itself edited, which is the
    // one thing in this call the agent already had.
    let mut mcp = McpProcess::start(&project.root);
    mcp.handshake(1);
    let answer = mcp.call_tool(
        2,
        "glyph_variants",
        json!({ "path": project.edited, "name": told.name }),
    );

    // The two surfaces are about the same declaration, each saying so in a
    // field rather than in a sentence.
    assert_eq!(
        field(&answer, &["type", "declaration"]),
        told.declaration,
        "{answer}"
    );
    assert!(
        answer["type"]["variants"]
            .as_array()
            .unwrap_or_else(|| panic!("no variant list: {answer}"))
            .iter()
            .any(|v| v == &json!(told.variant.as_str())),
        "the union the query answered about has to hold the variant the \
         diagnostic says is missing: {answer}"
    );

    // Hop 4. The sites. This is where the query earns its place: it names
    // every site, and the compiler named only the ones that stopped compiling.
    let sites = answer["sites"]
        .as_array()
        .unwrap_or_else(|| panic!("no sites: {answer}"))
        .clone();
    assert_eq!(sites.len(), project.sites, "{answer}");

    let by_query: BTreeSet<String> = sites
        .iter()
        .map(|s| field(s, &["declaration"]).to_string())
        .collect();
    assert!(
        by_query.is_superset(&told.entities),
        "every site the compiler reported has to be one the query names.\n\
         compiler: {:?}\nquery: {by_query:?}",
        told.entities
    );
    let only_the_query: Vec<&String> = by_query.difference(&told.entities).collect();
    assert_eq!(
        only_the_query.len(),
        project.sites - project.diagnosed,
        "the query has to name a site no diagnostic did: {by_query:?} against {:?}",
        told.entities
    );
    for extra in &only_the_query {
        let site = sites
            .iter()
            .find(|s| field(s, &["declaration"]) == extra.as_str())
            .expect("the site is in the list it came from");
        assert_eq!(
            site["state"],
            json!("has_catch_all"),
            "the site the compiler never mentions is the one whose catch-all \
             silently takes the new variant: {site}"
        );
    }

    // Hop 5. The fix, at the sites the query named and nowhere else.
    let touched = repair(&project.root, &sites, &told.home, &told.variant);
    let named: BTreeSet<PathBuf> = sites
        .iter()
        .map(|s| project.root.join(field(s, &["path"])))
        .collect();
    assert_eq!(touched, named, "the edit went where the answer pointed");

    // Hop 6. Green.
    let (code, fixed) = check_json(&project.src);
    assert_eq!(code, 0, "the loop has to close: {fixed}");
    assert_eq!(fixed["ok"], json!(true), "{fixed}");
    assert_eq!(fixed["errors"], json!(0), "{fixed}");

    assert_eq!(mcp.finish(), 0);
}

#[test]
fn repairing_only_what_the_compiler_reported_leaves_the_absorbing_site_behind() {
    let project = fixture::write(unique_tmp("repair_partial"));
    fixture::add_the_variant(&project);

    let (code, broken) = check_json(&project.src);
    assert_eq!(code, 1, "{broken}");
    let told = read_the_diagnostics(&broken, project.diagnosed);

    let mut mcp = McpProcess::start(&project.root);
    mcp.handshake(1);
    let answer = mcp.call_tool(
        2,
        "glyph_variants",
        json!({ "path": project.edited, "name": told.name }),
    );
    let sites = answer["sites"]
        .as_array()
        .unwrap_or_else(|| panic!("no sites: {answer}"))
        .clone();

    // An agent that fixes what it was told about, and nothing else. The subset
    // is picked by matching the query's `declaration` against the diagnostic's
    // `entity`: the same identity, spelled the same way on both surfaces.
    let reported: Vec<Value> = sites
        .iter()
        .filter(|s| told.entities.contains(field(s, &["declaration"])))
        .cloned()
        .collect();
    assert_eq!(reported.len(), project.diagnosed, "{answer}");
    repair(&project.root, &reported, &told.home, &told.variant);

    let (code, green) = check_json(&project.src);
    assert_eq!(
        code, 0,
        "fixing every diagnostic gives a green tree, which is the problem: {green}"
    );
    assert_eq!(green["errors"], json!(0), "{green}");

    // Green, and one site still has no arm for the new variant. Nothing in the
    // build says so; the query does.
    let after = mcp.call_tool(
        3,
        "glyph_variants",
        json!({ "path": project.edited, "name": told.name }),
    );
    let unhandled: Vec<&Value> = after["sites"]
        .as_array()
        .unwrap_or_else(|| panic!("no sites: {after}"))
        .iter()
        .filter(|s| {
            !s["arms"]
                .as_array()
                .map(|arms| arms.iter().any(|a| a["variant"] == json!(told.variant.as_str())))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        unhandled.len(),
        project.sites - project.diagnosed,
        "a green tree with a site that names no arm for the new variant: {after}"
    );
    for site in &unhandled {
        assert_eq!(site["state"], json!("has_catch_all"), "{site}");
    }

    assert_eq!(mcp.finish(), 0);
}

/// Everything an agent needs to ask `glyph_impact` about a parameter type
/// change, and nothing it could use to fake the answer. The three callers are
/// three different kinds of site: one passes a string literal, one passes a
/// value of a union declared in another module, and one reads the function as
/// a value without applying it.
mod signature_fixture {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct Project {
        pub root: PathBuf,
        pub src: PathBuf,
        /// The file declaring the function, and the one the agent edits.
        pub edited: PathBuf,
        /// The `module::name` of the function.
        pub entity: String,
        /// The caller passing a primitive.
        pub literal_caller: String,
        /// The caller passing an imported union value.
        pub imported_caller: String,
        /// The module reading the function as a value.
        pub value_reader: String,
    }

    fn put(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    pub fn write(root: PathBuf) -> Project {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tag = format!("{}s{n}", std::process::id());
        let home = format!("lib{tag}");
        let cells = format!("cells{tag}");
        let (literal, imported, reader) =
            (format!("lit{tag}"), format!("imp{tag}"), format!("rd{tag}"));
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        put(
            &root.join("package.json"),
            "{ \"name\": \"signature\", \"glyph\": { \"src\": \"src\" } }",
        );
        let edited = src.join(format!("{home}.glyph"));
        // The body does not read the parameter, so the edit under test (the
        // parameter's type) breaks nothing inside the declaration itself.
        put(
            &edited,
            &format!("module {home}\n\npub fn label(s: string) -> number {{\n  return 1\n}}\n"),
        );
        put(
            &src.join(format!("{cells}.glyph")),
            &format!("module {cells}\n\npub type Cell =\n  | Text\n  | Blank\n"),
        );
        put(
            &src.join(format!("{literal}.glyph")),
            &format!(
                "module {literal}\n\nimport {home} {{ label }}\n\n\
                 pub fn probe() -> number {{\n  return label(\"x\")\n}}\n"
            ),
        );
        put(
            &src.join(format!("{imported}.glyph")),
            &format!(
                "module {imported}\n\nimport {home} {{ label }}\nimport {cells} {{ Cell }}\n\n\
                 pub fn probe(c: Cell) -> number {{\n  return label(c)\n}}\n"
            ),
        );
        put(
            &src.join(format!("{reader}.glyph")),
            &format!(
                "module {reader}\n\nimport {home} {{ label }}\n\n\
                 pub const sizer: fn(string) -> number = label\n"
            ),
        );
        Project {
            root,
            src,
            edited,
            entity: format!("{home}::label"),
            literal_caller: format!("{literal}::probe"),
            imported_caller: format!("{imported}::probe"),
            value_reader: format!("{reader}::sizer"),
        }
    }

    /// The agent's edit: the parameter's type, and only that.
    pub fn change_the_parameter_type(project: &Project) {
        let text = std::fs::read_to_string(&project.edited).expect("read the function's file");
        assert!(text.contains("(s: string)"), "{text}");
        put(&project.edited, &text.replace("(s: string)", "(s: number)"));
    }
}

/// An agent about to change a parameter's type asks what breaks, over the real
/// protocol, and gets a different verdict for each kind of site, each with the
/// reason. Then it makes the edit and the compiler confirms the verdicts: the
/// site called WILL_FAIL is the one E0211 lands on, and the site called
/// UNDETERMINED is the one the compiler says nothing about.
#[test]
fn a_signature_type_change_gets_one_verdict_per_kind_of_site() {
    let project = signature_fixture::write(unique_tmp("signature_type"));
    let (code, clean) = check_json(&project.src);
    assert_eq!(code, 0, "the fixture starts green: {clean}");

    let mut mcp = McpProcess::start(&project.root);
    mcp.handshake(1);
    let answer = mcp.call_tool(
        2,
        "glyph_impact",
        json!({ "entity": project.entity, "change": { "kind": "change_signature_type" } }),
    );
    assert_eq!(field(&answer, &["entity"]), project.entity, "{answer}");

    let by_entity: BTreeMap<String, &Value> = answer["impact"]
        .as_array()
        .unwrap_or_else(|| panic!("no `impact` in {answer}"))
        .iter()
        .filter_map(|e| e["entity"].as_str().map(|n| (n.to_string(), e)))
        .collect();
    let verdict_of = |name: &str| -> &Value {
        by_entity
            .get(name)
            .unwrap_or_else(|| panic!("no entry for `{name}` in {answer}"))
    };
    let literal = verdict_of(&project.literal_caller);
    let imported = verdict_of(&project.imported_caller);
    let reader = verdict_of(&project.value_reader);

    assert_eq!(literal["verdict"], json!("WILL_FAIL"), "{literal}");
    assert_eq!(literal["diagnostic"], json!("E0211"), "{literal}");
    assert_eq!(imported["verdict"], json!("UNDETERMINED"), "{imported}");
    assert_eq!(reader["verdict"], json!("NOT_INDEXED"), "{reader}");
    for entry in [literal, imported, reader] {
        assert!(
            field(entry, &["because"]).len() > 20,
            "a verdict with no reason: {entry}"
        );
    }
    // The three are three different claims, and the answer says which is
    // which in a field, not in prose an agent would have to parse.
    let seen: BTreeSet<&str> = [literal, imported, reader]
        .iter()
        .filter_map(|e| e["verdict"].as_str())
        .collect();
    assert_eq!(seen.len(), 3, "{answer}");

    // The edit, then the compiler's own word on it.
    signature_fixture::change_the_parameter_type(&project);
    let (code, broken) = check_json(&project.src);
    assert_eq!(code, 1, "the edit has to break the build: {broken}");
    let diagnostics = broken["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics: {broken}"));
    let reported: BTreeSet<(String, String)> = diagnostics
        .iter()
        .filter_map(|d| {
            Some((
                d["entity"].as_str()?.to_string(),
                d["code"].as_str()?.to_string(),
            ))
        })
        .collect();
    assert!(
        reported.contains(&(project.literal_caller.clone(), "E0211".to_string())),
        "the WILL_FAIL site has to be where E0211 lands: {broken}"
    );
    assert!(
        !reported.iter().any(|(e, _)| e == &project.imported_caller),
        "UNDETERMINED means the compiler says nothing here, and it said something: {broken}"
    );
    assert!(
        !reported.iter().any(|(e, _)| e == &project.value_reader),
        "NOT_INDEXED means the compiler says nothing here, and it said something: {broken}"
    );

    assert_eq!(mcp.finish(), 0);
}

/// One module with a union, an empty record, and a caller of each. The two
/// callers pass the same string literal; what differs is what the checker
/// compares it against.
mod declared_signature_fixture {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct Project {
        pub root: PathBuf,
        pub src: PathBuf,
        /// The one file, which the agent edits.
        pub edited: PathBuf,
        /// `module::tag`, whose `string` parameter is replaced by the union.
        pub tag: String,
        /// `module::fill`, whose parameter is the empty record.
        pub fill: String,
        /// The caller passing a literal to `tag`.
        pub tag_caller: String,
        /// The caller passing a literal to `fill`.
        pub fill_caller: String,
    }

    fn put(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    pub fn write(root: PathBuf) -> Project {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let shop = format!("shop{}d{n}", std::process::id());
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        put(
            &root.join("package.json"),
            "{ \"name\": \"declared\", \"glyph\": { \"src\": \"src\" } }",
        );
        let edited = src.join(format!("{shop}.glyph"));
        // Neither body reads its parameter, so the edit under test breaks
        // nothing inside the declaration it is made in.
        put(
            &edited,
            &format!(
                "module {shop}\n\n\
                 pub type Kind =\n  | Retail\n  | Wholesale\n\n\
                 pub type Blank = {{ }}\n\n\
                 pub fn tag(s: string) -> number {{\n  return 1\n}}\n\n\
                 pub fn fill(b: Blank) -> number {{\n  return 1\n}}\n\n\
                 pub fn tagged() -> number {{\n  return tag(\"x\")\n}}\n\n\
                 pub fn filled() -> number {{\n  return fill(\"x\")\n}}\n"
            ),
        );
        Project {
            root,
            src,
            edited,
            tag: format!("{shop}::tag"),
            fill: format!("{shop}::fill"),
            tag_caller: format!("{shop}::tagged"),
            fill_caller: format!("{shop}::filled"),
        }
    }

    /// The agent's edit: `tag`'s parameter becomes the module's own union.
    pub fn replace_the_parameter_with_the_union(project: &Project) {
        let text = std::fs::read_to_string(&project.edited).expect("read the module");
        assert!(text.contains("tag(s: string)"), "{text}");
        put(&project.edited, &text.replace("tag(s: string)", "tag(s: Kind)"));
    }
}

/// A `WILL_FAIL` under `change_signature_type` names the classes of
/// replacement the checker compares the argument against, and the checker has
/// to agree with each class it names. For a string literal the answer names a
/// replacement primitive and a replacement union or record declared in the
/// calling module with at least one field; the agent replaces `string` with
/// the module's union, and E0211 lands on the site the answer called
/// `WILL_FAIL`. The same literal against the empty record `type Blank = { }`
/// is `UNDETERMINED`, with the reason, and the compiler says nothing there
/// before or after.
///
/// The pairing itself (a primitive argument against a declared union
/// parameter) is asked over the protocol after the edit, on the site the
/// compiler has just diagnosed, since no green program holds that pairing.
#[test]
fn a_signature_type_change_to_a_declared_type_is_confirmed_by_the_checker() {
    let project = declared_signature_fixture::write(unique_tmp("declared_signature_type"));
    let (code, clean) = check_json(&project.src);
    assert_eq!(code, 0, "the fixture starts green: {clean}");

    let mut mcp = McpProcess::start(&project.root);
    mcp.handshake(1);
    let entry_for = |answer: &Value, caller: &str| -> Value {
        answer["impact"]
            .as_array()
            .unwrap_or_else(|| panic!("no `impact` in {answer}"))
            .iter()
            .find(|e| e["entity"] == caller && e["relation"] == "CALLS")
            .cloned()
            .unwrap_or_else(|| panic!("no CALLS entry for `{caller}` in {answer}"))
    };

    let tag = mcp.call_tool(
        2,
        "glyph_impact",
        json!({ "entity": project.tag, "change": { "kind": "change_signature_type" } }),
    );
    let tagged = entry_for(&tag, &project.tag_caller);
    assert_eq!(tagged["verdict"], json!("WILL_FAIL"), "{tagged}");
    assert_eq!(tagged["diagnostic"], json!("E0211"), "{tagged}");
    let because = field(&tagged, &["because"]);
    assert!(
        because.contains("replacement primitive") && because.contains("at least one field"),
        "the classes the argument is compared against are unnamed: {tagged}"
    );

    let fill = mcp.call_tool(
        3,
        "glyph_impact",
        json!({ "entity": project.fill, "change": { "kind": "change_signature_type" } }),
    );
    let filled = entry_for(&fill, &project.fill_caller);
    assert_eq!(filled["verdict"], json!("UNDETERMINED"), "{filled}");
    assert!(filled["diagnostic"].is_null(), "{filled}");
    assert!(
        field(&filled, &["because"]).contains("no fields"),
        "the exclusion is unnamed: {filled}"
    );

    // The edit, in the class the verdict named, then the compiler's own word.
    declared_signature_fixture::replace_the_parameter_with_the_union(&project);
    let (code, broken) = check_json(&project.src);
    assert_eq!(code, 1, "the edit has to break the build: {broken}");
    let reported: BTreeSet<(String, String)> = broken["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics: {broken}"))
        .iter()
        .filter_map(|d| {
            Some((
                d["entity"].as_str()?.to_string(),
                d["code"].as_str()?.to_string(),
            ))
        })
        .collect();
    assert!(
        reported.contains(&(project.tag_caller.clone(), "E0211".to_string())),
        "the WILL_FAIL site has to be where E0211 lands: {broken}"
    );
    assert!(
        !reported.iter().any(|(e, _)| e == &project.fill_caller),
        "UNDETERMINED means the compiler says nothing here, and it said something: {broken}"
    );

    // The site is now a primitive argument against a declared union parameter,
    // and the answer for that pairing is the diagnostic the compiler just gave.
    let after = mcp.call_tool(
        4,
        "glyph_impact",
        json!({ "entity": project.tag, "change": { "kind": "change_signature_type" } }),
    );
    let tagged = entry_for(&after, &project.tag_caller);
    assert_eq!(tagged["verdict"], json!("WILL_FAIL"), "{tagged}");
    assert_eq!(tagged["diagnostic"], json!("E0211"), "{tagged}");
    assert!(
        field(&tagged, &["because"]).contains("union"),
        "the shape the checker read is unnamed: {tagged}"
    );

    assert_eq!(mcp.finish(), 0);
}
