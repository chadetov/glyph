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
        "E0006" => "E0006: Glyph has no `if`/`else`\n\n\
            `match` is the only conditional (D3). One construct for every branch \
            means a reader (or an agent) looking for the decisions in a file \
            greps for one word, and every branch is forced to say which case it \
            handles.\n\n\
            Before:  if (ready) { go() } else { wait() }\n\
            After:   match ready {\n              \
              true => go(),\n              \
              false => wait(),\n            \
            }\n\n\
            A `match` on a `bool` must cover `true` and `false` (E0209), so the \
            `else`-less `if` has no silent counterpart here.",
        "E0007" => "E0007: range pattern in a `match` arm\n\n\
            A range or comparison pattern (`500..599 =>`) is not in v1. The `..` \
            token lexes but carries no meaning in pattern position.\n\n\
            Enumerate the values as separate arms, or match on a boolean you \
            compute first:\n\n\
            let server_error = status >= 500 && status <= 599\n\
            match server_error {\n              \
              true => retry(),\n              \
              false => done(),\n            \
            }",
        "E0008" => "E0008: assignment requires `mut`\n\n\
            Glyph marks every mutation (D5). A bare `x = e` is not an \
            assignment statement; reassigning an existing binding is spelled \
            `mut x = e`, and introducing a new one is `let x = e`.\n\n\
            Before:  total = total + 1\n\
            After:   mut total = total + 1\n\n\
            The mark is what makes mutation greppable: every place a value \
            changes starts with `mut`, so searching for `mut total` finds all \
            of them. The same applies to fields and elements (`mut r.count = \
            1`, `mut xs[0] = v`) and inside a `match` arm (`1 => mut total = \
            5,`).",

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
            Declare it, add the import, or fix the spelling.\n\n\
            One common case: `mut x = e`. Glyph is not `let mut` — `mut` reassigns \
            an *existing* binding, so the name must already be introduced with `let`. \
            If `x` is unresolved here, add a preceding `let`:\n\n\
            Before:  mut total = 0\n\
            After:   let total = 0\n         mut total = total + 5",
        "E0104" => "E0104: unresolved module\n\n\
            An `import` names a module that does not exist in the project or the \
            standard library.\n\n\
            Check the path and that the module is present.",
        "E0105" => "E0105: unknown exported name\n\n\
            The module exists but does not export the name you imported. A name \
            is exported only if it is declared `pub`.\n\n\
            Before:  import std/result { Maybe }\n\
            After:   import std/result { Result }   // a name the module exports\n\n\
            The same check runs on a type written through a namespace import, so \
            `import lib` plus `lib.Secret` reports this too when `Secret` is not \
            `pub`. Which spelling you use does not change what you can see.",
        "E0106" => "E0106: unused import (warning)\n\n\
            An imported name is never referenced in this module, so the import \
            does nothing. A dead import is greppability noise: a reader searching \
            for a name finds an import that promises a dependency the module does \
            not actually have.\n\n\
            Remove it. This is a warning — it does not fail the build.",
        "E0107" => "E0107: unused variable (warning)\n\n\
            A `let` binding is never read. Either it is dead code, or a use was \
            dropped by mistake.\n\n\
            Remove the binding, or — if it is deliberately unused (a destructured \
            field you don't need, say) — prefix its name with `_`:\n\n\
            Before:  let result = compute()   // never read\n\
            After:   let _result = compute()  // intentionally unused\n\n\
            This is a warning — it does not fail the build.",
        "E0108" => "E0108: unreachable code (warning)\n\n\
            A `return`, `break`, or `continue` earlier in the same block always \
            leaves it first, so the statements after it cannot run.\n\n\
            Remove the dead statements, or move the terminator so they can run. \
            This is a warning — it does not fail the build.",
        "E0109" => "E0109: reserved word used as a name\n\n\
            Glyph's keyword set is smaller than TypeScript's, so a word like \
            `class`, `new`, `switch`, `default`, `typeof`, `static`, `eval`, or \
            `arguments` lexes as an ordinary identifier. Glyph compiles to \
            TypeScript, and these words cannot name a `function`, `const`, \
            parameter, or local binding in the emitted code, so Glyph rejects \
            them at the source rather than let `tsc` fail on generated code.\n\n\
            This only affects names in a binding position. Object keys, record \
            fields, and member access are unaffected (`{ default: v }` and \
            `x.new` are fine).\n\n\
            Rename the declaration or binding, e.g. `class` -> `klass`, \
            `new` -> `create`, `switch` -> `select`.",
        "E0110" => "E0110: declaration shadows a global the emitted module uses\n\n\
            Some names are already bound in every module Glyph emits: the \
            JavaScript globals the generated TypeScript refers to (`Error`, \
            `Number`, `Object`, `Array`, `Promise`, `Date`) and the prelude \
            names in scope without an import (`number`, `par`, `print`, \
            `assert`, and the primitive type names `string`, `int`, `bigint`, \
            `bool`, `void`, `unknown`).\n\n\
            Unlike E0109 these are legal TypeScript identifiers, so nothing \
            downstream catches the collision. A tagged union with an `Error` \
            variant emits `export function Error(...)` at module top level, and \
            the `new Error(...)` the compiler emits below it then calls the \
            variant constructor:\n\n\
            type Value =\n  \
              | Num(number)\n  \
              | Error(string)   // E0110\n\n\
            Rename the declaration: `Error` -> `Failure`, `Number` -> `Num`, \
            `number` -> `amount`. Record fields, object keys, and local \
            bindings are unaffected; this checks top-level `fn`, `type`, \
            `const`, `component`, and variant names.",
        "E0111" => "E0111: primitive union is a tagged union of variant names\n\n\
            In Glyph, `A | B` declares a tagged union whose members are variant \
            *constructors* (D8), not a union of two types. So this:\n\n\
            type Key = string | number\n\n\
            declares variants named `string` and `number`, which shadow the \
            prelude names (E0110's problem) and mean something you did not \
            write. Glyph has no primitive-union syntax.\n\n\
            Name each case:\n\n\
            type Key =\n  \
              | Text(string)\n  \
              | Count(number)\n\n\
            then `match` over it. If you need the raw TypeScript union at a \
            boundary, `extern_ts(\"string | number\")` spells it verbatim and \
            leaves the checking to `tsc`.",

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
            unknown-typed or non-record value is left alone. A record declared in \
            a sibling module counts, under every import spelling, and the message \
            names that record's own type.",
        "E0211" => "E0211: argument type mismatch\n\n\
            A call argument's type is incompatible with the parameter it is passed \
            to. v1 reports this only when both types are fully known and provably \
            differ (primitives, different named types, a generic over a different \
            base).\n\n\
            Pass a value of the expected type, or change the parameter's type.",

        "E0212" => "E0212: cannot reassign a `const`\n\n\
            `mut N = ...` targets a module-level `const`, but `const` is immutable \
            (D20). Only a function-level `let` may be reassigned with `mut`.\n\n\
            Move the binding into a function as a `let`, or compute the new value \
            without reassigning the `const`.",
        "E0213" => "E0213: wrong number of arguments\n\n\
            A call passes more or fewer arguments than the function declares. \
            Glyph has no optional parameters, no default values, and no \
            variadics: one argument per parameter, always.\n\n\
            Supply the missing argument, or drop the extra one. If a parameter \
            is genuinely optional, model it in the type — `Option<T>` with an \
            explicit `None` at the call site says so where a defaulted \
            parameter would hide it.",
        "E0214" => "E0214: a component takes one props record\n\n\
            A `component` lowers to a React function component, which is called \
            with a single props object (D19). Declaring several positional \
            parameters would bind the first to the whole props object and leave \
            the rest `undefined` at run time.\n\n\
            Before:  component Row(label: string, count: number) -> Component\n\
            After:   type RowProps = {\n              \
              label: string,\n              \
              count: number,\n            \
            }\n            \
            component Row(props: RowProps) -> Component\n\n\
            A component with no parameters is also fine.",
        "E0215" => "E0215: aliasing an `owned` handle\n\n\
            `let g = h` where `h` is a live `owned` handle creates a second name \
            for one resource, and either name could then consume it — which is \
            exactly what single-consumption (D25) rules out.\n\n\
            Consume the handle directly (move it into an `owned` parameter, e.g. \
            `close(h)`) instead of rebinding it.",
        "E0216" => "E0216: unreachable match arm\n\n\
            An earlier arm is total — a bare binding (`x =>`) or `else` — so it \
            matches every value and no later arm can run. Glyph's `match` is \
            first-match-wins (D9), so the arm below it is dead code.\n\n\
            It is also a soundness guard: a leading binding catch-all lowers to \
            a `switch` `default`, and JavaScript gives every `case` priority \
            over `default` regardless of source order, so the shadowed arm would \
            quietly win at run time.\n\n\
            Move the catch-all below the specific arms, or delete the dead one.",
        "E0217" => "E0217: discarded `Result` (warning)\n\n\
            A `Result`-typed expression is used as a statement and thrown away, \
            so its `Err` case is silently ignored. That is the failure mode \
            errors-as-values exists to prevent.\n\n\
            Before:  write_text(path, body)\n\
            After:   write_text(path, body)?          // propagate\n\
            or:      let _ = write_text(path, body)   // deliberately ignore\n\
            or:      match write_text(path, body) { ... }\n\n\
            This is a warning — it does not fail the build.",
        "E0218" => "E0218: non-exhaustive number/string match\n\n\
            `number` and `string` are unbounded, so a `match` with only literal \
            arms can never cover every value. Since `match` is the only conditional \
            (D3), that gap would become a runtime throw in the emitted `switch` \
            default.\n\n\
            Add an `else` arm (or a bare-identifier binding) to cover the rest:\n\n\
            match n {\n  \
              0 => \"zero\",\n  \
              else => \"other\",\n\
            }",

        "E0219" => "E0219: unknown `@redact` field\n\n\
            A `@redact fields: [...]` annotation (D24) names a field the type does \
            not have — a typo, or a field that was renamed. Redaction is \
            type-level enforcement, so masking a non-existent field is a hard \
            error rather than a silent no-op. Only record types have redactable \
            fields.\n\n\
            @redact fields: [ssn]\n\
            type User = {\n  \
              name: string,\n  \
              ssn: string,   // the name must match a real field\n\
            }",

        "E0220" => "E0220: unknown variant in a `match` arm\n\n\
            A bare `match` arm head is read by shape (the same rule the resolver \
            uses): a lowercase name (`x`, `rest`) is a fresh binding, while a \
            PascalCase name (`Loading`) is a variant reference. A PascalCase head \
            that names no variant of the scrutinee's union is a typo or a \
            wrong-union variant, not a binding.\n\n\
            Left as a binding it would act as a silent catch-all (masking a \
            missing variant and misrouting values at runtime), so Glyph escalates \
            it to an error and suggests the nearest real variant:\n\n\
            type Feed = | Loading | Loaded | Failed\n\
            match f {\n  \
              Loading => 1,\n  \
              Loadign => 2,   // E0220: did you mean `Loading`?\n\
            }\n\n\
            Fix the spelling, or add the variant to the union. A genuinely \
            missing variant is still reported separately as E0200.",

        "E0221" => "E0221: unknown annotation\n\n\
            An `@<name>` the compiler does not recognize (D27). Unknown \
            annotations are rejected rather than ignored: a typo like `@puer` \
            would otherwise sit in the source looking like it meant something.\n\n\
            The recognized set is `@example`, `@doc`, `@redact`, `@open`, \
            `@pure`, and `@public`.\n\n\
            Fix the spelling, or delete the annotation.",
        "E0222" => "E0222: `await` outside an `async fn`\n\n\
            Glyph has no user-visible `Promise`: an `async fn -> T` is awaited \
            to a `T`, and `await` only appears inside one. Written anywhere \
            else it has nothing to suspend.\n\n\
            Before:  fn total() -> number { return await fetch_count() }\n\
            After:   async fn total() -> number { return await fetch_count() }\n\n\
            The innermost enclosing callable decides, so a synchronous lambda \
            inside an `async fn` is its own context and cannot `await` either \
            — write `async fn(x) { ... }` for the lambda, or move the `await` \
            out of it.",
        "E0223" => "E0223: a `match` arm produces no value\n\n\
            The `match` is used as a value (bound by a `let`, assigned with \
            `mut`, returned, or the body's tail in a function with a declared \
            return type), but one arm yields nothing: an empty block, or a \
            block whose last statement is a `let`/`mut`/`for`/`loop`.\n\n\
            Such an arm lowers to `case X: { break; }`, so the value would be \
            `undefined` at run time with no TypeScript error.\n\n\
            Before:  let label = match s {\n              \
              Loading => {},\n              \
              Ready(v) => v,\n            \
            }\n\
            After:   let label = match s {\n              \
              Loading => \"\",\n              \
              Ready(v) => v,\n            \
            }\n\n\
            An arm that ends in `return`, `break`, or `continue` diverges and \
            needs no value. `X => {}` remains a legal no-op where the `match` \
            is a statement rather than a value.",

        // ----- emitter (E03xx) -----
        "E0300" => "E0300: construct not supported by the emitter\n\n\
            The program type-checks but uses a construct the v1 TypeScript emitter \
            does not handle yet.\n\n\
            Rewrite using a supported form; see `docs/language/spec.md` for what \
            v1 emits.",
        "E0301" => "E0301: misplaced `<else>`\n\n\
            An `<else>` is paired with its `<if>` only when it is the \
            immediately following sibling (D6). Anything between them — even a \
            `<p>` — breaks the pairing, and the emitter refuses rather than \
            guess which `<if>` was meant.\n\n\
            Before:  <if cond={ready}>...</if>\n            \
            <p>note</p>\n            \
            <else>...</else>\n\
            After:   <if cond={ready}>...</if>\n            \
            <else>...</else>\n            \
            <p>note</p>",
        "E0302" => "E0302: `?` in a match nested inside a larger expression\n\n\
            A `match` that is the whole value of a `let`, `mut`, or `return` \
            lowers to a `switch` statement, where `?` works: it returns the \
            `Err` from the enclosing function. A `match` sitting inside a \
            larger expression (a call argument, an operand) lowers to an \
            immediately-invoked arrow instead, and an arrow cannot return from \
            the function around it.\n\n\
            Before:  return Ok(shout(match id {\n              \
              None => load(b)?,\n              \
              Some(n) => name(n),\n            \
            }))\n\
            After:   let s = match id {\n              \
              None => load(b)?,\n              \
              Some(n) => name(n),\n            \
            }\n            \
            return Ok(shout(s))\n\n\
            Binding the match first also reads better: the propagation point is \
            a statement, not buried in an argument list.",
        "E0303" => "E0303: `?` cannot be used in this position\n\n\
            `?` expands to two statements placed before the one it appears in: \
            a `const` holding the `Result`, and an early `return` of the `Err`. \
            It is therefore only legal where a statement can be inserted ahead \
            of it. A `match` scrutinee is emitted as a plain expression, with no \
            such slot.\n\n\
            Before:  let x = match load(p)? {\n              \
              None => \"none\",\n              \
              Some(v) => v,\n            \
            }\n\
            After:   let id = load(p)?\n            \
            let x = match id {\n              \
              None => \"none\",\n              \
              Some(v) => v,\n            \
            }\n\n\
            This is a positional rule, not a missing feature: `?` works in a \
            `let`, a `return`, an argument, a template, and a `match` arm body. \
            A `?` inside an arm of a `match` that is nested in a larger \
            expression is E0302, a different rule with a different fix.",
        "E0310" => "E0310: no `main` to run\n\n\
            `glyph run` executes a program's entry point, `main(argv)`. The module \
            you pointed it at compiles fine but is a library — it exports functions \
            and types, but has no `fn main`, so there is nothing to execute.\n\n\
            Either add an entry point:\n\n\
            fn main(argv: Array<string>) -> number {\n  \
              // ...\n  \
              return 0\n\
            }\n\n\
            or, if it is meant to be a library, build it with `glyph build` (which \
            emits the TypeScript) and import it from a module that does have a \
            `main`.",

        _ => return None,
    };
    Some(text)
}

/// Every code that `explain` documents, for the catalogue test and tooling.
/// Kept in step with the table in `docs/error-codes.md`, which the test below
/// reads: a code in one and not the other fails the build.
pub const ALL_CODES: &[&str] = &[
    "E0001", "E0002", "E0003", "E0004", "E0005", "E0006", "E0007", "E0008", "E0100", "E0101",
    "E0102", "E0103", "E0104",
    "E0105", "E0106", "E0107", "E0108", "E0109", "E0110", "E0111", "E0200", "E0201", "E0202",
    "E0203", "E0204",
    "E0205",
    "E0206", "E0207", "E0208",
    "E0209", "E0210", "E0211", "E0212", "E0213", "E0214", "E0215", "E0216", "E0217", "E0218",
    "E0219", "E0220", "E0221", "E0222", "E0223", "E0300", "E0301",
    "E0302", "E0303", "E0310",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalogue every diagnostic's footer points a reader at. Read here so
    /// the test below can compare the two directions; `ALL_CODES` alone only
    /// proved that codes we already knew about had text, which let three codes
    /// ship in the catalogue with `--explain` answering "no documentation".
    const CATALOGUE: &str = include_str!("../../../../docs/error-codes.md");

    /// Every `E0nnn` in a table row of `docs/error-codes.md`. The phase-range
    /// table at the top of the file names ranges (`E000x`, `E01xx`), not codes,
    /// so a row's first cell only counts when it is all digits after the `E`.
    fn catalogued_codes() -> Vec<String> {
        CATALOGUE
            .lines()
            .filter(|l| l.starts_with("| `E"))
            .filter_map(|l| l.split('`').nth(1))
            .filter(|c| c.len() == 5 && c[1..].chars().all(|ch| ch.is_ascii_digit()))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn every_catalogued_code_has_an_explanation() {
        for code in ALL_CODES {
            assert!(explain(code).is_some(), "missing --explain text for {code}");
            // The body should at least restate the code.
            assert!(explain(code).unwrap().contains(code), "{code} body omits its code");
        }
    }

    #[test]
    fn the_doc_catalogue_and_explain_agree() {
        let documented = catalogued_codes();
        assert!(
            documented.len() > 30,
            "docs/error-codes.md parsed as {} rows; the table format changed",
            documented.len()
        );
        for code in &documented {
            assert!(
                explain(code).is_some(),
                "{code} is in docs/error-codes.md but `glyph --explain {code}` says there is no documentation"
            );
        }
        for code in ALL_CODES {
            assert!(
                documented.iter().any(|d| d == code),
                "{code} has --explain text but no row in docs/error-codes.md"
            );
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
