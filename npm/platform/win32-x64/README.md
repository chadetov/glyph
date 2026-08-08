# @glyphlang/win32-x64

The prebuilt `glyph` binary for Windows (x64).

You do not install this package directly. It is an optional dependency of
[`@glyphlang/glyph`](https://www.npmjs.com/package/@glyphlang/glyph), which picks
the one package matching your platform and runs the binary inside it:

```sh
npm install -g @glyphlang/glyph
```

The binary is built and published from source by the release workflow in
[chadetov/glyph](https://github.com/chadetov/glyph), with npm provenance and a
SLSA build attestation, so it can be traced to the commit and CI run that
produced it:

```sh
npm audit signatures
```

Glyph is a statically typed language that transpiles to TypeScript, designed so
AI agents can read, write, and modify code safely. See
[glyphlang.io](https://glyphlang.io).

Licensed MIT OR Apache-2.0.
