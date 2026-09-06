#!/usr/bin/env bash
# Build the playground's WebAssembly module from the Rust compiler.
#
# Produces playground/pkg/ (the wasm-bindgen --target web output the page loads).
# Requires the wasm32-unknown-unknown target and a wasm-bindgen-cli whose version
# is exactly the `wasm-bindgen` crate pinned in crates/glyph-wasm/Cargo.toml.
# That manifest is the only place the version is written; this script reads it
# from there and refuses to run against any other cli version, printing the
# install command for the right one.
#
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version "$(playground/build.sh --pin)" --locked
#
# Usage: playground/build.sh          (run from anywhere)
#        playground/build.sh --pin    print the pinned wasm-bindgen version and exit
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
compiler="$repo/glyph-compiler"
manifest="$compiler/crates/glyph-wasm/Cargo.toml"

pin="$(sed -nE 's/^wasm-bindgen[[:space:]]*=[[:space:]]*"=([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' "$manifest")"
if [[ -z "$pin" ]]; then
  echo "error: no exact wasm-bindgen pin (wasm-bindgen = \"=X.Y.Z\") in $manifest" >&2
  exit 1
fi

if [[ "${1:-}" == "--pin" ]]; then
  echo "$pin"
  exit 0
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "error: wasm-bindgen-cli is not installed. The crate pins $pin, so install that:" >&2
  echo "  cargo install wasm-bindgen-cli --version $pin --locked" >&2
  exit 1
fi
have="$(wasm-bindgen --version | awk '{print $2}')"
if [[ "$have" != "$pin" ]]; then
  echo "error: wasm-bindgen-cli $have is installed but the crate pins $pin; they must match exactly." >&2
  echo "  cargo install wasm-bindgen-cli --version $pin --locked" >&2
  exit 1
fi

echo "building glyph-wasm for wasm32-unknown-unknown (release)…"
( cd "$compiler" && cargo build -p glyph-wasm --target wasm32-unknown-unknown --release )

wasm="$compiler/target/wasm32-unknown-unknown/release/glyph_wasm.wasm"
echo "running wasm-bindgen $have → $here/pkg …"
wasm-bindgen "$wasm" --out-dir "$here/pkg" --target web

# Optional size optimization if binaryen's wasm-opt is installed.
if command -v wasm-opt >/dev/null 2>&1; then
  echo "optimizing with wasm-opt…"
  wasm-opt -Oz "$here/pkg/glyph_wasm_bg.wasm" -o "$here/pkg/glyph_wasm_bg.wasm"
else
  echo "wasm-opt not found; skipping size optimization (optional)."
fi

echo "done. Serve locally with:  (cd $here && python3 -m http.server 8080)"
