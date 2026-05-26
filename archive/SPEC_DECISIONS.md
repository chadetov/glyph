# Glyph Grammar — Spec Decisions

Every decision the example corpus left ambiguous, resolved with the rationale
the Glyph manifesto's four pillars (abstraction, verifiability, diff stability,
greppability) make defensible.

Decisions follow the principle: **prefer the choice an established language has
already validated, unless a Glyph pillar overrides it.** Novelty for its own
sake is rejected.

---

## D1. Statements terminate at significant newlines

**Decision.** A statement ends at a newline that is *not* inside `()`, `[]`,
`{}`, or `<>`. No semicolons. No semicolon insertion. The lexer tracks bracket
depth; tokens inside brackets do not produce statement-ending newlines.

**Rationale.** Identical to Go and Python. No ASI ambiguity (JavaScript's
mistake), no semicolon noise (TypeScript's daily friction). Bracket-depth
tracking is mechanical and has zero corner cases in 50 years of practice.

**Greppability.** Every statement starts at column 0 of its own line (or column
N inside a block). `grep -n "^fn "` finds every top-level function.

---

## D2. `match` arms are comma-separated, trailing comma required

**Decision.** Inside `match { ... }`, every arm ends with `,`, including the
last. This holds whether the arm body is an expression (`=> Ok(n),`) or a block
(`=> { ... },`).

**Rationale.** The example corpus mixes both styles. Picking "always comma"
makes the grammar one rule instead of two and makes diffs minimal when an arm
is added at the end — the existing last arm doesn't change. This is the Rust
choice, for the same reason.

**Diff stability.** Adding an arm at the end is a one-line addition. Without a
trailing comma, it's a two-line diff (the previous last arm gains a comma).

**Note.** File 04's `Help => { ... }` arm appears in the corpus *without* a
trailing comma. Under this rule that's a syntax error; the example will be
corrected when we produce the canonical formatter. The grammar is the spec.

---

## D3. `match` is an expression, `if` does not exist as a control form

**Decision.** `match` is the only conditional. It is an expression in all
positions: RHS of `=`, inside `return`, as a statement, as an arm body. There
is no `if`/`else` statement or expression at the value level. The `<if>` JSX
directive is a separate template construct (see D6).

**Rationale.** The corpus uses `match` for everything, including binary
conditions (`match n > 0 { true => ..., false => ... }`). Keeping one
construct is the abstraction pillar applied literally: fewer ways to do the
same thing. Rust gradually moved this direction; Glyph starts there.

**Cost.** `match x > 0 { true => a, false => b }` is more verbose than
`if x > 0 { a } else { b }`. Accepted. The win is one syntactic form for
"branch on a value," exhaustive by default.

---

## D4. Function expressions: same syntax as declarations, name optional

**Decision.** `fn(args) -> T { body }` is the anonymous form. `fn name(args)
-> T { body }` is the declaration form. Return type is optional in both. The
two forms share one grammar rule with an optional name.

**Rationale.** One syntactic shape, two contexts. Greppability: `grep -n "^fn
[a-z_]"` still finds every named declaration unambiguously because anonymous
forms don't sit at column 0.

---

## D5. `mut` is a statement prefix; legal targets are assignments and method calls

**Decision.** `mut` prefixes exactly one statement. The statement must be one
of:
- An assignment: `mut x = expr`, `mut x[k] = expr`, `mut x.field = expr`
- A method call: `mut x.method(args)`

`mut foo()` (calling a free function) is a **syntax error**, not just bad
style. The grammar rejects it.

**Rationale.** The corpus only uses `mut` in these two shapes. Restricting at
the grammar level means `grep -n "^\s*mut "` is a complete audit of all
mutation in a file. This is the verifiability pillar: the syntactic form makes
the claim "this line mutates state" checkable by the eye, not just by the
typechecker.

**Why not allow `mut` on any expression statement?** Because the typechecker
would then have to be the gate on what's actually a mutation, and an agent
reading code can't trust the syntactic form. Better to make the grammar
narrow.

---

## D6. JSX is a parallel sub-grammar; directives are regular elements

**Decision.** JSX exists as a sub-grammar entered after `<` in expression
position. Inside JSX:
- Attribute values are either string literals or `{expr}`.
- Children are JSX elements, text runs, or `{expr}`.
- The names `if`, `else`, `for`, `match`, `case` are *reserved* as element
  names — they parse as regular elements but the typechecker treats them as
  compiler directives.
- Directive-specific attributes (`cond`, `value`, `in`, `key`, `bind`) are
  attributes like any other syntactically.

**Rationale.** Treating directives as ordinary elements at the grammar level
keeps the JSX sub-grammar small and uniform. The semantic distinction is the
typechecker's job. This is how every JSX-flavored language handles its own
extensions (Svelte, Solid, Vue templates).

**`<case Loaded bind={users}>`.** The element name is `case`. `Loaded` is the
first positional attribute (no name, no value). `bind={users}` is a named
attribute. Grammar allows positional attributes before named ones, like HTML's
boolean attributes generalized.

---

## D7. Types and values disambiguated by context; `<` is overloaded

**Decision.** Type expressions appear only in these contexts: after `:` in
annotations, after `->` in return types, inside `<...>` in generic parameters
or arguments, after `as` in casts (if cast survives), and on the RHS of
`type X = ...`. Everywhere else, `<` is the less-than operator.

**Rationale.** TypeScript solved this. The grammar uses GLR (tree-sitter's
default) to handle the few genuinely ambiguous spots (`fn foo<T>(x: T)` —
generic parameter vs. comparison — resolved by the `fn` keyword preceding it).

**Cost.** A type can never be a first-class runtime value via the same syntax;
`Schema<T>.schema` reflection uses a separate descriptor (the verifiability
pillar). Acceptable.

---

## D8. Tagged unions: leading `|` required on multi-line, omitted on single-line

**Decision.** Two forms:
```
type X = A | B | C
type Y =
  | A
  | B({ field: string })
  | C
```
The multi-line form requires a leading `|` on the first variant. Single-line
form forbids it. The lexer-level rule: if the first token after `=` on the
type line is `|`, the union is multi-line; otherwise it's single-line.

**Rationale.** The corpus consistently uses leading `|` for multi-line and
omits it for single-line. Codifying this in the grammar means the formatter
has exactly one canonical form per case — no fiddly "wrap if longer than 80
chars" rule. Diff stability: adding a variant to a multi-line union is a
one-line diff.

---

## D9. `else` is the catch-all in `match`; `_` is a value-position wildcard

**Decision.** Two distinct wildcards with one rule:
- `else` is the catch-all *arm* in `match`. It is a pattern keyword. It appears
  only as the entire pattern of a `match` arm.
- `_` is a *binding* wildcard inside a pattern. It says "match anything here,
  bind nothing." Examples: `Err(_)` (match Err with any payload, bind nothing),
  `["help", ..._]` (rest pattern that discards).

**Rationale.** The corpus uses both. They serve different roles: `else` is an
arm-level escape hatch, `_` is a position-level discard. Conflating them would
require complex disambiguation. Rust does the same (`_` for binding, no
`else` because `_` already covers the catch-all arm case; Glyph adds `else`
because it reads more naturally for whole-arm catch-alls and the corpus
already uses it).

---

## D10. No object literal shorthand

**Decision.** `{ post, comments }` is a syntax error. You must write
`{ post: post, comments: comments }`.

**Rationale.** The corpus never uses shorthand. Greppability pillar:
`grep -n "post:"` finds every site where the `post` field is being set, with
no false negatives from shorthand. The cost is keystrokes; the benefit is that
field assignments are always visible at their use site.

---

## D11. Spread is allowed in arrays and objects, position-flexible

**Decision.** `[...xs, a, b]`, `[a, ...xs, b]`, `[a, b, ...xs]` all legal.
Same for objects: `{ ...obj, field: value }` and `{ field: value, ...obj }`.
Multiple spreads in one literal allowed.

**Rationale.** Modern JS/TS allows this. The corpus uses it. No reason to
restrict.

---

## D12. One string syntax: `"..."` with standard escapes, embedded newlines allowed

**Decision.** Strings are double-quoted. Escapes: `\n`, `\t`, `\r`, `\"`,
`\\`, `\u{HEX}`. Raw newlines inside the literal are preserved verbatim
(see file 04's `help_text`). No template literals. No interpolation syntax.

**Rationale.** Interpolation is a feature the corpus doesn't use; it can be
added later (as `"hello ${name}"`) without breaking the grammar — the
unescaped `${` is currently illegal, so the addition is forward-compatible.
String concatenation with `+` is the current idiom. Acceptable for v1.

---

## D13. Numeric literals: integers and decimals, underscore separators allowed

**Decision.** Numbers match `/-?\d+(_\d+)*(\.\d+(_\d+)*)?/`. Optional `e`
exponent: `1.5e10`. No hex/octal/binary literals in v1.

**Rationale.** The corpus uses only integers, but a language that can't write
`3.14` is not credible. Underscore separators (`1_000_000`) are universally
accepted (Java, Python, Rust, Swift, modern JS). Hex/octal/binary deferred —
trivial to add later, forward-compatible.

---

## D14. Comments: `//` only

**Decision.** Line comments with `//`. No block comments. No doc comments
syntax (yet — doc comments will be `///` when added, forward-compatible).

**Rationale.** The corpus uses only `//`. Block comments are a well-known
source of nested-comment confusion and diff churn (a block comment opening on
line 10 changes the meaning of every line until its close). One comment form
is the greppability pillar applied to comments themselves.

---

## D15. Imports: `import path` and `import path { names }` and `import path as alias`

**Decision.** Three forms:
```
import std/http                    // namespace import; refer as `http.foo`
import std/result { Result, Ok }   // named import; refer as `Result`
import std/http as h               // aliased namespace import
```
No `import * as`, no default imports, no re-exports. Paths are
slash-separated, always full paths from the module root. No relative imports
(`./foo` is illegal).

**Rationale.** Three forms cover every legitimate use. The corpus shows the
first two; the third (`as`) is included because aliasing is occasionally
necessary for name collisions and adding it later would silently change parse
trees of existing files. No barrel files, no re-exports — the manifesto
commits to this for diff stability.

---

## D16. `void` is a reserved word, both type and value

**Decision.** `void` is a keyword. As a type it denotes the unit type. As a
value it denotes the unit value. `Ok(void)` and `-> void` both legal. You
cannot name a variable `void`.

**Rationale.** Matches the corpus. Reserving it is mandatory; allowing it as
an identifier would create the same `null`/`undefined` ambiguity TypeScript
fights with.

---

## D17. Trailing commas allowed everywhere they're meaningful

**Decision.** Trailing commas legal in: array literals, object literals,
function parameter lists, function argument lists, type field lists, generic
parameter lists, `match` arm lists, import name lists, tuple/positional
patterns.

**Rationale.** Universal modern practice. Diff stability: appending an item
doesn't modify the previous line.

---

## D18. Postfix `?` binds tighter than `.`; precedence table is PRECEDENCE.md

**Decision.** The precedence table in `PRECEDENCE.md` is normative. The
grammar encodes those 12 levels exactly. Specifically:
- `r.map_err(f)?` parses as `((r.map_err(f))?)`
- `await fetch(url)?` parses as `(await fetch(url))?`
- `?.` is a single token for optional chaining; `result?.field` is a syntax
  error when `result` is `Result`-typed (caught later by the typechecker).

**Rationale.** Already documented and locked in step 2.

---

## D19. `component` is a separate top-level declaration form

**Decision.** `component Name(props: T) -> Component { body }`. Grammatically
identical to `fn` except the keyword and the implied JSX-returning body.
Return type is optional and defaults to `Component`.

**Rationale.** Keeping `component` as its own keyword (rather than `fn` with a
decorator or naming convention) makes `grep -n "^component "` an exhaustive
audit of all UI components in a codebase. Greppability pillar.

---

## D20. `const` is module-level only; `let` is function-level only

**Decision.** `const X = expr` legal only at module scope. `let x = expr`
legal only inside function bodies (including `component` and anonymous `fn`
bodies). Mixing them is a syntax error caught by the grammar (via separate
rules in different scopes).

**Rationale.** Two roles, two keywords, no overlap. `grep -n "^const "` finds
every module-level constant. `grep -n "let "` (or with whitespace prefix)
finds every local. Greppability pillar applied to bindings.

**`const` is also implicitly immutable.** `mut` cannot target a `const`.
The grammar lets the typechecker enforce this.

---

## Summary table

| #   | Decision                          | Pillar                |
| --- | --------------------------------- | --------------------- |
| D1  | Significant newlines              | greppability          |
| D2  | Trailing commas in match arms     | diff stability        |
| D3  | `match` only, no `if`             | abstraction           |
| D4  | One `fn` form, name optional      | abstraction           |
| D5  | `mut` only on assigns/method calls| verifiability         |
| D6  | JSX directives are elements       | abstraction           |
| D7  | Types vs values by context        | (TS compatibility)    |
| D8  | Tagged union punctuation          | diff stability        |
| D9  | `else` (arm) vs `_` (position)    | abstraction           |
| D10 | No object shorthand               | greppability          |
| D11 | Flexible spread                   | (TS compatibility)    |
| D12 | One string syntax                 | abstraction           |
| D13 | Integers and decimals             | (baseline)            |
| D14 | `//` only                         | greppability          |
| D15 | Three import forms                | diff stability        |
| D16 | `void` reserved                   | verifiability         |
| D17 | Trailing commas everywhere        | diff stability        |
| D18 | Precedence per PRECEDENCE.md      | (locked)              |
| D19 | `component` separate keyword      | greppability          |
| D20 | `const` module, `let` local       | greppability          |
