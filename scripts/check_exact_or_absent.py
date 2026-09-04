#!/usr/bin/env python3
"""The rule that outranks every other requirement, with something behind it.

Every edge is exact or absent, and absence of an edge means absence of a
relation, never "analysis did not reach here."

A rule stated in a document is a rule until somebody is in a hurry. This drives
the impact surface against a corpus of deliberately degenerate projects and
asserts that none of them produces a manufactured answer. The failure it exists
to catch is not a crash. It is a confident, well-formed reply that reads as a fact and is
not one: a site claimed as proven when the compiler could not key it, a partial
list shaped exactly like a complete one, an empty result standing in for a
question that does not apply.

The corpus was probed against the compiler before these expectations were
written, so what is asserted here is what the compiler actually does, not what
the design hoped it did. Two cases came back wrong on the first pass and were
recorded as KNOWN rather than quietly dropped: an expectation removed to make a
suite green is the exact dishonesty this file exists to prevent. Both have since
been fixed and promoted, G189 and then G190, and KNOWN is empty.

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


def ask(project: pathlib.Path, path: str, name: str, proposed: str = "") -> dict:
    """One glyph_variants call. Returns {'error': str} or the answer.

    With `proposed` the call is the change form: the answer states a
    consequence per site instead of a state.
    """
    arguments = {"path": path, "name": name}
    if proposed:
        arguments["proposed_variant"] = proposed
    return call(project, "glyph_variants", arguments)


def call(project: pathlib.Path, tool: str, arguments: dict):
    """One MCP tool call over stdio. Returns the parsed answer, or
    {'error': str} for a refusal."""
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "exact-or-absent", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": tool, "arguments": arguments}},
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


def edges_of(answer: dict, relation: str = "") -> list:
    """Every edge of `answer`, or only the ones of `relation`."""
    relations = answer.get("relations") or {}
    if relation:
        return (relations.get(relation) or {}).get("edges") or []
    out = []
    for entry in relations.values():
        out.extend(entry.get("edges") or [])
    return out


def froms(answer: dict, relation: str) -> list:
    """The declaration each edge of `relation` sits in, in answer order."""
    return [e.get("from") for e in edges_of(answer, relation)]


def unindexed_paths(answer: dict, relation: str):
    """The files `relation` could not read, or None when it states no coverage."""
    entry = (answer.get("relations") or {}).get(relation) or {}
    if "unindexed" not in entry:
        return None
    return [u.get("path") for u in entry["unindexed"]]


def references(project: pathlib.Path, path: str, name: str, relation: str = "") -> dict:
    arguments = {"path": path, "name": name}
    if relation:
        arguments["relation"] = relation
    return call(project, "glyph_references", arguments)


def reference_sites(project: pathlib.Path, path: str, name: str):
    """glyph_references, as {(file, the source text the site covers)}.

    Every relation's edges, flattened. The covered text is read out of the
    corpus rather than trusted from the answer, because the span is what a
    workspace rename writes over.
    """
    a = references(project, path, name)
    if "error" in a:
        return a, None
    out = set()
    for edge in edges_of(a):
        text = (project / edge["path"]).read_text()
        starts, at = [], 0
        for line in text.splitlines(keepends=True):
            starts.append(at)
            at += len(line)

        def offset(end, edge=edge, starts=starts):
            p = edge["range"][end]
            return starts[p["line"]] + p["character"]

        out.add((edge["path"], text[offset("start"):offset("end")]))
    return None, out


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

    Was G190. The answer used to be `{"sites": []}`, shaped exactly like a
    tagged union that genuinely has no match sites. An empty list is a claim:
    it says the relation was computed and found nothing, and here it was never
    applicable. The tool refuses now and names what the declaration is.

    Both halves are asserted, because a refusal that fired on everything would
    pass the first half and destroy the surface. `Status` in the same file is a
    union nothing matches on, and it still answers, still with `[]`.
    """
    a = ask(CORPUS / "unsupported-entity", "src/m.glyph", "User")
    if "error" not in a:
        return False, f"answered a record with a site list instead of refusing: {a}"
    if "record" not in a["error"]:
        return False, f"refused without saying what the declaration is: {a['error'][:120]}"
    b = ask(CORPUS / "unsupported-entity", "src/m.glyph", "Status")
    if "error" in b:
        return False, f"refused a union that merely has no match sites: {b['error'][:120]}"
    if b.get("sites") != []:
        return False, f"the corpus union is meant to have no sites: {sites(b, 'sites')}"
    return True, "refuses the record; a union with no sites still answers []"


def case_variants_are_named_or_explicitly_unread() -> tuple[bool, str]:
    """An answer says what the union's variants are, or why it cannot read them.

    Leaving the field out spells "this union has no variants" and "this answer
    did not reach them" the same way, which is the ambiguity the empty site
    list carried. A caller adding a variant needs the current list to see what
    it is changing.
    """
    a = ask(CORPUS / "ambiguous-identity", "src/a.glyph", "Status")
    if "error" in a:
        return False, f"refused: {a['error'][:120]}"
    ty = a.get("type") or {}
    if ty.get("variants") == ["Open", "Closed"]:
        return True, "names the union's own variants, in declaration order"
    if ty.get("variants") is None and ty.get("variants_unavailable"):
        return False, (
            "could not read the variants of a declaration this project holds: "
            f"{ty['variants_unavailable'][:100]}"
        )
    return False, f"no variant list and no reason for its absence: {ty}"


def case_missing_identity_change() -> tuple[bool, str]:
    """The unkeyable site again, asked as a change rather than as a lookup.

    A decided consequence here is the manufactured answer in its most
    expensive form: the caller edits believing the compiler will stop at the
    site, and the compiler never keyed it to this type at all.
    """
    a = ask(CORPUS / "missing-identity", "src/models.glyph", "Status", proposed="Pending")
    if "error" in a:
        return False, f"refused instead of naming the site: {a['error'][:120]}"
    decided = [
        s.get("declaration")
        for s in a.get("sites", []) + a.get("nested", [])
        if s.get("consequence") in ("WILL_FAIL", "ABSORBS")
    ]
    if decided:
        return False, f"stated a decided consequence for a site it cannot key: {decided}"
    unkeyed = a.get("unkeyed", [])
    if not unkeyed:
        return False, "dropped the unkeyable site from the change answer"
    wrong = [s.get("consequence") for s in unkeyed if s.get("consequence") != "NOT_INDEXED"]
    if wrong:
        return False, f"an unkeyed site carries a consequence other than NOT_INDEXED: {wrong}"
    return True, "the unkeyable site is NOT_INDEXED, never a decided consequence"


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

    Was G189. `glyph check --no-tsc` used to report "no diagnostics" and exit 0
    here, while the impact surface asked about the same file correctly answered
    `scrutinee_unresolved`. One unresolvable name, two surfaces, one saying it
    did not know and one saying everything was fine.

    The project's `package.json` declares no dependency named `nowhere`, so no
    `npm install` could make that import resolve, and the resolver reports E0104
    without needing the TypeScript pass. The distinction this rests on is the
    one that keeps it safe: a declared dependency that is not installed yet is
    still an npm package, and stays unreported.
    """
    proc = subprocess.run(
        [str(BIN), "check", "src", "--no-tsc"],
        capture_output=True, text=True, cwd=CORPUS / "unresolved-import", timeout=180,
    )
    out = proc.stdout + proc.stderr
    if proc.returncode == 0:
        return False, '"no diagnostics" and exit 0 on a program importing a module that does not exist'
    if "nowhere" not in out:
        return False, f"failed without naming the import that cannot resolve: {out[:200]}"
    # `src/pkg.glyph` imports `tinylog`, which the manifest declares and which is
    # not installed. Reporting it would be the false positive this whole
    # distinction exists to avoid, and it is worse than the gap was.
    if "tinylog" in out:
        return False, f"reported a declared dependency that is merely not installed: {out[:200]}"
    return True, "names `nowhere`, leaves the declared-but-uninstalled `tinylog` alone"


def case_import_spelling() -> tuple[bool, str]:
    """Two projects, one symbol, and the consumer's import spelled two ways.

    `render` declares `label`; `policy` calls it. One project's consumer writes
    `import render { label }` and the other writes `import render` and
    `render.label(s)`. The reference set has to be the same either way.

    Was G186. The namespace spelling used to answer with the declaration and
    nothing else, and the calling module was absent from it entirely. That is
    the partial list shaped exactly like a complete one: not "no references",
    which would be wrong but visible, but a one-entry answer that reads like a
    symbol nobody calls.

    The covered text is asserted as well as the file. This relation is what
    workspace rename writes its edits from, so a site reported as the whole
    `render.label` would rewrite the namespace along with the name.

    What this does not assert: which relation the site lands under. The
    qualified call is reported, but as REFERENCES rather than CALLS, because
    `callee_name_spans` records a callee only when it is a bare identifier. A
    green result here is not coverage of that half.
    """
    root = CORPUS / "import-spelling"
    answers = {}
    for spelling in ("named", "qualified"):
        err, found = reference_sites(root / spelling, "src/render.glyph", "label")
        if err:
            return False, f"{spelling}: refused: {err['error'][:120]}"
        answers[spelling] = found

    for spelling, found in answers.items():
        if ("src/policy.glyph", "label") not in found:
            return False, f"{spelling} lost the call in the consumer: {sorted(found)}"
        wide = [t for _, t in found if t != "label"]
        if wide:
            return False, f"{spelling} reported a span wider than the name: {wide}"

    if answers["named"] != answers["qualified"]:
        return False, (
            "the answer depends on how the consumer spelled its import: "
            f"named={sorted(answers['named'])} qualified={sorted(answers['qualified'])}"
        )
    return True, "both spellings name the declaration and the call, at the name"


def case_calls_relation() -> tuple[bool, str]:
    """CALLS holds the sites that apply the symbol, and says what it missed.

    `app::bill` writes `charge(1)`. `app::handler` writes `apply(charge)`,
    which passes the function rather than applying it, and only the first stops
    compiling when `charge` gains a parameter. Collapsed into one list the two
    are the same fact, which is how a semantic graph becomes a dependency
    graph.

    The coverage half is asserted per relation rather than once per answer.
    `src/unreadable.glyph` does not parse, so occurrences in it were never
    read, and a CALLS list that quietly dropped it would be a partial list
    shaped exactly like a complete one.
    """
    a = references(CORPUS / "relations", "src/lib.glyph", "charge", "CALLS")
    if "error" in a:
        return False, f"refused: {a['error'][:120]}"
    calls = froms(a, "CALLS")
    if calls != ["app::bill"]:
        return False, f"CALLS is not the applied sites alone: {calls}"
    missed = unindexed_paths(a, "CALLS")
    if missed is None:
        return False, "CALLS states no coverage, so its list cannot be read as complete"
    if missed != ["src/unreadable.glyph"]:
        return False, f"the file the sweep could not read is not named: {missed}"
    claimed = {e.get("provenance") for e in edges_of(a, "CALLS")}
    if claimed != {"PROVED"}:
        return False, f"an edge into a module this project holds is not PROVED: {claimed}"
    return True, "the applied site alone, with the unreadable file named"


def case_references_relation() -> tuple[bool, str]:
    """REFERENCES holds every occurrence that is not an application.

    The declaration's own name, the import binding, and `apply(charge)`. The
    call in `app::bill` belongs to the other relation and must not appear
    twice: the two lists partition the occurrences, so a caller reading one of
    them is reading a fact rather than a subset of a vaguer one.

    Coverage is asserted again rather than trusted from the CALLS case. Two
    relations resolved to different depths and returned under one coverage
    statement is exactly the failure the rule exists to prevent.
    """
    a = references(CORPUS / "relations", "src/lib.glyph", "charge", "REFERENCES")
    if "error" in a:
        return False, f"refused: {a['error'][:120]}"
    named = froms(a, "REFERENCES")
    if "app::handler" not in named:
        return False, f"the symbol passed as an argument is missing: {named}"
    if "app::bill" in named:
        return False, f"the applied site is in both relations: {named}"
    if "lib::charge" not in named:
        return False, f"the declaration itself is missing: {named}"
    missed = unindexed_paths(a, "REFERENCES")
    if missed != ["src/unreadable.glyph"]:
        return False, f"REFERENCES states its coverage differently or not at all: {missed}"
    if edges_of(a, "CALLS"):
        return False, "a relation nobody asked for came back with edges"
    return True, "the non-applied sites alone, with its own coverage statement"


def case_provenance_is_proved_or_asserted() -> tuple[bool, str]:
    """An edge the compiler proved and an edge a `.d.ts` claimed are two facts.

    `lib::charge` is a Glyph declaration this project holds, so the resolver
    checked the far end against something it parsed. `tinylog::log` exists
    because `.types/ambient.d.ts` says it does, and `tsc` is what checks it.
    `nowhere::audit` is neither, and reporting it as asserted would name a
    declaration file that does not exist.

    The third value is what keeps the other two honest. Two values would force
    every unresolvable import into one of them, and `ASSERTED` is the one it
    would land on.
    """
    root = CORPUS / "provenance"
    got = {}
    for name in ("charge", "log", "audit"):
        a = references(root, "src/app.glyph", name)
        if "error" in a:
            return False, f"`{name}` refused: {a['error'][:120]}"
        got[name] = a

    want = {"charge": "PROVED", "log": "ASSERTED", "audit": "UNDETERMINED"}
    for name, expected in want.items():
        actual = got[name].get("provenance")
        if actual != expected:
            return False, f"`{name}` is {actual}, not {expected}"

    detail = got["log"].get("provenance_detail") or ""
    if "ambient.d.ts" not in detail:
        return False, f"an asserted edge does not name what asserts it: {detail[:120]}"

    for name, a in got.items():
        stamped = {e.get("provenance") for e in edges_of(a)}
        if stamped and stamped != {want[name]}:
            return False, f"`{name}` edges disagree with the answer: {stamped}"
    return True, "proved, asserted (naming the `.d.ts`), and neither, kept apart"



def case_field_entity() -> tuple[bool, str]:
    """A record field is addressable, and its impact set states its own limits.

    `model` declares `email` on `User` and again on `Contact`, `app` reads both,
    one function reads `email` off an `extern_ts` value the checker sees nothing
    behind, and one constructs a `User` with a record literal.

    Five things have to hold at once, and four of them are about absence.
    The proven site is keyed to the record that declares it, so the other
    record's read stays out. The opaque site is over a type that never resolved
    to a field set, so it is neither promoted into the proven list (a claimed
    edge the compiler never made) nor dropped (which would say the site does
    not exist). The record literal is a class of site this relation does
    not hold at all, so the answer says so rather than letting a list of member
    accesses read as the whole impact set. And coverage is stated per file, so a
    project file the sweep could not read is named rather than left out of a
    list that would then read as complete.
    """
    root = CORPUS / "field-entity"
    a = references(root, "src/model.glyph", "User.email")
    if "error" in a:
        return False, f"refused a record field: {a['error'][:140]}"
    if a.get("entity") != "model::User.email":
        return False, f"the answer is not about the field that was asked for: {a.get('entity')}"

    proven = [(s.get("declaration"), s.get("access")) for s in a.get("sites", [])]
    if ("app::greet", "read") not in proven:
        return False, f"lost the read of the field it was asked about: {proven}"
    if ("model::User", "declaration") not in proven:
        return False, f"the field's own declaration is not a site: {proven}"
    if any(d == "app::reach" for d, _ in proven):
        return False, f"merged the same-named field on another record: {proven}"
    if any(d == "app::opaque" for d, _ in proven):
        return False, f"claimed a proven edge for a site it could not key: {proven}"

    unkeyed = a.get("unkeyed", [])
    if not any(s.get("declaration") == "app::opaque" for s in unkeyed):
        return False, f"dropped the unkeyable site entirely; unkeyed={unkeyed}"
    claimed = [s for s in unkeyed if s.get("indexed") is not False]
    if claimed:
        return False, f"an unkeyed site does not say it is unkeyed: {claimed}"
    unexplained = [s.get("declaration") for s in unkeyed if not s.get("not_indexed")]
    if unexplained:
        return False, f"an unkeyed site with no reason for it: {unexplained}"

    coverage = a.get("unindexed")
    if coverage is None:
        return False, (
            "the answer states no per-file coverage, so a file the sweep could not "
            "read would be missing from it with nothing said"
        )
    if coverage:
        return False, f"named a file as unreadable that this corpus parses: {coverage}"

    classes = a.get("not_indexed")
    if not classes:
        return False, (
            "the answer states no limits, so a list of member accesses reads as "
            "the whole impact set; the record literal in src/app.glyph is not in it"
        )
    if not any("record literal" in c for c in classes):
        return False, f"the construction class is unstated: {classes}"

    # A bare field name is still not an address, because two records here
    # declare `email` and answering about either would be a pick.
    bare = references(root, "src/model.glyph", "email")
    if "error" not in bare:
        return False, f"answered a bare field name as if it were an address: {bare}"
    return True, (
        "keyed to its own record; the opaque site named, the coverage and the "
        "literal class both stated"
    )


def summary_arithmetic(a: dict, buckets_key: str) -> str:
    """Why `a`'s summary is not arithmetic over the list beside it, or "".

    The point of a summary is that two callers reading one answer reach the
    same figures. That only holds if the figures are computed from the very
    objects the answer carries, so every total here is checked back against
    the list rather than against a second expectation.
    """
    s = a.get("summary")
    if s is None:
        return "the answer carries no summary, so the totals are the caller's arithmetic again"
    listed = a.get("sites", [])
    if s.get("sites") != len(listed):
        return f"the site total disagrees with the list it summarises: {s.get('sites')} vs {len(listed)}"
    paths = {x.get("path") for x in listed if x.get("path") is not None}
    if s.get("files") != len(paths):
        return f"the file total disagrees with the sites' own paths: {s.get('files')} vs {len(paths)}"
    buckets = s.get(buckets_key)
    if buckets is None:
        return f"no `{buckets_key}` breakdown in the summary: {s}"
    if sum(buckets.values()) != s["sites"]:
        return f"the breakdown does not partition the total: {buckets} against {s['sites']} sites"
    if s.get("not_counted") is None:
        return (
            "the summary states no exclusions at all, so a total that covers everything "
            "and a total that quietly left something out are spelled the same way"
        )
    # One line for the total, one per bucket that has sites in it, one per
    # exclusion. A caller who prints `lines` and acts on them has to see the
    # caveat there and not only in the object.
    want = 1 + sum(1 for n in buckets.values() if n) + len(s["not_counted"])
    if len(s.get("lines", [])) != want:
        return f"the rendered lines do not carry every count and exclusion: {s.get('lines')}"
    return ""


def case_summary_states_what_it_could_not_count() -> tuple[bool, str]:
    """Was G198. A total says what it could not count, or it is a partial list
    with an authoritative figure in front of it.

    The corpus holds three sites the relation keyed across two files, one site
    it could not key (a file whose module line disagrees with its path,
    declaring a namesake `Kind`), and one file that does not parse. "3 match
    sites across 2 files" is true of the first group and, said on its own, is
    the partial-list-as-complete failure in its most persuasive form: a number
    reads as authoritative in a way a list does not, and nothing in it tells
    the caller that a fourth site and an entire file sit outside it.

    Rounding away is what is being tested, so both directions are asserted.
    The unkeyable site must not be folded into the total, which would claim an
    edge this project never made, and it must not be dropped from the summary
    either, which would let the total read as complete.

    Both forms are checked. `proposed_variant` splits the sites by
    consequence, the lookup form splits them by state, and a summary that
    stated its exclusions in one and not the other would leave a caller
    reading the other one exactly as misinformed.
    """
    root = CORPUS / "summary-totals"
    for label, proposed, key in [
        ("as a change", "Pending", "consequences"),
        ("as a lookup", "", "states"),
    ]:
        a = ask(root, "src/kinds.glyph", "Kind", proposed=proposed)
        if "error" in a:
            return False, f"{label}: refused: {a['error'][:120]}"
        wrong = summary_arithmetic(a, key)
        if wrong:
            return False, f"{label}: {wrong}"

        s = a["summary"]
        excluded = {e.get("what"): e for e in s["not_counted"]}

        # The site this project could not key. In the total it would be a
        # claimed edge; missing from the exclusions it would be a site the
        # caller never learns about.
        unkeyed = a.get("unkeyed", [])
        if len(unkeyed) != 1:
            return False, f"{label}: the corpus is meant to hold one unkeyable site: {unkeyed}"
        if any(site.get("declaration") == "stray::spell" for site in a["sites"]):
            return False, f"{label}: counted a site it could not key as one it did"
        if "unkeyed" not in excluded:
            return False, (
                f"{label}: rounded the unkeyable site away: the total says "
                f"{s['sites']} sites and says nothing about the one under `unkeyed`"
            )
        if excluded["unkeyed"].get("sites") != len(unkeyed):
            return False, f"{label}: the exclusion miscounts it: {excluded['unkeyed']}"

        # The file the sweep never opened. Its site count is unknown, and
        # `null` is the only honest figure for it: zero would be a number this
        # answer does not have.
        unread = a.get("unindexed")
        if unread is None:
            return False, f"{label}: the answer names no file the sweep could not read"
        if [u.get("path") for u in unread] != ["src/unreadable.glyph"]:
            return False, f"{label}: the unreadable file is not named: {unread}"
        if "unindexed" not in excluded:
            return False, (
                f"{label}: the total counts only the files it could read and says so nowhere"
            )
        if excluded["unindexed"].get("sites") is not None:
            return False, (
                f"{label}: put a site count on a file nothing was read from: "
                f"{excluded['unindexed']}"
            )
        if excluded["unindexed"].get("files") != len(unread):
            return False, f"{label}: the exclusion miscounts the files: {excluded['unindexed']}"

    # A summary with nothing to exclude states that too, explicitly. An absent
    # field would spell "nothing was left out" and "exclusions were never
    # worked out" the same way, which is the ambiguity an empty site list used
    # to carry.
    clean = ask(CORPUS / "unsupported-entity", "src/m.glyph", "Status")
    if "error" in clean:
        return False, f"a union with no sites refused: {clean['error'][:120]}"
    if clean.get("summary", {}).get("not_counted") != []:
        return False, (
            "a summary with nothing to exclude omits the field rather than emptying it: "
            f"{clean.get('summary')}"
        )
    return True, "the unkeyable site and the unread file are both stated beside the total"


HARD = [
    ("missing identity", case_missing_identity),
    ("ambiguous identity", case_ambiguous_identity),
    ("unsupported entity (field)", case_unsupported_entity_field),
    ("malformed module", case_malformed_module),
    ("unresolved import (impact surface)", case_unresolved_import),
    ("unresolved import (compiler)", case_unresolved_import_compiles),
    ("unsupported entity (record)", case_unsupported_entity_record),
    ("variants named or unread", case_variants_are_named_or_explicitly_unread),
    ("missing identity (as a change)", case_missing_identity_change),
    ("references under either import spelling", case_import_spelling),
    ("CALLS is the applied sites, with coverage", case_calls_relation),
    ("REFERENCES is everything else, with coverage", case_references_relation),
    ("provenance: proved vs asserted", case_provenance_is_proved_or_asserted),
    ("a record field is an entity, with its limits", case_field_entity),
    ("a total states what it could not count", case_summary_states_what_it_could_not_count),
]

KNOWN: list[tuple[str, object, str]] = []


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

    known = f", {len(KNOWN)} known gaps unchanged" if KNOWN else ""
    print(f"exact-or-absent OK: {len(HARD)} invariants hold{known}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
