//! A minimal Model Context Protocol (MCP) server exposing Glyph's language
//! analysis to a coding agent as tools. It speaks JSON-RPC 2.0 over stdio with
//! newline-delimited messages (the MCP stdio transport), and reuses the pure
//! `crate::analysis` queries — hover, go-to-definition, workspace references,
//! symbol search, and diagnostics — so the agent surface is a thin adapter over
//! the same semantics the editor path uses, not a second implementation.
//!
//! Positions are LSP-style: 0-based `line` and a 0-based UTF-16 `character`.
//! Paths in tool arguments are relative to the project root (or absolute); paths
//! in results are reported relative to the root when possible.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::analysis::{
    analyze, analyze_full, outline_of, Analysis, Definition, LineIndex, OutlineKind, OutlineSymbol,
    SymbolTarget,
};
use crate::{collect_glyph_files, module_path_of};

/// The MCP protocol revision this server implements.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the MCP server over stdio until stdin closes. `root` is the project root
/// used for workspace queries (references, symbols) and to resolve relative file
/// paths in tool arguments.
pub fn run_stdio(root: PathBuf) {
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
        if let Some(resp) = handle(&req, &root) {
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
fn handle(req: &Value, root: &Path) -> Option<Value> {
    let method = req.get("method")?.as_str()?;
    let id = req.get("id").cloned();
    match method {
        "initialize" => Some(ok(id?, initialize_result())),
        "tools/list" => Some(ok(id?, json!({ "tools": tool_specs() }))),
        "tools/call" => {
            let id = id?;
            let params = req.get("params").cloned().unwrap_or(Value::Null);
            let (text, is_error) = match call_tool(&params, root) {
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
            "description": "Every reference to the symbol at a position across the whole project — the declaration, all uses, and each importing module's import binding. A local binding is file-scoped.",
            "inputSchema": { "type": "object", "properties": { "path": file, "line": line, "character": character }, "required": ["path", "line", "character"] }
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
fn call_tool(params: &Value, root: &Path) -> Result<String, String> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("missing tool `name`")?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    match name {
        "glyph_diagnostics" => tool_diagnostics(&args, root),
        "glyph_hover" => tool_hover(&args, root),
        "glyph_definition" => tool_definition(&args, root),
        "glyph_references" => tool_references(&args, root),
        "glyph_symbols" => tool_symbols(&args, root),
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
            location_value(&path, root, &text, &index, start, start)
        }
        Some(Definition::InModule { module_path, name }) => {
            let file = root.join(&module_path).with_extension("glyph");
            let ftext = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            let (start, end) = crate::analysis::find_symbol_span(&outline_of(&ftext), &name)
                .ok_or_else(|| format!("`{name}` is not defined in module `{module_path}`"))?;
            let index = LineIndex::new(&ftext);
            location_value(&file, root, &ftext, &index, start, end)
        }
    };
    Ok(to_json(&value))
}

fn tool_references(args: &Value, root: &Path) -> Result<String, String> {
    let (path, text) = read_file(args, root)?;
    let (line, character) = position(args)?;
    let Some(a) = analyze_full(&text) else {
        return Ok("[]".to_string());
    };
    let offset = LineIndex::new(&text).offset(&text, line, character);
    let this_module =
        module_path_of(root, &path).ok_or("the file is not under the project root")?;

    let mut out: Vec<Value> = Vec::new();
    match a.symbol_target(offset, &text, &this_module) {
        Some(SymbolTarget::Global { module, name }) => {
            for (fpath, ftext) in workspace_files(root) {
                let Some(fm) = module_path_of(root, &fpath) else {
                    continue;
                };
                let Some(fa) = analyze_full(&ftext) else {
                    continue;
                };
                push_occurrences(&mut out, &fpath, root, &ftext, &fa, &fm, &module, &name);
            }
        }
        Some(SymbolTarget::Local) => {
            let index = LineIndex::new(&text);
            for (s, e) in a.references(offset, &text, true) {
                out.push(location_value(&path, root, &text, &index, s, e));
            }
        }
        None => {}
    }
    Ok(to_json(&out))
}

fn tool_symbols(args: &Value, root: &Path) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let mut out: Vec<Value> = Vec::new();
    for (fpath, ftext) in workspace_files(root) {
        let index = LineIndex::new(&ftext);
        for top in outline_of(&ftext) {
            push_symbol(&mut out, &query, &fpath, root, &ftext, &index, &top, None);
            for child in &top.children {
                push_symbol(
                    &mut out,
                    &query,
                    &fpath,
                    root,
                    &ftext,
                    &index,
                    child,
                    Some(top.name.as_str()),
                );
            }
        }
    }
    Ok(to_json(&out))
}

/// Collect every global occurrence of `(sym_module, name)` in one analyzed file
/// into `out` as location values.
fn push_occurrences(
    out: &mut Vec<Value>,
    fpath: &Path,
    root: &Path,
    ftext: &str,
    fa: &Analysis,
    file_module: &str,
    sym_module: &str,
    name: &str,
) {
    let index = LineIndex::new(ftext);
    for (s, e) in fa.global_occurrences(file_module, sym_module, name, ftext, true) {
        out.push(location_value(fpath, root, ftext, &index, s, e));
    }
}

fn push_symbol(
    out: &mut Vec<Value>,
    query: &str,
    fpath: &Path,
    root: &Path,
    ftext: &str,
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
        "location": location_value(fpath, root, ftext, index, sym.span.0, sym.span.1),
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

fn read_file(args: &Value, root: &Path) -> Result<(PathBuf, String), String> {
    let raw = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing `path`")?;
    let path = {
        let p = Path::new(raw);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    };
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

fn location_value(
    file: &Path,
    root: &Path,
    text: &str,
    index: &LineIndex,
    start: u32,
    end: u32,
) -> Value {
    json!({
        "path": display_path(root, file),
        "range": range_json(index, text, start, end),
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

    /// Invoke a tool and return the parsed JSON of its text content.
    fn call(root: &Path, name: &str, args: Value) -> (Value, bool) {
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        let resp = handle(&req, root).expect("response");
        let result = &resp["result"];
        let is_error = result["isError"].as_bool().unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        (serde_json::from_str(text).unwrap_or(Value::Null), is_error)
    }

    #[test]
    fn initialize_and_tools_list() {
        let root = tmp_root();
        let init = handle(
            &json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {} }),
            &root,
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], "glyph-mcp");

        let list =
            handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }), &root).unwrap();
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
        let root = tmp_root();
        assert!(handle(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            &root
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
}
