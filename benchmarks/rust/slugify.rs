use regex::Regex;
use std::sync::LazyLock;

static NON_ALPHANUMERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
static EDGE_DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-+|-+$").unwrap());

pub fn slugify(s: &str) -> String {
    let lower = s.to_lowercase();
    let dashed = NON_ALPHANUMERIC.replace_all(&lower, "-");
    EDGE_DASHES.replace_all(&dashed, "").to_string()
}
