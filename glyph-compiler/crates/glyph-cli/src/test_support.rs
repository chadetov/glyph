//! Toolchain gates shared by the test suites.
//!
//! Many tests shell out to `tsc`, `tsx`, `node` or the `typescript` package
//! and cannot run without them. Returning early on a missing tool used to make
//! libtest report the test as passed, so a CI image without `tsc` would have
//! gone green having checked nothing. The rule here is that a missing tool is
//! a skip on a developer machine and a failure under CI: when the `CI`
//! environment variable is set (to anything but an empty string, `0` or
//! `false`; GitHub Actions sets `CI=true`), [`tool_missing`] panics naming the
//! tool.
//!
//! Every test that skips on a missing tool goes through [`tool_missing`],
//! directly or through one of the `*_available` helpers, so the policy lives
//! in one place.

/// True when the `CI` environment variable says this is a CI run.
pub fn in_ci() -> bool {
    match std::env::var("CI") {
        Ok(v) => {
            let v = v.trim();
            !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false"))
        }
        Err(_) => false,
    }
}

/// A test needs `tool` and it is not there. Under CI this panics, because a
/// test that returns early is reported as passed. Locally it prints that the
/// test skipped and returns, so the caller can `return`.
#[track_caller]
pub fn tool_missing(tool: &str) {
    if in_ci() {
        panic!(
            "`{tool}` is not available and CI is set: this test would have \
             skipped and been reported as passed. Install `{tool}` on the CI \
             image (unset CI to skip locally)."
        );
    }
    eprintln!("skipping: `{tool}` is not available (this is a failure when CI is set)");
}

/// True when `cmd --version` runs and exits zero.
pub fn runs(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// True when `cmd` runs; otherwise reports it through [`tool_missing`] (a
/// panic under CI) and returns false.
#[track_caller]
pub fn require(cmd: &str) -> bool {
    if runs(cmd) {
        return true;
    }
    tool_missing(cmd);
    false
}

/// `tsc` is runnable by name. A panic under CI when it is not.
#[track_caller]
pub fn tsc_available() -> bool {
    require("tsc")
}

/// Both `node` and `tsx` are runnable. `glyph run` shells out to `tsx`, which
/// itself needs `node`. A panic under CI when either is missing.
#[track_caller]
pub fn js_toolchain_available() -> bool {
    require("node") && require("tsx")
}
