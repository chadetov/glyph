# Distribution and supply chain

Glyph does not run its own package registry, and it does not need to. It compiles
to TypeScript and publishes to **npm**, so a Glyph library is an npm package and
inherits the whole npm supply chain: the registry, versioning, search, scoping,
auditing, mirrors, and private hosting. This page is the concrete map of what you
get and how to use it.

## Publishing a package

`glyph publish` builds, type-checks, audit-gates, and publishes to npm:

```sh
glyph publish            # in a project with a package.json carrying the "glyph" key
```

Under the hood it emits the TypeScript, runs `tsc --strict`, and publishes the
package. Consumers install it with `npm install your-package` and import the
emitted `.ts`/types like any other dependency.

## The registry you get

- **One canonical registry.** npm is the single place to publish and find Glyph
  packages, no fragmentation, no second index to learn.
- **Search and discovery.** [npmjs.com](https://www.npmjs.com) search, download
  counts, last-publish date, and dependents all apply, so you can judge whether a
  package is alive before depending on it.
- **Scopes and namespacing.** Publish under a scope (`@your-org/pkg`) to avoid
  name collisions and typosquatting; Glyph's own packages live under
  `@glyphlang/*`.
- **Immutable, mirrored versions.** A published version is never mutated or
  removed out from under a build, and npm's CDN mirrors it globally, so a build
  that works today works in years.
- **Semantic versioning.** npm's semver ranges and resolver apply unchanged, and
  a project commits a `package-lock.json` so every machine resolves the identical
  tree.
- **Multiple major versions coexist.** npm lets `foo@1` and `foo@2` sit in one
  dependency tree, so a diamond dependency doesn't deadlock.

## Consuming packages

Glyph imports installed npm packages directly, `import zod { z }` resolves
against the package's own published types. For deep boundary validation of a
package's data types, materialize them with `glyph gen dts <package>`. See
[external-imports](external-imports.md).

## Supply-chain hygiene

- **Provenance.** Glyph's own releases publish with [npm provenance
  attestations](https://docs.npmjs.com/generating-provenance-statements), tying
  the package to the source commit and CI run that built it. Do the same for your
  packages with `npm publish --provenance`.
- **Auditing.** `npm audit` (and Dependabot, Socket, or your scanner of choice)
  works on a Glyph project's dependencies exactly as on any npm project.
- **Private and self-hosted registries.** Point npm at a private registry
  (Verdaccio, GitHub Packages, Artifactory) with an `.npmrc`; `glyph publish` and
  the generated tsconfig respect it, so internal packages stay behind your
  firewall.
- **License metadata.** The `license` field in `package.json` is machine-readable
  for compliance scanners; Glyph itself is dual [MIT](../../LICENSE-MIT) /
  [Apache-2.0](../../LICENSE-APACHE).

## The escape hatch

Because the published artifact is plain, readable TypeScript you own, a Glyph
dependency is never a lock-in: the consumer gets `.ts` that builds with `tsc`
whether or not they use Glyph. That is the deliberate trade of riding npm instead
of building a parallel ecosystem.
