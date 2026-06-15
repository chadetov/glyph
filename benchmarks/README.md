# Glyph benchmarks

The manifesto's empirical claim: *"an agent given the same task produces correct code faster in Glyph than in TypeScript, and the reviewer finishes in half the time."*

This directory instruments that claim from day one (brainstorm Q10 resolution: benchmarks track starts in step 4, not at step 11).

## What's measured

For each function, the same task is implemented in **Glyph, TypeScript, Python, Rust, Go**. Per commit on `main`, the measure script computes:

- **Token count** — primary metric. Lower is denser. A real LLM token count (tiktoken `cl100k_base`) when tiktoken is installed, falling back to a dependency-free proxy otherwise; the `tokenizer` field in each result records which was used. See methodology.
- **Line count** — secondary metric. Lower is shorter.
- **Diff size** — how localized an edit stays (the diff-stability pillar), measured by `diff_stability.sh` on Glyph's own pipeline: a controlled one-line Glyph edit is rebuilt through the transpiler and the changed lines in the emitted TypeScript are counted. A cross-language formatter race is deliberately *not* used: modern formatters are themselves diff-stable, so it would be uninformative.

A companion **verifiability** demo lives in `verifiability/`: paired programs where Glyph rejects a bug at compile time that `tsc --strict` accepts (`verifiability/check.sh` asserts the invariant).

Results are written to `results/<timestamp>.json` and checked into git so the trajectory is visible. The synthesized findings are in `FINDINGS.md`.

## The first three functions (Phase 0)

Functions picked to translate naturally across all five languages — no language-specific tricks, no JSX.

| Function | What | Stresses |
|---|---|---|
| `parse_user` | Parse a JSON-shaped record into a typed value or error | Verifiability — runtime descriptors |
| `load_feed` | Async network call with error-as-value handling | Verifiability — `Result` + `?` |
| `slugify` | Lowercase + strip non-alphanumerics + collapse spaces | Greppability — pure transformation |

By end of phase 1 week 8, the set grows to 5–10 functions. By phase 8 (killer demo), 20+.

## Running the benchmarks

```bash
pip install tiktoken    # for real token counts (otherwise the proxy is used)

./measure.sh            # density: tokens + lines, all functions × all languages
./measure.sh parse_user # density for one function across all languages
./diff_stability.sh     # diff stability: one-line edit → emitted-TS diff
```

`measure.sh` writes `results/<timestamp>.json`; `diff_stability.sh` writes `results/diff-stability-<timestamp>.json`. Both also print a summary to stdout.

## Methodology notes

- Token counts are **real LLM tokens** when tiktoken is installed (`count_tokens.py`, `cl100k_base`), and fall back to a dependency-free proxy (each identifier/number run and each standalone symbol is one token) otherwise. The `tokenizer` field in each result file records which produced the numbers. A different encoding (o200k_base, or Anthropic's) shifts the absolute counts but not the cross-language ranking, which is driven by structure.
- Line counts exclude blank lines and comments. The point is to measure semantic density, not coding style.
- Diff stability is measured on Glyph's own pipeline (`diff_stability.sh`): a controlled one-line Glyph edit is rebuilt through the transpiler and the changed lines in the emitted TypeScript are counted, confirming the pipeline does not amplify a small edit and that `glyph fmt` adds no churn. A cross-language formatter race is intentionally avoided as uninformative (Prettier, Black, and rustfmt are already diff-stable).
- Honesty over flattery: these are structural metrics (density, verifiability, diff locality). They are the *drivers* the manifesto bets make agents more productive; the productivity claim itself is a hypothesis to be validated with a real agent study (future work), not asserted here.
