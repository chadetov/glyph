# sitegen

## What it is

A static site generator over Markdown. It reads a content directory, splits
YAML front matter with `gray-matter`, validates that front matter at the
boundary, renders the body with `marked`, sorts posts newest first, and writes
one HTML file per post plus an index. A post with bad front matter is reported
with its slug and counted as a failure rather than aborting the run.

## Running it

```sh
glyph run examples/apps/sitegen/main.glyph
glyph run examples/apps/sitegen/main.glyph content site
```

## What it changed in Glyph

Shipped **0.1.74**, and its finding blocks a whole class of npm package.

**G112: Glyph had no default-import form, so a CommonJS `export =` callable
package was unreachable.** The single widest interop gap found so far. A package
whose export *is* a function cannot be called at all, and all three import
spellings failed differently: one a TypeScript error, one another, and one a
parse error. The reach is express, lodash, debug, chalk, minimist, commander,
and this app's own `gray-matter`.

The app shipped with the workaround in place and a comment saying why: a named
export reached through the same namespace compiles and runs, so it used that
instead of the documented entry point. The fix added `import pkg { default as p
}`, with `as` legal only after `default`, and it was verified against
gray-matter's own documented entry point rather than a synthetic case.

## What it exercises

Two real npm dependencies imported by bare name with no adapter and no
`.d.ts`, the D15 `default as` form, boundary validation returning the compiler's
own `Issue` type, and array patterns with rest for argument defaults.
