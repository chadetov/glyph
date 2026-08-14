#!/usr/bin/env python3
"""Release discipline: clippy passes locally before anything is pushed.

CI runs `cargo clippy --workspace --all-targets --locked -- -D warnings`, and
`cargo build` and `cargo test` do not. So a lint finding is invisible to every
local check and only surfaces after a push, a PR, and a CI round-trip.

That is not hypothetical. Cutting 0.1.74 pushed a release branch whose only
defect was a helper left behind after its last caller was replaced; `-D warnings`
turns dead code into an error, so CI failed the release and the fix, the
re-push, and the second CI run cost more than the check would have. The suite was
green the whole time, which is exactly why a green suite is not the signal.

This runs the same command with the same flags, so a local pass means the same
thing CI's pass means. Run it with the other gates before a release commit.

Hard-fails (exit 1) when clippy reports anything, or when cargo is missing.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKSPACE = ROOT / "glyph-compiler"

# Byte-for-byte what `.github/workflows/ci.yml` runs in its Clippy step. If that
# changes, change this; a gate that checks something else is worse than none,
# because it reports a pass CI will not honour.
COMMAND = [
    "cargo",
    "clippy",
    "--workspace",
    "--all-targets",
    "--locked",
    "--",
    "-D",
    "warnings",
]


def main() -> int:
    if not WORKSPACE.is_dir():
        print(f"missing {WORKSPACE.relative_to(ROOT)}")
        return 1

    try:
        result = subprocess.run(
            COMMAND,
            cwd=WORKSPACE,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        print("cargo not found on PATH; clippy is part of the release gate.")
        return 1

    if result.returncode == 0:
        print("clippy OK: --workspace --all-targets --locked -- -D warnings")
        return 0

    # Clippy writes findings to stderr. Print them verbatim: the whole value of
    # running it locally is reading the same text CI would have shown, without
    # waiting for CI to show it.
    sys.stderr.write(result.stderr)
    print(
        "\nclippy failed. CI runs this exact command with `-D warnings`, so a "
        "push would fail the same way.",
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
