# zipper

## What it is

A CLI shell over a virtual filesystem, navigated by a Huet zipper. It loads a
tree from a fixture and replays a script: `ls`, `cd`, `up`, `mkdir`, `touch`,
`rm`. The navigation core is a generic rose tree plus a zipper with no domain
knowledge, so a `down` move only touches the crumb it pushes and `up` reconstructs
an ancestor from its crumb instead of re-walking from the root. The shipped script
deliberately drives every error path.

## Running it

```sh
cd examples/apps/zipper && glyph run main.glyph
glyph run main.glyph path/to/fixture.json path/to/script.txt
```

## What it changed in Glyph

It provoked **G139** (0.1.90): a match arm over an imported union was refused
once the union was generic, because the payload's storage was read off the
instantiation instead of the union. Neither half of that combination showed it on
its own. A local generic union is an application over a type the module declares;
an imported non-generic union is proof of its own shape. Only imported *and*
generic landed on the arm nothing covered, and this app's tree is that shape.

**The provenance is worth reading, because it corrects the attribution.** The
app did file the error, but its filed reproduction spells both arms with plain
bindings, and that program compiles clean on the current tree and compiled clean
with the fix deleted too. An object pattern of bindings and a wildcard is
irrefutable, so the match never routes through the code the fix changed. The app
was building against an intermediate working state. No released compiler ever
failed that program.

So the honest framing is: the app whose shape provoked the investigation, whose
filed reproduction did not reproduce, and whose class the fix now covers. That
distinction is the kind of thing this ledger exists to keep straight.

It also had a bug of its own, fixed in 0.1.95: `cd` checked that the current
focus was a directory rather than that the named child was one, so it could walk
into a leaf and get stuck.

## What it exercises

The generics-heaviest app: a generic self-referential union, generic record
and error types, and eight generic functions with zero domain knowledge,
instantiated once by the CLI. A `where` refinement validated two levels down
inside the fixture decoder, and nested patterns inside constructor payloads,
which is the G139 shape.
