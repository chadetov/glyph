#!/usr/bin/env python3
"""tokenize.py — real LLM token count for a source file.

Counts tokens with a real byte-pair tokenizer (OpenAI tiktoken) rather than
the dependency-free symbol proxy in measure.sh. This is the authoritative
density metric: it is the actual number of tokens the file costs an LLM.

The cl100k_base encoding (GPT-4 / GPT-3.5-turbo) is used as a widely cited,
reproducible standard. A different encoding would shift the absolute numbers
but, across languages expressing the same task, leaves the ranking intact.

Usage:
  python3 tokenize.py <file> [encoding]   # prints the integer token count

Requires: pip install tiktoken
"""

import sys

import tiktoken


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: tokenize.py <file> [encoding]", file=sys.stderr)
        return 2
    path = sys.argv[1]
    encoding = sys.argv[2] if len(sys.argv) > 2 else "cl100k_base"
    enc = tiktoken.get_encoding(encoding)
    with open(path, "r", encoding="utf-8") as fh:
        text = fh.read()
    print(len(enc.encode(text)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
