//! `glyph doctor` — check that the JavaScript toolchain Glyph shells out to is
//! present and new enough, and say clearly how to fix it if not.
//!
//! `glyph run`/`build --check` invoke `tsx` and `tsc`; when they're missing or
//! too old the failure used to surface late and cryptically. `doctor` surfaces
//! it up front: it reports each tool's resolved version against a minimum, with
//! per-tool remediation, and exits 0 only if everything is satisfied. `--json`
//! prints the same as a machine-readable object.
//!
//! It also reports **Glyph's own version against the registry**, because nothing
//! else did. A pinned project (which is what `glyph init` now writes) never
//! learns that a release happened, and the only channel that could tell it was a
//! website you have to already know about. `doctor` is the right home for the
//! check: it is a command you run to ask questions, so a network call is
//! expected here in a way it never would be in `build`.
//!
//! **An available update is not a broken toolchain.** It never changes the exit
//! code. `doctor` runs in CI, and a green pipeline must not turn red because
//! someone published a release ten minutes ago.

use crate::registry::{self, Latest};
use std::process::Command;

/// A checked tool and the verdict.
struct Check {
    name: &'static str,
    /// The minimum major version required, or `None` for "any version".
    min_major: Option<u32>,
    found: Option<String>,
    major: Option<u32>,
    remedy: &'static str,
}

impl Check {
    fn ok(&self) -> bool {
        match (&self.found, self.min_major, self.major) {
            (None, _, _) => false,
            (Some(_), None, _) => true,
            (Some(_), Some(min), Some(maj)) => maj >= min,
            // Found but its version couldn't be parsed: treat as satisfied
            // rather than fail on an unexpected `--version` format.
            (Some(_), Some(_), None) => true,
        }
    }

    fn status(&self) -> &'static str {
        match (&self.found, self.ok()) {
            (None, _) => "missing",
            (Some(_), true) => "ok",
            (Some(_), false) => "outdated",
        }
    }
}

/// What the registry says about this binary, for the report.
enum Release {
    /// Running the newest published version.
    Current,
    /// A newer version exists.
    Update { latest: String },
    /// Running a build newer than anything published: a local dev build, or a
    /// release between its version bump and its publish. Distinguished from
    /// `Current` because saying "the newest published release" of a build that
    /// is ahead of the registry is false, and this is the state every dev build
    /// and every pre-publish CI run is in.
    Ahead { latest: String },
    /// The registry was not reachable, or the check was skipped.
    Unknown { why: &'static str },
}

/// Run `doctor`. Returns the process exit code (0 iff every tool is satisfied).
///
/// `offline` skips the registry lookup entirely, for an air-gapped machine or a
/// CI job that should make no network calls at all.
pub fn run(json: bool, offline: bool) -> i32 {
    let checks = vec![
        check("node", Some(18), "Install Node 18+ from https://nodejs.org"),
        check("tsx", None, "npm install -g tsx"),
        check("tsc", Some(5), "npm install -g typescript@6"),
    ];

    let all_ok = checks.iter().all(Check::ok);
    let release = release_status(offline);

    if json {
        print_json(&checks, all_ok, &release);
    } else {
        print_human(&checks, all_ok, &release);
    }

    // Deliberately independent of `release`: see the module comment.
    if all_ok {
        0
    } else {
        1
    }
}

fn release_status(offline: bool) -> Release {
    if offline {
        return Release::Unknown {
            why: "skipped (--offline)",
        };
    }
    match registry::latest() {
        Latest::Unknown(why) => Release::Unknown { why },
        Latest::Known(latest) => classify(&latest, registry::current()),
    }
}

/// Which of the three orderings this binary stands in against the registry.
///
/// Pure so the three cases are pinned by tests: two of them are only reachable
/// in production for a few minutes around a publish, and the third only once a
/// release has actually shipped, so none of them is exercised by ordinary use.
fn classify(latest: &str, current: &str) -> Release {
    match registry::is_newer(latest, current) {
        Some(true) => Release::Update {
            latest: latest.to_string(),
        },
        Some(false) if latest == current => Release::Current,
        Some(false) => Release::Ahead {
            latest: latest.to_string(),
        },
        None => Release::Unknown {
            why: "unexpected npm output",
        },
    }
}

/// Look the tool up on `PATH` (and, implicitly via the shell, in
/// `./node_modules/.bin` when a project runner is used), read `--version`, and
/// extract its major version.
fn check(name: &'static str, min_major: Option<u32>, remedy: &'static str) -> Check {
    let output = Command::new(name).arg("--version").output();
    let found = match output {
        Ok(o) => {
            // Only the first line: some tools (`tsx --version`) print their own
            // version and then Node's on a second line.
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let first = stdout
                .lines()
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| stderr.lines().next().map(str::trim).filter(|s| !s.is_empty()));
            first.map(str::to_string)
        }
        Err(_) => None,
    };
    let major = found.as_deref().and_then(parse_major);
    Check {
        name,
        min_major,
        found,
        major,
        remedy,
    }
}

/// Pull the major version out of a `--version` string: the first run of digits
/// that starts a dotted version (`v22.1.0`, `Version 6.0.2`, `tsx v4.19.0`,
/// `5.9.2`).
fn parse_major(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Only accept a digit run that begins a dotted version.
            if i < bytes.len() && bytes[i] == b'.' {
                return s[start..i].parse().ok();
            }
        }
        i += 1;
    }
    None
}

fn print_human(checks: &[Check], all_ok: bool, release: &Release) {
    println!("glyph — {}", registry::current());
    match release {
        Release::Current => println!("  [ok]       the newest published release"),
        Release::Update { latest } => {
            println!(
                "  [update]   {latest} is published — `glyph upgrade` moves this project to it"
            );
            println!("             what changed: {}", registry::RELEASE_NOTES);
        }
        Release::Ahead { latest } => {
            println!("  [ok]       ahead of the registry, where the newest is {latest}")
        }
        Release::Unknown { why } => println!("  [unknown]  could not check for updates ({why})"),
    }
    println!();
    println!("glyph doctor — JavaScript toolchain");
    for c in checks {
        let min = c
            .min_major
            .map(|m| format!(" (need {m}+)"))
            .unwrap_or_default();
        match c.status() {
            "ok" => println!(
                "  [ok]       {}: {}",
                c.name,
                c.found.as_deref().unwrap_or("")
            ),
            "outdated" => println!(
                "  [outdated] {}: {}{min} — {}",
                c.name,
                c.found.as_deref().unwrap_or(""),
                c.remedy
            ),
            _ => println!("  [missing]  {}{min} — {}", c.name, c.remedy),
        }
    }
    if all_ok {
        println!("All good. `glyph run` and `glyph build --check` are ready.");
    } else {
        println!("Some tools are missing or outdated; `glyph run`/`--check` need them.");
    }
}

fn print_json(checks: &[Check], all_ok: bool, release: &Release) {
    let glyph = match release {
        Release::Current => format!(
            "{{ \"version\": \"{}\", \"status\": \"ok\", \"latest\": \"{}\" }}",
            registry::current(),
            registry::current()
        ),
        Release::Update { latest } => format!(
            "{{ \"version\": \"{}\", \"status\": \"update\", \"latest\": \"{latest}\", \
             \"notes\": \"{}\" }}",
            registry::current(),
            registry::RELEASE_NOTES
        ),
        Release::Ahead { latest } => format!(
            "{{ \"version\": \"{}\", \"status\": \"ahead\", \"latest\": \"{latest}\" }}",
            registry::current()
        ),
        Release::Unknown { why } => format!(
            "{{ \"version\": \"{}\", \"status\": \"unknown\", \"latest\": null, \
             \"reason\": \"{why}\" }}",
            registry::current()
        ),
    };
    let tools: Vec<String> = checks
        .iter()
        .map(|c| {
            format!(
                "{{ \"name\": \"{}\", \"status\": \"{}\", \"version\": {}, \"remedy\": \"{}\" }}",
                c.name,
                c.status(),
                c.found
                    .as_deref()
                    .map(|v| format!("\"{}\"", v.replace('"', "'")))
                    .unwrap_or_else(|| "null".to_string()),
                c.remedy
            )
        })
        .collect();
    println!(
        "{{ \"ok\": {all_ok}, \"glyph\": {glyph}, \"tools\": [ {} ] }}",
        tools.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::{classify, parse_major, Release};

    #[test]
    fn classifies_this_binary_against_the_registry() {
        // Behind: the case the whole registry check exists to report.
        assert!(matches!(
            classify("0.1.73", "0.1.72"),
            Release::Update { ref latest } if latest == "0.1.73"
        ));
        // Level.
        assert!(matches!(classify("0.1.73", "0.1.73"), Release::Current));
        // Ahead. Every dev build and every release between its version bump and
        // its publish sits here, and calling it `Current` printed a plain
        // falsehood ("the newest published release") for that whole window.
        assert!(matches!(
            classify("0.1.72", "0.1.73"),
            Release::Ahead { ref latest } if latest == "0.1.72"
        ));
        // Numeric, not lexical: the case a string compare gets backwards.
        assert!(matches!(classify("0.1.100", "0.1.99"), Release::Update { .. }));
        assert!(matches!(classify("0.1.99", "0.1.100"), Release::Ahead { .. }));
        // Unreadable answer is reported, never guessed in either direction.
        assert!(matches!(classify("garbage", "0.1.73"), Release::Unknown { .. }));
    }

    #[test]
    fn parses_major_from_common_version_strings() {
        assert_eq!(parse_major("v22.1.0"), Some(22));
        assert_eq!(parse_major("Version 6.0.2"), Some(6));
        assert_eq!(parse_major("tsx v4.19.0"), Some(4));
        assert_eq!(parse_major("5.9.2"), Some(5));
        assert_eq!(parse_major("no version here"), None);
    }
}
