# Glyph guide

Task-oriented documentation. For the design rationale read
[`../manifesto.md`](../manifesto.md); for the precise rules read
[`../language/spec.md`](../language/spec.md).

**Time to productivity:** a working TypeScript developer is writing real Glyph in
about an hour, five minutes for the [tour](tour.md), thirty for the
[TypeScript-developer deltas](for-typescript-developers.md), and the rest in the
[tutorial](tutorial.md). The one thing that trips people up is trying to write
TypeScript and translate; read [how to think in Glyph](how-to-think.md) first and
that hour is smoother.

## A path through it

**Start:** [tour](tour.md) → [getting-started](getting-started.md) →
[how to think in Glyph](how-to-think.md).
**Coming from TypeScript:** [for-typescript-developers](for-typescript-developers.md),
then [tutorial](tutorial.md).
**Building something:** [cookbook](cookbook.md) for recipes,
[idioms](idioms.md) for conventions, [typed-apis](typed-apis.md) and
[external-imports](external-imports.md) for real dependencies.
**When you're stuck:** [troubleshooting](troubleshooting.md),
[anti-patterns](anti-patterns.md), [error codes](../error-codes.md).

## Every page

| Read this | When |
|---|---|
| [`tour.md`](tour.md) | You have five minutes and want to see the whole language |
| [`getting-started.md`](getting-started.md) | You want to install Glyph and run your first program |
| [`how-to-think.md`](how-to-think.md) | You want the mental model, so the rules feel obvious not restrictive |
| [`for-typescript-developers.md`](for-typescript-developers.md) | You know TypeScript and want the deltas and gotchas |
| [`tutorial.md`](tutorial.md) | You want to build something real (a todo CLI) |
| [`cookbook.md`](cookbook.md) | You want a paste-and-adapt recipe for a specific task |
| [`idioms.md`](idioms.md) | You want the conventions fluent Glyph follows |
| [`anti-patterns.md`](anti-patterns.md) | You want to know which habits fight the language |
| [`performance.md`](performance.md) | You want to know what is cheap and where the runtime cost is |
| [`troubleshooting.md`](troubleshooting.md) | A build or run failed and you want the fix |
| [`migration.md`](migration.md) | You want to adopt Glyph in an existing TypeScript project |
| [`distribution.md`](distribution.md) | You want to publish a package and understand the npm-backed supply chain |
| [`deployment.md`](deployment.md) | You want to know where a Glyph program runs (node, serverless, edge, containers) |
| [`typed-apis.md`](typed-apis.md) | You are building an API and want validated request/response DTOs without a separate `zod` schema |
| [`editor-setup.md`](editor-setup.md) | You want VS Code (or another editor) set up with diagnostics, hover, and format-on-save |
| [`external-imports.md`](external-imports.md) | You want to import an npm package or Node builtin (and how `.types/` works) |
| [`../reference/stdlib.md`](../reference/stdlib.md) | You want every std module and its exact signatures |
| [`../language/spec.md`](../language/spec.md) | You need the exact language reference |
| [`../error-codes.md`](../error-codes.md) | You hit a diagnostic and want the fix |

Every code sample in this guide compiles with the current compiler.

## Improving these docs

Found something wrong or unclear? Every page has an **Edit** button on GitHub
(top-right of the file view), or open a pull request against
[`docs/guide/`](https://github.com/chadetov/glyph/tree/main/docs/guide). Small
fixes, a clearer sentence, a missing recipe, are welcome and reviewed quickly.
