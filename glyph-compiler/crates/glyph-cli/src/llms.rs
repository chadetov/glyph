//! `glyph llms`: the bootstrap document, and the same knowledge as data.
//!
//! `glyph llms` prints `AGENTS.md`, which is prose written by hand. Every line
//! of it is a claim about the compiler that nothing compares against the
//! compiler, and the claims drifted: the document said the MCP server exposed
//! five tools while the server served seven, for two releases (G222).
//!
//! `glyph llms --json` is the other direction. Every section of it is read out
//! of a table the compiler already keeps and nothing in it is written by hand:
//!
//! - `diagnostics` is the code catalogue in `explain.rs`, with the `--explain`
//!   prose and the counter-example the corpus pairs with the code, compiled at
//!   the moment the answer is built.
//! - `prelude` is `glyph_resolver::build_prelude`, the table the resolver
//!   itself looks names up in.
//! - `stdlib` is the `StdlibStubs` export list crossed with the checker's own
//!   signature tables, rendered by the checker's own `display_ty`.
//! - `decisions` is the D-decision index, parsed out of `docs/language/spec.md`.
//! - `tools` is `tool_specs()`, the same object the MCP server serves, plus the
//!   `glyph query` verb that asks each tool from the command line.
//!
//! Where a fact is not modeled, the answer says so in a `*_absent` field beside
//! an explicit `null`, which is the rule the structured diagnostic follows: an
//! agent reads absence one way everywhere, and a stdlib function the checker
//! has no signature for is never described by a sentence somebody wrote.

use std::collections::BTreeMap;

use serde::Serialize;

use glyph_resolver::{build_prelude, is_stdlib_type_only, PreludeKind, StdlibStubs, SymbolKind};
use glyph_typechecker::display_ty;

use crate::explain::{self, CodeEntry, CODES};

/// The spec, embedded so `glyph llms --json` answers with no repo checkout, the
/// way the bootstrap itself is embedded.
const SPEC: &str = include_str!("../../../../docs/language/spec.md");

// ============================================================================
// The document
// ============================================================================

/// Everything `glyph llms --json` answers, in one object.
#[derive(Debug, Serialize)]
pub struct Knowledge {
    /// The compiler that answered. Every section below is that compiler's own
    /// table, so the version is what says which compiler they are tables of.
    pub version: &'static str,
    pub diagnostics: Diagnostics,
    pub prelude: Prelude,
    pub stdlib: Stdlib,
    pub decisions: Decisions,
    pub tools: Tools,
}

/// Build the whole document. The counter-examples are compiled while this runs,
/// which is most of its cost and the reason it is not free: a recorded
/// diagnostic is a claim about a compiler that has moved, and this is the
/// compiler in your hand answering about a program in your hand.
pub fn knowledge() -> Knowledge {
    Knowledge {
        version: env!("CARGO_PKG_VERSION"),
        diagnostics: diagnostics(),
        prelude: prelude(),
        stdlib: stdlib(),
        decisions: decisions(),
        tools: tools(),
    }
}

// ============================================================================
// diagnostics
// ============================================================================

#[derive(Debug, Serialize)]
pub struct Diagnostics {
    /// Where these came from, so an answer read on its own says what produced
    /// it rather than leaving the reader to guess it was typed.
    pub source: &'static str,
    pub count: usize,
    pub codes: Vec<CodeDoc>,
}

/// One diagnostic code, everything the compiler holds about it.
#[derive(Debug, Serialize)]
pub struct CodeDoc {
    pub code: &'static str,
    /// The phase that raises it: `parser`, `resolver`, `typechecker`, `emitter`.
    pub phase: &'static str,
    /// The catalogue sentence. `docs/error-codes.md` and `AGENTS.md` are
    /// written from this string, so the three cannot disagree.
    pub meaning: &'static str,
    /// The one-line repair, from the same table.
    pub fix: &'static str,
    /// The first line of the `--explain` prose, without the code.
    pub title: String,
    /// The whole `--explain` text, the bytes the terminal prints.
    pub explanation: String,
    /// The help line, read off the diagnostic the counter-example draws.
    pub help: Option<String>,
    pub help_absent: Option<String>,
    pub note: Option<String>,
    pub note_absent: Option<String>,
    /// The catalogue section for this code.
    pub docs: String,
    /// A wrong program from `tests/negative/` that draws the code, with the
    /// diagnostic this compiler draws from it, compiled when this was built.
    pub counter_example: Option<explain::CounterExample>,
    pub counter_example_absent: Option<String>,
}

fn diagnostics() -> Diagnostics {
    let codes: Vec<CodeDoc> = CODES.iter().map(code_doc).collect();
    Diagnostics {
        source: "the code catalogue in `glyph-cli`, with each counter-example compiled here",
        count: codes.len(),
        codes,
    }
}

fn code_doc(entry: &'static CodeEntry) -> CodeDoc {
    // Every code in the table is documented: the catalogue test fails the build
    // otherwise, so the fallbacks below describe a state the suite forbids
    // rather than one a reader will meet.
    let answer = explain::explain_json(entry.code);
    match answer {
        Some(a) => CodeDoc {
            code: entry.code,
            phase: entry.phase,
            meaning: entry.meaning,
            fix: entry.fix,
            title: a.title,
            explanation: a.explanation,
            help: a.help,
            help_absent: a.help_absent,
            note: a.note,
            note_absent: a.note_absent,
            docs: a.docs,
            counter_example: a.counter_example,
            counter_example_absent: a.counter_example_absent,
        },
        None => CodeDoc {
            code: entry.code,
            phase: entry.phase,
            meaning: entry.meaning,
            fix: entry.fix,
            title: String::new(),
            explanation: String::new(),
            help: None,
            help_absent: Some(format!("`glyph --explain {}` has no text", entry.code)),
            note: None,
            note_absent: Some(format!("`glyph --explain {}` has no text", entry.code)),
            docs: String::new(),
            counter_example: None,
            counter_example_absent: Some(format!(
                "`glyph --explain {}` has no text, so there is nothing to read a counter-example from",
                entry.code
            )),
        },
    }
}

// ============================================================================
// prelude
// ============================================================================

#[derive(Debug, Serialize)]
pub struct Prelude {
    pub source: &'static str,
    pub count: usize,
    pub names: Vec<PreludeName>,
}

/// One name the resolver puts in scope in every module with no import.
#[derive(Debug, Serialize)]
pub struct PreludeName {
    pub name: String,
    /// `type`, `value`, `namespace` or `type-operator`. What the name can be
    /// written as, which is the question an agent holding it has.
    pub role: &'static str,
    /// The resolver's own kind for it, the discriminant `PreludeKind` carries.
    pub kind: &'static str,
    /// The signature, where the checker models one. It models none for a
    /// prelude name: a prelude type is a shape the lowerer builds at each use
    /// site and a prelude constructor is typed against the type it constructs,
    /// so neither has a signature to print.
    pub signature: Option<String>,
    pub signature_absent: Option<&'static str>,
}

const PRELUDE_SIGNATURE_ABSENT: &str =
    "the prelude is a name table the resolver looks names up in, not a module with signatures: a \
     prelude type is lowered at each use site and a constructor is typed against the type it \
     constructs, so there is no one signature to read";

fn prelude() -> Prelude {
    let built = build_prelude();
    // `by_name` is a hash map, so the declaration order the table was interned
    // in is recovered from the symbol ids rather than from iteration order: two
    // runs must answer the same list in the same order.
    let mut ids: Vec<_> = built.by_name.values().copied().collect();
    ids.sort_by_key(|id| id.0);
    let names: Vec<PreludeName> = ids
        .into_iter()
        .filter_map(|id| {
            let sym = built.table.get(id)?;
            let SymbolKind::Prelude { kind } = sym.kind else {
                return None;
            };
            let (role, wire) = prelude_role(kind);
            Some(PreludeName {
                name: sym.name.to_string(),
                role,
                kind: wire,
                signature: None,
                signature_absent: Some(PRELUDE_SIGNATURE_ABSENT),
            })
        })
        .collect();
    Prelude {
        source: "`glyph_resolver::build_prelude`, the table the resolver resolves against",
        count: names.len(),
        names,
    }
}

/// `(role, wire name)` for a prelude kind.
///
/// A match rather than a derived `Debug` string: a name added to the prelude
/// fails this build until somebody says which of the four roles it has, which
/// is the question the answer exists to settle.
fn prelude_role(kind: PreludeKind) -> (&'static str, &'static str) {
    match kind {
        PreludeKind::String => ("type", "string"),
        PreludeKind::Number => ("type", "number"),
        PreludeKind::Int => ("type", "int"),
        PreludeKind::BigInt => ("type", "bigint"),
        PreludeKind::Bool => ("type", "bool"),
        PreludeKind::Void => ("type", "void"),
        PreludeKind::UnknownTop => ("type", "unknown"),
        PreludeKind::Never => ("type", "never"),
        PreludeKind::Result => ("type", "Result"),
        PreludeKind::Option => ("type", "Option"),
        PreludeKind::Nullable => ("type", "Nullable"),
        PreludeKind::Array => ("type", "Array"),
        PreludeKind::Record => ("type", "Record"),
        PreludeKind::Schema => ("type", "Schema"),
        PreludeKind::Component => ("type", "Component"),
        PreludeKind::Issue => ("type", "Issue"),
        PreludeKind::InferOutput => ("type-operator", "infer_output"),
        PreludeKind::Ok => ("value", "Ok"),
        PreludeKind::Err => ("value", "Err"),
        PreludeKind::Some => ("value", "Some"),
        PreludeKind::None => ("value", "None"),
        PreludeKind::Par => ("namespace", "par"),
        PreludeKind::Print => ("value", "print"),
        PreludeKind::Assert => ("value", "assert"),
    }
}

// ============================================================================
// stdlib
// ============================================================================

#[derive(Debug, Serialize)]
pub struct Stdlib {
    pub source: &'static str,
    pub modules: Vec<StdlibModule>,
    /// The three counts are disjoint and sum to the export total, so a reader
    /// sees the shape of the gap without counting the list.
    ///
    /// `modeled` is a signature with a type in every position. A signature the
    /// checker renders with a `?` somewhere is `partially_modeled`: the `?` is
    /// `Ty::Unknown`, so the table models the arity and the return and compares
    /// nothing at that position, and counting it as modeled published a
    /// complete-looking signature an agent cannot read a parameter type out of.
    pub modeled: usize,
    pub partially_modeled: usize,
    pub unmodeled: usize,
}

#[derive(Debug, Serialize)]
pub struct StdlibModule {
    /// The import path: `std/array`.
    pub path: String,
    pub exports: Vec<StdlibExport>,
}

#[derive(Debug, Serialize)]
pub struct StdlibExport {
    pub name: String,
    /// `type` for a name the module exports only as a type (`fs.FsError`),
    /// `value` otherwise. From the emitter's own type-only table, which is what
    /// decides whether an import of the name carries `type`.
    pub kind: &'static str,
    /// The signature, rendered by the checker's own `display_ty` from the type
    /// its own tables hold.
    pub signature: Option<String>,
    pub signature_absent: Option<String>,
    /// Set when the signature carries a `?`: which positions the table leaves
    /// `unknown`, so a reader is never handed a `?` with nothing said about it.
    /// `None` for a complete signature and for an export with none at all.
    pub signature_partial: Option<String>,
}

fn stdlib() -> Stdlib {
    let built = build_prelude();
    let stubs = StdlibStubs::new();
    let mut by_path: BTreeMap<String, Vec<StdlibExport>> = BTreeMap::new();
    let (mut modeled, mut partially_modeled, mut unmodeled) = (0usize, 0usize, 0usize);
    for (path, exports) in stubs.iter() {
        // `names` is a `BTreeSet`, so this is the module's export list in one
        // order on every machine.
        let names: Vec<String> = exports.names.iter().map(|n| n.to_string()).collect();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let ty = glyph_typechecker::stdlib_signature(&built, path, &name);
            let (signature, absent, partial) = match ty {
                Some(ty) => {
                    let rendered = display_ty(&ty);
                    // `?` is `display_ty`'s rendering of `Ty::Unknown`, which is
                    // what the table holds for a position it does not model.
                    let holes = rendered.matches('?').count();
                    match holes {
                        0 => {
                            modeled += 1;
                            (Some(rendered), None, None)
                        }
                        n => {
                            partially_modeled += 1;
                            (
                                Some(rendered),
                                None,
                                Some(format!(
                                    "this signature carries {n} `?`, which is the checker's \
                                     rendering of `unknown`: the table models \
                                     `{path}::{name}`'s arity and its return and leaves that \
                                     many positions unmodeled, so an argument at one of them \
                                     is compared by nothing here and `tsc` on a full `glyph \
                                     build` is what reads it"
                                )),
                            )
                        }
                    }
                }
                None => {
                    unmodeled += 1;
                    (
                        None,
                        Some(format!(
                            "the checker models no signature for `{path}::{name}`: the runtime \
                             ships TypeScript no Glyph pass reads, so a call to it is typed \
                             `unknown` here and checked by `tsc` alone"
                        )),
                        None,
                    )
                }
            };
            out.push(StdlibExport {
                kind: match is_stdlib_type_only(path, &name) {
                    true => "type",
                    false => "value",
                },
                name,
                signature,
                signature_absent: absent,
                signature_partial: partial,
            });
        }
        by_path.insert(path.to_string(), out);
    }
    Stdlib {
        source: "the resolver's export list for each `std/` module, with each signature read out \
                 of the checker's own tables and rendered by `display_ty`. The three counts are \
                 disjoint and sum to the export total: `modeled` is a signature with a type in \
                 every position, `partially_modeled` is one the checker renders with a `?` \
                 somewhere, and `unmodeled` is an export the tables hold no type for at all. A \
                 `?` is `unknown`: the tables model a function's arity and its return and leave \
                 the parameters `unknown`, so modeling a function introduces no new \
                 argument-type diagnostic, and `signature_partial` says how many positions of \
                 that signature are left, so a signature with a `?` is never counted as a \
                 complete one",
        modules: by_path
            .into_iter()
            .map(|(path, exports)| StdlibModule { path, exports })
            .collect(),
        modeled,
        partially_modeled,
        unmodeled,
    }
}

// ============================================================================
// decisions
// ============================================================================

#[derive(Debug, Serialize)]
pub struct Decisions {
    pub source: &'static str,
    pub count: usize,
    /// Numbers the spec uses for two different decisions. The index reports
    /// both rather than picking one, because a reader joining on `D43` has to
    /// know the key is not unique in the document it came from.
    pub duplicate_numbers: Vec<u32>,
    pub decisions: Vec<Decision>,
}

#[derive(Debug, Serialize)]
pub struct Decision {
    /// The number, so `D30` sorts and joins as a number rather than a string.
    pub number: u32,
    pub id: String,
    /// The bolded first sentence: what the decision decided.
    pub title: String,
    /// The section of the spec it sits under.
    pub section: String,
    /// The spec's own text for it, whole, including the title sentence.
    pub body: String,
}

/// Parse the D-decision index out of the spec.
///
/// The spec writes one decision per top-level list item, opening
/// `- **D30. <title>**`, under a `## <section>` heading, and continuation lines
/// are indented. That shape is the index: parsing it is what keeps this from
/// being a second list of decisions that falls behind the first.
fn decisions() -> Decisions {
    let mut out: Vec<Decision> = Vec::new();
    let mut section = String::new();
    let mut current: Option<(u32, String, String, String)> = None;
    for line in SPEC.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            section = rest.trim().to_string();
        }
        let head = line
            .strip_prefix("- **D")
            .and_then(|rest| rest.split_once('.').map(|(n, t)| (n.to_string(), t)))
            .and_then(|(n, t)| n.parse::<u32>().ok().map(|n| (n, t)));
        match head {
            Some((number, rest)) => {
                if let Some(done) = current.take() {
                    out.push(finish(done));
                }
                // The title runs to the closing `**` of the bold opener.
                let title = rest
                    .trim_start()
                    .split_once("**")
                    .map(|(t, _)| t)
                    .unwrap_or(rest)
                    .trim()
                    .trim_end_matches('.')
                    .to_string();
                current = Some((number, title, section.clone(), line.to_string()));
            }
            None => {
                if let Some((_, _, _, body)) = current.as_mut() {
                    // A new top-level list item that is not a decision ends the
                    // one being read; an indented line continues it.
                    if line.starts_with("- ") || line.starts_with("## ") || line.starts_with("# ") {
                        let done = current.take().expect("current is Some in this arm");
                        out.push(finish(done));
                    } else {
                        body.push('\n');
                        body.push_str(line);
                    }
                }
            }
        }
    }
    if let Some(done) = current.take() {
        out.push(finish(done));
    }
    // A stable sort, so two decisions the spec gives one number keep the order
    // the spec wrote them in.
    out.sort_by_key(|d| d.number);
    let mut duplicate_numbers: Vec<u32> = out
        .windows(2)
        .filter(|w| w[0].number == w[1].number)
        .map(|w| w[0].number)
        .collect();
    duplicate_numbers.dedup();
    Decisions {
        source: "`docs/language/spec.md`, parsed from its own decision list",
        count: out.len(),
        duplicate_numbers,
        decisions: out,
    }
}

fn finish((number, title, section, body): (u32, String, String, String)) -> Decision {
    Decision {
        number,
        id: format!("D{number}"),
        title,
        section,
        body: body.trim_end().to_string(),
    }
}

// ============================================================================
// tools
// ============================================================================

#[derive(Debug, Serialize)]
pub struct Tools {
    pub source: &'static str,
    pub count: usize,
    /// The server's own `instructions` string, the one an MCP client reads
    /// before it calls anything.
    pub instructions: String,
    /// The closed relation vocabulary, in the long form. Four argument schemas
    /// used to carry a copy of it each.
    pub relations: String,
    pub tools: Vec<Tool>,
}

#[derive(Debug, Serialize)]
pub struct Tool {
    pub name: String,
    /// The one-paragraph contract `tools/list` carries.
    pub description: String,
    /// The full field-by-field text: `answer` for the tool itself, and
    /// `arguments` for any argument whose own text moved here too. It used to
    /// be the description, which every session paid for whether it called the
    /// tool or not; it lives here now and the description points at it.
    pub manual: Option<serde_json::Value>,
    pub manual_absent: Option<String>,
    /// The JSON schema for the tool's arguments, the same object the server
    /// serves.
    pub input_schema: serde_json::Value,
    /// The `glyph query` verb that asks this tool from the command line, for an
    /// agent with no MCP client.
    pub cli_verb: Option<String>,
    pub cli_verb_absent: Option<String>,
}

fn tools() -> Tools {
    use clap::Subcommand;

    // The verbs clap itself knows, rather than a second list of them here.
    let command = crate::query::QueryCommand::augment_subcommands(clap::Command::new("query"));
    let verbs: Vec<String> = command
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();

    let specs = glyph_lsp::tool_specs();
    let manual = glyph_lsp::tool_manual();
    // `tool_specs()` answers the bare array the server puts under `tools`.
    let listed = specs.as_array().cloned().unwrap_or_default();
    let relations = manual
        .get("relations")
        .and_then(|r| r.as_str())
        .unwrap_or_default()
        .to_string();
    let manual = manual.get("tools").cloned().unwrap_or(serde_json::Value::Null);
    let tools: Vec<Tool> = listed
        .iter()
        .map(|t| {
            let name = t
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let verb = name.strip_prefix("glyph_").unwrap_or("").to_string();
            let (cli_verb, cli_verb_absent) = match verbs.contains(&verb) {
                true => (Some(format!("glyph query {verb}")), None),
                false => (
                    None,
                    Some(format!(
                        "`glyph query` has no `{verb}` verb, so this tool is reachable over MCP only"
                    )),
                ),
            };
            let (manual, manual_absent) = match manual.get(name.as_str()) {
                Some(entry) => (Some(entry.clone()), None),
                None => (None, Some(format!("no manual entry is written for `{name}`"))),
            };
            Tool {
                description: t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
                input_schema: t.get("inputSchema").cloned().unwrap_or(serde_json::Value::Null),
                name,
                manual,
                manual_absent,
                cli_verb,
                cli_verb_absent,
            }
        })
        .collect();
    Tools {
        source: "`tool_specs()` in `glyph-lsp`, the object the MCP server serves",
        count: tools.len(),
        instructions: glyph_lsp::instructions().to_string(),
        relations,
        tools,
    }
}

// ============================================================================
// The commands
// ============================================================================

/// `glyph llms` — print the bootstrap document.
pub fn run_bootstrap() -> i32 {
    print!("{}", crate::LLMS_BOOTSTRAP);
    0
}

/// `glyph llms --json` — print the whole knowledge document.
pub fn run_json() -> i32 {
    match serde_json::to_string_pretty(&knowledge()) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("glyph llms: could not serialize the answer: {e}");
            2
        }
    }
}

/// `glyph llms --negative [CODE]` — the wrong programs the corpus pairs with a
/// code, compiled here, and the `catches/` case when there is one.
pub fn run_negative(code: Option<&str>) -> i32 {
    match code {
        None => {
            let listing = explain::negative_index();
            match serde_json::to_string_pretty(&listing) {
                Ok(s) => {
                    println!("{s}");
                    0
                }
                Err(e) => {
                    eprintln!("glyph llms: could not serialize the answer: {e}");
                    2
                }
            }
        }
        Some(code) => match explain::negative_for_code(code) {
            Some(answer) => match serde_json::to_string_pretty(&answer) {
                Ok(s) => {
                    println!("{s}");
                    0
                }
                Err(e) => {
                    eprintln!("glyph llms: could not serialize the answer: {e}");
                    2
                }
            },
            None => {
                eprintln!(
                    "glyph llms --negative {code}: not a diagnostic code this compiler documents. \
                     `glyph llms --negative` with no code lists the codes that have a case."
                );
                2
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prelude section answers for every name the resolver put in scope,
    /// in the order the table interned them, and says why each has no
    /// signature rather than leaving a bare null.
    #[test]
    fn the_prelude_section_is_the_resolvers_own_table() {
        let p = prelude();
        let built = build_prelude();
        assert_eq!(p.count, built.by_name.len());
        assert_eq!(p.names.first().map(|n| n.name.as_str()), Some("string"));
        for name in &p.names {
            assert!(name.signature.is_none());
            assert!(name.signature_absent.is_some(), "{}", name.name);
        }
        let by_name: Vec<&str> = p.names.iter().map(|n| n.name.as_str()).collect();
        for expected in ["Result", "Option", "Nullable", "infer_output", "par"] {
            assert!(by_name.contains(&expected), "prelude lost `{expected}`");
        }
    }

    /// Every stdlib module the resolver seeds is in the answer, with every name
    /// it exports, and a name the checker does not model says so.
    #[test]
    fn the_stdlib_section_is_the_resolver_seed_crossed_with_the_checkers_tables() {
        let s = stdlib();
        let stubs = StdlibStubs::new();
        assert_eq!(s.modules.len(), stubs.iter().count());
        let array = s
            .modules
            .iter()
            .find(|m| m.path == "std/array")
            .expect("std/array is seeded");
        let range = array
            .exports
            .iter()
            .find(|e| e.name == "range")
            .expect("std/array exports range");
        assert_eq!(
            range.signature.as_deref(),
            Some("fn(number) -> Array<number>"),
            "the signature is the checker's own, rendered by display_ty"
        );
        // Every export is either modeled or says why not, and never both.
        for module in &s.modules {
            for export in &module.exports {
                assert_eq!(
                    export.signature.is_none(),
                    export.signature_absent.is_some(),
                    "{}::{}",
                    module.path,
                    export.name
                );
            }
        }
        assert!(s.modeled > 0 && s.partially_modeled > 0 && s.unmodeled > 0);
        assert_eq!(
            s.modeled + s.partially_modeled + s.unmodeled,
            s.modules.iter().map(|m| m.exports.len()).sum::<usize>()
        );
    }

    /// The three counts are the tables, counted, and a `?` is never published
    /// as a complete signature.
    ///
    /// The review that found this had `modeled: 100` over 76 signatures
    /// carrying a `?` where a parameter type belongs, each with
    /// `signature_absent: null`, so the document read as though the checker
    /// knew what to pass to `fs.read_text`. The counts are recomputed from the
    /// export list here rather than trusted, and each of the three states is
    /// asserted to say the one thing it means.
    #[test]
    fn the_stdlib_counts_are_the_tables_counted() {
        let s = stdlib();
        let (mut complete, mut partial, mut absent) = (0usize, 0usize, 0usize);
        for module in &s.modules {
            for e in &module.exports {
                match (&e.signature, &e.signature_partial) {
                    (Some(sig), None) => {
                        assert!(
                            !sig.contains('?'),
                            "{}::{} is counted complete and renders a `?`: {sig}",
                            module.path,
                            e.name
                        );
                        assert!(e.signature_absent.is_none());
                        complete += 1;
                    }
                    (Some(sig), Some(why)) => {
                        assert!(
                            sig.contains('?'),
                            "{}::{} is counted partial and renders no `?`: {sig}",
                            module.path,
                            e.name
                        );
                        assert!(
                            why.contains(&sig.matches('?').count().to_string()),
                            "the reason does not say how many positions are left: {why}"
                        );
                        assert!(e.signature_absent.is_none());
                        partial += 1;
                    }
                    (None, None) => {
                        assert!(
                            e.signature_absent.is_some(),
                            "{}::{} has no signature and says nothing about it",
                            module.path,
                            e.name
                        );
                        absent += 1;
                    }
                    (None, Some(why)) => panic!(
                        "{}::{} has no signature and a partiality reason: {why}",
                        module.path, e.name
                    ),
                }
            }
        }
        assert_eq!(
            (complete, partial, absent),
            (s.modeled, s.partially_modeled, s.unmodeled),
            "the counts in the document disagree with the export list it publishes"
        );
    }

    /// The decision index is the spec's own list, parsed rather than retyped.
    #[test]
    fn the_decision_index_is_parsed_from_the_spec() {
        let d = decisions();
        assert!(d.count >= 45, "parsed only {} decisions", d.count);
        let numbers: Vec<u32> = d.decisions.iter().map(|x| x.number).collect();
        let mut sorted = numbers.clone();
        sorted.sort_unstable();
        assert_eq!(numbers, sorted, "the index is not sorted by number");
        // The spec numbers two different decisions `D43`, and the index says so
        // rather than dropping one of them.
        assert!(
            d.duplicate_numbers.contains(&43),
            "the spec stopped using D43 twice: {:?}",
            d.duplicate_numbers
        );
        assert_eq!(
            d.count,
            numbers.len(),
            "every parsed decision is in the list"
        );
        let d30 = d
            .decisions
            .iter()
            .find(|x| x.number == 30)
            .expect("D30 is in the spec");
        assert!(!d30.title.is_empty());
        assert!(d30.title.len() < 200, "the title ran past the bold opener");
        assert!(d30.body.starts_with("- **D30."));
        assert!(!d30.section.is_empty());
    }

    /// Every tool the server serves is in the answer, with the `glyph query`
    /// verb that asks it.
    #[test]
    fn every_tool_carries_its_cli_verb_and_its_manual() {
        let t = tools();
        assert_eq!(t.count, 11, "the tool set changed; say so in AGENTS.md too");
        assert!(!t.instructions.is_empty());
        for tool in &t.tools {
            assert!(
                tool.cli_verb.is_some(),
                "{} has no `glyph query` verb: {:?}",
                tool.name,
                tool.cli_verb_absent
            );
            assert!(
                tool.manual.is_some(),
                "{} has no manual entry: {:?}",
                tool.name,
                tool.manual_absent
            );
            assert!(!tool.description.is_empty(), "{}", tool.name);
            assert!(!tool.input_schema.is_null(), "{}", tool.name);
        }
    }
}
