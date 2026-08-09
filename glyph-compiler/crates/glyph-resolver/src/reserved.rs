//! TypeScript reserved words that Glyph's lexer permits as identifiers.
//!
//! Glyph's keyword set (`glyph-lexer`'s `KEYWORDS`) is smaller than
//! TypeScript's reserved-word set, so a name like `class`, `new`, `switch`, or
//! `eval` lexes as an ordinary identifier and can be used as a declaration,
//! parameter, or binding name. The emitter copies such a name verbatim, so it
//! reaches `tsc` in a binding position where it is illegal (a plain reserved
//! word cannot name a `function`/`const`/parameter, and `eval`/`arguments`
//! cannot be bound in a strict-mode module). The result is a cascade of opaque
//! `tsc` errors mapped back to the Glyph source.
//!
//! Glyph is deliberately stricter than TypeScript, so the fix is to reject
//! these as identifier names at resolve time (a clean Glyph diagnostic) rather
//! than mangle them at emit time (which would break import-name matching across
//! modules). This list is the TypeScript reserved words that are *not* already
//! Glyph keywords — the ones that would otherwise slip through. Words already in
//! `KEYWORDS` (`let`, `const`, `interface`, `for`, `if`, `return`, `void`, ...)
//! never reach here because the lexer tokenizes them as keywords.
//!
//! Object keys, record field names, and member access are unaffected: those are
//! not binding positions and emit as-is (`{ default: v }`, `x.new` are valid
//! TS). Only names that become a TS *binding* identifier are checked.

/// True if `name` is a TypeScript reserved word that is not a Glyph keyword,
/// and so is illegal as an emitted TS binding identifier.
pub(crate) fn is_reserved_ts_word(name: &str) -> bool {
    matches!(
        name,
        // Reserved keywords (always, in any mode) that Glyph does not itself
        // reserve.
        "case"
            | "catch"
            | "class"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "enum"
            | "export"
            | "extends"
            | "finally"
            | "function"
            | "instanceof"
            | "new"
            | "null"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "while"
            | "with"
            // Strict-mode reserved words (a TS module is always strict).
            | "implements"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
            // Not keywords, but illegal as binding names in a strict-mode
            // module — the footgun that produces the opaque `tsc` "Invalid use
            // of 'eval'" / 'arguments' error.
            | "eval"
            | "arguments"
    )
}

// ---------------------------------------------------------------------------
// Global-shadow guard
// ---------------------------------------------------------------------------

/// Names that are legal TypeScript identifiers but are already bound in every
/// emitted module, so a Glyph top-level declaration carrying one silently
/// rebinds it.
///
/// This is a different claim from [`is_reserved_ts_word`]: those words `tsc`
/// itself rejects in a binding position (a loud, if badly-attributed, error).
/// These ones compile. `type Value = | Num(number) | Error(string)` emits
/// `export function Error(...)` at module top level, and every later
/// `new Error(...)` in the same module resolves to the variant constructor.
/// `type Key = string | number` emits `export const number`, which shadows the
/// prelude `number` namespace and breaks `number.to_string`. Both build clean
/// and mean the wrong thing, which is why the check lives here (verifiability)
/// instead of being left to `tsc`.
///
/// # Keeping this list true
///
/// Group 1 is derived from `glyph-emit`: every JavaScript global that appears
/// inside a string literal the emitter writes into its output. **Adding a new
/// global reference to the emitter requires adding it here**, the same
/// both-directions discipline as the lexer's `KEYWORDS` table. The
/// `emitter_globals_are_all_listed` test below greps `glyph-emit/src` and fails
/// when the two drift apart.
///
/// Group 2 is the ambient prelude: the globals declared in
/// `runtime/glyph-prelude.d.ts` (`par`, `print`, `assert`, `number`) plus
/// Glyph's primitive type names, which `crates/glyph-resolver/src/prelude.rs`
/// puts in scope in every module without an import.
///
/// Std namespace names (`io`, `math`, `path`, ...) are deliberately *not*
/// here: they are only in scope in a module that imports them, and that case is
/// already `E0100` (duplicate top-level name) at the same declaration span.
/// Rejecting `fn path` in a module that never imports `std/path` would make
/// E0110's "this declaration would shadow it" claim false.
pub(crate) fn shadowed_global(name: &str) -> Option<ShadowOrigin> {
    if JS_GLOBALS.contains(&name) {
        return Some(ShadowOrigin::JsGlobal);
    }
    if PRELUDE_GLOBALS.contains(&name) {
        return Some(ShadowOrigin::Prelude);
    }
    None
}

/// JavaScript globals the emitted TypeScript refers to. Keep in step with
/// `glyph-emit`; see [`shadowed_global`].
pub(crate) const JS_GLOBALS: &[&str] = &[
    // `Object.keys` / `Object.entries` / `Object.values` in record descriptors
    // and `for ... in` lowering.
    "Object",
    // `Array<T>` in emitted type positions and `Array.isArray` in descriptors.
    "Array",
    // `Promise<T>` as every `async fn`'s emitted return type.
    "Promise",
    // `Number.isInteger` in the `int` boundary check (D31).
    "Number",
    // `new Error(...)` in the lowering of `?`, `match` fallthrough, and the
    // descriptor `parse` throw paths.
    "Error",
    // `Record<string, unknown>` in emitted field access and in `redact`. A
    // TypeScript built-in utility type, not a Glyph prelude name: it belongs
    // here so E0110 says where it actually comes from.
    "Record",
    // `Date` is deliberately absent: the emitter never writes it (the only
    // occurrence in glyph-emit is a test fixture for `extern_ts("Date.now()")`),
    // so a module declaring `type Date` shadows nothing and rejecting it would
    // make E0110's claim false. `examples/corpus/calendar.glyph` is the real
    // program this would have broken. The drift test below adds it the day the
    // emitter starts referencing it.
];

/// Prelude names in scope in every module with no import: the ambient globals
/// declared in `runtime/glyph-prelude.d.ts` plus Glyph's primitive type names.
pub(crate) const PRELUDE_GLOBALS: &[&str] = &[
    // Ambient values from runtime/glyph-prelude.d.ts.
    "par", "print", "assert", "number", // Primitive type names (see prelude.rs).
    "string", "int", "bigint", "bool", "void", "unknown", "never",
    // Ambient *types* the emitter writes on its own initiative. A module-local
    // declaration of one of these wins over the ambient prelude and breaks the
    // emitted code, so it is the same failure E0110 already names for JS
    // globals.
    //
    // `Issue[]` in every descriptor's `parse` return type and in the record
    // descriptor's `const __issues: Issue[] = []`.
    "Issue",
    // `Record` is emitted the same way but is not a prelude name: it is a
    // TypeScript built-in, so it sits in JS_GLOBALS with `Array`/`Promise`/
    // `Error`, and E0110 names that origin instead of this one.
    //
    // Deliberately absent, on the `Date` precedent: `Schema`, `Component`,
    // `Option`. The emitter only ever writes those because the *user* wrote
    // them in a type annotation, so a module declaring one shadows nothing.
    // `Result`, `Ok`, `Err`, `infer_output` and the `schema` factory are all
    // emitted under `__Glyph`-prefixed aliases, so they cannot be shadowed.
];

/// Where a shadowed name comes from. Drives the E0110 message, whose whole job
/// is to name the mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowOrigin {
    JsGlobal,
    Prelude,
}

impl ShadowOrigin {
    /// The phrase that goes into "the emitted module references the {} `X`".
    pub fn describe(self) -> &'static str {
        match self {
            ShadowOrigin::JsGlobal => "JavaScript global",
            ShadowOrigin::Prelude => "Glyph prelude global",
        }
    }
}

/// True if `name` is one of Glyph's primitive type names. Used to recognize the
/// `type Key = string | number` misparse, where D8's tagged-union syntax turns
/// bare primitives into variant constructors.
pub(crate) fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name,
        "string" | "number" | "int" | "bigint" | "bool" | "void" | "unknown" | "never"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_reserved_words() {
        for w in [
            "class", "new", "switch", "eval", "arguments", "default", "typeof", "this", "static",
            "yield", "enum", "function",
        ] {
            assert!(is_reserved_ts_word(w), "{w} should be reserved");
        }
    }

    #[test]
    fn allows_ordinary_and_glyph_keyword_names() {
        // Glyph keywords never reach here (the lexer eats them), and ordinary
        // identifiers are fine.
        for w in [
            "eval_result", "klass", "widget", "let", "const", "interface", "match", "value", "map",
            "filter", "News", "classy",
        ] {
            assert!(!is_reserved_ts_word(w), "{w} should be allowed");
        }
    }

    #[test]
    fn flags_js_globals_the_emitter_references() {
        for w in ["Error", "Number", "Object", "Array", "Promise", "Record"] {
            assert_eq!(
                shadowed_global(w),
                Some(ShadowOrigin::JsGlobal),
                "{w} should be flagged as a JS global"
            );
        }
        // `Date` is not emitted by the compiler, so it stays legal (see
        // JS_GLOBALS). If this flips, the corpus calendar module needs renaming.
        assert_eq!(shadowed_global("Date"), None);
    }

    #[test]
    fn flags_prelude_globals() {
        for w in [
            "number", "par", "assert", "print", "string", "int", "bigint", "bool", "void",
            "unknown", "never", "Issue",
        ] {
            assert_eq!(
                shadowed_global(w),
                Some(ShadowOrigin::Prelude),
                "{w} should be flagged as a prelude global"
            );
        }
        // The other half of the rule: a prelude type the emitter never writes
        // on its own initiative stays legal, same as `Date` among the JS
        // globals. `Schema`/`Component` only reach the output because the user
        // wrote them in an annotation.
        assert_eq!(shadowed_global("Schema"), None);
        assert_eq!(shadowed_global("Component"), None);
    }

    #[test]
    fn allows_ordinary_names_and_unimported_std_namespaces() {
        // `path`/`io`/`math` name std modules but are only in scope when the
        // module imports them, which is already E0100.
        for w in [
            "Value", "make_error", "ErrorKind", "Numbers", "path", "io", "math", "json", "Result",
            "Ok",
        ] {
            assert_eq!(shadowed_global(w), None, "{w} should be allowed");
        }
    }

    /// Anti-drift: every JavaScript global the emitter writes into its output
    /// must be in `JS_GLOBALS`. `Number` was harmless until the `int` boundary
    /// check started emitting `Number.isInteger`, which is exactly how the
    /// shadow bug shipped. Scans string literals in `glyph-emit/src` for
    /// `new Global(` / `Global.member` over a curated set of standard globals.
    #[test]
    fn emitter_globals_are_all_listed() {
        const CANDIDATES: &[&str] = &[
            "Object",
            "Array",
            "Promise",
            "Number",
            "Error",
            "Record",
            "Date",
            "JSON",
            "Math",
            "String",
            "Boolean",
            "Symbol",
            "Map",
            "Set",
            "WeakMap",
            "WeakSet",
            "RegExp",
            "BigInt",
            "Proxy",
            "Reflect",
            "Intl",
            "ArrayBuffer",
            "TypeError",
            "RangeError",
            "SyntaxError",
            "Function",
        ];

        let emit_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../glyph-emit/src")
            .canonicalize()
            .expect("glyph-emit/src must exist next to glyph-resolver");

        // Ambient prelude *types*. Same drift risk, different list: these are
        // shadowable by a module-local `type X`, which is how a module-local
        // `Issue` silently broke every descriptor in its module.
        const PRELUDE_CANDIDATES: &[&str] = &[
            "Issue",
            "Schema",
            "Component",
            "Option",
            "Result",
            "infer_output",
        ];

        let mut seen: Vec<(String, String)> = Vec::new();
        let mut seen_prelude: Vec<(String, String)> = Vec::new();
        for entry in std::fs::read_dir(&emit_src).expect("read glyph-emit/src") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let full = std::fs::read_to_string(&path).expect("read emit source");
            // Only the emitter's own output counts. Test modules quote plenty of
            // TypeScript the compiler never writes (`extern_ts("Date.now()")`).
            let src = match full.find("\n#[cfg(test)]") {
                Some(i) => full[..i].to_string(),
                None => full,
            };
            for literal in string_literals(&src) {
                for g in CANDIDATES {
                    if mentions_global(&literal, g) {
                        seen.push((
                            (*g).to_string(),
                            path.file_name().unwrap().to_string_lossy().into_owned(),
                        ));
                    }
                }
                for g in PRELUDE_CANDIDATES {
                    if mentions_global(&literal, g) {
                        seen_prelude.push((
                            (*g).to_string(),
                            path.file_name().unwrap().to_string_lossy().into_owned(),
                        ));
                    }
                }
            }
        }

        // Guard the scanner itself: if it stops finding anything, it stops
        // guarding anything.
        for expected in ["Object", "Array", "Promise", "Number", "Error"] {
            assert!(
                seen.iter().any(|(g, _)| g == expected),
                "the drift scan found no `{expected}` in glyph-emit's emitted strings; \
                 the scanner is broken, not the emitter"
            );
        }
        {
            let expected = "Issue";
            assert!(
                seen_prelude.iter().any(|(g, _)| g == expected),
                "the drift scan found no `{expected}` in glyph-emit's emitted strings; \
                 the scanner is broken, not the emitter"
            );
        }

        let missing: Vec<_> = seen
            .iter()
            .filter(|(g, _)| !JS_GLOBALS.contains(&g.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "glyph-emit emits references to JavaScript globals that reserved.rs does not guard \
             against shadowing: {missing:?}. Add them to JS_GLOBALS (and to the reserved-word \
             table in docs/reference/reserved-words.md)."
        );

        let missing: Vec<_> = seen_prelude
            .iter()
            .filter(|(g, _)| !PRELUDE_GLOBALS.contains(&g.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "glyph-emit emits references to ambient prelude types that reserved.rs does not \
             guard against shadowing: {missing:?}. A module-local `type X` with one of these \
             names wins over the prelude and breaks the emitted module. Add them to \
             PRELUDE_GLOBALS (and to the reserved-word table in \
             docs/reference/reserved-words.md)."
        );
    }

    /// The reserved-word reference page exists so a reader can grep one file
    /// instead of three source files. That only holds while it is complete:
    /// every lexer keyword, every TS reserved word, and every shadowed global
    /// must appear on it.
    #[test]
    fn the_reserved_word_reference_lists_every_name() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repo root");
        let doc = std::fs::read_to_string(repo.join("docs/reference/reserved-words.md"))
            .expect("docs/reference/reserved-words.md must exist");

        let token_rs =
            std::fs::read_to_string(repo.join("glyph-compiler/crates/glyph-lexer/src/token.rs"))
                .expect("read token.rs");
        let keywords: Vec<&str> = token_rs
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.strip_prefix("(\"")?;
                let (word, tail) = rest.split_once('"')?;
                tail.starts_with(", Token::").then_some(word)
            })
            .collect();
        assert!(keywords.len() > 25, "keyword scrape found {keywords:?}");

        let ts_words: Vec<&str> = TS_RESERVED_FOR_DOCS.to_vec();

        for name in keywords
            .iter()
            .chain(ts_words.iter())
            .chain(JS_GLOBALS.iter())
            .chain(PRELUDE_GLOBALS.iter())
        {
            assert!(
                doc.contains(*name),
                "docs/reference/reserved-words.md does not list `{name}`"
            );
        }

        // And the reverse for the TS list, so a word dropped from the guard
        // does not linger in the docs as a rule Glyph no longer enforces.
        for w in &ts_words {
            assert!(is_reserved_ts_word(w), "`{w}` is documented but not guarded");
        }
    }

    /// The TS reserved words the doc page tabulates. Mirrors
    /// `is_reserved_ts_word`, which is a `matches!` and cannot be iterated.
    const TS_RESERVED_FOR_DOCS: &[&str] = &[
        "case",
        "catch",
        "class",
        "debugger",
        "default",
        "delete",
        "do",
        "enum",
        "export",
        "extends",
        "finally",
        "function",
        "instanceof",
        "new",
        "null",
        "super",
        "switch",
        "this",
        "throw",
        "try",
        "typeof",
        "var",
        "while",
        "with",
        "implements",
        "package",
        "private",
        "protected",
        "public",
        "static",
        "yield",
        "eval",
        "arguments",
    ];

    /// True when `literal` uses `global` the way emitted TypeScript would:
    /// `Object.keys`, `new Error(`, `Promise<T>`. A whole-word match followed
    /// by `.`, `(`, or `<`, so prose like "non-exhaustive match" and Rust
    /// identifiers embedded in format strings do not trip it.
    fn mentions_global(literal: &str, global: &str) -> bool {
        let bytes = literal.as_bytes();
        let mut from = 0;
        while let Some(rel) = literal[from..].find(global) {
            let start = from + rel;
            let end = start + global.len();
            let before_ok = start == 0
                || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
            // `[` matters as much as the rest: `Issue[]` is how the descriptor
            // return type is written, and not seeing it is exactly how the
            // `Issue` shadow bug shipped.
            let after_ok = matches!(
                bytes.get(end),
                Some(b'.') | Some(b'(') | Some(b'<') | Some(b'[')
            );
            if before_ok && after_ok {
                return true;
            }
            from = end;
        }
        false
    }

    /// Every double-quoted Rust string literal in `src`, with escapes left as
    /// written. Good enough for the drift scan: it only needs the emitted text.
    fn string_literals(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '"' {
                let mut j = i + 1;
                let mut buf = String::new();
                while j < bytes.len() && bytes[j] != '"' {
                    if bytes[j] == '\\' && j + 1 < bytes.len() {
                        buf.push(bytes[j + 1]);
                        j += 2;
                        continue;
                    }
                    if bytes[j] == '\n' {
                        break;
                    }
                    buf.push(bytes[j]);
                    j += 1;
                }
                out.push(buf);
                i = j + 1;
                continue;
            }
            i += 1;
        }
        out
    }
}
