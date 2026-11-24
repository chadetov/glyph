#!/usr/bin/env python3
"""Generate the VS Code extension's TextMate grammar from the compiler.

The keyword rules come from `KEYWORDS` in glyph-lexer's token.rs, the table the
lexer itself uses, and the primitive types from `is_primitive_type_name` in
glyph-resolver's reserved.rs. The one thing this script decides is which scope
each keyword gets, and that table must cover the lexer's table exactly: a
keyword added to the lexer and not classified here is an error, not a word the
editor silently leaves uncoloured.

    python3 scripts/gen_grammar.py           # rewrite the grammar
    python3 scripts/gen_grammar.py --check   # exit 1 if it is out of date

Writes editors/vscode/syntaxes/glyph.tmLanguage.json. After a change, run
scripts/publish_grammar.sh --check to see whether the Linguist mirror differs.
Python standard library only.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TOKEN_RS = ROOT / "glyph-compiler" / "crates" / "glyph-lexer" / "src" / "token.rs"
RESERVED_RS = ROOT / "glyph-compiler" / "crates" / "glyph-resolver" / "src" / "reserved.rs"
GRAMMAR = ROOT / "editors" / "vscode" / "syntaxes" / "glyph.tmLanguage.json"

# Scope for every reserved word. Order within a group is the order the words
# appear in the generated regex, sorted for a stable diff.
DECLARATION = "storage.type.glyph"
CONTROL = "keyword.control.glyph"
OTHER = "keyword.other.glyph"
CONSTANT = "constant.language.glyph"

SCOPE_OF = {
    "component": DECLARATION,
    "fn": DECLARATION,
    "import": DECLARATION,
    "interface": DECLARATION,
    "module": DECLARATION,
    "pub": DECLARATION,
    "record": DECLARATION,
    "type": DECLARATION,
    "async": CONTROL,
    "await": CONTROL,
    "break": CONTROL,
    "continue": CONTROL,
    "defer": CONTROL,
    "else": CONTROL,
    "for": CONTROL,
    "if": CONTROL,
    "in": CONTROL,
    "loop": CONTROL,
    "match": CONTROL,
    "return": CONTROL,
    "as": OTHER,
    "const": OTHER,
    "is": OTHER,
    "let": OTHER,
    "mut": OTHER,
    "new": OTHER,
    "owned": OTHER,
    "resource": OTHER,
    "void": OTHER,
    "where": OTHER,
    "false": CONSTANT,
    "true": CONSTANT,
}


class GrammarError(Exception):
    pass


def lexer_keywords() -> list[str]:
    src = TOKEN_RS.read_text()
    m = re.search(r"static KEYWORDS: &\[\(&str, Token\)\] = &\[(.*?)\n\];", src, re.S)
    if not m:
        raise GrammarError(f"{TOKEN_RS.relative_to(ROOT)}: no KEYWORDS table to read")
    words = re.findall(r'^\s*\("([a-z_]+)", Token::', m.group(1), re.M)
    if len(words) < 20:
        raise GrammarError(f"{TOKEN_RS.relative_to(ROOT)}: KEYWORDS table read as {words}")
    return words


def primitive_types() -> list[str]:
    src = RESERVED_RS.read_text()
    m = re.search(r"fn is_primitive_type_name\(name: &str\) -> bool \{(.*?)\n\}", src, re.S)
    if not m:
        raise GrammarError(f"{RESERVED_RS.relative_to(ROOT)}: no is_primitive_type_name to read")
    names = re.findall(r'"([a-z]+)"', m.group(1))
    if len(names) < 5:
        raise GrammarError(f"{RESERVED_RS.relative_to(ROOT)}: primitive types read as {names}")
    return names


def words_rule(scope: str, words: list[str]) -> dict:
    return {"name": scope, "match": "\\b(" + "|".join(words) + ")\\b"}


def build() -> dict:
    keywords = lexer_keywords()
    unclassified = [w for w in keywords if w not in SCOPE_OF]
    stale = [w for w in SCOPE_OF if w not in keywords]
    if unclassified:
        raise GrammarError(
            f"keywords in the lexer with no scope in {pathlib.Path(__file__).name}: "
            f"{', '.join(unclassified)}. Add each to SCOPE_OF."
        )
    if stale:
        raise GrammarError(
            f"SCOPE_OF names words the lexer no longer reserves: {', '.join(stale)}"
        )

    def group(scope: str) -> list[str]:
        return sorted(w for w in keywords if SCOPE_OF[w] == scope)

    return {
        "$schema": "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
        "name": "Glyph",
        "scopeName": "source.glyph",
        "patterns": [
            {"include": "#" + name}
            for name in (
                "comments", "strings", "annotations", "declarations", "keywords",
                "constants", "numbers", "types", "calls", "members", "operators",
            )
        ],
        "repository": {
            "comments": {"patterns": [
                {"name": "comment.line.double-slash.glyph", "match": "//.*$"},
            ]},
            "strings": {"patterns": [
                {"name": "string.quoted.triple.glyph", "begin": '"""', "end": '"""'},
                {
                    "name": "string.quoted.double.glyph",
                    "begin": '"',
                    "end": '"',
                    "patterns": [
                        {"name": "constant.character.escape.glyph", "match": "\\\\."},
                        {
                            "name": "meta.template.expression.glyph",
                            "begin": "\\$\\{",
                            "end": "\\}",
                            "beginCaptures": {"0": {
                                "name": "punctuation.definition.template-expression.begin.glyph",
                            }},
                            "endCaptures": {"0": {
                                "name": "punctuation.definition.template-expression.end.glyph",
                            }},
                            "patterns": [{"include": "$self"}],
                        },
                    ],
                },
            ]},
            "annotations": {"patterns": [
                {"name": "entity.name.tag.annotation.glyph", "match": "@[A-Za-z_][A-Za-z0-9_]*"},
            ]},
            "declarations": {"patterns": [words_rule(DECLARATION, group(DECLARATION))]},
            "keywords": {"patterns": [
                words_rule(CONTROL, group(CONTROL)),
                words_rule(OTHER, group(OTHER)),
            ]},
            "constants": {"patterns": [words_rule(CONSTANT, group(CONSTANT))]},
            "types": {"patterns": [
                words_rule("support.type.primitive.glyph", primitive_types()),
                {
                    "name": "entity.name.function.constructor.glyph",
                    "match": "\\b([A-Z][A-Za-z0-9_]*)(?=\\s*\\()",
                },
                {"name": "entity.name.type.glyph", "match": "\\b[A-Z][A-Za-z0-9_]*\\b"},
            ]},
            "numbers": {"patterns": [
                {"name": "constant.numeric.glyph", "match": "\\b[0-9]+(\\.[0-9]+)?\\b"},
            ]},
            "calls": {"patterns": [
                {"name": "entity.name.function.call.glyph", "match": "\\b[a-z_][A-Za-z0-9_]*(?=\\()"},
            ]},
            "operators": {"patterns": [
                {"name": "keyword.operator.arrow.glyph", "match": "->|=>"},
                {"name": "keyword.operator.comparison.glyph", "match": "==|!=|<=|>=|<|>"},
                {"name": "keyword.operator.logical.glyph", "match": "&&|\\|\\||!"},
                {"name": "keyword.operator.union.glyph", "match": "\\|"},
                {"name": "keyword.operator.try.glyph", "match": "\\?"},
                {"name": "keyword.operator.assignment.glyph", "match": "="},
                {"name": "keyword.operator.arithmetic.glyph", "match": "\\+|-|\\*|/|%"},
                {"name": "punctuation.accessor.glyph", "match": "\\."},
                {"name": "punctuation.separator.glyph", "match": "[,:;]"},
            ]},
            "members": {"patterns": [
                {
                    "name": "variable.other.property.glyph",
                    "match": "(?<=\\.)\\s*[a-z_][A-Za-z0-9_]*\\b",
                },
            ]},
        },
    }


def render() -> str:
    return json.dumps(build(), indent=2) + "\n"


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    try:
        text = render()
    except GrammarError as e:
        print(f"gen_grammar: {e}", file=sys.stderr)
        return 1
    current = GRAMMAR.read_text() if GRAMMAR.exists() else ""
    rel = GRAMMAR.relative_to(ROOT)
    if text == current:
        print(f"{rel} is up to date")
        return 0
    if check:
        print(f"{rel} is out of date; run python3 scripts/gen_grammar.py", file=sys.stderr)
        return 1
    GRAMMAR.write_text(text)
    print(f"wrote {rel}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
