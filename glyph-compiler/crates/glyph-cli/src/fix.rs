//! `glyph fix` — apply the safe, mechanical autofixes.
//!
//! Today that is one rule: drop the dead names out of an `import` (the E0106
//! lint). An import whose every bound name is unused loses the whole
//! declaration; a named import (`import M { a, b, c }`) with only some names
//! dead (say `b`) keeps the declaration and drops just `b`. Only `Named` can
//! be partially dead — `Namespace`/`Aliased`/`Default` each bind a single
//! name, so for those "some dead" and "all dead" are the same case.

use std::path::{Path, PathBuf};

use glyph_ast::{Decl, ImportKind};
use glyph_resolver::{
    build_prelude, collect_module_symbols, module_lints, resolve_module, ResolveError,
};

pub struct FixReport {
    pub changed: Vec<PathBuf>,
    pub removed_imports: usize,
}

/// Apply the safe autofixes across every `.glyph` file under `src` (a directory,
/// or a single file). Rewrites files in place and returns what changed.
pub fn fix_project(src: &Path) -> Result<FixReport, String> {
    let mut files = Vec::new();
    collect_glyph_files(src, &mut files);
    let prelude = build_prelude();
    let mut report = FixReport { changed: Vec::new(), removed_imports: 0 };

    for f in files {
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
                edits.push((imp.span.start, imp.span.end, new_text));
                report.removed_imports += dead_count;
            }
        }
        if edits.is_empty() {
            continue;
        }

        let new_source = apply_edits(&source, edits);
        std::fs::write(&f, &new_source).map_err(|e| format!("{}: {e}", f.display()))?;
        report.changed.push(f);
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
    let mut scan = end as usize;
    while scan > start as usize && source.as_bytes().get(scan - 1) == Some(&b'\n') {
        scan -= 1;
    }
    let line_end = match source[scan..].find('\n') {
        Some(i) => (scan + i) as u32 + 1,
        None => source.len() as u32,
    };
    (line_start, line_end)
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
