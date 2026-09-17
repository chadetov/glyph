//! `glyph fix` — apply the repairs the compiler fully determines.
//!
//! Four rules. The first drops the dead names out of an `import` (the E0106
//! lint). An import whose every bound name is unused loses the whole
//! declaration; a named import (`import M { a, b, c }`) with only some names
//! dead (say `b`) keeps the declaration and drops just `b`. Only `Named` can
//! be partially dead: `Namespace`/`Aliased`/`Default` each bind a single
//! name, so for those "some dead" and "all dead" are the same case.
//!
//! The other three read the structured diagnostics of a real check and edit
//! the source the diagnostic points at:
//!
//! * **E0200**, a non-exhaustive match, gains one arm per name in
//!   `missing_variants`, with the pattern the union's own declaration implies
//!   and a body that says out loud it is unwritten (see `render_arms`).
//! * **E0220**, an arm head that is not a variant, takes the `suggestion` the
//!   checker already computed, and only when there is exactly one.
//! * **E0210**, a field that the record does not declare, takes the one
//!   declared field within edit distance one of it, and only when there is
//!   exactly one.
//!
//! None of the three guesses. Where the compiler does not hold the answer the
//! rule declines and says why, and the reason reaches the command's output
//! beside what it did apply. A rule that cannot state where its answer came
//! from does not run: the variant payload shapes come from `glyph_symbol`,
//! which is the same code `glyph query symbol` answers with, rather than from
//! a second reading of the union's declaration here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glyph_ast::{Decl, ImportKind, Module};
use glyph_resolver::{
    build_prelude, collect_module_symbols, module_lints, resolve_module, ResolveError,
};

use crate::diagnostic::Diagnostic;

pub struct FixReport {
    pub changed: Vec<PathBuf>,
    pub removed_imports: usize,
    /// One entry per diagnostic-driven repair that was written.
    pub applied: Vec<Applied>,
    /// One entry per diagnostic a rule matched and did not repair, with the
    /// reason. A repair that guesses is worse than no repair, so a rule that
    /// stops has to say what stopped it rather than going quiet.
    pub declined: Vec<Declined>,
    /// Problems with the run rather than with any one diagnostic: a tree that
    /// would not check at all, so only the import rule could run.
    pub notices: Vec<String>,
}

/// One repair `glyph fix` wrote.
#[derive(Debug, Clone)]
pub struct Applied {
    pub code: String,
    pub file: PathBuf,
    /// What was written, in one clause: "added arms for `Paid`, `Refunded`".
    pub what: String,
}

/// One repair `glyph fix` did not write, and why.
#[derive(Debug, Clone)]
pub struct Declined {
    pub code: String,
    pub file: PathBuf,
    pub why: String,
}

/// Apply the safe autofixes across every `.glyph` file under `src` (a directory,
/// or a single file). Rewrites files in place and returns what changed.
pub fn fix_project(src: &Path) -> Result<FixReport, String> {
    let mut files = Vec::new();
    collect_glyph_files(src, &mut files);
    let prelude = build_prelude();
    let mut report = FixReport {
        changed: Vec::new(),
        removed_imports: 0,
        applied: Vec::new(),
        declined: Vec::new(),
        notices: Vec::new(),
    };

    for f in files.iter().cloned() {
        let source =
            std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let Ok(module) = glyph_parser::parse(&source) else { continue };
        let Ok(symbols) = collect_module_symbols(&module) else { continue };
        let (resolved, _errs) = resolve_module(&module, symbols, &prelude);

        let unused: std::collections::HashSet<String> = module_lints(&module, &resolved)
            .iter()
            .filter_map(|e| match e {
                ResolveError::UnusedImport { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        if unused.is_empty() {
            continue;
        }

        // One edit per import decl that has at least one dead name: either drop
        // the whole declaration (its line(s), including the trailing newline)
        // or, for a partially-dead `Named` import, splice a rewritten name list
        // over just that decl's own byte span.
        let mut edits: Vec<(u32, u32, String)> = Vec::new();
        for item in &module.items {
            let Decl::Import(imp) = item else { continue };
            let names: Vec<String> = match &imp.kind {
                ImportKind::Namespace => imp
                    .path
                    .segments
                    .last()
                    .map(|s| vec![s.to_string()])
                    .unwrap_or_default(),
                ImportKind::Aliased(a) => vec![a.to_string()],
                ImportKind::Default(local) => vec![local.to_string()],
                ImportKind::Named(ns) => ns.iter().map(|n| n.to_string()).collect(),
            };
            let dead_count = names.iter().filter(|n| unused.contains(*n)).count();
            if dead_count == 0 {
                continue;
            }
            if dead_count == names.len() {
                let (start, end) = full_line_span(&source, imp.span.start, imp.span.end);
                edits.push((start, end, String::new()));
                report.removed_imports += dead_count;
            } else if let ImportKind::Named(ns) = &imp.kind {
                let kept: Vec<&str> =
                    ns.iter().map(|n| n.as_ref()).filter(|n| !unused.contains(*n)).collect();
                let path_text =
                    imp.path.segments.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("/");
                let new_text = format!("import {} {{ {} }}", path_text, kept.join(", "));
                // Replace the decl's own text and nothing else. `imp.span.end`
                // sits PAST the newline terminating the import (see
                // `decl_text_end`), so splicing over the raw span consumes that
                // newline and welds the rewritten import onto what follows.
                let decl_end = decl_text_end(&source, imp.span.start, imp.span.end);
                edits.push((imp.span.start, decl_end, new_text));
                report.removed_imports += dead_count;
            }
        }
        if edits.is_empty() {
            continue;
        }

        let new_source = apply_edits(&source, edits);
        // A tool that edits source is held to a higher bar than one that only
        // reports. Re-parse what is about to be written and refuse to write it
        // if the rewrite broke the file, so a bad fix fails loudly instead of
        // reporting success over a tree it corrupted.
        if glyph_parser::parse(&new_source).is_err() {
            return Err(format!(
                "{}: the rewritten file does not parse, so nothing was written. \
                 This is a bug in `glyph fix`; please report it with the file.",
                f.display()
            ));
        }
        std::fs::write(&f, &new_source).map_err(|e| format!("{}: {e}", f.display()))?;
        report.changed.push(f);
    }

    // The import rule has written its edits, so a check now reads the text the
    // diagnostic offsets below are counted against. Running it the other way
    // round would hand every rule a set of offsets one import-removal stale.
    apply_diagnostic_rules(src, &files, &mut report)?;

    Ok(report)
}

/// The byte range of the physical line(s) spanning `[start, end)`, including
/// the trailing newline of the last of them, so deleting it doesn't leave a
/// blank line behind. An import decl never shares a line with anything else,
/// so this is exact even when the decl itself spans several lines.
fn full_line_span(source: &str, start: u32, end: u32) -> (u32, u32) {
    let line_start = source[..start as usize].rfind('\n').map(|i| i as u32 + 1).unwrap_or(0);

    // An import decl's span already ends PAST its own newline: the parser sets
    // the end from `peek_span()` while the peeked token is the `Newline`, so
    // `end` is the first byte of the following line. Scanning forward from
    // `end` therefore finds the NEXT line's newline and deletes a line the
    // author wrote. `glyph fix` on a file whose second import is unused
    // removed the import and the `fn main` line under it.
    //
    // Walk back over the newline the span already covers before looking for
    // the end of the line, so `end` lands inside the decl's own line whatever
    // the parser handed us.
    let scan = decl_text_end(source, start, end) as usize;
    let line_end = match source[scan..].find('\n') {
        Some(i) => (scan + i) as u32 + 1,
        None => source.len() as u32,
    };
    (line_start, line_end)
}

/// The end of a decl's own text, with the line terminator it sits on excluded.
///
/// An import decl's span already ends PAST its own newline: the parser takes
/// the end from `peek_span()` while the peeked token is the `Newline`, so `end`
/// is the first byte of the following line. Any caller that wants "the decl
/// itself" has to walk that back. Removing a whole import does so through
/// `full_line_span`; the partial-prune path added in 0.1.99 did not, and
/// spliced a rewritten import over a range that included the newline, joining
/// it to the next line and leaving the file unparseable.
fn decl_text_end(source: &str, start: u32, end: u32) -> u32 {
    let mut scan = end as usize;
    while scan > start as usize
        && matches!(source.as_bytes().get(scan - 1), Some(&b'\n') | Some(&b'\r'))
    {
        scan -= 1;
    }
    scan as u32
}

/// Splice a set of `(start, end, replacement)` byte-range edits into `source`
/// in one left-to-right pass. Edits are never overlapping (each comes from a
/// distinct import decl) so sorting by start is enough to apply them in order.
fn apply_edits(source: &str, mut edits: Vec<(u32, u32, String)>) -> String {
    edits.sort_by_key(|(start, _, _)| *start);
    let mut out = String::with_capacity(source.len());
    let mut pos = 0usize;
    for (start, end, replacement) in &edits {
        out.push_str(&source[pos..*start as usize]);
        out.push_str(replacement);
        pos = *end as usize;
    }
    out.push_str(&source[pos..]);
    out
}

fn collect_glyph_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if p.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_glyph_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(p);
        }
    }
}


// ---------------------------------------------------------------------------
// The diagnostic-driven rules
// ---------------------------------------------------------------------------

/// The codes this pass repairs. A diagnostic with any other code is left alone.
const REPAIRABLE: [&str; 3] = ["E0200", "E0210", "E0220"];

/// Check the tree, then apply every rule that the resulting diagnostics fully
/// determine.
///
/// The check is the real one (`glyph check --no-tsc`, the same call the command
/// makes), so the offsets, the union identities and the checker's own
/// suggestions all come from the compiler rather than from a second reading of
/// the source here.
fn apply_diagnostic_rules(
    src: &Path,
    files: &[PathBuf],
    report: &mut FixReport,
) -> Result<(), String> {
    let check = match crate::check::check_path(src, false, false) {
        Ok(c) => c,
        Err(e) => {
            report.notices.push(format!(
                "the tree did not check, so only the unused-import rule ran: {e}"
            ));
            return Ok(());
        }
    };

    // Group by the file the diagnostic is in, resolved to a path this run
    // actually collected. A diagnostic whose file cannot be placed in exactly
    // one of the collected files is left alone rather than written to a guess.
    let mut by_file: BTreeMap<PathBuf, (PathBuf, Vec<Diagnostic>)> = BTreeMap::new();
    for d in &check.structured {
        if !REPAIRABLE.contains(&d.code.as_str()) {
            continue;
        }
        let Some((path, root)) = place_diagnostic(&check.project_srcs, files, d) else {
            continue;
        };
        by_file
            .entry(path)
            .or_insert_with(|| (root, Vec::new()))
            .1
            .push(d.clone());
    }

    for (path, (root, diags)) in by_file {
        fix_one_file(&path, &root, &diags, report)?;
    }
    Ok(())
}

/// The absolute path a diagnostic is about, and the project root its identities
/// are counted from.
///
/// `Diagnostic::file` is a path under the project root that produced it, and a
/// tree may hold several projects (D41), so the pairing is (root, root/file).
/// The answer has to be one of the files this run collected: `glyph fix` writes
/// only inside the tree it was pointed at.
fn place_diagnostic(
    roots: &[PathBuf],
    files: &[PathBuf],
    d: &Diagnostic,
) -> Option<(PathBuf, PathBuf)> {
    let mut found: Option<(PathBuf, PathBuf)> = None;
    for root in roots {
        let candidate = root.join(&d.file);
        if !files.iter().any(|f| same_file(f, &candidate)) {
            continue;
        }
        if found.is_some() {
            // Two projects hold a file with this relative path; which one the
            // diagnostic came from is not decidable from the diagnostic alone.
            return None;
        }
        found = Some((candidate, root.clone()));
    }
    found
}

/// Whether two paths name the same file, comparing canonical forms when both
/// canonicalize and the paths themselves otherwise.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Apply every rule that fires on one file, in one rewrite.
fn fix_one_file(
    path: &Path,
    root: &Path,
    diags: &[Diagnostic],
    report: &mut FixReport,
) -> Result<(), String> {
    let source =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Ok(module) = glyph_parser::parse(&source) else {
        // A file that does not parse has parse diagnostics of its own, and
        // every rule here needs the declarations.
        return Ok(());
    };

    let mut edits: Vec<(u32, u32, String)> = Vec::new();
    let mut applied: Vec<Applied> = Vec::new();
    let mut arm_plans: Vec<ArmPlan> = Vec::new();

    for d in diags {
        match d.code.as_str() {
            "E0220" => match plan_e0220(&source, d, diags) {
                Ok(edit) => {
                    applied.push(Applied {
                        code: d.code.clone(),
                        file: path.to_path_buf(),
                        what: format!("renamed the arm head to `{}`", edit.2),
                    });
                    edits.push(edit);
                }
                Err(why) => report.declined.push(Declined {
                    code: d.code.clone(),
                    file: path.to_path_buf(),
                    why,
                }),
            },
            "E0210" => match plan_e0210(&source, d) {
                Ok(edit) => {
                    applied.push(Applied {
                        code: d.code.clone(),
                        file: path.to_path_buf(),
                        what: format!("renamed the field read to `{}`", edit.2),
                    });
                    edits.push(edit);
                }
                Err(why) => report.declined.push(Declined {
                    code: d.code.clone(),
                    file: path.to_path_buf(),
                    why,
                }),
            },
            "E0200" => {
                // An arm head the checker could not read as a variant (E0220)
                // inside this same match means the set of variants the match
                // mentions is not settled: repairing the head can make the
                // match exhaustive, and the arms added here would then be
                // duplicates. Fix the head first; `glyph fix` run again sees
                // whatever is left.
                if diags.iter().any(|o| {
                    o.code == "E0220"
                        && o.range.start.offset >= d.range.start.offset
                        && o.range.end.offset <= d.range.end.offset
                }) {
                    report.declined.push(Declined {
                        code: d.code.clone(),
                        file: path.to_path_buf(),
                        why: "this match also has an arm head that is not a variant (E0220), \
                              so which variants it already covers is not settled. Run `glyph \
                              fix` again once that is repaired."
                            .to_string(),
                    });
                    continue;
                }
                match plan_e0200(&source, path, root, d) {
                    Ok(plan) => arm_plans.push(plan),
                    Err(why) => report.declined.push(Declined {
                        code: d.code.clone(),
                        file: path.to_path_buf(),
                        why,
                    }),
                }
            }
            _ => {}
        }
    }

    if !arm_plans.is_empty() {
        if let Some(planned) = plan_arm_edits(&source, &module, path, root, &arm_plans, report) {
            edits.extend(planned.edits);
            applied.extend(planned.applied);
        }
    }

    if edits.is_empty() {
        return Ok(());
    }

    let new_source = apply_edits(&source, edits);
    // Same bar the import rule is held to: re-parse what is about to be written
    // and refuse to write a file the rewrite broke.
    if glyph_parser::parse(&new_source).is_err() {
        return Err(format!(
            "{}: the rewritten file does not parse, so nothing was written. \
             This is a bug in `glyph fix`; please report it with the file.",
            path.display()
        ));
    }
    std::fs::write(path, &new_source).map_err(|e| format!("{}: {e}", path.display()))?;
    if !report.changed.iter().any(|c| c == path) {
        report.changed.push(path.to_path_buf());
    }
    report.applied.extend(applied);
    Ok(())
}

// ---------------------------------------------------------------------------
// E0220: the checker's own suggestion, when there is exactly one
// ---------------------------------------------------------------------------

/// Replace a match arm's head with the variant the checker suggested.
///
/// The diagnostic's range covers the whole pattern (`Feed.Loadign(x)`), not the
/// head, so the head is the identifier before the payload and after the last
/// qualifier. `alternatives` is the suggestion list the checker already
/// computed; more than one of them, or none, and there is nothing determined to
/// write.
///
/// The suggestion is a nearest name, not a coverage answer: the checker will
/// suggest `Loading` for `Loadign` whether or not a `Loading` arm is already
/// there, and writing it over a match that has one produces `E0305`, an arm
/// that can never run. Whether the suggested variant is still free is settled
/// by the match's own `E0200`: a variant the match is missing is free, and a
/// match with no `E0200` at all has every variant spoken for.
fn plan_e0220(
    source: &str,
    d: &Diagnostic,
    siblings: &[Diagnostic],
) -> Result<(u32, u32, String), String> {
    let alts = d
        .alternatives
        .as_ref()
        .ok_or("the checker computed no nearest variant for this head")?;
    if alts.len() != 1 {
        return Err(format!(
            "the checker computed {} candidate variants ({}), and only one determines the repair",
            alts.len(),
            quoted_list(alts)
        ));
    }
    let suggestion = &alts[0];
    let enclosing = siblings
        .iter()
        .filter(|o| {
            o.code == "E0200"
                && o.range.start.offset <= d.range.start.offset
                && o.range.end.offset >= d.range.end.offset
        })
        .min_by_key(|o| o.range.end.offset - o.range.start.offset);
    match enclosing {
        None => {
            return Err(format!(
                "the other arms of this match already cover every case, so renaming this head to `{suggestion}` would write an arm that can never run (E0305)"
            ))
        }
        Some(m) => {
            let missing = m.missing_variants.as_deref().unwrap_or(&[]);
            if !missing.iter().any(|v| v == suggestion) {
                return Err(format!(
                    "`{suggestion}` is not one of the cases this match is missing ({}), so renaming this head to it would write an arm that can never run (E0305)",
                    quoted_list(missing)
                ));
            }
        }
    }
    let (start, end) = head_ident_span(source, d)
        .ok_or("the arm head is not a plain name in the source this diagnostic points at")?;
    Ok((start, end, alts[0].clone()))
}

/// The byte span of the head identifier inside a match-arm pattern span: what
/// is left after dropping the payload (`(...)`) and every qualifier (`Feed.`).
fn head_ident_span(source: &str, d: &Diagnostic) -> Option<(u32, u32)> {
    let (s, e) = range_bytes(source, d)?;
    let text = &source[s..e];
    let head = match text.find('(') {
        Some(i) => &text[..i],
        None => text,
    };
    let after_dot = head.rfind('.').map(|i| i + 1).unwrap_or(0);
    let raw = &head[after_dot..];
    let lead = raw.len() - raw.trim_start().len();
    let name = raw.trim();
    if !is_plain_ident(name) {
        return None;
    }
    let start = s + after_dot + lead;
    Some((start as u32, (start + name.len()) as u32))
}

// ---------------------------------------------------------------------------
// E0210: the did-you-mean, when exactly one field is within edit distance one
// ---------------------------------------------------------------------------

/// Replace a field read the record does not declare with the one declared field
/// a single character away from it.
///
/// `alternatives` carries the record's own field list. Zero candidates within
/// distance one is a name that was not a typo; several is a choice the compiler
/// does not make. Both decline.
fn plan_e0210(source: &str, d: &Diagnostic) -> Result<(u32, u32, String), String> {
    let fields = d
        .alternatives
        .as_ref()
        .ok_or("the diagnostic carries no field list for the record")?;
    let (s, e) = range_bytes(source, d)
        .ok_or("the diagnostic's range is not a byte range of this file")?;
    let text = &source[s..e];
    let after_dot = text
        .rfind('.')
        .map(|i| i + 1)
        .ok_or("the access this diagnostic points at is not a `.field` read")?;
    let raw = &text[after_dot..];
    let lead = raw.len() - raw.trim_start().len();
    let wrong = raw.trim();
    if !is_plain_ident(wrong) {
        return Err("the field name this diagnostic points at is not a plain name".to_string());
    }
    let near: Vec<&String> = fields
        .iter()
        .filter(|f| distance_is_one(f, wrong))
        .collect();
    match near.len() {
        1 => {
            let start = s + after_dot + lead;
            Ok((
                start as u32,
                (start + wrong.len()) as u32,
                near[0].clone(),
            ))
        }
        0 => Err(format!(
            "no declared field is one character away from `{wrong}`; the record declares {}",
            quoted_list(fields)
        )),
        n => Err(format!(
            "{n} declared fields are one character away from `{wrong}` ({}), and only one \
             determines the repair",
            quoted_list(&near.iter().map(|s| (*s).clone()).collect::<Vec<_>>())
        )),
    }
}

// ---------------------------------------------------------------------------
// E0200: one arm per missing variant
// ---------------------------------------------------------------------------

/// Everything one non-exhaustive match needs to gain its arms.
struct ArmPlan {
    /// Byte offset the new arms are spliced in at: just past the last arm's
    /// own text, inside the braces.
    at: usize,
    /// The indentation the existing arms sit at.
    indent: String,
    /// The union as the diagnostic names it, for the marker comment.
    union_label: String,
    /// The module key the union is declared in, when that is not this file's
    /// own module: `orders` for a `cause` of `orders::OrderStatus`. `None` for
    /// a union this module declares, whose variant names are already in scope.
    ///
    /// The arms name variants, and a variant of an imported union reaches this
    /// file only through an import. This is what says which import to read.
    union_module: Option<String>,
    patterns: Vec<PatternPlan>,
}

/// One missing case, and what the compiler holds about its shape.
struct PatternPlan {
    /// The variant's name, or the literal's text for a string-literal union.
    name: String,
    /// The payload type as the checker renders it, `None` for a variant that
    /// declares none.
    payload: Option<String>,
    /// True for a member of a string-literal union (D30), whose pattern is the
    /// literal itself rather than a variant name.
    literal: bool,
}

/// Work out where the arms go and what shape each one has.
fn plan_e0200(
    source: &str,
    path: &Path,
    root: &Path,
    d: &Diagnostic,
) -> Result<ArmPlan, String> {
    let missing = d
        .missing_variants
        .as_ref()
        .filter(|m| !m.is_empty())
        .ok_or("the diagnostic names no missing variants")?;
    let cause = d.cause.as_deref().ok_or_else(|| {
        match d.union.as_ref().map(|u| u.name.clone()) {
            Some(name) => format!(
                "`{name}` is not declared in this project, so no tool keys its variants and \
                 `glyph fix` has no payload shapes to write patterns from"
            ),
            None => "the diagnostic names no declaration to read the variants from".to_string(),
        }
    })?;

    let shape = union_shape(root, path, cause)?;
    let mut patterns = Vec::new();
    for name in missing {
        match &shape {
            UnionShape::Literals(all) => {
                if !all.iter().any(|l| l == name) {
                    return Err(format!(
                        "`{cause}` does not list `{name}` among its literals, so the compiler \
                         and the diagnostic disagree and neither is written"
                    ));
                }
                patterns.push(PatternPlan {
                    name: name.clone(),
                    payload: None,
                    literal: true,
                });
            }
            UnionShape::Variants(all) => {
                let found = all.iter().find(|(v, _)| v == name).ok_or_else(|| {
                    format!(
                        "`{cause}` does not declare a variant `{name}`, so the compiler and the \
                         diagnostic disagree and neither is written"
                    )
                })?;
                patterns.push(PatternPlan {
                    name: name.clone(),
                    payload: found.1.clone(),
                    literal: false,
                });
            }
        }
    }

    let (at, indent) = match_insertion_point(source, d)
        .ok_or("the match this diagnostic points at does not end in a `}` in the source")?;
    let own_module = d.module.as_deref();
    let union_module = cause
        .rsplit_once("::")
        .map(|(m, _)| m.to_string())
        .filter(|m| Some(m.as_str()) != own_module);
    Ok(ArmPlan {
        at,
        indent,
        union_label: d
            .union
            .as_ref()
            .map(|u| u.name.clone())
            .unwrap_or_else(|| cause.to_string()),
        union_module,
        patterns,
    })
}

/// How this file reaches the variants of a union another module declares, and
/// the import edit that makes it reach the ones it does not reach yet.
///
/// Three import spellings, three answers. A namespace or aliased import binds
/// one name and every variant is reached through it, so the patterns are
/// written `orders.Paid(...)` and nothing is added. A named import binds each
/// variant separately, so the missing ones are spliced into its own name list.
/// A union this module declares needs neither.
///
/// This is half of G236. The other half is the self-check: a variant this
/// writes an import for is unresolved in a single-module read whatever the
/// import list says, which is why the check runs against the project.
enum VariantAccess {
    /// Write the patterns as they are; nothing to import.
    InScope,
    /// Write the patterns qualified by this binding.
    Qualified(String),
    /// Write the patterns bare, and splice this import's name list.
    Import { at: (u32, u32), text: String },
}

fn variant_access(
    module: &Module,
    source: &str,
    plan: &ArmPlan,
) -> Result<VariantAccess, String> {
    let Some(union_module) = plan.union_module.as_deref() else {
        return Ok(VariantAccess::InScope);
    };
    // A string-literal union has no variant names to bring into scope: the
    // patterns are the literals themselves.
    if plan.patterns.iter().all(|p| p.literal) {
        return Ok(VariantAccess::InScope);
    }
    // A union whose variants the prelude re-exports is already in scope with
    // no import: `Ok` and `Err` resolve in every module, which is why
    // `match r { Ok(v) => v, }` is a program somebody writes (G231).
    //
    // Keyed on the prelude's re-export table and not on `std/` as a whole.
    // `import std/array { map }` is a real import that a repair cannot skip;
    // what the prelude brings in is a fixed list, and only that list counts.
    if plan
        .patterns
        .iter()
        .all(|p| glyph_resolver::prelude_declaring_module(&p.name) == Some(union_module))
    {
        return Ok(VariantAccess::InScope);
    }
    for item in &module.items {
        let Decl::Import(imp) = item else { continue };
        let path_text = imp
            .path
            .segments
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join("/");
        if path_text != union_module {
            continue;
        }
        return match &imp.kind {
            ImportKind::Namespace => Ok(VariantAccess::Qualified(
                imp.path
                    .segments
                    .last()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| union_module.to_string()),
            )),
            ImportKind::Aliased(a) => Ok(VariantAccess::Qualified(a.to_string())),
            ImportKind::Named(ns) => {
                let mut names: Vec<String> = ns.iter().map(|n| n.to_string()).collect();
                let missing: Vec<&PatternPlan> = plan
                    .patterns
                    .iter()
                    .filter(|p| !names.iter().any(|n| n == &p.name))
                    .collect();
                if missing.is_empty() {
                    return Ok(VariantAccess::InScope);
                }
                for p in &missing {
                    if module.items.iter().any(|d| binds_name(d, &p.name)) {
                        return Err(format!(
                            "`{}` is already bound in this module by something other \
                             than the import of `{union_module}`, so `glyph fix` cannot \
                             bring the variant into scope without renaming what is there",
                            p.name
                        ));
                    }
                    names.push(p.name.clone());
                }
                let decl_end = decl_text_end(source, imp.span.start, imp.span.end);
                Ok(VariantAccess::Import {
                    at: (imp.span.start, decl_end),
                    text: format!("import {union_module} {{ {} }}", names.join(", ")),
                })
            }
            ImportKind::Default(_) => Err(format!(
                "`{union_module}` is imported as a default binding, which names no \
                 variants, and `glyph fix` does not rewrite an import you wrote"
            )),
        };
    }
    Err(format!(
        "this module has no `import {union_module}`, so `glyph fix` cannot tell how \
         the variants of `{}` are meant to be spelled here",
        plan.union_label
    ))
}

/// What a union declares, as the compiler answers it.
enum UnionShape {
    Variants(Vec<(String, Option<String>)>),
    Literals(Vec<String>),
}

/// Ask `glyph_symbol` what `entity` declares.
///
/// The same `call_tool` an MCP client reaches and `glyph query symbol` prints,
/// so the payload shapes written into an arm are the ones the compiler reports
/// everywhere else. Reading the union's declaration a second time here would be
/// a second answer to the same question, and the two would disagree the first
/// time either moved.
fn union_shape(root: &Path, path: &Path, entity: &str) -> Result<UnionShape, String> {
    // An absolute path, because the tool resolves a relative one against its
    // own root and `path` here is already spelled relative to that root: the
    // two composed would name `src/src/main.glyph`.
    let file = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let answer = glyph_lsp::call_mcp_tool(
        root.to_path_buf(),
        "glyph_symbol",
        serde_json::json!({ "entity": entity, "path": file.to_string_lossy() }),
    )
    .map_err(|why| format!("`glyph_symbol` did not answer for `{entity}`: {why}"))?;
    let value: serde_json::Value = serde_json::from_str(&answer)
        .map_err(|e| format!("`glyph_symbol`'s answer for `{entity}` did not parse: {e}"))?;

    if let Some(lits) = value.get("literals").and_then(|v| v.as_array()) {
        return Ok(UnionShape::Literals(
            lits.iter()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect(),
        ));
    }
    let variants = value
        .get("variants")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            let why = value
                .get("variants_absent")
                .and_then(|v| v.as_str())
                .unwrap_or("it reports no variants");
            format!("`glyph_symbol` holds no variant list for `{entity}`: {why}")
        })?;
    Ok(UnionShape::Variants(
        variants
            .iter()
            .filter_map(|v| {
                let name = v.get("name")?.as_str()?.to_string();
                let payload = v
                    .get("payload")
                    .and_then(|p| p.as_str())
                    .map(str::to_string);
                Some((name, payload))
            })
            .collect(),
    ))
}

/// Where the new arms go, and at what indentation.
///
/// The diagnostic's range is the whole `match` expression, so the last byte is
/// its closing brace and the last non-space byte before that is the end of the
/// last arm's own text (its trailing comma, which D8 requires). The new arms
/// are spliced in there, at the indentation of the line that byte sits on,
/// which is the arms' own indentation whatever shape the last arm's body took.
fn match_insertion_point(source: &str, d: &Diagnostic) -> Option<(usize, String)> {
    let (s, e) = range_bytes(source, d)?;
    let bytes = source.as_bytes();
    let mut close = e.checked_sub(1)?;
    while close > s && bytes[close].is_ascii_whitespace() {
        close -= 1;
    }
    if bytes[close] != b'}' {
        return None;
    }
    let mut at = close.checked_sub(1)?;
    while at > s && bytes[at].is_ascii_whitespace() {
        at -= 1;
    }
    if at <= s {
        return None;
    }
    let at = at + 1;
    let line_start = source[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let indent: String = source[line_start..at]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    Some((at, indent))
}

/// The edits one file's `E0200` repairs come to, with what to report for them.
struct PlannedArms {
    edits: Vec<(u32, u32, String)>,
    applied: Vec<Applied>,
}

/// Build the text edits for every planned match in one file, and settle the two
/// questions that are per-file rather than per-match: how `process.exit` is
/// spelled here, and whether the payload patterns can be destructured.
fn plan_arm_edits(
    source: &str,
    module: &Module,
    path: &Path,
    root: &Path,
    plans: &[ArmPlan],
    report: &mut FixReport,
) -> Option<PlannedArms> {
    let (exit_call, import_edit) = match process_exit_call(module, source) {
        Ok(v) => v,
        Err(why) => {
            for plan in plans {
                report.declined.push(Declined {
                    code: "E0200".to_string(),
                    file: path.to_path_buf(),
                    why: why.clone(),
                });
                let _ = plan;
            }
            return None;
        }
    };

    // How each match's variants are spelled here, and the import list that has
    // to grow for them to be. One answer per plan, and a plan whose access
    // cannot be settled declines on its own rather than taking the file's
    // other matches down with it.
    let mut access: Vec<VariantAccess> = Vec::new();
    for plan in plans {
        match variant_access(module, source, plan) {
            Ok(a) => access.push(a),
            Err(why) => {
                report.declined.push(Declined {
                    code: "E0200".to_string(),
                    file: path.to_path_buf(),
                    why,
                });
                return None;
            }
        }
    }
    // What the file's named imports bound before this rule touched them, so
    // the report names the variants it added and not the ones already there.
    let imported_before: Vec<String> = module
        .items
        .iter()
        .filter_map(|d| match d {
            Decl::Import(i) => Some(&i.kind),
            _ => None,
        })
        .filter_map(|k| match k {
            ImportKind::Named(ns) => Some(ns.iter().map(|n| n.to_string())),
            _ => None,
        })
        .flatten()
        .collect();
    // An import decl is rewritten whole, so two matches over unions from the
    // same module must not each splice their own version of it.
    let mut import_rewrites: Vec<(u32, u32, String)> = Vec::new();
    for a in &access {
        let VariantAccess::Import { at, text } = a else { continue };
        match import_rewrites.iter_mut().find(|(s, e, _)| (*s, *e) == *at) {
            Some(existing) => existing.2 = merge_named_imports(&existing.2, text),
            None => import_rewrites.push((at.0, at.1, text.clone())),
        }
    }

    // A record payload is destructured by field name, which is the shape that
    // puts the payload's parts in front of whoever writes the body. A field
    // name can still be illegal as a binding (E0109: TypeScript reserves it),
    // and rather than keep a second copy of that list here, the destructured
    // form is assembled, offered to the compiler's own collect stage, and
    // dropped for a whole-payload binding if it made the file worse.
    let baseline = project_error_codes(root, path, source);
    let mut last_why: Option<String> = None;
    for destructure in [true, false] {
        let mut edits: Vec<(u32, u32, String)> = Vec::new();
        let mut applied = Vec::new();
        if let Some((at, text)) = &import_edit {
            edits.push((*at as u32, *at as u32, text.clone()));
        }
        edits.extend(import_rewrites.iter().cloned());
        for (plan, access) in plans.iter().zip(access.iter()) {
            let qualifier = match access {
                VariantAccess::Qualified(q) => Some(q.as_str()),
                _ => None,
            };
            edits.push((
                plan.at as u32,
                plan.at as u32,
                render_arms(plan, destructure, &exit_call, qualifier),
            ));
            // Every byte this rule writes is named in the report. The arms
            // were; the two imports were not, and a report that enumerates
            // some of its own edits is what a CI log or a review captures.
            let mut what = format!(
                "added {} arm(s) to the match on `{}`: {}",
                plan.patterns.len(),
                plan.union_label,
                quoted_list(
                    &plan
                        .patterns
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>()
                )
            );
            let mut wrote: Vec<String> = Vec::new();
            if import_edit.is_some() && applied.is_empty() {
                wrote.push("`import std/process` for the arm bodies".to_string());
            }
            if let VariantAccess::Import { .. } = access {
                if let Some(union_module) = plan.union_module.as_deref() {
                    let added: Vec<String> = plan
                        .patterns
                        .iter()
                        .filter(|p| !imported_before.iter().any(|n| n == &p.name))
                        .map(|p| p.name.clone())
                        .collect();
                    if !added.is_empty() {
                        wrote.push(format!(
                            "{} into `import {union_module}`",
                            quoted_list(&added)
                        ));
                    }
                }
            }
            if !wrote.is_empty() {
                what.push_str(&format!(", and wrote {}", wrote.join(" and ")));
            }
            applied.push(Applied {
                code: "E0200".to_string(),
                file: path.to_path_buf(),
                what,
            });
        }
        let candidate = apply_edits(source, edits.clone());
        let after = project_error_codes(root, path, &candidate);
        match regression(&baseline, &after) {
            None => return Some(PlannedArms { edits, applied }),
            // Keep the first shape's reason: the destructured form is the one
            // the rule prefers, and its failure is the one worth reporting if
            // the whole-payload form fails too.
            Some(why) if last_why.is_none() || !destructure => last_why = Some(why),
            Some(_) => {}
        }
    }

    let why = last_why.unwrap_or_else(|| {
        "the arms this fix would write do not check cleanly in this project, and the check \
         reported no diagnostic naming why"
            .to_string()
    });
    for _ in plans {
        report.declined.push(Declined {
            code: "E0200".to_string(),
            file: path.to_path_buf(),
            why: why.clone(),
        });
    }
    None
}

/// Merge two rewrites of one named import into the union of their name lists.
///
/// Two matches in one file over unions from the same module each ask for their
/// own variants, and the import decl is rewritten whole, so the second rewrite
/// would otherwise drop the first one's names.
fn merge_named_imports(a: &str, b: &str) -> String {
    let names_of = |text: &str| -> (String, Vec<String>) {
        let Some((head, rest)) = text.split_once('{') else {
            return (text.to_string(), Vec::new());
        };
        let list = rest.trim_end().trim_end_matches('}');
        (
            head.to_string(),
            list.split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
        )
    };
    let (head, mut names) = names_of(a);
    let (_, extra) = names_of(b);
    for n in extra {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    format!("{}{{ {} }}", head, names.join(", "))
}

/// The error codes `text` draws at `path` inside its project, or `None` when
/// the project could not be read at all.
///
/// The project and not the file (G236). A variant declared in another module
/// is an unresolved name in a single-module read, so the arms this rule writes
/// looked like new `E0103`s to a check that read the candidate alone, and the
/// rule declined every `match` over an imported union. This is the same
/// project database `glyph check` and `glyph_diagnostics` answer from, and the
/// candidate is never written to disk.
fn project_error_codes(root: &Path, path: &Path, text: &str) -> Option<Vec<(String, String)>> {
    let diags = glyph_lsp::file_diagnostics_with_text(root.to_path_buf(), path, text).ok()?;
    let mut out: Vec<(String, String)> = diags
        .iter()
        .filter(|d| d.severity == "error")
        .map(|d| (d.code.clone(), d.message.clone()))
        .collect();
    out.sort();
    Some(out)
}

/// The first diagnostic the candidate raised that the file did not, as the
/// sentence a decline reports, or `None` when nothing regressed.
///
/// The reason names the diagnostic. "the arms this fix would write do not
/// collect cleanly in this module" was the whole of what a decline used to
/// say, and the release claims every refusal says why; a catch-all is not a
/// why.
fn regression(
    before: &Option<Vec<(String, String)>>,
    after: &Option<Vec<(String, String)>>,
) -> Option<String> {
    let (Some(before), Some(after)) = (before, after) else {
        return Some(
            "the file this fix would write could not be checked against its project, so \
             nothing was written"
                .to_string(),
        );
    };
    for d in after {
        if after.iter().filter(|c| *c == d).count() > before.iter().filter(|c| *c == d).count() {
            return Some(format!(
                "the arms this fix would write draw `[{}] {}`, which this file does not draw \
                 now, so nothing was written",
                d.0, d.1
            ));
        }
    }
    None
}

/// The arm text for one planned match, indentation included, ready to splice in
/// just past the last existing arm.
///
/// Each arm carries a `TODO(glyph fix)` line naming the case, and a body that
/// prints which case was reached and then leaves through `process.exit`, whose
/// return type is `never` (D43). `never` contributes nothing to the arm join,
/// so the same body is legal whether the match is an expression owing a value
/// or a statement owing none, and the compiler needs no guess about what the
/// arm should produce. It is not a body anyone would mistake for a finished
/// one: it says so in a comment, and it says so again at runtime.
fn render_arms(
    plan: &ArmPlan,
    destructure: bool,
    exit_call: &str,
    qualifier: Option<&str>,
) -> String {
    let mut out = String::new();
    let i = &plan.indent;
    for p in &plan.patterns {
        let (pattern, label) = if p.literal {
            (format!("\"{}\"", escape_glyph_string(&p.name)), "literal")
        } else {
            // A namespace or aliased import binds the module, not the
            // variants, so a pattern over one is written through the binding.
            let head = match qualifier {
                Some(q) => format!("{q}.{}", p.name),
                None => p.name.clone(),
            };
            (
                match &p.payload {
                    None => head,
                    Some(payload) => match record_field_names(payload).filter(|_| destructure) {
                        Some(fields) => format!("{head}({{ {} }})", fields.join(", ")),
                        None => format!("{head}(payload)"),
                    },
                },
                "variant",
            )
        };
        out.push('\n');
        out.push_str(&format!(
            "{i}// TODO(glyph fix): `{}` {label} `{}` is unhandled; write this arm\n",
            plan.union_label, p.name
        ));
        out.push_str(&format!("{i}{pattern} => {{\n"));
        out.push_str(&format!(
            "{i}  print(\"unhandled {} {label} {} (arm written by glyph fix)\")\n",
            plan.union_label, p.name
        ));
        out.push_str(&format!("{i}  {exit_call}\n"));
        out.push_str(&format!("{i}}},"));
    }
    out
}

/// How this module reaches `std/process.exit`, and the import to add when it
/// does not reach it yet.
///
/// Returns the call text and, when one is needed, the byte offset an
/// `import std/process` goes at with the text to put there.
fn process_exit_call(
    module: &Module,
    source: &str,
) -> Result<(String, Option<(usize, String)>), String> {
    for item in &module.items {
        let Decl::Import(imp) = item else { continue };
        let segments: Vec<&str> = imp.path.segments.iter().map(|s| s.as_ref()).collect();
        if segments != ["std", "process"] {
            continue;
        }
        return match &imp.kind {
            ImportKind::Namespace => {
                let bound = segments.last().copied().unwrap_or("process");
                Ok((format!("{bound}.exit(1)"), None))
            }
            ImportKind::Aliased(a) => Ok((format!("{a}.exit(1)"), None)),
            ImportKind::Named(ns) if ns.iter().any(|n| n.as_ref() == "exit") => {
                Ok(("exit(1)".to_string(), None))
            }
            _ => Err("this module imports `std/process` in a form that does not bind `exit`, \
                      and `glyph fix` does not rewrite an import you wrote"
                .to_string()),
        };
    }

    if module.items.iter().any(|d| binds_name(d, "process")) {
        return Err(
            "the name `process` is already bound in this module, so `glyph fix` cannot reach \
             `std/process.exit` for the arm bodies"
                .to_string(),
        );
    }

    let anchor = module
        .items
        .iter()
        .rev()
        .find_map(|d| match d {
            Decl::Import(i) => Some(i.span),
            _ => None,
        })
        .map(|sp| sp.start as usize)
        .or_else(|| module.module_path.as_ref().map(|m| m.span.start as usize))
        .ok_or(
            "this file has no `module` line and no import to put `import std/process` after",
        )?;
    let at = end_of_line_at(source, anchor);
    Ok((
        "process.exit(1)".to_string(),
        Some((at, "\nimport std/process".to_string())),
    ))
}

/// Whether a declaration binds `name` at the module's top level.
fn binds_name(decl: &Decl, name: &str) -> bool {
    match decl {
        Decl::Import(i) => match &i.kind {
            ImportKind::Namespace => i.path.segments.last().map(|s| s.as_ref()) == Some(name),
            ImportKind::Aliased(a) => a.as_ref() == name,
            ImportKind::Default(local) => local.as_ref() == name,
            ImportKind::Named(ns) => ns.iter().any(|n| n.as_ref() == name),
        },
        Decl::Fn(f) => f.name.as_ref() == name,
        Decl::Type(t) => t.name.as_ref() == name,
        Decl::Const(c) => c.name.as_ref() == name,
        Decl::Component(c) => c.name.as_ref() == name,
        Decl::Interface(i) => i.name.as_ref() == name,
    }
}

/// The offset of the newline that ends the line `offset` sits on, with a span
/// that already ran past its own newline walked back first (an import decl's
/// span does, see `decl_text_end`).
fn end_of_line_at(source: &str, offset: usize) -> usize {
    let mut o = offset.min(source.len());
    let bytes = source.as_bytes();
    while o > 0 && matches!(bytes[o - 1], b'\n' | b'\r') {
        o -= 1;
    }
    match source[o..].find('\n') {
        Some(i) => o + i,
        None => source.len(),
    }
}

// ---------------------------------------------------------------------------
// Small shared helpers
// ---------------------------------------------------------------------------

/// A diagnostic's range as a byte range of `source`, when it is one.
fn range_bytes(source: &str, d: &Diagnostic) -> Option<(usize, usize)> {
    let s = d.range.start.offset as usize;
    let e = d.range.end.offset as usize;
    if e <= s || e > source.len() || !source.is_char_boundary(s) || !source.is_char_boundary(e) {
        return None;
    }
    Some((s, e))
}

/// True when `name` is a bare identifier: a letter or `_`, then letters, digits
/// or `_`.
fn is_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True when `a` and `b` differ by exactly one character edit: a substitution,
/// an insertion, or a deletion.
///
/// Not a general edit distance. The rule only ever asks about distance one, and
/// answering that question directly is exact, allocation-light, and impossible
/// to get subtly wrong the way a truncated matrix can be.
fn distance_is_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    match a.len() as i64 - b.len() as i64 {
        0 => a.iter().zip(b.iter()).filter(|(x, y)| x != y).count() == 1,
        1 => one_deletion_apart(&a, &b),
        -1 => one_deletion_apart(&b, &a),
        _ => false,
    }
}

/// `long` with exactly one character dropped equals `short`.
fn one_deletion_apart(long: &[char], short: &[char]) -> bool {
    let mut i = 0;
    let mut j = 0;
    let mut dropped = false;
    while i < long.len() && j < short.len() {
        if long[i] == short[j] {
            i += 1;
            j += 1;
        } else if dropped {
            return false;
        } else {
            dropped = true;
            i += 1;
        }
    }
    true
}

/// The field names of a record type as the checker renders it (`{ a: int, b:
/// string }`), or `None` when the text is not a record type or any field name
/// is not a plain name.
///
/// The commas are split at brace/bracket/paren/angle depth zero so a nested
/// record or a generic argument does not end a field, and a split that does not
/// yield a plain name answers `None` rather than a guess: the caller falls back
/// to binding the whole payload.
fn record_field_names(payload: &str) -> Option<Vec<String>> {
    let text = payload.trim();
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    let mut fields = Vec::new();
    for part in split_top_level(inner) {
        let head = split_top_level_once(&part)?;
        let name = head.trim().trim_end_matches('?').trim();
        if !is_plain_ident(name) {
            return None;
        }
        fields.push(name.to_string());
    }
    if fields.is_empty() {
        return None;
    }
    Some(fields)
}

/// Split on commas at nesting depth zero.
fn split_top_level(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    let mut prev = '\0';
    for c in text.chars() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            '<' => depth += 1,
            // `->` in a function type is not a closing angle bracket.
            '>' if prev != '-' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                prev = c;
                continue;
            }
            _ => {}
        }
        current.push(c);
        prev = c;
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// The text before the first `:` at nesting depth zero.
fn split_top_level_once(text: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut prev = '\0';
    for (i, c) in text.char_indices() {
        match c {
            '{' | '[' | '(' | '<' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            '>' if prev != '-' => depth -= 1,
            ':' if depth == 0 => return Some(text[..i].to_string()),
            _ => {}
        }
        prev = c;
    }
    None
}

/// `"a"`, `"b"` for a list of names, for a sentence that enumerates them.
fn quoted_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Escape a string literal's text for a Glyph double-quoted string. `${` would
/// open an interpolation, so the backslash before `$` matters as much as the
/// one before a quote.
fn escape_glyph_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `glyph fix` must not eat the line after the import it removes.
    ///
    /// An import decl's span already ends past its own newline: the parser
    /// takes the end from `peek_span()` while the peeked token is the
    /// `Newline`. Scanning forward from there found the NEXT line's newline,
    /// so removing an unused import took the following line with it. On this
    /// fixture that line is `fn main() -> void {`, and the file was left
    /// unparseable. A tool that edits source is held to a higher bar than one
    /// that only reports: a wrong report wastes a minute, a wrong edit costs
    /// work.
    #[test]
    fn removing_an_import_does_not_eat_the_line_below_it() {
        let src = "module m\nimport std/io\nimport std/string\nfn main() -> void {\n  io.println(\"hi\")\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix_below_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        fix_project(&dir).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(
            after.contains("fn main() -> void {"),
            "the line after the removed import must survive:\n{after}"
        );
        assert!(!after.contains("import std/string"), "dead import gone:\n{after}");
        assert!(after.contains("import std/io"), "live import kept:\n{after}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pruning SOME names out of an import must not eat the newline that
    /// terminates it. The whole-import path learned this once (see
    /// `removing_an_import_does_not_eat_the_line_below_it`); the partial-prune
    /// path shipped in 0.1.99 spliced over the raw span and welded the
    /// rewritten import onto the next line, so `glyph fix` reported success and
    /// left a file that would not parse. Both shapes below are ordinary Glyph.
    #[test]
    fn partial_prune_keeps_the_newline_before_the_next_declaration() {
        let src = "module m\nimport std/result { Result, Ok, Err }\nfn main() -> void {\n  let _ = Ok(1)\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix_pp_decl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        fix_project(&dir).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(
            glyph_parser::parse(&after).is_ok(),
            "the rewritten file must still parse:\n{after}"
        );
        assert!(
            after.contains("}\nfn main() -> void {"),
            "the newline terminating the import must survive:\n{after}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same defect with an import on the following line rather than a
    /// declaration: consecutive imports are at least as common.
    #[test]
    fn partial_prune_keeps_the_newline_before_the_next_import() {
        let src = "module m\nimport std/result { Result, Ok, Err }\nimport std/io\nfn main() -> void {\n  io.println(\"hi\")\n  let _ = Ok(1)\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix_pp_imp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        fix_project(&dir).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(
            glyph_parser::parse(&after).is_ok(),
            "the rewritten file must still parse:\n{after}"
        );
        assert!(
            after.contains("import std/io"),
            "the import on the next line must survive:\n{after}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two adjacent dead imports produced overlapping edit ranges, which
    /// panicked in `apply_edits` on a reversed slice index.
    #[test]
    fn two_adjacent_dead_imports_do_not_overlap() {
        let src = "module m\nimport std/io\nimport std/string\nimport std/math\nfn main() -> void {\n  io.println(\"hi\")\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix_adj_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        fix_project(&dir).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(after.contains("fn main() -> void {"), "{after}");
        assert!(!after.contains("std/string"), "{after}");
        assert!(!after.contains("std/math"), "{after}");
        assert!(after.contains("import std/io"), "{after}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_a_fully_unused_import_keeps_a_used_one() {
        let src = "module m\n\
            import std/io\n\
            import std/string\n\
            fn main() -> void {\n  io.println(\"hi\")\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        let report = fix_project(&dir).unwrap();
        assert_eq!(report.removed_imports, 1, "one unused import removed");
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(!after.contains("import std/string"), "unused import gone:\n{after}");
        assert!(after.contains("import std/io"), "used import kept:\n{after}");
        assert!(after.contains("io.println"), "body intact:\n{after}");
    }

    #[test]
    fn keeps_a_partially_used_named_import() {
        // `Result` and `Ok` are used, `Err` is not: `Err` is trimmed but the
        // import stays (the still-live names must not disappear with it).
        let src = "module m\n\
            import std/result { Result, Ok, Err }\n\
            fn f() -> Result<number, string> {\n  return Ok(1)\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        let report = fix_project(&dir).unwrap();
        assert_eq!(report.removed_imports, 1, "the one dead name is trimmed");
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(!after.contains("Err"), "dead name gone:\n{after}");
        assert!(after.contains("Result") && after.contains("Ok"), "live names kept:\n{after}");
        assert!(after.contains("Result<number, string>"), "body intact:\n{after}");
    }

    /// A stale G152 reproduction: `glyph fix` used to report "removed 0 unused
    /// import(s)" and leave a named import with *some* dead names byte-for-byte
    /// untouched, so the E0106 warning for each dead name never went away no
    /// matter how many times you ran `fix`. Only the all-names-dead case was
    /// ever handled. Two of three names are dead here; only `Ok` is used.
    #[test]
    fn trims_the_dead_names_out_of_a_partially_used_named_import() {
        let src = "module m\n\
            import std/result { Result, Ok, Err }\n\
            fn f() -> number {\n  return match Ok(1) {\n    Ok(x) => x,\n    else => 0,\n  }\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        let report = fix_project(&dir).unwrap();
        assert_eq!(report.removed_imports, 2, "both dead names (Result, Err) trimmed");
        assert_eq!(report.changed.len(), 1);
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(!after.contains("Result"), "dead name gone:\n{after}");
        assert!(!after.contains("Err"), "dead name gone:\n{after}");
        assert!(after.contains("Ok"), "live name kept:\n{after}");
        assert!(after.contains("std/result"), "import path kept:\n{after}");
        assert!(after.contains("match Ok(1)"), "body intact:\n{after}");
    }

    /// Named-import lists may span multiple lines (`parse_comma_separated` is
    /// called with `skip_newlines: true` here), so a text-range fix keyed on
    /// the import's byte span, not "the line", must handle this shape too.
    #[test]
    fn trims_a_dead_name_from_a_multi_line_named_import() {
        let src = "module m\n\
            import std/result {\n  Result,\n  Ok,\n  Err,\n}\n\
            fn f() -> Result<number, string> {\n  return Ok(1)\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix4_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        let report = fix_project(&dir).unwrap();
        assert_eq!(report.removed_imports, 1, "the one dead name (Err) is trimmed");
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(!after.contains("Err"), "dead name gone:\n{after}");
        assert!(after.contains("Result") && after.contains("Ok"), "live names kept:\n{after}");
        assert!(after.contains("Result<number, string>"), "body intact:\n{after}");
        // The rewritten import must still be one legal `import` declaration
        // starting at the beginning of a line, whatever line shape it picks.
        assert!(
            after.lines().any(|l| l.trim_start().starts_with("import std/result")),
            "still a valid import line:\n{after}"
        );
    }
}
