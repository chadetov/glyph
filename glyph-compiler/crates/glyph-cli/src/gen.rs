//! `glyph gen openapi` — turn an OpenAPI 3 / Swagger 2 / JSON Schema document
//! into real, committed Glyph `type` declarations.
//!
//! The point (0.1.3 Track B, Q40): we do not *infer* or *alias* an external
//! schema, we **generate** a first-class Glyph type for it. Every generated
//! `type X = { ... }` is greppable, lives in one place, and — because it is an
//! ordinary Glyph record — carries a D8 runtime descriptor, so a request or
//! response body validates through `X.parse(...)` for free. That is the whole
//! answer to "every API needs a hand-written DTO."
//!
//! ## Scope (MVP, the wire-correct 80%)
//!
//! Mapped faithfully: objects (`properties` + `required`) → records, primitives
//! (`string`/`integer`/`number`/`boolean`), `array` → `Array<T>`, `$ref` →
//! the referenced named type, `nullable`/non-required → `Option<T>` /
//! `field?: T`, `additionalProperties` → `Record<string, T>`, and object
//! `allOf` merged into one record.
//!
//! Deliberately *narrowed* (with a reported note, never silently): a `string`
//! `enum` becomes `string` — Glyph has no string-literal-union type, and mapping
//! it to a tagged union would tag by constructor name (`{tag:"Red"}`) and reject
//! the real wire value (`"red"`). `oneOf`/`anyOf` without a discriminator, and
//! any construct we cannot represent faithfully, become `unknown` with a note.
//! We would rather emit an honest `unknown` you can grep for than a validator
//! that lies about the wire.
//!
//! Output is assembled as Glyph source, then parsed and run through the
//! canonical formatter — so the file is always `glyph fmt`-clean, regeneration
//! is idempotent, and a mapping bug that produced unparseable source is caught
//! here rather than shipped.

use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not valid JSON or YAML: {msg}")]
    Parse { path: PathBuf, msg: String },
    #[error("{path} has no schemas: expected `components.schemas` (OpenAPI 3), `definitions` (Swagger 2), or a top-level object schema")]
    NoSchemas { path: PathBuf },
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A generated snippet failed to parse. This is a generator bug, not user
    /// error; we surface the offending source so it is diagnosable.
    #[error("internal: generated Glyph did not parse ({reason}).\n--- generated ---\n{source_text}")]
    GeneratedInvalid { reason: String, source_text: String },
    #[error("`node` not found on PATH. `glyph gen dts` needs Node.js to read TypeScript declarations; install it from https://nodejs.org.")]
    NodeMissing,
    #[error("the `typescript` package is not resolvable. Install it with `npm install -g typescript@6`, then re-run `glyph gen dts`.")]
    TypescriptMissing,
    #[error("the installed TypeScript is the native port (7.x), whose compiler API `glyph gen dts` does not yet support. Install the classic compiler with `npm install -g typescript@6` (or add `typescript@^6` to the project), then re-run.")]
    TypescriptUnsupported,
    #[error("reading the TypeScript declarations failed: {msg}")]
    Helper { msg: String },
}

/// Summary of one `glyph gen openapi` run.
#[derive(Debug)]
pub struct GenReport {
    pub out_file: PathBuf,
    pub type_count: usize,
    /// Constructs that could not be mapped faithfully and how they were
    /// narrowed. Reported to the user; never silent.
    pub notes: Vec<String>,
}

/// Generate Glyph types from an OpenAPI / JSON-Schema document at `spec_path`,
/// writing one `.glyph` file into `out_dir`.
pub fn openapi(spec_path: &Path, out_dir: &Path) -> Result<GenReport, GenError> {
    let raw = std::fs::read_to_string(spec_path).map_err(|e| GenError::Read {
        path: spec_path.to_path_buf(),
        source: e,
    })?;
    let doc = parse_doc(&raw).map_err(|msg| GenError::Parse {
        path: spec_path.to_path_buf(),
        msg,
    })?;

    let schemas = locate_schemas(&doc).ok_or_else(|| GenError::NoSchemas {
        path: spec_path.to_path_buf(),
    })?;

    let module_name = sanitize_module(stem_of(spec_path));
    let source_label = spec_path.display().to_string();
    let regen = format!("glyph gen openapi {}", spec_path.display());
    render_and_write(schemas, &module_name, &source_label, &regen, out_dir)
}

/// Turn a schema map into a formatted, self-validated `.glyph` file on disk.
/// Shared by `glyph gen openapi` and `glyph gen dts` — both reduce to a JSON
/// Schema `definitions` map, so both flow through the one mapper.
fn render_and_write(
    schemas: Vec<(String, Value)>,
    module_name: &str,
    source_label: &str,
    regen_cmd: &str,
    out_dir: &Path,
) -> Result<GenReport, GenError> {
    let mut gen = Generator::default();
    let body = gen.emit_module(module_name, source_label, regen_cmd, schemas);

    // Canonicalize + self-validate: parse the generated source and reprint it.
    let module = glyph_parser::parse(&body).map_err(|e| GenError::GeneratedInvalid {
        reason: format!("{e:?}"),
        source_text: body.clone(),
    })?;
    let comments = glyph_lexer::comments(&body);
    let formatted = glyph_formatter::format_module(&module, &comments, &body);

    std::fs::create_dir_all(out_dir).map_err(|e| GenError::Write {
        path: out_dir.to_path_buf(),
        source: e,
    })?;
    let out_file = out_dir.join(format!("{module_name}.glyph"));
    std::fs::write(&out_file, &formatted).map_err(|e| GenError::Write {
        path: out_file.clone(),
        source: e,
    })?;

    Ok(GenReport {
        out_file,
        type_count: gen.type_count,
        notes: gen.notes,
    })
}

fn stem_of(path: &Path) -> &str {
    // Strip a compound extension like `.d.ts` down to the base name.
    let mut stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("generated");
    if let Some(inner) = Path::new(stem).file_stem().and_then(|s| s.to_str()) {
        // `user.d` -> `user`
        stem = inner;
    }
    stem
}

/// The node helper that reads a `.d.ts` and prints a JSON Schema `definitions`
/// map. Bundled into the binary; written to a temp file at run time.
const TS_TO_SCHEMA: &str = include_str!("../../../runtime/tools/ts-to-schema.mjs");

/// Generate Glyph types from a TypeScript `.d.ts` declaration file.
///
/// Shells out to a bundled node helper that uses the TypeScript compiler to
/// read the declarations and emit JSON Schema, which then flows through the
/// exact same mapper as `glyph gen openapi`. `node` must be on PATH and the
/// `typescript` package must be resolvable (a global `npm install -g
/// typescript` is found via NODE_PATH).
pub fn dts(dts_path: &Path, out_dir: &Path) -> Result<GenReport, GenError> {
    if !dts_path.exists() {
        return Err(GenError::Read {
            path: dts_path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        });
    }

    // Write the helper next to a per-process temp name so concurrent runs do
    // not collide.
    let helper = std::env::temp_dir().join(format!("glyph-ts-to-schema-{}.mjs", std::process::id()));
    std::fs::write(&helper, TS_TO_SCHEMA).map_err(|e| GenError::Write {
        path: helper.clone(),
        source: e,
    })?;

    // Resolve global node_modules so `import "typescript"` works without a
    // local install. Best-effort: if `npm root -g` fails, node falls back to
    // its default resolution.
    let global_root = std::process::Command::new("npm")
        .args(["root", "-g"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let mut cmd = std::process::Command::new("node");
    cmd.arg(&helper).arg(dts_path);
    if let Some(root) = &global_root {
        cmd.env("NODE_PATH", root);
    }
    let output = cmd.output();
    let _ = std::fs::remove_file(&helper);

    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(GenError::NodeMissing);
        }
        Err(e) => {
            return Err(GenError::Helper {
                msg: format!("failed to run node: {e}"),
            })
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("GLYPH_GEN_NO_TYPESCRIPT") {
            return Err(GenError::TypescriptMissing);
        }
        if stderr.contains("GLYPH_GEN_TS_UNSUPPORTED") {
            return Err(GenError::TypescriptUnsupported);
        }
        return Err(GenError::Helper {
            msg: stderr.trim().to_string(),
        });
    }

    let doc: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| GenError::Helper {
            msg: format!("helper did not emit valid JSON: {e}"),
        })?;
    let schemas = locate_schemas(&doc).ok_or_else(|| GenError::NoSchemas {
        path: dts_path.to_path_buf(),
    })?;

    let module_name = sanitize_module(stem_of(dts_path));
    let source_label = dts_path.display().to_string();
    let regen = format!("glyph gen dts {}", dts_path.display());
    render_and_write(schemas, &module_name, &source_label, &regen, out_dir)
}

/// Parse a spec as JSON first, then YAML. OpenAPI ships as both.
fn parse_doc(raw: &str) -> Result<Value, String> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return Ok(v);
    }
    serde_yaml::from_str::<Value>(raw).map_err(|e| e.to_string())
}

/// Find the schema map: OpenAPI 3 `components.schemas`, Swagger 2 `definitions`,
/// JSON Schema `$defs`/`definitions`, or a single top-level object schema.
fn locate_schemas(doc: &Value) -> Option<Vec<(String, Value)>> {
    let candidates = [
        doc.get("components").and_then(|c| c.get("schemas")),
        doc.get("definitions"),
        doc.get("$defs"),
    ];
    for c in candidates.into_iter().flatten() {
        if let Some(obj) = c.as_object() {
            if !obj.is_empty() {
                return Some(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
        }
    }
    // A bare JSON Schema document: `{ "title": "Pet", "type": "object", ... }`.
    if doc.get("type").and_then(|t| t.as_str()) == Some("object") && doc.get("properties").is_some()
    {
        let name = doc
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Root")
            .to_string();
        return Some(vec![(name, doc.clone())]);
    }
    None
}

#[derive(Default)]
struct Generator {
    type_count: usize,
    notes: Vec<String>,
}

impl Generator {
    fn note(&mut self, msg: impl Into<String>) {
        self.notes.push(msg.into());
    }

    fn emit_module(
        &mut self,
        module_name: &str,
        source_label: &str,
        regen_cmd: &str,
        schemas: Vec<(String, Value)>,
    ) -> String {
        let mut out = String::new();
        out.push_str(&format!("module {module_name}\n\n"));
        out.push_str(&format!(
            "// Generated from {source_label}. Every type below is a real Glyph record\n\
             // with a runtime descriptor, so a request or response body validates\n\
             // through `T.parse(value)`. Regenerate with `{regen_cmd}`.\n\n",
        ));

        for (raw_name, schema) in &schemas {
            let name = sanitize_type(raw_name);
            let decl = self.emit_type(&name, schema);
            out.push_str(&decl);
            out.push('\n');
            self.type_count += 1;
        }
        out
    }

    /// Emit one top-level `type Name = ...` declaration.
    fn emit_type(&mut self, name: &str, schema: &Value) -> String {
        // allOf of objects → one merged record.
        if let Some(all) = schema.get("allOf").and_then(|v| v.as_array()) {
            if let Some(fields) = self.merge_all_of(name, all) {
                return self.render_record(name, &fields);
            }
            self.note(format!(
                "{name}: `allOf` mixes non-object members; emitted as `unknown`."
            ));
            return format!("type {name} = unknown\n");
        }

        // A string enum has no faithful Glyph representation; narrow to string.
        if schema.get("enum").is_some() && type_of(schema) == Some("string") {
            let vals = schema
                .get("enum")
                .and_then(|e| e.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| format!("\"{s}\""))
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .unwrap_or_default();
            self.note(format!(
                "{name}: string enum narrowed to `string` (Glyph has no string-literal union); allowed values: {vals}."
            ));
            return format!("// enum: {vals}\ntype {name} = string\n");
        }

        // Object with properties → record.
        if is_object(schema) {
            let fields = self.object_fields(name, schema);
            return self.render_record(name, &fields);
        }

        // Everything else at the top level: emit an alias to the mapped type.
        let ty = self.type_ref(name, schema);
        format!("type {name} = {ty}\n")
    }

    /// Collect `(field_name, type, optional)` for an object schema.
    fn object_fields(&mut self, owner: &str, schema: &Value) -> Vec<(String, String, bool)> {
        let required: Vec<&str> = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut fields = Vec::new();
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            for (fname, fschema) in props {
                if !is_ident(fname) {
                    self.note(format!(
                        "{owner}.{fname}: field name is not a legal Glyph identifier; skipped."
                    ));
                    continue;
                }
                // A `nullable` field is mapped to an optional field of the base
                // type, not `Option<T>`: on the wire the value is the bare value
                // or JSON `null`, whereas Glyph's `Option` has a tagged runtime
                // shape (`{tag:"Some",...}`) that would reject the real payload.
                let nullable = fschema
                    .get("nullable")
                    .and_then(|n| n.as_bool())
                    .unwrap_or(false);
                if nullable {
                    self.note(format!(
                        "{owner}.{fname}: `nullable` mapped to an optional field; a literal JSON `null` is treated as absent."
                    ));
                }
                let optional = nullable || !required.contains(&fname.as_str());
                let ty = self.type_ref(&format!("{owner}.{fname}"), fschema);
                fields.push((fname.clone(), ty, optional));
            }
        } else if let Some(ap) = schema.get("additionalProperties") {
            // A free-form object: `Record<string, T>`.
            let v = if ap.is_object() {
                self.type_ref(owner, ap)
            } else {
                "unknown".to_string()
            };
            fields.push(("__record__".to_string(), format!("Record<string, {v}>"), false));
        }
        fields
    }

    /// Render a record `type`, or a `Record<K,V>` alias when the object was
    /// free-form (single synthetic `__record__` field).
    fn render_record(&self, name: &str, fields: &[(String, String, bool)]) -> String {
        if let [(k, ty, _)] = fields {
            if k == "__record__" {
                return format!("type {name} = {ty}\n");
            }
        }
        if fields.is_empty() {
            return format!("type {name} = {{}}\n");
        }
        let mut out = format!("type {name} = {{\n");
        for (fname, ty, optional) in fields {
            let q = if *optional { "?" } else { "" };
            out.push_str(&format!("  {fname}{q}: {ty},\n"));
        }
        out.push_str("}\n");
        out
    }

    /// Merge `allOf` members into one field list, following `$ref`s to other
    /// object schemas is out of scope for the MVP: only inline object members
    /// and their properties are merged. Returns `None` if any member is not an
    /// inline object (so the caller can fall back to `unknown`).
    fn merge_all_of(&mut self, owner: &str, members: &[Value]) -> Option<Vec<(String, String, bool)>> {
        let mut fields = Vec::new();
        for m in members {
            if m.get("$ref").is_some() {
                // Referenced object: inline its fields by name via a spread is
                // not expressible; we note and keep going with what we can.
                self.note(format!(
                    "{owner}: `allOf` `$ref` member not inlined (MVP); its fields are omitted."
                ));
                continue;
            }
            if is_object(m) {
                fields.extend(self.object_fields(owner, m));
            } else {
                return None;
            }
        }
        Some(fields)
    }

    /// Map a schema to a Glyph *type expression* string (used in field / alias
    /// position). `ctx` is a dotted path for diagnostics.
    fn type_ref(&mut self, ctx: &str, schema: &Value) -> String {
        if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
            let base = ref_name(r);
            return sanitize_type(&base);
        }
        // `nullable` is handled at the field level (as optionality); the type
        // itself is always the base type, so its descriptor matches the wire.
        self.type_ref_inner(ctx, schema)
    }

    fn type_ref_inner(&mut self, ctx: &str, schema: &Value) -> String {
        // Inline enum in field position: narrow to the base primitive.
        if schema.get("enum").is_some() && type_of(schema) == Some("string") {
            self.note(format!("{ctx}: inline string enum narrowed to `string`."));
            return "string".to_string();
        }

        match type_of(schema) {
            Some("string") => "string".to_string(),
            Some("integer") | Some("number") => "number".to_string(),
            Some("boolean") => "bool".to_string(),
            Some("array") => {
                let item = schema.get("items");
                match item {
                    Some(it) => format!("Array<{}>", self.type_ref(ctx, it)),
                    None => {
                        self.note(format!("{ctx}: array without `items`; emitted `Array<unknown>`."));
                        "Array<unknown>".to_string()
                    }
                }
            }
            Some("object") | None if is_object(schema) => {
                // Inline nested object → inline record literal.
                let fields = self.object_fields(ctx, schema);
                if let [(k, ty, _)] = fields.as_slice() {
                    if k == "__record__" {
                        return ty.clone();
                    }
                }
                if fields.is_empty() {
                    return "unknown".to_string();
                }
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(f, ty, opt)| format!("{f}{}: {ty}", if *opt { "?" } else { "" }))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            }
            _ => {
                if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
                    self.note(format!(
                        "{ctx}: `oneOf`/`anyOf` has no faithful representation without a discriminator; emitted `unknown`."
                    ));
                } else {
                    self.note(format!("{ctx}: unrecognized schema; emitted `unknown`."));
                }
                "unknown".to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Small schema helpers
// ---------------------------------------------------------------------------

/// The `type` field as a string, if present and scalar.
fn type_of(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(|t| t.as_str())
}

fn is_object(schema: &Value) -> bool {
    type_of(schema) == Some("object")
        || (schema.get("properties").is_some() && schema.get("type").is_none())
        || (schema.get("additionalProperties").is_some() && schema.get("type").is_none())
}

/// The final path segment of a `$ref` (`#/components/schemas/Pet` → `Pet`).
fn ref_name(r: &str) -> String {
    r.rsplit('/').next().unwrap_or(r).to_string()
}

/// Is `s` a legal Glyph identifier? (Keywords are allowed as record field
/// names via `expect_field_name`, so only the shape matters here.)
fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Sanitize a schema key into a PascalCase Glyph type name. Non-ident chars
/// become word boundaries; a leading digit is prefixed with `T`.
fn sanitize_type(raw: &str) -> String {
    let mut out = String::new();
    let mut upper_next = true;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            if upper_next {
                out.extend(c.to_uppercase());
                upper_next = false;
            } else {
                out.push(c);
            }
        } else {
            upper_next = true;
        }
    }
    if out.is_empty() {
        return "Generated".to_string();
    }
    if out.chars().next().unwrap().is_ascii_digit() {
        out.insert(0, 'T');
    }
    out
}

/// Sanitize a file stem into a lowercase Glyph module name.
fn sanitize_module(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == '_' {
            out.push('_');
        }
        // other chars dropped
    }
    if out.is_empty() || out.chars().next().unwrap().is_ascii_digit() {
        out.insert(0, 'm');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gen_from(json: &str) -> (String, Vec<String>) {
        let doc: Value = serde_json::from_str(json).unwrap();
        let schemas = locate_schemas(&doc).expect("schemas");
        let mut g = Generator::default();
        let body = g.emit_module("t", "spec.json", "glyph gen openapi spec.json", schemas);
        // Prove the generated source parses and formats.
        let module = glyph_parser::parse(&body).expect("generated source parses");
        let comments = glyph_lexer::comments(&body);
        let formatted = glyph_formatter::format_module(&module, &comments, &body);
        (formatted, g.notes)
    }

    #[test]
    fn object_with_required_and_optional() {
        let (out, _) = gen_from(
            r#"{"components":{"schemas":{"Task":{"type":"object",
               "required":["id","title"],
               "properties":{"id":{"type":"integer"},"title":{"type":"string"},
                             "done":{"type":"boolean"}}}}}}"#,
        );
        assert!(out.contains("type Task = {"), "got:\n{out}");
        assert!(out.contains("id: number,"), "got:\n{out}");
        assert!(out.contains("title: string,"), "got:\n{out}");
        assert!(out.contains("done?: bool,"), "optional non-required; got:\n{out}");
    }

    #[test]
    fn ref_array_and_nullable() {
        let (out, _) = gen_from(
            r##"{"components":{"schemas":{
               "Bag":{"type":"object","required":["items","owner"],
                 "properties":{
                   "items":{"type":"array","items":{"$ref":"#/components/schemas/Task"}},
                   "owner":{"type":"string","nullable":true}}},
               "Task":{"type":"object","properties":{"id":{"type":"integer"}}}}}}"##,
        );
        assert!(out.contains("items: Array<Task>"), "got:\n{out}");
        // `nullable` maps to an optional field of the base type (not Option<T>),
        // so the descriptor matches the wire value rather than a tagged Option.
        assert!(out.contains("owner?: string"), "got:\n{out}");
    }

    #[test]
    fn string_enum_narrows_to_string_with_note() {
        let (out, notes) = gen_from(
            r#"{"definitions":{"Color":{"type":"string","enum":["red","green"]}}}"#,
        );
        assert!(out.contains("type Color = string"), "got:\n{out}");
        assert!(notes.iter().any(|n| n.contains("narrowed to `string`")), "notes: {notes:?}");
    }

    #[test]
    fn additional_properties_becomes_record() {
        let (out, _) = gen_from(
            r#"{"definitions":{"Meta":{"type":"object",
               "additionalProperties":{"type":"string"}}}}"#,
        );
        assert!(out.contains("type Meta = Record<string, string>"), "got:\n{out}");
    }

    #[test]
    fn oneof_without_discriminator_is_unknown_with_note() {
        let (out, notes) = gen_from(
            r#"{"definitions":{"Weird":{"oneOf":[{"type":"string"},{"type":"integer"}]}}}"#,
        );
        assert!(out.contains("type Weird = unknown"), "got:\n{out}");
        assert!(notes.iter().any(|n| n.contains("oneOf")), "notes: {notes:?}");
    }

    #[test]
    fn sanitizes_type_names() {
        assert_eq!(sanitize_type("pet-store.Category"), "PetStoreCategory");
        assert_eq!(sanitize_type("123abc"), "T123abc");
        assert_eq!(sanitize_type("user"), "User");
    }

    #[test]
    fn end_to_end_writes_a_parseable_file() {
        // Exercise the public entry point: spec on disk → a `.glyph` file whose
        // module name is the spec stem, containing the generated types.
        let dir = std::env::temp_dir().join(format!("glyph-gen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let spec = dir.join("orders.json");
        std::fs::write(
            &spec,
            r#"{"definitions":{"Order":{"type":"object","required":["id"],
               "properties":{"id":{"type":"integer"},"note":{"type":"string"}}}}}"#,
        )
        .unwrap();

        let report = openapi(&spec, &dir.join("out")).expect("gen succeeds");
        assert_eq!(report.type_count, 1);
        assert!(report.out_file.ends_with("orders.glyph"));
        let text = std::fs::read_to_string(&report.out_file).unwrap();
        assert!(text.starts_with("module orders"), "got:\n{text}");
        assert!(text.contains("type Order = {"), "got:\n{text}");
        // The written file must itself parse (the self-validation guarantee).
        assert!(glyph_parser::parse(&text).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dts_generates_when_typescript_is_available() {
        // `glyph gen dts` shells out to node + the `typescript` package. Where
        // either is absent (some CI/sandboxes), the command returns a clean
        // Missing error rather than a failure — assert that contract, and the
        // full mapping only when the toolchain is present.
        let dir = std::env::temp_dir().join(format!("glyph-dts-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("acct.d.ts");
        std::fs::write(
            &src,
            "export interface Account { id: number; name: string; nickname?: string; \
             tags: string[]; kind: \"free\" | \"paid\"; }",
        )
        .unwrap();

        match dts(&src, &dir.join("out")) {
            Ok(report) => {
                let text = std::fs::read_to_string(&report.out_file).unwrap();
                assert!(text.starts_with("module acct"), "got:\n{text}");
                assert!(text.contains("id: number,"), "got:\n{text}");
                assert!(text.contains("nickname?: string,"), "got:\n{text}");
                assert!(text.contains("tags: Array<string>,"), "got:\n{text}");
                assert!(text.contains("kind: string,"), "enum narrowed; got:\n{text}");
                assert!(glyph_parser::parse(&text).is_ok());
            }
            Err(GenError::NodeMissing)
            | Err(GenError::TypescriptMissing)
            | Err(GenError::TypescriptUnsupported) => {
                // Toolchain absent or incompatible (e.g. CI installs the
                // TypeScript 7 native port); the clean-skip contract holds.
            }
            Err(e) => panic!("unexpected gen dts error: {e}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
