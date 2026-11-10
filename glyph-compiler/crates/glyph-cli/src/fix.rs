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

/// The codes this pass reads. A diagnostic with any other code is left alone.
///
/// `E0210` and `E0220` are repaired when the compiler determines the repair.
/// `E0200` is read and never repaired: each one is reported as declined, and
/// its `missing_variants` is what the `E0220` rule asks to learn whether the
/// variant it would rename a head to is still free.
const RULE_CODES: [&str; 3] = ["E0200", "E0210", "E0220"];

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
        fix_one_file(&path, &diags, report)?;
    }
    Ok(())
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

    for d in diags {
        match d.code.as_str() {
            "E0220" => match plan_e0220(&source, &module, d, diags) {
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
            if let Some(catch_all) = catch_all_arm(&owner.arms) {
                return Err(format!(
                    "this match is missing no case because its {catch_all} handles `{suggestion}` now, so renaming this head to `{suggestion}` would move that case out of it, and which arm should handle it is not something the compiler determines"
                ));
            }
            return Err(format!(
                "the other arms of this match already cover every case, so renaming this head to `{suggestion}` would write an arm that can never run (E0305)"
            ));
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

/// The arm of a match that takes every value the others leave, described the
/// way the decline names it, when the match has one.
///
/// The same three shapes the checker counts as a catch-all: `else`, `_`, and a
/// lowercase name, which binds rather than naming a variant.
fn catch_all_arm(arms: &[glyph_ast::MatchArm]) -> Option<String> {
    use glyph_ast::Pattern;
    arms.iter().find_map(|arm| match &arm.pattern {
        Pattern::Else { .. } => Some("`else` arm".to_string()),
        Pattern::Wildcard { .. } => Some("`_` arm".to_string()),
        Pattern::Ident { name, .. }
            if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) =>
        {
            Some(format!("catch-all arm `{name}`"))
        }
        _ => None,
    })
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
