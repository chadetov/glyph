# Glyph benchmarks

The manifesto's empirical claim: *"an agent given the same task produces correct code faster in Glyph than in TypeScript, and the reviewer finishes in half the time."*

This directory instruments that claim from day one (brainstorm Q10 resolution: benchmarks track starts in step 4, not at step 11).

## What's measured

For each function, the same task is implemented in **Glyph, TypeScript, Python, Rust**. Per commit on `main`, the measure script computes:

- **Token count** — primary metric. Lower is denser.
- **Line count** — secondary metric. Lower is shorter.
- **Diff size** — measures a controlled edit (add a field to a record, add an error variant). Lower is more diff-stable.

Results are written to `results/<commit-sha>.json` and checked into git so the trajectory is visible.

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

- Token counts use OpenAI's `tiktoken` (cl100k_base) as the canonical tokenizer. Anthropic's tokenizer would shift the absolute numbers but not the relative ranking.
- Line counts exclude blank lines and comments. The point is to measure semantic density, not coding style.
- Diff size is measured by `git diff --stat` after applying a controlled edit defined in `edits/<function>.patch`.
- Each implementation must pass equivalent unit tests (`tests/<function>.json` with input/expected pairs).
