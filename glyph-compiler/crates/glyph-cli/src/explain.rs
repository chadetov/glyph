//! Long-form documentation for diagnostic codes — `glyph --explain <code>`.
//!
//! Each entry expands on the one-line `help` shown in a diagnostic: what the
//! error means, why Glyph enforces it, and a small before/after. The codes
//! match the `code()` methods on the compiler's error enums and are catalogued
//! in `docs/error-codes.md`.

/// Return the long-form documentation for `code`, or `None` if unknown. The
/// match is case-insensitive so `e0042` and `E0042` both work.
pub fn explain(code: &str) -> Option<&'static str> {
    let text = match code.to_ascii_uppercase().as_str() {
        // ----- parser (E000x) -----
        "E0001" => "E0001: lexical error\n\n\
            The lexer could not turn the source into tokens. Common causes: an \
            unterminated string, an invalid escape (only \\n \\t \\r \\\" \\\\ \
            \\u{HEX} are allowed), or a stray character.\n\n\
            Check the highlighted span and fix the malformed token.",
        "E0002" => "E0002: expected a different token\n\n\
            The parser needed a specific token here and found another. Glyph is \
            deliberately stricter than TypeScript, so this often means a rule you \
            would not hit in TS: trailing commas are required in multi-line lists, \
            there is no `if`/`else` (use `match`), and statements end at newlines.\n\n\
            Add the expected token shown in the message.",
        "E0003" => "E0003: unexpected token\n\n\
            This token cannot appear in this position. It is usually a typo, a \
            stray operator, or a construct that belongs somewhere else.\n\n\
            Remove or correct it.",
        "E0004" => "E0004: expected end of file\n\n\
            Parsing finished a top-level item but more tokens remain, and they do \
            not start a new declaration. Usually a missing `}` earlier, or an \
            extra token after a declaration.\n\n\
            Balance your braces and remove the stray tokens.",
        "E0005" => "E0005: construct not implemented\n\n\
            This syntax is recognized but not supported by the current compiler. \
            Rewrite using a supported construct; see `docs/language/spec.md`.",

        // ----- resolver (E01xx) -----
        "E0100" => "E0100: duplicate name\n\n\
            Two top-level declarations share a name. Glyph requires one \
            declaration site per name so `grep` finds exactly one definition \
            (greppability).\n\n\
            Rename one of them.",
        "E0101" => "E0101: relative import\n\n\
            Imports must use an absolute module path. Relative paths (`./`, `../`) \
            are not allowed (D15): they make a file's dependencies depend on where \
            it sits, which hurts greppability and refactoring.\n\n\
            Before:  import ./util { helper }\n\
            After:   import myapp/util { helper }",
        "E0102" => "E0102: barrel file\n\n\
            This module contains only imports and no declarations. Glyph imports do \
            not re-export, so such a file does nothing — it is the barrel-file \
            anti-pattern D15 forbids (barrel files scatter a symbol's definition \
            across re-export hops).\n\n\
            Add a real declaration, or delete the file and import from the source \
            module directly.",
        "E0103" => "E0103: unresolved name\n\n\
            A name is used but never declared, imported, or in the prelude. Usually \
            a typo or a missing import.\n\n\
            Declare it, add the import, or fix the spelling.",
        "E0104" => "E0104: unresolved module\n\n\
            An `import` names a module that does not exist in the project or the \
            standard library.\n\n\
            Check the path and that the module is present.",
        "E0105" => "E0105: unknown exported name\n\n\
            The module exists but does not export the name you imported.\n\n\
            Before:  import std/result { Maybe }\n\
            After:   import std/result { Result }   // a name the module exports",

        // ----- typechecker (E02xx) -----
        "E0200" => "E0200: non-exhaustive match\n\n\
            A `match` over a tagged union must handle every variant. Unions are \
            sealed (D9): adding a variant later forces every match to be updated, \
            so a missing variant cannot silently fall through at runtime.\n\n\
            Add an arm for each missing variant, or an `else` arm to catch the \
            rest (which forfeits the exhaustiveness guarantee):\n\n\
            match feed {\n  \
              Loading => ...,\n  \
              Loaded => ...,\n  \
              Failed => ...,   // the missing arm\n\
            }",
        "E0201" => "E0201: `?` outside a Result-returning function\n\n\
            The `?` operator returns the `Err` to the caller, so it is only valid \
            inside a function whose return type is `Result<_, _>`.\n\n\
            Either change the function to return `Result`, or handle the value \
            with `match` instead of `?`.",
        "E0202" => "E0202: `?` on a non-Result\n\n\
            `?` unwraps a `Result<T, E>` to its `T`, propagating `Err`. The operand \
            here is not a `Result`, so there is nothing to unwrap.\n\n\
            Drop the `?`, or make the expression return a `Result`.",
        "E0203" => "E0203: `?` error type mismatch\n\n\
            `?` propagates the operand's error type `E`, which must match the \
            enclosing function's `Result<_, E>` exactly. v1 has no automatic error \
            conversion.\n\n\
            Map the error first so the types line up:\n\n\
            let user = fetch(id).map_err(to_app_error)?",
        "E0204" => "E0204: type mismatch\n\n\
            A value's type does not match the type required at its position (for \
            example, a `return` whose value differs from the declared return \
            type).\n\n\
            Change the value, or the declared type, so the two agree.",
        "E0205" => "E0205: `owned` requires a resource type\n\n\
            The `owned` modifier is the narrow D25 carve-out for resource handles \
            (files, sockets, connections). It is only meaningful on a type marked \
            `resource`.\n\n\
            Drop `owned`, or declare the type `resource type X { ... }`.",
        "E0206" => "E0206: `owned` resource not consumed\n\n\
            An `owned` handle must be consumed exactly once on every path before \
            the function returns — consuming means moving it into an `owned` \
            parameter (for example `close(handle)`). Some path here leaves it \
            open. Note that `?` is an early return: a handle held across a `?` \
            leaks on the Err path.\n\n\
            Consume the handle on every path (including before any `?`).",
        "E0207" => "E0207: `owned` resource used after move\n\n\
            Once an `owned` handle is consumed (moved), it cannot be used again — \
            double-consuming or reading it is an error.\n\n\
            Reorder so every use comes before the single consume.",
        "E0208" => "E0208: non-exhaustive array match\n\n\
            A `match` over an array must cover every length. `[]` covers the empty \
            array, `[a, b]` covers exactly length two, and `[first, ...rest]` \
            covers every length of one or more.\n\n\
            Add an arm for the missing length, a `[first, ...rest]` arm, or a \
            catch-all binding.",
        "E0209" => "E0209: non-exhaustive bool match\n\n\
            Since `match` is the only conditional (D3), a `match` over a `bool` \
            must cover both `true` and `false`, or carry a catch-all.\n\n\
            match ready {\n  \
              true => ...,\n  \
              false => ...,\n\
            }",

        "E0210" => "E0210: no such field\n\n\
            A field access `x.field` where `x`'s type is a record (or named record \
            type) that has no field by that name — usually a typo or a renamed \
            field.\n\n\
            Check the field name, or add the field to the type. Only a value whose \
            type resolves to a concrete record is checked; access on an \
            unknown-typed or non-record value is left alone.",
        "E0211" => "E0211: argument type mismatch\n\n\
            A call argument's type is incompatible with the parameter it is passed \
            to. v1 reports this only when both types are fully known and provably \
            differ (primitives, different named types, a generic over a different \
            base).\n\n\
            Pass a value of the expected type, or change the parameter's type.",

        // ----- emitter (E03xx) -----
        "E0300" => "E0300: construct not supported by the emitter\n\n\
            The program type-checks but uses a construct the v1 TypeScript emitter \
            does not handle yet.\n\n\
            Rewrite using a supported form; see `docs/language/spec.md` for what \
            v1 emits.",

        _ => return None,
    };
    Some(text)
}

/// Every code that `explain` documents, for the catalogue test and tooling.
pub const ALL_CODES: &[&str] = &[
    "E0001", "E0002", "E0003", "E0004", "E0005", "E0100", "E0101", "E0102", "E0103", "E0104",
    "E0105", "E0200", "E0201", "E0202", "E0203", "E0204", "E0205", "E0206", "E0207", "E0208",
    "E0209", "E0210", "E0211", "E0300",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogued_code_has_an_explanation() {
        for code in ALL_CODES {
            assert!(explain(code).is_some(), "missing --explain text for {code}");
            // The body should at least restate the code.
            assert!(explain(code).unwrap().contains(code), "{code} body omits its code");
        }
    }

    #[test]
    fn explain_is_case_insensitive_and_rejects_unknown() {
        assert!(explain("e0200").is_some());
        assert!(explain("E0200").is_some());
        assert!(explain("E9999").is_none());
        assert!(explain("nonsense").is_none());
    }
}
