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

You **cannot** write the `node:` prefix — a colon is not a legal character in a
Glyph import path, so `import node:http { ... }` fails:

```
[E0002] parse: expected newline after import, found Colon
```

Use the bare name (`import http { ... }`); Node resolves `"http"` to the builtin
just as it resolves `"node:http"`.

## Giving the type-checker types: `.types/`

`glyph build` type-checks the emitted TypeScript with `tsc --strict`, so every
external module needs a type declaration. Drop an ambient declaration file under
your source root's `.types/` directory:

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

## Runtime caveat

A `.types/*.d.ts` file gives the **type-checker** types; it is not the
implementation. For the emitted TypeScript to actually run (`glyph run`, or
running `dist/` with Node/tsx), the real module must be resolvable at runtime — a
Node builtin always is, and an npm package must be installed in the environment
where the code runs.
