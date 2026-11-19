#!/usr/bin/env python3
"""Write the segments file a translator edits for one page in one locale.

A translator never edits a translated page's HTML. They edit a segments file:
every piece of text on the English page, in order, each with a stable id, the
English and a slot for the translation. scripts/i18n_apply.py then rebuilds
the translated page from the English source and that file, so the
translation's structure is the English structure by construction.

    python3 scripts/i18n_extract.py PAGE LOCALE      # one page
    python3 scripts/i18n_extract.py catches LOCALE   # the case texts of /catches/

PAGE is a URL path or a source path: `/start/`, `start`, `/` (the home page),
`404`, or `site/pages/start/index.html`. LOCALE is one of the site's locales
(pt-br, es, ja, tr, zh-cn, ko, de, ru, th).

The segments file sits beside the translated page it produces:

  site/pages/start/index.html            English source
  site/i18n/es/start/index.segments.json what the translator edits
  site/i18n/es/start/index.html          what i18n_apply.py writes

A segments file is JSON:

  {"page": "start/index.html", "locale": "es", "source_hash": "...",
   "segments": [{"id": "h-1a2b3c4d", "kind": "html", "en": "...", "tr": ""}, ...]}

The translator fills every "tr". Kinds:

  title, description, keywords      the front matter (keywords from `head`)
  prev.*, next.*, ns.*              the prev/next pair and the next-steps block
  html                              a run of text with its inline markup, from a
                                    heading, paragraph, list item, table cell,
                                    caption, button or link label
  attr:title, attr:alt,
  attr:aria-label, attr:placeholder an attribute outside any html segment
  script                            a string an inline script shows on the page

Never extracted: <pre> blocks, <code>/<kbd>/<samp> on their own, inline
scripts other than the strings they show, URLs, class names, ids, and the
{{...}} placeholders the generator fills, which stay inside the text they sit
in. A run whose only text is a file path or a `$` command is left alone.

Re-running extract on a page that already has a segments file merges by id:
an id whose English is unchanged keeps its translation, a new id starts empty,
and an id whose English changed starts empty with the old English and the old
translation beside it as "was_en" and "was_tr", so the translator sees what to
redo. The id of a text segment is a hash of its English, so an edited sentence
gets a new id; identical text on one page shares one id and one translation.

When a translated page exists with no segments file (a page translated by hand
before this tool), extract reads its translations out of it, aligned segment by
segment with the English, as long as it is current (its source_hash matches)
and the two line up exactly. Otherwise it refuses rather than guess.

`catches` writes site/i18n/<locale>/catches.json, the per-case texts the
generated /catches/ page and the home page's case cards show, merged the same
way. The labels around them are UI strings in strings.json.

Python standard library only.
"""
from __future__ import annotations

import sys

sys.dont_write_bytecode = True

import difflib  # noqa: E402
import hashlib  # noqa: E402
import html  # noqa: E402
import json  # noqa: E402
import pathlib  # noqa: E402
import re  # noqa: E402

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build_site  # noqa: E402
import site_i18n  # noqa: E402

ROOT = site_i18n.ROOT
PAGES = site_i18n.PAGES
I18N = site_i18n.I18N


class ExtractError(Exception):
    pass


# ---------------------------------------------------------------- HTML tree

VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
        "param", "source", "track", "wbr"}
# Elements that sit inside a line of text.
INLINE = {"a", "abbr", "b", "bdi", "br", "cite", "code", "data", "del", "dfn", "em", "i", "img",
          "ins", "kbd", "label", "mark", "q", "s", "samp", "small", "span", "strong", "sub",
          "sup", "time", "u", "var", "wbr"}
# Elements whose content is never read as markup.
OPAQUE = {"pre", "script", "style", "svg", "textarea", "template", "math"}
# Inline elements whose content is code: it stays English and never makes a
# run translatable by itself.
CODEISH = {"code", "kbd", "samp"}
TR_ATTRS = ("title", "alt", "aria-label", "placeholder")

TAG = re.compile(r"<!--.*?-->|<(/?)([a-zA-Z][a-zA-Z0-9-]*)((?:[^>\"']|\"[^\"]*\"|'[^']*')*)>", re.S)
ATTR = re.compile(r"([^\s=/>\"']+)(?:\s*=\s*(?:\"([^\"]*)\"|'([^']*)'|([^\s>\"']+)))?")
PLACEHOLDER = re.compile(r"\{\{[a-z_]+(?::[a-z-]+)?\}\}")


class El:
    def __init__(self, tag: str, attrs: list, start: int, open_end: int):
        self.tag, self.attrs, self.start, self.open_end = tag, attrs, start, open_end
        self.close_start = self.end = open_end
        self.children: list = []

    @property
    def cls(self) -> str | None:
        for name, value, _, _ in self.attrs:
            if name == "class":
                return value
        return None


class Text:
    def __init__(self, start: int, end: int):
        self.start, self.end = start, end


def parse_attrs(text: str, offset: int) -> list:
    """[(name, value, value start, value end)], offsets into the whole body."""
    out = []
    for m in ATTR.finditer(text):
        for g in (2, 3, 4):
            if m.group(g) is not None:
                out.append((m.group(1).lower(), m.group(g), offset + m.start(g), offset + m.end(g)))
                break
        else:
            out.append((m.group(1).lower(), None, -1, -1))
    return out


def parse(body: str) -> El:
    """The body as a tree of El and Text with offsets. Opaque elements keep no
    children. Tolerant of an end tag that closes several open elements."""
    root = El("#root", [], 0, 0)
    stack = [root]
    pos = 0
    while pos < len(body):
        m = TAG.search(body, pos)
        if not m:
            stack[-1].children.append(Text(pos, len(body)))
            break
        if m.start() > pos:
            stack[-1].children.append(Text(pos, m.start()))
        pos = m.end()
        if m.group(0).startswith("<!--"):
            continue
        closing, tag = m.group(1), m.group(2).lower()
        if closing:
            if any(e.tag == tag for e in stack[1:]):
                while stack[-1].tag != tag:
                    e = stack.pop()
                    e.close_start = e.end = m.start()
                e = stack.pop()
                e.close_start, e.end = m.start(), m.end()
            continue
        el = El(tag, parse_attrs(m.group(3), m.start(3)), m.start(), m.end())
        stack[-1].children.append(el)
        if tag in VOID or m.group(3).rstrip().endswith("/"):
            continue
        if tag in OPAQUE:
            close = re.compile(rf"</{tag}\s*>", re.I).search(body, m.end())
            if not close:
                raise ExtractError(f"<{tag}> at offset {m.start()} is never closed")
            el.close_start, el.end = close.start(), close.end()
            pos = close.end()
            continue
        stack.append(el)
    for e in stack[1:]:
        e.close_start = e.end = len(body)
    root.end = root.close_start = len(body)
    return root


# ---------------------------------------------------------------- what is text

LETTER = re.compile(r"[^\W\d_]")


def visible(body: str, node) -> str:
    """The text a node shows that a translator would translate: no code, no
    placeholders, no entities."""
    if isinstance(node, Text):
        t = PLACEHOLDER.sub(" ", body[node.start:node.end])
        return re.sub(r"&[a-zA-Z]+;|&#x?[0-9a-fA-F]+;", " ", t)
    if node.tag in CODEISH or node.tag in OPAQUE:
        return ""
    return "".join(visible(body, c) for c in node.children)


def has_letters(body: str, node) -> bool:
    return bool(LETTER.search(visible(body, node)))


def inline(node) -> bool:
    """Text, or an inline element with nothing but inline elements inside
    (an <svg> or a block inside a span breaks the run there)."""
    if isinstance(node, Text):
        return True
    if node.tag not in INLINE:
        return False
    return node.tag in CODEISH or all(inline(c) for c in node.children)


def carries_text(body: str, node) -> bool:
    """A text node with letters, or an unclassed inline element that holds
    such text itself (not only through classed children)."""
    if isinstance(node, Text):
        return has_letters(body, node)
    return (node.tag in INLINE and node.tag not in CODEISH and node.cls is None and inline(node)
            and direct_text(body, node))


def direct_text(body: str, el: El) -> bool:
    """Whether `el` holds text of its own. A container of only classed
    elements (cards, tiles, tag rows, status items) is structure, and its
    children are segmented one by one."""
    return any(carries_text(body, c) for c in el.children)


# A run that is only a file name, a path, a shell command, a code-like
# token, an error code or a version number stays as written.
PATHLIKE = re.compile(r"^(?:\$ .*|\S*[./()<>\[\]{}=:_]\S*|(?:[A-Z]+\d+|\d+(?:\.\d+)+)(?:[ ,/]+(?:[A-Z]+\d+))*)$")


def norm(fragment: str) -> str:
    return re.sub(r"\s+", " ", fragment).strip()


def seg_id(prefix: str, text: str) -> str:
    return f"{prefix}-{hashlib.sha256(text.encode()).hexdigest()[:8]}"


class Seg:
    """One occurrence of a segment in a body: kind, English, and the byte
    range a translation replaces."""

    def __init__(self, kind: str, en: str, start: int, end: int):
        self.kind, self.en, self.start, self.end = kind, en, start, end
        prefix = {"html": "h", "script": "j"}.get(kind, "a")
        self.id = seg_id(prefix, (kind + "\n" + en) if prefix == "a" else en)


SCRIPT_STRING = re.compile(r"\.(?:textContent|innerText)\s*=\s*\"((?:[^\"\\]|\\.)*)\"")


def body_segments(body: str) -> list[Seg]:
    """Every translatable occurrence in a page body, in document order."""
    out: list[Seg] = []

    def attrs(el: El) -> None:
        for name, value, vs, ve in el.attrs:
            if name in TR_ATTRS and value and LETTER.search(value):
                out.append(Seg(f"attr:{name}", html.unescape(value), vs, ve))

    def walk(el: El) -> None:
        if el.tag != "#root":
            attrs(el)
        if el.tag == "script":
            inner = body[el.open_end:el.close_start]
            for m in SCRIPT_STRING.finditer(inner):
                text = json.loads(f'"{m.group(1)}"')
                if LETTER.search(text):
                    out.append(Seg("script", text, el.open_end + m.start(1), el.open_end + m.end(1)))
            return
        if el.tag in OPAQUE or el.tag in CODEISH:
            return
        if not direct_text(body, el):
            for c in el.children:
                if isinstance(c, El):
                    walk(c)
            return
        run: list = []

        def trimmable(n) -> bool:
            """Whitespace, or an element that shows nothing (an empty span,
            an icon). An element holding a number or a placeholder stays in
            the run, so a translation can move it."""
            if isinstance(n, Text):
                return not body[n.start:n.end].strip()
            return not re.sub(r"<[^>]+>", "", body[n.start:n.end]).strip()

        def flush() -> None:
            items = list(run)
            run.clear()
            # Trim whitespace and letterless elements (icons, number badges,
            # empty spans) off both ends; walk the trimmed elements for
            # their attributes.
            while items and trimmable(items[0]):
                if isinstance(items[0], El):
                    walk(items[0])
                items.pop(0)
            tail = []
            while items and trimmable(items[-1]):
                if isinstance(items[-1], El):
                    tail.append(items[-1])
                items.pop()
            if items:
                start, end = items[0].start, items[-1].end
                # Leading and trailing whitespace inside the edge text nodes
                # stays outside the segment.
                frag = body[start:end]
                start += len(frag) - len(frag.lstrip())
                end -= len(frag) - len(frag.rstrip())
                text = norm(body[start:end])
                plain = norm(html.unescape(re.sub(r"<[^>]+>", "", PLACEHOLDER.sub("", text))))
                if any(has_letters(body, n) for n in items) and not PATHLIKE.match(plain):
                    out.append(Seg("html", text, start, end))
            for t in reversed(tail):
                walk(t)

        for c in el.children:
            if inline(c):
                run.append(c)
            else:
                flush()
                walk(c)
        flush()

    walk(parse(body))
    return out


# ---------------------------------------------------------------- front matter


def pipe_fields(value: str) -> list[str]:
    return [p.strip() for p in value.split(" | ")]


KEYWORDS = re.compile(r'(<meta name="keywords" content=")([^"]*)(")')


def front_segments(fm: dict) -> list[tuple[str, str, str]]:
    """[(id, kind, English)] for the front matter."""
    out = [("title", "title", fm["title"]), ("description", "description", fm["description"])]
    m = KEYWORDS.search(fm.get("head", ""))
    if m:
        out.append(("keywords", "keywords", html.unescape(m.group(2))))
    for key in ("prev", "next"):
        if key in fm:
            href, label, title = pipe_fields(fm[key])
            out += [(f"{key}.label", f"{key}.label", label), (f"{key}.title", f"{key}.title", title)]
    if fm.get("next_steps"):
        lines = [ln.strip() for ln in fm["next_steps"].split("\n") if ln.strip()]
        out.append(("ns.heading", "ns.heading", lines[0]))
        label, href, _icon, title, desc, cta = pipe_fields(lines[1])
        out += [("ns.label", "ns.label", label), (f"ns.{href}.title", "ns.title", title),
                (f"ns.{href}.desc", "ns.desc", desc), ("ns.cta", "ns.cta", cta)]
        for line in lines[2:]:
            href, _icon, title, desc = pipe_fields(line)
            out += [(f"ns.{href}.title", "ns.title", title), (f"ns.{href}.desc", "ns.desc", desc)]
    return out


# ---------------------------------------------------------------- pages and paths


def translatable_pages() -> list[pathlib.Path]:
    """Every English source a locale can translate: all of site/pages/
    except the release notes."""
    out = [p for p in build_site.page_files()
           if not any(p.relative_to(PAGES).as_posix().startswith(u) for u in site_i18n.UNTRANSLATED)]
    return out + [build_site.NOT_FOUND]


def resolve_page(arg: str) -> pathlib.Path:
    """An English source from a URL path or a source path."""
    p = pathlib.Path(arg)
    if p.suffix == ".html":
        full = (pathlib.Path.cwd() / p).resolve() if not p.is_absolute() else p
        if not full.is_file():
            full = (PAGES / arg).resolve()
        if full.is_file() and PAGES in full.parents:
            return full
        raise ExtractError(f"{arg}: not a page under site/pages/")
    rel = arg.strip("/")
    if rel in ("404", "404.html"):
        return build_site.NOT_FOUND
    path = PAGES / rel / "index.html" if rel else PAGES / "index.html"
    if not path.is_file():
        raise ExtractError(f"{arg}: no English page at {path.relative_to(ROOT)}")
    return path


def check_locale(loc: str) -> str:
    if loc not in site_i18n.TRANSLATED:
        raise ExtractError(f"{loc!r} is not a locale; the locales are {', '.join(site_i18n.TRANSLATED)}")
    return loc


def page_rel(english: pathlib.Path) -> str:
    return english.relative_to(PAGES).as_posix()


def translation_path(english: pathlib.Path, loc: str) -> pathlib.Path:
    return I18N / loc / page_rel(english)


def segments_path(english: pathlib.Path, loc: str) -> pathlib.Path:
    t = translation_path(english, loc)
    return t.with_name(t.stem + ".segments.json")


def english_page(english: pathlib.Path) -> tuple[dict, str]:
    if english == build_site.NOT_FOUND:
        return build_site.parse_front(english)
    return build_site.parse_page(english)


def unique(english: pathlib.Path) -> list[dict]:
    """The segments of an English page, each id once, in first-seen order."""
    fm, body = english_page(english)
    out, seen = [], set()
    for sid, kind, en in front_segments(fm):
        if sid not in seen:
            seen.add(sid)
            out.append({"id": sid, "kind": kind, "en": en})
    for s in body_segments(body):
        if s.id not in seen:
            seen.add(s.id)
            out.append({"id": s.id, "kind": s.kind, "en": s.en})
    return out


def adopt(english: pathlib.Path, loc: str) -> dict[str, str]:
    """Translations read out of a hand-written translated page, by id."""
    tpath = translation_path(english, loc)
    tfm, tbody = build_site.parse_front(tpath, site_i18n.TRANSLATION_KEYS)
    if tfm.get("source_hash") != site_i18n.english_hash(english):
        raise ExtractError(f"{tpath.relative_to(ROOT)} is older than its English page and has no "
                           f"segments file; its text cannot be aligned with the current English")
    efm, ebody = english_page(english)
    merged = site_i18n.merge_front(efm, site_i18n.Translation(loc, tpath, english, tfm, tbody))
    efront, tfront = front_segments(efm), front_segments(merged)
    eb, tb = body_segments(ebody), body_segments(tbody)
    if [k for _, k, _ in efront] != [k for _, k, _ in tfront] or [s.kind for s in eb] != [s.kind for s in tb]:
        raise ExtractError(f"{tpath.relative_to(ROOT)}: its text does not line up segment by segment "
                           f"with the English ({len(eb)} English segments, {len(tb)} translated)")
    out: dict[str, str] = {}
    for (sid, _, _), (_, _, tr) in zip(efront, tfront):
        out.setdefault(sid, tr)
    for e, t in zip(eb, tb):
        out.setdefault(e.id, t.en)
    return out


def merge(fresh: list[dict], old: list[dict]) -> list[dict]:
    """Carry translations from `old` into `fresh` by id. Where an English
    segment was replaced, its old English and translation ride along as
    was_en and was_tr."""
    by_id = {s["id"]: s for s in old}
    out = []
    for s in fresh:
        o = by_id.get(s["id"])
        rec = {"id": s["id"], "kind": s["kind"], "en": s["en"], "tr": (o or {}).get("tr", "")}
        if o is None or not rec["tr"]:
            if o is not None and o.get("was_en"):
                rec["was_en"], rec["was_tr"] = o["was_en"], o["was_tr"]
        out.append(rec)
    old_ids, new_ids = [s["id"] for s in old], [s["id"] for s in out]
    sm = difflib.SequenceMatcher(a=old_ids, b=new_ids, autojunk=False)
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op != "replace":
            continue
        for o, n in zip(old[i1:i2], out[j1:j2]):
            if o["kind"] == n["kind"] and o.get("tr") and not n["tr"] and o["id"] not in new_ids:
                n["was_en"], n["was_tr"] = o["en"], o["tr"]
    return out


def write_json(path: pathlib.Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
    if not path.is_file() or path.read_text() != text:
        path.write_text(text)


ABOUT = ("Translate each \"tr\" from its \"en\"; leave \"id\", \"kind\" and \"en\" alone. Keep every "
         "tag, every <code> element's content and every {{...}} placeholder; words may move around "
         "them. was_en/was_tr show what an edited English segment said before. Then run "
         "scripts/i18n_apply.py. See site/i18n/README.md.")


def extract(english: pathlib.Path, loc: str) -> tuple[pathlib.Path, dict]:
    spath = segments_path(english, loc)
    fresh = unique(english)
    if spath.is_file():
        old = json.loads(spath.read_text())["segments"]
    elif translation_path(english, loc).is_file():
        found = adopt(english, loc)
        old = [{"id": s["id"], "kind": s["kind"], "en": s["en"], "tr": found.get(s["id"], "")} for s in fresh]
    else:
        old = []
    segs = merge(fresh, old)
    data = {"_about": ABOUT, "page": page_rel(english), "locale": loc,
            "source_hash": site_i18n.english_hash(english), "segments": segs}
    write_json(spath, data)
    return spath, data


# ---------------------------------------------------------------- catches

CASE_FIELDS = ("page_title", "why", "ts_verdict", "glyph_verdict", "ts_result", "glyph_result",
               "page_note", "summary")
CATCHES_ABOUT = ("The texts of each case in catches/ as the generated pages show them. Translate "
                 "each \"tr\" from its \"en\"; keep tags, <code> content and placeholders. A case "
                 "whose English changes shows in English until its \"tr\" is redone. See "
                 "site/i18n/README.md.")


def catches_path(loc: str) -> pathlib.Path:
    return I18N / loc / "catches.json"


def case_english() -> dict[str, dict[str, str]]:
    import site_catches
    out = {}
    for c in site_catches.load():
        out[c["slug"]] = {f: c[f] for f in CASE_FIELDS if isinstance(c.get(f), str) and c[f].strip()}
    return out


def extract_catches(loc: str) -> tuple[pathlib.Path, dict]:
    path = catches_path(loc)
    old = json.loads(path.read_text()).get("cases", {}) if path.is_file() else {}
    cases = {}
    for slug, fields in case_english().items():
        rec = {}
        for f, en in fields.items():
            o = old.get(slug, {}).get(f, {})
            entry = {"en": en, "tr": o.get("tr", "") if o.get("en") == en else ""}
            if o.get("tr") and o.get("en") != en:
                entry["was_en"], entry["was_tr"] = o["en"], o["tr"]
            elif o.get("was_en") and not entry["tr"]:
                entry["was_en"], entry["was_tr"] = o["was_en"], o["was_tr"]
            rec[f] = entry
        cases[slug] = rec
    data = {"_about": CATCHES_ABOUT, "locale": loc, "cases": cases}
    write_json(path, data)
    return path, data


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[0] in ("-h", "--help"):
        print(__doc__.split("\n\n")[0] + "\n\n    python3 scripts/i18n_extract.py PAGE LOCALE\n"
              "    python3 scripts/i18n_extract.py catches LOCALE")
        return 0 if argv and argv[0] in ("-h", "--help") else 2
    try:
        loc = check_locale(argv[1])
        if argv[0] == "catches":
            path, data = extract_catches(loc)
            entries = [e for rec in data["cases"].values() for e in rec.values()]
            todo = sum(not e["tr"] for e in entries)
            print(f"wrote {path.relative_to(ROOT)}: {len(data['cases'])} cases, {len(entries)} texts, "
                  f"{todo} to translate")
            return 0
        english = resolve_page(argv[0])
        if any(page_rel(english).startswith(u) for u in site_i18n.UNTRANSLATED):
            raise ExtractError(f"{page_rel(english)} is not translated (the release notes stay English)")
        path, data = extract(english, loc)
    except (ExtractError, build_site.SiteError, site_i18n.I18nError) as e:
        print(f"i18n_extract: {e}", file=sys.stderr)
        return 1
    segs = data["segments"]
    todo = [s for s in segs if not s["tr"]]
    redo = [s for s in todo if s.get("was_en")]
    print(f"wrote {path.relative_to(ROOT)}: {len(segs)} segments, {len(todo)} to translate"
          + (f" ({len(redo)} changed in English, old text in was_en/was_tr)" if redo else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
