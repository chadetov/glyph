//! Which standard-library exports are types with no runtime binding.
//!
//! Lives in its own file rather than beside the export seeds in
//! `module_graph.rs`, because `the_resolver_seed_lists_every_runtime_export`
//! finds those seeds by *scraping that file as text* for a `"std/x"` string
//! followed by its list. A second table of the same shape in the same file is
//! picked up instead of the real one, and the gate then reports every stdlib
//! function as unlisted. Keeping the two apart keeps that scraper looking at
//! exactly one table.

/// The standard-library exports that are **types only**: the runtime `.ts`
/// declares them with `export type` and ships no value of the same name.
///
/// The emitter needs this to write `import { type Option, Some, None }` rather
/// than `import { Option, Some, None }`. `tsc` elides a type-only name from a
/// value import list, which is why the second form type-checks; a type
/// *stripper* does not, and the result is a hard ESM link error against a
/// module that genuinely has no such runtime export (G114). The inline `type`
/// modifier exists precisely so a tool with no type information can strip it.
///
/// A module-declared Glyph type needs no entry here and never will: it emits a
/// runtime descriptor `const` under its own name, so the imported binding
/// exists. This is only about the hand-written standard library.
///
/// `stdlib_type_only_exports_match_the_runtime` in `glyph-cli/tests/bootstrap.rs`
/// keeps this in step with `runtime/std/*.ts`, so a new `export type` cannot
/// quietly go missing from it.
const STDLIB_TYPE_ONLY: &[(&str, &[&str])] = &[
    ("std/bytes", &["Bytes", "BytesError"]),
    ("std/collections", &["Deque"]),
    ("std/decimal", &["Decimal"]),
    ("std/fs", &["FileInfo", "FsError"]),
    (
        "std/http",
        &[
            "Fetch",
            "Handler",
            "HttpError",
            "HttpErrorKind",
            "RedirectPolicy",
            "Request",
            "Response",
        ],
    ),
    ("std/log", &["Level"]),
    ("std/dns", &["MailHost"]),
    ("std/net", &["Socket"]),
    ("std/option", &["Option"]),
    ("std/random", &["Rng"]),
    ("std/result", &["Result"]),
    ("std/set", &["Set"]),
    ("std/sqlite", &["Db", "Row"]),
    ("std/store", &["Store"]),
    ("std/stream", &["Stream"]),
    ("std/taint", &["Tainted", "Trusted"]),
    ("std/url", &["Param", "Url"]),
    ("std/task", &["Settled"]),
    ("std/timers", &["Timer"]),
    ("std/websocket", &["Socket"]),
];

/// Whether `name`, imported from `module_path`, is a standard-library export
/// with no runtime binding, so it must carry the inline `type` modifier.
pub fn is_stdlib_type_only(module_path: &str, name: &str) -> bool {
    STDLIB_TYPE_ONLY
        .iter()
        .any(|(m, names)| *m == module_path && names.contains(&name))
}

/// Every `(module, name)` pair the table declares, for the gate that reconciles
/// it against the runtime sources.
pub fn stdlib_type_only_pairs() -> Vec<(&'static str, &'static str)> {
    STDLIB_TYPE_ONLY
        .iter()
        .flat_map(|(m, names)| names.iter().map(move |n| (*m, *n)))
        .collect()
}
