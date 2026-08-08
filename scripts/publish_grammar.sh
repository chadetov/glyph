#!/usr/bin/env bash
# Mirror the TextMate grammar to the standalone repo Linguist vendors.
#
# The grammar is developed here, in editors/vscode/syntaxes/, and mirrored to
# chadetov/glyph-tmlanguage because Linguist vendors a grammar from its own repo
# (script/add-grammar <url>) and requires a whitelisted license on it.
#
# Two copies can drift, and a stale mirror means GitHub highlights .glyph files
# with a grammar that no longer matches the language. Run this after any change
# to the grammar; it is a no-op when the two already agree.
#
#   ./scripts/publish_grammar.sh            # mirror if changed
#   ./scripts/publish_grammar.sh --check    # exit 1 if they differ, change nothing
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/editors/vscode/syntaxes/glyph.tmLanguage.json"
REPO="git@github.com:chadetov/glyph-tmlanguage.git"
WORK="${TMPDIR:-/tmp}/glyph-tmlanguage-mirror"

[ -f "$SRC" ] || { echo "missing $SRC"; exit 1; }

rm -rf "$WORK"
git clone -q --depth 1 "$REPO" "$WORK"

if diff -q "$SRC" "$WORK/glyph.tmLanguage.json" >/dev/null 2>&1; then
  echo "grammar mirror is up to date"
  rm -rf "$WORK"
  exit 0
fi

if [ "${1:-}" = "--check" ]; then
  echo "grammar has diverged from chadetov/glyph-tmlanguage:"
  diff "$WORK/glyph.tmLanguage.json" "$SRC" || true
  echo
  echo "run ./scripts/publish_grammar.sh to mirror it"
  rm -rf "$WORK"
  exit 1
fi

cp "$SRC" "$WORK/glyph.tmLanguage.json"
# Samples travel with it: a grammar change is checked against real programs.
cp "$ROOT/examples/apps/minesweeper.glyph" "$WORK/samples/minesweeper.glyph"
cp "$ROOT/examples/apps/expenses.glyph" "$WORK/samples/expenses.glyph"
cp "$ROOT/examples/apps/csvql/sql.glyph" "$WORK/samples/sql.glyph"

git -C "$WORK" add -A
git -C "$WORK" commit -q -m "Update the grammar from the main repository"
git -C "$WORK" push -q origin main
echo "mirrored to chadetov/glyph-tmlanguage"
rm -rf "$WORK"
