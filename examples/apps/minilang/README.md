# minilang

## What it is

A tiny scripting language implemented end to end: lexer, Pratt parser,
tree-walking evaluator, and a REPL. It has numbers, strings, booleans, nil,
arrays, records and first-class closures, plus seven builtins. Its rule is that
nothing throws: a lexer, parser or runtime failure is a value carrying the span
that caused it, and all three render the same way, with the offending line quoted
and a caret under the column. Interpreter limits come from a config file through
a record descriptor, so runaway recursion is a diagnostic rather than a blown
host stack.

## Running it

```sh
glyph run examples/apps/minilang/main.glyph
glyph run examples/apps/minilang/main.glyph -- --tokens
glyph run examples/apps/minilang/main.glyph -- --repl
```

## What it changed in Glyph

**No gap is attributed to it.** It appears in the record only as a victim and
then a beneficiary of the `io.read_line` defect (**G81**, fixed 0.1.62): before
that fix it was a REPL that evaluated nothing until you hung up. It is also
evidence for **G82**, that `std/io` cannot write without a newline, so it prints
nothing at all before each read.

Its value here is as the largest single Glyph file in the tree, at 2,205 lines.
That it was written at all is the finding.

## What it exercises

Recursive tagged unions for the AST and for tokens, four separate error unions
each carrying spans, `Record<string, Value>` environments with an `Option<Env>`
parent chain, and record-descriptor validation of the config. No `@example`
rows.
