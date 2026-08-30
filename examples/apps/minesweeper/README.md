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

## What it changed in Glyph

The first round pointed at an ordinary program rather than an integration: no
npm dependency, no server, no JSX. Eight gaps came out of it.

**G23 (0.1.38): `glyph fmt` relocated any comment written inside a construct.**
One pass over a nine-line file produced three corruptions, including an
array-element comment that escaped its `const` and landed above an unrelated
type, where it reads as that type's documentation. Nothing warned: exit 0, `tsc`
passed, and the mangled output is a fixed point, so `glyph fmt --check` in CI
accepts it. The app had to hoist every field comment out of its records to
survive a format.

**G30** has two halves. There was no counted range, so the most common bounded
loop could not use the keyword D21 built for bounded loops and got hand-rolled,
which costs greppability. And `xs[i]` typed as `Unknown`, so `cells[999]`
type-checked clean, passed `tsc --strict`, and handed back `undefined` where the
compiler claimed `Cell`. Closed in 0.1.70 by bounds-checking the emitted read,
after measuring `noUncheckedIndexedAccess` at 428 errors across the tree.

**G24 is still open and still visible here.** `?` is rejected in an
expression-form match arm, which is why a separate two-step parse exists in the
command parser.

## What it exercises

Tagged unions kept deliberately narrow (`Visibility` is only about visibility,
so "revealed" and "mined" cannot be confused), error variants carrying context
such as the legal range, a seeded RNG, and an interactive read loop.
