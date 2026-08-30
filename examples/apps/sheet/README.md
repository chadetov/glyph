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
no test.

**G67 is still open and still visible here**: a `for` binding carries no element
type, so a string-literal union's exhaustiveness evaporates inside a loop and the
compiler's own help text suggests adding the `else` that forfeits the guarantee.
The annotation working around it is load-bearing and says so.

## What it exercises

Forty-seven `@example` rows, the densest single file in the tree. Deep tagged
unions including a recursive formula AST, a string-literal union in a record
field, and one-step descriptor validation at both file boundaries.
