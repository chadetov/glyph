#!/usr/bin/env python3
"""The rule that outranks every other requirement, with something behind it.

Every edge is exact or absent, and absence of an edge means absence of a
relation, never "analysis did not reach here."

A rule stated in a document is a rule until somebody is in a hurry. This drives
the impact surface against five deliberately degenerate projects and asserts
that none of them produces a manufactured answer. The failure it exists to catch
is not a crash. It is a confident, well-formed reply that reads as a fact and is
not one: a site claimed as proven when the compiler could not key it, a partial
list shaped exactly like a complete one, an empty result standing in for a
question that does not apply.

The corpus was probed against the compiler before these expectations were
written, so what is asserted here is what the compiler actually does, not what
the design hoped it did. Two cases came back wrong on the first pass and are
recorded as KNOWN below rather than quietly dropped: an expectation removed to
make a suite green is the exact dishonesty this file exists to prevent.

A KNOWN case that starts passing is reported too. It means somebody fixed it,
and the entry should be promoted to a hard assertion in the same change.
"""
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CORPUS = ROOT / "tests" / "exact-or-absent"
BIN = ROOT / "glyph-compiler" / "target" / "release" / "glyph"


def ask(project: pathlib.Path, path: str, name: str) -> dict:
    """One glyph_variants call over stdio. Returns {'error': str} or the answer."""
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "exact-or-absent", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": "glyph_variants", "arguments": {"path": path, "name": name}}},
    ]
    proc = subprocess.run(
        [str(BIN), "mcp"],
        input="".join(json.dumps(r) + "\n" for r in reqs),
        capture_output=True, text=True, cwd=project, timeout=120,
    )
    last = [l for l in proc.stdout.splitlines() if l.strip()]
    if not last:
        return {"error": "the server answered nothing"}
    body = json.loads(last[-1]).get("result", {})
    text = body.get("content", [{}])[0].get("text", "")
    if body.get("isError"):
        return {"error": text}
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {"error": f"unparseable answer: {text[:200]}"}


def sites(a: dict, key: str) -> list:
    return [s.get("declaration") for s in a.get(key, [])]


def case_missing_identity() -> tuple[bool, str]:
    """A file whose module header disagrees with its path cannot be keyed.

    The site must be named as unkeyable, not silently promoted into `sites`
    (which would claim a proven edge) and not dropped (which would claim there
    is no relation).
    """
    a = ask(CORPUS / "missing-identity", "src/models.glyph", "Status")
    if "error" in a:
        return False, f"refused instead of naming the site: {a['error']}"
    if sites(a, "sites"):
        return False, f"claimed a proven edge for a site it cannot key: {sites(a, 'sites')}"
    if "models::label" not in sites(a, "unkeyed"):
        return False, f"dropped the site entirely; unkeyed={sites(a, 'unkeyed')}"
    return True, "named under unkeyed, absent from sites"


def case_ambiguous_identity() -> tuple[bool, str]:
    """Two modules declare `Status`. The answer must be about one of them.

    Merging both modules' sites under one name would be a manufactured edge:
    `b::lb` matches on b's Status, which is a different type that happens to
    share a spelling.
    """
    a = ask(CORPUS / "ambiguous-identity", "src/a.glyph", "Status")
    if "error" in a:
        return False, f"refused: {a['error']}"
    found = sites(a, "sites") + sites(a, "nested") + sites(a, "unkeyed")
    if "b::lb" in found:
        return False, "merged a same-named type from another module into the answer"
    if "a::la" not in found:
        return False, f"lost the site it should have found: {found}"
    return True, "resolved in the querying file's context; the same-named type stayed out"


def case_unsupported_entity_field() -> tuple[bool, str]:
    """A record field is not an entity this surface models. It must say so."""
    a = ask(CORPUS / "unsupported-entity", "src/m.glyph", "email")
    if "error" not in a:
        return False, f"answered a question it does not model instead of refusing: {a}"
    return True, "refused and named what the module does declare"


def case_unsupported_entity_record() -> tuple[bool, str]:
    """A record has no variants, so the question does not apply to it.

    KNOWN FAILING, G190. The answer is `{"sites": []}`, which is shaped exactly
    like a tagged union that genuinely has no match sites. An empty list is a
    claim: it says the relation was computed and found nothing. Here it was
    never applicable.
    """
    a = ask(CORPUS / "unsupported-entity", "src/m.glyph", "User")
    if "error" in a:
        return True, "refuses a non-union entity"
    kind = (a.get("type") or {}).get("kind")
    if a.get("sites") == [] and kind == "declaration":
        return False, 'returns {"sites": []}, indistinguishable from a union with no matches'
    return True, f"distinguishes the case (kind={kind})"


def case_malformed_module() -> tuple[bool, str]:
    """A file that does not parse yields no relation, and must not read as one."""
    a = ask(CORPUS / "malformed-module", "src/m.glyph", "Status")
    if "error" not in a:
        return False, f"answered from a file that does not parse: {a}"
    return True, "refused and pointed at the diagnostics"


def case_unresolved_import() -> tuple[bool, str]:
    """A scrutinee whose type never resolved must be reported as unresolved."""
    a = ask(CORPUS / "unresolved-import", "src/m.glyph", "Status")
    if "error" in a:
        return True, f"refused: {a['error'][:60]}"
    states = [s.get("state") for s in a.get("sites", [])]
    if "exhaustive" in states or "has_catch_all" in states:
        return False, f"claimed a decided state for an unresolvable scrutinee: {states}"
    if "scrutinee_unresolved" not in states:
        return False, f"dropped the site instead of naming it unresolved: {states}"
    return True, "named the site scrutinee_unresolved"


def case_unresolved_import_compiles() -> tuple[bool, str]:
    """The compiler must not call a program with an unresolvable import clean.

    KNOWN FAILING, G189. `glyph check --no-tsc` reports "no diagnostics" and
    exits 0 here, while the impact surface asked about the same file correctly
    answers `scrutinee_unresolved`. One unresolvable name, two surfaces, one
    saying it does not know and one saying everything is fine.
    """
    proc = subprocess.run(
        [str(BIN), "check", "src", "--no-tsc"],
        capture_output=True, text=True, cwd=CORPUS / "unresolved-import", timeout=180,
    )
    if proc.returncode != 0:
        return True, "reports the unresolvable import without needing tsc"
    return False, '"no diagnostics" and exit 0 on a program importing a module that does not exist'


HARD = [
    ("missing identity", case_missing_identity),
    ("ambiguous identity", case_ambiguous_identity),
    ("unsupported entity (field)", case_unsupported_entity_field),
    ("malformed module", case_malformed_module),
    ("unresolved import (impact surface)", case_unresolved_import),
]

KNOWN = [
    ("unsupported entity (record)", case_unsupported_entity_record, "G190"),
    ("unresolved import (compiler)", case_unresolved_import_compiles, "G189"),
]


def main() -> int:
    if not BIN.exists():
        print(f"missing {BIN.relative_to(ROOT)}; run cargo build --release first")
        return 1

    failures, promoted = [], []
    for label, fn in HARD:
        ok, why = fn()
        print(f"{'PASS' if ok else 'FAIL'}  {label:36} {why}")
        if not ok:
            failures.append((label, why))

    for label, fn, gap in KNOWN:
        ok, why = fn()
        if ok:
            print(f"FIXED {label:36} {why}  ({gap})")
            promoted.append((label, gap))
        else:
            print(f"KNOWN {label:36} {why}  ({gap})")

    print()
    if failures:
        print(f"{len(failures)} of {len(HARD)} invariant checks failed:")
        for label, why in failures:
            print(f"  {label}: {why}")
        print()
        print("An impact answer that manufactures a relation is worse than no impact")
        print("answer, because the caller edits believing it has seen the surface.")
        return 1

    if promoted:
        print("A known-failing case now passes. Promote it to a hard assertion and")
        print("close its entry, in the same change that fixed it:")
        for label, gap in promoted:
            print(f"  {label} ({gap})")
        return 1

    print(f"exact-or-absent OK: {len(HARD)} invariants hold, {len(KNOWN)} known gaps unchanged.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
