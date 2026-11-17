# Fuzzing

Four targets, run with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz).
The crate is outside the workspace on purpose: it needs nightly and libfuzzer,
and a workspace member that does not build on the pinned stable toolchain would
break `cargo test --workspace` for everyone.

```sh
cargo +nightly fuzz run parse            fuzz/corpus/parse            fuzz/seeds -- -max_total_time=60
cargo +nightly fuzz run lex              fuzz/corpus/lex              fuzz/seeds -- -max_total_time=60
cargo +nightly fuzz run format_idempotent fuzz/corpus/format_idempotent fuzz/seeds -- -max_total_time=60
cargo +nightly fuzz run format_preserves_emit fuzz/corpus/format_preserves_emit fuzz/seeds -- -max_total_time=60
```

The first directory is the working corpus the fuzzer writes to; the second is
read-only input. `corpus/` is created on first run.

## The targets

**`lex`** asserts only that the lexer returns, for the token loop and for the
JSX text scanner. **`parse`** asserts that the parser returns, and two things
about what it returns. The error it reports never starts after the first lex
error of the lexer it drives: the source is lexed with every JSX text run the
parser scanned blanked out, because text is scanned raw and an eager lex of the
whole file would fail on a contraction the parser reads as text. And every JSX
text child is exactly the source at its span. The parser is the one component
reachable from bytes nobody wrote by hand, since an editor, an LSP client and an
agent all hand it arbitrary text. A rejection is a value; a panic is a crashed
language server.

**`format_idempotent`** checks a property rather than the absence of a panic,
and it is the one worth the most. Formatting twice must equal formatting once.
Diff stability is a pillar, and a formatter that is not idempotent makes a file
oscillate between two spellings, so every save is a diff and `glyph fmt --check`
passes on one of them and fails on the other. G23 was exactly this shape: the
formatter moved comments out of the construct they documented, and the mangled
output was itself a fixed point, so nothing downstream noticed.

Before either, it checks that the output says what the input did: the
formatted text parses to the same AST as the source, spans aside, along with
the few things the layout changes by design (annotation order, the whitespace
of JSX text). A first pass that changes the program can be a fixed point.
G335 printed `(load()?).run()` after `let a = 1`, which reads as a call on
`1`, and G338 deleted a union's `where` refinement; text idempotence found the
first by luck and never saw the second. The comparison lives in
`glyph-formatter/tests/common/ast_shape.rs`, which the target includes by
path and the corpus test in `tests/format.rs` runs over every seed, example
and catch.

It also holds the ordered literals and comments to what the source wrote. JSX
text counts there as the text it renders, the same item as a string child that
renders it, so a formatter that changes what a page says fails it.

Both properties are asked of a file `glyph fmt` formats. Since 0.1.137 it can
refuse one instead, when a comment sits where the canonical layout has no
place for it (D14), and a refusal has a property of its own: the input holds a
comment, and the same input with every comment blanked out formats. That is
what keeps a refusal from being a way out of the other two (record 86). The
formatter's own output is never refused.

**`format_preserves_emit`** asserts that formatting leaves the emitted
TypeScript unchanged, byte for byte. G316 needed it: `glyph fmt` wrote JSX text
ending in four spaces as the text and a one-space `{" "}`, a fixed point that
changed the program. It runs the resolver and the checker on fuzzed input, so it
is a target of its own; a panic in either fails here rather than hiding every
formatter check in `format_idempotent`.

## What is committed, and what is not

`seeds/` holds real programs from `examples/` and the minimized reproductions of fuzz findings, and it is committed. It is the
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

At 0.1.133, with JSX text in the stream and the emit target added, local runs
stopped at their first failure: `format_idempotent` after about 263k
executions on G318 (`//` in JSX text read as a comment) and
`format_preserves_emit` after about 314k on G319 (`}` in JSX text). Both are
filed and are fixed by the JSX-text scanner planned for 0.1.135; the nightly
fails on them until then, and fork mode keeps it fuzzing past them.
