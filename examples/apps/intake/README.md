# intake

## What it is

A batch validator for JSON applicant records. It checks five fields
independently and merges every field's failure across the whole batch into one
report grouped by cause, instead of stopping at the first bad record. Exit 0 if
all passed, 1 if any was rejected, 2 if the input was unreadable.

## Running it

```sh
glyph run examples/apps/intake -- examples/apps/intake/sample.json
```

## What it changed in Glyph

**Nothing recorded, and it is a demonstration app rather than a probe.** It has
no gap number, no round, and no mention in the release history. Its whole git
history is one commit.

What it demonstrates is the shape `Result` cannot express. `Result<T, E>` stops
at the first `Err`, and someone validating a form wants every field's complaint
at once, so the app defines `Validated<E, T> = Valid | Invalid({ errors })` and
combinators over it. `map3` is written as an explicit eight-arm nested match so
every `Valid`/`Invalid` combination is an exhaustively checked arm.

## What it exercises

The heaviest use of `where` refinements in the tree: five of them declared as
types (`Age = int where value >= 0 && value <= 130`, `Email`, `Name`, `Salary`,
`Zip`). A hand-rolled generic union with generic combinators, a generic
higher-order function taking a parse function as a value, and an exhaustive match
over issue kinds so a fourth cause fails the build until it is handled.
