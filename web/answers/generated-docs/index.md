<!-- Markdown copy of https://glyphlang.io/answers/generated-docs/, generated from the same source as the page. Last changed 2026-11-21. -->

24 · Docs that can't drift

# Is the documentation my agent reads generated, or written by hand and out of date?

Generated. `glyph llms --json` emits the diagnostic catalogue, the prelude, the stdlib signatures, the language decisions and the tool manual out of the compiler's own tables, and a gate fails the build when a hand-written copy disagrees. Every Glyph example in the document an agent bootstraps from is compiled, including the wrong ones.

glyph llms --json, the E0200 entry (trimmed)

```
{
  "code": "E0200",
  "phase": "typechecker",
  "meaning": "Non-exhaustive `match` on a tagged union (yours, a prelude
    `Result`/`Option`, or a stdlib one such as `fs.ErrorKind`), or a
    string-literal union (`\"free\" | \"pro\"`, D30) missing a literal ...",
  "fix": "Add an arm for each missing variant",
  "help": "Add an arm for each missing variant.",
  "note": "Tagged unions are sealed (D9): adding a variant forces every
           match to be updated. A `_`/`else` catch-all is allowed but
           forfeits that guarantee.",
  "counter_example": {
    "name": "alias_of_local_union_not_exhaustive",
    "files": [ { "source": "module main\n\ntype Shape =\n  | Circle\n ..." } ],
    "diagnostic": { "code": "E0200",
      "message": "non-exhaustive match on `Shape`: missing variants `Tri`",
      "missing_variants": ["Tri"] },
    "corrected": null,
    "corrected_absent": "`tests/negative/` pairs no repaired program with a
      case: an entry is a wrong program and the code it draws. A fix
      invented here would compile nowhere and be checked by nothing."
  }
}
```

Seven sections, each read off a table the compiler already has.

## Why it matters

The question is not rhetorical, because the honest answer used to be "written by hand, and yes."

- **The tool list** The agent bootstrap said the MCP server "exposes five tools" and listed five. The server served seven. The two the server's own instructions tell an agent to reach for first appeared in it zero times.
- **The error codes** The 58 error codes lived in three hand-maintained copies, checked only for whether the code string was present.
- **The examples** Of 19 Glyph examples in that document, 2 were compiled and 17 were skipped as fragments.

Every one of those is the same defect. A document about a compiler, maintained beside the compiler, by people who meant well. An agent that reads it cannot tell which sentences are still true, and neither could we.

## One table for the diagnostics

A diagnostic entry carries the catalogue sentence, the one-line fix, the long explanation, and the help and note taken from a diagnostic the compiler drew.

51 of the 77 codes also carry a wrong program that draws the code, compiled at the moment you ask; the other 26 say the corpus has no case for it.

**Details three copies merged into one **

77 codes come out of one table in the compiler, and `docs/error-codes.md` and the catalogue table in the agent bootstrap are written from it by a script that fails on drift. Merging the three copies that used to exist turned up a code with an explanation and no place in the list, and a row whose escaped pipe had quietly broken the table it sat in.

## The prelude, the stdlib, and the decisions

The prelude is the 24 names the resolver resolves against, with what each one is. The stdlib is 40 modules and 340 exports, and each signature is read out of the checker's own tables and printed by the same type printer a diagnostic uses, so the signature in the document is the signature the checker will hold you to.

"stdlib"

```
{ "path": "std/array", "exports": [
  { "name": "filter",
    "signature":
      "fn(Array<T>, fn(T) -> bool) -> Array<T>" },
  { "name": "find",
    "signature":
      "fn(Array<T>, fn(T) -> bool) -> Option<T>" },
  { "name": "fold",
    "signature":
      "fn(Array<T>, U, fn(U, T) -> U) -> U" } ] }

"modeled": 129, "partially_modeled": 46,
"unmodeled": 165
```

"prelude" and "decisions"

```
"prelude": { "count": 24, "names": [
  { "name": "int", "role": "type" },
  { "name": "Ok",  "role": "value" },
  { "name": "par", "role": "namespace" }, ... ] }

"decisions": { "count": 49,
  "duplicate_numbers": [],
  "decisions": [
    { "id": "D9",
      "section": "Control flow & expressions",
      "title": "`else` is a catch-all arm; `_` is a
        position-level wildcard; a PascalCase arm
        head is a variant reference ..." }, ... ] }
```

**Details how the exports split **

The exports split three ways and never two: 129 signatures are modeled in every position, 46 carry a `?` where a parameter type belongs and are counted `partially_modeled` with a line saying how many positions are left, and 165 have no signature at all. A `?` is `unknown` rendered, so it is never published as a complete signature.

**History why `duplicate_numbers` exists **

`duplicate_numbers` is there because the generator reads the spec's own list, and the first time it ran it found two different decisions both numbered D43 and printed `"duplicate_numbers": [43]`. It reports a collision rather than deduplicating it, which is how anyone found out. Nothing in the compiler keys on a D number, so no program was affected; a reader following a citation was. That one is fixed: D43 is the `mut`-across-`await` rule and `never` is D47, so the list comes back empty. There are 49 decisions today, D1 to D49.

## The wrong programs are readable too

The compiler's negative corpus is a set of programs that must fail, with the code each one must draw. It used to be visible only to `cargo test`.

An agent learning what a rule refuses is exactly the reader it was written for, so it answers now, and each case is compiled when you ask so the diagnostic beside it is this compiler's.

glyph llms --negative E0008

```
{
  "code": "E0008",
  "meaning": "Assignment without `mut` (`x = e` should be `mut x = e`, or
              `let x = e` for a new binding; D5)",
  "fix": "Write `mut x = e`, or `let x = e` for a new binding",
  "negative": [ {
    "name": "bare_assignment_without_mut",
    "files": [ { "path": "bare_assignment_without_mut.glyph",
      "source": "module neg\n\nfn count() -> number {\n  let total = 0\n
                 total = total + 1\n  return total\n}\n" } ],
    "diagnostic": { "code": "E0008", "stage": "parse",
      "message": "assignment requires `mut`",
      "help": "Glyph marks every mutation (D5): write `mut x = ...` to
               reassign an existing binding, or `let x = ...` to
               introduce a new one.",
      "range": { "start": { "line": 5, "col": 3, "offset": 53 }, ... } } } ],
  "catches": [],
  "catches_absent": "`catches/` pairs no TypeScript-accepted program with E0008"
}
```

**Details coverage, and the `catches/` corpus **

With no code it lists 117 cases over 53 of the 77 codes in 0.1.149, and names the 24 that have none rather than leaving you to infer coverage from what came back. A second corpus, `catches/`, pairs a program `tsc --strict` accepts with the Glyph that refuses it, and those come back on the same answer where they exist. Both are embedded from the directories themselves, so a case added to either is in the answer with no second edit to remember.

## Every example compiles, including the wrong ones

Generating the reference is half of it. The other half is the prose an agent bootstraps from, where the examples are the part it copies.

Every Glyph fence in that document is either a complete program that compiles, or is marked as a fragment of a named complete example whose lines it has to appear in.

- **The bootstrap** It was 19 fences with 2 compiled; it is 24 with 20 compiled and 4 marked, and none skipped.
- **The stdlib reference** The stdlib reference went from 4 of 8 to 8 of 8.
- **The wrong forms** A fence can also be marked as a program that must fail, naming the code it draws, which is how the document carries the five wrong forms it warns you about: the one that draws `E0228`, the one that draws `E0008`, and three more. A warning paired with the diagnostic is checkable. A warning written in prose is a promise.

**Details two things compiling the documents found **

Compiling the documents found two things reading them was never going to find. An inline triple backtick had read as a fence opener, so 250 lines counted as one snippet and nothing inside them was checked. And a worked server in the stdlib reference called `http.serve`, which `std/http` does not export. The function is `listen`.

## What it costs to hand over, and what it stopped costing

The eleven MCP tools used to carry their whole manual in the `tools/list` reply, which every session pays for on connect: 31,446 bytes of description in a 45,509-byte reply. The same eleven tools carry 8,627 in a 20,386-byte reply now.

Nothing was deleted. The previous text is held verbatim, keyed by tool name, and published inside `glyph llms --json`, so the contract you need at the moment of the call is in the description and the rest is one command away instead of in every session's first reply.

**Details the test that holds the count **

A test reads the tool list and the document together, so a document naming a tool the server does not serve, or a count that disagrees with the list, fails the build. That is the check "exposes five tools" needed.

### Offline and local

All of this is offline and local. `glyph llms` prints the prose, `glyph llms --json` the data, `glyph llms --negative` the wrong programs.

The same prose is served at [llms.txt](https://glyphlang.io/llms.txt) for an agent with a network and no binary. What an agent asks about *your* code, rather than about the language, is the [query tools](https://glyphlang.io/answers/ask-the-compiler/).

## Where it stands

### Shipping today

- `glyph llms --json` emits seven sections out of the compiler

  **Details**

  `glyph llms --json` emits seven sections out of the compiler: 77 diagnostic codes, 51 of them with a compiled counter-example, the 32 reserved words, the 24 prelude names, 40 stdlib modules with 340 exports, 49 language decisions parsed from the spec, and the eleven tools with their `glyph query` verbs and full manuals.

- `docs/error-codes.md` and the bootstrap's diagnostic table are written from the compiler's own code table

  **Details**

  `docs/error-codes.md` and the bootstrap's diagnostic table are written from the compiler's own code table by a script that fails on drift and re-mirrors `llms.txt` with it.

- A test holds the tool catalogue to what the server serves.
- Every Glyph fence in the bootstrap and the stdlib reference compiles or is a marked fragment of one that does

  **Details**

  Every Glyph fence in the bootstrap and the stdlib reference compiles or is a marked fragment of one that does, and five wrong forms are programs paired with the code each draws.

- `glyph llms --negative` answers from the same corpora `cargo test` runs.

### Where it stops

- Two counts of the wrong programs appear in this document and they count different things

  **Details**

  Two counts of the wrong programs appear in this document and they count different things: 51 of the 77 codes carry a counter-example from `tests/negative/`, and `glyph llms --negative` lists 117 cases over 53 codes in 0.1.149 because it reads the refusals in `catches/` as well; the next release adds the 22 refusals added to `catches/` since, for 139 cases over 55 codes.

- In 0.1.149, 24 codes have no wrong program in either corpus, and the answer names them rather than inventing one.
- 165 of the 340 stdlib exports come back with no signature

  **Details**

  165 of the 340 stdlib exports come back with no signature: the implementation is TypeScript that `tsc` checks and the compiler's own tables model nothing for it, so `glyph_symbol` on `std/result::all` or `std/io::println` reports `kind: null` with that reason.

- Of the 175 that do have one, 46 still carry a `?` where a parameter type belongs.
- And the document is a snapshot you fetch, not a service: there is no retrieval layer, no embeddings, and no chat surface over it.

Code examples on this page are checked against Glyph 0.1.158 on every build. Last changed 21 Nov 2026.
