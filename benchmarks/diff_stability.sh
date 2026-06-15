#!/usr/bin/env bash
# diff_stability.sh — measure how localized a one-line Glyph edit stays.
#
# The manifesto's diff-stability pillar: "a one-line change produces a one-line
# diff." This harness measures that property on Glyph's own pipeline, which is
# where it is real and reproducible (modern formatters for other languages are
# already diff-stable, so a cross-language formatter race is not meaningful).
#
# For each controlled edit to diff_stability/pricing.glyph it:
#   1. builds the unedited program to TypeScript (baseline),
#   2. applies the one-line edit to a copy and rebuilds,
#   3. counts changed lines in the Glyph source and in the emitted TypeScript,
#   4. confirms `glyph fmt` is idempotent on the edited source (no churn).
#
# Output: results/diff-stability-<timestamp>.json plus a summary to stdout.
#
# Usage: ./diff_stability.sh

set -euo pipefail

cd "$(dirname "$0")"

# Locate the release glyph binary.
GLYPH=""
for cand in \
  "../glyph-compiler/target/release/glyph" \
  "../glyph-compiler/target/debug/glyph" \
  "$(command -v glyph || true)"; do
  if [[ -n "$cand" && -x "$cand" ]]; then GLYPH="$cand"; break; fi
done
if [[ -z "$GLYPH" ]]; then
  echo "diff_stability.sh: no glyph binary found." >&2
  echo "Build one first: (cd ../glyph-compiler && cargo build --release)" >&2
  exit 1
fi

# Detect whether `glyph fmt` is implemented in this binary (older builds stub it).
FMT_OK=0
__fmt_probe=$(mktemp)
cp "$(dirname "$0")/diff_stability/pricing.glyph" "$__fmt_probe" 2>/dev/null || true
if "$GLYPH" fmt "$__fmt_probe" >/dev/null 2>&1; then FMT_OK=1; fi
rm -f "$__fmt_probe"

FIXTURE_DIR="diff_stability"
FIXTURE_FILE="pricing.glyph"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H-%M-%SZ")
OUTPUT="results/diff-stability-${TIMESTAMP}.json"
mkdir -p results

# Controlled one-line edits: "label|sed-expression". Each is a single-value
# change an agent would realistically make.
EDITS=(
  "per-seat price 12 to 10|s/pricePerSeat: 12,/pricePerSeat: 10,/"
  "plan name Starter to Solo|s/name: \"Starter\",/name: \"Solo\",/"
  "seat count 5 to 9|s/seats: 5,/seats: 9,/"
)

# Build a source dir to TypeScript; echo the emitted .ts path.
build_ts() {
  local src="$1" out="$2"
  "$GLYPH" build "$src" --out "$out" >/dev/null 2>&1
  echo "$out/${FIXTURE_FILE%.glyph}.ts"
}

# Count changed lines (one side) between two files.
changed() { diff "$1" "$2" | grep -cE "^$3" || true; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Baseline build (unedited fixture).
mkdir -p "$WORK/base"
cp "$FIXTURE_DIR/$FIXTURE_FILE" "$WORK/base/"
BASE_TS=$(build_ts "$WORK/base" "$WORK/base_out")

echo "diff-stability — one-line Glyph edit to emitted TypeScript"
echo "glyph: $GLYPH"
echo

{
  echo "{"
  echo "  \"timestamp\": \"${TIMESTAMP}\","
  echo "  \"fixture\": \"${FIXTURE_DIR}/${FIXTURE_FILE}\","
  echo "  \"edits\": ["
  FIRST=1
  for entry in "${EDITS[@]}"; do
    label="${entry%%|*}"
    expr="${entry#*|}"

    dir="$WORK/$(echo "$label" | tr ' ' '_')"
    mkdir -p "$dir"
    sed "$expr" "$FIXTURE_DIR/$FIXTURE_FILE" > "$dir/$FIXTURE_FILE"

    # Glyph source diff.
    g_minus=$(changed "$FIXTURE_DIR/$FIXTURE_FILE" "$dir/$FIXTURE_FILE" '<')
    g_plus=$(changed "$FIXTURE_DIR/$FIXTURE_FILE" "$dir/$FIXTURE_FILE" '>')

    # Emitted TypeScript diff.
    edited_ts=$(build_ts "$dir" "${dir}_out")
    t_minus=$(changed "$BASE_TS" "$edited_ts" '<')
    t_plus=$(changed "$BASE_TS" "$edited_ts" '>')

    # glyph fmt idempotence on the edited source (skipped if fmt unavailable).
    if [[ $FMT_OK -eq 1 ]]; then
      "$GLYPH" fmt "$dir/$FIXTURE_FILE" >/dev/null 2>&1 || true
      cp "$dir/$FIXTURE_FILE" "$dir/fmt1.glyph"
      "$GLYPH" fmt "$dir/fmt1.glyph" >/dev/null 2>&1 || true
      if diff -q "$dir/$FIXTURE_FILE" "$dir/fmt1.glyph" >/dev/null 2>&1; then
        fmt_idem=true
      else
        fmt_idem=false
      fi
    else
      fmt_idem=null
    fi

    printf '  edit: %-26s glyph -%s/+%s  ->  TS -%s/+%s   (fmt idempotent: %s)\n' \
      "$label" "$g_minus" "$g_plus" "$t_minus" "$t_plus" "$fmt_idem" >&2

    [[ $FIRST -eq 0 ]] && echo ","
    FIRST=0
    printf '    {"edit": "%s", "glyph_lines_removed": %s, "glyph_lines_added": %s, "ts_lines_removed": %s, "ts_lines_added": %s, "fmt_idempotent": %s}' \
      "$label" "$g_minus" "$g_plus" "$t_minus" "$t_plus" "$fmt_idem"
  done
  echo ""
  echo "  ]"
  echo "}"
} >"$OUTPUT"

echo
echo "wrote $OUTPUT"
