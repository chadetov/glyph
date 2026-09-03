# minesweeper

## What it is

Terminal Minesweeper with a reproducible board: a 9x9 grid, lazy first-click
mine placement so the first revealed cell and its eight neighbours are always
safe, flood-fill reveal, a flag and unflag command loop, and a seeded RNG so a
piped transcript replays byte for byte.

## Running it

```sh
glyph run examples/apps/minesweeper/main.glyph beginner --seed 7
printf 'reveal 4 4\nflag 0 0\nquit\n' | glyph run examples/apps/minesweeper/main.glyph beginner --seed 7
```

## What it exercises

Tagged unions kept deliberately narrow (`Visibility` is only about visibility,
so "revealed" and "mined" cannot be confused), error variants carrying context
such as the legal range, a seeded RNG, and an interactive read loop.

## What it changed in Glyph

The first round pointed at an ordinary program rather than an integration: no
npm dependency, no server, no JSX. Eight gaps came out of it; three shaped this
file directly, and all three are closed.

**G23 (0.1.38): `glyph fmt` relocated any comment written inside a construct.**
The printer flushed pending `//` comments at declaration and statement
granularity only, so a comment written inside a record body, a union variant
list, or an array literal stayed pending and was re-emitted above the next
declaration or statement instead. Nothing warned: exit 0, `tsc` passed, and the
mangled output was a fixed point, so `glyph fmt --check` would have accepted
it. This file has the exact shape that used to trigger it: the `OFFSETS` array
a few lines below `Offset` carries a comment before three of its nine entries
("The row above.", "The same row...", "The row below."), which is an
array-element comment of the kind the bug used to walk out of the literal and
onto an unrelated declaration. Before the fix landed, every field and variant
comment in this file had to be hoisted onto its own declaration line to survive
a format pass. Now they sit exactly where they were written, inside the
`Visibility` union, the `Cell` record, and the `OFFSETS` literal, and
`glyph fmt --check` reports the file already formatted.

**G30, both halves, closed across two releases.** `for` had nothing that
produced a counted range, so the most common bounded loop could not use the
keyword D21 built for bounded loops and had to be hand-rolled from
`loop`/`match`/`break` instead, which costs greppability. `array.range(count)`
and `array.range_from(start, end)` shipped in 0.1.52 and this file was ported
in the same pass: every loop here reads `for i in array.range(n)` or
`array.range_from(lo, hi)`, and the emitted TypeScript did not change.
Separately, `xs[i]` typed as `Unknown`, so `cells[999]` type-checked clean,
passed `tsc --strict`, and handed back `undefined` where the compiler claimed
`Cell`. That half closed in 0.1.70: the emitted read is now bounds-checked and
throws a `RangeError` naming the index and the length instead of returning
`undefined`. This file never depended on that throw, because `cell_at` and
`neighbors` already check `in_bounds` before every index; the fix is a safety
net under code that was already careful.

**G24 (0.1.49): `?` was rejected in an expression-form `match` arm.** `=>
f(x)?` failed to compile while `=> { return f(x)? }` and `=> return
Ok(f(x)?)` both did: one call site in the emitter used `self.expr` where every
other statement-value position used `self.emit_value`, a missed call site
rather than a design decision. 0.1.49 fixed the arm that is the whole value of
a `let`, a `mut`, or a `return`, so the unwrap and its early exit now land
inside the arm's own case. `coord_command` in this file used to work around
the gap: a `Verb` tag, one `match` to read the verb from its name, a second
`match` to run `parse_coord` and hand its `Err` through by hand, and a third
`match` to build the right constructor from the tag, because putting
`parse_coord(...)?` directly in the arm that builds the command was rejected.
That workaround is gone. `coord_command` is now a single `match` on the verb
name with the fallible call inline in each arm:

```glyph
fn coord_command(board: Board, name: string, r: string, c: string) -> Result<Command, CommandError> {
  return match name {
    "reveal" => Ok(Reveal({ at: parse_coord(board, r, c)? })),
    "r" => Ok(Reveal({ at: parse_coord(board, r, c)? })),
    "flag" => Ok(Flag({ at: parse_coord(board, r, c)? })),
    "f" => Ok(Flag({ at: parse_coord(board, r, c)? })),
    "unflag" => Ok(Unflag({ at: parse_coord(board, r, c)? })),
    "u" => Ok(Unflag({ at: parse_coord(board, r, c)? })),
    else => Err(UnknownCommand({ name: name })),
  }
}
```

`verb_of` is gone with it; the one caller that used it only to tell a known
verb apart from an unknown one now calls a small `is_coord_verb(name) -> bool`
instead. Checked before and after: `glyph check .` still exits 0 with the same
one passing `@example`, `glyph fmt --check` reports the file already
formatted, and a piped transcript (`reveal 4 4`, `flag 0 0`, `reveal 1 1`, an
unknown command, a bare `reveal`, `quit`) prints byte-identical output to what
the three-match version produced.

Nothing in the current source is standing in for an open gap. If a future pass
finds one here, it should stay in the file until the gap it names is actually
closed, not before.
