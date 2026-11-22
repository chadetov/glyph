<!-- Markdown copy of https://glyphlang.io/answers/embedding/, generated from the same source as the page. Last changed 2026-11-21. -->

10 · Inside your app

# Can I adopt Glyph inside my existing app, or is it all-or-nothing?

No rewrite. Keep the app in TypeScript and write the part you can't afford to get wrong (models, permissions, state transitions, money math) in Glyph. `glyph build` emits modules your app imports **like any local file**, with no bundler config and no path aliases.

A hybrid layout: Glyph domain core, TypeScript everything else

```
src/
  glyph/          # models.glyph, permissions.glyph, analytics.glyph
  generated/      # glyph build --out src/generated src/glyph
  components/     # hand-written React/TSX

// BoardContext.tsx: the app imports compiled Glyph like a local module
import { TaskCard, can_move } from "../generated/models";

const parsed = TaskCard.parse(payload);  // Result, not an exception
```

One directory of Glyph sources, compiled into the app, imported by the UI.

## Why it matters

Most teams can't rewrite an application, and most of an application doesn't need Glyph. The part that does is the domain core: the rules an AI agent will edit next month and nobody wants silently wrong.

Keeping that core in Glyph gives you exhaustive `match`, no casts, and validated boundaries exactly where they pay, while the React components, the styling, and the infrastructure stay plain TypeScript.

## Nothing to add in the host

Everything the emitted code needs travels with it.

- **Relative imports** Imports are relative, including the bundled standard library, so your project's `tsconfig.json` and bundler need nothing added: a stock Vite scaffold compiles and bundles the output as-is, and the same files pass a host `tsc --strict`.
- **`pub` exports** Mark what the app imports with `pub`; a declaration without it is private to its module.

This isn't a hypothetical: an outside engineer [built a React Kanban board this way](https://github.com/yildizadem/glyph-kanban), with the RBAC matrix, the analytics formulas, and the status transitions in four Glyph modules under a hand-written React UI.

## Where it stands

### Shipping today

- Emitted output drops into a host project unchanged: relative specifiers throughout, prelude types carried by the code itself.
- Verified against a stock Vite scaffold, host `tsc --strict`, esbuild, and `tsx`.
- The [deployment guide](https://github.com/chadetov/glyph/blob/main/docs/guide/deployment.md) documents the hybrid layout.

### On the way

- No watch mode yet: rerun `glyph build` after editing `.glyph` sources (wire it into your `dev` script).
- Modules using `std/fs` or `std/net` need `@types/node` in the host, like any Node-touching TypeScript.

Code examples on this page are checked against Glyph 0.1.164 on every build. Last changed 21 Nov 2026.
