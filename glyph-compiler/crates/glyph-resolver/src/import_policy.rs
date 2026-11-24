//! A project-level banned import (D74): `glyph.banned_imports` in the
//! project's `package.json`.
//!
//! A project can name an import path its code may not use, and the import to
//! write instead: a raw HTTP client behind the project's retry and auth
//! wrapper, `std/fs` behind a storage module. The point is the agent that
//! reaches for the library it knows rather than the wrapper it was never told
//! about. An import of a banned path is `E0115`, whose help names the
//! replacement, and the same specifier quoted inside an `extern_ts` body is
//! `E0115` too, so the escape hatch is not a way around it.
//!
//! The `glyph` key is a closed schema from here on: an unknown key, or an
//! unknown field in a ban, is `E0116`. A typo (`banned_import`, `alow_in`) must
//! not turn a policy off without a word.
//!
//! Two halves, both free of filesystem access like the rest of this crate:
//!
//! - [`read_glyph_config`] reads the manifest text with a span-preserving JSON
//!   scan (the diagnostics point at a key or a value, and `serde_json` keeps no
//!   spans) and validates the `glyph` key against facts the caller supplies.
//! - [`verify_banned_imports`] checks one module against the policy that read
//!   produced.

use std::borrow::Cow;

use glyph_ast::{Decl, Module, Span};

use crate::error::ResolveError;
use crate::module_graph::path_key;

// ============================================================================
// The policy
// ============================================================================

/// One banned import path and what to write instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBan {
    /// The banned path as written after `import`: an npm package (scoped
    /// included), `std/<module>`, or a project module path. It bans itself and
    /// every path under it (`axios` bans `axios/lib/core`, not `axios-retry`).
    pub key: String,
    /// The import to write instead.
    pub use_instead: String,
    /// Whether `glyph query exports --module` answers for `use_instead`: a
    /// module of this project or a `std/` module. An npm package is not one
    /// it covers, so the help leaves the hint off for it (record 205).
    pub use_has_exports: bool,
    /// One line, quoted verbatim in the error.
    pub reason: Option<String>,
    /// Project modules that may import it, beyond `use_instead`. An entry
    /// ending in `/` covers that directory's modules, recursively.
    pub allow_in: Vec<String>,
}

impl ImportBan {
    /// Whether this ban covers the import path `path`.
    pub fn covers(&self, path: &str) -> bool {
        path == self.key
            || path
                .strip_prefix(self.key.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }

    /// Whether the module at `module_path` may import a path this ban covers:
    /// the replacement itself, or a module `allow_in` names.
    pub fn allows(&self, module_path: &str) -> bool {
        if module_path == self.use_instead {
            return true;
        }
        self.allow_in.iter().any(|entry| match entry.strip_suffix('/') {
            Some(dir) => module_path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')),
            None => module_path == entry,
        })
    }
}

/// The bans a project declares. Empty for a project with no `banned_imports`,
/// and for a tree with no marked project at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportPolicy {
    pub bans: Vec<ImportBan>,
}

impl ImportPolicy {
    pub fn is_empty(&self) -> bool {
        self.bans.is_empty()
    }

    /// The ban that refuses `path` in the module at `module_path`, if any.
    ///
    /// Two keys can cover one path (`axios` and `axios/lib`). The longer key is
    /// asked first, because it is the more specific statement about the path,
    /// and a module is refused when any covering ban does not allow it.
    pub fn refusal(&self, path: &str, module_path: &str) -> Option<&ImportBan> {
        let mut covering: Vec<&ImportBan> = self.bans.iter().filter(|b| b.covers(path)).collect();
        covering.sort_by_key(|b| std::cmp::Reverse(b.key.len()));
        covering.into_iter().find(|b| !b.allows(module_path))
    }

    /// The ban covering `path`, regardless of who imports it.
    pub fn covering(&self, path: &str) -> Option<&ImportBan> {
        self.bans
            .iter()
            .filter(|b| b.covers(path))
            .max_by_key(|b| b.key.len())
    }
}

// ============================================================================
// E0115: the check over one module
// ============================================================================

/// Every banned import in `module`, and every banned specifier quoted in one of
/// its `extern_ts` bodies, as `E0115`.
///
/// `module_path` is the module's path under its project root, which is what
/// `use` and `allow_in` are written in. `source` is the module's text: the
/// import path's own span starts after the `import` keyword, and the
/// `extern_ts` bodies are found in the token stream the parser read rather than
/// by a walk of every expression and type position, which is a list a new
/// position could fall out of.
pub fn verify_banned_imports(
    module: &Module,
    source: &str,
    module_path: &str,
    policy: &ImportPolicy,
) -> Vec<ResolveError> {
    if policy.is_empty() {
        return Vec::new();
    }
    let mut errors = Vec::new();
    for item in &module.items {
        let Decl::Import(imp) = item else { continue };
        let path = path_key(&imp.path);
        if let Some(ban) = policy.refusal(&path, module_path) {
            errors.push(banned(path, ban, import_path_span(source, imp.path.span)));
        }
    }
    for (raw, span) in extern_ts_bodies(source) {
        for specifier in quoted_strings(&raw) {
            // Glyph writes a Node builtin by its bare name (`import fs`), so
            // a ban's key is `fs`; raw TypeScript may spell it `node:fs`.
            let specifier = specifier
                .strip_prefix("node:")
                .map(str::to_string)
                .unwrap_or(specifier);
            if let Some(ban) = policy.refusal(&specifier, module_path) {
                errors.push(banned(specifier, ban, span));
            }
        }
    }
    errors
}

/// Drop the `E0104` an import would also get when it is banned. "Install
/// axios" is the wrong advice about a package the project has said not to
/// use; the ban is the one thing to say.
pub fn drop_superseded_by_ban(
    errors: Vec<ResolveError>,
    module: &Module,
    module_path: &str,
    policy: &ImportPolicy,
) -> Vec<ResolveError> {
    if policy.is_empty() {
        return errors;
    }
    let banned_spans: Vec<Span> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Decl::Import(imp) => policy
                .refusal(&path_key(&imp.path), module_path)
                .map(|_| imp.span),
            _ => None,
        })
        .collect();
    errors
        .into_iter()
        .filter(|e| !(e.code() == "E0104" && banned_spans.contains(&e.span())))
        .collect()
}

fn banned(path: String, ban: &ImportBan, span: Span) -> ResolveError {
    ResolveError::BannedImport {
        under: (path != ban.key).then(|| ban.key.clone()),
        path,
        use_instead: ban.use_instead.clone(),
        use_has_exports: ban.use_has_exports,
        reason: ban.reason.clone(),
        span,
    }
}

/// The span of the path in an `import` line. The parser's path span starts at
/// the `import` keyword, and the error belongs on the path.
fn import_path_span(source: &str, span: Span) -> Span {
    let (start, end) = (span.start as usize, span.end as usize);
    let Some(text) = source.get(start..end) else {
        return span;
    };
    let Some(rest) = text.strip_prefix("import") else {
        return span;
    };
    let skipped = rest.len() - rest.trim_start().len();
    Span::new((start + "import".len() + skipped) as u32, span.end)
}

/// The raw text of every `extern_ts("...")` in `source`, with the span of its
/// string literal, in source order.
///
/// Read from the tokens the parser itself read: the source is lexed with the
/// JSX text runs the parser scanned blanked out, which is the token stream the
/// parse consumed (the formatter's check against the lexer reads it the same
/// way). A string or a comment that merely contains `extern_ts(` is not a
/// token sequence of `extern_ts`, `(`, string, and is not reported.
fn extern_ts_bodies(source: &str) -> Vec<(String, Span)> {
    if !source.contains("extern_ts") {
        return Vec::new();
    }
    let trace = glyph_parser::parse_traced(source);
    let mut bytes = source.as_bytes().to_vec();
    for span in &trace.jsx_text_spans {
        for b in bytes
            .iter_mut()
            .take(span.end as usize)
            .skip(span.start as usize)
        {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    // Blanking whole bytes inside a run keeps it valid UTF-8 only when every
    // multi-byte character is blanked whole, which a run boundary guarantees.
    let Ok(blanked) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    let Ok(tokens) = glyph_lexer::tokenize(&blanked) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for window in tokens.windows(3) {
        let [name, open, body] = window else { continue };
        let glyph_lexer::Token::Identifier(n) = &name.token else { continue };
        if n.as_ref() != "extern_ts" || !matches!(open.token, glyph_lexer::Token::LParen) {
            continue;
        }
        if let glyph_lexer::Token::String(raw) = &body.token {
            out.push((
                glyph_lexer::resolve_escaped_dollars(raw).into_owned(),
                body.span,
            ));
        }
    }
    out
}

/// Every string quoted in a piece of raw TypeScript with `'`, `"` or a
/// backtick (with no `${`), which is how a module specifier is written in
/// `require('axios')`, `import("axios")` or `` import(`axios`) ``.
fn quoted_strings(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let q = chars[i];
        if q == '\'' || q == '"' || q == '`' {
            let mut j = i + 1;
            let mut s = String::new();
            let mut closed = false;
            while j < chars.len() {
                let c = chars[j];
                if c == '\\' && j + 1 < chars.len() {
                    s.push(chars[j + 1]);
                    j += 2;
                    continue;
                }
                if c == q {
                    closed = true;
                    break;
                }
                s.push(c);
                j += 1;
            }
            if closed && !(q == '`' && s.contains("${")) {
                out.push(s);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// `E0115`'s message.
pub(crate) fn banned_import_message(path: &str, under: Option<&str>, reason: Option<&str>) -> String {
    let mut msg = format!("`{path}` is banned in this project");
    if let Some(key) = under {
        msg.push_str(&format!(" (it is under `{key}`)"));
    }
    if let Some(reason) = reason {
        msg.push_str(": ");
        msg.push_str(reason);
    }
    msg
}

/// `E0115`'s help. It names the replacement and where the ban is set, and
/// deliberately not `allow_in`: telling the agent how to lift the ban defeats
/// it, and the person who owns `package.json` already knows.
pub(crate) fn banned_import_help(use_instead: &str, use_has_exports: bool) -> Cow<'static, str> {
    let tail = "The ban is set in `glyph.banned_imports` in package.json.";
    if use_has_exports {
        Cow::Owned(format!(
            "import `{use_instead}` instead; `glyph query exports --module {use_instead}` lists what it offers. {tail}"
        ))
    } else {
        Cow::Owned(format!("import `{use_instead}` instead. {tail}"))
    }
}

// ============================================================================
// E0116: reading the `glyph` key
// ============================================================================

/// One problem with the `glyph` key (`E0116`), at a byte span of the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub span: Span,
    pub message: String,
    pub help: Option<String>,
}

impl ConfigError {
    pub fn code(&self) -> &'static str {
        "E0116"
    }
}

/// What validating the `glyph` key needs to know about the project, supplied
/// by the caller because this crate does not read the filesystem.
pub struct ProjectFacts<'a> {
    /// The compiler's version, which the unknown-key message names.
    pub version: &'a str,
    /// Whether a module path (`http/client`) is a module under the source root.
    pub project_module: &'a dyn Fn(&str) -> bool,
    /// Whether a path (`migrations`) is a directory under the source root.
    pub project_dir: &'a dyn Fn(&str) -> bool,
    /// Whether a `std/` path is a module the standard library carries.
    pub std_module: &'a dyn Fn(&str) -> bool,
}

/// The `glyph` key, read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlyphConfigRead {
    pub policy: ImportPolicy,
    pub errors: Vec<ConfigError>,
}

/// The keys of `glyph` the compiler reads. `imports` and `audit` are
/// `glyph publish`'s review-currency policy (Q22).
pub const GLYPH_KEYS: [&str; 4] = ["src", "banned_imports", "imports", "audit"];
const BAN_FIELDS: [&str; 3] = ["use", "reason", "allow_in"];
pub const IMPORT_AUDIT_FIELDS: [&str; 2] = ["audit", "last_reviewed"];
pub const AUDIT_POLICY_FIELDS: [&str; 2] = ["max_age_months", "enforce"];
const AUDIT_KINDS: [&str; 3] = ["first-party", "third-party", "internal"];
const DEPENDENCY_KEYS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// Read and validate the `glyph` key of a manifest.
///
/// `None` when the text is not JSON, is not an object, or has no `glyph` key:
/// such a manifest is not a project marker (D41), so it declares no policy and
/// has nothing to report. A manifest that does not parse loses its bans with
/// it; that is filed as its own gap rather than hidden here.
pub fn read_glyph_config(text: &str, facts: &ProjectFacts<'_>) -> Option<GlyphConfigRead> {
    let root = json::parse(text)?;
    let json::Value::Object(top) = &root.value else {
        return None;
    };
    let glyph = top.iter().rev().find(|m| m.key == "glyph")?;
    let mut read = GlyphConfigRead::default();
    let json::Value::Object(members) = &glyph.value.value else {
        read.errors.push(ConfigError {
            span: glyph.value.span,
            message: format!("`glyph` must be an object, and this is {}", glyph.value.value.kind()),
            help: Some("Write `\"glyph\": {}` to mark the project, with `src` and `banned_imports` inside it when you need them.".to_string()),
        });
        return Some(read);
    };
    let declared = declared_dependencies(top);
    let mut use_spans = Vec::new();
    for m in members {
        match m.key.as_str() {
            "src" => {
                if !matches!(m.value.value, json::Value::String(_)) {
                    read.errors.push(ConfigError {
                        span: m.value.span,
                        message: format!(
                            "`glyph.src` must be a string naming the source directory, and this is {}",
                            m.value.value.kind()
                        ),
                        help: Some("Write the directory relative to package.json, as in `\"src\": \"src\"`.".to_string()),
                    });
                }
            }
            "banned_imports" => read_bans(&m.value, facts, &declared, &mut read, &mut use_spans),
            "imports" => read_import_audits(&m.value, &mut read.errors),
            "audit" => read_audit_policy(&m.value, &mut read.errors),
            other => read.errors.push(unknown_member(
                &format!("glyph.{other}"),
                other,
                m.key_span,
                &GLYPH_KEYS,
                Some(facts.version),
            )),
        }
    }
    // `use` must not itself be banned. Checked once every ban is known, so the
    // order the entries are written in does not matter.
    let bans = read.policy.bans.clone();
    for (ban, use_span) in bans.iter().zip(use_spans) {
        if let Some(by) = read.policy.covering(&ban.use_instead) {
            // Whether it resolves is beside the point once it is banned.
            read.errors.retain(|e| e.span != use_span);
            read.errors.push(ConfigError {
                span: use_span,
                message: format!(
                    "`glyph.banned_imports.{}.use` names `{}`, which is itself banned{}",
                    ban.key,
                    ban.use_instead,
                    if by.key == ban.use_instead { String::new() } else { format!(" (it is under `{}`)", by.key) },
                ),
                help: Some("Name the module that wraps the banned import, not another banned path.".to_string()),
            });
        }
    }
    Some(read)
}

fn read_bans(
    value: &json::Spanned,
    facts: &ProjectFacts<'_>,
    declared: &[String],
    read: &mut GlyphConfigRead,
    use_spans: &mut Vec<Span>,
) {
    let json::Value::Object(entries) = &value.value else {
        read.errors.push(ConfigError {
            span: value.span,
            message: format!(
                "`glyph.banned_imports` must be an object mapping an import path to its ban, and this is {}",
                value.value.kind()
            ),
            help: Some(BAN_SHAPE_HELP.to_string()),
        });
        return;
    };
    for e in entries {
        let at = format!("glyph.banned_imports.{}", e.key);
        if let Some(problem) = key_problem(&e.key) {
            read.errors.push(ConfigError {
                span: e.key_span,
                message: format!("`{at}` is not an import path: {problem}"),
                help: Some(
                    "A key is an import path as written after `import` (`axios`, `@aws-sdk/client-s3`, `std/fs`, `db/raw`), and it bans that path and every path under it.".to_string(),
                ),
            });
            continue;
        }
        let json::Value::Object(fields) = &e.value.value else {
            read.errors.push(ConfigError {
                span: e.value.span,
                message: format!("`{at}` must be an object, and this is {}", e.value.value.kind()),
                help: Some(BAN_SHAPE_HELP.to_string()),
            });
            continue;
        };
        let mut use_instead: Option<(String, bool, Span)> = None;
        let mut reason = None;
        let mut allow_in = Vec::new();
        let mut saw_use = false;
        for f in fields {
            let field_at = format!("{at}.{}", f.key);
            match f.key.as_str() {
                "use" => {
                    saw_use = true;
                    let json::Value::String(u) = &f.value.value else {
                        read.errors.push(ConfigError {
                            span: f.value.span,
                            message: format!("`{field_at}` must be a string naming the import to write instead, and this is {}", f.value.value.kind()),
                            help: None,
                        });
                        continue;
                    };
                    let is_project = (facts.project_module)(u);
                    if let Some((problem, help)) = use_problem(u, is_project, facts, declared) {
                        read.errors.push(ConfigError {
                            span: f.value.span,
                            message: format!("`{field_at}` names `{u}`, {problem}"),
                            help: Some(help.to_string()),
                        });
                    }
                    let has_exports = is_project || (u.starts_with("std/") && (facts.std_module)(u));
                    use_instead = Some((u.clone(), has_exports, f.value.span));
                }
                "reason" => match &f.value.value {
                    json::Value::String(r) if !r.contains('\n') && !r.contains('\r') => {
                        reason = Some(r.clone());
                    }
                    json::Value::String(_) => read.errors.push(ConfigError {
                        span: f.value.span,
                        message: format!("`{field_at}` must be one line"),
                        help: Some("The reason is quoted verbatim in the error, after the banned path.".to_string()),
                    }),
                    other => read.errors.push(ConfigError {
                        span: f.value.span,
                        message: format!("`{field_at}` must be a string, and this is {}", other.kind()),
                        help: None,
                    }),
                },
                "allow_in" => {
                    let json::Value::Array(items) = &f.value.value else {
                        read.errors.push(ConfigError {
                            span: f.value.span,
                            message: format!("`{field_at}` must be an array of module paths, and this is {}", f.value.value.kind()),
                            help: Some("Write `[\"storage\", \"migrations/\"]`: a module path, or a directory ending in `/`.".to_string()),
                        });
                        continue;
                    };
                    for item in items {
                        let json::Value::String(entry) = &item.value else {
                            read.errors.push(ConfigError {
                                span: item.span,
                                message: format!("`{field_at}` holds {}, where a module path belongs", item.value.kind()),
                                help: None,
                            });
                            continue;
                        };
                        if let Some((problem, help)) = allow_in_problem(entry, facts) {
                            read.errors.push(ConfigError {
                                span: item.span,
                                message: format!("`{field_at}` names `{entry}`, {problem}"),
                                help: Some(help),
                            });
                        }
                        allow_in.push(entry.clone());
                    }
                }
                other => read.errors.push(unknown_member(&field_at, other, f.key_span, &BAN_FIELDS, None)),
            }
        }
        if !saw_use {
            read.errors.push(ConfigError {
                span: e.key_span,
                message: format!("`{at}` has no `use`: a ban names the import to write instead"),
                help: Some(BAN_SHAPE_HELP.to_string()),
            });
        }
        if let Some((use_instead, use_has_exports, use_span)) = use_instead {
            use_spans.push(use_span);
            read.policy.bans.push(ImportBan {
                key: e.key.clone(),
                use_instead,
                use_has_exports,
                reason,
                allow_in,
            });
        }
    }
}

const BAN_SHAPE_HELP: &str = "A ban is `\"axios\": { \"use\": \"http/client\", \"reason\": \"...\", \"allow_in\": [\"...\"] }`, where `use` is required.";

/// Why a ban key is not an import path, or `None` when it is one.
fn key_problem(key: &str) -> Option<&'static str> {
    if key.is_empty() {
        Some("it is empty")
    } else if key.ends_with('/') {
        Some("it ends in `/`, and a key already bans every path under it")
    } else if key.contains(' ') {
        Some("it contains a space")
    } else if key.contains('*') {
        Some("it contains `*`, and a key already bans every path under it")
    } else {
        None
    }
}

/// Why `use` does not name something importable, with the help for it, or
/// `None`.
fn use_problem(
    u: &str,
    is_project: bool,
    facts: &ProjectFacts<'_>,
    declared: &[String],
) -> Option<(&'static str, &'static str)> {
    if u.starts_with("std/") {
        return (!(facts.std_module)(u)).then_some((
            "which is not a module of the standard library",
            "`glyph llms --json` lists the `std/` modules in its `stdlib` section.",
        ));
    }
    if is_project || declared.iter().any(|d| d == package_name(u)) {
        return None;
    }
    // Both readings the rule allows are named, so an agent is not sent to
    // write a module called `luxon` when the package was meant (record 205).
    Some((
        "which is neither a module of this project nor a package declared in package.json's dependencies",
        "Write the path of a module under the source root (`http/client` for `src/http/client.glyph`), or add the package to package.json's `dependencies` (it does not have to be installed).",
    ))
}

/// Why an `allow_in` entry names nothing, with the help for it.
fn allow_in_problem(entry: &str, facts: &ProjectFacts<'_>) -> Option<(&'static str, String)> {
    if let Some(dir) = entry.strip_suffix('/') {
        if dir.is_empty() || !(facts.project_dir)(dir) {
            return Some((
                "which is not a directory under the source root",
                "An entry ending in `/` names a directory under the source root, and covers every module below it.".to_string(),
            ));
        }
        return None;
    }
    if (facts.project_module)(entry) {
        return None;
    }
    let help = if (facts.project_dir)(entry) {
        format!("`{entry}` is a directory: write `{entry}/` to allow every module under it.")
    } else {
        "An entry is a module path under the source root (`storage` for `src/storage.glyph`), or a directory ending in `/`.".to_string()
    };
    Some(("which is not a module of this project", help))
}

/// The npm package an import path names: `@scope/name` for a scoped path, the
/// first segment otherwise.
fn package_name(path: &str) -> &str {
    let mut cut = path.match_indices('/');
    let end = if path.starts_with('@') {
        cut.nth(1).map(|(i, _)| i)
    } else {
        cut.next().map(|(i, _)| i)
    };
    &path[..end.unwrap_or(path.len())]
}

fn declared_dependencies(top: &[json::Member]) -> Vec<String> {
    let mut out = Vec::new();
    for m in top.iter().filter(|m| DEPENDENCY_KEYS.contains(&m.key.as_str())) {
        if let json::Value::Object(deps) = &m.value.value {
            out.extend(deps.iter().map(|d| d.key.clone()));
        }
    }
    out
}

fn read_import_audits(value: &json::Spanned, errors: &mut Vec<ConfigError>) {
    let json::Value::Object(entries) = &value.value else {
        errors.push(ConfigError {
            span: value.span,
            message: format!("`glyph.imports` must be an object, and this is {}", value.value.kind()),
            help: None,
        });
        return;
    };
    for e in entries {
        let at = format!("glyph.imports.{}", e.key);
        let json::Value::Object(fields) = &e.value.value else {
            errors.push(ConfigError {
                span: e.value.span,
                message: format!("`{at}` must be an object, and this is {}", e.value.value.kind()),
                help: None,
            });
            continue;
        };
        for f in fields {
            let field_at = format!("{at}.{}", f.key);
            match f.key.as_str() {
                "audit" => match &f.value.value {
                    json::Value::String(s) if AUDIT_KINDS.contains(&s.as_str()) => {}
                    other => errors.push(ConfigError {
                        span: f.value.span,
                        message: format!(
                            "`{field_at}` must be one of `first-party`, `third-party`, `internal`, and this is {}",
                            other.kind()
                        ),
                        help: None,
                    }),
                },
                "last_reviewed" => {
                    if !matches!(f.value.value, json::Value::String(_)) {
                        errors.push(ConfigError {
                            span: f.value.span,
                            message: format!("`{field_at}` must be a `YYYY-MM-DD` string, and this is {}", f.value.value.kind()),
                            help: None,
                        });
                    }
                }
                other => errors.push(unknown_member(&field_at, other, f.key_span, &IMPORT_AUDIT_FIELDS, None)),
            }
        }
    }
}

fn read_audit_policy(value: &json::Spanned, errors: &mut Vec<ConfigError>) {
    let json::Value::Object(fields) = &value.value else {
        errors.push(ConfigError {
            span: value.span,
            message: format!("`glyph.audit` must be an object, and this is {}", value.value.kind()),
            help: None,
        });
        return;
    };
    for f in fields {
        let field_at = format!("glyph.audit.{}", f.key);
        match f.key.as_str() {
            "max_age_months" => {
                let whole = matches!(&f.value.value, json::Value::Number(n) if n.parse::<u32>().is_ok());
                if !whole {
                    errors.push(ConfigError {
                        span: f.value.span,
                        message: format!("`{field_at}` must be a whole number of months, and this is {}", f.value.value.kind()),
                        help: None,
                    });
                }
            }
            "enforce" => {
                if !matches!(f.value.value, json::Value::Bool(_)) {
                    errors.push(ConfigError {
                        span: f.value.span,
                        message: format!("`{field_at}` must be `true` or `false`, and this is {}", f.value.value.kind()),
                        help: None,
                    });
                }
            }
            other => errors.push(unknown_member(&field_at, other, f.key_span, &AUDIT_POLICY_FIELDS, None)),
        }
    }
}

/// The error for a key or field the schema does not have, with the nearest
/// known name when it is close enough to be a typo.
fn unknown_member(
    at: &str,
    name: &str,
    span: Span,
    known: &[&str],
    version: Option<&str>,
) -> ConfigError {
    let nearest = known
        .iter()
        .map(|k| (crate::error::edit_distance(name, k), *k))
        .filter(|(d, k)| *d <= (k.len() / 3).max(1) + 1)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k);
    let listed = known.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
    let mut message = match version {
        Some(v) => format!("`{at}` is not a key glyph {v} reads (known: {listed})"),
        None => format!("`{at}` is not a field Glyph reads (known: {listed})"),
    };
    if let Some(k) = nearest {
        message.push_str(&format!("; did you mean `{k}`?"));
    }
    ConfigError {
        span,
        message,
        help: Some(
            "The `glyph` key is a closed schema: a misspelled key would otherwise switch a setting off without a word.".to_string(),
        ),
    }
}

// ============================================================================
// A JSON reader that keeps spans
// ============================================================================

/// The smallest JSON reader that keeps byte spans, which is all E0116 needs
/// from it. It accepts exactly RFC 8259 JSON: no comments, no trailing comma.
pub(crate) mod json {
    use glyph_ast::Span;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Number(String),
        String(String),
        Array(Vec<Spanned>),
        Object(Vec<Member>),
    }

    impl Value {
        /// The value's kind with an article, for a message.
        pub fn kind(&self) -> &'static str {
            match self {
                Value::Null => "null",
                Value::Bool(_) => "a boolean",
                Value::Number(_) => "a number",
                Value::String(_) => "a string",
                Value::Array(_) => "an array",
                Value::Object(_) => "an object",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct Spanned {
        pub value: Value,
        pub span: Span,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct Member {
        pub key: String,
        pub key_span: Span,
        pub value: Spanned,
    }

    pub fn parse(text: &str) -> Option<Spanned> {
        let mut p = Parser { s: text.as_bytes(), text, i: 0 };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        (p.i == p.s.len()).then_some(v)
    }

    struct Parser<'a> {
        s: &'a [u8],
        text: &'a str,
        i: usize,
    }

    /// Deeper than any manifest, shallow enough that a hostile file cannot
    /// overflow the stack.
    const MAX_DEPTH: usize = 128;

    impl Parser<'_> {
        fn ws(&mut self) {
            while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.i += 1;
            }
        }

        fn eat(&mut self, b: u8) -> bool {
            if self.s.get(self.i) == Some(&b) {
                self.i += 1;
                true
            } else {
                false
            }
        }

        fn value(&mut self, depth: usize) -> Option<Spanned> {
            if depth > MAX_DEPTH {
                return None;
            }
            let start = self.i;
            let value = match *self.s.get(self.i)? {
                b'{' => self.object(depth)?,
                b'[' => self.array(depth)?,
                b'"' => Value::String(self.string()?),
                b't' => self.literal("true", Value::Bool(true))?,
                b'f' => self.literal("false", Value::Bool(false))?,
                b'n' => self.literal("null", Value::Null)?,
                b'-' | b'0'..=b'9' => self.number()?,
                _ => return None,
            };
            Some(Spanned { value, span: Span::new(start as u32, self.i as u32) })
        }

        fn literal(&mut self, word: &str, v: Value) -> Option<Value> {
            if self.s[self.i..].starts_with(word.as_bytes()) {
                self.i += word.len();
                Some(v)
            } else {
                None
            }
        }

        fn number(&mut self) -> Option<Value> {
            let start = self.i;
            self.eat(b'-');
            match self.s.get(self.i)? {
                b'0' => self.i += 1,
                b'1'..=b'9' => {
                    while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                        self.i += 1;
                    }
                }
                _ => return None,
            }
            if self.eat(b'.') {
                let digits = self.i;
                while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
                if self.i == digits {
                    return None;
                }
            }
            if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
                self.i += 1;
                if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                    self.i += 1;
                }
                let digits = self.i;
                while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
                if self.i == digits {
                    return None;
                }
            }
            Some(Value::Number(self.text[start..self.i].to_string()))
        }

        fn hex4(&mut self) -> Option<u32> {
            let h = self.text.get(self.i..self.i + 4)?;
            let v = u32::from_str_radix(h, 16).ok()?;
            self.i += 4;
            Some(v)
        }

        fn string(&mut self) -> Option<String> {
            self.i += 1; // opening quote
            let mut out = String::new();
            loop {
                let b = *self.s.get(self.i)?;
                match b {
                    b'"' => {
                        self.i += 1;
                        return Some(out);
                    }
                    b'\\' => {
                        self.i += 1;
                        let e = *self.s.get(self.i)?;
                        self.i += 1;
                        match e {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let hi = self.hex4()?;
                                let c = if (0xD800..0xDC00).contains(&hi) {
                                    if !(self.eat(b'\\') && self.eat(b'u')) {
                                        return None;
                                    }
                                    let lo = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&lo) {
                                        return None;
                                    }
                                    char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))?
                                } else {
                                    char::from_u32(hi)?
                                };
                                out.push(c);
                            }
                            _ => return None,
                        }
                    }
                    0x00..=0x1f => return None,
                    _ => {
                        // Copy one whole UTF-8 character.
                        let ch = self.text[self.i..].chars().next()?;
                        out.push(ch);
                        self.i += ch.len_utf8();
                    }
                }
            }
        }

        fn array(&mut self, depth: usize) -> Option<Value> {
            self.i += 1;
            let mut items = Vec::new();
            self.ws();
            if self.eat(b']') {
                return Some(Value::Array(items));
            }
            loop {
                self.ws();
                items.push(self.value(depth + 1)?);
                self.ws();
                if self.eat(b',') {
                    continue;
                }
                if self.eat(b']') {
                    return Some(Value::Array(items));
                }
                return None;
            }
        }

        fn object(&mut self, depth: usize) -> Option<Value> {
            self.i += 1;
            let mut members = Vec::new();
            self.ws();
            if self.eat(b'}') {
                return Some(Value::Object(members));
            }
            loop {
                self.ws();
                if self.s.get(self.i) != Some(&b'"') {
                    return None;
                }
                let key_start = self.i;
                let key = self.string()?;
                let key_span = Span::new(key_start as u32, self.i as u32);
                self.ws();
                if !self.eat(b':') {
                    return None;
                }
                self.ws();
                let value = self.value(depth + 1)?;
                members.push(Member { key, key_span, value });
                self.ws();
                if self.eat(b',') {
                    continue;
                }
                if self.eat(b'}') {
                    return Some(Value::Object(members));
                }
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(
        modules: &'a [&'a str],
        dirs: &'a [&'a str],
    ) -> (impl Fn(&str) -> bool + 'a, impl Fn(&str) -> bool + 'a) {
        (move |m: &str| modules.contains(&m), move |d: &str| dirs.contains(&d))
    }

    fn read(text: &str) -> GlyphConfigRead {
        let (module, dir) = facts(&["storage", "http/client", "migrations/m1"], &["http", "migrations"]);
        let std_module = |m: &str| m == "std/fs" || m == "std/io";
        let f = ProjectFacts {
            version: "0.1.187",
            project_module: &module,
            project_dir: &dir,
            std_module: &std_module,
        };
        read_glyph_config(text, &f).expect("a marker")
    }

    const FIXTURE: &str = r#"{
  "name": "p",
  "glyph": {
    "src": "src",
    "banned_imports": {
      "axios":  { "use": "http/client", "reason": "requests go through the retry and auth wrapper" },
      "std/fs": { "use": "storage", "allow_in": ["storage", "migrations/"] }
    }
  }
}"#;

    #[test]
    fn the_record_fixture_reads_clean() {
        let r = read(FIXTURE);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.policy.bans.len(), 2);
        let axios = &r.policy.bans[0];
        assert_eq!(axios.use_instead, "http/client");
        assert!(axios.use_has_exports);
        assert!(axios.covers("axios") && axios.covers("axios/lib/core"));
        assert!(!axios.covers("axios-retry"));
        let fs = &r.policy.bans[1];
        assert!(fs.allows("storage") && fs.allows("migrations/m1") && fs.allows("migrations/a/b"));
        assert!(!fs.allows("main") && !fs.allows("migrations"));
    }

    fn one_error(text: &str) -> ConfigError {
        let r = read(text);
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        r.errors[0].clone()
    }

    #[test]
    fn a_misspelled_field_is_named_with_its_path_and_the_nearest_field() {
        let text = FIXTURE.replace("\"allow_in\"", "\"alow_in\"");
        let e = one_error(&text);
        assert_eq!(
            e.message,
            "`glyph.banned_imports.std/fs.alow_in` is not a field Glyph reads (known: `use`, `reason`, `allow_in`); did you mean `allow_in`?"
        );
        assert_eq!(&text[e.span.start as usize..e.span.end as usize], "\"alow_in\"");
    }

    #[test]
    fn a_misspelled_key_names_the_version_and_the_known_keys() {
        let text = FIXTURE.replace("\"banned_imports\"", "\"banned_import\"");
        let e = one_error(&text);
        assert_eq!(
            e.message,
            "`glyph.banned_import` is not a key glyph 0.1.187 reads (known: `src`, `banned_imports`, `imports`, `audit`); did you mean `banned_imports`?"
        );
    }

    #[test]
    fn a_use_naming_no_module_is_refused_at_the_value() {
        let text = FIXTURE.replace("\"http/client\"", "\"http/clinet\"");
        let e = one_error(&text);
        assert_eq!(
            e.message,
            "`glyph.banned_imports.axios.use` names `http/clinet`, which is neither a module of this project nor a package declared in package.json's dependencies"
        );
        assert_eq!(&text[e.span.start as usize..e.span.end as usize], "\"http/clinet\"");
    }

    #[test]
    fn a_use_naming_a_declared_dependency_is_not_resolved() {
        let text = FIXTURE
            .replace("\"http/client\"", "\"ky\"")
            .replace("\"name\": \"p\",", "\"name\": \"p\", \"dependencies\": { \"ky\": \"^1\" },");
        assert!(read(&text).errors.is_empty());
    }

    #[test]
    fn an_allow_in_entry_naming_nothing_is_refused() {
        let text = FIXTURE.replace("\"migrations/\"", "\"nope\"");
        let e = one_error(&text);
        assert_eq!(
            e.message,
            "`glyph.banned_imports.std/fs.allow_in` names `nope`, which is not a module of this project"
        );
        let text = FIXTURE.replace("\"migrations/\"", "\"migrations\"");
        let e = one_error(&text);
        assert_eq!(e.help.as_deref(), Some("`migrations` is a directory: write `migrations/` to allow every module under it."));
    }

    #[test]
    fn a_use_that_is_itself_banned_is_refused() {
        let text = FIXTURE.replace("\"storage\", \"allow_in\"", "\"axios/wrapped\", \"allow_in\"");
        let e = one_error(&text);
        assert_eq!(
            e.message,
            "`glyph.banned_imports.std/fs.use` names `axios/wrapped`, which is itself banned (it is under `axios`)"
        );
    }

    #[test]
    fn bad_keys_and_a_bad_src_are_refused() {
        for key in ["", "axios/", "ax ios", "axios*"] {
            let text = FIXTURE.replace("\"axios\":", &format!("\"{key}\":"));
            let r = read(&text);
            assert!(r.errors.iter().any(|e| e.message.contains("is not an import path")), "{key}: {:?}", r.errors);
        }
        let e = one_error(&FIXTURE.replace("\"src\": \"src\"", "\"src\": 5"));
        assert_eq!(e.message, "`glyph.src` must be a string naming the source directory, and this is a number");
    }

    #[test]
    fn unknown_keys_anywhere_in_glyph_are_refused_and_publish_keys_are_known() {
        let e = one_error(r#"{"glyph": {"src": "src", "bogus": 1}}"#);
        assert!(e.message.starts_with("`glyph.bogus` is not a key glyph"), "{}", e.message);
        let r = read(r#"{"glyph": {"imports": {"vendor/x": {"audit": "third-party", "last_reviewed": "2026-01-01"}}, "audit": {"max_age_months": 6, "enforce": true}}}"#);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let e = one_error(r#"{"glyph": {"audit": {"max_age_month": 6}}}"#);
        assert!(e.message.contains("did you mean `max_age_months`?"), "{}", e.message);
    }

    #[test]
    fn a_manifest_that_does_not_parse_or_carries_no_glyph_key_is_not_read() {
        let (module, dir) = facts(&[], &[]);
        let std_module = |_: &str| false;
        let f = ProjectFacts { version: "x", project_module: &module, project_dir: &dir, std_module: &std_module };
        assert!(read_glyph_config("{ \"glyph\": {}, }", &f).is_none());
        assert!(read_glyph_config("{ \"name\": \"x\" }", &f).is_none());
        assert!(read_glyph_config("{ \"glyph\": {} }", &f).is_some());
    }

    #[test]
    fn json_strings_decode_escapes_and_keep_spans() {
        let v = json::parse("{\"a\\u00e9\\\"\": [1, -2.5e3, true, null, \"\\ud83d\\ude00\"]}").expect("parses");
        let json::Value::Object(m) = v.value else { panic!() };
        assert_eq!(m[0].key, "aé\"");
        assert_eq!(m[0].key_span, Span::new(1, 12));
        assert!(json::parse("[1,]").is_none());
        assert!(json::parse("01").is_none());
    }

    fn policy() -> ImportPolicy {
        read(FIXTURE).policy
    }

    fn codes(src: &str, module_path: &str) -> Vec<String> {
        let m = glyph_parser::parse(src).expect("parses");
        verify_banned_imports(&m, src, module_path, &policy())
            .iter()
            .map(|e| e.to_string())
            .collect()
    }

    #[test]
    fn a_banned_import_is_refused_outside_its_allowed_modules() {
        let src = "module main\nimport std/fs\npub fn main() {}\n";
        assert_eq!(codes(src, "main"), vec!["`std/fs` is banned in this project".to_string()]);
        assert!(codes(src, "storage").is_empty());
        assert!(codes(src, "migrations/m1").is_empty());
        let sub = "module main\nimport axios/lib/core { x }\npub fn main() {}\n";
        assert_eq!(
            codes(sub, "main"),
            vec!["`axios/lib/core` is banned in this project (it is under `axios`): requests go through the retry and auth wrapper".to_string()]
        );
        assert!(codes("module main\nimport axios-retry\npub fn main() {}\n", "main").is_empty());
    }

    #[test]
    fn the_error_sits_on_the_import_path() {
        let src = "module main\nimport   axios { default as ax }\npub fn main() {}\n";
        let m = glyph_parser::parse(src).expect("parses");
        let errs = verify_banned_imports(&m, src, "main", &policy());
        let s = errs[0].span();
        assert_eq!(&src[s.start as usize..s.end as usize], "axios");
    }

    #[test]
    fn a_banned_specifier_in_an_extern_ts_body_is_refused_and_a_comment_is_not() {
        let src = "module main\n// extern_ts(\"require('axios')\")\npub fn main() {\n  let s = \"extern_ts('axios')\"\n  let ax = extern_ts(\"require('axios')\")\n  let t: extern_ts(\"typeof import(\\\"axios/lib/core\\\")\") = ax\n}\n";
        let found = codes(src, "main");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("`axios` is banned"));
        assert!(found[1].starts_with("`axios/lib/core` is banned"));
        assert!(codes(src, "http/client").is_empty());
    }

    #[test]
    fn the_help_names_the_replacement_and_never_allow_in() {
        let h = banned_import_help("http/client", true);
        assert_eq!(
            h,
            "import `http/client` instead; `glyph query exports --module http/client` lists what it offers. The ban is set in `glyph.banned_imports` in package.json."
        );
        assert!(!h.contains("allow_in"));
    }
}
