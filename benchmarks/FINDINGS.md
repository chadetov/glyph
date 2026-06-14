# Glyph Benchmark Findings

## Thesis

Glyph is a statically typed language that transpiles to readable TypeScript, designed so AI agents can read, write, and modify code safely. The benchmarks below show that Glyph is denser than equivalent TypeScript while rejecting, at compile time, two classes of bug that `tsc --strict` accepts and that agents are known to introduce.

These are structural measurements of the language and toolchain, not a study of agent behavior. What they support, and what they do not, is stated explicitly in the final section.

## 1. Density

The same task was implemented in Glyph, TypeScript, Python, and Rust, and each implementation was measured with an approximate, dependency-free token proxy (`benchmarks/measure.sh`). Lower is denser.

| Function     | Glyph | TypeScript | Python | Rust |
|--------------|------:|-----------:|-------:|-----:|
| `load_feed`  |   174 |        263 |    207 |  330 |
| `parse_user` |   144 |        181 |    141 |  176 |
| `slugify`    |    50 |         56 |     60 |  143 |
| **Total**    | **368** |    **500** |  **408** | **649** |

Across the three functions, Glyph uses about 26% fewer tokens than the equivalent TypeScript (368 vs 500), about 43% fewer than Rust, and about 10% fewer than Python, while remaining fully statically typed. Python, the only other language in the set that beats TypeScript on density, is not statically typed.

Line counts (excluding blank lines and comments) follow the same ordering: Glyph 46, TypeScript 55, Python 57, Rust 67.

**Caveat.** The token metric is an approximate proxy, not a real tokenizer such as tiktoken. A real tokenizer would shift the absolute numbers; we expect it to leave the ranking intact, but that is an expectation, not a measured result. Only three functions have been measured so far (`parse_user`, `load_feed`, `slugify`), so these totals describe a small sample, not a representative corpus.

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

Diff stability is demonstrated live in the in-browser playground (`playground/`). Editing a single Glyph value, a per-seat price from `12` to `10`, produces a one-line TypeScript diff: minus one line and plus one line on each side, with nothing else changed.

Only this one edit has been measured, but it is not a coincidence of the example: the toolchain's structural guarantees make small, localized diffs the expected behavior. Those guarantees are:

- One fixed formatting layout, produced by `glyph fmt`.
- Required trailing commas.
- One element per line past two elements.
- No line-length reflow.
- No barrel files.

Together these remove the usual sources of incidental churn (reflowed lines, shifting commas, reordered re-exports), so a small semantic change maps to a small textual diff. A cross-language diff harness that measures this systematically is future work; today the claim rests on the playground demonstration and the formatting rules that make it hold.

## 4. What this proves, and what it does not

### What it shows

- For the three measured functions, Glyph is denser than the equivalent statically typed TypeScript by the approximate proxy metric.
- Glyph rejects two specific, common agent mistakes (a missing union variant, an unsafe cast) that `tsc --strict` accepts. Both are reproducible via `check.sh`.
- A small Glyph edit produces a small, localized TypeScript diff under the toolchain's fixed formatting rules.

### What it does not show

- **It does not prove agents write correct code faster in Glyph.** That is a hypothesis these structural metrics support, not a measured result. It has not been tested with a real agent study, and no speedup figure is claimed.
- **The token numbers are approximate.** They come from a dependency-free proxy, not a real tokenizer. The ranking is expected to be robust; the absolute values are not authoritative.
- **The density sample is small.** Three functions have been measured.
- **The verifiability result is narrow.** Glyph is not claimed to catch every type error TypeScript misses, only the two demonstrated cases. Some v1 typechecker checks are deferred to v1.1.
- **Diff stability has no cross-language harness yet.** It is demonstrated in the playground and backed by the formatting guarantees, but not yet measured systematically against other languages.

Glyph is early (v0.1). These findings are the current, honest state of the evidence; they are meant to be re-run and extended, not taken as final.

Repository: github.com/chadetov/glyph. License: MIT OR Apache-2.0.
