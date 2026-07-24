# External imports and `.types`

Glyph compiles to TypeScript and runs on Node, so it uses the npm ecosystem
directly: any npm package, and any Node builtin. This guide covers how an import
path becomes a TypeScript module specifier, how to give the type-checker types
for an external module, and the one runtime caveat to remember.

## The rule: import paths emit verbatim

A Glyph import path is emitted **unchanged** as the TypeScript module specifier.

```glyph
import std/io                  // import * as io from "std/io";   (namespaced)
import react { useState }      // import { useState } from "react";
import leftpad { leftpad }     // import { leftpad } from "leftpad";
```

The three import forms (D15):

- `import some/module` — namespaced; use it as `module.thing(...)`.
- `import some/module { a, b }` — named; `a` and `b` come into scope directly.
- `import some/module as alias` — aliased namespace.

The compiler only rewrites a specifier for a **sibling Glyph module** in your own
project (to a relative path) and for `std/*` (tsconfig-mapped to the bundled
runtime). Everything else — every npm package, every Node builtin — passes
through verbatim.

## npm packages

Import an npm package by its package name:

```glyph
import zod { z }               // import { z } from "zod";
```

If the package is installed in your project and ships its own types (or has an
`@types/...` companion), that is all you do. No stub, no adapter. `glyph build`
finds your project's `node_modules` and points `tsc` at it, so the package's
real types check your code, and a wrong call is a real error. See the zod
walkthrough below.

How the resolution works, and its one boundary: the build emits TypeScript into
an output directory that sits outside your project, so a bare `import ... from
"zod"` cannot reach your `node_modules` by the usual upward file walk. To fix
that, `glyph build` walks up from your source directory to the project root (the
nearest folder holding a `.git` or a `package.json`) and, if a `node_modules`
lives there, wires it into the generated `tsconfig.json`. The walk stops at that
root and never climbs past it, so a stray `node_modules` in a parent directory (a
common one in your home folder) is never used by mistake. A project with no
`node_modules` in scope builds exactly as before.

A package that has no types of its own, and no `@types/...`, still needs a
declaration you write. That is what `.types/` below is for.

## Node builtins: bare name, not `node:`

Import a Node builtin by its **bare name**:

```glyph
import http { createServer }   // import { createServer } from "http";
import fs { readFileSync }     // import { readFileSync } from "fs";
```

You **cannot** write the `node:` prefix: a colon is not a legal character in a
Glyph import path, so `import node:http { ... }` fails:

```
[E0002] parse: expected newline after import, found Colon
```

Use the bare name (`import http { ... }`); Node resolves `"http"` to the builtin
just as it resolves `"node:http"`.

The common builtins type-check **out of the box**, with nothing installed:
`glyph build` bundles ambient declarations for `fs`, `http`, `path`, `os`,
`crypto`, and `url` (plus the `process` global) under their bare names. For the
full, exact Node surface, install `@types/node` in your project. The build
detects it, prefers its complete typings, and skips the bundled shim, so there is
no duplicate-declaration conflict and a builtin API the shim does not cover (say
`os.uptime()`) type-checks the moment `@types/node` is present.

## Giving the type-checker types: `.types/`

`glyph build` type-checks the emitted TypeScript with `tsc --strict`, so an
external module with no types needs a declaration. Drop an ambient declaration
file under your source root's `.types/` directory:

```
src/
  main.glyph
  .types/
    http.d.ts
    leftpad.d.ts
```

Anything matching `<src>/.types/**/*.d.ts` is **auto-discovered**: it is copied
into the build output and included in the `tsc` run. No registration step.

You need `.types/` in two cases: a package that ships no types and has no
`@types/...`, or a module you want to declare yourself without installing
anything. An installed package that carries its own types does not need it (the
`node_modules` wiring above handles those), so reach for `.types/` only when
there is nothing to resolve.

## Worked example

`src/.types/http.d.ts`:

```ts
declare module "http" {
  export function createServer(
    handler: (req: unknown, res: unknown) => void,
  ): { listen(port: number): void };
}
```

`src/.types/leftpad.d.ts`:

```ts
declare module "leftpad" {
  export function leftpad(s: string, width: number): string;
}
```

`src/main.glyph`:

```glyph
module main

import std/io
import http { createServer }
import leftpad { leftpad }

fn main(argv: Array<string>) -> number {
  let padded = leftpad("7", 3)
  io.println(padded)
  let server = createServer(fn(req: unknown, res: unknown) -> void {
    io.println("request")
  })
  io.println("server created")
  return 0
}
```

Build it:

```sh
glyph build src --out dist
```

```
glyph build: 1 module(s) checked, no diagnostics; 1 TypeScript file(s) emitted.
glyph build: tsc --strict passed.
```

The emitted `dist/main.ts` carries the specifiers through verbatim:

```ts
import * as io from "std/io";
import { createServer } from "http";
import { leftpad } from "leftpad";
```

## Worked example: real zod, no stub

Install zod in a project (a folder with a `package.json`, so the build finds its
`node_modules`):

```sh
npm install zod
```

`src/main.glyph`:

```glyph
module main

import zod { z }

fn main(argv: Array<string>) -> number {
  let user_schema = z.object({
    name: z.string(),
    age: z.number(),
  })
  let user = user_schema.parse({ name: "Ada", age: 36 })
  print(user.name)
  return 0
}
```

Run it:

```sh
glyph run src/main.glyph
```

```
Ada
```

There is no `.types/zod.d.ts` and no adapter file. `glyph build` type-checks
`z.object`, `z.string`, and `.parse` against zod's own published types, and
`glyph run` executes against the installed zod (the same tsconfig `paths` entry
resolves the package for both `tsc` and the runtime). A call zod does not define
is a real error, mapped back onto your Glyph source:

```glyph
let n = z.string().nonexistent_method()
```

```
[TS2339] Error: tsc: Property 'nonexistent_method' does not exist on type 'ZodString'.
   ╭─[main:7:3]
```

One current limit: a value-derived type like `type User = z.infer<typeof
user_schema>` is not expressible yet (that is the value-derived-type work still
ahead). The parse result is fully typed, so `user.name` is a `string` here
without it; you just cannot name the derived type with `z.infer` today.

## Validating a package's types at the boundary

Type availability tells the checker what a package's types *are*. It does not, by
itself, validate a value that crosses from that package at runtime, a webhook
body, an SDK response, a row. When you want that boundary checked, materialize the
package's types into committed Glyph types with descriptors:

```sh
glyph gen dts api-types --out src/types
```

This resolves the installed package's own declaration entry from `node_modules`
(its `types`/`typings`/`exports` field, or a top-level `index.d.ts`) and writes a
committed `src/types/api-types.glyph` where each type is a real Glyph record with
an `is`/`parse`/`schema` descriptor. Import it and validate at the seam:

```glyph
import types/api_types { Customer }

match Customer.parse(webhook_body) {
  Ok(c) => handle(c),
  Err(issues) => reject(issues),
}
```

`Customer.parse` checks the value's structure deeply (nested records, arrays, and
optional fields all the way down), so a structurally-malformed payload is an `Err`
you handle, not a lie the type system waved through. It does not yet check leaf
*values*: an `integer` field is validated as a number (so `3.5` passes), and a
string enum as a `string`. The generated file records its own `glyph gen dts
api-types --out src/types` command, so `glyph regen` refreshes it when you bump
the dependency. This is the opt-in step: you run it for the types you actually
cross the boundary with, and the result is committed and greppable, not generated
invisibly on every build.

**What materializes today:** `gen dts` reads the top-level `interface` and `type`
declarations a package exports. A package whose types are a `declare namespace`
tree, heavy re-exports, or deep generics (many large SDKs) needs the deeper
`.d.ts` support tracked on the roadmap; for those, hand-write the specific shapes
you cross the boundary with, or reach for the `extern_ts` escape hatch, until that
lands.

`glyph gen zod` takes a package name too, for a package that *exports zod
schemas* (a shared-schema package). It resolves the package's runtime entry,
executes it, and materializes each exported schema:

```sh
glyph gen zod @acme/schemas --out src/gen
```

(`glyph gen openapi` stays file-based: an OpenAPI document is a file in your repo,
not something `node_modules` points at.)

## The escape hatch: `extern_ts` for types Glyph can't spell

Some TypeScript idioms have no Glyph form, most often a value-derived type like
`z.infer<typeof schema>`. For those, `extern_ts("...")` in type position emits its
string verbatim as the TypeScript type:

```glyph
import zod { z }

const user_schema = z.object({ name: z.string(), age: z.number() })

type User = extern_ts("z.infer<typeof user_schema>")

fn greet(u: User) -> string {
  return u.name
}
```

`type User` emits `export type User = z.infer<typeof user_schema>`, and `tsc`
checks it and every use of it: `u.name` is a `string`, and a bogus member inside
the string is a real error mapped back to your `.glyph`. What `extern_ts` opts out
of is only Glyph's own descriptor machinery, so an `extern_ts` type is opaque to
Glyph (no `.parse`), exactly like an imported `.d.ts` type. It is recognized only
in the `extern_ts("...")` shape, so it never shadows a type named `extern_ts`, and
every escape is greppable by `extern_ts`.

The string form is deliberately a little awkward: this is the rare-idiom fallback
so no library ever forces a hand-written adapter file, not a first-class way to
write types. For schemas you own, prefer materializing them with `glyph gen zod`
/ `gen dts` (real Glyph types with descriptors); reach for `extern_ts` when the
type genuinely lives in TypeScript and Glyph cannot name it.

`extern_ts` also works in **expression** position, for a grammar-hostile runtime
idiom:

```glyph
let now: unknown = extern_ts("Date.now()")
match now {
  is number => use_it(now),
  else => fallback(),
}
```

The expression form emits `(Date.now())` verbatim and is typed `unknown`, so, like
any untrusted value, you narrow or validate it before use (the `match` above).
Same containment as the type form: `tsc` checks the raw TypeScript, and only the
exact `extern_ts("...")` shape is special, so a variable named `extern_ts` is
unaffected.

## Runtime caveat

A `.types/*.d.ts` file gives the **type-checker** types; it is not the
implementation. For the emitted TypeScript to actually run (`glyph run`, or
running `dist/` with Node/tsx), the real module must be resolvable at runtime — a
Node builtin always is, and an npm package must be installed in the environment
where the code runs.
