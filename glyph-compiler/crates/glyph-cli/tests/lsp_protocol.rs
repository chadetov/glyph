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
