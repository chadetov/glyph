#!/usr/bin/env bash
# measure.sh — token count, line count, diff size per function × language.
# Output: results/<timestamp>.json
#
# Counts lines and a token count. When tiktoken is installed the token count is
# a real LLM token count (count_tokens.py, cl100k_base); otherwise it falls back to
# the dependency-free symbol proxy. The "tokenizer" field in the output records
# which method produced the numbers. Diff size is populated by diff_stability.sh.
#
# Usage:
#   ./measure.sh                    # measure all functions × all languages
#   ./measure.sh <function_name>    # measure one function across all languages

set -euo pipefail

cd "$(dirname "$0")"

FUNCTION_FILTER="${1:-}"
LANGUAGES=(glyph typescript python rust go)
EXTENSIONS=(glyph ts py rs go)

TIMESTAMP=$(date -u +"%Y-%m-%dT%H-%M-%SZ")
OUTPUT="results/${TIMESTAMP}.json"
mkdir -p results

# Prefer the real tokenizer (tiktoken via tokenize.py); fall back to the proxy.
ENCODING="cl100k_base"
if python3 -c "import tiktoken" >/dev/null 2>&1; then
  TOKENIZER="$ENCODING"
else
  TOKENIZER="approx-proxy"
fi

count_lines() {
  # Lines excluding blank and full-line comments.
  local file="$1"
  local ext="${file##*.}"
  case "$ext" in
    glyph|ts|rs|go) grep -cvE '^\s*(//|$)' "$file" ;;
    py)          grep -cvE '^\s*(#|$)' "$file" ;;
    *)           wc -l <"$file" ;;
  esac
}

count_tokens_proxy() {
  # Dependency-free proxy: each identifier/number run is one token, and each
  # standalone symbol is one. A stable density proxy, not a tiktoken-exact count.
  local file="$1"
  grep -oE '[A-Za-z0-9_]+|[^[:space:][:alnum:]_]' "$file" | wc -l | tr -d ' '
}

count_tokens() {
  # Real LLM token count when tiktoken is available; proxy otherwise.
  local file="$1"
  if [[ "$TOKENIZER" != "approx-proxy" ]]; then
    python3 count_tokens.py "$file" "$ENCODING"
  else
    count_tokens_proxy "$file"
  fi
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
  echo "  \"tokenizer\": \"${TOKENIZER}\","
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
