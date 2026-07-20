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
    #[error("the `typescript` package is not resolvable. Install it with `npm install -g typescript`, then re-run `glyph gen dts`.")]
    TypescriptMissing,
    #[error("TypeScript is installed but its compiler API could not be loaded (neither the classic `createSourceFile` API nor the 7.x native `typescript/unstable` API). Reinstall it with a normal `npm install typescript` (5, 6, and 7 are all supported), then re-run.")]
    TypescriptUnsupported,
    #[error("`tsx` not found on PATH. `glyph gen zod` needs it to execute the schema module; install it with `npm install -g tsx`.")]
    TsxMissing,
    #[error("the `zod` package is not resolvable from the schema file's project. Install it with `npm install zod`, then re-run `glyph gen zod`.")]
    ZodMissing,
    #[error("converting these zod schemas needs zod 4 (with `z.toJSONSchema`) or the `zod-to-json-schema` package. Install one, then re-run `glyph gen zod`.")]
    ZodUnsupported,
    #[error("running the schema helper failed: {msg}")]
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
/// writing one `.glyph` file into `out_dir`. With `client`, also emit a typed
/// client function per operation over `std/http`.
pub fn openapi(
    spec_path: &Path,
    out_dir: &Path,
    client: bool,
    handlers: bool,
) -> Result<GenReport, GenError> {
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

    let mut gen = Generator::default();
    let (imports, trailer) = if client || handlers {
        let ops = gen.collect_operations(doc.get("paths"));
        let mut parts = Vec::new();
        if client {
            parts.push(gen.emit_client(&ops));
        }
        if handlers {
            // Prefix handler stubs when the client is also emitted, so their
            // names don't collide with the client functions.
            parts.push(gen.emit_handlers(&ops, client));
        }
        assemble_extras(&parts)
    } else {
        (String::new(), String::new())
    };
    render_and_write(gen, schemas, module_name, source_label, regen, imports, trailer, out_dir)
}

/// Turn a schema map into a formatted, self-validated `.glyph` file on disk.
/// Shared by `glyph gen openapi` and `glyph gen dts` — both reduce to a JSON
/// Schema `definitions` map, so both flow through the one mapper. `imports` and
/// `trailer` carry optional client/handler code emitted alongside the types.
#[allow(clippy::too_many_arguments)]
fn render_and_write(
    mut gen: Generator,
    schemas: Vec<(String, Value)>,
    module_name: String,
    source_label: String,
    regen_cmd: String,
    imports: String,
    trailer: String,
    out_dir: &Path,
) -> Result<GenReport, GenError> {
    let module_name = &module_name;
    let body = gen.emit_module(module_name, &source_label, &regen_cmd, schemas, &imports, &trailer);

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

    let doc = match run_helper(TS_TO_SCHEMA, "glyph-ts-to-schema", "node", dts_path) {
        HelperOutcome::Ok(v) => v,
        HelperOutcome::RunnerMissing => return Err(GenError::NodeMissing),
        HelperOutcome::Exit(stderr) => {
            if stderr.contains("GLYPH_GEN_NO_TYPESCRIPT") {
                return Err(GenError::TypescriptMissing);
            }
            if stderr.contains("GLYPH_GEN_TS_UNSUPPORTED") {
                return Err(GenError::TypescriptUnsupported);
            }
            return Err(GenError::Helper { msg: stderr });
        }
        HelperOutcome::Io(msg) => return Err(GenError::Helper { msg }),
    };
    let schemas = locate_schemas(&doc).ok_or_else(|| GenError::NoSchemas {
        path: dts_path.to_path_buf(),
    })?;

    let module_name = sanitize_module(stem_of(dts_path));
    let source_label = dts_path.display().to_string();
    let regen = format!("glyph gen dts {}", dts_path.display());
    render_and_write(
        Generator::default(),
        schemas,
        module_name,
        source_label,
        regen,
        String::new(),
        String::new(),
        out_dir,
    )
}

/// The `tsx` helper that executes a module of zod schemas and prints a JSON
/// Schema `definitions` map. Bundled into the binary.
const ZOD_TO_SCHEMA: &str = include_str!("../../../runtime/tools/zod-to-schema.mjs");

/// Generate Glyph types from a TypeScript module of zod schemas.
///
/// Runs a bundled `tsx` helper that imports the module (a zod schema is a
/// runtime value, so the module is executed), converts each exported schema to
/// JSON Schema, and feeds the same mapper as `glyph gen openapi`/`dts`. `tsx`
/// must be on PATH and `zod` resolvable from the file's project.
pub fn zod(file: &Path, out_dir: &Path) -> Result<GenReport, GenError> {
    if !file.exists() {
        return Err(GenError::Read {
            path: file.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        });
    }

    let doc = match run_helper(ZOD_TO_SCHEMA, "glyph-zod-to-schema", "tsx", file) {
        HelperOutcome::Ok(v) => v,
        HelperOutcome::RunnerMissing => return Err(GenError::TsxMissing),
        HelperOutcome::Exit(stderr) => {
            if stderr.contains("GLYPH_GEN_NO_ZOD") {
                return Err(GenError::ZodMissing);
            }
            if stderr.contains("GLYPH_GEN_ZOD_UNSUPPORTED") {
                return Err(GenError::ZodUnsupported);
            }
            return Err(GenError::Helper { msg: stderr });
        }
        HelperOutcome::Io(msg) => return Err(GenError::Helper { msg }),
    };
    let schemas = locate_schemas(&doc).ok_or_else(|| GenError::NoSchemas {
        path: file.to_path_buf(),
    })?;

    let module_name = sanitize_module(stem_of(file));
    let source_label = file.display().to_string();
    let regen = format!("glyph gen zod {}", file.display());
    render_and_write(
        Generator::default(),
        schemas,
        module_name,
        source_label,
        regen,
        String::new(),
        String::new(),
        out_dir,
    )
}

/// The result of running a bundled node/tsx helper that prints JSON to stdout.
enum HelperOutcome {
    Ok(Value),
    /// The runner (`node`/`tsx`) was not found on PATH.
    RunnerMissing,
    /// The helper exited non-zero; carries its trimmed stderr (which may hold a
    /// `GLYPH_GEN_*` sentinel the caller maps to a specific diagnostic).
    Exit(String),
    /// A local failure: could not write the helper, spawn it, or parse its JSON.
    Io(String),
}

/// Write `script` to a temp file, run it with `runner <script> <arg>`, and parse
/// its stdout as JSON. NODE_PATH is set to the global module root so a global
/// install of the helper's dependency is resolvable.
fn run_helper(script: &str, name: &str, runner: &str, arg: &Path) -> HelperOutcome {
    let helper = std::env::temp_dir().join(format!("{name}-{}.mjs", std::process::id()));
    if let Err(e) = std::fs::write(&helper, script) {
        return HelperOutcome::Io(format!("cannot write helper: {e}"));
    }

    let global_root = std::process::Command::new("npm")
        .args(["root", "-g"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let mut cmd = std::process::Command::new(runner);
    cmd.arg(&helper).arg(arg);
    if let Some(root) = &global_root {
        cmd.env("NODE_PATH", root);
    }
    let output = cmd.output();
    let _ = std::fs::remove_file(&helper);

    match output {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HelperOutcome::RunnerMissing,
        Err(e) => HelperOutcome::Io(format!("failed to run {runner}: {e}")),
        Ok(o) if !o.status.success() => {
            HelperOutcome::Exit(String::from_utf8_lossy(&o.stderr).trim().to_string())
        }
        Ok(o) => match serde_json::from_slice(&o.stdout) {
            Ok(v) => HelperOutcome::Ok(v),
            Err(e) => HelperOutcome::Io(format!("helper did not emit valid JSON: {e}")),
        },
    }
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
        imports: &str,
        trailer: &str,
    ) -> String {
        let mut out = String::new();
        out.push_str(&format!("module {module_name}\n\n"));
        if !imports.is_empty() {
            out.push_str(imports);
            out.push('\n');
        }
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
        if !trailer.is_empty() {
            out.push_str(trailer);
        }
        out
    }

    /// Collect every operation in `paths` into a flat, deterministic list, with
    /// deduped function names. Shared by client and handler codegen.
    fn collect_operations(&mut self, paths: Option<&Value>) -> Vec<Op> {
        let Some(obj) = paths.and_then(|p| p.as_object()) else {
            self.note("client/handlers requested but the spec has no `paths`; emitted types only.");
            return Vec::new();
        };

        let mut ops = Vec::new();
        let mut seen_ids: Vec<String> = Vec::new();
        // Deterministic order: paths come from a sorted map already.
        for (path_str, item) in obj {
            let Some(item_obj) = item.as_object() else { continue };
            let shared_params = item_obj.get("parameters");
            for method in ["get", "post", "put", "patch", "delete"] {
                let Some(op) = item_obj.get(method).and_then(|m| m.as_object()) else {
                    continue;
                };
                let verb = if method == "delete" { "del" } else { method };

                let base_id = op
                    .get("operationId")
                    .and_then(|v| v.as_str())
                    .map(sanitize_fn)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| synth_op_id(method, path_str));
                let mut op_id = base_id.clone();
                let mut n = 2;
                while seen_ids.contains(&op_id) {
                    op_id = format!("{base_id}{n}");
                    n += 1;
                }
                seen_ids.push(op_id.clone());

                let param_defs = self.collect_op_params(op, shared_params);
                let path_params: Vec<(String, String)> = path_param_names(path_str)
                    .into_iter()
                    .filter(|name| is_ident(name))
                    .map(|name| {
                        let ty = param_defs
                            .iter()
                            .find(|(n, _)| n == &name)
                            .map(|(_, t)| t.clone())
                            .unwrap_or_else(|| "string".to_string());
                        (name, ty)
                    })
                    .collect();

                let body_type = if matches!(verb, "post" | "put" | "patch") {
                    self.request_body_type(op)
                } else {
                    None
                };

                ops.push(Op {
                    method: method.to_uppercase(),
                    verb: verb.to_string(),
                    op_id,
                    path: path_str.clone(),
                    path_params,
                    body_type,
                });
            }
            for other in ["head", "options", "trace"] {
                if item_obj.contains_key(other) {
                    self.note(format!("{path_str}: `{other}` operation skipped."));
                }
            }
        }
        ops
    }

    /// Emit a typed client function per operation. Each takes a `base` URL, its
    /// typed path params, and (for post/put/patch) a typed body, and returns
    /// `Result<Response, HttpError>`. The response body is `unknown` by design —
    /// validate it with the matching DTO's `.parse`.
    fn emit_client(&mut self, ops: &[Op]) -> Emitted {
        let mut e = Emitted::default();
        if ops.is_empty() {
            return e;
        }
        for op in ops {
            e.http_verb(&op.verb);
            let mut sig = vec!["base: string".to_string()];
            for (name, ty) in &op.path_params {
                sig.push(format!("{name}: {ty}"));
            }
            if let Some(bt) = &op.body_type {
                sig.push(format!("body: {bt}"));
            }
            let names: Vec<String> = op.path_params.iter().map(|(n, _)| n.clone()).collect();
            let url = url_template(&op.path, &names);
            let call_args = match &op.body_type {
                Some(_) => format!("{url}, body"),
                None => url,
            };
            e.body.push_str(&format!(
                "async fn {id}({params}) -> Result<Response, HttpError> {{\n  \
                   return await {verb}({call_args})\n}}\n\n",
                id = op.op_id,
                params = sig.join(", "),
                verb = op.verb,
            ));
        }
        e.http("Response");
        e.http("HttpError");
        e.result("Result");
        e.banner = "// Typed client, one function per operation. Each returns the HTTP\n\
             // Response; validate its `unknown` body with the matching type's\n\
             // `.parse`. `base` is the server origin, e.g. \"http://localhost:8137\"."
            .to_string();
        e
    }

    /// Emit a handler stub per operation plus a `route` dispatcher that matches
    /// method + path (via array patterns over `segments(req)`, so `/tasks/{id}`
    /// binds `id`). Stubs return 501; the user fills them in. When `prefix` is
    /// set (client is also generated into the same module), stubs are named
    /// `handle_<op>` to avoid colliding with the client functions.
    fn emit_handlers(&mut self, ops: &[Op], prefix: bool) -> Emitted {
        let mut e = Emitted::default();
        if ops.is_empty() {
            return e;
        }
        let name_of = |op: &Op| {
            if prefix {
                format!("handle_{}", op.op_id)
            } else {
                op.op_id.clone()
            }
        };

        // One handler stub per operation.
        for op in ops {
            let mut sig = vec!["req: Request".to_string()];
            for (name, _) in &op.path_params {
                // Path segments arrive as strings from the router.
                sig.push(format!("{name}: string"));
            }
            let hint = match &op.body_type {
                Some(bt) => format!(
                    "  // Validate the body: match {bt}.parse(req.body) {{ Ok(input) => ..., Err(issues) => ... }}\n"
                ),
                None => String::new(),
            };
            e.body.push_str(&format!(
                "// TODO: implement ({method} {path}).\nfn {name}({params}) -> Result<Response, string> {{\n{hint}  \
                   return Ok(json(501, {{ error: \"not implemented\" }}))\n}}\n\n",
                method = op.method,
                path = op.path,
                name = name_of(op),
                params = sig.join(", "),
            ));
        }

        // The router: match method, then path segments.
        let mut methods: Vec<String> = Vec::new();
        for op in ops {
            if !methods.contains(&op.method) {
                methods.push(op.method.clone());
            }
        }
        let mut router = String::from("fn route(req: Request) -> Result<Response, string> {\n  return match req.method {\n");
        for m in &methods {
            router.push_str(&format!("    \"{m}\" => match segments(req) {{\n"));
            for op in ops.iter().filter(|o| &o.method == m) {
                let pat = segment_pattern(&op.path, &op.path_params);
                let args: Vec<String> = std::iter::once("req".to_string())
                    .chain(op.path_params.iter().map(|(n, _)| n.clone()))
                    .collect();
                router.push_str(&format!(
                    "      {pat} => {name}({args}),\n",
                    name = name_of(op),
                    args = args.join(", "),
                ));
            }
            router.push_str("      else => Ok(json(404, { error: \"not found\" })),\n    },\n");
        }
        router.push_str("    else => Ok(json(405, { error: \"method not allowed\" })),\n  }\n}\n");
        e.body.push_str(&router);

        e.http("Request");
        e.http("Response");
        e.http("json");
        e.http("segments");
        e.result("Result");
        e.result("Ok");
        e.banner =
            "// Server handlers. Each stub returns 501 — fill it in. `route` dispatches\n\
             // by method and path; wire it up with `await serve(PORT, route)` in main."
                .to_string();
        e
    }

    /// Collect `(name, glyph_type)` for an operation's path parameters, merging
    /// path-item-level and operation-level `parameters` (only `in: path`).
    fn collect_op_params(
        &mut self,
        op: &serde_json::Map<String, Value>,
        shared: Option<&Value>,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let lists = [shared, op.get("parameters")];
        for list in lists.into_iter().flatten() {
            let Some(arr) = list.as_array() else { continue };
            for p in arr {
                if p.get("in").and_then(|v| v.as_str()) != Some("path") {
                    continue;
                }
                let Some(name) = p.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let ty = match p.get("schema") {
                    Some(s) => self.type_ref(&format!("param {name}"), s),
                    None => "string".to_string(),
                };
                out.push((name.to_string(), ty));
            }
        }
        out
    }

    /// The Glyph type of an operation's JSON request body, if any.
    fn request_body_type(&mut self, op: &serde_json::Map<String, Value>) -> Option<String> {
        let schema = op
            .get("requestBody")?
            .get("content")?
            .get("application/json")?
            .get("schema")?;
        Some(self.type_ref("request body", schema))
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

/// One HTTP operation, flattened from the spec's `paths`.
struct Op {
    /// Upper-case HTTP method, e.g. `GET`.
    method: String,
    /// The `std/http` client verb: get/post/put/patch/del.
    verb: String,
    /// The generated function name (deduped).
    op_id: String,
    /// The path template, e.g. `/tasks/{id}`.
    path: String,
    /// Path parameters as `(name, glyph_type)`, in path order.
    path_params: Vec<(String, String)>,
    /// The request body's Glyph type (post/put/patch), if any.
    body_type: Option<String>,
}

/// Accumulated output of a codegen pass: the imports it needs and its function
/// text, kept separate so client and handler passes can be merged.
#[derive(Default)]
struct Emitted {
    /// `std/http` names to import, in first-seen order.
    http: Vec<String>,
    /// `std/result` names to import, in first-seen order.
    result: Vec<String>,
    /// A comment banner printed before the functions.
    banner: String,
    /// The generated function declarations.
    body: String,
}

impl Emitted {
    fn http(&mut self, name: &str) {
        if !self.http.iter().any(|n| n == name) {
            self.http.push(name.to_string());
        }
    }
    fn http_verb(&mut self, verb: &str) {
        self.http(verb);
    }
    fn result(&mut self, name: &str) {
        if !self.result.iter().any(|n| n == name) {
            self.result.push(name.to_string());
        }
    }
}

/// Merge one or more codegen passes into `(imports, trailer)` for the module:
/// union the imports, concatenate each pass's banner + body.
fn assemble_extras(parts: &[Emitted]) -> (String, String) {
    let mut http: Vec<String> = Vec::new();
    let mut result: Vec<String> = Vec::new();
    for p in parts {
        if p.body.is_empty() {
            continue;
        }
        for n in &p.http {
            if !http.contains(n) {
                http.push(n.clone());
            }
        }
        for n in &p.result {
            if !result.contains(n) {
                result.push(n.clone());
            }
        }
    }
    let mut imports = String::new();
    if !http.is_empty() {
        imports.push_str(&format!("import std/http {{ {} }}\n", http.join(", ")));
    }
    if !result.is_empty() {
        imports.push_str(&format!("import std/result {{ {} }}\n", result.join(", ")));
    }

    let mut trailer = String::new();
    for p in parts {
        if p.body.is_empty() {
            continue;
        }
        trailer.push_str(&p.banner);
        trailer.push_str("\n\n");
        trailer.push_str(&p.body);
    }
    let trailer = if trailer.is_empty() {
        String::new()
    } else {
        trailer.trim_end().to_string() + "\n"
    };
    (imports, trailer)
}

/// Build a Glyph array pattern for a path's segments: `/tasks/{id}` with a
/// declared `id` param → `["tasks", id]` (literal segment + capture binding). A
/// path with no segments (`/`) yields `[]`.
fn segment_pattern(path: &str, params: &[(String, String)]) -> String {
    let parts: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') {
                let name = &seg[1..seg.len() - 1];
                if is_ident(name) && params.iter().any(|(n, _)| n == name) {
                    return name.to_string();
                }
            }
            format!("\"{seg}\"")
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

/// Sanitize an operationId into a legal Glyph function name: keep alphanumerics
/// and underscores, drop other characters, and prefix a leading digit.
fn sanitize_fn(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'f');
    }
    out
}

/// Synthesize a function name from a method and path, e.g. GET `/tasks/{id}` →
/// `get_tasks_id`.
fn synth_op_id(method: &str, path: &str) -> String {
    let mut parts = vec![method.to_string()];
    for seg in path.split('/') {
        let seg = seg.trim_matches(|c| c == '{' || c == '}');
        if seg.is_empty() {
            continue;
        }
        let cleaned: String = seg
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    sanitize_fn(&parts.join("_"))
}

/// The `{name}` placeholders in a path template, in order.
fn path_param_names(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find('}') {
            names.push(after[..close].to_string());
            rest = &after[close + 1..];
        } else {
            break;
        }
    }
    names
}

/// Build a Glyph template string for a request URL: `/tasks/{id}` with a
/// declared `id` param → `"${base}/tasks/${id}"`. Placeholders whose name is not
/// a legal identifier are left as literal text (they won't be interpolated).
fn url_template(path: &str, params: &[String]) -> String {
    let mut body = String::from("${base}");
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        body.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(close) = after.find('}') {
            let name = &after[..close];
            if params.iter().any(|p| p == name) && is_ident(name) {
                body.push_str(&format!("${{{name}}}"));
            } else {
                body.push_str(&format!("{{{name}}}"));
            }
            rest = &after[close + 1..];
        } else {
            body.push_str(&rest[open..]);
            rest = "";
            break;
        }
    }
    body.push_str(rest);
    format!("\"{body}\"")
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
        let body = g.emit_module("t", "spec.json", "glyph gen openapi spec.json", schemas, "", "");
        // Prove the generated source parses and formats.
        let module = glyph_parser::parse(&body).expect("generated source parses");
        let comments = glyph_lexer::comments(&body);
        let formatted = glyph_formatter::format_module(&module, &comments, &body);
        (formatted, g.notes)
    }

    /// Full pipeline including client and/or handler emission, returning the
    /// formatted module.
    fn gen_ops(json: &str, client: bool, handlers: bool) -> (String, Vec<String>) {
        let doc: Value = serde_json::from_str(json).unwrap();
        let schemas = locate_schemas(&doc).unwrap_or_default();
        let mut g = Generator::default();
        let ops = g.collect_operations(doc.get("paths"));
        let mut parts = Vec::new();
        if client {
            parts.push(g.emit_client(&ops));
        }
        if handlers {
            parts.push(g.emit_handlers(&ops, client));
        }
        let (imports, trailer) = assemble_extras(&parts);
        let body = g.emit_module("t", "spec.json", "glyph gen openapi spec.json", schemas, &imports, &trailer);
        let module = glyph_parser::parse(&body).expect("generated source parses");
        let comments = glyph_lexer::comments(&body);
        let formatted = glyph_formatter::format_module(&module, &comments, &body);
        (formatted, g.notes)
    }

    #[test]
    fn client_emits_typed_functions_per_operation() {
        let (out, _) = gen_ops(
            r##"{
              "paths": {
                "/tasks": {
                  "get": { "operationId": "listTasks", "responses": {} },
                  "post": { "operationId": "createTask", "requestBody": { "content":
                    { "application/json": { "schema": { "$ref": "#/components/schemas/NewTask" } } } },
                    "responses": {} }
                },
                "/tasks/{id}": {
                  "get": { "operationId": "getTask",
                    "parameters": [{ "name": "id", "in": "path", "schema": { "type": "integer" } }],
                    "responses": {} },
                  "delete": {
                    "parameters": [{ "name": "id", "in": "path", "schema": { "type": "integer" } }],
                    "responses": {} }
                }
              },
              "components": { "schemas": {
                "NewTask": { "type": "object", "required": ["title"],
                  "properties": { "title": { "type": "string" } } } } }
            }"##,
            true,
            false,
        );
        // Imports only the verbs actually used.
        assert!(out.contains("import std/http { get, post, del, Response, HttpError }"), "got:\n{out}");
        // Named operation with a typed body.
        assert!(out.contains("async fn createTask(base: string, body: NewTask) -> Result<Response, HttpError>"), "got:\n{out}");
        // Typed path param and interpolated URL.
        assert!(out.contains("async fn getTask(base: string, id: number)"), "got:\n{out}");
        assert!(out.contains("return await get(\"${base}/tasks/${id}\")"), "got:\n{out}");
        // Synthesized name for the op with no operationId.
        assert!(out.contains("async fn delete_tasks_id(base: string, id: number)"), "got:\n{out}");
        assert!(out.contains("return await del(\"${base}/tasks/${id}\")"), "got:\n{out}");
    }

    #[test]
    fn client_without_paths_notes_and_emits_nothing() {
        let (out, notes) = gen_ops(r#"{ "components": { "schemas": {} } }"#, true, false);
        assert!(!out.contains("async fn"), "got:\n{out}");
        assert!(notes.iter().any(|n| n.contains("no `paths`")), "notes: {notes:?}");
    }

    #[test]
    fn handlers_emit_stubs_and_a_router() {
        let (out, _) = gen_ops(
            r##"{
              "paths": {
                "/tasks": {
                  "get": { "operationId": "listTasks", "responses": {} },
                  "post": { "operationId": "createTask", "requestBody": { "content":
                    { "application/json": { "schema": { "$ref": "#/components/schemas/NewTask" } } } },
                    "responses": {} }
                },
                "/tasks/{id}": {
                  "get": { "operationId": "getTask",
                    "parameters": [{ "name": "id", "in": "path", "schema": { "type": "integer" } }],
                    "responses": {} }
                }
              },
              "components": { "schemas": {
                "NewTask": { "type": "object", "required": ["title"],
                  "properties": { "title": { "type": "string" } } } } }
            }"##,
            false,
            true,
        );
        // A typed stub per operation; path params arrive as strings.
        assert!(out.contains("fn listTasks(req: Request) -> Result<Response, string>"), "got:\n{out}");
        assert!(out.contains("fn getTask(req: Request, id: string) -> Result<Response, string>"), "got:\n{out}");
        // A router matching method then path segments (array patterns).
        assert!(out.contains("fn route(req: Request) -> Result<Response, string>"), "got:\n{out}");
        assert!(out.contains("match req.method"), "got:\n{out}");
        assert!(out.contains("[\"tasks\"] => listTasks(req)"), "got:\n{out}");
        assert!(out.contains("[\"tasks\", id] => getTask(req, id)"), "got:\n{out}");
        // The body-parse hint for the POST stub.
        assert!(out.contains("NewTask.parse(req.body)"), "got:\n{out}");
    }

    #[test]
    fn client_and_handlers_together_do_not_collide() {
        // With both, handler stubs are prefixed so names stay unique.
        let (out, _) = gen_ops(
            r#"{ "paths": { "/ping": { "get": { "operationId": "ping", "responses": {} } } } }"#,
            true,
            true,
        );
        assert!(out.contains("async fn ping(base: string)"), "client; got:\n{out}");
        assert!(out.contains("fn handle_ping(req: Request)"), "prefixed handler; got:\n{out}");
        assert!(out.contains("=> handle_ping(req)"), "router calls prefixed; got:\n{out}");
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

        let report = openapi(&spec, &dir.join("out"), false, false).expect("gen succeeds");
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

    #[test]
    fn zod_generates_or_skips_cleanly() {
        // `glyph gen zod` needs tsx + a resolvable zod. Where either is absent
        // (most CI/sandboxes), it must return a clean Missing/Unsupported error
        // rather than a crash; assert that contract, and the mapping only when
        // the toolchain is present.
        let dir = std::env::temp_dir().join(format!("glyph-zod-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("schemas.ts");
        std::fs::write(
            &src,
            "import { z } from \"zod\";\n\
             export const Account = z.object({ id: z.number(), name: z.string(), \
             nickname: z.string().optional() });\n",
        )
        .unwrap();

        match zod(&src, &dir.join("out")) {
            Ok(report) => {
                let text = std::fs::read_to_string(&report.out_file).unwrap();
                assert!(text.contains("type Account = {"), "got:\n{text}");
                assert!(text.contains("id: number,"), "got:\n{text}");
                assert!(text.contains("nickname?: string,"), "got:\n{text}");
                assert!(glyph_parser::parse(&text).is_ok());
            }
            Err(GenError::TsxMissing)
            | Err(GenError::ZodMissing)
            | Err(GenError::ZodUnsupported) => {
                // Toolchain absent; the clean-skip contract holds.
            }
            Err(e) => panic!("unexpected gen zod error: {e}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
