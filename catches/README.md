# What Glyph catches that `tsc --strict` does not

Each directory here is one program written twice: the TypeScript a strict
project accepts, and the Glyph that refuses it. Both halves are run by
`scripts/check_catches.py`, so the claim is checked rather than asserted. The
TypeScript has to keep passing `tsc --strict`, or the case proves nothing. The
Glyph has to keep failing with the code it names, or the guarantee it documents
is gone.

There are five cases, and all five are evidence for verifiability. None is
evidence for greppability, abstraction or diff stability, and the check prints
those zeros rather than leaving them off.

A case that stops being demonstrable gets deleted, not softened. Six have been.
An `await` in a synchronous function looked like a good case until it turned
out `tsc` rejects that too. A local type named `Error` was a case until G63 made
it legal. Four more went in the review before the launch, each for a reason
that was checked by running it:

- A union imported from another module, matched without one of its cases. The
  TypeScript half was a single file, and importing the union changes nothing on
  the TypeScript side, so this was the plain exhaustiveness case again. The
  Glyph behaviour is a compiler test now.
- A typo on a field of an imported record. TypeScript catches that too
  (TS2339) when the import is typed. The case passed only because its
  TypeScript took the value as `any`, which is a different program.
- A `Nullable<int>` passed where an `int` is declared. The TypeScript was safe
  at run time, and `tsc` catches the widened forms the comment worried about.
  Glyph is stricter there, but that is not a bug TypeScript lets through.
- A local type named `Array`. What the case said goes wrong, a later
  `Array<string>` meaning something else, is TS2315 in TypeScript. Glyph refuses
  the name because its emitter needs the global. That was the only greppability
  case.

Run them:

```sh
python3 scripts/check_catches.py
```

The TypeScript is checked with the real `@types/node` installed, because one
case imports a socket type from `net` the way a real project would. The script
installs it on every run, so it needs npm and the network, and it fails rather
than skips without them.

## The cases

| Case | What TypeScript accepts | Glyph | Pillar |
|---|---|---|---|
| `non-exhaustive-match` | A `void` handler whose `switch` misses a union case, so that case does nothing and nothing says so | `E0200` | verifiability |
| `absent-map-key` | A read from a `Record<string, string>` typed as `string`, printing "undefined" when the key is not there | `E0224` | verifiability |
| `split-into-a-pair` | `const [user, host] = s.split("@")`, typing `host` as a `string` that is `undefined` when there is no `@` | `E0230` | verifiability |
| `unverifiable-parse` | A `value is Conn` guard that only checks a `net.Socket` field is present, so `parse` returns a typed value it never validated | `E0304` | verifiability |
| `unvalidated-boundary-read` | `const user: User = JSON.parse(body)`, where the annotation launders `any` | `E0204` | verifiability |

`absent-map-key` uses a typo because it makes the hole obvious, but the typo is
not the point. Glyph refuses the correctly spelled `row.name` too, with the same
`E0224`, because a `Record<string, string>` says nothing about which keys exist.
TypeScript's `noUncheckedIndexedAccess` catches this, but it is not part of
`--strict`, and a project has to know to turn it on.

`non-exhaustive-match` is the shape an event handler or reducer takes. A
`switch` that returns a value from each case does not make a fair case, because
`tsc` rejects the missing return (TS2366) unless the function ends in a cast.
typescript-eslint's `switch-exhaustiveness-check` and the `assertNever` pattern
both catch the `void` version when a project adopts them. In Glyph the check is
not something a project opts into, and it cannot be turned off.

In `unvalidated-boundary-read`, `json.parse` returns a
`Result<unknown, Array<Issue>>`, and Glyph's checker refuses it where a `User`
is declared, so `glyph check --no-tsc` fails on it as well as `glyph build`.
In TypeScript `JSON.parse` returns `any`, and no setting changes that.

## Adding one

A directory with `case.toml` (`code`, `pillar`, `title`, and an optional
`note`), `ts.ts`, and `glyph.glyph`. Extra `*.glyph` files in the directory are
built alongside. Then run the check: it will tell you if either half does not
behave the way the case says.

The bar is that both programs do the same thing. If the TypeScript has to be
bent into a different shape to compile, the case is comparing two programs
rather than one language against another, and it belongs in the bin.
