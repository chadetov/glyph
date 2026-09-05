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
import shutil
import subprocess
import sys
import tempfile

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



def impact(project: pathlib.Path, entity: str, change: dict, **rest) -> dict:
    """One glyph_impact call. Returns {'error': str} or the answer."""
    arguments = {"entity": entity, "change": change}
    arguments.update(rest)
    return call(project, "glyph_impact", arguments)


def verdicts_of(answer: dict, relation: str = "") -> dict:
    """The impact list as {(entity, line): verdict}, optionally one relation."""
    out = {}
    for entry in answer.get("impact") or []:
        if relation and entry.get("relation") != relation:
            continue
        out[(entry.get("entity"), entry.get("line"))] = entry.get("verdict")
    return out


def by_entity(answer: dict, relation: str = "") -> dict:
    """The same, keyed by declaration alone, for an answer with one site each."""
    out = {}
    for entry in answer.get("impact") or []:
        if relation and entry.get("relation") != relation:
            continue
        out.setdefault(entry.get("entity"), []).append(entry.get("verdict"))
    return out


VERDICTS = pathlib.Path("verdicts")

#: Every verdict a `glyph_impact` entry may carry. Closed, and each member is a
#: different claim about the boundary between what Glyph knows and what it does
#: not.
VERDICT_SET = ["WILL_FAIL", "ABSORBS", "SAFE", "UNDETERMINED", "NOT_INDEXED"]


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



BOUNDARY = pathlib.Path("boundary")

#: Every origin a node may carry. Closed, and each member says something
#: different about what the compiler is able to establish about the node.
ORIGIN_SET = ["glyph", "extern", "opaque-ts"]


def origins_in(answer: dict) -> set:
    """Every origin stamped on an edge of `answer`, both ends."""
    out = set()
    for edge in edges_of(answer):
        out.add(edge.get("to_origin"))
        if edge.get("from") is not None:
            out.add(edge.get("from_origin"))
    return out


def case_origin_is_a_node_attribute() -> tuple[bool, str]:
    """Three origins, stamped on the node and kept out of its key.

    `model::Kind` is declared in this project's Glyph source, so the compiler
    parsed the declaration and holds its variant list. `wire::Row` is declared
    here too and its definition is an `extern_ts` escape, so the resolver found
    the declaration and there is no shape behind it. `wirestat::Status` is
    declared by no Glyph module at all and exists because
    `.types/ambient.d.ts` says it does.

    `wire::Row` is why this is not provenance under a second name. Its edges
    come back `PROVED`, and correctly: the resolver read the declaration and
    the far end is a fact rather than a claim. What it is not is a declaration
    anything can see into, and a caller reading `PROVED` alone concludes the
    opposite.

    The key half is asserted across a spelling change. `Row` is asked for from
    the file that declares it and from the file that imports it, and both have
    to come back under `wire::Row`: an identity carrying the origin would give
    one declaration two names, which is the failure 0.1.108 spent a release
    removing for the module half.

    A fourth case keeps the three honest. `nowhere::audit` is declared by
    nothing, and rounding it to the nearest origin would name a declaration
    file that does not exist, so it carries no origin and says what was
    checked.
    """
    root = CORPUS / BOUNDARY
    want = {
        ("src/model.glyph", "Kind"): ("model::Kind", "PROVED", "glyph"),
        ("src/wire.glyph", "Row"): ("wire::Row", "PROVED", "extern"),
        ("src/app.glyph", "Row"): ("wire::Row", "PROVED", "extern"),
        ("src/handler.glyph", "Status"): ("wirestat::Status", "ASSERTED", "opaque-ts"),
    }
    answers = {}
    for (path, name), (entity, provenance, origin) in want.items():
        a = references(root, path, name)
        if "error" in a:
            return False, f"`{name}` from {path} refused: {a['error'][:120]}"
        answers[(path, name)] = a
        if a.get("entity") != entity:
            return False, f"`{name}` from {path} is keyed `{a.get('entity')}`, not `{entity}`"
        if a.get("provenance") != provenance:
            return False, f"`{entity}` is {a.get('provenance')}, not {provenance}"
        if a.get("origin") != origin:
            return False, f"`{entity}` has origin {a.get('origin')!r}, not {origin!r}"

    opaque = answers[("src/handler.glyph", "Status")]
    if "ambient.d.ts" not in (opaque.get("origin_detail") or ""):
        return False, (
            "an `opaque-ts` node does not name the declaration it came from: "
            f"{(opaque.get('origin_detail') or '')[:120]}"
        )
    ext = answers[("src/wire.glyph", "Row")]
    if "extern_ts" not in (ext.get("origin_detail") or ""):
        return False, (
            "an `extern` node does not say what made it one: "
            f"{(ext.get('origin_detail') or '')[:120]}"
        )

    # Both ends of every edge, because an edge lifted out of its reply names two
    # nodes and has to say what each of them is.
    for (path, name), (entity, _, origin) in want.items():
        a = answers[(path, name)]
        for edge in edges_of(a):
            if "from_origin" not in edge or "to_origin" not in edge:
                return False, f"an edge of `{entity}` states an origin for neither end or one: {edge}"
            if edge.get("to_origin") != origin:
                return False, f"an edge of `{entity}` disagrees with the answer: {edge['to_origin']}"
            if edge.get("from") is not None and edge.get("from_origin") not in ORIGIN_SET:
                return False, f"the near end of an edge of `{entity}` has no origin: {edge}"

    absent = references(CORPUS / "provenance", "src/app.glyph", "audit")
    if "error" in absent:
        return False, f"`nowhere::audit` refused: {absent['error'][:120]}"
    if absent.get("origin") is not None:
        return False, (
            "a node nothing in this project declares was rounded to an origin: "
            f"{absent.get('origin')}"
        )
    if not absent.get("origin_absent"):
        return False, "a node with no origin does not say what was checked to establish that"

    # The third surface. An impact answer names a node per entry, and a verdict
    # read without knowing what the node is is the thing this attribute exists
    # to stop.
    i = impact(root, "model::Kind", {"kind": "add_variant", "variant": "Void"})
    if "error" in i:
        return False, f"impact refused: {i['error'][:140]}"
    if i.get("origin") != "glyph":
        return False, f"the impact subject carries origin {i.get('origin')!r}"
    unstamped = [e.get("entity") for e in i.get("impact") or [] if e.get("origin") is None]
    if unstamped:
        return False, f"an impact entry names a node with no origin: {unstamped}"
    return True, "glyph, extern and opaque-ts on the node, one key across both spellings"


def case_opaque_node_gets_no_inside_verdict() -> tuple[bool, str]:
    """A node the compiler cannot read is never given a verdict reading it
    would settle.

    `handler::describe` matches on `wirestat::Status` and names two arms with
    no catch-all. Over a Glyph union that shape is `exhaustive`, and it is
    decidable because the declaration is there to check the arm list against.
    Here the declaration is a `.d.ts` no Glyph pass read, so whether those two
    arms are the whole union is not a question this project can answer, and
    `exhaustive` would be a manufactured fact wearing the shape of a computed
    one. The site is named `scrutinee_unresolved` instead, which is the other
    half: dropping it would say no match site exists.

    The change form is asserted too. A `proposed_variant` against a union with
    no readable variant list has to be refused rather than answered, because
    the consequence per site is a fact about an edit and nobody can say whether
    this edit is one.

    `model::Kind` is the control, and it carries the invariant's other half.
    The same two-arm shape over a declaration the compiler holds is
    `exhaustive`, so this is a rule about what cannot be established rather
    than a surface that decides nothing.
    """
    root = CORPUS / BOUNDARY
    a = ask(root, "src/handler.glyph", "Status")
    if "error" in a:
        return False, f"refused instead of naming the site: {a['error'][:140]}"
    if (a.get("type") or {}).get("origin") != "opaque-ts":
        return False, f"the node is not stamped `opaque-ts`: {a.get('type')}"
    states = [s.get("state") for s in a.get("sites", [])]
    if "exhaustive" in states:
        return False, (
            "a site over a node with no readable declaration was called exhaustive, "
            f"which is a claim about a variant list nothing here read: {states}"
        )
    if states != ["scrutinee_unresolved"]:
        return False, f"the site was dropped rather than named unresolved: {states}"
    counted = ((a.get("summary") or {}).get("states") or {}).get("exhaustive")
    if counted:
        return False, f"the summary counts {counted} exhaustive sites over an unreadable node"

    b = ask(root, "src/handler.glyph", "Status", proposed="Gone")
    if "error" not in b:
        return False, f"stated a consequence for an edit to a union it cannot read: {b}"

    ext = ask(root, "src/wire.glyph", "Row")
    if "error" in ext:
        return False, f"the extern node refused: {ext['error'][:140]}"
    if (ext.get("type") or {}).get("origin") != "extern":
        return False, f"the `extern_ts` node is not stamped `extern`: {ext.get('type')}"
    if any(s.get("state") == "exhaustive" for s in ext.get("sites", [])):
        return False, "a site over an `extern_ts` type was called exhaustive"

    control = ask(root, "src/model.glyph", "Kind")
    if "error" in control:
        return False, f"the control refused: {control['error'][:140]}"
    if (control.get("type") or {}).get("origin") != "glyph":
        return False, f"a declaration in this project's Glyph source is {control.get('type')}"
    if [s.get("state") for s in control.get("sites", [])] != ["exhaustive"]:
        return False, (
            "the control site over a declaration the compiler holds is not decided: "
            f"{[s.get('state') for s in control.get('sites', [])]}"
        )
    return True, "the unreadable node is named, not decided; the readable one still is"


GENERATED = pathlib.Path("generated")


def generated_edges(project: pathlib.Path, path: str, name: str):
    """The GENERATED_FROM entry of one answer, or an error string."""
    a = references(project, path, name, "GENERATED_FROM")
    if "error" in a:
        return None, a["error"]
    entry = (a.get("relations") or {}).get("GENERATED_FROM")
    if entry is None:
        return None, "the answer does not hold the relation it was asked for"
    return entry, None


def case_generated_from_is_answered_from_a_record() -> tuple[bool, str]:
    """R5. A generated declaration names the artifact it came from, and whether
    that artifact still holds the bytes it was generated from.

    Five files, and each one is a different answer rather than a different
    shade of the same one.

    `petstore.glyph` was generated and its spec is untouched, so the artifact
    is named and the comparison comes back `UNCHANGED`. `stale.glyph` was
    generated and its spec was edited afterwards in a way that changes no type,
    which is the staleness nothing on disk could see before: the emitted Glyph
    is identical and the recorded hash is not. `orphan.glyph` was generated
    from a spec that is now gone, and that is neither `UNCHANGED` nor
    `CHANGED`: an artifact nobody read was compared with nothing, and rounding
    it to a difference would state a fact about content this build never saw.
    The edge survives, because the record names the artifact whether or not the
    artifact is there.

    The two remaining files are the absences, and they are different absences.
    `hand.glyph` carries no record and no sign of a generator, so no
    declaration in it was generated and `[]` is exact. `legacy.glyph` carries a
    `glyph gen` header written before records existed: it *was* generated, this
    build cannot say from what, and `[]` there would say the opposite of the
    truth. It is named under the relation's own coverage instead.

    `appended::Note` is the sixth. It sits in a generated file, below the
    generated declarations, and the record does not name it. The record is what
    makes that answerable at all: without a per-entity list, "this file is
    generated" would sweep in a declaration the generator never wrote.
    """
    root = CORPUS / GENERATED
    want = {
        ("src/petstore.glyph", "Order"): ("petstore.yaml", "UNCHANGED"),
        ("src/stale.glyph", "Ticket"): ("stale.yaml", "CHANGED"),
        ("src/orphan.glyph", "Ghost"): ("orphan.yaml", None),
    }
    for (path, name), (source, state) in want.items():
        entry, err = generated_edges(root, path, name)
        if err:
            return False, f"`{name}` refused: {err[:140]}"
        edges = entry["edges"]
        if len(edges) != 1:
            return False, f"`{name}` has {len(edges)} generated-from edges, not one"
        edge = edges[0]
        if edge.get("source") != source:
            return False, f"`{name}` names `{edge.get('source')}`, not `{source}`"
        if edge.get("source_state") != state:
            return False, (
                f"`{name}` reports source state {edge.get('source_state')!r}, not {state!r}"
            )
        if state is None and not edge.get("source_state_absent"):
            return False, (
                "a source nothing could read was left with no state and no reason, so the "
                "answer does not say whether it was compared"
            )
        if state is not None and edge.get("source_hash_now") is None:
            return False, f"`{name}` states a source state and no hash it compared against"
        # A generator's record is a claim. PROVED would say the compiler
        # established that this artifact produced this declaration.
        if edge.get("provenance") != "ASSERTED":
            return False, f"a generation record is reported {edge.get('provenance')}, not ASSERTED"
        if edge.get("to") is not None or not edge.get("to_absent"):
            return False, (
                "the far end of a generated-from edge is keyed as if it were a declaration: "
                f"{edge.get('to')!r}"
            )

    for path, name in [("src/hand.glyph", "Receipt"), ("src/appended.glyph", "Note")]:
        entry, err = generated_edges(root, path, name)
        if err:
            return False, f"`{name}` refused: {err[:140]}"
        if entry["edges"] or entry["unindexed"] or entry["not_indexed"]:
            return False, (
                f"`{name}` was reported generated, or reported as unreachable: "
                f"{json.dumps(entry)[:160]}"
            )

    entry, err = generated_edges(root, "src/legacy.glyph", "Invoice")
    if err:
        return False, f"`Invoice` refused: {err[:140]}"
    if entry["edges"]:
        return False, "a file with no readable record produced an edge anyway"
    if [u.get("path") for u in entry["unindexed"]] != ["src/legacy.glyph"]:
        return False, (
            "a file generated before records existed reads as hand-written, so `[]` there "
            "says no declaration in it was generated"
        )
    if "before records existed" not in (entry["unindexed"][0].get("why") or ""):
        return False, f"the coverage entry does not say why: {entry['unindexed'][0]}"

    # Coverage is per relation. The sweep's unreadable file hides occurrences
    # from CALLS and REFERENCES and hides no generation record from this one.
    a = references(root, "src/petstore.glyph", "Order")
    if "error" in a:
        return False, f"the full answer refused: {a['error'][:140]}"
    if unindexed_paths(a, "GENERATED_FROM"):
        return False, (
            "GENERATED_FROM borrowed the sweep's coverage, reporting a gap where the record "
            f"it reads is one file: {unindexed_paths(a, 'GENERATED_FROM')}"
        )
    return True, "the artifact named, the comparison stated, and the two absences kept apart"


def case_the_generation_record_is_byte_identical() -> tuple[bool, str]:
    """R7. Two runs over one unchanged spec write the same bytes, and they are
    the bytes committed here.

    The record is diffed and committed along with the Glyph it annotates, so a
    serialization that reordered between runs would recreate exactly the churn
    the formatter exists to prevent. The check is the acceptance test as
    written: serialize twice, compare bytes. The third comparison is against
    the file in the corpus, which is what catches a change that is stable
    across two runs of one build and different from what shipped.

    The runs happen in a copy of the project with the same relative layout, and
    the paths are relative, so nothing in the output depends on where the copy
    is.
    """
    src = CORPUS / GENERATED
    committed = (src / "src" / "petstore.glyph").read_text()
    runs = []
    with tempfile.TemporaryDirectory() as tmp:
        work = pathlib.Path(tmp) / "generated"
        (work / "src").mkdir(parents=True)
        shutil.copy(src / "package.json", work / "package.json")
        shutil.copy(src / "petstore.yaml", work / "petstore.yaml")
        for _ in range(2):
            proc = subprocess.run(
                [str(BIN), "gen", "openapi", "petstore.yaml", "--out", "src"],
                cwd=work, capture_output=True, text=True, timeout=120,
            )
            if proc.returncode != 0:
                return False, f"`glyph gen openapi` failed: {proc.stderr.strip()[:160]}"
            runs.append((work / "src" / "petstore.glyph").read_text())

    if runs[0] != runs[1]:
        return False, "two runs over one unchanged spec wrote different bytes"
    if runs[0] != committed:
        return False, (
            "the committed file is not what this build generates, so the record in the "
            "corpus is a fixture rather than an output"
        )

    lines = [l for l in committed.splitlines() if l.startswith("// @generated-from entity ")]
    if lines != sorted(lines):
        return False, f"the entity lines are not sorted by entity id: {lines}"
    if not any(l.startswith("// glyph-graph ") for l in committed.splitlines()):
        return False, "the record carries no format version, so a later format cannot be told apart"
    return True, "identical across runs and identical to what is committed, entity lines sorted"

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

#: The one closed set of relation names. Every tool that names a relation names
#: it from here, and the spelling is the same in a request, in a reply, and in a
#: coverage statement.
VOCABULARY = ["CALLS", "REFERENCES", "MATCH_SITES", "FIELD_ACCESS", "GENERATED_FROM"]


def case_one_relation_vocabulary() -> tuple[bool, str]:
    """Was G193. One set of relation names, spelled the same way everywhere.

    The tree held two sets that never overlapped. `glyph_references` spelled
    CALLS and REFERENCES on the wire; `glyph_variants` named no relation at
    all, and its site kinds were the positional keys `sites`, `nested` and
    `unkeyed`, so position was the only thing telling them apart. A caller
    could not select a relation, because there was no set to select from, and
    could not read one off a reply either.

    The exact-or-absent half is what this case is for. A relation this answer
    does not hold is refused rather than answered with an empty list, because
    `[]` reads as "no such edges exist". And a site the compiler never keyed
    carries no relation at all, because stamping one on it would claim the edge
    its own list exists to deny.

    GENERATED_FROM was in here as the relation nothing answered, refused on
    those grounds. A surface answers it now, so the assertion flipped: it has
    to be selectable, under the same spelling the reply comes back under, from
    the address form that holds it.
    """
    a = references(CORPUS / "relations", "src/lib.glyph", "charge")
    if "error" in a:
        return False, f"refused a symbol: {a['error'][:120]}"
    named = set((a.get("relations") or {}).keys())
    if not named or not named <= set(VOCABULARY):
        return False, f"a reply names a relation outside the set: {sorted(named)}"
    for key, entry in (a.get("relations") or {}).items():
        for edge in entry.get("edges") or []:
            if edge.get("relation") != key:
                return False, (
                    f"an edge under `{key}` spells its relation "
                    f"`{edge.get('relation')}`, so the name is not one name"
                )

    v = ask(CORPUS / "summary-totals", "src/kinds.glyph", "Kind")
    if "error" in v:
        return False, f"refused a union: {v['error'][:120]}"
    if v.get("relation") != "MATCH_SITES":
        return False, f"the match answer names its relation `{v.get('relation')}`"
    for site in v.get("sites") or []:
        if site.get("relation") != "MATCH_SITES":
            return False, f"a keyed match site names `{site.get('relation')}`"
    for site in v.get("unkeyed") or []:
        if site.get("relation") is not None:
            return False, (
                "a site this project never keyed claims a relation: "
                f"{site.get('relation')}"
            )
        if not site.get("relation_absent"):
            return False, "an unkeyed site does not say why it stands in no relation"

    f = references(CORPUS / "field-entity", "src/model.glyph", "User.email")
    if "error" in f:
        return False, f"refused a field: {f['error'][:120]}"
    if f.get("relation") != "FIELD_ACCESS":
        return False, f"the field answer names its relation `{f.get('relation')}`"

    r = references(CORPUS / "relations", "src/lib.glyph", "charge", "MATCH_SITES")
    if "error" not in r:
        return False, (
            "`MATCH_SITES` came back as an answer rather than a refusal, and another "
            "surface is what answers it; an empty list reads as `no such edges exist`"
        )
    if "MATCH_SITES" not in r["error"]:
        return False, "the refusal for `MATCH_SITES` does not name it"

    # `GENERATED_FROM` used to be here, refused on the grounds that nothing
    # answered it. A surface answers it now, so what is asserted is the
    # opposite: it is selectable by the name the reply comes back under, from
    # the address form that holds it.
    g = references(CORPUS / "relations", "src/lib.glyph", "charge", "GENERATED_FROM")
    if "error" in g:
        return False, f"`GENERATED_FROM` is refused where it is answered: {g['error'][:120]}"
    if "GENERATED_FROM" not in (g.get("relations") or {}):
        return False, "asking for `GENERATED_FROM` returned an answer that does not hold it"

    return True, (
        "one set, spelled the same in request and reply; an unkeyed site names "
        "none, and a relation this answer does not hold is refused"
    )



def case_verdict_will_fail() -> tuple[bool, str]:
    """WILL_FAIL is a proof that the site stops compiling, and nothing weaker.

    Two shapes reach it over one union and both were run against the compiler
    before this was written. `sites::exhaustive` names every variant and has no
    catch-all. `sites::declining` also has an arm the checker read nothing from,
    which used to be answered `UNDETERMINED` on the grounds that an unread arm
    might take the new variant. It cannot: a declined arm is neither counted in
    `covered` nor treated as a catch-all, so E0200 fires for the new variant
    either way. Adding `Dash` to `Cell` reports E0200 at both sites and nothing
    at the third, which is what this asserts.
    """
    a = impact(CORPUS / VERDICTS, "cells::Cell", {"kind": "add_variant", "variant": "Dash"})
    if "error" in a:
        return False, f"refused: {a['error'][:160]}"
    seen = by_entity(a, "MATCH_SITES")
    for decl in ["sites::exhaustive", "sites::declining"]:
        if seen.get(decl) != ["WILL_FAIL"]:
            return False, (
                f"`{decl}` names every variant with no catch-all and the compiler "
                f"reports E0200 there; this answer says {seen.get(decl)}"
            )
    entry = next(e for e in a["impact"] if e.get("entity") == "sites::exhaustive")
    if entry.get("diagnostic") != "E0200":
        return False, f"a proved failure names no diagnostic: {entry}"
    return True, "both sites the compiler fails are WILL_FAIL, and each names E0200"


def case_verdict_absorbs() -> tuple[bool, str]:
    """ABSORBS is a proof that the change lands here and nothing reports it.

    `absorbs::absorbing` has a catch-all naming `Text` only, so `Dash` reaches
    `else`. Run against the compiler: adding the variant produces no diagnostic
    in that module at all. The verdict has to be its own, not WILL_FAIL and not
    SAFE, because the site keeps compiling and stops being right.
    """
    a = impact(CORPUS / VERDICTS, "cells::Cell", {"kind": "add_variant", "variant": "Dash"})
    if "error" in a:
        return False, f"refused: {a['error'][:160]}"
    seen = by_entity(a, "MATCH_SITES")
    if seen.get("absorbs::absorbing") != ["ABSORBS"]:
        return False, (
            "a catch-all site takes the new variant silently and the compiler "
            f"reports nothing there; this answer says {seen.get('absorbs::absorbing')}"
        )
    entry = next(e for e in a["impact"] if e.get("entity") == "absorbs::absorbing")
    if entry.get("diagnostic") is not None:
        return False, f"an absorbed change claims a diagnostic: {entry}"
    if not entry.get("diagnostic_absent"):
        return False, f"a null diagnostic does not say why it is null: {entry}"
    return True, "the catch-all site is ABSORBS, and it claims no diagnostic"


def case_verdict_safe() -> tuple[bool, str]:
    """SAFE is a proof that the site is still right, not a shrug.

    The same catch-all site, under a different change. Removing `Blank` leaves
    `absorbs::absorbing` naming no arm that is gone, and the module keeps
    compiling: run against the compiler, the only error is in `sites`, which
    imports the name. So one site is SAFE and another over the same union is
    WILL_FAIL under the same edit, which is what stops SAFE from being a
    default.
    """
    a = impact(CORPUS / VERDICTS, "cells::Cell", {"kind": "remove_variant", "variant": "Blank"})
    if "error" in a:
        return False, f"refused: {a['error'][:160]}"
    seen = by_entity(a, "MATCH_SITES")
    if seen.get("absorbs::absorbing") != ["SAFE"]:
        return False, (
            "a site naming no arm the edit removes keeps compiling and keeps "
            f"meaning what it meant; this answer says {seen.get('absorbs::absorbing')}"
        )
    if seen.get("sites::exhaustive") != ["WILL_FAIL"]:
        return False, (
            "a site whose arm names the removed variant stops resolving; this "
            f"answer says {seen.get('sites::exhaustive')}"
        )
    return True, "SAFE and WILL_FAIL land on two sites over one union under one edit"


def case_verdict_undetermined() -> tuple[bool, str]:
    """UNDETERMINED means reached and not decidable, and it has to stay rare.

    `sites::nested` reaches `Inner` through `A`'s payload and has a catch-all
    one level down. The relation records that catch-all with a depth and not
    with the union it belongs to, so it may be the scope a new `Inner` variant
    lands in or a sibling payload, and the answer cannot tell. That is the
    reason, and the case asserts it is the only entry carrying the verdict:
    every other site in the corpus is decidable, and a verdict that spreads is
    read as "probably safe", which is the inversion of what it means.

    It also asserts the entry is not NOT_INDEXED. The two are different claims:
    the relation does hold this site, and looking harder at this site is not
    what would settle it.
    """
    a = impact(CORPUS / VERDICTS, "cells::Inner", {"kind": "add_variant", "variant": "Z"})
    if "error" in a:
        return False, f"refused: {a['error'][:160]}"
    undecided = [e for e in a.get("impact") or [] if e.get("verdict") == "UNDETERMINED"]
    if [e.get("entity") for e in undecided] != ["sites::nested"]:
        return False, (
            "UNDETERMINED is not confined to the site that cannot be decided: "
            f"{[e.get('entity') for e in undecided]}"
        )
    entry = undecided[0]
    if entry.get("relation") != "MATCH_SITES":
        return False, f"an undetermined site names no relation: {entry}"
    if not entry.get("because"):
        return False, f"UNDETERMINED states no reason: {entry}"

    # The other half: the same corpus, a change whose sites are all decidable,
    # and none of them may land here for convenience.
    b = impact(CORPUS / VERDICTS, "cells::Cell", {"kind": "add_variant", "variant": "Dash"})
    if "error" in b:
        return False, f"refused: {b['error'][:160]}"
    stray = [e.get("entity") for e in b.get("impact") or [] if e.get("verdict") == "UNDETERMINED"]
    if stray:
        return False, f"a decidable site came back UNDETERMINED: {stray}"
    return True, "one site, reached and not decidable, and no decidable site joins it"


def case_verdict_not_indexed() -> tuple[bool, str]:
    """NOT_INDEXED means the question was never askable, which is not a shrug
    about one site.

    Two classes, both run against the compiler. Adding a parameter to
    `api::width` is E0213 at `api::called`, which applies it, and nothing at
    all at `api::sizer`, which reads it as a value: Glyph never compares a
    function value's arity against the type its use context expects. And
    `api::takes_string` declaring a named type instead of `string` produces no
    diagnostic at its call site, where a `bool` would be E0211 (G201).

    The case asserts the two verdicts do not blur. A NOT_INDEXED entry names
    the class the model does not hold, and no entry in either answer is
    UNDETERMINED, because nothing here was reached and left undecided.
    """
    a = impact(CORPUS / VERDICTS, "api::width", {"kind": "change_arity"})
    if "error" in a:
        return False, f"refused: {a['error'][:160]}"
    seen = by_entity(a)
    if seen.get("api::called") != ["WILL_FAIL"]:
        return False, f"the call site is not proved to fail: {seen.get('api::called')}"
    if seen.get("api::sizer") != ["NOT_INDEXED"]:
        return False, (
            "a function read as a value is a class Glyph does not check; this "
            f"answer says {seen.get('api::sizer')}"
        )
    if any(e.get("verdict") == "UNDETERMINED" for e in a.get("impact") or []):
        return False, "a class the model does not hold was reported as undecided"
    for entry in a["impact"]:
        if entry.get("verdict") == "NOT_INDEXED" and not entry.get("because"):
            return False, f"NOT_INDEXED names no class: {entry}"
    for search in a.get("searches") or []:
        if search["relation"] == "REFERENCES" and not search.get("not_indexed"):
            return False, f"the search states no class it cannot answer: {search}"

    b = impact(CORPUS / VERDICTS, "api::takes_string", {"kind": "change_signature_type"})
    if "error" in b:
        return False, f"refused: {b['error'][:160]}"
    verdicts = {e.get("verdict") for e in b.get("impact") or []}
    if verdicts != {"NOT_INDEXED"}:
        return False, (
            "a signature-type change ships one verdict and this answer carries "
            f"{sorted(verdicts)}"
        )
    return True, "a class absent from the model, said as that and not as undecided"


def case_impact_answers_a_second_hop() -> tuple[bool, str]:
    """A request past hop 1 is answered, not refused.

    The carrier is exact at hop 1 and empty at hop 2, because Glyph never
    infers a declaration's type from its body: a change to X can only
    invalidate expressions that name X. So the answer is the hop-1 answer,
    plus the question that would be exact, as a field a program can read
    rather than an error a program has to parse.
    """
    one = impact(CORPUS / VERDICTS, "cells::Cell", {"kind": "add_variant", "variant": "Dash"})
    two = impact(
        CORPUS / VERDICTS,
        "cells::Cell",
        {"kind": "add_variant", "variant": "Dash"},
        depth=2,
    )
    if "error" in two:
        return False, f"a hop-2 request was refused rather than answered: {two['error'][:160]}"
    if two.get("impact") != one.get("impact"):
        return False, "the hop-2 answer is not the hop-1 answer"
    nxt = two.get("next_query")
    if not isinstance(nxt, dict) or nxt.get("tool") != "glyph_impact":
        return False, f"no next_query names the question that would be exact: {nxt}"
    if not isinstance(nxt.get("arguments_template"), dict):
        return False, f"next_query names no arguments to fill in: {nxt}"
    if not nxt.get("roots"):
        return False, f"next_query names no root to ask about: {nxt}"
    if two.get("depth_answered") != 1:
        return False, f"the answer does not say which hop it answered: {two}"
    return True, "hop 1 exact, and the next question named as a field"


def case_a_change_is_required_for_a_consequence() -> tuple[bool, str]:
    """No edit named, no verdict. A lookup wearing a new name is the failure
    this argument exists to prevent, so the request refuses rather than
    answering every entry `REFERENCES`.

    The change kinds are closed too: a kind outside the set is an error, and so
    is one the entity cannot carry.
    """
    a = call(CORPUS / VERDICTS, "glyph_impact", {"entity": "cells::Cell"})
    if "error" not in a:
        return False, "an entity with no change was answered rather than refused"
    if "change" not in a["error"]:
        return False, f"the refusal does not name the missing field: {a['error'][:160]}"

    b = impact(CORPUS / VERDICTS, "cells::Cell", {"kind": "reticulate"})
    if "error" not in b:
        return False, "a change kind outside the closed set was answered"

    c = impact(CORPUS / VERDICTS, "api::width", {"kind": "add_variant", "variant": "Dash"})
    if "error" not in c:
        return False, "a variant was added to something that is not a union"
    return True, "a consequence needs an edit, and the kinds are a closed set"


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
    ("origin is a node attribute, not a key", case_origin_is_a_node_attribute),
    ("an opaque-ts node gets no inside verdict", case_opaque_node_gets_no_inside_verdict),
    ("GENERATED_FROM is read from a record", case_generated_from_is_answered_from_a_record),
    ("the record is byte-identical across runs", case_the_generation_record_is_byte_identical),
    ("a total states what it could not count", case_summary_states_what_it_could_not_count),
    ("one relation vocabulary, named the same way", case_one_relation_vocabulary),
    ("WILL_FAIL is proved, not assumed", case_verdict_will_fail),
    ("ABSORBS is its own answer", case_verdict_absorbs),
    ("SAFE is proved, not a default", case_verdict_safe),
    ("UNDETERMINED is reached and rare", case_verdict_undetermined),
    ("NOT_INDEXED is a class, not a shrug", case_verdict_not_indexed),
    ("a hop-2 request is answered", case_impact_answers_a_second_hop),
    ("a consequence needs a named change", case_a_change_is_required_for_a_consequence),
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
