# Deployment

Glyph compiles to standard TypeScript, which compiles to standard JavaScript, so
a Glyph program deploys **anywhere JavaScript runs**. There is no Glyph runtime to
install on the target; you ship the emitted output and run it with the same tools
you already use for a Node or TypeScript project.

## The build artifact

`glyph build src --out dist` emits one `.ts` per module plus a source map. For
deployment you typically compile that to `.js` with `tsc` (or bundle it), and ship
the result:

```sh
glyph build src --out dist
npx tsc                     # dist/*.ts -> dist/*.js, per your tsconfig
node dist/main.js
```

What you deploy is ordinary JavaScript. Nothing about it announces that it came
from Glyph, which is the point: your ops story is your existing JavaScript ops
story.

## Targets

- **Node servers and CLIs.** The default. Run the emitted JS with `node`, or run
  the `.ts` directly with `tsx` in development.
- **Containers.** The output runs on any Node base image, including minimal ones
  (`node:*-slim`, `distroless/nodejs`), so the image is small and the attack
  surface is a plain Node app, nothing Glyph-specific to harden.
- **Serverless and edge.** Because the artifact is standard JS/TS, it deploys to
  AWS Lambda, Cloudflare Workers, Vercel, Deno Deploy, and similar. Bundle the
  emitted output the same way you would a TypeScript function; there is no cold-
  start penalty beyond the JS runtime's own.
- **The browser and other hosts.** The emitted JS runs wherever a JS engine does;
  a front-end build (via React interop) bundles like any other TypeScript. A name
  the source module exports only as a type is emitted with the inline `type`
  modifier (`import { type Option, Some, None }`), so the output is safe for a
  bundler that elides type imports *and* for a plain type stripper (`swc`,
  `node --strip-types`, Bun), and it type-checks under `verbatimModuleSyntax`.
  What the build does **not** do yet is prune: the whole standard library is
  materialized whatever you import, and the runtime lands under `.glyph-runtime`,
  a directory name most static hosts hide. A bundler's tree-shaking answers both;
  a no-bundler deployment has to walk the graph and rename that directory
  itself.

## Embedding in an existing app (Vite, React)

You don't have to write a whole application in Glyph. A common shape keeps the
domain core (models, permissions, state transitions, calculations) in `.glyph`
files and the UI and infrastructure in ordinary TypeScript:

```
src/
  glyph/         # Glyph sources: models.glyph, permissions.glyph, ...
  generated/     # glyph build output, imported by the rest of the app
  components/    # hand-written React/TSX
```

```sh
glyph build src/glyph --out src/generated
```

Then import the output like any local module:

```ts
import { TaskCard, can_edit } from "./generated/models";

const parsed = TaskCard.parse(payload);   // Result, not an exception
```

Everything the emitted code needs travels with it: imports are relative
(including the bundled standard library under `.glyph-runtime/`), and the
prelude types ride in on a `/// <reference>` from the bootstrap module every
file imports. Your project's own `tsconfig.json` and bundler need no path
aliases and no plugins; a stock Vite scaffold compiles and bundles the output
as-is. Mark what the app imports with `pub`, since a declaration without it is
private to its module and is not exported.

Two caveats. Rerun `glyph build` after editing `.glyph` sources; there is no
watch mode yet, so wire it into your `dev` script or run it by hand. And a
module that uses the Node-flavored parts of the standard library (`std/fs`,
`std/net`, `std/process`) needs `@types/node` in the host project, same as any
TypeScript that touches Node.

## Bundle size and tree-shaking

The emitted TypeScript uses plain `export`s and no barrel files, so a bundler's
**tree-shaking** removes whatever you don't import, the standard-library runtime
included. You ship only the code you actually reach. Keep imports specific
(`import std/array`, not a re-export layer) and the dead-code elimination stays
effective.

## Reproducibility

`glyph build` is deterministic: the same sources and compiler produce the same
output. Combined with a committed `package-lock.json` and an immutable npm
version pin, a deployment is reproducible from source. `glyph run` additionally
caches the build and type-check by a fingerprint of the sources, so repeated runs
of an unchanged program skip straight to execution. "Sources" means everything
the build type-checks: `.glyph` files, `.d.ts` under `.types/`, and `.ts` or
`.tsx` under `extern/`.

## What Glyph does not change

Performance, cold start, and memory are the JavaScript runtime's, not Glyph's
(see [performance](performance.md)). Glyph doesn't make deployment faster or
slower than the equivalent hand-written TypeScript; it makes the TypeScript
easier to keep correct on the way there.
