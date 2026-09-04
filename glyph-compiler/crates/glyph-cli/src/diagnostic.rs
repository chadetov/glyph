//! Structured diagnostics for `--json`.
//!
//! The text pipeline renders each diagnostic to an ariadne report string. For
//! agents (and any tool) consuming Glyph's output, `--json` emits the same
//! diagnostics as structured data instead: a stable code, severity, message,
//! file, and a 1-based line/column range, plus the help and note. The build's
//! own diagnostics and the remapped `tsc` errors flow through the same shape.
//! The shape round-trips (it deserializes as well as serializes) because
//! `glyph run` caches a build's diagnostics beside its output in this format.
//!
//! A resolve/typecheck/emit diagnostic that lands inside a top-level
//! declaration also carries `entity`, the same `module::name` identity
//! `glyph_variants` reports for a match site over that declaration (0.1.107).
//! An agent can then act on a batch of `--json` diagnostics by entity without
//! re-deriving "which function is this" from a line number, which shifts
//! under every unrelated edit above the site. A diagnostic from a stage that
//! has no parsed module to look the entity up in (a parse failure, or a `tsc`
//! error that could not be remapped onto Glyph at all) carries no entity
//! rather than a guess.

use serde::{Deserialize, Serialize};

use glyph_ast::Span;
use glyph_emit::EmitError;
use glyph_parser::ParseError;
use glyph_resolver::ResolveError;
use glyph_typechecker::{DiagnosticUnion, Severity, TypeError};

/// One structured diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    /// `"error"` or `"warning"`.
    pub severity: String,
    pub message: String,
    pub file: String,
    pub range: Range,
    /// The compiler stage (`parse`/`resolve`/`typecheck`/`emit`/`tsc`).
    pub stage: String,
    /// The `module::name` identity of the top-level declaration this
    /// diagnostic belongs to, when one is known: the name the checker carried
    /// on the error (`TypeError::decl_name`) when it had the declaration in
    /// hand, otherwise the declaration whose span contains the diagnostic
    /// (see `entity_id`).
    ///
    /// Always present, as an explicit `null` when there is none, so a consumer
    /// reads this field the same way here and from the `glyph_diagnostics` MCP
    /// tool instead of testing for a missing key on one surface and a null on
    /// the other (G184). `default` keeps a cache written before the field
    /// existed readable.
    #[serde(default)]
    pub entity: Option<String>,
    /// The union this diagnostic is about, when it is about one: the
    /// exhaustiveness errors, which name a union and a set of its variants in
    /// their message. An agent repairing a non-exhaustive match needs the
    /// union's name to ask which other sites match on it, and reading that
    /// back out of the sentence is a regex over prose the compiler is free to
    /// rewrite (G195).
    ///
    /// Always present, as an explicit `null` when the diagnostic is about no
    /// union, for the same reason `entity` is (G184): one spelling of absence.
    #[serde(default)]
    pub union: Option<UnionEntity>,
    /// The variants the match leaves unmentioned, in declaration order and
    /// unquoted. Explicit `null` on a diagnostic that reports no such set,
    /// never an empty list, which would say "none are missing".
    #[serde(default)]
    pub missing_variants: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The union a diagnostic concerns, addressed rather than described.
///
/// `name` is what `glyph_variants` takes; `declaration` is the `module::name`
/// identity that tool reports back for the same declaration, so an answer and
/// a diagnostic can be cross-referenced. The shape is the one the MCP tools
/// already report a match-coverage type end under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnionEntity {
    /// `"declaration"` for a union declared in a project module,
    /// `"builtin"` for a prelude or stdlib one.
    pub kind: String,
    /// The module the union is declared in, counted from the same root the
    /// diagnostic's `entity` is counted from, so one declaration has one
    /// spelling within a diagnostic. `null` for a builtin.
    pub module: Option<String>,
    pub name: String,
    /// `module::name`. `null` for a builtin: a key invented for `Result`
    /// would name a module no project has.
    pub declaration: Option<String>,
}

impl UnionEntity {
    /// Qualify a union the checker named, given `this_module`: the module half
    /// of the file the diagnostic is on. A union declared in that file carries
    /// no module of its own on the error, precisely so this is the string that
    /// fills it in.
    pub fn new(union: &DiagnosticUnion, this_module: &str) -> Self {
        UnionEntity {
            kind: union.kind().to_string(),
            module: union.module(this_module).map(str::to_string),
            name: union.name().to_string(),
            declaration: union.declaration(this_module),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

/// A source position: 1-based `line`/`col` plus the byte `offset`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
    pub offset: u32,
}

impl Diagnostic {
    /// Build a diagnostic, computing line/col for the span's endpoints from
    /// `source`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        file: &str,
        source: &str,
        span: Span,
        code: &str,
        severity: &str,
        stage: &str,
        message: String,
        help: Option<&str>,
        note: Option<&str>,
        entity: Option<String>,
    ) -> Self {
        Diagnostic {
            code: code.to_string(),
            severity: severity.to_string(),
            message,
            file: file.to_string(),
            range: Range {
                start: pos_of(source, span.start),
                end: pos_of(source, span.end),
            },
            stage: stage.to_string(),
            entity,
            union: None,
            missing_variants: None,
            help: help.map(str::to_string),
            note: note.map(str::to_string),
        }
    }

    /// Attach the union this diagnostic concerns and the variants it reports
    /// unmentioned. Separate from `new` because most diagnostics are about no
    /// union at all, and because an argument list this long is already the
    /// wrong shape to add two more to.
    fn about_union(
        mut self,
        union: Option<UnionEntity>,
        missing_variants: Option<Vec<String>>,
    ) -> Self {
        self.union = union;
        self.missing_variants = missing_variants;
        self
    }
}

/// 1-based line/column (and the byte offset) of `offset` within `source`.
pub fn pos_of(source: &str, offset: u32) -> Pos {
    let clamped = (offset as usize).min(source.len());
    let mut line = 1u32;
    let mut last_line_start = 0usize;
    for (i, b) in source.as_bytes().iter().enumerate() {
        if i >= clamped {
            break;
        }
        if *b == b'\n' {
            line += 1;
            last_line_start = i + 1;
        }
    }
    // Column counts characters, not bytes, from the line start.
    let col = source[last_line_start..clamped].chars().count() as u32 + 1;
    Pos { line, col, offset }
}

pub fn from_parse_error(file: &str, source: &str, err: &ParseError) -> Diagnostic {
    // No `entity`: a file that failed to parse has no declaration table to
    // look one up in.
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        "error",
        "parse",
        format!("{err}"),
        err.help().as_deref(),
        None,
        None,
    )
}

pub fn from_resolve_error(
    file: &str,
    source: &str,
    err: &ResolveError,
    stage: &str,
    module: &glyph_ast::Module,
) -> Diagnostic {
    let severity = match err.severity() {
        glyph_resolver::Severity::Warning => "warning",
        glyph_resolver::Severity::Error => "error",
    };
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        severity,
        stage,
        format!("{err}"),
        err.help(),
        None,
        entity_id(file, module, err.span().start),
    )
}

pub fn from_type_error(
    file: &str,
    source: &str,
    err: &TypeError,
    module: &glyph_ast::Module,
) -> Diagnostic {
    let severity = match err.severity() {
        Severity::Warning => "warning",
        Severity::Error => "error",
    };
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        severity,
        "typecheck",
        format!("{err}"),
        err.help(),
        err.note(),
        // A checker that knew the declaration when it raised the error says
        // so on the error itself (`decl_name`), because some diagnostics are
        // reported at a span no declaration contains: an annotation is parsed
        // before the keyword a `Decl` span starts at, so the walk below would
        // report "no declaration here" for one. The walk answers the rest.
        err.decl_name()
            .map(|name| format!("{file}::{name}"))
            .or_else(|| entity_id(file, module, err.span().start)),
    )
    // The other entity an exhaustiveness error concerns: the union itself,
    // and the variants it leaves unmentioned. Qualified with `file`, the same
    // module string `entity` above is qualified with, so one declaration has
    // one spelling inside one diagnostic.
    .about_union(
        err.union().map(|u| UnionEntity::new(u, file)),
        err.missing_variants().map(<[String]>::to_vec),
    )
}

/// The `module::name` identity of the top-level declaration enclosing byte
/// `offset`, in the same spelling `glyph_variants` reports for a match site
/// over the same declaration (see `glyph-lsp/src/mcp.rs`'s `render_key`).
/// `None` when no top-level declaration contains the offset (a diagnostic on
/// the `module` line itself, or on an import).
///
/// The walk itself is `glyph_lsp::enclosing_decl_name`, the one copy of the
/// attribution rule, shared with the `glyph_diagnostics` MCP tool. Only the
/// module half is assembled here, because only the caller knows the root it is
/// counted from: `module_path` arrives already stripped of the project's `src`
/// root by `build::derive_module_path` (G180).
///
/// Without this, an agent acting on a `--json` diagnostic has to re-derive
/// "which function is this" from a line number, and a line number shifts
/// under every unrelated edit above the site — exactly the identity-loss
/// problem the `module::name` convention exists to prevent elsewhere.
pub fn entity_id(module_path: &str, module: &glyph_ast::Module, offset: u32) -> Option<String> {
    glyph_lsp::enclosing_decl_name(module, offset).map(|name| format!("{module_path}::{name}"))
}

pub fn from_emit_error(
    file: &str,
    source: &str,
    err: &EmitError,
    module: &glyph_ast::Module,
) -> Diagnostic {
    Diagnostic::new(
        file,
        source,
        err.span(),
        err.code(),
        "error",
        "emit",
        format!("{err}"),
        err.help(),
        err.note(),
        entity_id(file, module, err.span().start),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_of_computes_line_and_col() {
        let src = "abc\ndef\nghij";
        // offset 0 -> 1:1
        let p = pos_of(src, 0);
        assert_eq!((p.line, p.col), (1, 1));
        // offset of 'e' (index 5) -> line 2, col 2
        let p = pos_of(src, 5);
        assert_eq!((p.line, p.col), (2, 2));
        // offset of 'g' (index 8) -> line 3, col 1
        let p = pos_of(src, 8);
        assert_eq!((p.line, p.col), (3, 1));
    }

    /// Regression guard for the has_catch_all bug shape (0.1.107): a `--json`
    /// diagnostic on a non-exhaustive match must be attributable to the exact
    /// function it sits in, not just "the file", so an agent can act on a
    /// batch of these without re-parsing every site to find out which
    /// function each line number currently belongs to. The identity must also
    /// agree with what `glyph_variants` reports for the same declaration
    /// (`module::name`), so the two answers can be cross-referenced.
    #[test]
    fn entity_id_names_the_enclosing_declaration() {
        let src = "module main\n\n\
            fn describe_exhaustive(s: string) -> string {\n    s\n}\n\n\
            fn describe_catchall(s: string) -> string {\n    s\n}\n";
        let module = glyph_parser::parse(src).expect("fixture parses");

        let exhaustive_offset = src.find("describe_exhaustive").unwrap() as u32;
        assert_eq!(
            entity_id("main", &module, exhaustive_offset),
            Some("main::describe_exhaustive".to_string())
        );

        // A second declaration in the same file must not be misattributed to
        // the first just because it comes later.
        let catchall_offset = src.find("describe_catchall").unwrap() as u32;
        assert_eq!(
            entity_id("main", &module, catchall_offset),
            Some("main::describe_catchall".to_string())
        );

        // An offset before any declaration (the `module` line) names nothing.
        assert_eq!(entity_id("main", &module, 0), None);
    }

    /// G184: absence has one spelling. `entity` is always present, as an
    /// explicit `null` when the diagnostic has no enclosing declaration to
    /// name, so a consumer of `glyph check --json` and of the
    /// `glyph_diagnostics` MCP tool can read the field the same way instead of
    /// testing for a missing key on one surface and a null on the other.
    #[test]
    fn an_absent_entity_is_an_explicit_null() {
        let d = Diagnostic::new(
            "main.glyph",
            "module main\n",
            Span::new(0, 6),
            "E0200",
            "error",
            "typecheck",
            "boom".to_string(),
            None,
            None,
            None,
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"entity\":null"), "{json}");
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entity, None);

        // A cached build written before the key existed still reads back:
        // `glyph run` keeps a build's diagnostics beside its output in this
        // shape, and an old cache must not become unreadable.
        let old = r#"{"code":"E0200","severity":"error","message":"boom","file":"main.glyph","range":{"start":{"line":1,"col":1,"offset":0},"end":{"line":1,"col":7,"offset":6}},"stage":"typecheck"}"#;
        let parsed: Diagnostic = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.entity, None);
    }

    #[test]
    fn serializes_without_empty_optionals() {
        let d = Diagnostic::new(
            "main.glyph",
            "module main\n",
            Span::new(0, 6),
            "E0200",
            "error",
            "typecheck",
            "boom".to_string(),
            None,
            None,
            None,
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"code\":\"E0200\""), "{json}");
        assert!(json.contains("\"severity\":\"error\""), "{json}");
        assert!(!json.contains("help"), "no help key when None: {json}");
    }

    /// G181: an annotation's text sits in the gap between two declaration
    /// spans. `parse_annotations` runs before the keyword and every `Decl`
    /// span starts *at* the keyword, so a containment walk over top-level
    /// items matches nothing for an offset inside `@puer`. Both annotation
    /// diagnostics are emitted while the checker holds the declaration by
    /// reference, so the relation is known at the emission site; dropping it
    /// would make the answer read as "there is no declaration here", which is
    /// the one thing the exact-or-absent rule forbids.
    #[test]
    fn entity_names_the_declaration_an_annotation_decorates() {
        let src = "module main\n\n\
            @puer\n\
            fn f() -> number {\n    return 1\n}\n\n\
            @redact fields: [emial]\n\
            type Account = {\n    email: string,\n}\n";
        let module = glyph_parser::parse(src).expect("fixture parses");
        let symbols = glyph_resolver::collect_module_symbols(&module).expect("symbols collect");
        let prelude = glyph_resolver::build_prelude();
        let (resolved, _) = glyph_resolver::resolve_module(&module, symbols, &prelude);
        let (_types, errors) = glyph_typechecker::assign_types(&module, &resolved, &prelude);

        let unknown = errors
            .iter()
            .find(|e| e.code() == "E0221")
            .expect("E0221 emitted");
        assert_eq!(
            from_type_error("main", src, unknown, &module).entity.as_deref(),
            Some("main::f"),
            "an unknown annotation belongs to the declaration it decorates"
        );

        let redact = errors
            .iter()
            .find(|e| e.code() == "E0219")
            .expect("E0219 emitted");
        assert_eq!(
            from_type_error("main", src, redact, &module).entity.as_deref(),
            Some("main::Account"),
            "a bad `@redact` field belongs to the type it decorates"
        );
    }

    /// G195: the loop that repairs a non-exhaustive match is grep-free at
    /// every hop but the first. An agent gets E0200, and to ask which other
    /// sites match on the same union it needs the union's name and the
    /// variants it is missing. Both were in the sentence and nowhere else, so
    /// the only way out was a regex over prose the compiler is free to
    /// rewrite. Both are fields now, and the sentence is untouched.
    #[test]
    fn a_non_exhaustive_match_carries_its_union_and_its_missing_variants() {
        let src = "module billing\n\n\
            type PaymentResult =\n  | Settled\n  | Failed\n  | Pending\n\n\
            fn settle(r: PaymentResult) -> string {\n\
            \x20 return match r {\n    Settled => \"s\",\n    Failed => \"f\",\n  }\n\
            }\n";
        let d = e0200_of("billing", src);

        let union = d.union.as_ref().expect("E0200 names a union");
        assert_eq!(union.kind, "declaration");
        assert_eq!(union.name, "PaymentResult");
        assert_eq!(union.module.as_deref(), Some("billing"));
        assert_eq!(union.declaration.as_deref(), Some("billing::PaymentResult"));
        assert_eq!(d.missing_variants.as_deref(), Some(["Pending".to_string()].as_slice()));

        // The message is what it always was. This adds fields beside it.
        assert_eq!(
            d.message,
            "non-exhaustive match on `PaymentResult`: missing variants `Pending`"
        );
    }

    /// One declaration, one spelling. The union's module half and the
    /// enclosing declaration's are counted from the same root inside one
    /// diagnostic, which is the whole point of leaving the local module to the
    /// surface rather than taking the file's own `module` header (G172).
    #[test]
    fn the_union_and_the_entity_are_counted_from_the_same_root() {
        let src = "module billing\n\n\
            type PaymentResult =\n  | Settled\n  | Pending\n\n\
            fn settle(r: PaymentResult) -> string {\n\
            \x20 return match r {\n    Settled => \"s\",\n  }\n\
            }\n";
        // The module half `glyph build` passes is the project-relative path,
        // not the file's own header, and both halves of the diagnostic have to
        // use it.
        let d = e0200_of("app/billing", src);
        assert_eq!(d.entity.as_deref(), Some("app/billing::settle"));
        assert_eq!(
            d.union.as_ref().unwrap().declaration.as_deref(),
            Some("app/billing::PaymentResult")
        );
    }

    /// A prelude union has a fixed variant table and no declaration in any
    /// project module, so there is nothing to address. `builtin` says that;
    /// a `module::name` invented for `Result` would name a module no project
    /// has.
    #[test]
    fn a_prelude_unions_gap_is_a_builtin_with_no_declaration() {
        let src = "module app\n\n\
            import std/result { Ok, Err }\n\n\
            fn f(r: Result<number, string>) -> number {\n\
            \x20 return match r {\n    Ok(n) => n,\n  }\n\
            }\n";
        let d = e0200_of("app", src);
        let union = d.union.as_ref().expect("E0200 names a union");
        assert_eq!(union.kind, "builtin");
        assert_eq!(union.name, "Result");
        assert_eq!(union.module, None);
        assert_eq!(union.declaration, None);
        assert_eq!(d.missing_variants.as_deref(), Some(["Err".to_string()].as_slice()));
    }

    /// Absence has one spelling, and it means absence of a relation rather
    /// than "the analysis stopped here". A diagnostic about no union carries
    /// an explicit null for both fields, so a consumer reads them the same way
    /// it reads `entity` (G184), and a cache written before the keys existed
    /// still loads.
    #[test]
    fn a_diagnostic_about_no_union_says_so_explicitly() {
        let d = Diagnostic::new(
            "main.glyph",
            "module main\n",
            Span::new(0, 6),
            "E0210",
            "error",
            "typecheck",
            "boom".to_string(),
            None,
            None,
            None,
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"union\":null"), "{json}");
        assert!(json.contains("\"missing_variants\":null"), "{json}");

        let old = r#"{"code":"E0200","severity":"error","message":"boom","file":"main.glyph","range":{"start":{"line":1,"col":1,"offset":0},"end":{"line":1,"col":7,"offset":6}},"stage":"typecheck","entity":null}"#;
        let parsed: Diagnostic = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.union, None);
        assert_eq!(parsed.missing_variants, None);
    }

    /// The first E0200 a source produces, as `glyph check --json` would report
    /// it under the module path `module_path`.
    fn e0200_of(module_path: &str, src: &str) -> Diagnostic {
        let module = glyph_parser::parse(src).expect("fixture parses");
        let symbols = glyph_resolver::collect_module_symbols(&module).expect("symbols collect");
        let prelude = glyph_resolver::build_prelude();
        let (resolved, _) = glyph_resolver::resolve_module(&module, symbols, &prelude);
        let (_types, errors) = glyph_typechecker::assign_types(&module, &resolved, &prelude);
        let err = errors
            .iter()
            .find(|e| e.code() == "E0200")
            .unwrap_or_else(|| panic!("expected E0200: {errors:?}"));
        from_type_error(module_path, src, err, &module)
    }
}
