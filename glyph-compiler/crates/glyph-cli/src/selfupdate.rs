//! `glyph --update` — move the installed compiler to the newest release.
//!
//! This is not `glyph upgrade`, and the difference is the whole reason it is a
//! flag rather than a subcommand. Flags act on the tool: `--version`, `--help`,
//! `--explain`, and now `--update`. Subcommands act on your code, and `upgrade`
//! rewrites a project's pinned `@glyphlang/glyph` in `package.json`. Naming the
//! two `update` and `upgrade` as sibling subcommands would have been
//! indefensible; they are synonyms in English and opposites here.
//!
//! It only updates an install it can identify. A compiler reached through
//! `npx`, built from this repo with `cargo build`, or installed by some package
//! manager we did not write is not something to overwrite on a guess: the
//! command says what it found and prints the command it would have run. The
//! failure mode being avoided is a tool that reports success while the binary on
//! `PATH` is still the old one, which is the same class as a green build that
//! proves nothing.

use crate::registry::{self, Latest};
use std::path::Path;
use std::process::Command;

const PACKAGE: &str = "@glyphlang/glyph";
const NOTES: &str = "https://glyphlang.io/versions/";

pub enum Install {
    /// Running out of a global npm install, which is the one shape we can move.
    NpmGlobal,
    /// A build from this repo's `target/`, or anything else under a cargo tree.
    LocalBuild,
    /// Reached through `npx`, so the cache decides the version, not us.
    Npx,
    /// Somewhere we do not recognise.
    Unknown,
}

/// Where is the running executable installed?
///
/// Read off `current_exe()` rather than asking npm, because the question is
/// "which binary is about to be replaced", and only the path answers that. A
/// global npm install lives under `.../lib/node_modules/@glyphlang/...`; npx
/// unpacks into a `_npx` directory; a dev build sits under `target/debug` or
/// `target/release`.
pub fn classify(exe: &Path) -> Install {
    let p = exe.to_string_lossy().replace('\\', "/");
    if p.contains("/_npx/") {
        Install::Npx
    } else if p.contains("/target/debug/") || p.contains("/target/release/") {
        Install::LocalBuild
    } else if p.contains("/node_modules/@glyphlang/") || p.contains("/node_modules/.bin/") {
        Install::NpmGlobal
    } else {
        Install::Unknown
    }
}

fn advise(install: &Install, latest: &str) {
    println!();
    match install {
        Install::Npx => {
            println!("This compiler is running from an npx cache, so there is no install to move.");
            println!("npx already resolves the newest release each time unless you pin one:");
            println!("    npx --yes {PACKAGE}@{latest} --version");
        }
        Install::LocalBuild => {
            println!("This is a build from a Glyph source tree, not an installed release.");
            println!("Rebuild it from the source you have, or install the published one:");
            println!("    npm install -g {PACKAGE}@{latest}");
        }
        _ => {
            println!("Could not tell how this compiler was installed, so nothing was changed.");
            println!("If it came from npm, this is the command:");
            println!("    npm install -g {PACKAGE}@{latest}");
        }
    }
}

/// Returns the process exit code.
pub fn run(current: &str, dry_run: bool) -> i32 {
    let latest = match registry::latest() {
        Latest::Known(v) => v,
        Latest::Unknown(why) => {
            eprintln!("glyph --update: could not reach the registry ({why}).");
            eprintln!("Check your connection, or install a version directly:");
            eprintln!("    npm install -g {PACKAGE}@latest");
            return 1;
        }
    };

    if latest == current {
        println!("glyph {current} is the newest published release. Nothing to do.");
        return 0;
    }

    // A dev build between a bump and its publish is ahead of the registry, and
    // telling someone to "update" to an older version would be wrong.
    if registry::parse(current) > registry::parse(&latest) {
        println!("glyph {current} is newer than the newest published release ({latest}).");
        println!("This is a development build; there is nothing to update to.");
        return 0;
    }

    let exe = std::env::current_exe().unwrap_or_default();
    let install = classify(&exe);

    println!("glyph {current} is installed; {latest} is published.");
    println!("Release notes: {NOTES}");
    println!(
        "A 0.1.x release may reject code that compiled before, so read the notes \
         for anything between these two."
    );

    if !matches!(install, Install::NpmGlobal) {
        advise(&install, &latest);
        return 1;
    }

    if dry_run {
        println!();
        println!("Would run: npm install -g {PACKAGE}@{latest}");
        return 0;
    }

    println!();
    println!("Running: npm install -g {PACKAGE}@{latest}");
    let status = Command::new("npm")
        .args(["install", "-g", &format!("{PACKAGE}@{latest}"), "--no-audit", "--no-fund"])
        .status();

    match status {
        Ok(s) if s.success() => {
            // Do not claim the update landed. npm can exit zero having skipped an
            // optionalDependency it could not fetch, which is exactly how a
            // release once shipped with no platform binary, so the honest report
            // is to name what to check rather than to assert success.
            println!();
            println!("npm finished. Confirm with:");
            println!("    glyph --version");
            0
        }
        Ok(s) => {
            eprintln!();
            eprintln!("npm exited with {}. Nothing was changed by glyph.", s.code().unwrap_or(-1));
            eprintln!("A global install may need different permissions than your user has.");
            1
        }
        Err(e) => {
            eprintln!();
            eprintln!("could not run npm: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_npm_global_install_is_recognised() {
        let p = Path::new("/Users/x/.npm-global/lib/node_modules/@glyphlang/glyph/bin/glyph.js");
        assert!(matches!(classify(p), Install::NpmGlobal));
    }

    #[test]
    fn a_dev_build_is_not_something_to_overwrite() {
        let p = Path::new("/Users/x/glyph/glyph-compiler/target/release/glyph");
        assert!(matches!(classify(p), Install::LocalBuild));
    }

    #[test]
    fn an_npx_cache_is_not_an_install() {
        let p = Path::new("/Users/x/.npm/_npx/abc123/node_modules/@glyphlang/glyph/bin/glyph.js");
        assert!(matches!(classify(p), Install::Npx));
    }

    #[test]
    fn an_unrecognised_path_is_never_assumed_updatable() {
        let p = Path::new("/opt/homebrew/bin/glyph");
        assert!(matches!(classify(p), Install::Unknown));
    }
}
