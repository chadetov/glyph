# Glyph Playground

A zero-backend web playground: write Glyph on the left, watch the TypeScript it
compiles to on the right, and see the diff-stability pillar demonstrated by an
agent edit. The compiler runs entirely in the browser as WebAssembly — no
server, no API.

## How it works

`crates/glyph-wasm` exposes one function, `compile(source) -> { ts, diagnostics }`,
that runs the exact compiler front end (parse → resolve → typecheck → emit) in
memory. It is built to `wasm32-unknown-unknown` and wrapped with `wasm-bindgen`
into `pkg/`. The page (`index.html` + `playground.js` + `style.css`) loads that
module, compiles on every debounced keystroke, and renders the output and
diagnostics. No framework, no build step for the page itself.

```
playground/
  index.html       three panes: Glyph | TypeScript | diff stability
  style.css
  playground.js    loads the wasm, debounced compile, diagnostics, diff demo
  build.sh         rebuilds pkg/ from the Rust crate
  pkg/             wasm-bindgen output (generated; gitignored)
```

## Build

You need the wasm target and a `wasm-bindgen-cli` whose version matches the
`wasm-bindgen` crate pinned in `crates/glyph-wasm/Cargo.toml` (0.2.125):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.125
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
directory to GitHub Pages on push to `main` — enable Pages (source: GitHub
Actions) in the repository settings and it ships on the next push. Any static
host (Netlify, Vercel, S3, Cloudflare Pages) works too: run `build.sh`, then
upload `playground/`.

## Notes

- The `std/*` imports in the emitted TypeScript are shown verbatim; they resolve
  against the Glyph runtime when you build a real project with `glyph build`.
- The playground analyzes a single file (no project module graph), exactly like
  the language server does for one open document.
