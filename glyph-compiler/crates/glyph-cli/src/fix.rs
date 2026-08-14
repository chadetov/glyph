//! `glyph fix` — apply the safe, mechanical autofixes.
//!
//! Today that is one rule: remove an `import` whose every bound name is unused
//! (the E0106 lint). This is the unambiguously safe case; a partially-used
//! named import (`import M { a, b }` with only `b` unused) is left alone rather
//! than risk trimming the wrong name, so `glyph fix` never changes behavior.

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

        // Mark an import decl for removal only when *all* its bound names are unused.
        let mut remove_spans: Vec<(u32, u32)> = Vec::new();
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
            if !names.is_empty() && names.iter().all(|n| unused.contains(n)) {
                remove_spans.push((imp.span.start, imp.span.end));
            }
        }
        if remove_spans.is_empty() {
            continue;
        }

        let new_source = remove_lines_intersecting(&source, &remove_spans);
        std::fs::write(&f, &new_source).map_err(|e| format!("{}: {e}", f.display()))?;
        report.changed.push(f);
        report.removed_imports += remove_spans.len();
    }

    Ok(report)
}

/// Rebuild `source` dropping any line whose byte range intersects a removal
/// span. Import declarations are single-line, so each span drops exactly its
/// import line.
fn remove_lines_intersecting(source: &str, spans: &[(u32, u32)]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut pos = 0usize;
    for line in source.split_inclusive('\n') {
        let start = pos as u32;
        let end = (pos + line.len()) as u32;
        let intersects = spans.iter().any(|(s, e)| *s < end && *e > start);
        if !intersects {
            out.push_str(line);
        }
        pos += line.len();
    }
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
        // `Ok` is used, `Err` is not: the whole line stays (safe, no trimming).
        let src = "module m\n\
            import std/result { Result, Ok, Err }\n\
            fn f() -> Result<number, string> {\n  return Ok(1)\n}\n";
        let dir = std::env::temp_dir().join(format!("glyph_fix2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.glyph");
        std::fs::write(&file, src).unwrap();

        let report = fix_project(&dir).unwrap();
        assert_eq!(report.removed_imports, 0, "partial import left alone");
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(after.contains("import std/result { Result, Ok, Err }"), "{after}");
    }
}
