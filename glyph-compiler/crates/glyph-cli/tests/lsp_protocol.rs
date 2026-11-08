//! The language server answered over its own protocol.
//!
//! These tests speak LSP framing to `glyph lsp` the way an editor does:
//! `initialize` with the project as `rootUri`, `initialized`, `didOpen`, then
//! the request. Reaching the server through its wire is the point. The
//! analysis functions underneath are tested in `glyph-lsp`, and a fault in
//! what sits between them and the editor (the overlay, the cross-module
//! lookup, the position mapping) is invisible there.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

/// A running `glyph lsp` and the messages it has sent.
struct Lsp {
    child: Child,
    stdin: ChildStdin,
    inbox: Receiver<Value>,
    next_id: u64,
}

impl Lsp {
    /// Start a server and initialize it on `root`.
    fn start(root: &Path) -> Lsp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_glyph"))
            .arg("lsp")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn glyph lsp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, inbox) = channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.strip_prefix("Content-Length: ") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; length];
                if r.read_exact(&mut body).is_err() {
                    return;
                }
                if let Ok(message) = serde_json::from_slice(&body) {
                    if tx.send(message).is_err() {
                        return;
                    }
                }
            }
        });
        let mut lsp = Lsp { child, stdin, inbox, next_id: 0 };
        let root_uri = uri(root);
        lsp.request(
            "initialize",
            json!({ "processId": null, "rootUri": root_uri, "capabilities": {} }),
        );
        lsp.notify("initialized", json!({}));
        lsp
    }

    fn send(&mut self, message: &Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write");
        self.stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Send a request and return its `result`, skipping the notifications
    /// (diagnostics, logs) the server sends in between.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            // A freshly built binary can sit in the macOS launch scan for a
            // minute before it reads anything, so the wait is generous.
            let message = self
                .inbox
                .recv_timeout(Duration::from_secs(180))
                .unwrap_or_else(|_| panic!("no answer to {method}"));
            if message.get("id") == Some(&json!(id)) && message.get("method").is_none() {
                assert!(message.get("error").is_none(), "{method}: {message}");
                return message["result"].clone();
            }
        }
    }

    fn open(&mut self, file: &Path) {
        let text = std::fs::read_to_string(file).expect("read the opened file");
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri(file), "languageId": "glyph", "version": 1, "text": text,
            } }),
        );
    }
}

/// One `publishDiagnostics`: the version it carried and the diagnostics.
type Publish = (Option<i64>, Vec<Value>);

/// How long the server has to stay silent before a test takes the last
/// publish as final. Several times the server's 300 ms emitter debounce, so a
/// pending emitter run has landed by then even on a loaded machine.
const QUIET: Duration = Duration::from_secs(3);

/// A `publishDiagnostics` for `want`, or `None` for any other message.
fn as_publish(message: &Value, want: &str) -> Option<Publish> {
    (message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
        && message["params"]["uri"] == json!(want))
    .then(|| {
        (
            message["params"]["version"].as_i64(),
            message["params"]["diagnostics"].as_array().cloned().unwrap_or_default(),
        )
    })
}

impl Lsp {
    /// The next `publishDiagnostics` for `file`, skipping everything else the
    /// server sends.
    fn next_publish(&mut self, file: &Path) -> Publish {
        let want = uri(file);
        loop {
            let message = self
                .inbox
                .recv_timeout(Duration::from_secs(180))
                .unwrap_or_else(|_| panic!("no diagnostics published for {want}"));
            if let Some(p) = as_publish(&message, &want) {
                return p;
            }
        }
    }

    /// The diagnostics of the next publish for `file`.
    fn diagnostics_for(&mut self, file: &Path) -> Vec<Value> {
        self.next_publish(file).1
    }

    /// Every publish for `file` until the server has been silent for
    /// [`QUIET`], in arrival order. At least one is required.
    fn publishes_until_quiet(&mut self, file: &Path) -> Vec<Publish> {
        let want = uri(file);
        let mut out = vec![self.next_publish(file)];
        while let Ok(message) = self.inbox.recv_timeout(QUIET) {
            if let Some(p) = as_publish(&message, &want) {
                out.push(p);
            }
        }
        out
    }

    /// Replace `file`'s buffer with `text` at `version`, as a keystroke does.
    fn change(&mut self, file: &Path, version: i64, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri(file), "version": version },
                "contentChanges": [{ "text": text }],
            }),
        );
    }

    fn save(&mut self, file: &Path) {
        self.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri(file) } }));
    }
}

/// The codes in one publish's diagnostics.
fn codes_of(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| d["code"].as_str().unwrap_or_default().to_string())
        .collect()
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// A uniquely named, canonical temp directory. Canonical because the server
/// answers with the path it resolved, and `/tmp` is a symlink on macOS.
fn tmp_dir(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "glyph_lsp_protocol_{prefix}_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.canonicalize().expect("canonical temp dir")
}

const UI: &str = "module ui\n\npub type Props = { label: string, }\n\n\
    pub component Button(props: Props) {\n  return <button>{props.label}</button>\n}\n";

/// The three ways `main` can bring in `ui`, each with the qualifier its uses
/// are written with: none for the named import.
const SPELLINGS: [(&str, &str); 3] = [
    ("import ui { Button, Props }", ""),
    ("import ui", "ui."),
    ("import ui as u", "u."),
];

fn main_under(import: &str, ns: &str) -> String {
    format!(
        "module main\n\n{import}\n\npub fn pick(p: {ns}Props) -> string {{\n  \
         let _b = {ns}Button\n  return p.label\n}}\n\npub component App() {{\n  \
         return <div>\n    <{ns}Button label=\"go\" />\n    \
         <{ns}Button label=\"again\">twice</{ns}Button>\n  </div>\n}}\n"
    )
}

/// Each site as (what it is, a substring of its line, the name, which
/// occurrence of the name on that line).
const SITES: [(&str, &str, &str, usize); 5] = [
    ("type position", "pub fn pick", "Props", 0),
    ("value position", "let _b =", "Button", 0),
    ("a self-closing element", "label=\"go\"", "Button", 0),
    ("an opening tag", "label=\"again\"", "Button", 0),
    ("a closing tag", "label=\"again\"", "Button", 1),
];

fn site_in(text: &str, needle: &str, name: &str, nth: usize) -> (usize, usize) {
    let (line, l) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(needle))
        .unwrap_or_else(|| panic!("no line holds {needle}"));
    let (character, _) = l
        .match_indices(name)
        .nth(nth)
        .unwrap_or_else(|| panic!("no {name} #{nth} on {l}"));
    (line, character)
}

/// `(definition, hover)` at every site, one row per spelling, named first.
/// A definition is the target's file name and range, so three projects in
/// three directories can be compared.
fn answers_by_spelling() -> Vec<Vec<(Value, Value)>> {
    SPELLINGS
        .iter()
        .map(|(import, ns)| {
            let dir = tmp_dir("spelling");
            std::fs::write(dir.join("ui.glyph"), UI).expect("write ui");
            let main = main_under(import, ns);
            let main_path = dir.join("main.glyph");
            std::fs::write(&main_path, &main).expect("write main");
            let mut lsp = Lsp::start(&dir);
            lsp.open(&main_path);
            let row = SITES
                .iter()
                .map(|(_, needle, name, nth)| {
                    let (line, character) = site_in(&main, needle, name, *nth);
                    let at = json!({
                        "textDocument": { "uri": uri(&main_path) },
                        "position": { "line": line, "character": character },
                    });
                    let def = lsp.request("textDocument/definition", at.clone());
                    let def = match def.get("uri").and_then(Value::as_str) {
                        Some(u) => json!({
                            "file": u.rsplit('/').next(),
                            "range": def["range"],
                        }),
                        None => def,
                    };
                    let hover = lsp.request("textDocument/hover", at);
                    (def, hover["contents"]["value"].clone())
                })
                .collect();
            drop(lsp);
            let _ = std::fs::remove_dir_all(&dir);
            row
        })
        .collect()
}

/// G249, over the protocol. Go-to-definition on `Button` in `ui.Button`,
/// `<ui.Button>` and `</ui.Button>`, and on `Props` in `ui.Props`, lands on
/// the declaration in `ui.glyph`, at the range the named import's
/// definition lands on. Hover on the same segment answers what the named
/// import's hover answers. Before the fix the value and element positions
/// answered `null`, and the type position jumped to the `import` line.
#[test]
fn definition_and_hover_on_a_member_are_the_named_imports_answer() {
    let answers = answers_by_spelling();
    for ((what, ..), (def, hover)) in SITES.iter().zip(&answers[0]) {
        assert_eq!(def["file"], "ui.glyph", "{what}: {def}");
        assert!(def["range"].is_object(), "{what}: {def}");
        assert!(hover.as_str().is_some_and(|s| !s.is_empty()), "{what}: {hover}");
    }
    for (row, (import, _)) in answers.iter().zip(SPELLINGS).skip(1) {
        for ((what, ..), (got, want)) in SITES.iter().zip(row.iter().zip(&answers[0])) {
            assert_eq!(got, want, "{what} under `{import}`");
        }
    }
}

/// `glyph check --json --no-tsc` over `dir`: each diagnostic in `main.glyph`
/// as its code, severity and zero-based start, which is what an editor shows.
fn check_codes(dir: &Path) -> Vec<(String, String, u64, u64)> {
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .arg("check")
        .arg(dir)
        .arg("--no-tsc")
        .arg("--no-test")
        .arg("--json")
        .output()
        .expect("spawn `glyph check`");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let report: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("`glyph check --json` wrote non-JSON ({e}): {stdout}"));
    report["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics: {report}"))
        .iter()
        .filter(|d| d["file"] == json!("main.glyph"))
        .map(|d| {
            (
                d["code"].as_str().unwrap_or_default().to_string(),
                d["severity"].as_str().unwrap_or_default().to_string(),
                d["range"]["start"]["line"].as_u64().unwrap_or(0) - 1,
                d["range"]["start"]["col"].as_u64().unwrap_or(0) - 1,
            )
        })
        .collect()
}

/// G295. The emitter's refusals reach the editor. E0302 to E0305 are errors
/// of `glyph check` that the checker does not make; the server used to stop
/// after the checker and publish nothing for them, so the editor showed a
/// file as clean that `glyph check` refused. Each fixture is opened over the
/// protocol, and what the server publishes for it is what `glyph check`
/// reports for it: code, severity and position. The imported-record case is
/// the one that needs the project's tables, because E0304 there is the
/// declaring module's verdict (G210).
#[test]
fn the_editor_publishes_the_emitters_errors() {
    const ORDERS_OPAQUE: &str = "module orders\npub type Handle = extern_ts(\"{ fd: number }\")\n\
        pub type Order = {\n  id: string,\n  handle: Handle,\n}\n";
    let cases: [(&str, &str, Option<&str>); 4] = [
        (
            "E0302",
            "module main\npub type Id =\n  | None\n  | Some(number)\n\n\
             fn f(b: string) -> Result<string, string> {\n  return Ok(b)\n}\n\n\
             fn shout(s: string) -> string {\n  return s\n}\n\n\
             pub fn pick(id: Id, b: string) -> Result<string, string> {\n  \
             return Ok(shout(match id {\n    None => f(b)?,\n    Some(n) => b,\n  }))\n}\n",
            None,
        ),
        (
            "E0303",
            "module main\npub type Id =\n  | None\n  | Some(number)\n\n\
             fn load(p: string) -> Result<Id, string> {\n  return Ok(None)\n}\n\n\
             pub fn pick(p: string) -> Result<string, string> {\n  \
             let x = match load(p)? {\n    None => \"n\",\n    Some(n) => \"s\",\n  }\n  \
             return Ok(x)\n}\n",
            None,
        ),
        (
            "E0305",
            "module main\npub type U =\n  | A\n  | B\n\n\
             pub fn f(u: U) -> number {\n  return match u {\n    A => 1,\n    A => 2,\n    B => 3,\n  }\n}\n",
            None,
        ),
        (
            "E0304",
            "module main\nimport orders { Order }\n\n\
             pub fn take(v: unknown) -> bool {\n  return match Order.parse(v) {\n    \
             Ok(_) => true,\n    Err(_) => false,\n  }\n}\n",
            Some(ORDERS_OPAQUE),
        ),
    ];
    for (code, main, orders) in cases {
        let dir = tmp_dir(&code.to_lowercase());
        let main_path = dir.join("main.glyph");
        std::fs::write(&main_path, main).expect("write main");
        if let Some(orders) = orders {
            std::fs::write(dir.join("orders.glyph"), orders).expect("write orders");
        }
        let from_cli = check_codes(&dir);
        assert!(
            from_cli.iter().any(|(c, ..)| c == code),
            "the fixture no longer draws {code} from `glyph check`: {from_cli:?}"
        );

        let as_cli = |published: &[Value]| -> Vec<(String, String, u64, u64)> {
            published
                .iter()
                .map(|d| {
                    let severity = match d["severity"].as_u64() {
                        Some(1) => "error",
                        Some(2) => "warning",
                        _ => "other",
                    };
                    (
                        d["code"].as_str().unwrap_or_default().to_string(),
                        severity.to_string(),
                        d["range"]["start"]["line"].as_u64().unwrap_or(0),
                        d["range"]["start"]["character"].as_u64().unwrap_or(0),
                    )
                })
                .collect()
        };

        // An open publishes the whole list at once: the first publish is it.
        let mut lsp = Lsp::start(&dir);
        lsp.open(&main_path);
        let published = lsp.diagnostics_for(&main_path);
        assert_eq!(
            as_cli(&published),
            from_cli,
            "{code}: on open, the editor and `glyph check` disagree about main.glyph: {published:?}"
        );

        // A keystroke publishes the front end at once, with the emitter's
        // last refusal carried forward through the edit, and the whole list
        // once typing pauses. A trailing newline moves no position, so the
        // carried refusal is where `glyph check` puts it.
        lsp.change(&main_path, 2, &format!("{main}\n"));
        let after = lsp.publishes_until_quiet(&main_path);
        let (first_version, first) = &after[0];
        assert_eq!(*first_version, Some(2), "{code}: {after:?}");
        assert_eq!(
            as_cli(first),
            from_cli,
            "{code}: the keystroke's first publish dropped or moved the emitter's refusal: {first:?}"
        );
        let (last_version, last) = after.last().expect("a publish");
        assert_eq!(*last_version, Some(2), "{code}: {after:?}");
        assert_eq!(
            as_cli(last),
            from_cli,
            "{code}: once typing pauses, the editor and `glyph check` disagree: {after:?}"
        );
        drop(lsp);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A clean module, the same module with a `?` in the arm of a `match` nested
/// in a call (E0302, which only the emitter reports), and a clean edit of it.
const CLEAN_PICK: &str = "module main\npub type Id =\n  | None\n  | Some(number)\n\n\
    fn f(b: string) -> Result<string, string> {\n  return Ok(b)\n}\n\n\
    fn shout(s: string) -> string {\n  return s\n}\n\n\
    pub fn pick(id: Id, b: string) -> Result<string, string> {\n  \
    let r = f(b)?\n  return Ok(shout(match id {\n    None => r,\n    Some(n) => b,\n  }))\n}\n";
const E0302_PICK: &str = "module main\npub type Id =\n  | None\n  | Some(number)\n\n\
    fn f(b: string) -> Result<string, string> {\n  return Ok(b)\n}\n\n\
    fn shout(s: string) -> string {\n  return s\n}\n\n\
    pub fn pick(id: Id, b: string) -> Result<string, string> {\n  \
    let r = f(b)?\n  return Ok(shout(match id {\n    None => f(b)?,\n    Some(n) => r,\n  }))\n}\n";
const FIXED_PICK: &str = "module main\npub type Id =\n  | None\n  | Some(number)\n\n\
    fn f(b: string) -> Result<string, string> {\n  return Ok(b)\n}\n\n\
    fn shout(s: string) -> string {\n  return s\n}\n\n\
    pub fn pick(id: Id, b: string) -> Result<string, string> {\n  \
    let r = f(b)?\n  return Ok(shout(match id {\n    None => r,\n    Some(n) => r,\n  }))\n}\n";

/// G295: rapid edits never leave a stale emitter diagnostic on the file.
///
/// An E0302 is typed and fixed again inside the debounce window. The emitter
/// run started for the broken text must not publish once the fix has: no
/// publish after the fix's first one carries E0302, and the last publish is
/// the fix's, with no E0302 in it.
#[test]
fn a_fixed_emitter_error_never_comes_back_from_a_stale_run() {
    let dir = tmp_dir("stale");
    let main_path = dir.join("main.glyph");
    std::fs::write(&main_path, CLEAN_PICK).expect("write main");
    // The broken text draws E0302 at all, or this proves nothing.
    let broken_dir = tmp_dir("stale_broken");
    std::fs::write(broken_dir.join("main.glyph"), E0302_PICK).expect("write broken");
    assert!(
        check_codes(&broken_dir).iter().any(|(c, ..)| c == "E0302"),
        "the broken text no longer draws E0302"
    );
    let _ = std::fs::remove_dir_all(&broken_dir);

    let mut lsp = Lsp::start(&dir);
    lsp.open(&main_path);
    assert!(lsp.diagnostics_for(&main_path).is_empty(), "the opened text is clean");

    lsp.change(&main_path, 2, E0302_PICK);
    lsp.change(&main_path, 3, FIXED_PICK);
    let after = lsp.publishes_until_quiet(&main_path);
    let fixed_at = after
        .iter()
        .position(|(v, _)| *v == Some(3))
        .unwrap_or_else(|| panic!("the fix was never published: {after:?}"));
    for (version, diagnostics) in &after[fixed_at..] {
        assert!(
            !codes_of(diagnostics).iter().any(|c| c == "E0302"),
            "a stale E0302 (version {version:?}) landed after the fix: {after:?}"
        );
    }
    let (last_version, last) = after.last().expect("a publish");
    assert_eq!(*last_version, Some(3), "{after:?}");
    assert!(last.is_empty(), "the fixed text is clean: {after:?}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// G295: a save publishes the emitter's refusals at once, without the
/// debounce. A keystroke introduces an E0302, the editor saves, and the next
/// keystroke fixes it straight away. The fix supersedes any debounced run for
/// the broken text, so the only publish that can carry the E0302 is the
/// save's own.
#[test]
fn a_save_publishes_the_emitters_errors_at_once() {
    let dir = tmp_dir("save");
    let main_path = dir.join("main.glyph");
    std::fs::write(&main_path, CLEAN_PICK).expect("write main");
    let mut lsp = Lsp::start(&dir);
    lsp.open(&main_path);
    assert!(lsp.diagnostics_for(&main_path).is_empty(), "the opened text is clean");

    lsp.change(&main_path, 2, E0302_PICK);
    std::fs::write(&main_path, E0302_PICK).expect("write what the editor saves");
    lsp.save(&main_path);
    lsp.change(&main_path, 3, FIXED_PICK);
    let after = lsp.publishes_until_quiet(&main_path);
    assert!(
        after
            .iter()
            .any(|(v, d)| *v == Some(2) && codes_of(d).iter().any(|c| c == "E0302")),
        "the save did not publish the emitter's refusal: {after:?}"
    );
    let (last_version, last) = after.last().expect("a publish");
    assert_eq!(*last_version, Some(3), "{after:?}");
    assert!(last.is_empty(), "the fixed text is clean: {after:?}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The start of the first diagnostic with `code` in one publish.
fn start_of(diagnostics: &[Value], code: &str) -> Option<(u64, u64)> {
    diagnostics.iter().find(|d| d["code"] == json!(code)).map(|d| {
        (
            d["range"]["start"]["line"].as_u64().unwrap_or(0),
            d["range"]["start"]["character"].as_u64().unwrap_or(0),
        )
    })
}

/// Review 78, item 8a. A keystroke's publish carries the emitter's last
/// refusals forward, mapped through the edit, so they neither vanish while a
/// person types nor stay at a position the text has moved away from.
///
/// A comment line is typed above an E0302: the keystroke's own publish, the
/// one before any emitter run, has the E0302 one line lower. Then the E0302
/// is fixed inside the debounce window. The fix's edit replaces the refused
/// expression, so the carried E0302 is dropped with it, and nothing after the
/// fix's first publish carries one.
#[test]
fn an_emitter_error_moves_with_a_line_typed_above_it() {
    let dir = tmp_dir("carry");
    let main_path = dir.join("main.glyph");
    std::fs::write(&main_path, E0302_PICK).expect("write main");
    let mut lsp = Lsp::start(&dir);
    lsp.open(&main_path);
    let opened = lsp.diagnostics_for(&main_path);
    let (line, character) =
        start_of(&opened, "E0302").unwrap_or_else(|| panic!("the open has no E0302: {opened:?}"));

    let above = "pub fn pick(";
    let typed = E0302_PICK.replacen(above, &format!("// a note\n{above}"), 1);
    lsp.change(&main_path, 2, &typed);
    let (version, first) = lsp.next_publish(&main_path);
    assert_eq!(version, Some(2));
    assert_eq!(
        start_of(&first, "E0302"),
        Some((line + 1, character)),
        "the keystroke's publish did not carry the E0302 one line down: {first:?}"
    );

    let fixed = FIXED_PICK.replacen(above, &format!("// a note\n{above}"), 1);
    lsp.change(&main_path, 3, &fixed);
    let after = lsp.publishes_until_quiet(&main_path);
    let fixed_at = after
        .iter()
        .position(|(v, _)| *v == Some(3))
        .unwrap_or_else(|| panic!("the fix was never published: {after:?}"));
    for (version, diagnostics) in &after[fixed_at..] {
        assert!(
            !codes_of(diagnostics).iter().any(|c| c == "E0302"),
            "a stale E0302 (version {version:?}) landed after the fix: {after:?}"
        );
    }
    let (last_version, last) = after.last().expect("a publish");
    assert_eq!(*last_version, Some(3), "{after:?}");
    assert!(last.is_empty(), "the fixed text is clean: {after:?}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `main` reads `Order.parse`, which is E0304 when `orders` gives `Order` an
/// opaque field and clean when every field is plain: the verdict is the
/// declaring module's (G210), so it changes with a file `main` does not hold.
const MAIN_PARSES_ORDER: &str = "module main\nimport orders { Order }\n\n\
    pub fn take(v: unknown) -> bool {\n  return match Order.parse(v) {\n    \
    Ok(_) => true,\n    Err(_) => false,\n  }\n}\n";
const ORDERS_PLAIN: &str = "module orders\npub type Order = {\n  id: string,\n}\n";
const ORDERS_OPAQUE: &str = "module orders\npub type Handle = extern_ts(\"{ fd: number }\")\n\
    pub type Order = {\n  id: string,\n  handle: Handle,\n}\n";

/// Review 78, item 8d. An unsaved edit in a sibling republishes an open file
/// whose diagnostics depend on it. `main` is open and clean; `orders` is
/// opened and edited, never saved, so that `main`'s `Order.parse` becomes
/// E0304; and `main`, untouched, is republished with it.
#[test]
fn a_sibling_edit_republishes_an_open_file_that_depends_on_it() {
    let dir = tmp_dir("sibling");
    let main_path = dir.join("main.glyph");
    let orders_path = dir.join("orders.glyph");
    std::fs::write(&main_path, MAIN_PARSES_ORDER).expect("write main");
    std::fs::write(&orders_path, ORDERS_PLAIN).expect("write orders");
    let mut lsp = Lsp::start(&dir);
    lsp.open(&main_path);
    assert!(lsp.diagnostics_for(&main_path).is_empty(), "main opens clean");
    lsp.open(&orders_path);
    lsp.change(&orders_path, 2, ORDERS_OPAQUE);
    let after = lsp.publishes_until_quiet(&main_path);
    let (_, last) = after.last().expect("a publish");
    assert_eq!(codes_of(last), ["E0304"], "main was not republished with E0304: {after:?}");

    // And back: the sibling's fix clears it.
    lsp.change(&orders_path, 3, ORDERS_PLAIN);
    let after = lsp.publishes_until_quiet(&main_path);
    assert!(after.last().expect("a publish").1.is_empty(), "{after:?}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Review 78, item 8d, from disk: a sibling the editor does not have open
/// changes behind it (a branch switch, a generator), and the watched-file
/// notification republishes the open file that depends on it.
#[test]
fn a_sibling_changed_on_disk_republishes_an_open_file() {
    let dir = tmp_dir("sibling_disk");
    let main_path = dir.join("main.glyph");
    let orders_path = dir.join("orders.glyph");
    std::fs::write(&main_path, MAIN_PARSES_ORDER).expect("write main");
    std::fs::write(&orders_path, ORDERS_PLAIN).expect("write orders");
    let mut lsp = Lsp::start(&dir);
    lsp.open(&main_path);
    assert!(lsp.diagnostics_for(&main_path).is_empty(), "main opens clean");
    std::fs::write(&orders_path, ORDERS_OPAQUE).expect("rewrite orders");
    lsp.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&orders_path), "type": 2 }] }),
    );
    let after = lsp.publishes_until_quiet(&main_path);
    let (_, last) = after.last().expect("a publish");
    assert_eq!(codes_of(last), ["E0304"], "main was not republished with E0304: {after:?}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

impl Lsp {
    /// `glyph/applyEdit` replacing `file`'s whole buffer, `old`, with `new`.
    fn apply_whole(&mut self, file: &Path, old: &str, new: &str) -> Value {
        let lines: Vec<&str> = old.split('\n').collect();
        let last = lines.len() - 1;
        self.request(
            "glyph/applyEdit",
            json!({
                "uri": uri(file),
                "edits": [{
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": last, "character": lines[last].len() },
                    },
                    "newText": new,
                }],
            }),
        )
    }
}

/// G306. `glyph/applyEdit` refuses what `glyph check` refuses: the gate
/// checks the candidate inside its project and through the emitter, with
/// the list the editor publishes, where it checked the text alone and
/// stopped after the checker. An emitter refusal (E0302), an import of a
/// name the sibling does not export (E0105) and a wrong argument count
/// against an imported function (E0213) were each accepted. A rejected edit
/// leaves the buffer as it was, so a clean edit after it is checked against
/// the original text.
#[test]
fn apply_edit_refuses_what_glyph_check_refuses() {
    let dir = tmp_dir("apply_edit");
    let main_path = dir.join("main.glyph");
    let lib = "module lib\n\npub fn hello() -> string {\n  return \"hi\"\n}\n";
    let uses = |call: &str, name: &str| {
        format!(
            "module main\nimport lib {{ {name} }}\n\npub fn run() -> string {{\n  return {call}\n}}\n\n{}",
            CLEAN_PICK.trim_start_matches("module main\n")
        )
    };
    let clean = uses("hello()", "hello");
    std::fs::write(&main_path, &clean).expect("write main");
    std::fs::write(dir.join("lib.glyph"), lib).expect("write lib");
    let e0302 = uses("hello()", "hello").replace("    None => r,\n    Some(n) => b,", "    None => f(b)?,\n    Some(n) => r,");
    let cases = [
        ("E0302", e0302),
        ("E0105", uses("goodbye()", "goodbye")),
        ("E0213", uses("hello(1)", "hello")),
    ];

    let mut lsp = Lsp::start(&dir);
    lsp.open(&main_path);
    for (code, candidate) in &cases {
        let broken = tmp_dir("apply_edit_cli");
        std::fs::write(broken.join("main.glyph"), candidate).expect("write candidate");
        std::fs::write(broken.join("lib.glyph"), lib).expect("write lib");
        assert!(
            check_codes(&broken).iter().any(|(c, ..)| c == code),
            "the candidate no longer draws {code} from `glyph check`"
        );
        let _ = std::fs::remove_dir_all(&broken);

        let answer = lsp.apply_whole(&main_path, &clean, candidate);
        assert_eq!(answer["ok"], json!(false), "{code}: the gate accepted it: {answer}");
        assert_eq!(answer["rejected"], json!("verification_failed"), "{code}: {answer}");
        let codes = codes_of(answer["diagnostics"].as_array().expect("diagnostics"));
        assert!(codes.iter().any(|c| c == code), "{code}: rejected for {codes:?}");
    }

    // A point edit at the end of `return hello()`, so a buffer still holding
    // a rejected candidate would answer with that candidate's text.
    let edited = clean.replace("hello()", "hello() + \"!\"");
    let line = clean.lines().position(|l| l == "  return hello()").expect("the call line");
    let answer = lsp.request(
        "glyph/applyEdit",
        json!({
            "uri": uri(&main_path),
            "edits": [{
                "range": {
                    "start": { "line": line, "character": 16 },
                    "end": { "line": line, "character": 16 },
                },
                "newText": " + \"!\"",
            }],
        }),
    );
    assert_eq!(answer["ok"], json!(true), "a clean edit was refused: {answer}");
    assert_eq!(answer["content"], json!(edited));
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The zero-based LSP position of byte `offset` in ASCII `text`.
fn position_of(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let character = offset - before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    json!({ "line": line, "character": character })
}

/// A `TextEdit` replacing the first `old` in `text` with `new`.
fn replace_first(text: &str, old: &str, new: &str) -> Value {
    let start = text.find(old).unwrap_or_else(|| panic!("`{old}` is not in the text"));
    json!({
        "range": { "start": position_of(text, start), "end": position_of(text, start + old.len()) },
        "newText": new,
    })
}

impl Lsp {
    /// `glyph/applyEdit` with `edits` on `file`.
    fn apply(&mut self, file: &Path, edits: Value) -> Value {
        self.request("glyph/applyEdit", json!({ "uri": uri(file), "edits": edits }))
    }
}

/// The codes under `field` of an applyEdit answer.
fn answer_codes(answer: &Value, field: &str) -> Vec<String> {
    let list = answer[field]
        .as_array()
        .unwrap_or_else(|| panic!("no `{field}` list in {answer}"));
    codes_of(list)
}

/// A project of `files`, a server on it, and `open` opened.
fn gate_project(prefix: &str, files: &[(&str, &str)], open: &str) -> (PathBuf, PathBuf, Lsp) {
    let dir = tmp_dir(prefix);
    for (name, text) in files {
        std::fs::write(dir.join(name), text).expect("write a fixture");
    }
    let path = dir.join(open);
    let mut lsp = Lsp::start(&dir);
    lsp.open(&path);
    (dir, path, lsp)
}

/// Review 81, decision 2: the gate holds an edit to what it introduced. A
/// module that draws E0112 (nothing in it is reachable) used to refuse
/// every edit to it, a comment typo included, because the gate demanded an
/// empty list. The warning was there before the edit, so it is returned
/// under `preexisting` and does not refuse it.
#[test]
fn apply_edit_accepts_an_edit_to_a_module_that_draws_e0112() {
    let util = "module util\n\n// a helper nobody calls yet\nfn helper() -> number {\n  return 1\n}\n";
    let (dir, path, mut lsp) = gate_project("gate_e0112", &[("util.glyph", util)], "util.glyph");
    let answer = lsp.apply(&path, json!([replace_first(util, "nobody", "no one")]));
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "diagnostics"), Vec::<String>::new(), "{answer}");
    assert!(answer_codes(&answer, "preexisting").contains(&"E0112".to_string()), "{answer}");
    assert_eq!(answer["content"], json!(util.replace("nobody", "no one")));
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The sibling the fixtures below import `hello` from.
const GATE_LIB: &str = "module lib\n\npub fn hello() -> string {\n  return \"hi\"\n}\n";

/// The warning tier counts: an edit that adds an unused binding is refused
/// with E0107, and the file had nothing before, so `preexisting` is empty.
#[test]
fn apply_edit_refuses_an_edit_that_adds_an_unused_let() {
    let main = "module main\nimport lib { hello }\n\npub fn run() -> string {\n  return hello()\n}\n";
    let (dir, path, mut lsp) = gate_project(
        "gate_e0107",
        &[("main.glyph", main), ("lib.glyph", GATE_LIB)],
        "main.glyph",
    );
    let answer = lsp.apply(&path, json!([replace_first(main, "  return", "  let x = 1\n  return")]));
    assert_eq!(answer["ok"], json!(false), "accepted: {answer}");
    assert_eq!(answer["rejected"], json!("verification_failed"));
    assert_eq!(answer_codes(&answer, "diagnostics"), ["E0107"], "{answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), Vec::<String>::new(), "{answer}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two errors, one call each, both wrong argument counts against `hello`.
const TWO_ERRORS: &str = "module main\nimport lib { hello }\n\n\
    pub fn one() -> string {\n  return hello(1)\n}\n\n\
    pub fn two() -> string {\n  return hello(2)\n}\n";

/// Fixing one of two errors is accepted, and the other comes back under
/// `preexisting` at its new position. The old gate refused it, so an agent
/// had to repair a whole file in one edit.
#[test]
fn apply_edit_accepts_fixing_one_of_two_errors() {
    let (dir, path, mut lsp) = gate_project(
        "gate_one_of_two",
        &[("main.glyph", TWO_ERRORS), ("lib.glyph", GATE_LIB)],
        "main.glyph",
    );
    let answer = lsp.apply(&path, json!([replace_first(TWO_ERRORS, "hello(1)", "hello()")]));
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "diagnostics"), Vec::<String>::new(), "{answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), ["E0213"], "{answer}");
    let fixed = TWO_ERRORS.replacen("hello(1)", "hello()", 1);
    let at = fixed.find("hello(2)").expect("the second call");
    let range = &answer["preexisting"][0]["range"];
    assert_eq!(range["start"]["line"], position_of(&fixed, at)["line"], "{answer}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A whole-buffer replacement that changes one line is one edited line, not
/// an edited file: the error two functions away keeps its place and stays
/// `preexisting`, so the edit is accepted.
#[test]
fn apply_edit_reads_a_whole_buffer_replacement_as_the_lines_it_changed() {
    let (dir, path, mut lsp) = gate_project(
        "gate_whole_buffer",
        &[("main.glyph", TWO_ERRORS), ("lib.glyph", GATE_LIB)],
        "main.glyph",
    );
    let candidate = TWO_ERRORS.replacen("hello(1)", "hello()", 1);
    let answer = lsp.apply_whole(&path, TWO_ERRORS, &candidate);
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), ["E0213"], "{answer}");
    assert_eq!(answer["content"], json!(candidate));
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A parse error hides every later stage. Fixing it exposes the type error
/// behind it, which nothing before could have shown, and the edit is
/// refused with that error: one rule, no exemption for hidden stages.
#[test]
fn apply_edit_refuses_a_parse_fix_that_exposes_a_type_error() {
    let broken = "module main\nimport lib { hello }\n\n\
        pub fn one() -> string {\n  return hello(\n}\n\n\
        pub fn two() -> string {\n  return hello(2)\n}\n";
    let (dir, path, mut lsp) = gate_project(
        "gate_parse_fix",
        &[("main.glyph", broken), ("lib.glyph", GATE_LIB)],
        "main.glyph",
    );
    let answer = lsp.apply(&path, json!([replace_first(broken, "hello(\n", "hello()\n")]));
    assert_eq!(answer["ok"], json!(false), "accepted: {answer}");
    assert_eq!(answer_codes(&answer, "diagnostics"), ["E0213"], "{answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), Vec::<String>::new(), "{answer}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Adding a variant to a union a `match` in the same file already misses
/// one of changes that E0200's message (it now misses two), so the
/// diagnostic is the edit's own and the edit is refused.
#[test]
fn apply_edit_refuses_a_variant_that_changes_an_e0200() {
    let main = "module main\n\npub type Color =\n  | Red\n  | Green\n\n\
        pub fn name(c: Color) -> string {\n  return match c {\n    Red => \"red\",\n  }\n}\n";
    let (dir, path, mut lsp) = gate_project("gate_e0200", &[("main.glyph", main)], "main.glyph");
    let answer = lsp.apply(&path, json!([replace_first(main, "  | Green\n", "  | Green\n  | Blue\n")]));
    assert_eq!(answer["ok"], json!(false), "accepted: {answer}");
    assert_eq!(answer_codes(&answer, "diagnostics"), ["E0200"], "{answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), Vec::<String>::new(), "{answer}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Review 82, item 3(b). E0200's range is the whole `match`, so an edit to
/// one of its arms falls inside it. The diagnostic encloses the edit rather
/// than sitting in the text the edit wrote: it keeps its start, its end
/// moves by the edit's delta, and an unchanged message makes it
/// `preexisting`, so the edit is accepted. The variant test above still
/// holds, because there the message changes.
#[test]
fn apply_edit_accepts_an_edit_inside_an_e0200_it_leaves_unchanged() {
    let main = "module main\n\npub type S =\n  | A\n  | B\n  | C\n\n\
        pub fn f(s: S) -> number {\n  return match s {\n    A => 1,\n    B => 2,\n  }\n}\n";
    let (dir, path, mut lsp) = gate_project("gate_enclosing", &[("main.glyph", main)], "main.glyph");
    let answer = lsp.apply(&path, json!([replace_first(main, "A => 1", "A => 10")]));
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "diagnostics"), Vec::<String>::new(), "{answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), ["E0200"], "{answer}");
    assert_eq!(answer["content"], json!(main.replace("A => 1", "A => 10")));
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The one message that embeds a source position is the lexer's (`lex
/// error: ... at offset N`). An edit above it moves the offset, and the
/// gate maps the number through the edit the way it maps the range, so the
/// lex error stays `preexisting` and the edit is accepted.
#[test]
fn apply_edit_maps_the_offset_inside_a_lex_error() {
    let main = "module main\n\n// note\npub fn run() -> string {\n  return \"open\n}\n";
    let (dir, path, mut lsp) = gate_project("gate_lex", &[("main.glyph", main)], "main.glyph");
    let answer = lsp.apply(&path, json!([replace_first(main, "// note", "// a longer note")]));
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), ["E0001"], "{answer}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Several `TextEdit`s are several regions. A diagnostic between two of
/// them shifts by the first one's delta only and stays `preexisting`; a
/// diagnostic one of them touches is dropped, so the error the edit leaves
/// in text it wrote is the edit's own. A diagnostic that strictly encloses
/// one (`hello(2)` → `hello(3)` edits only the `2`) keeps its start and is
/// matched on its message (review 82, item 3(b)): the same wrong count is
/// the same error, so it stays `preexisting`.
#[test]
fn apply_edit_maps_through_each_text_edit() {
    let (dir, path, mut lsp) = gate_project(
        "gate_multi",
        &[("main.glyph", TWO_ERRORS), ("lib.glyph", GATE_LIB)],
        "main.glyph",
    );
    let fix_one = json!([
        replace_first(TWO_ERRORS, "pub fn one", "// first\npub fn one"),
        replace_first(TWO_ERRORS, "hello(1)", "hello()"),
    ]);
    let answer = lsp.apply(&path, fix_one);
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), ["E0213"], "{answer}");

    let rewrite_inside_two = json!([
        replace_first(TWO_ERRORS, "hello(1)", "hello()"),
        replace_first(TWO_ERRORS, "hello(2)", "hello(3)"),
    ]);
    let answer = lsp.apply(&path, rewrite_inside_two);
    assert_eq!(answer["ok"], json!(true), "refused: {answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), ["E0213"], "{answer}");

    let rewrite_two = json!([
        replace_first(TWO_ERRORS, "hello(1)", "hello()"),
        replace_first(TWO_ERRORS, "return hello(2)", "return (hello(2))"),
    ]);
    let answer = lsp.apply(&path, rewrite_two);
    assert_eq!(answer["ok"], json!(false), "accepted: {answer}");
    assert_eq!(answer_codes(&answer, "diagnostics"), ["E0213"], "{answer}");
    assert_eq!(answer_codes(&answer, "preexisting"), Vec::<String>::new(), "{answer}");
    drop(lsp);
    let _ = std::fs::remove_dir_all(&dir);
}
