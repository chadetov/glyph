# Glyph Launch Plan — Item 7: Ship the LSP

## The original item

> **7. Ship the LSP (4 weeks).** Non-negotiable. Diagnostics, go-to-definition, hover types, autocomplete, rename, find-references. Use tower-lsp if you're in Rust. TS developers will not adopt anything without this and they will judge it harshly. Make it fast.

## Response

The instinct is right and the framing is wrong in two places.

### What's right

No LSP, no adoption. TypeScript developers don't evaluate languages by reading manifestos; they install the extension, type three characters, and judge you on whether the completion popup feels alive. If hover doesn't show types in under ~50ms they assume the whole project is unserious. `tower-lsp` is the correct choice if the compiler is Rust; don't reinvent the protocol layer.

### What's wrong, or at least underspecified

**Four weeks is a fantasy budget for that feature list.** Diagnostics and hover, sure. Go-to-definition across modules with the full-path import rule, fine — actually easier than in TS because barrel files are banned. But **rename** and **find-references** done correctly require a project-wide symbol index with incremental updates, and incremental compilation is the part that eats months, not weeks. If rename ships in four weeks it will be textual and will corrupt code on the first edge case, which is worse than not shipping it.

Cut rename and find-references from the four-week target. Ship them in a v1.1 once incremental compilation is real. Diagnostics + hover + go-to-def + completion is already an ambitious month.

**"Make it fast" is not a spec.** The number that matters for TS-developer retention is keystroke-to-diagnostic latency on a warm file. Set a budget — I'd argue p95 under 100ms for diagnostics on a file under 1000 lines, p95 under 30ms for completion — and gate the launch on hitting it. "Fast" without a number means the team ships something that feels fast on the maintainer's M3 Max and dies on a reviewer's work laptop.

**One item missing from the list that belongs above rename: formatter-on-save integration.** Given that diff stability is one of the four pillars, an LSP that doesn't format on save out of the box undermines the pillar on day one. This is cheap to ship and the demo value is enormous — a TS developer pastes messy code, hits save, watches it snap into Glyph's fixed-width layout, and that's the moment they get the manifesto without reading it.

**One assumption worth interrogating:** the LSP is downstream of the compiler's query architecture. If the compiler isn't already built around incremental, demand-driven queries (salsa-style), then "ship the LSP in 4 weeks" is really "rewrite the compiler in 4 weeks and also ship an LSP." Worth confirming where the compiler stands before committing to the timeline — this is the single biggest risk to the date.

## Recommendation

Keep item 7, but:

- **Tighten the scope** to diagnostics + hover + go-to-def + completion + format-on-save
- **Defer rename and find-references** to v1.1, called out explicitly in the launch communication
- **Attach latency numbers** as launch gates (p95 < 100ms diagnostics, p95 < 30ms completion)
- **Verify the compiler can support incremental queries** before the clock starts

If any of those four can't be true, the honest move is to push the four weeks to eight rather than ship something TS developers will judge harshly — which, as the original item correctly notes, they will.
