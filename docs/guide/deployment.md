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
  a front-end build (via React interop) bundles like any other TypeScript.

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
