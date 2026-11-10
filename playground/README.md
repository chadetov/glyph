# Glyph Playground

A zero-backend web playground: write Glyph on the left, watch the TypeScript it
compiles to on the right, and see the diff-stability pillar demonstrated by an
agent edit. The compiler runs entirely in the browser as WebAssembly, with no
server and no API.

## How it works

`crates/glyph-wasm` exposes one function,
`compile(source) -> { ts, diagnostics, assumed_external_imports }`, running the
compiler front end (parse, resolve, typecheck, emit) in memory over one module.
It is built to `wasm32-unknown-unknown` and wrapped with `wasm-bindgen` into
`pkg/`. The page (`index.html` + `playground.js` + `style.css`) loads that
module, compiles on every debounced keystroke, and renders the output and
diagnostics. No framework and no bundler; the one generated file besides
`pkg/` is the example picker.

```
playground/
  index.html       three panes: Glyph | TypeScript | diff stability
  style.css
  playground.js    loads the wasm, debounced compile, diagnostics, diff demo,
                   example picker, share links
  examples/        the plain programs in the picker, one .glyph file each
  examples.js      the picker data (generated; gitignored)
  build.sh         writes examples.js, then rebuilds pkg/ from the Rust crate
  pkg/             wasm-bindgen output (generated; gitignored)
```

## Examples

The picker opens on a `catches/` case, so the first thing on screen is a
program Glyph refuses and `tsc --strict` accepts. `build.sh` runs
`scripts/gen_playground_examples.py`, which reads the cases and
`playground/examples/*.glyph` from disk on every build, so the page has no copy
of a case to fall out of date. A case is shown when its code is an `E` code and
it is one module: this page compiles one module and never runs `tsc`, so a case
refused with a `TS` code is listed by name instead. A test in
`crates/glyph-wasm` applies the same rule and compiles every shown case through
`compile`, failing if one is not refused with its code, and checks that each
program in `examples/` compiles without an error.

## Share links

Share copies a link with the program in the fragment, which the browser does
not send to a server. `#example=<slug>` names an unedited picker entry;
`#z=<base64url>` is the UTF-8 source compressed with `deflate-raw` through the
browser's `CompressionStream`, written when it is available and shorter; and
`#code=<base64url>` is the uncompressed UTF-8 source, which every browser can
read.

## Build

You need the wasm target and a `wasm-bindgen-cli` whose version is exactly the
`wasm-bindgen` crate pinned in `crates/glyph-wasm/Cargo.toml`, currently 0.2.129.
The manifest is the one place that version is written; `build.sh` reads it from
there, prints it with `--pin`, and refuses to run against any other cli version.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version "$(playground/build.sh --pin)" --locked
```

Then:

```sh
playground/build.sh
```

(Optionally install binaryen's `wasm-opt` first to shrink the binary; the script
uses it if present.)

## Run locally

ES modules and WebAssembly must be served over HTTP (not `file://`):

```sh
cd playground
python3 -m http.server 8080
# open http://localhost:8080
```

## Deploy

The static `playground/` directory (with a built `pkg/`) is all you need. The
`.github/workflows/playground.yml` workflow builds the wasm and deploys the
directory to GitHub Pages on push to `main`. Enable Pages (source: GitHub
Actions) in the repository settings and it ships on the next push. Any static
host (Netlify, Vercel, S3, Cloudflare Pages) works too: run `build.sh`, then
upload `playground/`.

## Notes

- The `std/*` imports in the emitted TypeScript are shown verbatim; they resolve
  against the Glyph runtime when you build a real project with `glyph build`.
- The playground compiles one module, so it has no project module graph. That
  changes the output as well as the analysis: six of the tables `glyph build`
  hands the emitter are read only for an imported name, so an import of a
  sibling module emits a bare specifier here and a presence check where
  `glyph build` emits a relative specifier and the check the type declares. The
  compiler reports which imports it had to assume were npm packages in
  `assumed_external_imports`, and the page says so beside the output. See G170
  in `docs/dogfooding-gaps.md`.
- The project scan itself is shared with the CLI (`glyph_emit::ProjectTables`),
  so a table the emitter grows reaches both surfaces rather than one.
