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

At runtime the emitted TypeScript runs `import { z } from "zod"`, so the package
must be installed where that code runs (see the runtime caveat below).

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

Anything matching `<src>/.types/**/*.d.ts` is **auto-discovered** — it is copied
into the build output and included in the `tsc` run. No registration step. For an
npm package that already ships its own types, you would instead install the
package (or its `@types/...`) where the build resolves modules; the `.types/`
path is the zero-dependency way to declare a module yourself.

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

## Runtime caveat

A `.types/*.d.ts` file gives the **type-checker** types; it is not the
implementation. For the emitted TypeScript to actually run (`glyph run`, or
running `dist/` with Node/tsx), the real module must be resolvable at runtime — a
Node builtin always is, and an npm package must be installed in the environment
where the code runs.
