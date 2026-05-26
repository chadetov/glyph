#!/usr/bin/env bash
# measure.sh — token count, line count, diff size per function × language.
# Output: results/<timestamp>.json
#
# Phase 0 stub: counts lines only. Token counting (tiktoken) and diff-size
# measurement (against edits/<function>.patch) wire up in phase 1 week 8.
#
# Usage:
#   ./measure.sh                    # measure all functions × all languages
#   ./measure.sh <function_name>    # measure one function across all languages

set -euo pipefail

cd "$(dirname "$0")"

FUNCTION_FILTER="${1:-}"
LANGUAGES=(glyph typescript python rust)
EXTENSIONS=(glyph ts py rs)

TIMESTAMP=$(date -u +"%Y-%m-%dT%H-%M-%SZ")
OUTPUT="results/${TIMESTAMP}.json"
mkdir -p results

count_lines() {
  # Lines excluding blank and full-line comments.
  local file="$1"
  local ext="${file##*.}"
  case "$ext" in
    glyph|ts|rs) grep -cvE '^\s*(//|$)' "$file" ;;
    py)          grep -cvE '^\s*(#|$)' "$file" ;;
    *)           wc -l <"$file" ;;
  esac
}

discover_functions() {
  # Take filenames from glyph/ as the canonical set.
  for f in glyph/*.glyph; do
    basename "$f" .glyph
  done
}

{
  echo "{"
  echo "  \"timestamp\": \"${TIMESTAMP}\","
  echo "  \"measurements\": ["

  FIRST=1
  for fn in $(discover_functions); do
    if [[ -n "$FUNCTION_FILTER" && "$fn" != "$FUNCTION_FILTER" ]]; then
      continue
    fi
    for i in "${!LANGUAGES[@]}"; do
      lang="${LANGUAGES[$i]}"
      ext="${EXTENSIONS[$i]}"
      file="${lang}/${fn}.${ext}"
      if [[ ! -f "$file" ]]; then
        continue
      fi
      lines=$(count_lines "$file")
      [[ $FIRST -eq 0 ]] && echo ","
      FIRST=0
      printf '    {"function": "%s", "language": "%s", "file": "%s", "lines": %s, "tokens": null, "diff_size": null}' \
        "$fn" "$lang" "$file" "$lines"
    done
  done
  echo ""
  echo "  ]"
  echo "}"
} >"$OUTPUT"

echo "wrote $OUTPUT"
