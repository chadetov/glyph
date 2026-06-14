#!/usr/bin/env bash
# Build the playground's WebAssembly module from the Rust compiler.
#
# Produces playground/pkg/ (the wasm-bindgen --target web output the page loads).
# Requires: the wasm32-unknown-unknown target and wasm-bindgen-cli whose version
# matches the `wasm-bindgen` crate pinned in crates/glyph-wasm/Cargo.toml.
#
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.125
#
# Usage: playground/build.sh   (run from anywhere)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
compiler="$repo/glyph-compiler"

echo "building glyph-wasm for wasm32-unknown-unknown (release)…"
( cd "$compiler" && cargo build -p glyph-wasm --target wasm32-unknown-unknown --release )

wasm="$compiler/target/wasm32-unknown-unknown/release/glyph_wasm.wasm"
echo "running wasm-bindgen → $here/pkg …"
wasm-bindgen "$wasm" --out-dir "$here/pkg" --target web

# Optional size optimization if binaryen's wasm-opt is installed.
if command -v wasm-opt >/dev/null 2>&1; then
  echo "optimizing with wasm-opt…"
  wasm-opt -Oz "$here/pkg/glyph_wasm_bg.wasm" -o "$here/pkg/glyph_wasm_bg.wasm"
else
  echo "wasm-opt not found; skipping size optimization (optional)."
fi

echo "done. Serve locally with:  (cd $here && python3 -m http.server 8080)"
