//! E0244 (G389): a built-in member TypeScript's library marks `@deprecated`.
//!
//! ```text
//! s.substr(1, 2)
//! // ^ E0244 (warning): `s.substr` is deprecated: A legacy feature for browser compatibility
//! //   help: write `s.slice(1, 3)`, which reads the same characters
//! ```
//!
//! A warning, not an error: every one of these members runs in every engine
//! (Annex B keeps them) and `tsc --strict` accepts them. The tag's text comes
//! from the generated row and is quoted as written; the help is computed from
//! the row's replacement and the call as written. `glyph fix` writes nothing:
//! `substr(a, n)` as `slice(a, a + n)` would evaluate `a` twice, and the
//! renames are left for a later release (decision record 182, section 5b).

use glyph_ast::{Expr, UnaryOp};

use crate::builtin_members::{Deprecated, Replacement};
use crate::pending::expr_source;

/// The deprecated member as written: the receiver, the dot, the member, and
/// the call's arguments when the member is called.
pub(crate) struct DeprecatedUse<'a> {
    pub recv: &'a Expr,
    pub optional: bool,
    pub method: &'a str,
    pub deprecated: Deprecated,
    /// `None` for a read that is not a call.
    pub args: Option<&'a [Expr]>,
}

/// `e` as code a help can quote, when it has a plain spelling: a name, a
/// member chain, a number literal (negated included), or a call over those.
/// A string literal is left out because it cannot stand inside a `${}` of
/// another string, and `expr_source`'s `...` would read as a spread.
fn plain(e: &Expr) -> Option<String> {
    let text = match e {
        Expr::Unary { op: UnaryOp::Neg, operand, .. } => match operand.as_ref() {
            Expr::Number { raw, .. } => format!("-{raw}"),
            _ => return None,
        },
        other => expr_source(other),
    };
    (!text.contains("...") && !text.contains('"')).then_some(text)
}

/// Whether `e` is a name or a member chain, read without running anything,
/// so writing it twice in a help changes nothing.
fn read_only(e: &Expr) -> bool {
    match e {
        Expr::Ident { .. } | Expr::Number { .. } => true,
        Expr::Member { object, .. } => read_only(object),
        _ => false,
    }
}

/// A non-negative integer literal's value.
fn count(e: &Expr) -> Option<u64> {
    match e {
        Expr::Number { raw, .. } => raw.parse::<u64>().ok(),
        _ => None,
    }
}

/// E0244's message and help for `u`.
pub(crate) fn deprecated_member(u: &DeprecatedUse) -> (String, String) {
    let dot = if u.optional { "?." } else { "." };
    let recv = plain(u.recv);
    let message = match &recv {
        Some(r) => format!("`{r}{dot}{}` is deprecated: {}", u.method, u.deprecated.tag),
        None => format!("`{}` is deprecated: {}", u.method, u.deprecated.tag),
    };
    let help = match u.deprecated.replacement {
        Replacement::Rename(to) => rename_help(u, recv.as_deref(), dot, to),
        Replacement::Slice => slice_help(u, recv.as_deref(), dot),
        Replacement::Markup { tag, attr } => markup_help(u, recv.as_deref(), tag, attr),
    };
    (message, help)
}

fn rename_help(u: &DeprecatedUse, recv: Option<&str>, dot: &str, to: &str) -> String {
    let same = "the same function under its standard name";
    let args = u.args.map(|a| a.iter().map(plain).collect::<Option<Vec<_>>>());
    match (recv, args) {
        (Some(r), Some(Some(a))) => format!("write `{r}{dot}{to}({})`, {same}", a.join(", ")),
        (Some(r), None) => format!("write `{r}{dot}{to}`, {same}"),
        _ => format!("call `{to}` in its place, {same}"),
    }
}

fn slice_help(u: &DeprecatedUse, recv: Option<&str>, dot: &str) -> String {
    let general = "write `slice(start, start + length)` in its place, which reads the same characters when the \
                   start and the length are zero or more";
    let (Some(r), Some(args)) = (recv, u.args) else {
        return general.to_string();
    };
    match args {
        // `substr(a)` and `slice(a)` agree for every `a`, a negative one
        // counting from the end in both.
        [a] => match plain(a) {
            Some(a) => format!("write `{r}{dot}slice({a})`, which reads the same characters"),
            None => general.to_string(),
        },
        [a, n] => match (count(a), count(n)) {
            (Some(start), Some(len)) => match start.checked_add(len) {
                Some(end) => format!("write `{r}{dot}slice({start}, {end})`, which reads the same characters"),
                None => general.to_string(),
            },
            _ => match (plain(a), plain(n)) {
                (Some(sa), Some(sn)) if read_only(a) => format!(
                    "write `{r}{dot}slice({sa}, {sa} + {sn})`, which reads the same characters when `{sa}` and \
                     `{sn}` are zero or more"
                ),
                _ => general.to_string(),
            },
        },
        _ => general.to_string(),
    }
}

fn markup_help(u: &DeprecatedUse, recv: Option<&str>, tag: &str, attr: Option<&str>) -> String {
    // A receiver or an argument that is not a plain name is written as a
    // placeholder: a string literal cannot stand inside another string's
    // `${}`, and a call there would hide the markup.
    let text = recv.filter(|_| read_only(u.recv)).unwrap_or("text").to_string();
    match attr {
        None => format!("build the markup as text: `\"<{tag}>${{{text}}}</{tag}>\"`"),
        Some(attr) => {
            let value = match u.args {
                Some([a]) if read_only(a) => plain(a),
                _ => None,
            }
            .unwrap_or_else(|| attr.to_string());
            match u.args {
                // A number is written into the markup as it stands, and
                // holds no `"` to escape.
                Some([Expr::Number { raw, .. }]) => format!(
                    "build the markup as text: `\"<{tag} {attr}=\\\"{raw}\\\">${{{text}}}</{tag}>\"`"
                ),
                _ => format!(
                    "build the markup as text: `\"<{tag} {attr}=\\\"${{{value}}}\\\">${{{text}}}</{tag}>\"`; `{}` also \
                     writes each `\"` in `{value}` as `&quot;`",
                    u.method
                ),
            }
        }
    }
}
