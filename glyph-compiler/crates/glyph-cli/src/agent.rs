//! `glyph check --agent`: the same diagnostics, plus what an agent needs to
//! make the next edit without asking a second question.
//!
//! Two additions per diagnostic.
//!
//! `constraints` are the things an edit repairing this diagnostic must keep.
//! They are not advice and they are not a repair: each one is an invariant the
//! compiler already enforces, written out so that the edit which satisfies the
//! diagnostic does not quietly break the guarantee the diagnostic existed to
//! protect. The two failure modes this exists for are both real: an `E0200` is
//! silenced with an `else` arm, which forfeits the sealed-union guarantee (D9)
//! and makes the next variant's addition compile everywhere; and an `E0204` is
//! silenced with a cast, which moves a type error to runtime. A code the
//! compiler holds no such invariant for gets an empty list rather than a
//! sentence invented to fill it.
//!
//! `symbols` is the `glyph_symbol` description of every symbol the diagnostic
//! names: the declaration at fault (`cause`), the declaration the diagnostic
//! sits in (`entity`), and the types in `expected`/`actual` when they name a
//! declaration rather than a structure or a primitive. The descriptions come
//! from `glyph_lsp::call_mcp_tool`, which is the same `call_tool` an MCP client
//! reaches and `glyph query symbol` prints, so this surface cannot describe a
//! union differently from the tool that describes unions. A symbol the tool
//! refuses or cannot key is listed in `symbols_absent` with the tool's own
//! reason, because "we did not look" and "we looked and it is not there" are
//! the two answers an agent must never confuse.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::diagnostic::Diagnostic;

/// The primitive and ambient type names that name no declaration, so asking
/// `glyph_symbol` about them would be asking about a spelling rather than a
/// symbol.
const NOT_A_DECLARATION: [&str; 10] = [
    "string", "number", "int", "bigint", "bool", "void", "unknown", "never", "any", "Component",
];

/// Render every diagnostic as the `--json` object plus `constraints`,
/// `symbols` and `symbols_absent`.
pub fn enrich(diagnostics: &[Diagnostic], project_srcs: &[PathBuf]) -> Vec<Value> {
    let mut cache: HashMap<(PathBuf, String), Result<Value, String>> = HashMap::new();
    diagnostics
        .iter()
        .map(|d| enrich_one(d, project_srcs, &mut cache))
        .collect()
}

fn enrich_one(
    d: &Diagnostic,
    project_srcs: &[PathBuf],
    cache: &mut HashMap<(PathBuf, String), Result<Value, String>>,
) -> Value {
    let mut value = serde_json::to_value(d).unwrap_or_else(|_| json!({}));
    let Some(object) = value.as_object_mut() else {
        return value;
    };

    let mut symbols: Vec<Value> = Vec::new();
    let mut absent: Vec<Value> = Vec::new();
    match locate(d, project_srcs) {
        None => {
            for request in symbol_keys(d) {
                absent.push(json!({
                    "symbol": request.key,
                    "reason": "this diagnostic is not about a file in any project this check \
                               covered, so there is no project to resolve the symbol in",
                }));
            }
        }
        Some((root, file)) => {
            let mut described: Vec<String> = Vec::new();
            for request in symbol_keys(d) {
                let SymbolRequest { key, bare_name } = request;
                if described.contains(&key) {
                    continue;
                }
                let entry = cached(cache, &root, &file, &key);
                match entry {
                    Ok(answer) => {
                        described.push(key);
                        symbols.push(answer);
                    }
                    Err(reason) => {
                        // A type spelled bare in a diagnostic is in scope in
                        // the module the diagnostic is in, and that module may
                        // have it by import rather than by declaration, which
                        // `glyph_symbol` refuses: a symbol is described where
                        // it is declared. The project's own symbol index is
                        // asked for declarations of that name, and when it
                        // holds exactly one the answer is not a guess. More
                        // than one, or none, and the refusal stands.
                        let resolved = bare_name
                            .as_deref()
                            .and_then(|name| sole_declaration(&root, name));
                        match resolved {
                            Some(entity) if !described.contains(&entity) => {
                                match cached(cache, &root, &file, &entity) {
                                    Ok(answer) => {
                                        described.push(entity);
                                        symbols.push(answer);
                                    }
                                    Err(why) => {
                                        absent.push(json!({ "symbol": key, "reason": why }))
                                    }
                                }
                            }
                            Some(_) => {}
                            None => absent.push(json!({ "symbol": key, "reason": reason })),
                        }
                    }
                }
            }
        }
    }
    // After the symbols, because one constraint is written from them: the
    // values a union accepts are the union's own `construct` spellings, and
    // `glyph_symbol` is what holds those. A constraint the compiler cannot
    // state from an answer in hand is not written at all.
    object.insert("constraints".to_string(), json!(constraints(d, &symbols)));
    object.insert("symbols".to_string(), json!(symbols));
    object.insert("symbols_absent".to_string(), json!(absent));
    value
}

/// The repair constraints for one diagnostic, stated only where the compiler
/// holds them.
fn constraints(d: &Diagnostic, symbols: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    match d.code.as_str() {
        "E0200" => {
            let union = d
                .union
                .as_ref()
                .map(|u| u.name.clone())
                .unwrap_or_else(|| "the scrutinee's type".to_string());
            match d.missing_variants.as_deref() {
                Some(missing) if !missing.is_empty() => out.push(format!(
                    "preserve exhaustiveness: this match must name every case of `{union}`, so \
                     add one arm for each of {}.",
                    list(missing)
                )),
                _ => out.push(format!(
                    "preserve exhaustiveness: this match must name every case of `{union}`."
                )),
            }
            // The `else` constraint holds only for a union some module
            // declares. A prelude union is not one this project can add a
            // variant to, so the guarantee an `else` would forfeit is not one
            // this project owns, and claiming it would be claiming more than
            // the compiler knows.
            if d.union.as_ref().is_some_and(|u| u.kind == "declaration") {
                out.push(format!(
                    "do not add an `else` arm: `{union}` is declared in this project, and a \
                     catch-all forfeits the guarantee that adding a variant to it later forces \
                     this match to be updated (D9)."
                ));
            }
        }
        "E0204" | "E0211" => {
            match d.expected.as_deref() {
                Some(expected) => out.push(format!(
                    "do not cast: the declared type is the contract. This position requires \
                     `{expected}`, so produce a value of that type or change the declaration \
                     that requires it."
                )),
                None => out.push(
                    "do not cast: the declared type is the contract. Produce a value of the \
                     declared type, or change the declaration."
                        .to_string(),
                ),
            }
            if let Some(sentence) = accepted_values(d, symbols) {
                out.push(sentence);
            }
        }
        "E0205" => out.push(
            "`owned` applies only to a type declared `resource` (D25); it is not a general \
             modifier."
                .to_string(),
        ),
        "E0206" => out.push(
            "keep the consume: an `owned` resource is consumed exactly once on every path out \
             of its scope (D25), so every path that leaves without consuming needs its own \
             consume."
                .to_string(),
        ),
        "E0207" => out.push(
            "keep the consume: this handle is already consumed on this path, and an `owned` \
             resource is consumed exactly once (D25), so it cannot be used again after that."
                .to_string(),
        ),
        "E0215" => out.push(
            "keep the consume: an `owned` handle has exactly one binding and cannot be aliased \
             (D25), because a second name is a second place the consume could have to happen."
                .to_string(),
        ),
        "E0210" => {
            let record = d
                .cause
                .clone()
                .unwrap_or_else(|| "the record".to_string());
            match d.alternatives.as_deref().filter(|a| !a.is_empty()) {
                Some(fields) => out.push(format!(
                    "the record's fields are closed: `{record}` declares {} and no others, so \
                     read one of them or add the field to the declaration.",
                    list(fields)
                )),
                None => out.push(format!(
                    "the record's fields are closed: read a field `{record}` declares, or add \
                     the field to the declaration."
                )),
            }
        }
        _ => {}
    }
    out
}

/// One symbol to describe: the key to ask under, and the bare type name it came
/// from when the key was assembled from a type spelling rather than read off
/// the diagnostic.
struct SymbolRequest {
    key: String,
    bare_name: Option<String>,
}

/// Every symbol this diagnostic names, in a stable order and without repeats.
fn symbol_keys(d: &Diagnostic) -> Vec<SymbolRequest> {
    let mut out: Vec<SymbolRequest> = Vec::new();
    let mut push = |key: String, bare_name: Option<String>| {
        if !out.iter().any(|r| r.key == key) {
            out.push(SymbolRequest { key, bare_name });
        }
    };
    if let Some(cause) = &d.cause {
        push(cause.clone(), None);
    }
    if let Some(union) = d.union.as_ref().and_then(|u| u.declaration.clone()) {
        push(union, None);
    }
    if let Some(entity) = &d.entity {
        push(entity.clone(), None);
    }
    for ty in [d.expected.as_deref(), d.actual.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Some(key) = type_as_entity(ty, d.module.as_deref()) {
            push(key, Some(ty.trim().to_string()));
        }
    }
    out
}

/// `glyph_symbol` for `key`, answered once per (project, key) per run.
fn cached(
    cache: &mut HashMap<(PathBuf, String), Result<Value, String>>,
    root: &Path,
    file: &Path,
    key: &str,
) -> Result<Value, String> {
    cache
        .entry((root.to_path_buf(), key.to_string()))
        .or_insert_with(|| describe(root, file, key))
        .clone()
}

/// The one declaration in this project named `name`, or `None` when there are
/// none or several.
///
/// `glyph_symbols` is the project's own symbol index, so this is the compiler's
/// answer to "where is `Sheet` declared" rather than a second walk of the
/// import edges here. Several declarations of one name is not a question the
/// index settles, and the caller reports the refusal it already had.
fn sole_declaration(root: &Path, name: &str) -> Option<String> {
    let answer = glyph_lsp::call_mcp_tool(
        root.to_path_buf(),
        "glyph_symbols",
        json!({ "query": name }),
    )
    .ok()?;
    let found: Value = serde_json::from_str(&answer).ok()?;
    let mut exact = found
        .as_array()?
        .iter()
        .filter(|s| s.get("name").and_then(|n| n.as_str()) == Some(name))
        .filter_map(|s| s.get("entity").and_then(|e| e.as_str()).map(str::to_string));
    let first = exact.next()?;
    match exact.next() {
        Some(_) => None,
        None => Some(first),
    }
}

/// The `module::name` a rendered type names, when it names a declaration at
/// all.
///
/// A structural type (`{ id: string }`), an application (`Array<int>`), a
/// function type and a primitive all describe a shape rather than address a
/// declaration, and there is nothing for `glyph_symbol` to be asked about. A
/// bare name is qualified with the module the diagnostic is in, which is where
/// that spelling is in scope; a name the diagnostic's module does not declare
/// comes back as a refusal and is reported as one.
fn type_as_entity(ty: &str, module: Option<&str>) -> Option<String> {
    let name = ty.trim();
    if name.is_empty() || NOT_A_DECLARATION.contains(&name) {
        return None;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return None,
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(format!("{}::{name}", module?))
}

/// The project root a diagnostic's file belongs to, and the file itself, when
/// exactly one root holds it.
fn locate(d: &Diagnostic, project_srcs: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let mut found: Option<(PathBuf, PathBuf)> = None;
    for root in project_srcs {
        let candidate = root.join(&d.file);
        if !candidate.exists() {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some((root.clone(), candidate));
    }
    found
}

/// Ask `glyph_symbol` about one `module::name`.
fn describe(root: &Path, file: &Path, entity: &str) -> Result<Value, String> {
    let path = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let answer = glyph_lsp::call_mcp_tool(
        root.to_path_buf(),
        "glyph_symbol",
        json!({ "entity": entity, "path": path.to_string_lossy() }),
    )?;
    serde_json::from_str(&answer)
        .map_err(|e| format!("`glyph_symbol` answered for `{entity}`, and it did not parse: {e}"))
}

/// What a value at this position may be, when the compiler holds a form to
/// write one in. `None` when it does not, and then no sentence is written.
///
/// `alternatives` on an `E0204` or an `E0211` is a bare name list, and a bare
/// name list is not a list of values. For a tagged union it is the variant
/// names, and a variant with a payload is a constructor rather than a value:
/// the compiler told an agent "the accepted values here are `Red`, `Green`",
/// the agent wrote `takesColor(Green)`, and `tsc` answered `TS2345: Argument
/// of type '(fields: { hex: string; }) => Color' is not assignable to
/// parameter of type 'Color'`. For a string-literal union it is the literals
/// with their quotes stripped, so `read` and `write` rendered in backticks are
/// identifiers and the values are `"read"` and `"write"`. One field carried
/// both shapes and the sentence flattened them into one.
///
/// The form comes from `glyph_symbol`'s answer for the declared type, which
/// this diagnostic already fetched: `construct` for a variant, the literal for
/// a member of a string-literal union. Those are the compiler's own spellings,
/// so this cannot describe a union differently from the tool that describes
/// unions. A declared type with no answer here (an inline union, a type the
/// tool refused) gets no sentence, which is the same bar every other
/// constraint is held to.
fn accepted_values(d: &Diagnostic, symbols: &[Value]) -> Option<String> {
    let alternatives = d.alternatives.as_deref().filter(|a| !a.is_empty())?;
    let expected = d.expected.as_deref()?.trim();
    let entity = type_as_entity(expected, d.module.as_deref());
    let answer = symbols.iter().find(|s| {
        let named = |k: &str| s.get(k).and_then(|v| v.as_str());
        Some(expected) == named("name") || (entity.is_some() && entity.as_deref() == named("entity"))
    })?;

    if let Some(literals) = answer.get("literals").and_then(|v| v.as_array()) {
        let known: Vec<&str> = literals.iter().filter_map(|l| l.as_str()).collect();
        if !alternatives.iter().all(|a| known.contains(&a.as_str())) {
            return None;
        }
        let written: Vec<String> = alternatives.iter().map(|a| format!("\"{a}\"")).collect();
        return Some(format!(
            "the values this position accepts are {}.",
            list(&written)
        ));
    }

    let variants = answer.get("variants").and_then(|v| v.as_array())?;
    let mut written: Vec<String> = Vec::new();
    for name in alternatives {
        let variant = variants
            .iter()
            .find(|v| v.get("name").and_then(|n| n.as_str()) == Some(name.as_str()))?;
        written.push(variant.get("construct").and_then(|c| c.as_str())?.to_string());
    }
    Some(format!(
        "the values this position accepts are the cases of `{expected}`, each written as {}.",
        list(&written)
    ))
}

/// `` `a` ``, `` `b` `` for a sentence that enumerates names.
fn list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
