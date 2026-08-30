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

## What it changed in Glyph

Shipped **0.1.40**, and it found a bug in the command everyone runs.

**G38: `glyph run` computed a build report and threw its diagnostics away.** On
the success path the runner read `report.emitted` and nothing else, so
`report.diagnostics` and `report.error_count` never reached anyone.
`glyph run solo.glyph` printed the program's output and exited 0 while
`glyph build .` on the identical tree printed E0204 on a sibling and E0106 on
the file itself. A build now writes its diagnostics into the staging directory
so a warm cache still reports them.

**G39: member access and call arguments against `Ty::Unknown` were unchecked.**
A misspelled `xs.pusj(x)` and a wrong-arity stdlib call both compiled. The
manifesto promises no `any`; this was one, spelled `Unknown`, at the boundary
where the promise is made. Half closed in 0.1.71.

It was also one of three apps that became interactive with no source change when
`io.read_line` was fixed in 0.1.62: before that it could not answer `look` while
you typed.

## What it exercises

Five tagged unions matched exhaustively, `Record<string, T>` as the keyed world
store, `Option<string>`, and a descriptor round-trip at the save boundary. Five
`@example` rows. No generics, no async.
