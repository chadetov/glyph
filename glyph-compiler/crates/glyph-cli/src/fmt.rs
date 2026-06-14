//! `glyph fmt` — format Glyph source in place.
//!
//! A path may be a single `.glyph` file or a directory (walked recursively,
//! skipping dot-directories and `target/`, mirroring `glyph build`). Each file
//! is parsed and reprinted in the one canonical layout; a file whose contents
//! already match is left untouched. A file that fails to parse is reported and
//! skipped — formatting never writes output derived from unparseable source.

use std::path::{Path, PathBuf};

use glyph_formatter::format_module;

#[derive(Debug, thiserror::Error)]
pub enum FmtError {
    #[error("path does not exist: {0}")]
    Missing(PathBuf),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Summary of a formatting run.
#[derive(Debug, Default)]
pub struct FmtReport {
    /// Files rewritten because their layout changed.
    pub formatted: Vec<PathBuf>,
    /// Files already in canonical form.
    pub unchanged: Vec<PathBuf>,
    /// Files skipped because they did not parse, with the rendered reason.
    pub failed: Vec<(PathBuf, String)>,
}

/// Format `path` (a file or directory tree) in place.
pub fn format_path(path: &Path) -> Result<FmtReport, FmtError> {
    if !path.exists() {
        return Err(FmtError::Missing(path.to_path_buf()));
    }
    let mut files = Vec::new();
    if path.is_dir() {
        collect_glyph_files(path, &mut files)?;
    } else {
        files.push(path.to_path_buf());
    }
    files.sort();

    let mut report = FmtReport::default();
    for file in files {
        let src = std::fs::read_to_string(&file).map_err(|e| FmtError::Io {
            path: file.clone(),
            source: e,
        })?;
        match glyph_parser::parse(&src) {
            Ok(module) => {
                // Comments are recovered separately (the parser drops them) and
                // re-emitted in place by the formatter.
                let comments = glyph_lexer::comments(&src);
                let formatted = format_module(&module, &comments, &src);
                if formatted == src {
                    report.unchanged.push(file);
                } else {
                    std::fs::write(&file, formatted).map_err(|e| FmtError::Io {
                        path: file.clone(),
                        source: e,
                    })?;
                    report.formatted.push(file);
                }
            }
            Err(e) => report.failed.push((file, format!("{e:?}"))),
        }
    }
    Ok(report)
}

/// Recursively collect `.glyph` files, skipping dot-directories and `target/`
/// (the same walk policy as `glyph build`).
fn collect_glyph_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), FmtError> {
    for entry in std::fs::read_dir(dir).map_err(|e| FmtError::Io {
        path: dir.to_path_buf(),
        source: e,
    })? {
        let entry = entry.map_err(|e| FmtError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| FmtError::Io {
            path: path.clone(),
            source: e,
        })?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            collect_glyph_files(&path, out)?;
        } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("glyph") {
            out.push(path);
        }
    }
    Ok(())
}
