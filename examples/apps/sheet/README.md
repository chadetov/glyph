# sheet

## What it is

A terminal spreadsheet with formulas. It loads a grid and a command script
through validating descriptors, parses each formula into an expression tree,
works out the dependency graph, recalculates in topological order, applies
scripted edits in batches, and prints the grid. Every failure is a value: a bad
reference, a divide by zero and a circular reference all land in the cell as an
error that downstream cells propagate instead of silently computing a wrong
number.

## Running it

```sh
glyph run examples/apps/sheet/main.glyph
glyph run examples/apps/sheet/main.glyph inspect D7
```

## What it changed in Glyph

The largest app in the tree, and the first whose domain vocabulary collided
head-on with the emitted module's own. A spreadsheet cell holds a number, a
label, nothing, or an error, which wants to be spelled `Number | Text | Empty |
Error`. Two of those four names are already bound in every module the compiler
emits.

**G63: a top-level declaration silently shadowed a global the emitted module
depends on.** Declaring an `Error` variant emitted a top-level `export function
Error`, so every `new Error(...)` the compiler writes below it called the variant
instead. The build did not fail there; it failed at an unrelated `match` with a
TypeScript type error, in the wrong place and with the wrong explanation.
`Number` was harmless until `int` shipped and the boundary check started emitting
`Number.isInteger`, which is how the defect reached a release without anyone
writing a program that hit it. Closed in 0.1.70 by capturing the compiler's own
globals rather than mangling user names: this module alone needed 162 captured
references.

**G65: `==` meant a deep comparison in an `@example` and reference equality in
the program.** A test that passes while the code it tests is wrong is worse than
no test. Closed in 0.1.66: `==` is now value equality on every type (D42), so a
record or tagged union compares by structure wherever it is written, in a
function body or an `@example`, and a primitive comparison still lowers to
`===`.

**G67 is closed.** A `for` binding used to carry no element type, so a
string-literal union's exhaustiveness evaporated inside a loop and the
compiler's own help text suggested adding the `else` that forfeits the
guarantee. `apply_batch` carried a `let cmd: CommandSpec = bound` annotation
and a comment saying it was load-bearing for exactly that reason. Closed in
0.1.98 together with G37, by giving each loop binding its own span
(`ForBinding { name, span }`) so a two-binding `for i, bound in commands`
types `bound` the same way a single-binding loop already did. The annotation
and its comment are gone: `apply_batch`'s `match cmd.op { "clear" => ..., "set"
=> ... }` is exhaustive today with no `else` and no annotation, because
`cmd.op`'s type (`"set" | "clear"`) survives the loop.

None of the three findings above still has a workaround in this file.

## What it exercises

Forty-six `@example` rows, the densest single file in the tree. Deep tagged
unions including a recursive formula AST, a string-literal union in a record
field, and one-step descriptor validation at both file boundaries.
