//! `glyph upgrade` — move a project's pinned compiler to a newer release.
//!
//! `glyph init` pins the compiler exactly, because a caret on a `0.x` version
//! floats the patch and this line ships new diagnostics in patch releases by
//! policy. An exact pin is the right default and it has a cost: without a way to
//! move it, a project stays on the version that scaffolded it forever. This is
//! that way. Upgrading becomes one command run on purpose, which is the whole
//! point of pinning in the first place.
//!
//! It rewrites the `@glyphlang/glyph` entry in `package.json` and runs
//! `npm install` so the change takes effect. It does not touch source, and it
//! prints the release-notes URL, because the releases it moves across are
//! allowed to reject code that previously compiled.

use crate::registry::{self, Latest};
use std::path::{Path, PathBuf};
use std::process::Command;

pub enum UpgradeError {
    /// No `package.json` at or above the given directory.
    NoManifest(PathBuf),
    /// The manifest is not readable or not writable.
    Io(String),
    /// The manifest has no `@glyphlang/glyph` dependency to move.
    NotPinned(PathBuf),
    /// The registry could not be reached and no explicit `--to` was given.
    NoTarget(&'static str),
    /// `npm install` failed after the pin was rewritten.
    InstallFailed(String),
}

impl std::fmt::Display for UpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoManifest(dir) => write!(
                f,
                "no `package.json` in {} or any parent. `glyph upgrade` moves a \
                 project's pinned compiler, so it needs the project's manifest; \
                 run it inside the project, or `glyph init` one first",
                dir.display()
            ),
            Self::Io(e) => write!(f, "{e}"),
            Self::NotPinned(path) => write!(
                f,
                "{} does not depend on `{}`. Nothing to upgrade: this project \
                 uses whichever compiler is on PATH, which is the thing pinning \
                 avoids. Add it with `npm install --save-dev {}`",
                path.display(),
                registry::PACKAGE,
                registry::PACKAGE
            ),
            Self::NoTarget(why) => write!(
                f,
                "could not ask npm for the latest version ({why}). Name one \
                 explicitly with `glyph upgrade --to <version>`"
            ),
            Self::InstallFailed(e) => write!(
                f,
                "the pin was updated but `npm install` failed: {e}. Run \
                 `npm install` yourself to finish, or `git checkout package.json` \
                 to undo"
            ),
        }
    }
}

pub struct UpgradeReport {
    pub manifest: PathBuf,
    pub from: String,
    pub to: String,
    /// True when the manifest already named the target, so nothing was written.
    pub already: bool,
    pub installed: bool,
}

/// Find the `package.json` governing `dir`, climbing to the filesystem root.
///
/// Climbing matters because the command is most useful from wherever the
/// developer happens to be standing in their project, not only from its root.
fn find_manifest(dir: &Path) -> Option<PathBuf> {
    let start = match dir.canonicalize() {
        Ok(p) => p,
        Err(_) => dir.to_path_buf(),
    };
    let mut cur = start.as_path();
    loop {
        let candidate = cur.join("package.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        cur = cur.parent()?;
    }
}

/// The version currently pinned for `@glyphlang/glyph`, and the exact text of
/// the whole `"name": "value"` pair so the rewrite can be a literal replacement
/// rather than a re-serialization.
///
/// Rewriting the one pair textually is deliberate: parsing and re-emitting the
/// manifest would reformat and reorder a file the developer owns, and turn a
/// one-line diff into a whole-file one. `package.json` is theirs, not ours.
fn find_pin(manifest: &str) -> Option<(String, String)> {
    let needle = format!("\"{}\"", registry::PACKAGE);
    let key_at = manifest.find(&needle)?;
    let after_key = key_at + needle.len();
    let rest = &manifest[after_key..];

    let colon_rel = rest.find(':')?;
    // Only whitespace may sit between the key and its colon; anything else means
    // this was a mention of the name somewhere other than a dependency entry.
    if !rest[..colon_rel].chars().all(char::is_whitespace) {
        return None;
    }

    let value_start_rel = rest[colon_rel + 1..].find('"')? + colon_rel + 1;
    if !rest[colon_rel + 1..value_start_rel]
        .chars()
        .all(char::is_whitespace)
    {
        return None;
    }
    let value_end_rel = rest[value_start_rel + 1..].find('"')? + value_start_rel + 1;

    let value = rest[value_start_rel + 1..value_end_rel].to_string();
    let pair = manifest[key_at..after_key + value_end_rel + 1].to_string();
    Some((value, pair))
}

/// Move `dir`'s project to `to` (or the latest published version).
pub fn run(
    dir: &Path,
    to: Option<String>,
    install: bool,
    dry_run: bool,
) -> Result<UpgradeReport, UpgradeError> {
    let manifest_path =
        find_manifest(dir).ok_or_else(|| UpgradeError::NoManifest(dir.to_path_buf()))?;
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|e| UpgradeError::Io(format!("cannot read {}: {e}", manifest_path.display())))?;

    let (from, pair) =
        find_pin(&manifest).ok_or_else(|| UpgradeError::NotPinned(manifest_path.clone()))?;

    let target = match to {
        Some(v) => v,
        None => match registry::latest() {
            Latest::Known(v) => v,
            Latest::Unknown(why) => return Err(UpgradeError::NoTarget(why)),
        },
    };

    // An exact pin is the point, so the rewrite never reintroduces a range.
    let replacement = format!("\"{}\": \"{target}\"", registry::PACKAGE);
    let already = pair == replacement;

    if already || dry_run {
        return Ok(UpgradeReport {
            manifest: manifest_path,
            from,
            to: target,
            already,
            installed: false,
        });
    }

    let updated = manifest.replacen(&pair, &replacement, 1);
    std::fs::write(&manifest_path, updated)
        .map_err(|e| UpgradeError::Io(format!("cannot write {}: {e}", manifest_path.display())))?;

    let mut installed = false;
    if install {
        let root = manifest_path.parent().unwrap_or(Path::new("."));
        let status = Command::new("npm").arg("install").current_dir(root).status();
        match status {
            Ok(s) if s.success() => installed = true,
            Ok(s) => return Err(UpgradeError::InstallFailed(format!("exit {s}"))),
            Err(e) => return Err(UpgradeError::InstallFailed(e.to_string())),
        }
    }

    Ok(UpgradeReport {
        manifest: manifest_path,
        from,
        to: target,
        already: false,
        installed,
    })
}

#[cfg(test)]
mod tests {
    use super::{find_manifest, find_pin};

    #[test]
    fn reads_an_exact_pin() {
        let m = r#"{ "devDependencies": { "@glyphlang/glyph": "0.1.72" } }"#;
        let (v, pair) = find_pin(m).expect("pin");
        assert_eq!(v, "0.1.72");
        assert_eq!(pair, r#""@glyphlang/glyph": "0.1.72""#);
    }

    #[test]
    fn reads_a_caret_range_so_an_older_scaffold_can_still_be_moved() {
        // Every project scaffolded before the pin became exact carries a caret,
        // and those are exactly the ones that most need upgrading.
        let m = r#"{ "devDependencies": { "@glyphlang/glyph": "^0.1.72" } }"#;
        let (v, _) = find_pin(m).expect("pin");
        assert_eq!(v, "^0.1.72");
    }

    #[test]
    fn tolerates_whitespace_and_newlines_around_the_colon() {
        let m = "{\n  \"devDependencies\": {\n    \"@glyphlang/glyph\"  :\n      \"0.1.70\"\n  }\n}";
        let (v, _) = find_pin(m).expect("pin");
        assert_eq!(v, "0.1.70");
    }

    #[test]
    fn a_manifest_without_the_dependency_has_no_pin() {
        let m = r#"{ "devDependencies": { "typescript": "^6.0.0" } }"#;
        assert!(find_pin(m).is_none());
    }

    #[test]
    fn a_mention_that_is_not_a_dependency_entry_is_not_a_pin() {
        // The name inside a script line is not a version to rewrite.
        let m = r#"{ "scripts": { "x": "npx @glyphlang/glyph build" } }"#;
        assert!(find_pin(m).is_none());
    }

    #[test]
    fn finds_the_manifest_from_a_subdirectory() {
        let dir = std::env::temp_dir().join(format!("glyph-upgrade-{}", std::process::id()));
        let nested = dir.join("src").join("deep");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(dir.join("package.json"), "{}").expect("write");

        let found = find_manifest(&nested).expect("climb to the manifest");
        assert_eq!(found.file_name().unwrap(), "package.json");
        assert_eq!(
            found.parent().unwrap().canonicalize().unwrap(),
            dir.canonicalize().unwrap()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
