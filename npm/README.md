# Glyph on npm

The `glyph` CLI is distributed on npm the way esbuild and swc are: a tiny
launcher package plus one prebuilt-binary package per platform. There is no
postinstall download and no curl-pipe-bash install script.

## What a user runs

```sh
npm install -g glyph     # or: npx glyph run app.glyph
glyph run app.glyph
```

`npm install` pulls the `glyph` launcher and, through `optionalDependencies`
filtered by `os`/`cpu`, exactly one `@glyphlang/<platform>` package carrying the
binary for the user's machine. The launcher (`glyph/bin/glyph.js`) resolves that
binary and forwards argv to it.

## Layout

```
npm/
  glyph/                  the user-facing launcher package (published as `glyph`)
    package.json          bin: glyph -> bin/glyph.js; optionalDependencies on the 5 platform packages
    bin/glyph.js          resolves the platform binary and execs it
    bin/resolve.js        platform -> @glyphlang/<platform> mapping (unit-tested)
  platform/<key>/         one package per platform (@glyphlang/<key>); binary staged at release time
    package.json          os/cpu filters so npm installs only the matching one
    bin/glyph             the prebuilt binary (staged by CI, gitignored)
  scripts/
    stage.mjs             copy a built binary into its platform package
    resolve.test.mjs      node --test for the resolution logic
```

Supported platform keys: `darwin-x64`, `darwin-arm64`, `linux-x64`,
`linux-arm64`, `win32-x64`.

## How a release is cut

The `Release` GitHub Actions workflow (`.github/workflows/release.yml`) does it
on a `v*` tag push:

1. Builds `glyph` in release mode on a native runner for each platform.
2. Stages each binary into `npm/platform/<key>/bin/` via `stage.mjs`.
3. Publishes the five `@glyphlang/<platform>` packages, then the `glyph` launcher
   (platform packages first, so the launcher's optional deps already exist).

Publishing uses npm trusted publishing, with no stored token. Each of the six
packages has a trusted publisher on npmjs.com for this repository's
`release.yml`, and the publish job exchanges its GitHub OIDC token for a
short-lived publish credential. A package without one fails at its
`npm publish`. Check all six with `npm trust list @glyphlang/<name>` before
pushing a tag, because a missing one cannot be detected before the first
package is already published.

To cut a release: bump the `version` in `npm/glyph/package.json`, the five
`npm/platform/*/package.json`, and the launcher's `optionalDependencies` to the
same number, then push a matching `vX.Y.Z` tag.

## Local development

The launcher honors a `GLYPH_BINARY` override, so you can exercise it against a
locally built binary without staging or installing anything:

```sh
cargo build -p glyph-cli --manifest-path glyph-compiler/Cargo.toml
GLYPH_BINARY="$PWD/glyph-compiler/target/debug/glyph" node npm/glyph/bin/glyph.js --version
```

Run the resolver unit tests with:

```sh
node --test npm/scripts/resolve.test.mjs
```
