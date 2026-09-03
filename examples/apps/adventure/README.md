# adventure

## What it is

A ten-room text adventure. The world is a keyed store: `World` holds
`rooms: Record<string, Room>` and every exit names a room id rather than
embedding a `Room`, so each room exists exactly once. It parses free-form stdin
into a `Command` union, applies state-dependent rules (the cellar is dark until
the lantern is lit, exits can be locked behind an inventory item), and writes a
save file that has to validate back into a `World`.

## Running it

```sh
glyph run examples/apps/adventure/main.glyph

# plays deterministically to completion:
printf 'north\ntake lantern\ndown\nlook\nquit\n' | glyph run examples/apps/adventure/main.glyph
```

## What it found, and what happened

Shipped **0.1.40**, and it found a bug in the command everyone runs, plus one
that is still open.

**G38, fixed.** `glyph run` computed a full build report and only ever read
`report.emitted` off it, so `report.diagnostics` and `report.error_count` never
reached anyone. `glyph run solo.glyph` printed the program's output and exited 0
while `glyph build .` on the identical tree printed E0204 on a sibling and
E0106 on the file itself, seconds apart, same compiler, same source. Fixed in
0.1.40: `run_file` now returns every diagnostic alongside the outcome, `glyph
run` prints them before dispatching, and a build writes them to
`.glyph-diagnostics.json` in the staging directory so a warm cache still
reports what it found instead of going quiet on the second run. There is
nothing left in this app's source that relates to G38; it was a bug in the
runner, not something the app's code had to work around.

**G39, half fixed.** Member access and call arguments against a receiver typed
`Ty::Unknown` went unchecked: a misspelled `xs.pusj(x)` or a wrong-arity stdlib
call both compiled clean. Phase 1 (0.1.71) modeled optional arguments for six
stdlib functions (`string.index_of`, `string.slice`, `string.pad_start`,
`string.pad_end`, `array.slice`, `json.stringify`), so a `match` on one of
those with a missing arm is now a compile-time E0200 instead of a runtime
throw. What phase 1 did not touch is still true: a receiver that is genuinely
`Ty::Unknown` (an unmodeled stdlib call, a misspelled method) still gets no
member-access or arity checking at all. This app does not call anything left
unmodeled by phase 1, so nothing in `main.glyph` is written around the gap; the
finding came from probing the compiler with stdlib calls during this app's
dogfooding round, not from a pattern this app's code needed to survive. The
risk G39 names is still real for any Glyph program calling a stdlib function
outside that six-function list: it just is not one this file happens to call.

It was also one of three apps that became interactive with no source change when
`io.read_line` was fixed in 0.1.62: before that it could not answer `look` while
you typed.

## What it exercises

Five tagged unions matched exhaustively, `Record<string, T>` as the keyed world
store, `Option<string>`, and a descriptor round-trip at the save boundary. Five
`@example` rows. No generics, no async.
