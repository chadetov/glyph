# Step 7 — LSP

Status: **in progress — increment 1 shipped.** Full discussion in `archive/glyph-lsp-discussion.md`.

## Increment 1 (shipped): diagnostics + formatting

The `glyph-lsp` crate (`crates/glyph-lsp/`, tower-lsp + tokio) is launched by
`glyph lsp` over stdio — the transport an editor extension spawns. It serves:

- **Diagnostics** — the compiler front end (parse → resolve → typecheck, with
  stdlib-stub import verification) runs over each open document; every error is
  published with its stable code (`E0xxx`), `help` text, and a UTF-16 line/
  character range, on open/change, and cleared on close. A parse failure
  short-circuits (no AST for later phases).
- **Document formatting** — returns the canonical `glyph fmt` layout as a
  whole-document edit; unparseable source is left untouched, matching the CLI.

The analysis (`analysis.rs`: `analyze` + a UTF-16 `LineIndex`) holds no protocol
types and is unit-tested without an LSP runtime; the diagnostic computation is a
synchronous call that never holds a lock or a non-`Send` value across an
`await`. Verified end to end with a real `initialize`/`didOpen` JSON-RPC
exchange (an `E0210` field-typo squiggle with the right range).

## Increment 2 (shipped): hover + go-to-definition

- **Hover** — the innermost typed expression under the cursor renders its type
  (`Array<number>`, `Result<User, string>`, `{ name: string }`, …) via a public
  `display_ty`, shown as a fenced `glyph` block.
- **Go-to-definition** — the name reference under the cursor resolves to its
  definition: a local binding or a module-level declaration jumps within the
  file; a prelude built-in yields nothing; cross-module targets await workspace
  support. Both reuse the resolution map and type-map side tables (a `TypeMap`
  iterator and a UTF-16 `LineIndex::offset` were added).

Verified end to end: hovering a literal shows `number`, and a call jumps to its
`fn` declaration.

## Increment 3 (shipped): completion

A flat candidate list the editor filters by prefix: Glyph keywords, the open
module's top-level declarations (and a union's variant constructors), and the
prelude names (`Result`/`Ok`/`Option`/`string`/`print`/…), each with an editor
icon kind. It falls back to keywords + prelude when the file does not parse —
exactly when completion matters most (mid-edit). Verified end to end (43
candidates including a module `fn`, a keyword, and prelude `Result`/`Ok`).

**The revised v1 LSP core is complete: diagnostics, hover, go-to-definition,
completion, and format-on-save.** Member completion after `.` is a refinement
left for later.

## Editor client (shipped): VS Code extension

`editors/vscode/` is a minimal VS Code extension (plain CommonJS, no build
step): a TextMate grammar for `.glyph` highlighting plus a Language Client that
launches `glyph lsp` over stdio (configurable via `glyph.serverPath`). `npm
install` then F5 brings up squiggles, hover, completion, and format-on-save.
This makes the language server actually usable in an editor — the "ship the
LSP" bar — and is the editor-support prerequisite for any external trial. Full
activation is verified by launching it in VS Code (cannot be exercised in CI).

## Increment 4 (shipped): document symbols + workspace symbol index (Q12)

- **Document symbols** — `textDocument/documentSymbol` returns the file outline
  (fn/component/type/const, with a union's variants nested), powering the editor
  outline, breadcrumbs, and in-file picker.
- **Workspace symbol index (Q12)** — `workspace/symbol` walks every `.glyph`
  under the workspace root (captured at `initialize`), parses each (parse-only,
  preferring an open buffer over disk), and returns all top-level declarations
  filtered by the query, each with its file location. This answers "what's
  importable from where" — verified across a two-file workspace (`alpha` from one
  file, `Bravo`/`CHARLIE` from another).

## Increment 5 (shipped): cross-module go-to-definition

Go-to-definition on an imported name now jumps into the *target file*: an
`ImportNamed` reference carries its module path and original name, which the
server resolves to a `.glyph` under the workspace root (`sub/b` →
`<root>/sub/b.glyph`, preferring an open buffer over disk) and locates the
declaration in. Within-file definitions still resolve locally; a `std/*` import
(no project source) or unresolved target yields nothing. Verified end to end: a
`greet` call in `app.glyph` jumps to its `fn greet` in `lib.glyph`.

Go-to-definition is now complete (within-file + cross-module). Navigation —
diagnostics, hover, both definition modes, completion, document + workspace
symbols, format-on-save — is the full editor experience minus rename/find-refs.

## Increment 6 (shipped): canonical agent view (Q32, tractable core)

`glyph_formatter::canonical_view(source) -> Result<String, _>` is the
agent-facing rendering of a file: the `glyph fmt` layout, every content line
tagged with a stable `Lddd` number, and a per-declaration content fingerprint
(FNV-1a/64 over the declaration's *canonical* bytes — invariant under
reformatting and whitespace, moving only when the declaration's content does;
the start is pulled back to cover any leading annotations). The line numbers are
decoupled from physical position (the `#`-prefixed fingerprint/header lines sit
*between* numbered lines), giving the future position mapper a stable coordinate.

It is one pure function with two surfaces: the `glyph canonical <file>` CLI
command and a custom LSP request `glyph/canonicalView` (`{ uri }` →
`{ content, error }`). Verified end to end over JSON-RPC (a `fn add` view with
its fingerprint) and by unit tests (numbering, per-decl fingerprints, stability
under reformatting, content-sensitivity, annotation coverage).

**This lands the tractable Q32 core.** The research-heavy parts — SSA-like value
renaming (`$0`, `$1`, …) and the bidirectional text↔canonical *position* mapper
— remain a deliberate v1.1 increment.

**Remaining step-7 v1 scope (the AI-agent-channel pieces — deserve a focused
session, not a tired one):**

- **Q32 — SSA renaming + position mapper.** The canonical view (layout + line
  numbers + per-declaration fingerprints) ships above. What remains is the
  research-heavy tail: SSA-like value names and a bidirectional text↔canonical
  position mapper, so an agent's edit in canonical coordinates maps back to a
  buffer edit. Deliberate v1.1 increment.
- **Q29 — `applyEdit` RPC.** ⚠️ **Needs design reconciliation before building.**
  The sketched `edit { … } @verify { … }` block syntax is on CLAUDE.md's
  explicitly-abandoned "signature-rich direction" list. An `applyEdit` RPC can
  still exist, but its surface must be re-derived in TS-family terms (e.g. a
  plain structured edit + a re-run of the existing `@example`/`--test` gate),
  not the abandoned annotation block. Do not build it as originally sketched.

Rename + find-references remain v1.1. The analysis will move onto the salsa
`glyph-db` queries for incremental multi-file work later (the substrate is
already there).

## Updates from brainstorm session 1 (2026-05-26)

- **Q5 → hybrid compiler architecture.** The typechecker is built around salsa-style queries from day one (in step 4). Step 7's 4-week LSP budget is preserved — diagnostics, hover, and go-to-def reuse the existing compiler queries; no compiler refactor needed before LSP work starts.

## Updates from brainstorm session 2 (2026-05-26)

- **Q32 → LSP exposes virtual agent view.** The LSP must serve a virtual document `agent://file.glyph.canonical` for any open Glyph file. The canonical document has stable line numbers (`L001`, `L002`, ...), SSA-like value names (`$0`, `$1`, ...), and a `@hash:blake3:...` per declaration. Agents query the LSP RPC for the canonical form. **Scope addition for v1**: the canonical-document generator (a normalized AST printer with stable numbering) plus the bidirectional position mapper (text position ↔ canonical position).
- **Likely scope expansion**: if Q29 (structured edit API) lands as Option B (LSP RPC), the LSP also exposes an `applyEdit` operation that accepts `edit { ... } @verify { ... }` blocks and returns `{ ok, rejected, reason }`. Decide Q29 in session 3.
- **Revised estimate**: 4 weeks holds for diagnostics + hover + go-to-def + completion + format-on-save. With Q32's virtual document, realistic estimate is **5–6 weeks**. If Q29 lands as LSP RPC too, **6–8 weeks**.

## Updates from brainstorm session 3 (2026-05-26)

- **Q12 → discoverability via LSP workspace indexing.** "What's importable from where" is answered by the workspace symbol index. No dedicated CLI command needed in v1.
- **Q29 → confirmed as LSP RPC (`applyEdit`).** Agents call the LSP with an `edit { ... } @verify { ... }` block; the LSP either applies it atomically or returns a structured rejection (`{ ok: false, failed: "all_tests_pass", counterexamples: [...] }`). This is the workflow that makes "the agent broke the file" structurally impossible.
- **Estimate confirmed at 5–6 weeks** (with Q29 lands in v1, possibly stretching to 6–8 if `applyEdit` semantics get complicated).

**Step 7 v1 deliverable list:**
1. Diagnostics (Elm-quality bar, from Q6)
2. Hover types
3. Go-to-definition (cross-module via D15 full-path imports)
4. Completion
5. Format-on-save (calls `glyph fmt`)
6. Virtual document `agent://file.glyph.canonical` (Q32)
7. `applyEdit` RPC for structured edits (Q29)
8. Workspace symbol index for discoverability (Q12)

Rename and find-references stay deferred to v1.1 (the original rescoping holds).

## What the original strategy said

> Ship the LSP (4 weeks). Non-negotiable. Diagnostics, go-to-definition, hover types, autocomplete, rename, find-references. Use tower-lsp if you're in Rust. Make it fast.

## What's right about it

No LSP, no adoption. TypeScript developers don't evaluate languages by reading manifestos — they install the extension, type three characters, and judge on whether the completion popup feels alive. If hover doesn't show types in under ~50ms they assume the project is unserious. `tower-lsp` is the right choice if the compiler is Rust; don't reinvent the protocol layer.

## What's wrong about it

**Four weeks is a fantasy budget for that feature list.** Diagnostics and hover are achievable. Go-to-definition across modules with the full-path import rule is actually easier than in TS because barrel files are banned. But **rename** and **find-references** done correctly require a project-wide symbol index with incremental updates, and **incremental compilation is the part that eats months, not weeks**. If rename ships in four weeks it will be textual and will corrupt code on the first edge case — which is worse than not shipping it at all.

**"Make it fast" is not a spec.** The number that matters for TS-developer retention is keystroke-to-diagnostic latency on a warm file. Without a budget, the team ships something that feels fast on the maintainer's M3 Max and dies on a reviewer's work laptop.

**Formatter-on-save is missing from the original list and belongs in v1.** Given that diff stability is one of the four pillars, an LSP that doesn't format on save out of the box undermines the pillar on day one. Cheap to ship; the demo value is enormous — a TS developer pastes messy code, hits save, watches it snap into Glyph's fixed-width layout, and that's the moment they get the manifesto without reading it.

## Revised v1 scope

Ship in 4 weeks (or honestly slip to 8):

- **Diagnostics**
- **Hover types**
- **Go-to-definition**
- **Completion**
- **Format-on-save**

**Defer to v1.1:** rename, find-references. Called out explicitly in the launch communication.

## Launch gates (concrete, not "fast")

- **p95 < 100ms** diagnostics on a warm file under 1000 lines.
- **p95 < 30ms** completion.

Gate the launch on hitting them. If the build can't, push the four weeks to eight rather than ship something TS developers judge harshly — which, as the original framing correctly notes, they will.

## Load-bearing prerequisite

**The LSP is downstream of the compiler's query architecture.** If the compiler isn't built around incremental, demand-driven queries (salsa-style), then "ship the LSP in 4 weeks" is really "rewrite the compiler in 4 weeks *and* ship an LSP."

The step-4 transpiler plan currently says **"dumb visitor, no IR."** That position is inconsistent with what step 7 needs. One of the two has to give:

- Adopt salsa-style queries in step 4 from day one (more upfront work, transpiler ships later, LSP timeline plausible).
- Or accept that step 4 ships fast on a dumb visitor and step 7 is **8–12 weeks**, not 4, with the first few weeks being a compiler refactor into incremental queries.

This is the single biggest risk to the launch date. Tracked as blocker in `open-questions.md`.

## Connection to step 5 (typechecker)

The error-message bar — tsc-quality vs Elm-quality — is also a step-7 concern, because LSP diagnostics *are* the error messages. Decide the bar in step 5; pay for it in step 7.
