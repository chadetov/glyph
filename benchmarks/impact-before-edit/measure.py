#!/usr/bin/env python3
"""Measure the has_catch_all bug shape against search alone (0.1.107).

The fixture (fixture/main.glyph, fixture/main_after.glyph) is the reproduction
this release's item was scoped against: `UserStatus` has two match sites,
`describe_exhaustive` (every variant named) and `describe_catchall` (an
`else` arm). `main_after.glyph` adds a third variant, `Suspended`. When that
lands for real, `describe_exhaustive` fails to compile and `describe_catchall`
stays green and silently routes `Suspended` into `"other"`.

Two ways to find, *before* making that edit, which match sites over
`UserStatus` are catch-all (the ones an added variant slips past silently):

  1. search alone   - grep the file for a catch-all arm (`else =>` / `_ =>`).
                       Cheap, and blind to which union a catch-all belongs to.
                       `fixture/main.glyph` also matches on `Direction` with a
                       catch-all of its own, so a plain grep for the arm shape
                       returns it too - a false positive for anyone asking
                       specifically about `UserStatus`.
  2. glyph_variants - the MCP tool 0.1.106 shipped, called with
                       `{"path": ..., "name": "UserStatus"}`. It is already
                       scoped to the union in question, so the unrelated
                       `Direction` site never enters the answer.

This script runs both against the *unedited* fixture (search alone cannot see
the future variant either way; the question both methods answer is "which
sites are catch-all now"), then confirms the compiler side of the bug shape
against `main_after.glyph`: the exhaustive site's diagnostic carries `entity`
(0.1.107's other half) and the catch-all site produces no diagnostic at all.

Usage:
    ./measure.py            # run the comparison, write results/<timestamp>.json
Requires `glyph` on PATH (or set GLYPH to the binary).
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURE_BEFORE = HERE / "fixture" / "main.glyph"
FIXTURE_AFTER = HERE / "fixture" / "main_after.glyph"
GLYPH_JSON = HERE / "fixture" / "glyph.json"
GLYPH = os.environ.get("GLYPH", "glyph")


def search_alone_catch_all_sites(source: str) -> list[str]:
    """What a plain text search for a catch-all arm finds: the name of the
    enclosing `fn`, for every `else =>` / `_ =>` line in the file. Blind to
    which type the enclosing `match` scrutinizes - that is exactly the
    limitation this measures.
    """
    lines = source.splitlines()
    sites = []
    current_fn = None
    for line in lines:
        m = re.match(r"\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(", line)
        if m:
            current_fn = m.group(1)
        if re.search(r"(^|\s)(else|_)\s*=>", line):
            sites.append(current_fn)
    return sites


def run_mcp_tool(project_dir: Path, name: str, arguments: dict) -> dict:
    """Call one MCP tool against `glyph mcp <project_dir>` over stdio and
    return its decoded JSON result. The transport is one JSON-RPC object per
    line (see glyph-lsp/src/mcp.rs `run_stdio`).
    """
    proc = subprocess.Popen(
        [GLYPH, "mcp", str(project_dir)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        },
    ]
    payload = "".join(json.dumps(r) + "\n" for r in requests)
    out, err = proc.communicate(payload, timeout=30)
    responses = [json.loads(line) for line in out.splitlines() if line.strip()]
    call_resp = next(r for r in responses if r.get("id") == 2)
    text = call_resp["result"]["content"][0]["text"]
    return json.loads(text)


def glyph_check_json(project_dir: Path, glyph_file: str) -> dict:
    result = subprocess.run(
        [GLYPH, "check", glyph_file, "--no-tsc", "--json"],
        cwd=project_dir,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def stage_project(source: Path) -> Path:
    tmp = Path(tempfile.mkdtemp(prefix="glyph-impact-before-edit-"))
    shutil.copy(GLYPH_JSON, tmp / "glyph.json")
    shutil.copy(source, tmp / "main.glyph")
    return tmp


def main() -> int:
    ok = True
    findings: dict = {}

    # --- 1. search alone vs glyph_variants, both against the unedited fixture ---
    before_source = FIXTURE_BEFORE.read_text()
    search_sites = search_alone_catch_all_sites(before_source)
    findings["search_alone"] = {
        "method": "grep for `else =>` / `_ =>`, whole file",
        "catch_all_sites_found": search_sites,
        "note": "cannot attribute a hit to the union it matches on",
    }

    before_dir = stage_project(FIXTURE_BEFORE)
    try:
        variants = run_mcp_tool(before_dir, "glyph_variants", {"path": "main.glyph", "name": "UserStatus"})
    finally:
        shutil.rmtree(before_dir, ignore_errors=True)

    tool_catch_all = [
        s["declaration"] for s in variants.get("sites", []) if s.get("state") == "has_catch_all"
    ]
    tool_exhaustive = [
        s["declaration"] for s in variants.get("sites", []) if s.get("state") == "exhaustive"
    ]
    findings["glyph_variants"] = {
        "method": 'tools/call glyph_variants {"path": "main.glyph", "name": "UserStatus"}',
        "catch_all_sites_found": tool_catch_all,
        "exhaustive_sites_found": tool_exhaustive,
    }

    # Ground truth, by construction of the fixture: exactly one UserStatus
    # catch-all site (describe_catchall); describe_direction matches on
    # Direction, not UserStatus, and must not appear.
    search_alone_false_positives = [s for s in search_sites if s != "describe_catchall"]
    search_alone_hit = "describe_catchall" in search_sites
    tool_false_positives = [d for d in tool_catch_all if not d.endswith("::describe_catchall")]
    tool_hit = any(d.endswith("::describe_catchall") for d in tool_catch_all)

    findings["comparison"] = {
        "search_alone": {
            "found_the_real_site": search_alone_hit,
            "false_positives": search_alone_false_positives,
            "precision": (1.0 / len(search_sites)) if search_sites else 0.0,
        },
        "glyph_variants": {
            "found_the_real_site": tool_hit,
            "false_positives": tool_false_positives,
            "precision": (
                sum(1 for d in tool_catch_all if d.endswith("::describe_catchall")) / len(tool_catch_all)
                if tool_catch_all
                else 0.0
            ),
        },
    }

    if not search_alone_hit or not search_alone_false_positives:
        print("FAIL: the search-alone baseline stopped matching its own premise", file=sys.stderr)
        ok = False
    if not tool_hit or tool_false_positives:
        print("FAIL: glyph_variants did not cleanly isolate the UserStatus catch-all site", file=sys.stderr)
        ok = False

    # --- 2. the compiler side of the same bug shape, after the edit lands ---
    after_dir = stage_project(FIXTURE_AFTER)
    try:
        after_report = glyph_check_json(after_dir, "main.glyph")
    finally:
        shutil.rmtree(after_dir, ignore_errors=True)

    e0200 = [d for d in after_report.get("diagnostics", []) if d.get("code") == "E0200"]
    catchall_mentioned = any("describe_catchall" in d.get("message", "") for d in after_report.get("diagnostics", []))
    findings["after_edit_diagnostics"] = {
        "e0200_count": len(e0200),
        "e0200_entity": e0200[0].get("entity") if e0200 else None,
        "describe_catchall_mentioned_anywhere": catchall_mentioned,
    }

    if len(e0200) != 1 or e0200[0].get("entity") != "main::describe_exhaustive":
        print("FAIL: the exhaustive-site diagnostic no longer carries the expected entity", file=sys.stderr)
        ok = False
    if catchall_mentioned:
        print("FAIL: describe_catchall now appears in diagnostics; the silent half of the bug shape no longer reproduces", file=sys.stderr)
        ok = False

    findings["ok"] = ok
    findings["timestamp"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")

    results_dir = HERE / "results"
    results_dir.mkdir(exist_ok=True)
    out_path = results_dir / f"{findings['timestamp']}.json"
    out_path.write_text(json.dumps(findings, indent=2) + "\n")

    print(json.dumps(findings, indent=2))
    print(f"\nwrote {out_path}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
