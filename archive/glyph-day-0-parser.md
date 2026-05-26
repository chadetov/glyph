# Glyph — Day 0 Parser & Roadmap Notes

A working document capturing decisions and open questions from the day-0 discussion. Two parts: feedback on roadmap items 8–9 (formatter, package story, installer, playground), then the parser conversation that's the real day-0 work.

---

## Part 1 — Roadmap items 8 and 9

### Item 8: Formatter and package story

**Keep:** Fixed-width, one-element-per-line-above-two-elements, no config. This follows directly from the diff stability pillar and Example 3 of the manifesto is its strongest argument. Ship as described.

**Keep:** npm piggyback. Zero ecosystem to bootstrap, instant access to the largest package registry, matches the "compiles to TypeScript, imports from npm" stance. Don't build a registry — it's a five-year distraction masquerading as a two-week task.

**Reconsider:** `glyph.json` as a wrapper over `package.json`. Two files invite drift, and now agents have to reason about which file is authoritative for what. Use a `"glyph"` key inside `package.json` instead. One file, one source of truth, composes cleanly with existing npm tooling, and more consistent with the "not configurable" stance.

### Item 9: Installer and playground

**Keep:** Playground as the highest-leverage marketing artifact. For a language whose pitch is "agents can read and edit this," a visitor needs to *see* Glyph and the TS it compiles to, side by side, in under 30 seconds. Default example should produce a *meaningful* TS diff — the `load_feed` function from `02_async_errors.txt` compiling to TS with try/catch and manual Promise.all error handling sells the language better than the manifesto does.

**Add:** A third pane showing the same code edited by an agent — a one-line semantic change producing a one-line diff. Diff stability is the pillar that's hardest to *feel* from a static sample. Verifiability shows up in type signatures, greppability in naming. Diff stability is invisible until you watch an edit happen.

**Reconsider:** Curl-pipe-bash installer. Target audience already has Node and trusts npm. Ship `glyph` as an npm package (`npm install -g glyph`, `npx glyph`). Lower friction, cross-platform by default, no "do I trust this shell script" hesitation. Curl-pipe-bash is a Rust/Go convention; for a transpile-to-TS language it's an unforced reach for credibility through aesthetics.

### Sequencing concern

Items 8 and 9 are both "make it real to outsiders" tasks and are correctly placed late. But the formatter is downstream of every syntactic decision. The parser must be genuinely frozen before week one of formatter work, or the formatter gets rewritten twice.

---

## Part 2 — The parser conversation

Day 0 is the right time. Parser decisions calcify faster than any other part of a language. Once a thousand lines of Glyph exist (the test corpus counts), every syntactic choice becomes a migration.

### What the four sample files already commit you to

Reading the samples as spec rather than illustration, these are decided whether intended or not:

- Curly-brace blocks, semicolon-free, newline-terminated statements
- Trailing commas everywhere
- `fn` for functions, `type` for type aliases, `component` as a distinct keyword
- Pattern matching as an **expression** (`return match e { ... }`)
- Tagged unions with `|` and payload syntax `Variant({ field: type })`
- Generics with `<T>`
- Postfix `?` for Result propagation
- Prefix `await`
- Records as structural object literals
- Arrays with `[...]` and spread

Most of this is uncontroversial and tracks TypeScript closely enough that a TS dev reads it on day one — which is what MANIFESTO promises.

### Where the samples conflict with themselves

These need resolution before any grammar gets written, because each is a fork affecting dozens of downstream decisions.

#### 1. Statement vs expression for `match`

`format_parse_error` uses `return match e { ... }` — match is an expression.

`object_schema` has `match issues.length { 0 => Ok(result), else => Err(issues) }` as the last thing in a block, no `return`.

So: is the block's value the match's value (Rust-style implicit return), or is `return` required and the validator code is buggy?

**Recommendation:** mandatory `return` everywhere. Uglier but greppable, and greppability is the wedge pillar. Against personal aesthetics, but more manifesto-consistent.

#### 2. `mut` semantics

Validator has `mut result[key] = value` and `mut result.push(...)`. So `mut` is a *statement prefix* marking any mutating operation, not just assignment.

PRECEDENCE.md only mentions `mut` for assignment. The push case is making a real claim: "this method call mutates, and you must acknowledge it."

How does the parser know `push` mutates and `map` doesn't? Two options:

- (a) Every method has a mutation annotation in its signature; type checker enforces `mut` at call sites. This is a serious type-system feature not yet mentioned in the manifesto.
- (b) `mut` is purely syntactic sugar with no checking — a convention. This is a lie the language tells.

Decide now. Pick (a) or drop the call-site `mut`.

#### 3. Trailing `?` in different positions

`result?` for propagation is clear. But `array.find(...).map(f)?` — does `?` apply to the result of `map`?

The parser doesn't know types. So `?` is purely syntactic; the type checker rejects it later if the expression isn't a Result. State this explicitly in PRECEDENCE.md.

#### 4. JSX-with-directives in `03_react_component.txt`

Biggest unresolved thing. `<match value={...}><case Loaded bind={users}>...</case></match>` is a parser nightmare and a bigger semantic nightmare. `bind={users}` introduces a binding in scope inside the case body — that's not JSX, that's a macro.

How does the parser distinguish `<match>` (directive) from `<Match>` (user component)? Lowercase tag? Reserved keyword list? What about a user component called `If`?

**Recommendation:** Parser-level directives. The grammar knows about `<match>`, `<if>`, `<for>`, `<case>`, `<else>`. The set is small, the manifesto already promises "compiler-owned directives," and error messages can say "expected `<case>` inside `<match>`" instead of "Component `case` is not defined." Desugaring-level is more flexible but produces terrible errors.

### The big day-0 decisions to force

Beyond the conflicts above, decide each of these before grammar work begins.

**Significant whitespace:** No. Braces are load-bearing, indentation is formatter-enforced but parser-irrelevant. Python-style indentation is hostile to grep and to LLMs that occasionally drop a space.

**Statement terminators:** Newlines only. Ban semicolons entirely. One way to terminate a statement.

**Trailing commas:** Required in multi-line literals. Formatter adds them; parser rejects their absence. Diff stability win — adding a field never modifies the previous line.

**Expression-oriented or statement-oriented blocks:** Tied to the match question. Pick one and apply ruthlessly. Mixing is the worst outcome.

**Generics with `<>`:** TypeScript hacks around the `f<T>(x)` vs `f < T > (x)` ambiguity with contextual lookahead. Since you compile to TS and TS handles this, you can steal that approach. Know that the parser will have a non-trivial lookahead component as a result.

**Trailing closures / lambda shorthand:** No. `array.filter(file.items, fn(t) { return t.id != id })` is verbose but consistent with the manifesto. Explicit parameter names are more greppable than a magic `it`.

**String interpolation:** This one is worth reconsidering. Samples use `"foo " + bar + " baz"` concatenation only. Defensible (one syntax for the operation) but `"foo ${bar} baz"` is universal in modern languages and omitting it feels archaic. The parser cost is contained. Greppability cuts both ways — `${user.name}` and `" + user.name + "` are equally greppable, but the first is more readable. Push: add interpolation.

**Macros / metaprogramming:** None. Manifesto already says so. But `record User { ... }` generates a runtime parser — that's compiler-level codegen, not user macros. State explicitly that the compiler has a fixed set of derivations (`.parse`, `.schema`, possibly `.equals`) and users cannot add their own. Verifiability win and a complexity ceiling.

### Concrete next two weeks

1. **Write `GRAMMAR.md`** — a 200-line EBNF-ish sketch of every syntactic construct, with one real example per construct (not toy examples). Include the JSX directive set. Include the precedence table already in PRECEDENCE.md. Include resolutions to all four conflicts above.

2. **Write a corpus of 30–50 Glyph files** exercising every grammar production, *before* writing the parser. These become the test suite. Half should be deliberately broken in interesting ways so error messages can be validated.

3. **Then write the parser.** Hand-rolled recursive descent in TypeScript (eating your own dog food, mostly — the compiler is in TS until Glyph can self-host). Parser generators are tempting and wrong: error messages are the user's primary interaction with the parser, and generators give bad ones.

### Open questions to resolve next

In rough priority order:

1. Match as expression or statement (cascades through every block)
2. `mut` semantics (type-system feature or convention)
3. JSX directives parser-level vs desugaring
4. String interpolation yes/no
5. Postfix `?` parser-level vs type-checker-level rejection
