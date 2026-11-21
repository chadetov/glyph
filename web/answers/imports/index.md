<!-- Markdown copy of https://glyphlang.io/answers/imports/, generated from the same source as the page. Last changed 2026-11-21. -->

09 · Adoption

# Can I use my existing TypeScript, JavaScript, and npm libraries?

Yes. Glyph compiles to **ordinary TypeScript** and rides npm: a `.glyph` module imports from any package and is itself importable from your existing `.ts`. You adopt it **one file at a time**.

form.glyph

```
import react { useState }
import react-hook-form { useForm }
import @hookform/resolvers/zod {
  zodResolver,
}
```

form.ts: emitted

```
import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver }
  from "@hookform/resolvers/zod";
```

You import packages by name (including hyphenated and npm-scoped ones) and Glyph emits the import you'd expect.

## Why it matters

Nobody rewrites a codebase to try a language. Because Glyph's output *is* TypeScript, a `.glyph` file drops into the corner of an existing TS/React/Node project, imports the same packages, and exports back to the `.ts` around it.

That gradual path is the whole reason to target TypeScript instead of shipping a new runtime.

## Installed packages bring their own types

If the package is installed and ships its own types (or has an `@types/...`), that is all you do. No stub, no adapter file.

`glyph build` finds your project's `node_modules` and points the type-checker at it, so the package's real types check your code and a wrong call is a real error.

Here is zod used inline, with nothing hand-written:

main.glyph

```
import zod { z }

let user_schema = z.object({
  name: z.string(),
  age: z.number(),
})
let user = user_schema.parse(input)
print(user.name)
```

a call zod doesn't define

```
module main

import zod { z }

pub fn main() -> number {
  // ZodString has no such method:
  let n = z.string().nonexistent_method()
  return 0
}

// [TS2339] Property 'nonexistent_method'
//   does not exist on type 'ZodString'.
//   mapped back onto main.glyph:7
```

`glyph run` executes the same program against the installed zod, so it is not only a type-check: the one config entry that resolves the package for the type-checker resolves it for the runtime too.

A package that ships *no* types and has no `@types/...` still needs a declaration you write, which is what the `.types/` folder is for (the same `.d.ts` mechanism TypeScript already uses).

**Details subpath imports, and validated types from a `.d.ts` **

Subpath imports (`pkg/sub`) resolve through the package's `exports` map too, so a subpath's own types are checked.

And when you want a library's shape as a first-class, *validated* Glyph type rather than a type-only import, `glyph gen dts` materializes a `.d.ts` into real descriptor-bearing types: see [do I have to hand-write a validator for every API?](https://glyphlang.io/answers/typed-apis/)

## Value-derived types, JSX spread, and the escape hatch

Value-derived types are first-class. JSX prop spread (`<input {...register()} />`) is first-class too.

So a library never forces a hand-written adapter file.

For the rare TypeScript idiom Glyph still can't spell, `extern_ts("...")` is the scoped, greppable escape hatch: `tsc` checks it, but be precise, it is a trust boundary, not a runtime-validated one (opaque to Glyph, a value comes back `unknown` to narrow).

**Details how `z.infer<typeof s>` resolves **

`type User = z.infer<typeof user_schema>` works directly: `typeof user_schema` is a type query over a real value reference (a typo is an unresolved-name error), and `z.infer<...>` is an ordinary member-generic type. It emits verbatim and `tsc` reduces it, so `u.name` is a `string`; validation comes from the schema's own `parse`, which is how zod already works.

## When you need whole hand-written TypeScript

Sometimes the escape hatch isn't a type or an expression but real runtime code: a node-stream loop, a `new Promise`, a worker thread. Put the `.ts` under `<src>/extern/` and reach it with `import extern/<name>`.

```
// src/extern/raw_server.ts   (hand-written TypeScript)
export function serve_raw(port: number, handler: (raw: string) => string): void { ... }

// src/main.glyph
import extern/raw_server { serve_raw }   // typed from the .ts; a wrong arg is a real tsc error
```

Relative imports stay illegal in Glyph source (a greppability rule), so `extern/*` is the one reserved path, and `grep -rn 'import extern/'` finds every place you left the language.

**Details staging, the other direction, and the node shim **

The build stages `<src>/extern/**` into the output and emits a relative specifier, so `tsc` type-checks the extern together with your Glyph code and a rebuild never prunes it.

It is the inverse of the easy direction, a `.ts` file importing your compiled Glyph, which needs nothing at all. A common extern (an http server, `fs`) type-checks against the bundled node shim with nothing installed.

## Watching a subprocess, with nothing installed

The bundled node declarations carry the async half of `child_process`, not just the blocking one, so a tool that reports a long command's output while it runs is Glyph rather than an extern:

```
module main

import child_process { spawn }
import std/io

pub async fn main(argv: Array<string>) -> number {
  let child = spawn("git", ["status", "--short"])

  // Ask for text and node decodes for you, holding back a character whose
  // bytes straddle two chunks. Decoding a raw chunk yourself does not.
  child.stdout.setEncoding("utf8")
  child.stdout.on("data", fn(chunk) {
    io.print("${chunk}")
  })
  child.on("close", fn(code) {
    io.println("git exited ${code}")
  })
  return 0
}
```

`io.print` and not `io.println`: a chunk is a slice of the stream, not a line, and one line can arrive across two of them.

The overloads follow `@types/node`, so the pipes come back non-null from a plain `spawn(command, args)` and nullable once you pass `stdio`. Reading `child.stdout` after `stdio: "inherit"` is `TS18047` here and `TS18047` with the real typings, rather than two different stories about the same value:

```
module main

import child_process { spawn }

pub fn main() -> void {
  let child = spawn("git", ["status"], { stdio: "inherit" })
  // [TS18047] 'child.stdout' is possibly 'null'.
  child.stdout.setEncoding("utf8")
}
```

## Edit the shim, and the next run is the shim you edited

A shim is a file you iterate on, so it has to count as a source.

`glyph run` caches a build under a fingerprint of your sources, and that fingerprint covers every `.ts` and `.tsx` under `extern/`, by path as well as by contents.

**Details what triggers a rebuild **

Edit one, rename one, add one, delete one, and the next run rebuilds and re-runs `tsc`. A symlinked shim is followed, so a `.ts` kept outside the source tree is hashed by what it actually contains.

That is what lets the shim own a type instead of both files declaring their own copy of it. Export the shape from the shim, import it in the Glyph module, and the compiler holds the two together on every run:

src/extern/web.ts

```
export type WebResponse = {
  status: number;
  content_type: string;
  location: string;   // "" when not a redirect
  body: string;
};
```

the shim renamed a field, the build says so

```
import extern/web { serve_web, WebResponse }

// location -> redirect_to in web.ts, nothing
// touched here, and glyph run stops:
[TS2353] Object literal may only specify
known properties, and 'location' does not
exist in type 'WebResponse'.
   ╰─ server:53:3
```

**History before 0.1.44, and the files that do not count **

Until 0.1.44 the fingerprint skipped `extern/` entirely, so that rename produced a clean, type-checked build running the previous copy of the shim. Files under `extern/` that are not `.ts` or `.tsx` still do not affect the cache, which is deliberate: a `README.md` next to your shim is copied into the output along with it, but nothing type-checks or runs it, so there is nothing to rebuild.

## Class-based clients: the database and messaging ones

Most database and messaging clients are class-based: you write `new Kafka(...)`, `new MongoClient(url)`, `new Redis()`, `new Pool(cfg)`. Glyph has `new` for exactly this.

a class-based client (kafkajs)

```
import kafkajs { Kafka }

let kafka = new Kafka({
  clientId: "app",
  brokers: ["localhost:9092"],
})
let producer = kafka.producer()
await producer.connect()
```

a factory client needs no `new`

```
import redis { createClient }

// createClient() is a function, so you
// just call it. No new, no wrapper.
let client = createClient()
await client.connect()
await client.set("k", "v")
```

It emits a verbatim TypeScript `new` and `tsc` checks it against the package's real constructor, so a wrong argument is a real error and an undefined callee is caught by name.

It stays interop-only: Glyph has no `class` of its own, and `new` only constructs a type that comes from a package, a `.types` declaration, or `extern_ts`.

## Where it stands

### Shipping today

- Import any npm package (hyphenated and scoped names included).
- An installed package that ships its own types (or has `@types/...`) type-checks and runs with no stub.
- Class-based clients construct with `new`, checked by `tsc` against the real constructor; factory clients import and call directly.
- Node builtins type-check out of the box

  **Details**

  Node builtins (`fs`, `path`, `http`, `child_process` including the streaming `spawn`, and the rest) type-check out of the box, with `@types/node` loaded automatically when you install it, and CI builds the compiler's own runtime against that package at `latest`, so a declaration the stdlib depends on cannot drift from node's real one.

- A declaration only your code reaches is not covered by that check yet, and a few are still wider than node.
- `.types/` ambient declarations for anything else.
- `glyph gen dts` to materialize a `.d.ts` into validated Glyph types.
- Clean `.ts` output your existing TypeScript imports back.
- `import extern/<name>` reaches hand-written TypeScript under `<src>/extern/`

  **Details**

  When you need whole hand-written TypeScript, `import extern/<name>` reaches a `.ts` under `<src>/extern/`, staged and type-checked with your code, and tracked by the `glyph run` cache so editing it rebuilds.

### On the way

- Materialization handles the shapes real SDKs ship, and value-derived types are first-class

  **Details**

  Materialization now handles the shapes real SDKs ship (namespaces, cross-file and aliased/namespace re-exports, subpath `exports`, and first-class generics), and value-derived types (`z.infer<typeof s>`) are first-class.

- The main gap left is that an un-materialized package's outputs are type-checked against its `.d.ts` but not runtime-validated unless you materialize them.
- On the `extern/` side, the source root differs between `glyph run` and `glyph build`

  **Details**

  On the `extern/` side, the source root differs between the two commands: `glyph run app.glyph` roots at the file's own directory, `glyph build src/` roots at the directory you name. A program in a subdirectory therefore looks for its shim in two different places, and until we pick one root, a project laid out that way needs the shim reachable from both. It fails loudly when it does not.

Code examples on this page are checked against Glyph 0.1.155 on every build. Last changed 21 Nov 2026.
