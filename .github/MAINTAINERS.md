# Maintainers

This file is the honest, public record of who maintains Glyph, so the project's
bus factor is visible rather than guessed at.

## Current maintainers

| Maintainer | Area | GitHub |
|---|---|---|
| Project lead | Language design, compiler, releases | [@chadetov](https://github.com/chadetov) |

## Honest status

Glyph is early and, today, **maintained primarily by one person**. That is a real
risk, and this file states it plainly rather than hiding it. Two things mitigate
it:

- **The output is yours.** Glyph compiles to plain, readable TypeScript you own
  and commit. If the project stalled tomorrow, your code keeps building with `tsc`
  and no Glyph dependency, the escape hatch described in
  [`docs/stability.md`](../docs/stability.md).
- **Permissive licensing.** [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE)
  means anyone can fork and continue.

Growing this list is an explicit goal (see
[`GOVERNANCE.md`](GOVERNANCE.md) → Becoming a maintainer). If you have been
contributing and want to help maintain, open a discussion.

## What maintainers do

- Review and merge pull requests, keeping CI green.
- Triage issues and shepherd RFCs.
- Cut releases (`v*` tag → the release workflow publishes to npm).
- Uphold the [Code of Conduct](CODE_OF_CONDUCT.md).
