# Fuzzing

Three targets, run with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz).
The crate is outside the workspace on purpose: it needs nightly and libfuzzer,
and a workspace member that does not build on the pinned stable toolchain would
break `cargo test --workspace` for everyone.

```sh
cargo +nightly fuzz run parse            fuzz/corpus/parse            fuzz/seeds -- -max_total_time=60
cargo +nightly fuzz run lex              fuzz/corpus/lex              fuzz/seeds -- -max_total_time=60
cargo +nightly fuzz run format_idempotent fuzz/corpus/format_idempotent fuzz/seeds -- -max_total_time=60
```

The first directory is the working corpus the fuzzer writes to; the second is
read-only input. `corpus/` is created on first run.

## The targets

**`parse`** and **`lex`** assert only that the function returns. The parser is
the one component reachable from bytes nobody wrote by hand, since an editor, an
LSP client and an agent all hand it arbitrary text. A rejection is a value; a
panic is a crashed language server.

**`format_idempotent`** checks a property rather than the absence of a panic,
and it is the one worth the most. Formatting twice must equal formatting once.
Diff stability is a pillar, and a formatter that is not idempotent makes a file
oscillate between two spellings, so every save is a diff and `glyph fmt --check`
passes on one of them and fails on the other. G23 was exactly this shape: the
formatter moved comments out of the construct they documented, and the mangled
output was itself a fixed point, so nothing downstream noticed.

## What is committed, and what is not

`seeds/` holds 40 real programs from `examples/`, and it is committed. It is the
starting point, not the working set.

`corpus/` is what the fuzzer grows, and it is not committed. It reached 11 MB
across the three targets within a few minutes of the first run, and even after
`cargo fuzz cmin` it was 11 MB, which does not belong in a repository people
clone to build a compiler. It is reproducible from `seeds/` at any time.

What does get committed is a crash. A crash without a committed reproduction is
a crash that comes back, so when the fuzzer finds something: minimize it with
`cargo fuzz tmin <target> <input>`, commit the minimized input under
`seeds/` with a name saying what it broke, and file it in
`docs/dogfooding-gaps.md` like any other finding. `artifacts/` holds the raw
crash input and is not committed, because the minimized one is the useful one.

## Baseline

At 0.1.95, seeded from real programs: `parse` 251k executions, `lex` 223k,
`format_idempotent` 309k. No crashes and no idempotency violations.
That is a floor, not a proof: these targets take raw bytes, so most inputs are
rejected early, and a structure-aware generator would reach deeper.
