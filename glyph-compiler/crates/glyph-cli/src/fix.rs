//! `glyph fix` — apply the repairs the compiler fully determines.
//!
//! Three rules. The first drops the dead names out of an `import` (the E0106
//! lint). An import whose every bound name is unused loses the whole
//! declaration; a named import (`import M { a, b, c }`) with only some names
//! dead (say `b`) keeps the declaration and drops just `b`. Only `Named` can
//! be partially dead: `Namespace`/`Aliased`/`Default` each bind a single
//! name, so for those "some dead" and "all dead" are the same case.
//!
//! The other two read the structured diagnostics of a real check and edit
//! the source the diagnostic points at:
//!
//! * **E0220**, an arm head that is not a variant, takes the `suggestion` the
//!   checker already computed, and only when there is exactly one.
//! * **E0210**, a field that the record does not declare, takes the one
//!   declared field within edit distance one of it, and only when there is
//!   exactly one.
//!
//! * **E0240**, a call that changes its receiver written as a bare
//!   statement, takes the `mut ` D5 asks for (G380). Only that one shape:
//!   an expression statement whose whole expression is the call, on a
//!   receiver that is a place. A call whose value is used, or an arm of a
//!   statement `match`, has a repair that depends on what was meant.
//!
//! * **E0242**, a built-in method that answers absence with `undefined`,
//!   takes the rewrite the checker attached: `xs.find(f)` becomes
//!   `array.find(xs, f)`, `xs.at(2)` becomes `array.get(xs, 2)`, and a `??`
//!   default becomes the `match` that handles the `None` (G392, D56). When
//!   the module has no `import std/array`, one is added (`ensure_import`),
//!   unless another binding already has that name.
//!
//! * **E0236**, `??` on an `Option` (G429, decision record 157), takes the
//!   `match` the checker attached: `u ?? 0` becomes `match u { Some(n) => n,
//!   None => 0, }`. That changes what the program does, to the one thing the
//!   text can mean, and it changes the expression's type from unchecked to
//!   the payload's, so the rewrite is written, the tree is checked again, and
//!   it is kept only when no new error appears (`apply_checked_rewrites`).
//!   A `Result` is declined: whether an `Err` is dropped, propagated or
//!   handled is not determined.
//!
//! Neither guesses. Where the compiler does not hold the answer the rule
//! declines and says why, and the reason reaches the command's output beside
//! what it did apply.
//!
//! **E0200**, a non-exhaustive match, is reported and never repaired. The
//! compiler knows which cases are missing and does not know what any of them
//! should do, and an arm with an invented body turns a compile error into a
//! runtime one. See `e0200_decline`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glyph_ast::{Decl, ImportKind};
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
            // The import's own span, read off this parse, must still be an
            // `import` in the text: the same guard the diagnostic rules get.
            if !source.get(imp.span.start as usize..).is_some_and(|t| t.starts_with("import")) {
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
    if apply_diagnostic_rules(src, &files, &mut report)? {
        apply_checked_rewrites(src, &files, &mut report)?;
    }

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

/// The codes this pass reads. A diagnostic with any other code is left alone.
///
/// `E0210`, `E0220`, `E0240` and `E0242` are repaired when the compiler
/// determines the repair. `E0236` on a `??` is repaired afterwards, one
/// rewrite at a time with a check after each (`apply_checked_rewrites`); here
/// only its declines are reported.
/// `E0200` is read and never repaired: each one is reported as declined, and
/// its `missing_variants` is what the `E0220` rule asks to learn whether the
/// variant it would rename a head to is still free.
const RULE_CODES: [&str; 6] = ["E0200", "E0210", "E0220", "E0236", "E0240", "E0242"];

/// Check the tree, then apply every rule that the resulting diagnostics fully
/// determine. Answers whether the check attached a rewrite that has to be
/// re-checked (E0236's), which `apply_checked_rewrites` then applies.
///
/// The check is the real one (`glyph check --no-tsc`, the same call the command
/// makes), so the offsets, the union identities and the checker's own
/// suggestions all come from the compiler rather than from a second reading of
/// the source here.
fn apply_diagnostic_rules(
    src: &Path,
    files: &[PathBuf],
    report: &mut FixReport,
) -> Result<bool, String> {
    let check = match crate::check::check_path(src, false, false) {
        Ok(c) => c,
        Err(e) => {
            report.notices.push(format!(
                "the tree did not check, so only the unused-import rule ran: {e}"
            ));
            return Ok(false);
        }
    };
    let has_checked = check
        .structured
        .iter()
        .any(|d| matches!(d.repair, Some(glyph_typechecker::AbsentRepair::Checked { .. })));

    // Group by the file the diagnostic is in, resolved to a path this run
    // actually collected. A diagnostic whose file cannot be placed in exactly
    // one of the collected files is left alone rather than written to a guess.
    let mut by_file: BTreeMap<PathBuf, Vec<Diagnostic>> = BTreeMap::new();
    for d in &check.structured {
        if !RULE_CODES.contains(&d.code.as_str()) {
            continue;
        }
        let Some(path) = place_diagnostic(&check.project_srcs, files, d) else {
            continue;
        };
        by_file.entry(path).or_default().push(d.clone());
    }

    for (path, diags) in by_file {
        // The checker's coverage relation for this file, from the same check
        // that produced its diagnostics: one load of the tree, one view of it.
        let coverage = check
            .coverage
            .iter()
            .find(|c| same_file(&c.file, &path))
            .map(|c| &c.coverage);
        fix_one_file(&path, &diags, coverage, report)?;
    }
    Ok(has_checked)
}

/// E0236 on a `??` (G429): write each `match` the checker attached, check the
/// tree again, and keep the rewrite only when no new error appears; otherwise
/// put the file back and decline, naming the error it would have caused.
///
/// The rewrite changes what the expression is: `u ?? 0` was an unchecked
/// value and becomes a `number`, so a slot that took it unchecked may now be
/// refused. "New" is the identity an edit through the protocol is held to
/// (review 88, 4b): code, severity, subject and range, with the file's
/// earlier diagnostics mapped through the edited regions, and an exact match
/// in every other file. Runs after the other rules, on a fresh check, so its
/// offsets are the file's current ones.
fn apply_checked_rewrites(src: &Path, files: &[PathBuf], report: &mut FixReport) -> Result<(), String> {
    let Ok(check) = crate::check::check_path(src, false, false) else {
        return Ok(());
    };
    let mut before: BTreeMap<PathBuf, Vec<Diagnostic>> = BTreeMap::new();
    let mut elsewhere_before: Vec<Diagnostic> = Vec::new();
    for d in &check.structured {
        match place_diagnostic(&check.project_srcs, files, d) {
            Some(path) => before.entry(path).or_default().push(d.clone()),
            None => elsewhere_before.push(d.clone()),
        }
    }
    // Per file, each rewrite's edits and the line that says what it changes.
    type Rewrites = Vec<(Vec<Edit>, String, glyph_typechecker::absent::NodeShape)>;
    let targets: Vec<(PathBuf, Rewrites)> = before
        .iter()
        .map(|(path, diags)| {
            let rewrites = diags
                .iter()
                .filter(|d| d.code == "E0236")
                .filter_map(|d| match &d.repair {
                    Some(glyph_typechecker::AbsentRepair::Checked { edits, what, node }) => {
                        Some((edits.clone(), what.clone(), node.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            (path.clone(), rewrites)
        })
        .filter(|(_, r)| !r.is_empty())
        .collect();

    for (path, rewrites) in targets {
        let source = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file_before = before.get(&path).cloned().unwrap_or_default();
        let mut kept: Vec<Edit> = Vec::new();
        // Reverted rewrites, with the new error's offset in `source` (or
        // its position elsewhere, already final), reported once the file
        // as written is known.
        let mut reverted: Vec<(String, Option<u32>, String)> = Vec::new();
        for (edits, what, node) in rewrites {
            if !node_holds(&source, &node) {
                report.declined.push(Declined { code: "E0236".to_string(), file: path.clone(), why: NODE_DRIFT.to_string() });
                continue;
            }
            let overlaps = edits
                .iter()
                .any(|(s, e, _)| kept.iter().any(|(ks, ke, _)| s < ke && ks < e && !(s == e && ks == ke)));
            if overlaps {
                report.declined.push(Declined {
                    code: "E0236".to_string(),
                    file: path.clone(),
                    why: "another repair in this file rewrites the same text; run `glyph fix` again".to_string(),
                });
                continue;
            }
            let mut candidate_edits = kept.clone();
            candidate_edits.extend(indent_edits(&source, &edits));
            candidate_edits.sort_by_key(|(s, _, _)| *s);
            let candidate = apply_edits(&source, candidate_edits.clone());
            if glyph_parser::parse(&candidate).is_err() {
                return Err(format!(
                    "{}: the rewritten file does not parse, so nothing was written. \
                     This is a bug in `glyph fix`; please report it with the file.",
                    path.display()
                ));
            }
            std::fs::write(&path, &candidate).map_err(|e| format!("{}: {e}", path.display()))?;
            let new_error = match crate::check::check_path(src, false, false) {
                Ok(after) => first_new_error(&source, &file_before, &elsewhere_before, &candidate_edits, &path, files, &after),
                Err(e) => Some(NewError { what: format!("the tree no longer checks ({e})"), offset: None, place: String::new() }),
            };
            match new_error {
                None => {
                    kept = candidate_edits;
                    report.applied.push(Applied { code: "E0236".to_string(), file: path.clone(), what });
                }
                Some(e) => {
                    // An offset in the candidate text is carried back into
                    // `source`, so the line reported is one the file has.
                    let offset = e.offset.map(|o| unmap_offset(&candidate_edits, o));
                    reverted.push((e.what, offset, e.place));
                }
            }
        }
        let settled = apply_edits(&source, kept.clone());
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        for (what, offset, place) in reverted {
            let place = match offset {
                Some(o) => {
                    let at = map_offset(&kept, o) as usize;
                    let line = settled[..at.min(settled.len())].matches('\n').count() + 1;
                    format!(" ({file_name}:{line})")
                }
                None => place,
            };
            report.declined.push(Declined {
                code: "E0236".to_string(),
                file: path.clone(),
                why: format!(
                    "the rewrite gives the expression its payload's type, and the check then reports {what}{place}, \
                     so it was reverted; the help names the `match` to write"
                ),
            });
        }
        std::fs::write(&path, &settled).map_err(|e| format!("{}: {e}", path.display()))?;
        if !kept.is_empty() && !report.changed.iter().any(|c| same_file(c, &path)) {
            report.changed.push(path.clone());
        }
    }
    Ok(())
}

/// An error a rewrite introduced: `[code] message`, and where it is.
struct NewError {
    what: String,
    /// Its byte offset in the rewritten file, when it is in that file.
    offset: Option<u32>,
    /// ` (file:line)` for an error in another file, which no edit moved.
    place: String,
}

/// `offset` in the text `edits` (sorted, applied to `source`) produce,
/// carried back into `source`: an offset inside an edit's new text is the
/// edit's start.
fn unmap_offset(edits: &[Edit], offset: u32) -> u32 {
    let mut shift: i64 = 0;
    for (s, e, text) in edits {
        let new_start = *s as i64 + shift;
        let new_end = new_start + text.len() as i64;
        if (offset as i64) < new_start {
            break;
        }
        if (offset as i64) < new_end {
            return *s;
        }
        shift += text.len() as i64 - (*e as i64 - *s as i64);
    }
    (offset as i64 - shift).max(0) as u32
}

/// `offset` in `source`, carried forward through `edits` (sorted): an
/// offset inside a replaced range lands at the start of its new text.
fn map_offset(edits: &[Edit], offset: u32) -> u32 {
    let mut shift: i64 = 0;
    for (s, e, text) in edits {
        if offset < *s {
            break;
        }
        if offset < *e {
            return (*s as i64 + shift) as u32;
        }
        shift += text.len() as i64 - (*e as i64 - *s as i64);
    }
    (offset as i64 + shift).max(0) as u32
}

/// The first error the check `after` reports that the tree did not have
/// before the edits, or `None`.
fn first_new_error(
    source: &str,
    file_before: &[Diagnostic],
    elsewhere_before: &[Diagnostic],
    edits: &[Edit],
    path: &Path,
    files: &[PathBuf],
    after: &crate::check::CheckReport,
) -> Option<NewError> {
    let mut here = Vec::new();
    let mut elsewhere = Vec::new();
    for d in &after.structured {
        match place_diagnostic(&after.project_srcs, files, d) {
            Some(p) if same_file(&p, path) => here.push(d.clone()),
            _ => elsewhere.push(d.clone()),
        }
    }
    let spans: Vec<(usize, usize, &str)> =
        edits.iter().map(|(s, e, t)| (*s as usize, *e as usize, t.as_str())).collect();
    let introduced = glyph_lsp::introduced_by_edits(source, file_before, &spans, here);
    let key = |d: &Diagnostic| {
        (d.file.clone(), d.code.clone(), d.severity.clone(), d.subject().to_string(), d.range.start.offset, d.range.end.offset)
    };
    let mut unmatched: Vec<_> = elsewhere_before.iter().map(key).collect();
    let mut new_elsewhere = Vec::new();
    for d in elsewhere {
        match unmatched.iter().position(|k| *k == key(&d)) {
            Some(i) => {
                unmatched.swap_remove(i);
            }
            None => new_elsewhere.push(d),
        }
    }
    if let Some(d) = introduced.into_iter().find(|d| d.severity == "error") {
        return Some(NewError { what: format!("[{}] {}", d.code, d.message), offset: Some(d.range.start.offset), place: String::new() });
    }
    new_elsewhere.into_iter().find(|d| d.severity == "error").map(|d| NewError {
        what: format!("[{}] {}", d.code, d.message),
        offset: None,
        place: format!(" ({}:{})", d.file, d.range.start.line),
    })
}

/// Each edit's newlines continued at the indentation of the line the edit
/// starts on, the way the checker's multi-line rewrites expect.
fn indent_edits(source: &str, edits: &[Edit]) -> Vec<Edit> {
    edits
        .iter()
        .map(|(s, e, text)| {
            let line_start = source[..*s as usize].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let indent: String =
                source[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            (*s, *e, text.replace('\n', &format!("\n{indent}")))
        })
        .collect()
}

/// The absolute path a diagnostic is about.
///
/// `Diagnostic::file` is a path under the project root that produced it, and a
/// tree may hold several projects (D41), so the pairing is (root, root/file).
/// The answer has to be one of the files this run collected: `glyph fix` writes
/// only inside the tree it was pointed at.
fn place_diagnostic(
    roots: &[PathBuf],
    files: &[PathBuf],
    d: &Diagnostic,
) -> Option<PathBuf> {
    let mut found: Option<PathBuf> = None;
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
        found = Some(candidate);
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
    diags: &[Diagnostic],
    coverage: Option<&glyph_db::MatchCoverage>,
    report: &mut FixReport,
) -> Result<(), String> {
    let source =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Ok(module) = glyph_parser::parse(&source) else {
        // A file that does not parse has parse diagnostics of its own, and
        // every rule here reads offsets into a parsed file.
        return Ok(());
    };

    let mut edits: Vec<(u32, u32, String)> = Vec::new();
    let mut applied: Vec<Applied> = Vec::new();
    // Imports a rule added, one per module path however many rewrites need it.
    let mut imports: Vec<(u32, u32, String)> = Vec::new();

    // Every rule below edits at the range its diagnostic carries, so a
    // repair a rule did plan is held to the node that range names before it
    // is written; a rule's own reason to decline comes first.
    let index = SpanIndex::new(&module);
    for d in diags {
        let guard = index.holds(&source, d);
        match d.code.as_str() {
            "E0220" => match plan_e0220(&source, &module, d, diags, coverage).and_then(|v| guard.clone().map(|()| v)) {
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
            "E0210" => match plan_e0210(&source, d).and_then(|v| guard.clone().map(|()| v)) {
                Ok((start, end, name, site)) => {
                    applied.push(Applied {
                        code: d.code.clone(),
                        file: path.to_path_buf(),
                        what: format!("renamed the {site} to `{name}`"),
                    });
                    edits.push((start, end, name));
                }
                Err(why) => report.declined.push(Declined {
                    code: d.code.clone(),
                    file: path.to_path_buf(),
                    why,
                }),
            },
            "E0240" => match plan_e0240(&source, d).and_then(|v| guard.clone().map(|()| v)) {
                Ok(edit) => {
                    applied.push(Applied {
                        code: d.code.clone(),
                        file: path.to_path_buf(),
                        what: "wrote `mut` before a call that changes its receiver".to_string(),
                    });
                    edits.push(edit);
                }
                Err(why) => report.declined.push(Declined {
                    code: d.code.clone(),
                    file: path.to_path_buf(),
                    why,
                }),
            },
            "E0242" => match plan_e0242(&source, d, &edits, &imports).and_then(|v| guard.clone().map(|()| v)) {
                Ok((rewrite, import)) => {
                    let what = match &import {
                        Some(i) => format!("wrote the `Option` form of the call, and `{}`", i.2.trim()),
                        None => "wrote the `Option` form of the call".to_string(),
                    };
                    applied.push(Applied { code: d.code.clone(), file: path.to_path_buf(), what });
                    edits.extend(rewrite);
                    if let Some(i) = import {
                        imports.push(i);
                    }
                }
                Err(why) => report.declined.push(Declined {
                    code: d.code.clone(),
                    file: path.to_path_buf(),
                    why,
                }),
            },
            // A `??` on an `Option` or a `Result`: its rewrite is applied
            // by `apply_checked_rewrites`, after a check of its own, and a
            // `?.` on one carries no repair and stays the help's.
            "E0236" => {
                if let Some(glyph_typechecker::AbsentRepair::Decline(why)) = &d.repair {
                    report.declined.push(Declined { code: d.code.clone(), file: path.to_path_buf(), why: why.clone() });
                }
            }
            "E0200" => report.declined.push(Declined {
                code: d.code.clone(),
                file: path.to_path_buf(),
                why: e0200_decline(d),
            }),
            _ => {}
        }
    }

    if edits.is_empty() {
        return Ok(());
    }
    edits.extend(imports);

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
    // The import rule records the path it collected; the diagnostic rules
    // record the one `place_diagnostic` built from the project root, which
    // can spell the same file differently (relative and absolute). One file
    // is counted once (record 151, F6).
    if !report.changed.iter().any(|c| same_file(c, path)) {
        report.changed.push(path.to_path_buf());
    }
    report.applied.extend(applied);
    Ok(())
}

// ---------------------------------------------------------------------------
// E0200: reported, never repaired
// ---------------------------------------------------------------------------

/// Why `glyph fix` writes nothing for a non-exhaustive match.
///
/// The compiler knows which cases are missing and the pattern each one takes,
/// and it does not know what the program should do in any of them. An arm whose
/// body the compiler made up passes `glyph check` and turns the missing case
/// into a runtime failure, which is the thing the exhaustiveness check exists
/// to prevent. So the rule declines every one and names the cases, and
/// `glyph check --agent` carries the variant shapes for whoever writes the arms.
fn e0200_decline(d: &Diagnostic) -> String {
    let cases = match d.missing_variants.as_deref() {
        Some(missing) if !missing.is_empty() => {
            format!("add an arm for each of {}", quoted_list(missing))
        }
        _ => "add an arm for each missing case".to_string(),
    };
    format!(
        "the compiler does not determine what a missing arm should do, so `glyph fix` \
         writes none: {cases}. `glyph check --agent` gives the pattern each case takes."
    )
}

// ---------------------------------------------------------------------------
// E0240: `mut ` before a bare statement that changes its receiver
// ---------------------------------------------------------------------------

/// Insert `mut ` before a call that changes its receiver, when the checker
/// decided that is the whole repair (G380, D5).
///
/// The checker places every E0240 by the position it stands in, and marks
/// the one shape `mut ` repairs: a statement whose value nothing reads, on a
/// place. The rule applies that decision (`Diagnostic::insert_before`)
/// rather than reading the shape off the AST a second time, which is how an
/// earlier version wrote `mut` before a block's returned tail and turned a
/// working function into one `tsc` refuses (record 151, F1). Every other
/// E0240 is declined with the reason.
fn plan_e0240(source: &str, d: &Diagnostic) -> Result<(u32, u32, String), String> {
    let (s, _) = range_bytes(source, d).ok_or("the diagnostic's range is not a byte range of this file")?;
    match &d.insert_before {
        Some(text) => Ok((s as u32, s as u32, text.clone())),
        None => Err(
            "the call's value is used, or it is an arm of a `match`, a deferred call or a call on a value no name \
             holds, so the repair depends on what it was for; the help names one"
                .to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// E0242: the `Option` form of a call that answers absence with `undefined`
// ---------------------------------------------------------------------------

/// A byte-range replacement in a file: `(start, end, text)`.
type Edit = (u32, u32, String);

/// Why a rewrite declines when the source at its node's range no longer
/// parses to that node (record 159, B1).
const NODE_DRIFT: &str = "the diagnostic's range does not hold the expression the rewrite was planned \
     against, so writing it would splice into the wrong text; the help names the change";

// ---------------------------------------------------------------------------
// The range guard
// ---------------------------------------------------------------------------

/// Every node of a parsed module that carries a span, read off the module's
/// `Debug` form: each node's span and its shape (its `Debug` text with the
/// spans removed), for the guard every diagnostic-driven rule passes before
/// it edits (record 159, B1; record 160).
///
/// A rule edits at the byte range its diagnostic carries. If that range
/// has drifted from the node it names (spans inside a `${}` start one column
/// early, G435), the rule writes into the wrong text and the result can
/// pass every check. So before a rule reads a range, the range must be the
/// span of a node in this parse; when the text there parses as an
/// expression, it must parse to that node's shape. A node whose text does
/// not parse as an expression (a pattern, a JSX attribute) is accepted
/// outside a template string and declined inside one, since that is where
/// spans are known to drift.
struct SpanIndex {
    /// (start, end, variant name, shape) per node.
    nodes: Vec<(u32, u32, String, String)>,
}

impl SpanIndex {
    fn new(module: &glyph_ast::Module) -> SpanIndex {
        let debug = format!("{module:?}");
        let bytes = debug.as_bytes();
        let mut close_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut hits: Vec<(u32, u32, usize)> = Vec::new();
        let mut in_str = false;
        let mut i = 0;
        const KEY: &str = "span: Span { start: ";
        while i < bytes.len() {
            let c = bytes[i];
            if in_str {
                match c {
                    b'\\' => i += 1,
                    b'"' => in_str = false,
                    _ => {}
                }
                i += 1;
                continue;
            }
            match c {
                b'"' => in_str = true,
                b'{' | b'(' | b'[' => stack.push(i),
                b'}' | b')' | b']' => {
                    if let Some(open) = stack.pop() {
                        close_of.insert(open, i);
                    }
                }
                b's' if debug[i..].starts_with(KEY) && i >= 2 && (bytes[i - 1] == b' ') && matches!(bytes[i - 2], b',' | b'{') => {
                    let rest = &debug[i + KEY.len()..];
                    let start: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    let after = &rest[start.len()..];
                    if let Some(end_text) = after.strip_prefix(", end: ") {
                        let end: String = end_text.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if let (Ok(s), Ok(e), Some(&open)) = (start.parse(), end.parse(), stack.last()) {
                            if bytes[open] == b'{' {
                                hits.push((s, e, open));
                            }
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let nodes = hits
            .into_iter()
            .filter_map(|(s, e, open)| {
                let close = *close_of.get(&open)?;
                let head = debug[..open].trim_end();
                let name_start = head
                    .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .map(|j| j + 1)
                    .unwrap_or(0);
                let name = head[name_start..].to_string();
                Some((s, e, name, strip_spans(&debug[name_start..=close])))
            })
            .collect();
        SpanIndex { nodes }
    }

    /// `Ok` when the diagnostic's range is a node's span and the text there
    /// is that node; otherwise why the rule declines.
    fn holds(&self, source: &str, d: &Diagnostic) -> Result<(), String> {
        let (s, e) = (d.range.start.offset, d.range.end.offset);
        let at: Vec<&(u32, u32, String, String)> = self.nodes.iter().filter(|n| n.0 == s && n.1 == e).collect();
        if at.is_empty() {
            return Err(NODE_DRIFT.to_string());
        }
        let parsed = source
            .get(s as usize..e as usize)
            .and_then(|slice| glyph_parser::parse_expression(slice).ok())
            .map(|x| glyph_typechecker::absent::expr_shape(&x));
        if let Some(shape) = &parsed {
            if at.iter().any(|n| &n.3 == shape) {
                return Ok(());
            }
        }
        let in_template = self.nodes.iter().any(|n| n.2 == "TemplateString" && n.0 <= s && e <= n.1);
        // A name an expression shares with a pattern (`Ident`, `Object`,
        // `Array`) or a JSX attribute (`String`) does not say which the node
        // is.
        let surely_expr = at.iter().any(|n| {
            matches!(
                n.2.as_str(),
                "Number" | "TemplateString" | "Bool" | "Void" | "Binary" | "Unary" | "Postfix" | "Call"
                    | "Member" | "Index" | "Await" | "New" | "Match" | "Lambda" | "Extern"
            )
        });
        if in_template || surely_expr {
            return Err(NODE_DRIFT.to_string());
        }
        Ok(())
    }
}

/// `Debug` text with every `Span { .. }` removed.
fn strip_spans(debug: &str) -> String {
    let mut out = String::with_capacity(debug.len());
    let mut rest = debug;
    while let Some(i) = rest.find("Span { start: ") {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => rest = &rest[i + j + 1..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
}

/// Whether `source` at the node's range parses to an expression of the
/// node's shape, the guard every planned rewrite passes before it is
/// written.
fn node_holds(source: &str, node: &glyph_typechecker::absent::NodeShape) -> bool {
    let Some(slice) = source.get(node.start as usize..node.end as usize) else {
        return false;
    };
    glyph_parser::parse_expression(slice).is_ok_and(|e| glyph_typechecker::absent::expr_shape(&e) == node.shape)
}

/// The rewrite the checker attached to an E0242, plus the import it needs
/// (G392, D56).
///
/// The checker decided the edits from the call as written, and decided where
/// none is determined (a string accessor, a computed `at` index, a `?.` call),
/// with the reason; this applies that decision. What it adds is the import:
/// `ensure_import` writes `import std/array` when the module needs it, and
/// declines when a binding of that name is already in the file. An edit that
/// would overlap one already planned in this file is declined too, so two
/// rewrites never splice into each other.
fn plan_e0242(
    source: &str,
    d: &Diagnostic,
    planned: &[(u32, u32, String)],
    imports: &[(u32, u32, String)],
) -> Result<(Vec<Edit>, Option<Edit>), String> {
    let (edits, binding, import) = match &d.repair {
        Some(glyph_typechecker::AbsentRepair::Rewrite { edits, binding, import, node }) => {
            if !node_holds(source, node) {
                return Err(NODE_DRIFT.to_string());
            }
            (edits, binding, *import)
        }
        Some(glyph_typechecker::AbsentRepair::Decline(why)) => return Err(why.clone()),
        Some(glyph_typechecker::AbsentRepair::Checked { .. }) | None => return Err("the checker attached no rewrite to this diagnostic".to_string()),
    };
    for (s, e, _) in edits {
        if *e as usize > source.len() || s > e {
            return Err("the rewrite's range is not a byte range of this file".to_string());
        }
        let overlaps = planned.iter().any(|(ps, pe, _)| s < pe && ps < e && !(s == e && ps == pe));
        if overlaps {
            return Err("another repair in this file rewrites the same text; run `glyph fix` again".to_string());
        }
    }
    let import = match import {
        None => None,
        Some(path) => {
            let line = format!("import {path}");
            match imports.iter().find(|i| i.2.trim() == line) {
                Some(_) => None,
                None => Some(ensure_import(source, path, binding)?),
            }
        }
    };
    Ok((indent_edits(source, edits), import))
}

/// The edit that adds `import <path>` to a module, binding `binding`, or why
/// it cannot be added.
///
/// A general helper: a rule whose rewrite calls through a `std/*` namespace
/// the module does not import asks for it here. The line goes after the last
/// `import`, or after the `module` line when there is none. It declines when
/// the name is already bound in the file: the import would collide with a
/// declaration, or a local of that name would shadow it at the call. Bindings
/// are found from the tokens, conservatively: any `binding` that is not
/// followed by `.` and does not end an import path counts, so a record field
/// of that name declines too.
fn ensure_import(source: &str, path: &str, binding: &str) -> Result<(u32, u32, String), String> {
    let Ok(tokens) = glyph_lexer::tokenize(source) else {
        return Err(format!("the file does not lex, so where `import {path}` goes is not determined"));
    };
    use glyph_lexer::Token;
    for (i, t) in tokens.iter().enumerate() {
        let Token::Identifier(name) = &t.token else { continue };
        if name.as_ref() != binding {
            continue;
        }
        let next_is_dot = matches!(tokens.get(i + 1).map(|n| &n.token), Some(Token::Dot | Token::QDot));
        let after_slash = i > 0 && matches!(tokens[i - 1].token, Token::Slash);
        if !next_is_dot && !after_slash {
            return Err(format!(
                "the rewrite calls `{binding}.…` from `{path}`, which this file does not import, and the name \
                 `{binding}` is already bound here; import `{path}` under another name and write the help's call"
            ));
        }
    }
    let Ok(module) = glyph_parser::parse(source) else {
        return Err(format!("the file does not parse, so where `import {path}` goes is not determined"));
    };
    let last_import = module.items.iter().filter_map(|d| match d {
        Decl::Import(i) => Some(full_line_span(source, i.span.start, i.span.end).1),
        _ => None,
    }).max();
    let at = match last_import {
        Some(end) => end,
        None => {
            let mut offset = 0usize;
            let mut found = None;
            for line in source.split_inclusive('\n') {
                if line.trim_start().starts_with("module ") {
                    found = Some(offset + line.len());
                    break;
                }
                offset += line.len();
            }
            found.unwrap_or(0) as u32
        }
    };
    // A module line with no import under it gets a blank line before the
    // first import, the layout `glyph fmt` keeps.
    let text = match last_import {
        Some(_) => format!("import {path}\n"),
        None => format!("\nimport {path}\n"),
    };
    Ok((at, at, text))
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
/// The checker suggests only a variant no other arm covers (G305), but a
/// variant reached through a nested sub-pattern (`A(X)`) is not covered and is
/// not missing either, and writing a variant the match does not need is not a
/// repair. So whether the suggested variant is free is still settled by the
/// match's own `E0200`: a variant the match is missing is free, and a match
/// with no `E0200` at all has every variant spoken for.
///
/// When it has every variant spoken for, the decline says by which arm, and
/// that is read from the checker's coverage relation for the match
/// (`coverage`), not from the arms' spelling: a lowercase arm may be a
/// binding or a lowercase variant (G224), and only the checker knows which
/// (G327, record 89).
///
/// A string-literal union (D30) takes the same repair with a different head:
/// the arm is a literal (`"of"`), the suggestion is the value unquoted (`off`),
/// and what is written is that value as a string literal again. Which of the
/// two the head is comes off `alternatives_kind`, which the checker set when
/// it built the suggestion; reading it off the source text instead would treat
/// any quoted head as a literal without the checker having said so.
fn plan_e0220(
    source: &str,
    module: &glyph_ast::Module,
    d: &Diagnostic,
    siblings: &[Diagnostic],
    coverage: Option<&glyph_db::MatchCoverage>,
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
    let pattern = (d.range.start.offset, d.range.end.offset);
    let owner = match_owning_arm(module, pattern)
        .ok_or("the arm this diagnostic points at is not an arm of a `match` in this file")?;

    // Two arms whose one suggestion is the same value cannot both be renamed
    // to it: the second would be unreachable (E0305). Which of them was meant
    // is not something the checker decided, so neither is renamed. A literal
    // head and a variant head make the same claim, so both are held to it.
    let mut rivals: Vec<(u32, &str)> = siblings
        .iter()
        .filter(|o| {
            o.code == "E0220"
                && owner.owns(o)
                && o.alternatives.as_deref().is_some_and(|a| a.contains(suggestion))
        })
        .filter_map(|o| {
            let (s, e) = range_bytes(source, o)?;
            Some((o.range.start.offset, &source[s..e]))
        })
        .collect();
    rivals.sort_by_key(|(at, _)| *at);
    rivals.dedup_by_key(|(at, _)| *at);
    if rivals.len() > 1 {
        let arms: Vec<String> = rivals.iter().map(|(_, text)| text.to_string()).collect();
        return Err(format!(
            "the rename is not determined: {} arms of this match ({}) would each be renamed to the same value, `{suggestion}`, and only one of them can take it",
            arms.len(),
            quoted_list(&arms)
        ));
    }

    // The E0200 that says whether the suggestion is free has to be the one
    // about this arm's own match, which is where the checker points it. One
    // whose range merely holds the arm can be an enclosing match's, missing a
    // case this match already handles.
    let enclosing = siblings.iter().find(|o| {
        o.code == "E0200" && (o.range.start.offset, o.range.end.offset) == owner.span
    });
    match enclosing {
        // No E0200 means no case is missing, and there are two reasons that
        // can be true, which need different sentences. Without a catch-all the
        // other arms name every case and the rename duplicates one (E0305).
        // With one, the rename would compile: it moves the suggested case out
        // of the catch-all into an arm written for some other value, which is
        // a change of behaviour the compiler did not decide. Telling that
        // reader about an unreachable arm sends them looking for a duplicate
        // that is not there.
        None => {
            return Err(match catch_all_arm(source, &owner, coverage) {
                CatchAll::Arm(catch_all) => format!(
                    "this match is missing no case because its {catch_all} handles `{suggestion}` now, so renaming this head to `{suggestion}` would move that case out of it, and which arm should handle it is not something the compiler determines"
                ),
                CatchAll::Unknown => format!(
                    "this match is missing no case, so some arm handles `{suggestion}` now; renaming this head to `{suggestion}` would move that case out of it, and the checker did not conclude this match's coverage, so `glyph fix` cannot say which arm that is"
                ),
                CatchAll::None => format!(
                    "the other arms of this match already cover every case, so renaming this head to `{suggestion}` would write an arm that can never run (E0305)"
                ),
            });
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
    if d.alternatives_kind.as_deref() == Some("literals") {
        // The whole literal is replaced, quotes included, and the value is
        // escaped the way the formatter writes one: a suggestion holding `"`,
        // `\` or `${` written between bare quotes would be a different string
        // or not a string at all.
        let (start, end) = literal_head_span(&owner.arms, pattern)
            .ok_or("the arm head is not a string literal in the source this diagnostic points at")?;
        return Ok((
            start,
            end,
            format!("\"{}\"", glyph_formatter::escape_string(suggestion)),
        ));
    }
    let (start, end) = head_ident_span(source, d)
        .ok_or("the arm head is not a plain name in the source this diagnostic points at")?;
    Ok((start, end, alts[0].clone()))
}

/// The `match` one arm belongs to: the whole expression's span, which is
/// where the checker points that match's `E0200`, and its arms.
struct OwningMatch {
    span: (u32, u32),
    arms: Vec<glyph_ast::MatchArm>,
}

impl OwningMatch {
    /// Whether the diagnostic points at exactly one of this match's arm
    /// patterns.
    fn owns(&self, d: &Diagnostic) -> bool {
        has_arm_at(&self.arms, (d.range.start.offset, d.range.end.offset))
    }
}

/// Whether one of `arms` has a pattern spanning exactly `at`.
fn has_arm_at(arms: &[glyph_ast::MatchArm], at: (u32, u32)) -> bool {
    arms.iter().any(|arm| {
        let span = arm.pattern.span();
        (span.start, span.end) == at
    })
}

/// The `match` in `module` that has an arm whose pattern spans exactly
/// `pattern`.
///
/// Ownership is decided by the arm, not by which match's span holds the
/// range: a match nested in another arm's body is held by both spans, and
/// only one of them has this pattern as an arm. A JSX `<match>` is read
/// through `as_value_match`, the one view the checker and the emitter use,
/// whose arms keep the `<case>` patterns' spans and whose span is the
/// element's.
fn match_owning_arm(module: &glyph_ast::Module, pattern: (u32, u32)) -> Option<OwningMatch> {
    use glyph_ast::{visit, Block, Expr, JsxAttr, JsxChild, JsxElement, JsxKind};

    fn block(b: &Block, at: (u32, u32), found: &mut Option<OwningMatch>) {
        for s in &b.stmts {
            visit::stmt_exprs(s, &mut |e| expr(e, at, found));
            visit::stmt_blocks(s, &mut |b| block(b, at, found));
        }
    }
    fn expr(e: &Expr, at: (u32, u32), found: &mut Option<OwningMatch>) {
        let span = e.span();
        if found.is_some() || span.start > at.0 || span.end < at.1 {
            return;
        }
        if let Expr::Match { arms, span, .. } = e {
            if has_arm_at(arms, at) {
                *found = Some(OwningMatch {
                    span: (span.start, span.end),
                    arms: arms.clone(),
                });
                return;
            }
        }
        visit::child_exprs(e, &mut |c| expr(c, at, found));
        visit::child_blocks(e, &mut |b| block(b, at, found));
        // `child_exprs` does not descend into a JSX element, so a `match`
        // inside one is reached here or not at all.
        if let Expr::Jsx(el) = e {
            jsx(el, at, found);
        }
    }
    fn jsx(el: &JsxElement, at: (u32, u32), found: &mut Option<OwningMatch>) {
        if found.is_some() || el.span.start > at.0 || el.span.end < at.1 {
            return;
        }
        if el.kind() == JsxKind::Match {
            if let Some(Expr::Match { arms, span, .. }) = el.as_value_match() {
                if has_arm_at(&arms, at) {
                    *found = Some(OwningMatch {
                        span: (span.start, span.end),
                        arms,
                    });
                    return;
                }
            }
        }
        for a in &el.attrs {
            if let JsxAttr::Expr { value, .. } | JsxAttr::Spread { value, .. } = a {
                expr(value, at, found);
            }
        }
        for c in &el.children {
            match c {
                JsxChild::Element(child) => jsx(child, at, found),
                JsxChild::Expr(e) => expr(e, at, found),
                JsxChild::Text { .. } => {}
            }
        }
    }

    let mut found = None;
    for item in &module.items {
        match item {
            Decl::Fn(f) => block(&f.body, pattern, &mut found),
            Decl::Component(c) => block(&c.body, pattern, &mut found),
            Decl::Const(c) => expr(&c.value, pattern, &mut found),
            Decl::Type(t) => {
                if let Some(pred) = &t.refinement {
                    expr(pred, pattern, &mut found);
                }
            }
            Decl::Interface(_) | Decl::Import(_) => {}
        }
    }
    found
}

/// What the checker recorded about the arm of a match that takes every value
/// the others leave.
enum CatchAll {
    /// The first arm the checker recorded as absorbing the rest at the
    /// match's own level, described for the decline (`` `_` arm ``,
    /// ``arm `other` ``).
    Arm(String),
    /// The checker counted every case against the full variant set and no
    /// arm absorbs the rest, so the other arms name every case.
    None,
    /// The checker did not conclude this match's coverage (an arm it
    /// declined to read, a scrutinee it did not resolve, or no site at all),
    /// so which arm takes the rest is not known.
    Unknown,
}

/// The arm that takes every value the others leave in `owner`, read from the
/// checker's coverage relation: the depth-0 catch-all with the lowest arm
/// number at the site whose `match` span is `owner`'s (record 89).
///
/// A catch-all at greater depth is inside a payload (`Ok(x)`) and absorbs
/// only that payload's union, so it is not the match's own. The arm's
/// spelling is never consulted: a lowercase name is a catch-all when it binds
/// and a single case when it names a lowercase variant (G224), and only the
/// checker has decided which.
fn catch_all_arm(
    source: &str,
    owner: &OwningMatch,
    coverage: Option<&glyph_db::MatchCoverage>,
) -> CatchAll {
    use glyph_typechecker::CoverageState;
    let Some(site) = coverage.and_then(|c| {
        c.sites().iter().find(|s| {
            let span = s.match_span();
            (span.start, span.end) == owner.span
        })
    }) else {
        return CatchAll::Unknown;
    };
    let first = site
        .catch_alls()
        .iter()
        .filter(|c| c.depth == 0)
        .map(|c| c.arm)
        .min();
    match first {
        Some(arm) => match owner.arms.get(usize::from(arm)) {
            Some(arm) => CatchAll::Arm(describe_arm(source, &arm.pattern)),
            None => CatchAll::Unknown,
        },
        None if site.state() == CoverageState::Exhaustive => CatchAll::None,
        None => CatchAll::Unknown,
    }
}

/// How a decline names a catch-all arm: by its keyword for `else` and `_`,
/// by its pattern as written otherwise.
fn describe_arm(source: &str, pattern: &glyph_ast::Pattern) -> String {
    use glyph_ast::Pattern;
    match pattern {
        Pattern::Else { .. } => "`else` arm".to_string(),
        Pattern::Wildcard { .. } => "`_` arm".to_string(),
        other => {
            let span = other.span();
            let text = source
                .get(span.start as usize..span.end as usize)
                .unwrap_or_default();
            format!("arm `{text}`")
        }
    }
}

/// The byte span of the string-literal arm pattern at exactly `pattern`, when
/// the arm there is one.
fn literal_head_span(arms: &[glyph_ast::MatchArm], pattern: (u32, u32)) -> Option<(u32, u32)> {
    use glyph_ast::{LiteralPattern, Pattern};
    arms.iter().find_map(|arm| match &arm.pattern {
        Pattern::Literal {
            value: LiteralPattern::String(_),
            span,
        } if (span.start, span.end) == pattern => Some((span.start, span.end)),
        _ => None,
    })
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

/// Replace a field the record does not declare with the one declared field a
/// single character away from it, and name the site the repair landed on so
/// the report says what it edited.
///
/// `alternatives` carries the record's own field list. Zero candidates within
/// distance one is a name that was not a typo; several is a choice the compiler
/// does not make. Both decline.
fn plan_e0210(source: &str, d: &Diagnostic) -> Result<(u32, u32, String, &'static str), String> {
    let fields = d
        .alternatives
        .as_ref()
        .ok_or("the diagnostic carries no field list for the record")?;
    let (s, e) = range_bytes(source, d)
        .ok_or("the diagnostic's range is not a byte range of this file")?;
    let text = &source[s..e];
    // Two shapes carry an E0210. A field read (`sheet.rowz`) names the field
    // after the last dot. A JSX attribute (`<Button varient="x" />`, G240)
    // names it before the `=`, and its own value may hold dots of its own
    // (`varient={cfg.mode}`), so the attribute form is recognized first and
    // the dot rule never sees it.
    let (name_at, raw, site) = match jsx_attr_name(text) {
        Some(name) => (0, name, "attribute"),
        None => {
            let after_dot = text
                .rfind('.')
                .map(|i| i + 1)
                .ok_or("the access this diagnostic points at is not a `.field` read")?;
            (after_dot, &text[after_dot..], "field read")
        }
    };
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
            let start = s + name_at + lead;
            Ok((
                start as u32,
                (start + wrong.len()) as u32,
                near[0].clone(),
                site,
            ))
        }
        // The receiver is a record or, since G288, an array, a string or a number, whose
        // members TypeScript's library declares; the wording names neither.
        0 => Err(format!(
            "nothing the type declares is one character away from `{wrong}`; it declares {}",
            quoted_list(fields)
        )),
        n => Err(format!(
            "{n} names the type declares are one character away from `{wrong}` ({}), and only \
             one determines the repair",
            quoted_list(&near.iter().map(|s| (*s).clone()).collect::<Vec<_>>())
        )),
    }
}

/// The attribute name of a JSX attribute an E0210 points at, or `None` when
/// the diagnostic's text is not one.
///
/// The span a JSX attribute diagnostic carries runs from the name to the end
/// of the value, so `varient="danger"` and `varient={cfg.mode}` both start
/// with the name and then an `=`. Requiring the name to be a plain identifier
/// and the very next character to be `=` is what keeps a field read out: no
/// `.field` access has an `=` in its span at all.
fn jsx_attr_name(text: &str) -> Option<&str> {
    let (name, _) = text.split_once('=')?;
    is_plain_ident(name).then_some(name)
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

/// `"a"`, `"b"` for a list of names, for a sentence that enumerates them.
fn quoted_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
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
