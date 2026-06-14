#!/usr/bin/env bash
# Verifiability demo: each <case>.glyph contains a bug that Glyph rejects at
# compile time, paired with <case>.ts containing the equivalent bug that
# `tsc --strict` accepts. This script asserts that invariant: every .glyph must
# FAIL to compile and every .ts must PASS tsc --strict. Exit 0 only if all hold.
#
# Requires `glyph` and `tsc` on PATH (or set GLYPH to the binary).
set -uo pipefail

cd "$(dirname "$0")"
GLYPH="${GLYPH:-glyph}"
fail=0

for g in *.glyph; do
  tmp="$(mktemp -d)"
  cp "$g" "$tmp/"
  if "$GLYPH" build "$tmp" --out "$tmp/out" --no-check >/dev/null 2>&1; then
    echo "FAIL: $g compiled, but it should be rejected"
    fail=1
  else
    echo "ok:   $g is rejected by glyph (as intended)"
  fi
  rm -rf "$tmp"
done

for t in *.ts; do
  if tsc --strict --noEmit --target es2022 --lib es2022 "$t" >/dev/null 2>&1; then
    echo "ok:   $t is accepted by tsc --strict (the bug TypeScript misses)"
  else
    echo "FAIL: $t was rejected by tsc, but the demo needs it to compile"
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "all verifiability demos hold: glyph catches what tsc --strict misses"
fi
exit "$fail"
