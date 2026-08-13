//! Asking npm which Glyph is published, for `doctor` and `upgrade`.
//!
//! The compiler has no HTTP client and this deliberately does not add one. npm
//! is already a prerequisite (it is how Glyph is installed, and `gen` already
//! shells out to `npm root -g`), so `npm view` answers the only registry
//! question we have without pulling a TLS stack and its dependency tree into a
//! compiler that currently has neither.
//!
//! Two rules hold everywhere this is used:
//!
//! - **Only commands the user runs to ask a question may call it.** `doctor` and
//!   `upgrade`, never `build`/`run`/`check`. A compiler that reaches the network
//!   on every build is a different product, and `glyph llms` advertises working
//!   offline.
//! - **Not reaching the registry is not a failure.** Offline is a normal state.
//!   Every caller degrades to saying it does not know.

use std::process::Command;

/// The published package this binary is a build of.
pub const PACKAGE: &str = "@glyphlang/glyph";

/// Where a human reads what changed between two versions.
pub const RELEASE_NOTES: &str = "https://glyphlang.io/versions/";

/// This binary's own version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// What the registry said, or why we do not know.
pub enum Latest {
    /// npm answered with a version.
    Known(String),
    /// npm is absent, unreachable, or answered something unparseable. Carries a
    /// short reason for the report; never an error the caller has to handle.
    Unknown(&'static str),
}

/// Ask npm for the latest published version.
///
/// `--fetch-retries=0` and a short `--fetch-timeout` matter: npm's defaults
/// retry with backoff, so an offline `glyph doctor` would sit for the better
/// part of a minute before admitting it could not connect. A diagnostic command
/// that hangs is worse than one that says "unknown".
pub fn latest() -> Latest {
    let output = Command::new("npm")
        .args([
            "view",
            PACKAGE,
            "version",
            "--fetch-retries=0",
            "--fetch-timeout=3000",
            "--no-audit",
            "--no-fund",
        ])
        .output();

    match output {
        Err(_) => Latest::Unknown("npm not found"),
        Ok(o) if !o.status.success() => Latest::Unknown("registry unreachable"),
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            match text.lines().map(str::trim).find(|l| !l.is_empty()) {
                Some(v) if parse(v).is_some() => Latest::Known(v.to_string()),
                Some(_) => Latest::Unknown("unexpected npm output"),
                None => Latest::Unknown("registry unreachable"),
            }
        }
    }
}

/// A dotted version as comparable numbers. `None` when it is not `x.y.z`.
///
/// Prerelease and build metadata (`0.1.73-rc.1`, `0.1.73+build`) are compared on
/// their release part only. Nothing in the 0.1.x line publishes them, and
/// treating `0.1.73-rc.1` as newer than `0.1.73` would be wrong.
pub fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let release = v.split(['-', '+']).next()?;
    let mut parts = release.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    match parts.next() {
        Some(_) => None,
        None => Some((major, minor, patch)),
    }
}

/// Is `candidate` a later version than `than`? `None` if either is unparseable,
/// so a caller reports "unknown" rather than guessing a direction.
pub fn is_newer(candidate: &str, than: &str) -> Option<bool> {
    Some(parse(candidate)? > parse(than)?)
}

#[cfg(test)]
mod tests {
    use super::{is_newer, parse};

    #[test]
    fn parses_dotted_versions() {
        assert_eq!(parse("0.1.72"), Some((0, 1, 72)));
        assert_eq!(parse("1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse("0.1.73-rc.1"), Some((0, 1, 73)));
        assert_eq!(parse("0.1"), None);
        assert_eq!(parse("0.1.72.1"), None);
        assert_eq!(parse("^0.1.72"), None);
        assert_eq!(parse("latest"), None);
    }

    #[test]
    fn orders_versions_numerically_not_lexically() {
        // The case a string compare gets wrong, and the one this line ships in.
        assert_eq!(is_newer("0.1.100", "0.1.99"), Some(true));
        assert_eq!(is_newer("0.1.73", "0.1.72"), Some(true));
        assert_eq!(is_newer("0.1.72", "0.1.72"), Some(false));
        assert_eq!(is_newer("0.1.71", "0.1.72"), Some(false));
        assert_eq!(is_newer("0.2.0", "0.1.99"), Some(true));
    }

    #[test]
    fn unparseable_versions_report_unknown_rather_than_a_direction() {
        assert_eq!(is_newer("garbage", "0.1.72"), None);
        assert_eq!(is_newer("0.1.72", "garbage"), None);
    }
}
