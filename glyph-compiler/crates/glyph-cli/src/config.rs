//! The `"glyph"` key in `package.json` (Q22) and the audit-currency check it
//! drives.
//!
//! Glyph deliberately has no separate `glyph.json`: one config file, one source
//! of truth, composing with existing npm tooling. The `"glyph"` key carries
//! per-import audit metadata; `glyph publish` checks that third-party imports
//! have been reviewed within a window (the supply-chain face of the
//! verifiability pillar). npm's own `package-lock.json` integrity hashes carry
//! the content-pinning load, so this layer is about *review currency*, not
//! integrity.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The fields of `package.json` Glyph reads. Unknown fields are ignored.
#[derive(Debug, Deserialize, Default)]
pub struct PackageJson {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub glyph: Option<GlyphConfig>,
}

/// The `"glyph"` key.
#[derive(Debug, Deserialize, Default)]
pub struct GlyphConfig {
    /// Per-import audit metadata, keyed by the import's module path.
    #[serde(default)]
    pub imports: BTreeMap<String, ImportAudit>,
    /// The audit-currency policy.
    #[serde(default)]
    pub audit: AuditPolicy,
    /// Source directory relative to `package.json`. When absent, `glyph publish`
    /// uses `src/` if it exists, else the project directory.
    #[serde(default)]
    pub src: Option<String>,
}

/// One import's audit record.
#[derive(Debug, Deserialize)]
pub struct ImportAudit {
    pub audit: AuditKind,
    /// `YYYY-MM-DD` of the last review. Required for `third-party` imports.
    #[serde(default)]
    pub last_reviewed: Option<String>,
}

/// Who vouches for an import.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuditKind {
    FirstParty,
    ThirdParty,
    Internal,
}

/// How strictly review currency is enforced.
#[derive(Debug, Deserialize)]
pub struct AuditPolicy {
    /// Maximum age of a third-party review before it is stale.
    #[serde(default = "default_max_age")]
    pub max_age_months: u32,
    /// When true (the default), stale reviews fail `glyph publish`; when false,
    /// they only warn.
    #[serde(default = "default_true")]
    pub enforce: bool,
}

fn default_max_age() -> u32 {
    6
}
fn default_true() -> bool {
    true
}

impl Default for AuditPolicy {
    fn default() -> Self {
        AuditPolicy {
            max_age_months: default_max_age(),
            enforce: default_true(),
        }
    }
}

/// Read and parse `<dir>/package.json`. `Ok(None)` if the file is absent;
/// `Err` if it is present but unreadable or malformed.
pub fn read_package_json(dir: &Path) -> Result<Option<PackageJson>, String> {
    let path = dir.join("package.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| format!("invalid {}: {e}", path.display()))
}

/// A `(year, month, day)` civil date.
pub type Date = (i64, u32, u32);

/// One import that failed the audit-currency check.
#[derive(Debug, PartialEq, Eq)]
pub struct StaleImport {
    pub path: String,
    pub reason: StaleReason,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StaleReason {
    /// A `third-party` import with no `last_reviewed` date.
    NeverReviewed,
    /// A `third-party` import whose review is older than the window.
    ReviewExpired { months_ago: i64, max: u32 },
    /// `last_reviewed` was not a `YYYY-MM-DD` date.
    BadDate(String),
}

/// Evaluate audit currency against `today`. Pure (the date is injected) so it is
/// unit-testable. Only `third-party` imports are checked; `internal` and
/// `first-party` are trusted.
pub fn check_audit_currency(config: &GlyphConfig, today: Date) -> Vec<StaleImport> {
    let mut stale = Vec::new();
    for (path, record) in &config.imports {
        if record.audit != AuditKind::ThirdParty {
            continue;
        }
        let reason = match &record.last_reviewed {
            None => Some(StaleReason::NeverReviewed),
            Some(s) => match parse_date(s) {
                None => Some(StaleReason::BadDate(s.clone())),
                Some(reviewed) => {
                    let months = months_between(reviewed, today);
                    if months > config.audit.max_age_months as i64 {
                        Some(StaleReason::ReviewExpired {
                            months_ago: months,
                            max: config.audit.max_age_months,
                        })
                    } else {
                        None
                    }
                }
            },
        };
        if let Some(reason) = reason {
            stale.push(StaleImport { path: path.clone(), reason });
        }
    }
    stale
}

/// Whole months from `from` to `to` (negative if `to` precedes `from`). Day of
/// month is accounted for so a review on the 20th is not "1 month old" on the
/// 5th of the next month.
fn months_between(from: Date, to: Date) -> i64 {
    let (fy, fm, fd) = from;
    let (ty, tm, td) = to;
    let mut months = (ty - fy) * 12 + (tm as i64 - fm as i64);
    if (td as i64) < (fd as i64) {
        months -= 1;
    }
    months
}

/// Parse `YYYY-MM-DD`. Returns `None` for any other shape.
fn parse_date(s: &str) -> Option<Date> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

/// Today's civil date in UTC, from the system clock.
pub fn today() -> Date {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs / 86_400)
}

/// Civil `(year, month, day)` from a count of days since the Unix epoch
/// (Howard Hinnant's algorithm). Avoids a date-library dependency.
fn civil_from_days(z: i64) -> Date {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> GlyphConfig {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn third_party_within_window_is_current() {
        let c = cfg(r#"{ "imports": { "vendor/x": { "audit": "third-party", "last_reviewed": "2026-04-02" } } }"#);
        assert!(check_audit_currency(&c, (2026, 6, 14)).is_empty());
    }

    #[test]
    fn third_party_past_window_is_stale() {
        let c = cfg(r#"{ "imports": { "vendor/x": { "audit": "third-party", "last_reviewed": "2025-01-01" } } }"#);
        let stale = check_audit_currency(&c, (2026, 6, 14));
        assert_eq!(stale.len(), 1);
        assert!(matches!(stale[0].reason, StaleReason::ReviewExpired { .. }));
    }

    #[test]
    fn third_party_without_review_is_stale() {
        let c = cfg(r#"{ "imports": { "vendor/x": { "audit": "third-party" } } }"#);
        let stale = check_audit_currency(&c, (2026, 6, 14));
        assert_eq!(stale[0].reason, StaleReason::NeverReviewed);
    }

    #[test]
    fn internal_and_first_party_are_trusted() {
        let c = cfg(r#"{ "imports": {
            "org/a": { "audit": "internal" },
            "org/b": { "audit": "first-party" }
        } }"#);
        assert!(check_audit_currency(&c, (2026, 6, 14)).is_empty());
    }

    #[test]
    fn the_window_is_configurable() {
        let c = cfg(r#"{
            "imports": { "vendor/x": { "audit": "third-party", "last_reviewed": "2026-01-01" } },
            "audit": { "max_age_months": 3 }
        }"#);
        // ~5 months elapsed by mid-June, past a 3-month window.
        assert_eq!(check_audit_currency(&c, (2026, 6, 14)).len(), 1);
    }

    #[test]
    fn a_malformed_date_is_reported() {
        let c = cfg(r#"{ "imports": { "vendor/x": { "audit": "third-party", "last_reviewed": "April 2026" } } }"#);
        assert!(matches!(check_audit_currency(&c, (2026, 6, 14))[0].reason, StaleReason::BadDate(_)));
    }

    #[test]
    fn civil_date_matches_known_epoch_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_888), (2024, 6, 14));
    }

    #[test]
    fn default_policy_is_six_months_enforced() {
        let p = AuditPolicy::default();
        assert_eq!(p.max_age_months, 6);
        assert!(p.enforce);
    }
}
