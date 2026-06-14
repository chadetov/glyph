#!/usr/bin/env bash
# measure.sh — token count, line count, diff size per function × language.
# Output: results/<timestamp>.json
#
# Counts lines and an approximate token count (identifier/number runs plus
# standalone symbols — dependency-free, a stable proxy for code density; not
# tiktoken-exact). Diff-size (against edits/<function>.patch) remains a later
# enhancement.
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

count_tokens() {
  # Approximate LLM token count: each identifier/number run is one token, and
  # each standalone symbol is one. Dependency-free; a stable proxy for density,
  # not a tiktoken-exact count.
  local file="$1"
  grep -oE '[A-Za-z0-9_]+|[^[:space:][:alnum:]_]' "$file" | wc -l | tr -d ' '
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
      tokens=$(count_tokens "$file")
      [[ $FIRST -eq 0 ]] && echo ","
      FIRST=0
      printf '    {"function": "%s", "language": "%s", "file": "%s", "lines": %s, "tokens": %s, "diff_size": null}' \
        "$fn" "$lang" "$file" "$lines" "$tokens"
    done
  done
  echo ""
  echo "  ]"
  echo "}"
} >"$OUTPUT"

echo "wrote $OUTPUT"
