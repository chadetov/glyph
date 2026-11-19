#!/usr/bin/env python3
"""Build a translated page from its English source and its segments file.

    python3 scripts/i18n_apply.py PAGE LOCALE           # write the translated page
    python3 scripts/i18n_apply.py PAGE LOCALE --check   # validate only, write nothing

PAGE and LOCALE are as for scripts/i18n_extract.py. This reads
site/i18n/<locale>/<path>/index.segments.json, puts each translation in place
of its English in a copy of the English source, and writes
site/i18n/<locale>/<path>/index.html with the translated front matter and
`source_hash` set to the English page's current hash. Everything that is not
a segment (tags, classes, ids, links, <pre> blocks, placeholders) comes from
the English file, so the translated page has the English structure by
construction.

It refuses, and writes nothing, when:

  - the English page changed since the segments file was extracted (run
    i18n_extract.py again: it keeps every translation whose English is the same)
  - a segment has no translation yet
  - a translation drops, adds or changes a tag or an attribute other than
    title/alt/aria-label/placeholder, changes the content of a <code>, <kbd>
    or <samp> element, or drops or adds a {{...}} placeholder (words may move
    around all of these freely)
  - a front-matter or attribute translation spans lines, or a prev/next or
    next-steps translation contains " | ", which separates their fields
  - the page it would write fails the structure check of scripts/check_i18n.py

It writes only the one translated page, so translators working on different
locales never touch the same file.

Python standard library only.
"""
from __future__ import annotations

import sys

sys.dont_write_bytecode = True

import html  # noqa: E402
import json  # noqa: E402
import pathlib  # noqa: E402
import re  # noqa: E402

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build_site  # noqa: E402
import i18n_extract as ex  # noqa: E402
import site_i18n  # noqa: E402

ROOT = site_i18n.ROOT


class ApplyError(Exception):
    pass


# ---------------------------------------------------------------- validation

OPEN_TAG = re.compile(r"<([a-zA-Z][a-zA-Z0-9-]*)((?:[^>\"']|\"[^\"]*\"|'[^']*')*)>")
CLOSE_TAG = re.compile(r"</([a-zA-Z][a-zA-Z0-9-]*)\s*>")
CODE = re.compile(r"<(code|kbd|samp)\b[^>]*>(.*?)</\1>", re.S)


def tag_signature(fragment: str) -> list[str]:
    """Every opening tag with its attributes, minus the translatable ones,
    sorted: the markup a translation must keep, in any order."""
    out = []
    for m in OPEN_TAG.finditer(fragment):
        attrs = sorted((n, v) for n, v, _, _ in ex.parse_attrs(m.group(2), 0) if n not in ex.TR_ATTRS)
        out.append(m.group(1).lower() + "".join(f" {n}={v!r}" for n, v in attrs))
    return sorted(out)


def balanced(fragment: str) -> bool:
    stack = []
    for m in re.finditer(r"<(/?)([a-zA-Z][a-zA-Z0-9-]*)(?:[^>\"']|\"[^\"]*\"|'[^']*')*>", fragment):
        tag = m.group(2).lower()
        if tag in ex.VOID:
            continue
        if not m.group(1):
            stack.append(tag)
        elif not stack or stack.pop() != tag:
            return False
    return not stack


def codes(fragment: str) -> list[str]:
    return sorted(ex.norm(m.group(2)) for m in CODE.finditer(fragment))


def placeholders(fragment: str) -> list[str]:
    return sorted(ex.PLACEHOLDER.findall(fragment))


PIPED = ("prev.", "next.", "ns.")


def fragment_errors(sid: str, kind: str, en: str, tr: str) -> list[str]:
    """What is wrong with one translation, as short sentences."""
    errs = []
    if placeholders(en) != placeholders(tr):
        errs.append(f"placeholders differ: English has {placeholders(en)}, the translation {placeholders(tr)}")
    if kind == "html":
        if not balanced(tr):
            errs.append("tags do not balance")
        if tag_signature(en) != tag_signature(tr):
            errs.append("the tags differ from the English (same tags and attributes, any order)")
        if codes(en) != codes(tr):
            errs.append("the content of a <code>/<kbd>/<samp> element changed; code stays English")
    else:
        if "\n" in tr:
            errs.append("must be one line")
        if "<" in tr and "<" not in en:
            errs.append("contains markup the English does not have")
        if kind.startswith(PIPED) and " | " in tr:
            errs.append('must not contain " | ", which separates the fields of this line')
    return [f"{sid}: {e}" for e in errs]


# ---------------------------------------------------------------- building


def attr_value(text: str) -> str:
    return text.replace("&", "&amp;").replace('"', "&quot;").replace("<", "&lt;").replace(">", "&gt;")


def build_page(english: pathlib.Path, loc: str, data: dict) -> tuple[str, list[str], list[str]]:
    """(translated page text, errors, untranslated ids) from a segments file's
    data. The text is only usable when errors and untranslated are empty."""
    errors: list[str] = []
    if data.get("page") != ex.page_rel(english) or data.get("locale") != loc:
        errors.append(f"the segments file is for {data.get('page')} in {data.get('locale')}, "
                      f"not {ex.page_rel(english)} in {loc}")
    if data.get("source_hash") != site_i18n.english_hash(english):
        errors.append("the English page changed since this segments file was extracted; "
                      f"run: python3 scripts/i18n_extract.py {ex.page_rel(english)} {loc}")
        return "", errors, []
    tr = {s["id"]: s.get("tr", "") for s in data.get("segments", [])}
    fresh = ex.unique(english)
    untranslated = [s["id"] for s in fresh if not (tr.get(s["id"]) or "").strip()]
    for s in fresh:
        t = tr.get(s["id"]) or ""
        if t.strip():
            errors += fragment_errors(s["id"], s["kind"], s["en"], t)
    if errors or untranslated:
        return "", errors, untranslated

    efm, ebody = ex.english_page(english)
    # The body: each occurrence replaced, last first so offsets hold.
    body = ebody
    for seg in sorted(ex.body_segments(ebody), key=lambda s: s.start, reverse=True):
        t = tr[seg.id].strip()
        if seg.kind == "html":
            new = t
        elif seg.kind == "script":
            new = json.dumps(t, ensure_ascii=False)[1:-1].replace("</", "<\\/")
        else:
            new = attr_value(html.unescape(t))
        body = body[:seg.start] + new + body[seg.end:]

    lines = ["---", f"title: {tr['title'].strip()}", f"description: {tr['description'].strip()}",
             f"source_hash: {site_i18n.english_hash(english)}"]
    if "keywords" in tr:
        head = ex.KEYWORDS.sub(lambda m: m.group(1) + attr_value(html.unescape(tr["keywords"].strip())) + m.group(3),
                               efm["head"], count=1)
        lines.append("head: |")
        lines += [f"  {ln}" if ln else "" for ln in head.split("\n")]
    for key in ("prev", "next"):
        if key in efm:
            href, _, _ = ex.pipe_fields(efm[key])
            lines.append(f"{key}: {href} | {tr[key + '.label'].strip()} | {tr[key + '.title'].strip()}")
    if efm.get("next_steps"):
        steps = [ln.strip() for ln in efm["next_steps"].split("\n") if ln.strip()]
        _, href, icon, _, _, _ = ex.pipe_fields(steps[1])
        out = [tr["ns.heading"].strip(),
               " | ".join([tr["ns.label"].strip(), href, icon, tr[f"ns.{href}.title"].strip(),
                           tr[f"ns.{href}.desc"].strip(), tr["ns.cta"].strip()])]
        for line in steps[2:]:
            href, icon, _, _ = ex.pipe_fields(line)
            out.append(" | ".join([href, icon, tr[f"ns.{href}.title"].strip(), tr[f"ns.{href}.desc"].strip()]))
        lines.append("next_steps: |")
        lines += [f"  {ln}" for ln in out]
    lines.append("---")
    text = "\n".join(lines) + "\n" + body
    errors += page_errors(english, loc, text)
    return text, errors, []


def page_errors(english: pathlib.Path, loc: str, text: str) -> list[str]:
    """The structure check of scripts/check_i18n.py on a page not yet written."""
    import check_i18n
    try:
        _, body = parse_front_text(text, ex.translation_path(english, loc))
    except build_site.SiteError as e:
        return [str(e)]
    _, ebody = ex.english_page(english)
    return check_i18n.page_problems(ebody, body)


def parse_front_text(text: str, where: pathlib.Path) -> tuple[dict, str]:
    """build_site.parse_front on text in memory."""
    class _P:
        def __init__(self, t):
            self.t = t

        def read_text(self):
            return self.t

        def __str__(self):
            return str(where.relative_to(ROOT))
    return build_site.parse_front(_P(text), site_i18n.TRANSLATION_KEYS)  # type: ignore[arg-type]


def load_segments(english: pathlib.Path, loc: str) -> dict:
    spath = ex.segments_path(english, loc)
    if not spath.is_file():
        raise ApplyError(f"no {spath.relative_to(ROOT)}; run: python3 scripts/i18n_extract.py "
                         f"{ex.page_rel(english)} {loc}")
    try:
        return json.loads(spath.read_text())
    except json.JSONDecodeError as e:
        raise ApplyError(f"{spath.relative_to(ROOT)}: not valid JSON: {e}") from None


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith("--")]
    check_only = "--check" in argv
    if len(args) != 2 or "-h" in argv or "--help" in argv:
        print(__doc__.split("\n\n")[1])
        return 0 if ("-h" in argv or "--help" in argv) else 2
    try:
        loc = ex.check_locale(args[1])
        english = ex.resolve_page(args[0])
        data = load_segments(english, loc)
        text, errors, untranslated = build_page(english, loc, data)
    except (ApplyError, ex.ExtractError, build_site.SiteError, site_i18n.I18nError) as e:
        print(f"i18n_apply: {e}", file=sys.stderr)
        return 1
    where = ex.segments_path(english, loc).relative_to(ROOT)
    if untranslated:
        print(f"i18n_apply: {where}: {len(untranslated)} segments have no translation yet: "
              f"{', '.join(untranslated[:8])}{' ...' if len(untranslated) > 8 else ''}", file=sys.stderr)
    for e in errors:
        print(f"i18n_apply: {where}: {e}", file=sys.stderr)
    if errors or untranslated:
        return 1
    out = ex.translation_path(english, loc)
    if check_only:
        same = out.is_file() and out.read_text() == text
        print(f"{where}: OK" + ("" if same else f" ({out.relative_to(ROOT)} is not up to date; run without --check)"))
        return 0
    if not out.is_file() or out.read_text() != text:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text)
        print(f"wrote {out.relative_to(ROOT)}")
    else:
        print(f"{out.relative_to(ROOT)} is up to date")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
