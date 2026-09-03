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

## What it found, and what happened

**G112: Glyph had no default-import form, so a CommonJS `export =` callable
package was unreachable.** The single widest interop gap found so far. A package
whose export *is* a function cannot be called at all, and all three import
spellings failed differently: one a TypeScript error, one another, and one a
parse error. The reach is express, lodash, debug, chalk, minimist, commander,
and this app's own `gray-matter`, whose documented entry point is the callable
`matter(text)`.

The app first shipped with a workaround in place: a named export reached
through the same `export =` namespace compiles and runs, so `load.glyph`
called that instead of the documented entry point, with a comment explaining
why. **Fixed in 0.1.74**, which added a fourth D15 import form,
`import pkg { default as p }`, with `as` legal only after `default` so general
renaming stays closed. `load.glyph` now imports gray-matter the documented
way:

```
import gray-matter { default as matter }
```

There is no workaround left in this app. The import above is the real entry
point, not a substitute for it, and `glyph check .` against this app passes
clean today: 4 modules, no diagnostics, `tsc --strict` passed.

## What it exercises

Two real npm dependencies imported by bare name with no adapter and no
`.d.ts`, the D15 `default as` form, boundary validation returning the compiler's
own `Issue` type, and array patterns with rest for argument defaults.
