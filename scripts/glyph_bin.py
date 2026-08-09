"""Finding the compiler, and refusing to use a stale one.

Shared by the checks that run the compiler against the docs. A stale binary is
not a neutral inconvenience: it reports failures that are not real. Building the
examples tree against a `target/release/glyph` older than the node-shim commit
produced `Cannot find name 'net'` on a file that was correct, which reads
exactly like a compiler gap and would have been written up as one.

Staleness is mtime against the crate sources and the runtime directory. A fresh
`git checkout` rewrites mtimes and can call a good binary stale; that direction
is safe, because the answer is "rebuild" and rebuilding is cheap. The other
direction, trusting a binary older than the code, is the one that costs a
morning.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMPILER = ROOT / "glyph-compiler"

CANDIDATES = ("target/release/glyph", "target/debug/glyph")
# What actually goes into the release binary. Test sources are deliberately
# excluded: `cargo build --release` does not compile them, so editing one leaves
# the binary correct while making it look older than the tree. That false
# positive is worse than useless, because the whole point of this check is that
# a failure means something.
WATCHED = (
    ("crates", "*.rs"),
    ("runtime", "*"),
)
EXCLUDED_PARTS = ("tests", "benches", "examples")


def newest_input() -> tuple[float, pathlib.Path | None]:
    newest, where = 0.0, None
    for sub, pattern in WATCHED:
        base = COMPILER / sub
        if not base.exists():
            continue
        for p in base.rglob(pattern):
            if not p.is_file():
                continue
            if any(part in EXCLUDED_PARTS for part in p.relative_to(base).parts[:-1]):
                continue
            m = p.stat().st_mtime
            if m > newest:
                newest, where = m, p
    for name in ("Cargo.toml", "Cargo.lock"):
        p = COMPILER / name
        if p.exists() and p.stat().st_mtime > newest:
            newest, where = p.stat().st_mtime, p
    return newest, where


def find() -> pathlib.Path | None:
    env = os.environ.get("GLYPH_BIN")
    if env:
        p = pathlib.Path(env)
        return p if p.exists() else None
    for rel in CANDIDATES:
        p = COMPILER / rel
        if p.exists():
            return p
    found = shutil.which("glyph")
    return pathlib.Path(found) if found else None


def staleness(binary: pathlib.Path) -> pathlib.Path | None:
    """The newest input that postdates the binary, or None if it is current."""
    newest, where = newest_input()
    if where is None:
        return None
    return where if binary.stat().st_mtime < newest else None


def resolve() -> pathlib.Path:
    """The compiler to check with. Exits rather than hand back a stale one."""
    binary = find()
    if binary is None:
        print("no glyph binary found.")
        print("  cd glyph-compiler && cargo build --release")
        print("  (or set GLYPH_BIN to one you trust)")
        sys.exit(1)

    # A binary named explicitly, or one found on PATH, is the caller's business.
    if os.environ.get("GLYPH_BIN") or COMPILER not in binary.parents:
        return binary

    newer = staleness(binary)
    if newer is not None:
        rel = newer.relative_to(ROOT)
        print(f"{binary.relative_to(ROOT)} is older than {rel}.")
        print()
        print("Checking docs against a stale compiler reports failures that are not")
        print("real, and they read exactly like compiler gaps. Rebuild first:")
        print("  cd glyph-compiler && cargo build --release")
        sys.exit(1)

    return binary
