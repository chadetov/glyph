# Glyph syntactic spec — D1 through D20

Condensed view of the 20 numbered decisions that drive the grammar. Each rule below cites the pillar it serves; the full rationale lives in `archive/SPEC_DECISIONS.md` and the implementation lives in `archive/grammar.js`. The grammar is normative — if it disagrees with this file, the grammar wins, but flag the divergence as a bug.

Principle: **prefer the choice an established language has already validated, unless a Glyph pillar overrides it.** Novelty for its own sake is rejected.

## Lexical

- **D1. Statements end at significant newlines.** A newline terminates a statement only at bracket depth zero (outside `()`, `[]`, `{}`, `<>`). No semicolons, no ASI. The external scanner tracks bracket depth. *[greppability — every declaration starts at column 0]*
- **D12. One string syntax: `"..."`.** Escapes `\n \t \r \" \\ \u{HEX}`. Embedded raw newlines preserved. **Update (session 3): template literals adopted — see D22.**  Originally deferred; the forward-compatibility window was used in session 3. *[abstraction]*
- **D22. Template literals: `"hello ${expr}"`.** `${...}` inside a string literal interpolates an expression. Interior expressions are restricted to literals, identifier reads, member access, `?` postfix, and parenthesized expressions — no statements, no nested string literals, no function declarations. Escape via `\${` for a literal dollar-brace. Replaces the previous "`+` concatenation only" idiom. *[abstraction — `"hello " + name + ", count is " + n` becomes `"hello ${name}, count is ${n}"`]*
- **D13. Numeric literals: integers and decimals.** `/-?\d+(_\d+)*(\.\d+(_\d+)*)?/` with optional `e` exponent. Underscore separators allowed (`1_000_000`). No hex/octal/binary in v1; deferred but forward-compatible. *[baseline]*
- **D14. `//` line comments only.** No block comments (nested-block-comment confusion is a known footgun). No doc-comment syntax yet — `///` is reserved for it, forward-compatible. *[greppability]*
- **D17. Trailing commas legal everywhere they're meaningful.** Array/object literals, parameter lists, argument lists, type field lists, generic parameter lists, match arm lists, import name lists, tuple patterns. *[diff stability]*

## Control flow & expressions

- **D3. `match` is the only conditional.** No `if`/`else` statement or expression at the value level. `match` is an expression in all positions. Cost: `match x > 0 { true => a, false => b }` is verbose. Accepted. The `<if>` JSX directive (D6) is separate. *[abstraction]*
- **D2. `match` arms are comma-separated, trailing comma required.** Holds for both expression arms (`=> Ok(n),`) and block arms (`=> { ... },`). *[diff stability]*
- **D9. `else` is a catch-all arm; `_` is a position-level wildcard.** `else` is a pattern keyword that appears only as the entire pattern of a `match` arm. `_` is a binding wildcard inside a pattern (`Err(_)`, `[..._]`). *[abstraction]*
- **D18. Postfix `?` binds tighter than `.`.** Precedence is normative in `archive/GLYPH.md §2` (originally referenced as `PRECEDENCE.md`, file never materialized). `r.map_err(f)?` parses as `((r.map_err(f))?)`. `await fetch(url)?` parses as `(await fetch(url))?`. `?.` is a separate optional-chaining token; `result?.field` is a syntax error when `result` is `Result`-typed (typechecker's call, not the grammar's). *[locked]*
- **D21. Two loop constructs: `for x in iter { ... }` and `loop { ... }`.** `for` is for bounded iteration over an iterable. `loop` is for unbounded retry/server loops; `break` and `continue` are legal inside `loop`. No `while` — use `loop { match cond { false => break, true => ... } }` or `iter.take_while(...)`. Both keywords sit at column 0 inside their function body, so `grep -n "^\s*for "` and `grep -n "^\s*loop\b"` audit iteration sites. Resolved in brainstorm session 1 from Q20. *[greppability — two keywords, two canonical uses]*

## Declarations

- **D4. One `fn` form, name optional.** `fn name(args) -> T { body }` is the declaration; `fn(args) -> T { body }` is the anonymous form. Return type optional in both. Anonymous forms never sit at column 0, so `grep -n "^fn [a-z_]"` is a complete audit of named declarations. *[greppability]*
- **D19. `component` is its own top-level keyword.** Grammatically identical to `fn` except for the keyword and an implied JSX-returning body. `grep -n "^component "` audits every UI component. *[greppability]*
- **D20. `const` is module-level only; `let` is function-level only.** Mixing them is a syntax error caught at the grammar level. `mut` cannot target a `const` (typechecker enforces). *[greppability]*
- **D15. Three import forms.** `import std/http` (namespace), `import std/result { Result, Ok }` (named), `import std/http as h` (aliased namespace). No `import *`, no default imports, no re-exports, no relative imports. Paths are slash-separated from the module root. *[diff stability]*

## Mutation & types

- **D5. `mut` is a statement prefix restricted to assigns and method calls.** Legal: `mut x = expr`, `mut x[k] = expr`, `mut x.field = expr`, `mut x.method(args)`. `mut foo()` (free function call) is a syntax error. `grep -n "^\s*mut "` is a complete audit of mutation. *[verifiability]*
- **D16. `void` is a reserved word, both type and value.** `Ok(void)` and `-> void` both legal. Cannot be used as an identifier. Avoids TS's `null`/`undefined` ambiguity. *[verifiability]*
- **D25. Narrow `owned` modifier for resource handles.** `let owned name: ResourceType = expr` introduces a value the typechecker tracks for single-consumption across every code path. Forgetting to consume = compile error. Double-consume = compile error. Returning without consuming = compile error. Restricted to types declared with the `resource` keyword (`resource type X = ...`; resolving implementation decision I1 — keyword, consistent with `record`/`component`). A handle is **consumed** by *moving* it into an `owned` parameter (`fn close(owned h: X)`); any other use borrows it (resolving the consume-model question in favor of move-into-`owned`-param over a type-declared disposer method). Because `owned` marks both the binding site (`let owned`) and every consuming position (`owned` params), `grep -n "owned"` is a complete audit of the ownership surface, paralleling D5's `grep "mut"`. **Narrow carve-out** from the manifesto's "no linear types" stance, scoped to resource discipline (files, sockets, db connections, locks). NOT a general affine/linear type system. See `docs/manifesto.md` for the carve-out language. *[verifiability, greppability]*
- **D7. Types vs values disambiguated by context.** Types appear only after `:`, after `->`, inside `<...>`, after `as` (if cast survives), and on the RHS of `type X = ...`. Everywhere else `<` is less-than. GLR handles the few genuinely ambiguous spots (the `fn` keyword preceding `<T>` is the disambiguator). *[TS compatibility]*
- **D8. Tagged union punctuation: leading `|` required on multi-line, omitted on single-line.** `type X = A | B | C` vs `type Y =\n  | A\n  | B({...})\n  | C`. Lexer rule: if the first token after `=` on the type line is `|`, the union is multi-line. *[diff stability]*

## Composite literals

- **D10. No object literal shorthand.** `{ post, comments }` is a syntax error; write `{ post: post, comments: comments }`. Cost: keystrokes. Benefit: `grep -n "post:"` finds every field assignment. *[greppability]*
- **D11. Spread allowed in arrays and objects, position-flexible.** `[...xs, a, b]`, `[a, ...xs, b]`, `[a, b, ...xs]`. Same for objects. Multiple spreads in one literal allowed. *[TS compatibility]*

## JSX

- **D6. JSX is a parallel sub-grammar; directives are regular elements.** JSX is entered after `<` in expression position. Attribute values: string literals or `{expr}`. Children: elements, text runs, `{expr}`. Names `if`, `else`, `for`, `match`, `case` are reserved as element names — they parse as ordinary elements and the typechecker treats them as compiler directives. Positional attributes allowed before named ones (`<case Loaded bind={users}>`). *[abstraction]*

## Annotations (session 3 additions)

- **D27. Annotations are `@<name> <args>` lines above declarations.** Annotations live one-per-line above a `fn`, `type`, `component`, `const`, or `module` declaration. Order is enforced by the formatter (canonical sort). Recognized v1 annotations: `@example` (D23), `@doc` (D26), `@redact` (D24), `@pure` (cf. D9 JSX purity classifier — required to make user fns JSX-callable), `@public`. The grammar accepts any `@<identifier>` form, so adding new annotations is forward-compatible. Unknown annotations are a hard error at compile time (no silent typos). *[abstraction — annotations carry compile-checked metadata that would otherwise be unverified comments]*
- **D23. `@example expr == expr` inline tests above function declarations.** Multiple `@example` lines per function are allowed. The test passes if the LHS evaluates equal to the RHS. The compiler runs every `@example` on `glyph build`; a failure fails the build. Property tests are a stdlib function (`test.property(predicate, generator)`), not a language primitive. *[verifiability — tests are colocated with functions; agents rewriting bodies cannot bypass them]*
- **D24. `@redact fields: [...]` on type declarations enforces PII redaction.** `@redact fields: [diagnosis, notes]` above a `type` declaration causes the runtime to substitute redaction sentinels for those fields whenever the value is logged, serialized, or sent across an stdlib observability boundary. The runtime descriptor (cf. Q8 resolution: descriptors at every type decl) carries the redaction metadata. Type-level enforcement, not convention. *[verifiability]*
- **D26. `@doc """ ... """` blocks with `@run` fences are executable documentation.** Triple-quoted `@doc` blocks contain Markdown. ` ```glyph @run ` fenced blocks inside the doc are compiled and executed on every `glyph build`. Failed `assert` inside a `@run` block fails the build. Same compile-time-execution machinery as D23 `@example`. *[verifiability — docs cannot rot]*

## Pillar attribution summary

| Pillar | Decisions |
|---|---|
| Verifiability | D5, D16, D23, D24, D25, D26 |
| Greppability | D1, D4, D10, D14, D19, D20, D21 |
| Diff stability | D2, D8, D15, D17 |
| Abstraction | D3, D6, D9, D12, D22, D27 |
| TS compatibility (no pillar override) | D7, D11 |
| Baseline / locked | D13, D18 |
