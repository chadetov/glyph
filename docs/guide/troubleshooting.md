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

**`import extern/x` is not found by one command and is by the other.** The two
commands pick the source root differently: `glyph run apps/app.glyph` roots at
`apps/`, so the shim must be `apps/extern/x.ts`, while `glyph build .` roots at
`.` and wants `./extern/x.ts`. Keep the shim reachable from both roots (a
symlink works) until this is settled. A missing one is a `TS2307` mapped onto
the `import` line, never a silent skip.

**`glyph run` seems to be running an older version of my code.** It should not,
and it is worth reporting if it does. The cache key covers every `.glyph` file,
every `.d.ts` under `.types/`, and every `.ts` and `.tsx` under `extern/`, by
path as well as by contents, so an edit, a rename, or a deletion rebuilds.
Before 0.1.44 the `extern/` half was missing, so editing a hand-written shim
left a stale build in place; upgrade if you are on an older version.

## Language errors people hit first

**E0006: Glyph has no `if` / `else`.** Branch with `match` (every branch is a
value, every case is checked). See [how to think in Glyph](how-to-think.md).

**E0008: assignment requires `mut`.** A bare `x = e` is not an assignment
statement. Reassigning an existing binding is `mut x = e`; introducing a new one
is `let x = e`. The mark is what makes every mutation greppable (D5), and it
applies to fields and elements too (`mut r.count = 1`, `mut xs[0] = v`).

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
an `else` (E0218). If you get E0218 on a `match` over a type you believe is a
string-literal union, check what the scrutinee's type actually is: E0218 means
the checker read it as a bare `string`. Importing the type from another module is
no longer a reason for that, whichever spelling you used.

**E0220: unknown variant in a `match` arm.** A PascalCase arm head that names no
variant of the union you are matching, usually a typo (`Loadign` for `Loading`)
or a variant from the wrong union. Glyph reads a capitalized bare head as a
variant reference, not a fresh binding, so it does not silently swallow the arm;
the message suggests the nearest real variant when one is close. A lowercase head
(`rest`) is still a binding. A qualified head over a union you imported by
namespace (`model.Loadign`) gets the same check; a misspelled bare head is caught
one stage earlier as an unresolved name (E0103) and again here.

**E0105: `N` is not exported by `M`.** The name you imported is either
misspelled or private. Declarations are module-private by default; mark the one
you want to expose `pub` in its own module. A type written through a namespace
import gets the same check at the annotation, so `import lib` plus a `lib.Secret`
parameter reports this too.

**E0221: unknown annotation.** An `@name` the compiler doesn't recognize (a typo
like `@puer`). The recognized set is small and documented in the spec (D27).

**E0223: this `match` arm produces no value.** The `match` is used as a value (a
`let`, a `mut`, a `return`, or the tail of a function with a declared return
type) and one arm yields nothing, usually `X => {}`. That arm lowers to
`case X: { break; }`, so the binding would be `undefined` at run time. Give the
arm a value, or `return` from it. `X => {}` is still a legal no-op where the
`match` is a statement.

**E0300: a block body in an arm of a match nested inside a larger expression.**
Only the nested form hits this, because it compiles to a closure. A `match` that
is the whole value of a `let` or a `mut` takes block arms, `await`, and `break`.
Pull the nested one out into its own `let` and the arms can be blocks again.

**E0302 / E0303: `?` in a position it cannot propagate from.** `?` becomes a
`const` plus an early `return` placed before the statement it sits in, so it
needs a statement to hoist into. E0302 is a `?` in an arm of a `match` nested in
a larger expression (that match is a closure, and the `return` would leave the
closure); E0303 is a position with no statement slot at all, such as a `match`
scrutinee. Both are fixed by binding first: `let id = load(p)?`, then match on
`id`.

**E0222: `await` outside an `async fn`.** Mark the enclosing function `async fn`.
The innermost callable decides, so a synchronous lambda inside an `async fn` is
its own context and cannot `await` either; write `async fn(x) { ... }` for the
lambda or move the `await` out of it.

## Runtime

**A stack trace points at a `.ts` file, not my `.glyph`.** `glyph run` executes
the emitted TypeScript. Build diagnostics and `tsc` errors are mapped back to
your `.glyph` source; a runtime stack from `tsx` is not yet remapped. The emitted
`.ts` (with a source map) is in your build output.

**`glyph run` printed an error and still exited 0.** The error is in a sibling
module, not in the file you ran. `glyph run` builds the whole directory and
reports everything it finds, but a module that failed to compile is only
unavailable to import; it does not stop a program that never imported it. The
exit code is whatever `main` returned. Run `glyph check` on the file or the
directory if you want the tree's health to decide the exit code without
running anything.

**I want to know whether a file compiles, without running it.** `glyph check
path.glyph` does exactly that: it type-checks the file in the context of its
directory, runs `tsc --strict` over the emitted TypeScript, writes nothing, and
never starts your program. It also accepts a directory. `--no-tsc` stops after
the Glyph stages when you want the fast answer and no toolchain. It runs your `@example` and `@doc @run`
tests too, the same gate `glyph build` uses, so it cannot report a clean tree
that `build` would fail. Pass `--no-test` when you want the answer without
executing anything.

**`T.parse` accepts a value I expected it to reject.** A field typed by an
imported `.d.ts` type is checked for presence only until you materialize that
type with `glyph gen dts`. Materialize it to get deep, leaf-level validation.

Still stuck? Open an issue at
<https://github.com/chadetov/glyph/issues> with the smallest program that
reproduces it.
