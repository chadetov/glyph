//! `glyph doctor` — check that the JavaScript toolchain Glyph shells out to is
//! present and new enough, and say clearly how to fix it if not.
//!
//! `glyph run`/`build --check` invoke `tsx` and `tsc`; when they're missing or
//! too old the failure used to surface late and cryptically. `doctor` surfaces
//! it up front: it reports each tool's resolved version against a minimum, with
//! per-tool remediation, and exits 0 only if everything is satisfied. `--json`
//! prints the same as a machine-readable object.

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

/// Run `doctor`. Returns the process exit code (0 iff every tool is satisfied).
pub fn run(json: bool) -> i32 {
    let checks = vec![
        check("node", Some(18), "Install Node 18+ from https://nodejs.org"),
        check("tsx", None, "npm install -g tsx"),
        check("tsc", Some(5), "npm install -g typescript@6"),
    ];

    let all_ok = checks.iter().all(Check::ok);

    if json {
        print_json(&checks, all_ok);
    } else {
        print_human(&checks, all_ok);
    }

    if all_ok {
        0
    } else {
        1
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

fn print_human(checks: &[Check], all_ok: bool) {
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

fn print_json(checks: &[Check], all_ok: bool) {
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
    println!("{{ \"ok\": {all_ok}, \"tools\": [ {} ] }}", tools.join(", "));
}

#[cfg(test)]
mod tests {
    use super::parse_major;

    #[test]
    fn parses_major_from_common_version_strings() {
        assert_eq!(parse_major("v22.1.0"), Some(22));
        assert_eq!(parse_major("Version 6.0.2"), Some(6));
        assert_eq!(parse_major("tsx v4.19.0"), Some(4));
        assert_eq!(parse_major("5.9.2"), Some(5));
        assert_eq!(parse_major("no version here"), None);
    }
}
