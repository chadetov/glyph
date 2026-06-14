# Glyph benchmarks

The manifesto's empirical claim: *"an agent given the same task produces correct code faster in Glyph than in TypeScript, and the reviewer finishes in half the time."*

This directory instruments that claim from day one (brainstorm Q10 resolution: benchmarks track starts in step 4, not at step 11).

## What's measured

For each function, the same task is implemented in **Glyph, TypeScript, Python, Rust**. Per commit on `main`, the measure script computes:

- **Token count** — primary metric. Lower is denser. (Currently an approximate, dependency-free count; see methodology.)
- **Line count** — secondary metric. Lower is shorter.
- **Diff size** — how localized a controlled edit stays (the diff-stability pillar). The `diff_size` field in the harness is not yet populated; diff stability is instead demonstrated live and measured in the playground (`playground/`), where a one-line Glyph edit produces a one-line TypeScript diff. A cross-language harness metric is future work.

A companion **verifiability** demo lives in `verifiability/`: paired programs where Glyph rejects a bug at compile time that `tsc --strict` accepts (`verifiability/check.sh` asserts the invariant).

Results are written to `results/<timestamp>.json` and checked into git so the trajectory is visible. The synthesized findings are in `FINDINGS.md`.

## The first three functions (Phase 0)

Functions picked to translate naturally across all four languages — no language-specific tricks, no JSX.

| Function | What | Stresses |
|---|---|---|
| `parse_user` | Parse a JSON-shaped record into a typed value or error | Verifiability — runtime descriptors |
| `load_feed` | Async network call with error-as-value handling | Verifiability — `Result` + `?` |
| `slugify` | Lowercase + strip non-alphanumerics + collapse spaces | Greppability — pure transformation |

By end of phase 1 week 8, the set grows to 5–10 functions. By phase 8 (killer demo), 20+.

## Running the benchmarks

```bash
./measure.sh           # measure all functions × all languages
./measure.sh parse_user # measure one function across all languages
```

Output: `results/<timestamp>.json` plus a one-line summary to stdout.

## Methodology notes

- Token counts are an **approximate, dependency-free** proxy (`measure.sh`): each identifier/number run is one token and each standalone symbol is one. This is a stable density proxy, not a tiktoken-exact count. A real tokenizer (e.g. `tiktoken` cl100k_base, or Anthropic's) would shift the absolute numbers but not the relative ranking; wiring one in is a future enhancement.
- Line counts exclude blank lines and comments. The point is to measure semantic density, not coding style.
- Diff stability is currently demonstrated in the playground (a measured one-line-edit → one-line-diff), not yet by this harness; a cross-language formatted-diff metric (apply a controlled edit, reformat with each language's canonical formatter, count changed lines) is the planned `diff_size` implementation.
- Honesty over flattery: these are structural metrics (density, verifiability, diff locality). They are the *drivers* the manifesto bets make agents more productive; the productivity claim itself is a hypothesis to be validated with a real agent study (future work), not asserted here.
