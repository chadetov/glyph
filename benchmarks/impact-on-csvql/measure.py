#!/usr/bin/env python3
"""Ask what breaks before the edit, then make the edit and check the answer.

The subject is `examples/apps/csvql`, an eleven-file CSV query engine in this
repository, and the change is one variant added to the `Value` union at its
centre. Nothing about it is staged: the app was written to be an app, the
union is where every stage of the engine meets, and adding a cell kind is a
change someone would plausibly make.

The run has two halves and they have to agree.

  Before the edit, `glyph_variants` is asked the change as a change
  (`proposed_variant`), not as a lookup. Every match site over `Value` comes
  back with a consequence: `WILL_FAIL` for a site that stops compiling once
  the variant exists, `ABSORBS` for a site whose catch-all keeps compiling
  and silently takes the new variant.

  Then the variant is added for real and `glyph check` runs. The set of
  declarations that report E0200 has to equal the set the tool called
  `WILL_FAIL`, and every site it called `ABSORBS` has to appear nowhere in
  the compiler's output at all.

That second equality is the whole instrument, and it needs no golden file: a
regression in the predictor or in the checker breaks it, because the two are
compared to each other. The fixture (`fixture/edit.json`) pins the numbers
separately, so the run also fails if csvql itself loses the sites rather than
if the two halves quietly agree on nothing.

Every figure printed as a total comes out of the answer's own `summary`,
never out of this script. Two callers tallying one reply their own way is how
a count and a list stop agreeing, so what is checked here is that the answer
is consistent with the list it shipped: `not_counted` empty and `unindexed`
empty are the answer saying its figures cover the project, and if either says
otherwise the run stops rather than reporting a subset as a whole.

Usage:
    ./measure.py            # writes results/<timestamp>.json, exits non-zero on divergence

Needs a `glyph` on PATH, or GLYPH=<path to binary>, whose `glyph_variants`
answer carries `summary` and a per-site `consequence`. Both arrived on the
0.1.110 line; 0.1.106 has neither, accepts `proposed_variant` and silently
answers the lookup form, and the run stops on that rather than reading it as
a prediction. No node toolchain: `check` runs with --no-tsc --no-test, and
the diagnostic the edit produces (E0200, exhaustiveness) comes out of the
Glyph stages before either.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
SPEC = json.loads((HERE / "fixture" / "edit.json").read_text())
APP = REPO / SPEC["app"]
GLYPH = os.environ.get("GLYPH", "glyph")

# What a plain text search for a catch-all arm looks like, and the enclosing
# named declaration to attribute a hit to. `fn(acc: number, ...)` (an
# anonymous closure) deliberately does not match: the hit inside one is
# attributed to the named function it sits in, which is what the tool reports.
FN_LINE = re.compile(r"^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[(<]")
CATCH_ALL_ARM = re.compile(r"(^|\s)(else|_)\s*=>")


def stage_app() -> Path:
    """Copy the app somewhere disposable. The app itself is never edited."""
    tmp = Path(tempfile.mkdtemp(prefix="glyph-impact-on-csvql-"))
    dest = tmp / APP.name
    shutil.copytree(
        APP,
        dest,
        # A local build may have left emitted TypeScript beside the sources.
        # Applications under examples/apps/ contain no TypeScript of their own
        # (scripts/check_apps_are_glyph.py enforces it), so dropping it keeps
        # the staged copy identical to what is committed.
        ignore=shutil.ignore_patterns("node_modules", "dist", ".glyph", "*.ts", "*.js"),
    )
    return dest


def run_mcp_tool(project_dir: Path, name: str, arguments: dict) -> tuple[dict, float]:
    """Call one MCP tool against `glyph mcp <project_dir>` over stdio. The
    transport is one JSON-RPC object per line (glyph-lsp/src/mcp.rs
    `run_stdio`). Returns the decoded result and the wall time of the whole
    exchange, process start and project walk included.
    """
    started = time.monotonic()
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
    out, err = proc.communicate(payload, timeout=180)
    elapsed = time.monotonic() - started
    responses = [json.loads(line) for line in out.splitlines() if line.strip()]
    try:
        call = next(r for r in responses if r.get("id") == 2)
    except StopIteration:
        raise SystemExit(f"{GLYPH} mcp returned no answer for id 2.\nstdout: {out}\nstderr: {err}")
    if "error" in call:
        raise SystemExit(f"{name} failed: {call['error']}")
    result = call["result"]
    if result.get("isError"):
        raise SystemExit(f"{name} refused: {result['content'][0]['text']}")
    return json.loads(result["content"][0]["text"]), elapsed


def glyph_check_json(project_dir: Path) -> tuple[dict, float]:
    started = time.monotonic()
    proc = subprocess.run(
        [GLYPH, "check", ".", "--no-tsc", "--no-test", "--json"],
        cwd=project_dir,
        capture_output=True,
        text=True,
    )
    elapsed = time.monotonic() - started
    if not proc.stdout.strip():
        raise SystemExit(f"glyph check wrote nothing to stdout.\nstderr: {proc.stderr}")
    return json.loads(proc.stdout), elapsed


def search_alone_catch_all_sites(app_dir: Path) -> list[dict]:
    """Grep for a catch-all arm across the app, the way someone would look for
    the sites an added variant slips past silently. Every hit is real; a text
    search just has no notion of which union the enclosing match scrutinizes.
    """
    hits = []
    for path in sorted(app_dir.glob("*.glyph")):
        current = None
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            named = FN_LINE.match(line)
            if named:
                current = named.group(1)
            if CATCH_ALL_ARM.search(line):
                hits.append(
                    {
                        "declaration": f"{path.stem}::{current}" if current else None,
                        "location": f"{path.name}:{lineno}",
                    }
                )
    return hits


def apply_edit(app_dir: Path) -> None:
    edit = SPEC["edit"]
    target = app_dir / edit["file"]
    source = target.read_text()
    anchor = edit["after_line"] + "\n"
    found = source.count(anchor)
    if found != 1:
        raise SystemExit(
            f"the fixture anchors on {anchor!r} in {edit['file']}, which occurs {found} times. "
            "csvql changed under the fixture; re-derive fixture/edit.json."
        )
    target.write_text(source.replace(anchor, anchor + edit["insert"] + "\n"))


def main() -> int:
    failures: list[str] = []
    union = SPEC["union"]
    variant = SPEC["proposed_variant"]
    expected = SPEC["expected"]

    app = stage_app()
    try:
        # --- 0. the app compiles before the edit -------------------------------
        # Without this the failures after the edit are not attributable to it.
        before_report, before_seconds = glyph_check_json(app)
        before_diags = before_report.get("diagnostics", [])
        if before_diags:
            failures.append(
                f"the unedited app is not clean: {len(before_diags)} diagnostics "
                f"({sorted({d.get('code') for d in before_diags})}). "
                "Every count below would be measuring something else."
            )

        # --- 1. ask the change as a change, before making it -------------------
        answer, predict_seconds = run_mcp_tool(
            app,
            "glyph_variants",
            {"path": union["path"], "name": union["name"], "proposed_variant": variant},
        )

        # The direct sites, which is what the answer's own totals count. A
        # site that reaches the union through a payload (`Ok(Text(t))` on a
        # `Result<Value, _>`) breaks exactly the way a direct one does, and the
        # answer files it under `nested` and names it in `not_counted`. csvql
        # has none today, and the `not_counted` check below refuses the run if
        # that changes rather than picking a definition of "site" here.
        sites = list(answer.get("sites") or [])

        # The totals come out of the answer, never out of this script. Glyph
        # states its own arithmetic (`summary`), so a benchmark that re-tallied
        # the list would be the second caller counting one reply its own way,
        # and two figures from one answer is the failure the summary exists to
        # end. What is checked here is that the answer is consistent with the
        # list it came with, not what the figures should have been.
        summary = answer.get("summary")
        if summary is None or any("consequence" not in s for s in sites):
            raise SystemExit(
                f"{GLYPH} answered glyph_variants without "
                + ("a `summary`" if summary is None else "a `consequence` on every site")
                + ". `proposed_variant` and the per-site consequences arrived in 0.1.110 and "
                "the `summary` block alongside them; an older binary accepts the argument, "
                "ignores it and returns the lookup form, which this benchmark cannot read as "
                "a prediction. 0.1.106 has neither."
            )

        by_consequence: dict[str, list[dict]] = {}
        for site in sites:
            by_consequence.setdefault(site.get("consequence"), []).append(site)

        will_fail = sorted(s["declaration"] for s in by_consequence.get("WILL_FAIL", []))
        absorbs = sorted(s["declaration"] for s in by_consequence.get("ABSORBS", []))
        counts = summary.get("consequences") or {}
        # Stated, not assumed. `not_counted` empty is the answer saying its
        # totals cover everything, and `unindexed` empty is it saying every
        # file in the project was read. Neither is inferred from silence.
        not_counted = summary.get("not_counted")
        unindexed = answer.get("unindexed")

        prediction = {
            "call": {
                "tool": "glyph_variants",
                "arguments": {
                    "path": union["path"],
                    "name": union["name"],
                    "proposed_variant": variant,
                },
            },
            "summary": summary,
            "unindexed": unindexed,
            "will_fail": will_fail,
            "absorbs": absorbs,
            "union_variants_before_the_edit": (answer.get("type") or {}).get("variants"),
            "seconds": round(predict_seconds, 3),
            "listing": [
                {
                    "consequence": s.get("consequence"),
                    "state": s.get("state"),
                    "location": f"{s.get('path')}:{s.get('line')}",
                    "declaration": s.get("declaration"),
                }
                for s in sorted(sites, key=lambda s: (s.get("path") or "", s.get("line") or 0))
            ],
        }

        if not_counted is None or unindexed is None:
            failures.append(
                "the answer states no `not_counted` or no `unindexed`. A total that does not "
                "say what it left out is a partial list with a figure in front of it."
            )
        if not_counted:
            failures.append(
                f"the answer counted everything except {not_counted}. The figures below cover "
                "less than the project, so the agreement check downstream is over a subset."
            )
        if unindexed:
            failures.append(
                f"{len(unindexed)} project file(s) were never read: {unindexed}. They may hold "
                "match sites over this union that no part of this run can see."
            )
        # With nothing left out, the answer's own totals have to match the list
        # it shipped them with. This is a consistency check on one answer, not a
        # second tally: it is what `not_counted: []` claims.
        if not not_counted and summary.get("sites") != len(sites):
            failures.append(
                f"the answer says {summary.get('sites')} sites, left nothing out, and shipped "
                f"{len(sites)} of them."
            )
        if not not_counted and counts.get("WILL_FAIL") != len(will_fail):
            failures.append(
                f"the answer says {counts.get('WILL_FAIL')} will fail and marks {len(will_fail)}."
            )
        if not not_counted and counts.get("ABSORBS") != len(absorbs):
            failures.append(
                f"the answer says {counts.get('ABSORBS')} absorb and marks {len(absorbs)}."
            )
        if summary.get("sites") != expected["sites"] or summary.get("files") != expected["files"]:
            failures.append(
                f"expected {expected['sites']} sites across {expected['files']} files, "
                f"got {summary.get('sites')} across {summary.get('files')}. csvql changed under "
                "the fixture; re-derive fixture/edit.json against the release you are on."
            )
        if will_fail != sorted(expected["will_fail"]):
            failures.append(f"expected WILL_FAIL {sorted(expected['will_fail'])}, got {will_fail}")
        if absorbs != sorted(expected["absorbs"]):
            failures.append(f"expected ABSORBS {sorted(expected['absorbs'])}, got {absorbs}")

        # --- 2. what a text search gets asked the same question ----------------
        search_hits = search_alone_catch_all_sites(app)
        search_true = [h for h in search_hits if h["declaration"] in set(absorbs)]
        baseline = {
            "method": "grep for `else =>` / `_ =>` across the app, hit attributed to its enclosing named fn",
            "hits": len(search_hits),
            "hits_in_a_value_match_site": [h["location"] for h in search_true],
            "precision": (len(search_true) / len(search_hits)) if search_hits else 0.0,
            "listing": search_hits,
        }
        if len(search_true) != len(absorbs):
            failures.append(
                f"the search baseline found {len(search_true)} of the {len(absorbs)} absorbing "
                "sites, so its recall is no longer 1.0 and the precision figure is comparing "
                "two different things. Re-read the baseline before quoting it."
            )
        if len(search_hits) <= len(search_true):
            failures.append(
                "the search baseline has no false positives any more, so csvql no longer has "
                "catch-alls outside Value and the comparison has stopped meaning anything."
            )

        # --- 3. make the edit for real, and check ------------------------------
        apply_edit(app)
        after_report, after_seconds = glyph_check_json(app)
        after_diags = after_report.get("diagnostics", [])
        e0200 = [d for d in after_diags if d.get("code") == "E0200"]
        other = [d for d in after_diags if d.get("code") != "E0200"]
        broke = sorted(d.get("entity") for d in e0200)
        mentioned = sorted(
            d
            for d in set(absorbs)
            if any(d.split("::")[-1] in (x.get("message") or "") for x in after_diags)
            or d in broke
        )

        outcome = {
            "edit": f"{SPEC['edit']['file']}: {SPEC['edit']['insert'].strip()}",
            "diagnostics": len(after_diags),
            "e0200": len(e0200),
            "e0200_entities": broke,
            "other_diagnostics": [
                {"code": d.get("code"), "entity": d.get("entity")} for d in other
            ],
            "absorbing_sites_mentioned_anywhere": mentioned,
            "seconds": round(after_seconds, 3),
        }

        # The instrument. No golden file on either side of this comparison:
        # the predictor and the checker are held against each other, so a
        # regression in either one breaks it.
        if broke != will_fail:
            failures.append(
                "the prediction and the compiler disagree.\n"
                f"    predicted WILL_FAIL: {will_fail}\n"
                f"    reported E0200:      {broke}\n"
                f"    predicted and silent: {sorted(set(will_fail) - set(broke))}\n"
                f"    reported unpredicted: {sorted(set(broke) - set(will_fail))}"
            )
        if mentioned:
            failures.append(
                f"the compiler now mentions {mentioned}, which the tool said would absorb the "
                "variant silently. Either the prediction is wrong or the compiler started "
                "catching this, and both change what this benchmark reports."
            )
        if other:
            failures.append(
                f"the edit produced {len(other)} diagnostic(s) that are not E0200: "
                f"{[d.get('code') for d in other]}. The count is no longer only about exhaustiveness."
            )
    finally:
        shutil.rmtree(app.parent, ignore_errors=True)

    ok = not failures
    findings = {
        "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ"),
        "glyph_version": subprocess.run(
            [GLYPH, "--version"], capture_output=True, text=True
        ).stdout.strip(),
        "app": SPEC["app"],
        "union": f"{union['path']}::{union['name']}",
        "proposed_variant": variant,
        "clean_before_the_edit": {
            "diagnostics": len(before_diags),
            "seconds": round(before_seconds, 3),
        },
        "prediction": prediction,
        "search_alone": baseline,
        "after_the_edit": outcome,
        "failures": failures,
        "ok": ok,
    }

    results_dir = HERE / "results"
    results_dir.mkdir(exist_ok=True)
    out_path = results_dir / f"{findings['timestamp']}.json"
    out_path.write_text(json.dumps(findings, indent=2, sort_keys=False) + "\n")

    print(f"{findings['glyph_version']} on {SPEC['app']}, {len(list(APP.glob('*.glyph')))} files")
    print(f"adding one variant to {union['name']}, "
          f"which has {len(prediction['union_variants_before_the_edit'] or [])} today: "
          f"{', '.join(prediction['union_variants_before_the_edit'] or [])}")
    print()
    print(f"before the edit, glyph_variants predicts ({prediction['seconds']}s), in its own words:")
    for line in summary.get("lines") or []:
        print(f"  {line}")
    if not not_counted:
        print("  and it states what it left out of those figures: nothing")
    print()
    for row in prediction["listing"]:
        tag = "FAILS" if row["consequence"] == "WILL_FAIL" else (
            "ABSORBS" if row["consequence"] == "ABSORBS" else row["consequence"]
        )
        print(f"     {tag:8} {row['location']:18} {row['declaration']}")
    print()
    print(f"after adding `{SPEC['edit']['insert'].strip().lstrip('| ')}` for real ({outcome['seconds']}s):")
    print(f"  the compiler reports {outcome['e0200']} E0200 failures"
          + (", and they are the ones predicted." if broke == will_fail else ", and they are NOT the ones predicted."))
    print(f"  the {len(absorbs)} catch-all sites report "
          + ("nothing." if not mentioned else f"something: {mentioned}"))
    print()
    print(f"a text search asked the same question: {baseline['hits']} catch-all arms across the app, "
          f"{len(search_true)} of them in a {union['name']} match site (precision "
          f"{baseline['precision']:.2f}), and nothing at all about the {len(will_fail)} that will fail.")
    print()
    if ok:
        print("PASS: the prediction and the compiler agree, site for site.")
    else:
        print("FAIL:")
        for f in failures:
            print(f"  - {f}")
    print(f"\nwrote {out_path}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
