# Troubleshooting

Common failures, and what they mean. For a specific error code, run
`glyph --explain E0217` (or any code) for the full explanation.

## Setup and toolchain

**`glyph: command not found`** after `npm install`. The binary is a platform
package pulled in as an optional dependency. If your install skipped optional
deps, reinstall without `--no-optional`, or run it through `npx @glyphlang/glyph`.

**`glyph run` says tsx or tsc is missing.** `glyph run` and `glyph build --check`
shell out to `tsx` (to run) and `tsc` (to type-check). Install them in your
project (`npm i -D tsx typescript`) or globally. `glyph init` scaffolds a
`package.json` that pins both. Run `glyph doctor` to check the toolchain.

**A build passes for me but fails in CI (or vice versa).** The type-check depends
on the TypeScript version on the path. Pin `typescript` in `devDependencies` so
every machine resolves the same one; a project pin wins over a global install.

**`import fs` (a node builtin) doesn't type-check.** Node builtins work out of
the box against a bundled shim. For the full surface, add `@types/node`
(`npm i -D @types/node`); the build detects it and loads it.

## Language errors people hit first

**"unexpected token" on `if` / `else`.** Glyph has no `if` statement. Branch with
`match` (every branch is a value, every case is checked). See
[how to think in Glyph](how-to-think.md).

**"expected `,`" at the end of a list.** Trailing commas are required, including
on the last element and the last `match` arm. This is what keeps adding an item
to a one-line diff.

**"unresolved name `boolean`" / `int` / `any`.** The boolean type is `bool`. An
integer is `int` (a boundary-validated `number`). There is no `any`; narrow an
`unknown` through `.parse` or `match`. The compiler suggests the Glyph spelling.

**E0217: `Result` used as a non-final statement.** You called something that
returns a `Result` and dropped it, discarding a possible `Err`. Handle it with
`match`, propagate it with `?`, or make it the block's final expression.

**E0200 / E0218: match is not exhaustive.** A `match` over a tagged union or a
string-literal union must cover every case (E0200 lists the missing ones). A
`match` over an unbounded `number`/`string` can never be exhaustive, so it needs
an `else` (E0218).

**E0220: unknown variant in a `match` arm.** A PascalCase arm head that names no
variant of the union you are matching, usually a typo (`Loadign` for `Loading`)
or a variant from the wrong union. Glyph reads a capitalized bare head as a
variant reference, not a fresh binding, so it does not silently swallow the arm;
the message suggests the nearest real variant when one is close. A lowercase head
(`rest`) is still a binding. This is caught for a module-local union whose type is
known at the match; a union imported from another module is checked for coverage
but not yet for this typo.

**E0105: `N` is not exported by `M`.** The name you imported is either
misspelled or private. Declarations are module-private by default; mark the one
you want to expose `pub` in its own module.

**E0221: unknown annotation.** An `@name` the compiler doesn't recognize (a typo
like `@puer`). The recognized set is small and documented in the spec (D27).

## Runtime

**A stack trace points at a `.ts` file, not my `.glyph`.** `glyph run` executes
the emitted TypeScript. Build diagnostics and `tsc` errors are mapped back to
your `.glyph` source; a runtime stack from `tsx` is not yet remapped. The emitted
`.ts` (with a source map) is in your build output.

**`glyph run` printed an error and still exited 0.** The error is in a sibling
module, not in the file you ran. `glyph run` builds the whole directory and
reports everything it finds, but a module that failed to compile is only
unavailable to import; it does not stop a program that never imported it. The
exit code is whatever `main` returned. Run `glyph build` on the directory if you
want the tree's health to decide the exit code.

**`T.parse` accepts a value I expected it to reject.** A field typed by an
imported `.d.ts` type is checked for presence only until you materialize that
type with `glyph gen dts`. Materialize it to get deep, leaf-level validation.

Still stuck? Open an issue at
<https://github.com/chadetov/glyph/issues> with the smallest program that
reproduces it.
