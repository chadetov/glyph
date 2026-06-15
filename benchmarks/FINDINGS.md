# Glyph Benchmark Findings

## Thesis

Glyph is a statically typed language that transpiles to readable TypeScript, designed so AI agents can read, write, and modify code safely. The benchmarks below show that Glyph is denser than equivalent TypeScript while rejecting, at compile time, two classes of bug that `tsc --strict` accepts and that agents are known to introduce.

These are structural measurements of the language and toolchain, not a study of agent behavior. What they support, and what they do not, is stated explicitly in the final section.

## 1. Density

The same task was implemented in Glyph, TypeScript, Python, Rust, and Go, and each implementation was measured with a real LLM tokenizer (`benchmarks/measure.sh`, which calls `count_tokens.py` using tiktoken's `cl100k_base` encoding). Lower is denser.

| Function     | Glyph | TypeScript | Python | Rust |  Go |
|--------------|------:|-----------:|-------:|-----:|----:|
| `load_feed`  |   166 |        257 |    242 |  323 | 346 |
| `parse_user` |   125 |        174 |    152 |  168 | 131 |
| `slugify`    |    50 |         47 |     41 |  129 |  87 |
| **Total**    | **341** |    **478** |  **435** | **620** | **564** |

Across the three functions, Glyph uses about 29% fewer tokens than the equivalent TypeScript (341 vs 478), about 22% fewer than Python, about 40% fewer than Go, and about 45% fewer than Rust, while remaining fully statically typed. The ranking by density is Glyph, Python, TypeScript, Go, Rust. Python is the only language that beats TypeScript here, and it is not statically typed; among the four statically typed languages Glyph is the densest by a wide margin, and Go and Rust — the other languages with explicit, checkable error handling — are the longest.

The advantage scales with the amount of real logic. On the two functions with actual control flow Glyph is well ahead (`load_feed` 166 vs 257, `parse_user` 125 vs 174); on the trivial `slugify` it is marginally *larger* than TypeScript and Python (50 vs 47 and 41), because its `module` declaration and `import` line are fixed overhead that a three-line function cannot amortize. Density is a property of expressing real work, not of one-liners.

Line counts (excluding blank lines and comments) follow the same ordering on the totals: Glyph 46, TypeScript 55, Python 57, Rust 67, Go 81.

**Caveat.** These are real `cl100k_base` token counts, not a proxy. A different encoding (o200k_base, or Anthropic's tokenizer) would shift the absolute numbers, but the cross-language ranking is driven by structure and is not sensitive to the choice. Only three functions have been measured so far (`parse_user`, `load_feed`, `slugify`), so these totals describe a small sample, not a representative corpus.

## 2. Verifiability

The `benchmarks/verifiability/` directory contains paired programs. In each pair, Glyph rejects a bug at compile time that `tsc --strict` accepts. The pairs are asserted by `benchmarks/verifiability/check.sh`.

### Exhaustiveness

An agent adds a new variant (`Triangle`) to a union and forgets to handle it in a `match`.

- **Glyph** rejects the program at compile time with `E0200`: `non-exhaustive match on Shape: missing variants Triangle`.
- **TypeScript** compiles the equivalent `switch` clean under `tsc --strict` and silently returns `0` for the `Triangle` case. TypeScript has no built-in exhaustiveness checking; the `assertNever` idiom is manual, and an agent that forgets the new case also forgets the guard.

### Unsafe cast

An agent treats an untrusted value as a known type.

- **Glyph** has no cast expression. `input as User` does not compile. There is no escape hatch: an unknown value must be validated through an `is`-match or through `json.parse` against a schema, which returns a `Result`.
- **TypeScript** compiles `input as User` clean under `tsc --strict` and throws at runtime if `input` is not actually a `User`.

Both demos are the same task in both languages, differing only in the safety the language enforces. Run `benchmarks/verifiability/check.sh` to reproduce the assertions.

These two demos do not show that Glyph catches every type error TypeScript misses. Some v1 typechecker checks are deferred to v1.1 (a fuller unifier among them). The claim is narrow: these two specific, common agent mistakes are rejected by Glyph and accepted by `tsc --strict`.

## 3. Diff stability

Diff stability is the property that a small semantic change produces a small textual diff, with no incidental churn. It is measured on Glyph's own pipeline by `benchmarks/diff_stability.sh`: for each controlled one-line edit to `diff_stability/pricing.glyph`, the harness rebuilds the program through the real transpiler and counts the changed lines in the emitted TypeScript.

| One-line Glyph edit            | Glyph source diff | Emitted TypeScript diff |
|--------------------------------|------------------:|------------------------:|
| per-seat price `12` → `10`     |        1 line     |          1 line         |
| plan name `Starter` → `Solo`   |        1 line     |          1 line         |
| seat count `5` → `9`           |        1 line     |          1 line         |

In every case a one-line edit maps to exactly one changed line downstream: the transpiler does not amplify a small change into a large diff, and `glyph fmt` is idempotent on the result (re-running it changes nothing). This is not a property of the cross-language formatter race, which is uninformative: modern formatters (Prettier, Black, rustfmt) are themselves diff-stable, so that comparison would mostly be a tie. The meaningful, Glyph-specific guarantee is that the source-to-TypeScript pipeline preserves edit locality. The toolchain rules that make this hold:

- One fixed formatting layout, produced by `glyph fmt`.
- Required trailing commas.
- One element per line past two elements.
- No line-length reflow.
- No barrel files.

Together these remove the usual sources of incidental churn (reflowed lines, shifting commas, reordered re-exports), so a small semantic change maps to a small textual diff. Run `benchmarks/diff_stability.sh` to reproduce the table.

## 4. What this proves, and what it does not

### What it shows

- For the three measured functions, Glyph is denser than the equivalent statically typed TypeScript by a real `cl100k_base` token count (~29% fewer tokens on the totals).
- Glyph rejects two specific, common agent mistakes (a missing union variant, an unsafe cast) that `tsc --strict` accepts. Both are reproducible via `check.sh`.
- A one-line Glyph edit produces a one-line diff in the emitted TypeScript across the measured edits, and `glyph fmt` adds no churn. Reproducible via `diff_stability.sh`.

### What it does not show

- **It does not prove agents write correct code faster in Glyph.** That is a hypothesis these structural metrics support, not a measured result. It has not been tested with a real agent study, and no speedup figure is claimed.
- **The density sample is small.** Three functions have been measured. The token counts are real (`cl100k_base`), but three functions are not a representative corpus, and a different encoding would move the absolute numbers (not, we expect, the ranking).
- **The verifiability result is narrow.** Glyph is not claimed to catch every type error TypeScript misses, only the two demonstrated cases. Some v1 typechecker checks are deferred to v1.1.
- **Diff stability is measured on Glyph's own pipeline, not cross-language.** The claim is that a Glyph edit stays localized through transpilation, on one small fixture. It is not a claim that Glyph diffs are smaller than well-formatted TypeScript diffs (they are typically comparable).

Glyph is early (v0.1). These findings are the current, honest state of the evidence; they are meant to be re-run and extended, not taken as final.

Repository: github.com/chadetov/glyph. License: MIT OR Apache-2.0.
