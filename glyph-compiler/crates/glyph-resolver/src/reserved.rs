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
}
