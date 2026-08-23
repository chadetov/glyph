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
project and for `std/*` — both become relative paths in the emitted TypeScript
(`./helpers`, `./.glyph-runtime/std/result`), so the output resolves under any
toolchain without configuration. Everything else — every npm package, every
Node builtin — passes through verbatim.

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
`crypto`, `url`, `net`, `timers`, `events`, `child_process`, `dns/promises` and
`zlib` (plus the `process` global) under their bare names. For the
full, exact Node surface, install `@types/node` in your project. The build
detects it, prefers its complete typings, and skips the bundled shim, so there is
no duplicate-declaration conflict and a builtin API the shim does not cover (say
`os.uptime()`) type-checks the moment `@types/node` is present.

Reach for a Node builtin only when the stdlib does not already cover what you
want. `std/timers` schedules work, `std/websocket` opens a connection,
`std/http` serves and fetches, `std/fs` reads and writes. Those are typed Glyph
with no import path to get wrong, and they are the same on any host; the
builtins are the escape hatch beneath them.

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
anything.

**Module declarations only.** A `declare var`, `declare function` or `declare
class` in `.types/` declares a *global*, and Glyph resolves names from modules,
so the global satisfies `tsc` and stays invisible to Glyph: using it is
`[E0103] unresolved name`. This is the decision, not an oversight. Everything a
Glyph program can reach is Glyph the compiler knows, with a runtime descriptor
behind it, and the price is that the standard library is the one door for a new
host capability. It also means `new` (D37) does not work on a global class. When
you need a global the stdlib does not wrap, file it and it gets a typed wrapper,
the way timers and WebSocket did.

You should not need it to reach the platform. If you find yourself declaring a
Node builtin or a host global by hand, that is a gap in Glyph rather than
something to work around: the applications under `examples/apps/` contain no
`.d.ts` and no `extern_ts`, and a CI check keeps it that way, so that the answer
to a missing capability is to add it to the stdlib rather than to write the
TypeScript that Glyph exists to replace. An installed package that carries its own types does not need it (the
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

```glyph needs-deps
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

```glyph needs-deps
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

You can name the schema's inferred type directly with `typeof`:

```glyph
type User = z.infer<typeof user_schema>

fn greet(u: User) -> string {
  return u.name
}
```

`typeof user_schema` is the type of the value `user_schema`, resolved as a real
reference (a typo is an unresolved-name error), and `z.infer<...>` is an ordinary
member-generic type, so the whole thing is first-class Glyph, no string escape.
It emits as `type User = z.infer<typeof user_schema>`; `tsc` reduces it, so
`u.name` is a `string`. The type is opaque to Glyph itself (no `.parse`
descriptor, like any imported type); validation comes from `user_schema.parse(...)`,
the schema's own parser, which is how zod already works.

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
you handle, not a lie the type system waved through. Leaf values are checked too:
a string enum materializes as a string-literal union (`tier: "free" | "pro"`), so
the descriptor checks *membership* (a `tier` of `"enterprise"` is rejected, not
just any non-string), and a JSON-Schema `integer` field materializes as `int`, so
a wire `3.5` fails its `.parse` where a plain `number` field would accept it. The
remaining gap is string *formats* (uuid, email, date-time), which are not yet
validated.

Generated wire types carry `@open`, so `Customer.parse` tolerates a field the API
adds later (a forward-compatible change) while still validating every field it
declares. Records are strict by default in Glyph; codegen opts these into
tolerance with the same greppable `@open` marker a hand-written record would use,
because a `.d.ts` and JSON Schema allow extra properties by default. A source
schema that closes the world (`additionalProperties: false`) stays strict, with no
`@open`. One consequence to know: an `@open` parse *retains* extra keys on the
returned object rather than stripping them, so if you re-serialize a parsed wire
value, any extra keys the sender included ride along. That is the deliberate trade
for tolerating additive API changes; if you need the strict, extra-keys-rejected
behavior for a specific type, remove its `@open` (or generate from a schema that
sets `additionalProperties: false`).

The generated file records its own `glyph gen dts api-types --out src/types`
command, so `glyph regen` refreshes it when you bump the dependency. This is the
opt-in step: you run it for the types you actually cross the boundary with, and
the result is committed and greppable, not generated invisibly on every build.

**What materializes today:** `gen dts` reads the `interface` and `type`
declarations a package exports, including those inside a `declare namespace` tree
(keyed by their qualified name, with bare cross-references resolved through the
scope) and those in sibling files that an `index` barrel re-exports: the entry
`.d.ts` plus every `.d.ts` reachable through a relative `import`/`export … from`
is walked, so a package that splits its types across files materializes fully.
That specifier may carry a file extension: `export * from "./tokens.js"` is how
every ESM-authored package refers to a sibling under
`moduleResolution: nodenext`, and `./tokens.ts` is TypeScript 5's spelling, so
both resolve to the declaration file that carries the types. A
generic is kept first-class: `interface Page<T> { items: T[] }` materializes as
`type Page<T> = { items: Array<T> }`, and a `Page<User>` keeps its argument, so
the type gets a real descriptor that validates each item as a `User`, not just for
presence. A bare specifier (`from "react"`) is not followed, since it points at
another package.

References through an aliased import (`import { Widget as W }` then a field typed
`W`), a re-export rename (`export { X as Y } from`), and a namespace alias
(`import * as ns` / `export * as ns from "./ns"` then `ns.Type`) all resolve: a
per-file binding map translates the written name back to its declaration. One
case the reader still cannot make safe, so `glyph gen` **flags it with a note**:
a type declared under the same name in more than one reachable file is kept
first-wins, which could bind a reference to the wrong shape (rename the collision
or materialize the intended file directly). For anything the materializer can't
reach, hand-write the shapes you cross the boundary with, or reach for the
`extern_ts` escape hatch.

**When two types want the same Glyph name.** A Glyph module is flat, so a
namespaced `Tokens.List` and a top-level `TokensList` both want to be written
`TokensList`, and only one of them can. `gen` stops rather than picking:

```
$ glyph gen dts marked --out src/types
glyph gen: `TokensList` is produced by 2 different types in marked:
             `Tokens.List`
             `TokensList`

Nothing was written. Give one of them a Glyph name:
  glyph gen dts marked --out src/types --rename Tokens.List=<GlyphName>
```

Nothing is written until you choose, because a name the generator invented would
appear in no source you could grep, and could change under you when the package
adds a type. `--rename` is repeatable and is recorded in the generated header, so
`glyph regen` replays your choice instead of stopping at the same collision.

A package whose API is *classes* rather than interfaces is a different matter:
`gen dts` reads `interface` and `type` declarations, so a field typed by a class
(or by a computed type like `Omit<T, K>`) materializes as a reference to a name
that was never written, and `glyph build` reports it as an unresolved name. `gen`
names each one in a note when it happens. Importing the class and constructing it
with `new` needs no generation at all and is checked by `tsc`, so that path is
unaffected.

`glyph gen zod` takes a package name too, for a package that *exports zod
schemas* (a shared-schema package). It resolves the package's runtime entry,
executes it, and materializes each exported schema:

```sh
glyph gen zod @acme/schemas --out src/gen
```

(`glyph gen openapi` stays file-based: an OpenAPI document is a file in your repo,
not something `node_modules` points at.)

## Class-based clients: `new`

Many database and messaging clients are class-based: you construct a client with
`new`. Glyph has `new` for exactly this, and nothing else:

```glyph
import kafkajs { Kafka }

async fn main() -> void {
  let kafka = new Kafka({ clientId: "app", brokers: ["localhost:9092"], })
  let producer = kafka.producer()
  await producer.connect()
}
```

`new <callee>(<args>)` emits a verbatim TypeScript `new` and is type-checked by
`tsc` against the package's real constructor: a wrong argument is a real error
mapped back to your Glyph source, and an undefined callee is `E0103` at resolve
time. It is greppable (`grep new` finds every construction site). The instance
carries no Glyph `.parse` descriptor (it is an external type, like anything from
a `.d.ts`); `tsc` supplies its type, so `kafka.producer()` and the chain that
follows are all checked.

This is interop-only. Glyph has **no `class` declarations** and gains none;
`new` only constructs a type that comes from an npm package, a `.types` ambient
declaration, or `extern_ts`. The same pattern covers `new MongoClient(url)`
(`mongodb`), `new Redis()` (`ioredis`), and `new Pool(cfg)` (`pg`).

A factory-style client needs no `new` at all. `node-redis`'s `createClient()`,
`mysql2`'s `createConnection()`, and any SDK you call as a function import and
work directly:

```glyph
import redis { createClient }

async fn main() -> void {
  let client = createClient()
  await client.connect()
  await client.set("k", "v")
}
```

## Hand-written TypeScript modules: `import extern/*`

`.types/` gives the type-checker *types* for a module that already exists at
runtime (an npm package, a Node builtin). Sometimes you need to write the runtime
code yourself in TypeScript: an idiom Glyph's grammar can't spell, a node-stream
loop, a `new Promise`, a worker thread. Glyph forbids relative imports (D15), and
a bare specifier only resolves through `node_modules`, so there is one reserved
path for this: put the `.ts` under `<src>/extern/` and import it as
`import extern/<name>`.

```
src/
  main.glyph
  extern/
    raw_server.ts     // hand-written TypeScript
```

```ts
// src/extern/raw_server.ts
export function serve_raw(port: number, handler: (raw: string) => string): void {
  // node http, stream reads, whatever Glyph can't express
}
```

```glyph
module main
import extern/raw_server { serve_raw }
// serve_raw is typed from the .ts; a wrong argument is a real tsc error.
```

The build stages `<src>/extern/**` verbatim into the output and emits a
**relative** specifier for the import, so the file resolves at build and run time
and `tsc` type-checks it together with your Glyph code: the extern's exported
types enforce your calls, and a wrong argument is a real error mapped back to the
`.glyph` source. The prune pass that clears stale output never touches
`extern/`, so a rebuild keeps it.

Your extern is a build input like any `.glyph` file, so `glyph run`'s cache
fingerprint covers it: editing a `.ts` or `.tsx` under `extern/`, renaming one,
adding one, or deleting one rebuilds and re-type-checks. Symlinks under
`extern/` are followed, so a shim that lives outside the source tree is still
hashed by its contents. Files under `extern/` that are not `.ts` or `.tsx` (a
`README.md` next to the shim) are copied into the output with the rest of
`extern/`, but nothing type-checks or runs them, so they do not invalidate the
cache.

One layout trap while this is still open: `extern/` is resolved against the
source root, and the two commands choose that root differently. `glyph run
apps/app.glyph` roots at `apps/`, so the shim must be `apps/extern/web.ts`,
while `glyph build .` roots at `.`, so it must be `./extern/web.ts`. If your
program lives in a subdirectory of what you build, keep the shim reachable from
both roots (a symlink is enough; the fingerprint follows it). A root that has no
shim fails loudly: `tsc` reports `TS2307` and it is mapped back onto the
`import extern/...` line.

An extern using the common Node surface (an `http` server, `fs`) type-checks
against the bundled shim with nothing installed: `createServer`, `req.on(...)`
including `"error"`, `server.listen(port, callback)`, `res.writeHead`/`end`. For
the full, exact Node API in your extern, install `@types/node` (the build detects
it and loads its complete typings).

This is the one supported way a Glyph module imports a local `.ts`. It is
deliberately narrow and greppable (`grep -rn 'import extern/'` finds every place
you left the language), and it is the inverse of the common direction, a `.ts`
file importing your compiled Glyph, which needs nothing special. Reach for it
only for genuine runtime code Glyph can't express; for a single inline type or
expression, `extern_ts("...")` below is lighter.

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
