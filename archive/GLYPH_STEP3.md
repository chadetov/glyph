# Glyph — Step 3: Tree-Sitter Grammar & Spec

> **Step 3 of the Glyph delivery plan.** Tree-sitter first, not a hand-written
> parser. The grammar file is the syntactic spec. If tree-sitter can't parse
> your syntax cleanly, your syntax is too clever.

This document consolidates the complete step-3 deliverable into one file:

1. [Overview](#1-overview)
2. [Spec decisions](#2-spec-decisions) — every contested syntax choice with rationale
3. [Operator precedence](#3-operator-precedence) — normative, locked in step 2
4. [Project layout](#4-project-layout)
5. [`grammar.js`](#5-grammarjs) — the tree-sitter grammar
6. [`scanner.c`](#6-scannerc) — external scanner for context-sensitive tokens
7. [Build files](#7-build-files) — `package.json`, `binding.gyp`, `.gitignore`
8. [Editor support](#8-editor-support) — syntax highlighting queries
9. [How to use it](#9-how-to-use-it)
10. [Known conflicts and next steps](#10-known-conflicts-and-next-steps)

---

## 1. Overview

Glyph is a TypeScript-family language designed for AI agents to read, write,
and modify safely. Four pillars: **abstraction**, **verifiability**, **diff
stability**, **greppability**.

This step turns the four-file example corpus locked in step 2 into a formal
syntactic spec via a tree-sitter grammar. The deliverables:

| Artifact | Purpose |
|----------|---------|
| `SPEC_DECISIONS.md` | 20 contested decisions, each defended by a pillar or by "established practice." |
| `grammar.js` | 84-rule tree-sitter grammar. Encodes the precedence table from `PRECEDENCE.md` exactly. |
| `src/scanner.c` | External scanner for significant newlines, JSX text runs, and string bodies. |
| `queries/highlights.scm` | Tree-sitter syntax-highlighting queries — instant editor support. |
| `examples/*.glyph` | The four hard-case files from step 2, copied in as the test corpus. |

The grammar passes structural validation (loads as JS, every rule evaluable
without crashes). The scanner compiles cleanly under `gcc -std=c99 -Wall
-Werror`. Final verification — running `tree-sitter generate` and parsing the
four example files — happens locally; the tree-sitter CLI binary wasn't
reachable in the build environment.

---

## 2. Spec decisions

Every decision the example corpus left ambiguous, resolved with rationale that
the four pillars make defensible. Principle: **prefer the choice an established
language has already validated, unless a Glyph pillar overrides it.** Novelty
for its own sake is rejected.

### D1. Statements terminate at significant newlines

**Decision.** A statement ends at a newline that is *not* inside `()`, `[]`,
`{}`, or `<>`. No semicolons. No semicolon insertion. The lexer tracks bracket
depth; tokens inside brackets do not produce statement-ending newlines.

**Rationale.** Identical to Go and Python. No ASI ambiguity (JavaScript's
mistake), no semicolon noise (TypeScript's daily friction). Bracket-depth
tracking is mechanical and has zero corner cases in 50 years of practice.

**Greppability.** Every statement starts at column 0 of its own line (or
column N inside a block). `grep -n "^fn "` finds every top-level function.

### D2. `match` arms are comma-separated, trailing comma required

**Decision.** Inside `match { ... }`, every arm ends with `,`, including the
last. This holds whether the arm body is an expression (`=> Ok(n),`) or a
block (`=> { ... },`).

**Rationale.** The example corpus mixes both styles. Picking "always comma"
makes the grammar one rule instead of two and makes diffs minimal when an arm
is added at the end — the existing last arm doesn't change. This is the Rust
choice, for the same reason.

**Diff stability.** Adding an arm at the end is a one-line addition. Without a
trailing comma, it's a two-line diff (the previous last arm gains a comma).

**Note.** File 04's `Help => { ... }` arm appears in the corpus *without* a
trailing comma. Under this rule that's a syntax error; the example will be
corrected when we produce the canonical formatter. The grammar is the spec.

### D3. `match` is an expression, `if` does not exist as a control form

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

### D4. Function expressions: same syntax as declarations, name optional

**Decision.** `fn(args) -> T { body }` is the anonymous form. `fn name(args)
-> T { body }` is the declaration form. Return type is optional in both. The
two forms share one grammar rule with an optional name.

**Rationale.** One syntactic shape, two contexts. Greppability: `grep -n "^fn
[a-z_]"` still finds every named declaration unambiguously because anonymous
forms don't sit at column 0.

### D5. `mut` is a statement prefix; legal targets are assignments and method calls

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

### D6. JSX is a parallel sub-grammar; directives are regular elements

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

### D7. Types and values disambiguated by context; `<` is overloaded

**Decision.** Type expressions appear only in these contexts: after `:` in
annotations, after `->` in return types, inside `<...>` in generic parameters
or arguments, after `as` in casts (if cast survives), and on the RHS of `type
X = ...`. Everywhere else, `<` is the less-than operator.

**Rationale.** TypeScript solved this. The grammar uses GLR (tree-sitter's
default) to handle the few genuinely ambiguous spots (`fn foo<T>(x: T)` —
generic parameter vs. comparison — resolved by the `fn` keyword preceding
it).

**Cost.** A type can never be a first-class runtime value via the same
syntax; `Schema<T>.schema` reflection uses a separate descriptor (the
verifiability pillar). Acceptable.

### D8. Tagged unions: leading `|` required on multi-line, omitted on single-line

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

### D9. `else` is the catch-all in `match`; `_` is a value-position wildcard

**Decision.** Two distinct wildcards with one rule:
- `else` is the catch-all *arm* in `match`. It is a pattern keyword. It
  appears only as the entire pattern of a `match` arm.
- `_` is a *binding* wildcard inside a pattern. It says "match anything here,
  bind nothing." Examples: `Err(_)` (match Err with any payload, bind
  nothing), `["help", ..._]` (rest pattern that discards).

**Rationale.** The corpus uses both. They serve different roles: `else` is an
arm-level escape hatch, `_` is a position-level discard. Conflating them
would require complex disambiguation. Rust does the same (`_` for binding, no
`else` because `_` already covers the catch-all arm case; Glyph adds `else`
because it reads more naturally for whole-arm catch-alls and the corpus
already uses it).

### D10. No object literal shorthand

**Decision.** `{ post, comments }` is a syntax error. You must write
`{ post: post, comments: comments }`.

**Rationale.** The corpus never uses shorthand. Greppability pillar: `grep -n
"post:"` finds every site where the `post` field is being set, with no false
negatives from shorthand. The cost is keystrokes; the benefit is that field
assignments are always visible at their use site.

**Asymmetry with patterns.** In *pattern* position, shorthand IS allowed
(`{ status }` binds `status`). In patterns the field name and the binding
name are typically the same and forcing `{ status: status }` would be noise.
This is deliberate.

### D11. Spread is allowed in arrays and objects, position-flexible

**Decision.** `[...xs, a, b]`, `[a, ...xs, b]`, `[a, b, ...xs]` all legal.
Same for objects: `{ ...obj, field: value }` and `{ field: value, ...obj }`.
Multiple spreads in one literal allowed.

**Rationale.** Modern JS/TS allows this. The corpus uses it. No reason to
restrict.

### D12. One string syntax: `"..."` with standard escapes, embedded newlines allowed

**Decision.** Strings are double-quoted. Escapes: `\n`, `\t`, `\r`, `\"`,
`\\`, `\u{HEX}`. Raw newlines inside the literal are preserved verbatim (see
file 04's `help_text`). No template literals. No interpolation syntax.

**Rationale.** Interpolation is a feature the corpus doesn't use; it can be
added later (as `"hello ${name}"`) without breaking the grammar — the
unescaped `${` is currently illegal, so the addition is forward-compatible.
String concatenation with `+` is the current idiom. Acceptable for v1.

### D13. Numeric literals: integers and decimals, underscore separators allowed

**Decision.** Numbers match `/-?\d+(_\d+)*(\.\d+(_\d+)*)?/`. Optional `e`
exponent: `1.5e10`. No hex/octal/binary literals in v1.

**Rationale.** The corpus uses only integers, but a language that can't write
`3.14` is not credible. Underscore separators (`1_000_000`) are universally
accepted (Java, Python, Rust, Swift, modern JS). Hex/octal/binary deferred —
trivial to add later, forward-compatible.

### D14. Comments: `//` only

**Decision.** Line comments with `//`. No block comments. No doc comments
syntax (yet — doc comments will be `///` when added, forward-compatible).

**Rationale.** The corpus uses only `//`. Block comments are a well-known
source of nested-comment confusion and diff churn (a block comment opening on
line 10 changes the meaning of every line until its close). One comment form
is the greppability pillar applied to comments themselves.

### D15. Imports: three forms, no relative paths

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

### D16. `void` is a reserved word, both type and value

**Decision.** `void` is a keyword. As a type it denotes the unit type. As a
value it denotes the unit value. `Ok(void)` and `-> void` both legal. You
cannot name a variable `void`.

**Rationale.** Matches the corpus. Reserving it is mandatory; allowing it as
an identifier would create the same `null`/`undefined` ambiguity TypeScript
fights with.

### D17. Trailing commas allowed everywhere they're meaningful

**Decision.** Trailing commas legal in: array literals, object literals,
function parameter lists, function argument lists, type field lists, generic
parameter lists, `match` arm lists, import name lists, tuple/positional
patterns.

**Rationale.** Universal modern practice. Diff stability: appending an item
doesn't modify the previous line.

### D18. Postfix `?` binds tighter than `.`; precedence table is `PRECEDENCE.md`

**Decision.** The precedence table in `PRECEDENCE.md` (reproduced in §3) is
normative. The grammar encodes those 12 levels exactly. Specifically:

- `r.map_err(f)?` parses as `((r.map_err(f))?)`
- `await fetch(url)?` parses as `(await fetch(url))?`
- `?.` is a single token for optional chaining; `result?.field` is a syntax
  error when `result` is `Result`-typed (caught later by the typechecker).

**Rationale.** Already documented and locked in step 2.

### D19. `component` is a separate top-level declaration form

**Decision.** `component Name(props: T) -> Component { body }`. Grammatically
identical to `fn` except the keyword and the implied JSX-returning body.
Return type is optional and defaults to `Component`.

**Rationale.** Keeping `component` as its own keyword (rather than `fn` with
a decorator or naming convention) makes `grep -n "^component "` an exhaustive
audit of all UI components in a codebase. Greppability pillar.

### D20. `const` is module-level only; `let` is function-level only

**Decision.** `const X = expr` legal only at module scope. `let x = expr`
legal only inside function bodies (including `component` and anonymous `fn`
bodies). Mixing them is a syntax error caught by the grammar (via separate
rules in different scopes).

**Rationale.** Two roles, two keywords, no overlap. `grep -n "^const "` finds
every module-level constant. `grep -n "let "` (or with whitespace prefix)
finds every local. Greppability pillar applied to bindings.

**`const` is also implicitly immutable.** `mut` cannot target a `const`. The
grammar lets the typechecker enforce this.

### Summary table

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

---

## 3. Operator precedence

Reproduced from `PRECEDENCE.md`. Normative. The grammar encodes these levels
exactly.

Highest (tightest binding) at the top.

| Level | Operators                          | Associativity |
|-------|------------------------------------|---------------|
| 1     | `.` `?.` `[]` `()` (call/index)    | left          |
| 2     | postfix `?` (Result propagation)   | left          |
| 3     | prefix `!` `-`                     | right         |
| 4     | `*` `/` `%`                        | left          |
| 5     | `+` `-`                            | left          |
| 6     | `<` `<=` `>` `>=`                  | left          |
| 7     | `==` `!=`                          | left          |
| 8     | `&&`                               | left          |
| 9     | `\|\|`                             | left          |
| 10    | `??`                               | right         |
| 11    | `await`                            | prefix        |
| 12    | `=` (assignment, only with `mut`)  | right         |

### Critical rules

**`await` binds looser than `?`.** `await fetch(url)?` parses as `(await
fetch(url))?`. Rationale: you await a `Promise<Result<T, E>>`, get a
`Result<T, E>`, then propagate.

**Postfix `?` binds tighter than member access.** `result?.field` is illegal
— `?` is Result propagation, not optional chaining. Optional chaining is
`?.` (a single token).

**Method chains bind left.** `r.map_err(f).and_then(g)?` parses as
`((r.map_err(f)).and_then(g))?`.

**`await` is a prefix operator, not a keyword statement.** `let x = await f()
+ await g()` is legal and means `(await f()) + (await g())`.

**Assignment is statement-level only.** `mut x = 5` is a statement. There are
no assignment expressions. No `if (x = foo())` foot-guns.

---

## 4. Project layout

```
tree-sitter-glyph/
├── grammar.js                 Main grammar definition (§5)
├── src/
│   └── scanner.c              External scanner (§6)
├── binding.gyp                Native build config (§7)
├── package.json               Node + tree-sitter metadata (§7)
├── .gitignore                 (§7)
├── queries/
│   └── highlights.scm         Syntax highlighting (§8)
├── examples/
│   ├── 01_validator.glyph     Test corpus from step 2
│   ├── 02_async_errors.glyph
│   ├── 03_react_component.glyph
│   └── 04_cli_tool.glyph
├── SPEC_DECISIONS.md          (§2 — see above)
├── PRECEDENCE.md              (§3 — see above)
├── MANIFESTO.md               Glyph's four pillars
└── README.md                  Quick-start
```

---

## 5. `grammar.js`

The full tree-sitter grammar. 84 rules. Validated structurally: loads as JS,
every rule evaluable.

```javascript
/**
 * Tree-sitter grammar for Glyph
 *
 * Grammar is the spec. Every rule below corresponds to a decision in
 * SPEC_DECISIONS.md (referenced as [Dn]) or PRECEDENCE.md.
 *
 * To regenerate the parser:
 *   npm install
 *   npx tree-sitter generate
 *   npx tree-sitter parse examples/01_validator.glyph
 *
 * To test against the corpus:
 *   npx tree-sitter parse examples/*.glyph
 */

// Precedence levels match PRECEDENCE.md exactly.
// Higher number = tighter binding in tree-sitter's `prec`.
const PREC = {
  // Level 12 (loosest in PRECEDENCE.md) is assignment — statement-level only,
  // so it doesn't participate in the expression precedence chain.
  AWAIT:        1,   // PRECEDENCE.md level 11 (prefix await)
  NULLISH:      2,   // level 10  ??       right
  LOGICAL_OR:   3,   // level 9   ||       left
  LOGICAL_AND:  4,   // level 8   &&       left
  EQUALITY:     5,   // level 7   == !=    left
  COMPARISON:   6,   // level 6   < <= > >=  left
  ADDITIVE:     7,   // level 5   + -      left
  MULTIPLICATIVE: 8, // level 4   * / %    left
  PREFIX_UNARY: 9,   // level 3   ! -      right
  POSTFIX_TRY:  10,  // level 2   ?        postfix; binds tighter than member
  MEMBER:       11,  // level 1   . ?. [] ()  left
  // Used to bias type-vs-value disambiguation; not in PRECEDENCE.md.
  TYPE_GENERIC: 20,
};

module.exports = grammar({
  name: 'glyph',

  extras: $ => [
    /[ \t]/,           // whitespace, but NOT newlines (D1: newlines are significant)
    $.line_comment,
  ],

  // Words: identifiers that might also be keywords are routed through here so
  // the lexer doesn't tokenize `match` as an identifier and confuse the parser.
  word: $ => $.identifier,

  // Conflicts the GLR engine needs to resolve dynamically.
  // Each entry says "these productions look alike for a few tokens; pick whichever
  // ends up valid."
  conflicts: $ => [
    // `Name<T>(...)` — generic call vs (Name < T) > (...)
    [$.call_expression, $.binary_expression],
    // `fn name<T>(...)` declaration vs `fn(...)` expression at parse start
    [$._function_declaration_head, $._function_expression_head],
    // Pattern `Name({ x })` vs expression `Name({ x })` inside match arms
    [$.constructor_pattern, $.call_expression],
    [$.object_pattern, $.object_literal],
    [$.array_pattern, $.array_literal],
    // Identifier-as-pattern vs identifier-as-expression in match arms
    [$.identifier_pattern, $.identifier_expression],
  ],

  externals: $ => [
    $._newline,            // significant newline (D1: outside brackets only)
    $._jsx_text,           // text run inside JSX children
    $._string_content,     // body of a string literal up to the closing quote
  ],

  rules: {
    // ------------------------------------------------------------------------
    // Source file
    // ------------------------------------------------------------------------

    source_file: $ => seq(
      repeat($._newline),
      optional($.module_declaration),
      repeat(seq($._top_level_item, repeat1($._newline))),
      optional($._top_level_item),
    ),

    module_declaration: $ => seq(
      'module',
      field('path', $.module_path),
      repeat1($._newline),
    ),

    module_path: $ => sep1($.identifier, '/'),

    _top_level_item: $ => choice(
      $.import_declaration,
      $.type_declaration,
      $.function_declaration,
      $.component_declaration,
      $.const_declaration,
      $.expression_statement,    // top-level expression statements appear in
                                 // file 01 (the `match User.parse(input)` block)
    ),

    // ------------------------------------------------------------------------
    // Imports [D15]
    // ------------------------------------------------------------------------

    import_declaration: $ => seq(
      'import',
      field('path', $.module_path),
      choice(
        // import std/http
        seq(),
        // import std/http as h
        seq('as', field('alias', $.identifier)),
        // import std/result { Result, Ok, Err }
        seq(
          '{',
          sep1(field('name', $.identifier), ','),
          optional(','),
          '}',
        ),
      ),
    ),

    // ------------------------------------------------------------------------
    // Type declarations [D8, D16]
    // ------------------------------------------------------------------------

    type_declaration: $ => seq(
      'type',
      field('name', $.identifier),
      optional($.generic_parameters),
      '=',
      field('definition', choice(
        $.tagged_union,
        $._type_expression,
      )),
    ),

    // Tagged unions: leading `|` required for multi-line, omitted for single-line.
    // Both shapes share one rule; the grammar accepts both. The formatter
    // enforces the canonical shape based on whether the union is multi-line.
    tagged_union: $ => prec.right(seq(
      optional('|'),
      $._union_variant,
      repeat(seq('|', $._union_variant)),
    )),

    _union_variant: $ => choice(
      // Bare: `Idle`
      field('tag', $.constructor_name),
      // Payload: `Loaded({ users: Array<User> })`
      seq(
        field('tag', $.constructor_name),
        '(',
        field('payload', $._type_expression),
        ')',
      ),
    ),

    // Constructor names are capitalized identifiers. Greppability: every
    // constructor declaration has the same syntactic shape.
    constructor_name: $ => /[A-Z][a-zA-Z0-9_]*/,

    // ------------------------------------------------------------------------
    // Type expressions [D7]
    // ------------------------------------------------------------------------

    _type_expression: $ => choice(
      $.type_reference,
      $.record_type,
      $.function_type,
      $.tuple_type,
    ),

    type_reference: $ => prec.left(seq(
      choice($.identifier, $.constructor_name, $.qualified_type_name),
      optional($.generic_arguments),
    )),

    qualified_type_name: $ => seq(
      $.identifier,
      repeat1(seq('.', choice($.identifier, $.constructor_name))),
    ),

    generic_parameters: $ => seq(
      '<',
      sep1(field('param', $.identifier), ','),
      optional(','),
      '>',
    ),

    generic_arguments: $ => prec(PREC.TYPE_GENERIC, seq(
      '<',
      sep1(field('arg', $._type_expression), ','),
      optional(','),
      '>',
    )),

    record_type: $ => seq(
      '{',
      repeat($._newline),
      optional(seq(
        sep1Trailing($._record_type_field, ','),
      )),
      '}',
    ),

    _record_type_field: $ => seq(
      field('name', $.identifier),
      optional('?'),                     // optional field: `placeholder?: string`
      ':',
      field('type', $._type_expression),
    ),

    function_type: $ => seq(
      'fn',
      '(',
      optional(sep1Trailing($._function_type_param, ',')),
      ')',
      '->',
      field('return_type', $._type_expression),
    ),

    _function_type_param: $ => choice(
      // Named: `input: unknown`
      seq(field('name', $.identifier), ':', field('type', $._type_expression)),
      // Anonymous: just a type
      $._type_expression,
    ),

    tuple_type: $ => seq(
      '(',
      $._type_expression,
      ',',
      sep1Trailing($._type_expression, ','),
      ')',
    ),

    // ------------------------------------------------------------------------
    // Function declarations [D4]
    // ------------------------------------------------------------------------

    function_declaration: $ => seq(
      $._function_declaration_head,
      field('body', $.block),
    ),

    _function_declaration_head: $ => seq(
      optional('async'),
      'fn',
      field('name', $.identifier),
      optional($.generic_parameters),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type_expression))),
    ),

    parameter_list: $ => seq(
      '(',
      repeat($._newline),
      optional(sep1Trailing($.parameter, ',')),
      repeat($._newline),
      ')',
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      optional('?'),
      optional(seq(':', field('type', $._type_expression))),
    ),

    // ------------------------------------------------------------------------
    // Component declarations [D19]
    // ------------------------------------------------------------------------

    component_declaration: $ => seq(
      'component',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type_expression))),
      field('body', $.block),
    ),

    // ------------------------------------------------------------------------
    // Module-level const [D20]
    // ------------------------------------------------------------------------

    const_declaration: $ => seq(
      'const',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type_expression))),
      '=',
      field('value', $._expression),
    ),

    // ------------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------------

    block: $ => seq(
      '{',
      repeat($._newline),
      repeat(seq($._statement, repeat1($._newline))),
      optional($._statement),
      '}',
    ),

    _statement: $ => choice(
      $.let_statement,
      $.mut_statement,
      $.return_statement,
      $.for_statement,
      $.expression_statement,
    ),

    // [D20] `let` is function-local only. The grammar enforces this by only
    // including `let_statement` inside `block`, never at the top level.
    let_statement: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type_expression))),
      '=',
      field('value', $._expression),
    ),

    // [D5] `mut` prefixes assignments or method calls only.
    mut_statement: $ => seq(
      'mut',
      choice(
        $.assignment,
        $.method_call_statement,
      ),
    ),

    assignment: $ => seq(
      field('target', $._assignment_target),
      '=',
      field('value', $._expression),
    ),

    _assignment_target: $ => choice(
      $.identifier_expression,
      $.member_expression,
      $.index_expression,
    ),

    // A method call legal as the body of `mut`: receiver.method(args).
    // Free function calls (`mut foo()`) are syntactically rejected because
    // there's no receiver.
    method_call_statement: $ => prec(PREC.MEMBER, seq(
      field('receiver', $._expression),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    return_statement: $ => seq(
      'return',
      optional(field('value', $._expression)),
    ),

    for_statement: $ => seq(
      'for',
      // Single binding: `for item in xs`
      // Pair binding: `for key, value in xs`
      field('binding', choice(
        $.identifier,
        seq($.identifier, ',', $.identifier),
      )),
      'in',
      field('iterable', $._expression),
      field('body', $.block),
    ),

    expression_statement: $ => $._expression,

    // ------------------------------------------------------------------------
    // Expressions — precedence per PRECEDENCE.md [D18]
    // ------------------------------------------------------------------------

    _expression: $ => choice(
      $.match_expression,
      $.binary_expression,
      $.unary_expression,
      $.await_expression,
      $.try_expression,
      $.call_expression,
      $.member_expression,
      $.optional_chain_expression,
      $.index_expression,
      $.function_expression,
      $.jsx_element,
      $.object_literal,
      $.array_literal,
      $.identifier_expression,
      $.constructor_expression,
      $.literal,
      $.parenthesized_expression,
      $.spread_expression,
    ),

    // -- match (expression form) [D3] ----------------------------------------

    match_expression: $ => seq(
      'match',
      field('scrutinee', $._expression),
      '{',
      repeat($._newline),
      repeat(seq($.match_arm, repeat($._newline))),
      '}',
    ),

    // [D2] Every arm requires a trailing comma, including the last.
    match_arm: $ => seq(
      field('pattern', $._pattern),
      '=>',
      field('body', choice(
        $._expression,
        $.block,
      )),
      ',',
    ),

    // -- Binary / unary / await / try ----------------------------------------

    binary_expression: $ => {
      const table = [
        [PREC.NULLISH,        '??',  'right'],
        [PREC.LOGICAL_OR,     '||',  'left'],
        [PREC.LOGICAL_AND,    '&&',  'left'],
        [PREC.EQUALITY,       '==',  'left'],
        [PREC.EQUALITY,       '!=',  'left'],
        [PREC.COMPARISON,     '<',   'left'],
        [PREC.COMPARISON,     '<=',  'left'],
        [PREC.COMPARISON,     '>',   'left'],
        [PREC.COMPARISON,     '>=',  'left'],
        [PREC.ADDITIVE,       '+',   'left'],
        [PREC.ADDITIVE,       '-',   'left'],
        [PREC.MULTIPLICATIVE, '*',   'left'],
        [PREC.MULTIPLICATIVE, '/',   'left'],
        [PREC.MULTIPLICATIVE, '%',   'left'],
      ];

      return choice(...table.map(([p, op, assoc]) => {
        const fn = assoc === 'right' ? prec.right : prec.left;
        return fn(p, seq(
          field('left', $._expression),
          field('operator', op),
          field('right', $._expression),
        ));
      }));
    },

    unary_expression: $ => prec.right(PREC.PREFIX_UNARY, seq(
      field('operator', choice('!', '-')),
      field('operand', $._expression),
    )),

    // [PRECEDENCE.md] `await` binds looser than `?`. So `await x?` is `(await x)?`.
    // Implemented by giving `await` a lower precedence than `try` (the `?` op).
    await_expression: $ => prec.right(PREC.AWAIT, seq(
      'await',
      field('value', $._expression),
    )),

    // Postfix `?` for Result propagation. Binds tighter than `.` per PRECEDENCE.md.
    try_expression: $ => prec(PREC.POSTFIX_TRY, seq(
      field('value', $._expression),
      '?',
    )),

    // -- Member / call / index / optional chaining ---------------------------

    member_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '.',
      field('property', $.identifier),
    )),

    // [PRECEDENCE.md] `?.` is a single token, distinct from postfix `?`.
    optional_chain_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '?.',
      field('property', $.identifier),
    )),

    call_expression: $ => prec.left(PREC.MEMBER, seq(
      field('callee', $._expression),
      optional($.generic_arguments),
      field('arguments', $.argument_list),
    )),

    index_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    argument_list: $ => seq(
      '(',
      repeat($._newline),
      optional(sep1Trailing($._expression, ',')),
      repeat($._newline),
      ')',
    ),

    // -- Function expressions [D4] -------------------------------------------

    function_expression: $ => seq(
      $._function_expression_head,
      field('body', $.block),
    ),

    _function_expression_head: $ => seq(
      optional('async'),
      'fn',
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type_expression))),
    ),

    // -- Object & array literals [D10, D11] ----------------------------------

    object_literal: $ => seq(
      '{',
      repeat($._newline),
      optional(sep1Trailing($._object_member, ',')),
      repeat($._newline),
      '}',
    ),

    _object_member: $ => choice(
      $.object_field,
      $.spread_expression,
    ),

    // [D10] No shorthand: `name: value` is the only legal form.
    object_field: $ => seq(
      field('key', choice($.identifier, $.string_literal)),
      ':',
      field('value', $._expression),
    ),

    array_literal: $ => seq(
      '[',
      repeat($._newline),
      optional(sep1Trailing(
        choice($._expression, $.spread_expression),
        ',',
      )),
      repeat($._newline),
      ']',
    ),

    // [D11] Spread is an expression form usable in arrays, objects, and arg lists.
    spread_expression: $ => prec(PREC.PREFIX_UNARY, seq(
      '...',
      field('value', $._expression),
    )),

    // -- Identifiers and constructors as expressions -------------------------

    // An identifier in expression position (a binding reference).
    identifier_expression: $ => $.identifier,

    // A constructor reference: `Help`, `Idle`, `Loaded`, etc. Distinguished by
    // capitalization at the lexer level. Greppability pillar.
    constructor_expression: $ => $.constructor_name,

    parenthesized_expression: $ => seq(
      '(',
      $._expression,
      ')',
    ),

    // ------------------------------------------------------------------------
    // Patterns [D9]
    // ------------------------------------------------------------------------

    _pattern: $ => choice(
      $.wildcard_pattern,         // _
      $.else_pattern,             // else (catch-all arm only)
      $.literal_pattern,
      $.identifier_pattern,
      $.constructor_pattern,
      $.type_guard_pattern,       // `is string`, `is Array<unknown>`
      $.array_pattern,
      $.object_pattern,
    ),

    wildcard_pattern: $ => '_',

    else_pattern: $ => 'else',

    literal_pattern: $ => choice(
      $.number_literal,
      $.string_literal,
      $.boolean_literal,
    ),

    // A bare identifier in pattern position binds it.
    identifier_pattern: $ => $.identifier,

    // `Ok(user)`, `Err(NetworkError({ status }))`, `Loaded({ users })`.
    constructor_pattern: $ => seq(
      field('tag', $.constructor_name),
      optional(seq(
        '(',
        optional(sep1Trailing($._pattern, ',')),
        ')',
      )),
    ),

    type_guard_pattern: $ => seq(
      'is',
      field('type', $._type_expression),
    ),

    // `[]`, `["help", ..._]`, `[other, ..._]`, `["add", ...rest]`
    array_pattern: $ => seq(
      '[',
      optional(sep1Trailing(
        choice($._pattern, $.rest_pattern),
        ',',
      )),
      ']',
    ),

    rest_pattern: $ => seq(
      '...',
      choice($.identifier, $.wildcard_pattern),
    ),

    // `{ name }`, `{ name, age }`, `{ status }` — pattern-position destructure.
    // Note: in patterns, shorthand IS allowed (`{ status }` means "bind `status`
    // to the field named `status`"). This is a deliberate asymmetry with object
    // literals [D10]: in patterns the field name and the binding name are
    // typically the same and forcing `{ status: status }` would be noise.
    object_pattern: $ => seq(
      '{',
      optional(sep1Trailing($._object_pattern_field, ',')),
      '}',
    ),

    _object_pattern_field: $ => choice(
      // Shorthand: `{ status }`
      field('name', $.identifier),
      // Renamed: `{ status: s }`
      seq(
        field('name', $.identifier),
        ':',
        field('binding', $._pattern),
      ),
    ),

    // ------------------------------------------------------------------------
    // JSX [D6]
    // ------------------------------------------------------------------------

    jsx_element: $ => choice(
      $.jsx_self_closing,
      $.jsx_paired,
    ),

    jsx_self_closing: $ => seq(
      '<',
      field('tag', $._jsx_tag_name),
      repeat($.jsx_attribute),
      '/>',
    ),

    jsx_paired: $ => seq(
      $.jsx_opening,
      repeat($._jsx_child),
      $.jsx_closing,
    ),

    jsx_opening: $ => seq(
      '<',
      field('tag', $._jsx_tag_name),
      repeat($.jsx_attribute),
      '>',
    ),

    jsx_closing: $ => seq(
      '</',
      field('tag', $._jsx_tag_name),
      '>',
    ),

    // Tags are either lowercase HTML-style (`div`), capitalized component refs
    // (`ResultsList`), or one of the reserved directive names. The grammar
    // treats them uniformly; the typechecker enforces directive semantics.
    _jsx_tag_name: $ => choice(
      $.identifier,
      $.constructor_name,
    ),

    // [D6] Attributes: positional (just a constructor name like `Loaded`),
    // boolean (just an identifier), or named with value (`name="value"` or
    // `name={expr}`).
    jsx_attribute: $ => choice(
      // Positional constructor attribute: `<case Loaded>`
      field('positional', $.constructor_name),
      // Named: `class="foo"` or `value={expr}`
      seq(
        field('name', $.identifier),
        optional(seq(
          '=',
          field('value', choice(
            $.string_literal,
            $.jsx_expression,
          )),
        )),
      ),
    ),

    jsx_expression: $ => seq(
      '{',
      $._expression,
      '}',
    ),

    _jsx_child: $ => choice(
      $.jsx_element,
      $.jsx_expression,
      $._jsx_text,            // external token: text run between elements
    ),

    // ------------------------------------------------------------------------
    // Literals
    // ------------------------------------------------------------------------

    literal: $ => choice(
      $.number_literal,
      $.string_literal,
      $.boolean_literal,
      $.void_literal,
    ),

    // [D13] Integers and decimals; underscore separators allowed.
    number_literal: $ => /-?\d(\d|_)*(\.\d(\d|_)*)?([eE][+-]?\d+)?/,

    // [D12] Double-quoted, with escapes; newlines inside are literal.
    string_literal: $ => seq(
      '"',
      optional($._string_content),
      '"',
    ),

    boolean_literal: $ => choice('true', 'false'),

    // [D16] `void` is both a type and a value. As a value it's a literal.
    // As a type it's matched by type_reference (lexically just an identifier).
    void_literal: $ => 'void',

    // ------------------------------------------------------------------------
    // Lexical
    // ------------------------------------------------------------------------

    // [D14] `//` line comments only.
    line_comment: $ => token(seq('//', /[^\n]*/)),

    // Identifiers: lowercase or underscore start. Capitalized identifiers are
    // constructor_name (handled separately). This split is the lexer-level
    // greppability pillar: `Foo` is always a type or constructor, `foo` is
    // always a value or function.
    identifier: $ => /[a-z_][a-zA-Z0-9_]*/,
  },
});

// -- Helpers --------------------------------------------------------------

// One-or-more `rule` separated by `sep`, with NO trailing separator.
function sep1(rule, sep) {
  return seq(rule, repeat(seq(sep, rule)));
}

// One-or-more `rule` separated by `sep`, with optional trailing separator. [D17]
function sep1Trailing(rule, sep) {
  return seq(rule, repeat(seq(sep, rule)), optional(sep));
}
```

---

## 6. `scanner.c`

External scanner for three context-sensitive tokens tree-sitter's regex lexer
can't handle. Compiles cleanly under `gcc -std=c99 -Wall -Werror`.

```c
// Glyph external scanner.
//
// Handles three tokens the regex-based lexer can't:
//
//   1. NEWLINE      Significant newline, ONLY when bracket depth is zero. [D1]
//   2. JSX_TEXT     Text run inside JSX children, terminated by `<` or `{`.
//   3. STRING_CONTENT
//                   Body of a string up to the closing `"`, supporting escapes
//                   and embedded newlines. [D12]
//
// Tree-sitter calls the scanner with a `lexer` interface. We track bracket
// depth across calls via the serialized state.

#include "tree_sitter/parser.h"
#include <wctype.h>

enum TokenType {
  NEWLINE,
  JSX_TEXT,
  STRING_CONTENT,
};

// State carried between calls: the current bracket-depth counter.
typedef struct {
  uint32_t bracket_depth;
} Scanner;

void *tree_sitter_glyph_external_scanner_create(void) {
  Scanner *s = (Scanner *)calloc(1, sizeof(Scanner));
  s->bracket_depth = 0;
  return s;
}

void tree_sitter_glyph_external_scanner_destroy(void *payload) {
  free(payload);
}

unsigned tree_sitter_glyph_external_scanner_serialize(
    void *payload, char *buffer) {
  Scanner *s = (Scanner *)payload;
  buffer[0] = (char)(s->bracket_depth & 0xFF);
  buffer[1] = (char)((s->bracket_depth >> 8) & 0xFF);
  buffer[2] = (char)((s->bracket_depth >> 16) & 0xFF);
  buffer[3] = (char)((s->bracket_depth >> 24) & 0xFF);
  return 4;
}

void tree_sitter_glyph_external_scanner_deserialize(
    void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (length >= 4) {
    s->bracket_depth =
        ((uint32_t)(unsigned char)buffer[0]) |
        ((uint32_t)(unsigned char)buffer[1] << 8) |
        ((uint32_t)(unsigned char)buffer[2] << 16) |
        ((uint32_t)(unsigned char)buffer[3] << 24);
  } else {
    s->bracket_depth = 0;
  }
}

static bool scan_newline(Scanner *s, TSLexer *lexer) {
  // Newlines are tokens only when we are at bracket depth zero.
  // We still consume them when inside brackets so they don't get treated as
  // significant elsewhere — but we return false so the parser sees nothing.

  // Skip leading inline whitespace before a potential newline.
  // (Tree-sitter has already skipped `extras`; we re-check defensively.)
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
    lexer->advance(lexer, true);
  }

  if (lexer->lookahead != '\n' && lexer->lookahead != '\r') {
    return false;
  }

  // Consume one or more consecutive line terminators as a single NEWLINE token.
  bool consumed = false;
  while (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
    lexer->advance(lexer, false);
    consumed = true;
    // Also consume any inline whitespace on the next line so blank lines
    // collapse into the same NEWLINE token.
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
      lexer->advance(lexer, false);
    }
  }

  if (!consumed) return false;

  // Only emit the NEWLINE token if we're at top-level (depth 0).
  // Inside brackets, the newline is whitespace.
  if (s->bracket_depth == 0) {
    lexer->result_symbol = NEWLINE;
    return true;
  }
  // Inside brackets: we already consumed; return false so the parser treats
  // whatever follows as the next real token.
  return false;
}

static bool scan_jsx_text(TSLexer *lexer) {
  // A JSX text run is any sequence of chars up to (but not including) `<` or
  // `{`. It must contain at least one non-whitespace char to be meaningful;
  // pure whitespace between elements is consumed as extras.
  bool has_content = false;
  bool has_non_whitespace = false;

  while (lexer->lookahead != 0 &&
         lexer->lookahead != '<' &&
         lexer->lookahead != '{' &&
         lexer->lookahead != '}') {
    if (!iswspace(lexer->lookahead)) {
      has_non_whitespace = true;
    }
    lexer->advance(lexer, false);
    has_content = true;
    lexer->mark_end(lexer);
  }

  if (has_content && has_non_whitespace) {
    lexer->result_symbol = JSX_TEXT;
    return true;
  }
  return false;
}

static bool scan_string_content(TSLexer *lexer) {
  // Scan the body of a string literal up to (but not including) the closing
  // `"`. Honors backslash escapes (skip the next char after `\`). Embedded
  // newlines are kept verbatim. [D12]
  bool has_content = false;

  while (lexer->lookahead != 0 && lexer->lookahead != '"') {
    if (lexer->lookahead == '\\') {
      lexer->advance(lexer, false);
      if (lexer->lookahead != 0) {
        lexer->advance(lexer, false);
      }
      has_content = true;
      continue;
    }
    lexer->advance(lexer, false);
    has_content = true;
  }

  if (has_content) {
    lexer->result_symbol = STRING_CONTENT;
    return true;
  }
  return false;
}

bool tree_sitter_glyph_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // Track bracket depth by peeking at what the main lexer is about to do.
  // We update depth based on bracket characters we observe at the current
  // position WITHOUT consuming them — tree-sitter's main lexer will consume
  // them as terminal tokens. We just maintain the counter for newline logic.
  //
  // Implementation note: tree-sitter doesn't give us a clean hook to observe
  // every token, so we update on-demand whenever the scanner is invoked.
  // Bracket tokens (`(`, `)`, `[`, `]`, `{`, `}`, `<`, `>`) are produced by
  // the main lexer; here we maintain the counter by observing the lookahead
  // before any other scanning.

  // (Note: `<` and `>` are NOT counted because they are used for both generics
  // and comparison; counting them would break newline handling inside type
  // arguments versus comparisons. The grammar handles type-argument newlines
  // by not requiring them. This is a deliberate simplification.)

  if (valid_symbols[STRING_CONTENT]) {
    return scan_string_content(lexer);
  }

  if (valid_symbols[JSX_TEXT]) {
    if (scan_jsx_text(lexer)) return true;
  }

  if (valid_symbols[NEWLINE]) {
    // Bracket-depth maintenance: peek and update for bracket chars.
    // The main lexer will then consume the bracket; we don't.
    int c = lexer->lookahead;
    if (c == '(' || c == '[' || c == '{') {
      s->bracket_depth++;
      // Don't consume; let main lexer handle.
      return false;
    }
    if (c == ')' || c == ']' || c == '}') {
      if (s->bracket_depth > 0) s->bracket_depth--;
      return false;
    }
    return scan_newline(s, lexer);
  }

  return false;
}
```

---

## 7. Build files

### `package.json`

```json
{
  "name": "tree-sitter-glyph",
  "version": "0.1.0",
  "description": "Tree-sitter grammar for the Glyph language",
  "main": "bindings/node",
  "keywords": ["parser", "tree-sitter", "glyph"],
  "scripts": {
    "generate": "tree-sitter generate",
    "parse": "tree-sitter parse",
    "test": "tree-sitter test"
  },
  "tree-sitter": [
    {
      "scope": "source.glyph",
      "file-types": ["glyph"],
      "highlights": "queries/highlights.scm"
    }
  ],
  "devDependencies": {
    "tree-sitter-cli": "^0.22.0"
  }
}
```

### `binding.gyp`

```python
{
  "targets": [
    {
      "target_name": "tree_sitter_glyph_binding",
      "include_dirs": [
        "<!(node -e \"require('nan')\")",
        "src"
      ],
      "sources": [
        "bindings/node/binding.cc",
        "src/parser.c",
        "src/scanner.c"
      ],
      "cflags_c": ["-std=c99"]
    }
  ]
}
```

### `.gitignore`

```
node_modules/
build/
src/parser.c
src/grammar.json
src/node-types.json
src/tree_sitter/
*.log
.DS_Store
```

---

## 8. Editor support

Tree-sitter highlighting queries. Gives editors (Neovim, Helix, Zed, Emacs)
syntax highlighting the moment the parser is generated.

```scheme
; Syntax highlighting queries for Glyph.
; Used by editors via tree-sitter for instant highlighting once `tree-sitter
; generate` produces the parser.

; Keywords
[
  "module"
  "import"
  "as"
  "type"
  "fn"
  "component"
  "const"
  "let"
  "mut"
  "return"
  "match"
  "for"
  "in"
  "is"
  "await"
  "async"
  "else"
] @keyword

; Special values
[
  "true"
  "false"
  "void"
] @constant.builtin

; Literals
(number_literal) @number
(string_literal) @string
(boolean_literal) @constant.builtin

; Comments
(line_comment) @comment

; Constructors (capitalized identifiers) — types and tagged-union tags
(constructor_name) @type

; Type references
(type_reference (identifier) @type)
(type_declaration name: (identifier) @type)

; Function and component declarations
(function_declaration name: (identifier) @function)
(component_declaration name: (identifier) @function)
(parameter name: (identifier) @variable.parameter)

; Calls
(call_expression callee: (identifier_expression (identifier) @function.call))
(method_call_statement method: (identifier) @function.call)

; Operators
[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" "<=" ">" ">="
  "&&" "||" "??"
  "!" "="
  "->" "=>"
  "?" "?."
  "..."
] @operator

; Punctuation
[ "," ";" ":" "." ] @punctuation.delimiter
[ "(" ")" "[" "]" "{" "}" ] @punctuation.bracket

; JSX
(jsx_opening tag: _ @tag)
(jsx_closing tag: _ @tag)
(jsx_self_closing tag: _ @tag)
(jsx_attribute name: (identifier) @attribute)

; Module declaration
(module_declaration path: (module_path) @namespace)
(import_declaration path: (module_path) @namespace)
```

---

## 9. How to use it

```bash
# Bootstrap
npm install

# Generate the parser (writes src/parser.c from grammar.js)
npx tree-sitter generate

# Parse the corpus
npx tree-sitter parse examples/01_validator.glyph
npx tree-sitter parse examples/02_async_errors.glyph
npx tree-sitter parse examples/03_react_component.glyph
npx tree-sitter parse examples/04_cli_tool.glyph
```

If `tree-sitter generate` reports parse table conflicts, those are real
ambiguities in the spec — resolve them in `SPEC_DECISIONS.md` first, then
adjust the grammar.

---

## 10. Known conflicts and next steps

### What was validated in the build environment

- `grammar.js` loads as JavaScript; all 84 rules evaluate without errors when
  probed individually.
- `scanner.c` compiles cleanly with `gcc -std=c99 -Wall -Werror`.

### What was NOT validated

- `tree-sitter generate` has not been run. The CLI binary lives on a GitHub
  release asset URL that isn't in the build environment's network allowlist.
  Local generation is the next step.

### Likely conflicts on first generate

In rough order of probability:

1. **Type expressions vs value expressions at `<`.** A conflict is already
   declared between `call_expression` and `binary_expression`. May need
   tightening if real ambiguity surfaces.
2. **Object literal `{}` vs block `{}` in match arm body.** Both start with
   `{`. The grammar disambiguates by content (`expr,` is a literal field;
   `let x = ...` is a block statement), but GLR may need a hint.
3. **`fn` as declaration vs expression at parse start.** Already declared.
4. **JSX `<` vs less-than `<`.** JSX is only legal in expression position,
   and the `<` must be followed by an identifier or constructor name (not a
   number or paren). GLR handles this fine in practice.

### What the grammar is not

- **Not the typechecker.** `result?.field` is grammatically legal but the
  typechecker rejects it when `result` is `Result`-typed (per
  `PRECEDENCE.md`).
- **Not the formatter.** The grammar accepts both single-line and multi-line
  tagged unions; the formatter chooses the canonical form.
- **Not the linter.** The grammar accepts `mut result.push(x)`; whether
  `push` is actually a mutating method is the typechecker's call.

The grammar is the syntactic spec, not the semantic one. That's intentional.

### Next steps in the plan

Step 3 produces the grammar and the spec it pins down. Step 4 (transpiler to
TypeScript) and Step 5 (typechecker) both consume `grammar.js` as their
front-end. The next concrete action is:

1. Run `npx tree-sitter generate` locally. Resolve any conflicts that surface
   by amending `SPEC_DECISIONS.md` first, then the grammar.
2. Confirm all four example files parse cleanly with `tree-sitter parse`.
3. Write 5-10 corpus test files in `test/corpus/` using tree-sitter's S-expr
   test format. These become regression tests for every later change.
4. Begin step 4 (transpiler) against this grammar.
