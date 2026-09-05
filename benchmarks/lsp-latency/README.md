# Editor latency

What `glyph lsp` makes a person wait for, measured over the real protocol
rather than by timing a function inside the compiler. `measure.py` is a client
speaking LSP framing over stdio to a `glyph lsp` child process, and every number
here is the round trip a keystroke actually takes.

```bash
python3 benchmarks/lsp-latency/measure.py
python3 benchmarks/lsp-latency/measure.py --binary /path/to/older/glyph --label baseline
```

Each run writes `benchmarks/results/lsp-latency-<label>-<timestamp>.json` and
prints a table.

## What it measures

| scenario | what the editor is doing |
|---|---|
| `keystroke` | `didChange` with one more character, timed until the `publishDiagnostics` carrying the matching version |
| `burst` | the same `didChange`, then hover, definition and document symbols over the new text, timed until the last of the four lands |
| `references` | a workspace-wide find-references, repeated over a buffer nobody touched |

`keystroke` and `burst` sweep three files at 535, 1,652 and 2,205 lines
(`examples/corpus/json_parser.glyph`, `examples/apps/sheet/main.glyph`,
`examples/apps/minilang/main.glyph`) so growth with file size is visible.
`references` runs against `examples/apps/csvql`, eleven files, and fails rather
than reports if the answer comes back from a single file, because a
workspace-wide query that never leaves the open document measures nothing about
a workspace.

Ten warm-up iterations then 100 measured, spread over three server processes.
The median is the headline; the mean, p95, min and max are in the JSON.

## Two choices worth knowing about

**The edit goes inside the last declaration, not past the end of the file.**
Both are real keystrokes, but a trailing edit changes no declaration, so a
per-declaration memo layer reuses everything and reports a latency nobody will
ever experience. Typing changes one declaration, so the harness does too.

**Run both binaries in one sitting.** A wall-clock number from another session
under another machine load is not comparable, and this lane has already recorded
one measurement that took an excavation to reproduce. Build the binary you want
to compare against into its own tree, then run the script twice with different
`--label` values.

## The overlay measurement (0.1.115)

The language server's own `CompilerDb`, fed by the editor's buffers, against the
server that re-ran the front end on every request. Both binaries built from this
repository, `6f6c81d` and `e40cb9c`, measured back to back. Medians, in
milliseconds:

| scenario | file | before | after | cut |
|---|---|---|---|---|
| keystroke | json_parser (535) | 1.93 | 1.35 | 30% |
| keystroke | sheet (1,652) | 10.13 | 8.27 | 18% |
| keystroke | minilang (2,205) | 15.96 | 13.78 | 14% |
| burst | json_parser | 6.86 | 1.93 | 72% |
| burst | sheet | 38.45 | 10.33 | 73% |
| burst | minilang | 64.38 | 16.97 | 74% |
| references | csvql (11 files) | 13.96 | 1.44 | 90% |

The burst is where the reuse shows: four requests over one unchanged buffer used
to cost four analyses and now cost one. The keystroke gains least because it is
the request that genuinely has new text to analyse.

The one number that moved the wrong way is the keystroke's growth exponent,
n^1.49 before and n^1.64 after, because the smallest file gains proportionally
more than the largest. Every size measured is faster than before.
