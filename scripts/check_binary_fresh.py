#!/usr/bin/env python3
"""The compiler you are about to verify against is built from the code you have.

Run this first in a gap-closing round. A `target/release/glyph` older than the
crate sources or the runtime directory does not fail loudly, it fails
plausibly: building the examples tree against one produced `Cannot find name
'net'` against a file that was correct, because the node shims that declare
`net` had landed in the runtime after the binary was built. That is
indistinguishable from a real gap until you rebuild.

Hard-fails (exit 1) when the binary is older than any crate source, any runtime
file, or the workspace manifests.
"""

from __future__ import annotations

import sys

import glyph_bin


def main() -> int:
    binary = glyph_bin.find()
    if binary is None:
        print("no glyph binary found.")
        print("  cd glyph-compiler && cargo build --release")
        return 1

    newer = glyph_bin.staleness(binary)
    rel = binary.relative_to(glyph_bin.ROOT) if glyph_bin.ROOT in binary.parents else binary

    if newer is not None:
        print(f"stale compiler: {rel} predates {newer.relative_to(glyph_bin.ROOT)}.")
        print()
        print("Rebuild before trusting anything it says:")
        print("  cd glyph-compiler && cargo build --release")
        return 1

    print(f"compiler OK: {rel} is newer than every crate source and runtime file.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
