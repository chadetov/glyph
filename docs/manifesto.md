# Glyph manifesto

A statically typed, transpile-to-TypeScript language built so code an AI agent writes can be read, checked and changed without guessing, for production systems where agents work on the codebase alongside people.

Full original text: `archive/MANIFESTO.md`.

## What Glyph is

It looks almost like TypeScript. A TS developer reads a Glyph file on day one without a tutorial. The differences are deliberate and small in number — every one of them exists to make code that an agent can reason about correctly, edit without breakage, and explain back to a human without lying.

Glyph is not a research language. No effect systems, no dependent types, no macros. The "no linear types" rule has exactly one v1 carve-out: a narrow `owned` modifier for resource handles (file/socket/db-connection) with single-consumption tracking — covering TS developers' real pain (forgotten `.close()`, leaked connections; cf. TC39 `using` stage 3) without committing to a general affine/linear system. See `docs/language/spec.md` D25. Otherwise: TypeScript with the ten footguns removed and four properties enforced — nothing more, nothing less.

## Who Glyph is for

TypeScript developers building with AI agents who feel the daily friction:

- Agents hallucinating APIs because TS's type erasure means runtime and compile-time disagree.
- Agents papering over uncertainty with `any` and `as unknown as T`.
- Agents rewriting whole files when asked to change one line, because the formatter cascades on every edit.
- Agents grepping for a symbol and finding ten unrelated matches because TS overloads syntactic forms.
- Reviewers who can't tell whether a PR is correct because the diff is twelve hundred lines of reflowed whitespace.

Glyph removes those costs. If you are building a Lisp for AI, a DSL for prompts, or a logic language for reasoning systems — Glyph is not for you.

## The four pillars

Every design decision is tested against these. If a feature improves one without harming the others, it ships. If not, it doesn't.

Each pillar is a property of the language, not of a project's configuration. That is the answer to the most common objection, "TypeScript can do that with the right settings." Often it can, in one repository, for the people who set it up. Glyph makes the property true of every Glyph file, so an agent opening a codebase it has never seen can rely on it without first finding out which settings that codebase chose.

### 1. Verifiability
Anything the type system claims is true at runtime. There is no `any` and no cast. A value that crosses a boundary (a JSON body, a request, an npm call) is checked against its type when it enters: `json.parse<User>(body)` returns a `Result` that is `Ok` only if the value really is a `User`, because every record and union has a runtime descriptor the compiler generates from the type. Where the compiler cannot build that check (an `extern_ts` type, say), it refuses the program (E0304) rather than trust it. A `match` must cover every case (E0200), and `Result` and `Option` must be opened, not read past.

Strict TypeScript gets close inside a program and stops at its edges. `JSON.parse(body) as User` compiles under every setting, and a validator such as zod is a second description of the type that has to be kept in step with the first. In Glyph the type is the validator.

### 2. Greppability
Every symbol has exactly one syntactic form at its declaration site. No method overloads, no decorators that rename, no implicit `this`, no declaration merging. `grep -n "fn parse_user"` finds the definition, always. Lint rules can ban some of these in a TypeScript project, but the language still allows them, so a reader of unfamiliar TypeScript cannot assume the ban; a reader of Glyph can.

The same rule covers meaning, not just names: one construct has one spelling. When two spellings of the same check would give different answers (`T.is(v)` and `match v { is T => … }`, for instance), that is treated as a bug and fixed so they agree.

### 3. Abstraction
Express intent at the level the writer is thinking. Pattern matching over `if`/`else` ladders and `switch`. `Result` over thrown exceptions. Named records over positional tuples. Tagged unions over flag fields. A small core of orthogonal primitives instead of TypeScript's accreted layers. Fewer ways to say a thing means fewer shapes for an agent to get wrong, and a smaller program for a reviewer to read.

### 4. Diff stability
A one-line change produces a one-line diff. `glyph fmt` has one canonical layout and no options: a list stays on one line while it fits the print width, and otherwise goes one element per line with a trailing comma. It never repacks a list to fill the line, so inserting or removing an element touches only that element's line.

The common objection here is that Prettier with `trailingComma: "all"` does the same for TypeScript. For the formatting half, mostly it does, and the difference is in three places:

- **The layout is a function of the code alone.** Glyph prints the same text for the same program whatever shape the input was in. Prettier keeps an object literal expanded if its author put a line break after the `{`, so two agents writing the same object can produce two layouts, and the next edit to either one is a reflow in someone's diff.
- **There is nothing to configure.** Prettier's output depends on a config file that differs between repositories, and is absent from some. Glyph's does not, so a file means the same layout in every project.
- **The formatter cannot change what a file says.** `glyph fmt` is tested by parsing its own output and comparing the syntax tree with the input's, and it refuses a file (E0500) rather than move a comment somewhere it would describe the wrong code.

The print width is the one place a change to a line can still move the lines around it: a rename that pushes a call past the limit expands it to one argument per line, and shortening it collapses the list back. That is what the rule costs. What it buys is a layout that depends on the code and the column it starts at and nothing else, so two agents editing the same file land on the same text and neither one's diff carries the other's reflow.

Imports are explicit and full-path, and there are no barrel files. Imports are not yet sorted by `glyph fmt` (G358).

**Weighting.** Verifiability and greppability are the wedge: they fix problems TypeScript developers actually feel and that settings do not fix. Abstraction and diff stability are the polish that makes daily use pleasant. When pillars conflict, the wedge wins.

## What Glyph deliberately is not

- **Not a research language.** If TypeScript developers don't already wish they had it, it doesn't ship.
- **Not a Lisp or DSL for AI.** General-purpose application language. Agents are the reader, not the runtime.
- **Not a TS replacement.** Glyph compiles to TS, imports from npm, is importable from `.ts` files. Adoption is per-file.
- **Not configurable.** One formatter, no options. One module resolution algorithm. One strictness level (strict). The cost of configurability is paid by every agent that has to reason about which dialect it's editing.
- **Not self-hosting in v1.0.** The compiler is Rust forever, or at least until v1.0 ships and users exist. Self-hosting was promoted from a year-three deferral to a v1.0 non-goal in session 2 (`archive/glyph_step6_session.md`).

## The bet

Within five years, the median line of production code will be written by an agent and reviewed by a human. The languages that win that era will be the ones designed for that workflow, not retrofitted to it. TypeScript will retrofit eventually — it always does. The window between now and then is where Glyph earns its place.

The hypothesis, stated as one we intend to measure rather than a number we have earned: an agent given the same task produces correct code more reliably in Glyph than in TypeScript, and the reviewer spends less time confirming it is correct. That is the bet every pillar, every example, and every "no" is designed to make true. We have not run the study yet, so we do not put a figure on it. When we do (same task, many trials, Glyph versus TypeScript, tracking correctness and review time), the result stands on its own, whichever way it falls.

The pillars make a single file trustworthy. What an agent needs on top of that is to ask about a whole program and get an answer it does not have to reconstruct. So the compiler answers questions, not just builds: what type an expression has, where a name is defined, what a change to a declaration breaks, whether a value of one type is accepted where another is declared, and, through the language server, whether a proposed edit introduces a new error before it is written to disk. `glyph check --json`, `glyph query`, the language server and the MCP server answer from the same compiler, so they agree. This is not a fifth pillar. It is how an agent uses the four: verifiability and greppability make the answers true, and the query surface lets an agent get them without reading the whole codebase.
