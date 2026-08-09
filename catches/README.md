# What Glyph catches that `tsc --strict` does not

Each directory here is one program written twice: the TypeScript a strict
project accepts, and the Glyph that refuses it. Both halves are run by
`scripts/check_catches.py`, so the claim is checked rather than asserted. The
TypeScript has to keep passing `tsc --strict`, or the case proves nothing. The
Glyph has to keep failing with the code it names, or the guarantee it documents
is gone.

This is the evidence behind the four pillars. A case that stops being
demonstrable gets deleted, not softened. One already was: an `await` in a
synchronous function looked like a good case until it turned out `tsc` rejects
that too, so the two programs were not equivalent and the case was proving
nothing.

Run them:

```sh
python3 scripts/check_catches.py
```

## The cases

| Case | What TypeScript accepts | Glyph | Pillar |
|---|---|---|---|
| `non-exhaustive-match` | A `switch` that misses a union case and returns `undefined` from a function declared to return `string` | `E0200` | verifiability |
| `imported-union-exhaustiveness` | The same, with the union imported from another module | `E0200` | verifiability |
| `absent-map-key` | A misspelled key on a `Record<string, string>`, typed `string`, printing "undefined" | `E0224` | verifiability |
| `unverifiable-parse` | A `value is Conn` guard that only checks a field is present, so `parse` returns a typed value it never validated | `E0304` | verifiability |
| `imported-record-field-typo` | A field read off a value that arrived as `any` from an untyped boundary | `E0210` | verifiability |
| `unvalidated-boundary-read` | `const user: User = JSON.parse(body)`, where the annotation launders `any` | `TS18046` | verifiability |
| `shadowed-global` | A local type named `Error`, shadowing the global for the rest of the module | `E0110` | greppability |

`unvalidated-boundary-read` is the one whose rejection comes from the `tsc` back
end rather than a Glyph code, and it is in the list on purpose. Glyph has no
`any` and no cast, so a boundary value stays `unknown` and there is no spelling
that gets past it. The difference is in what you are able to write, not in who
reports it.

## Adding one

A directory with `case.toml` (`code`, `pillar`, `title`, and an optional
`note`), `ts.ts`, and `glyph.glyph`. Extra `*.glyph` files in the directory are
built alongside, which is how the cross-module cases get their sibling. Then run
the check: it will tell you if either half does not behave the way the case
says.

The bar is that both programs do the same thing. If the TypeScript has to be
bent into a different shape to compile, the case is comparing two programs
rather than one language against another, and it belongs in the bin.
