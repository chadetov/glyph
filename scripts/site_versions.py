"""The site's release notes: /versions/, one page per month, and the feed.

Each release's notes live in their own file, site/releases/<version>.html:

    ---
    version: 0.1.150
    month: 2026-11
    date: 2026-11-20
    tags: breaking
    headline: One sentence that says what the release is for.
    ---
            <p>The notes, as HTML: paragraphs, h3 subheads, changelists, code.</p>

`version` must match the file name and `month` is YYYY-MM. `date` is the day
it shipped, YYYY-MM-DD in that month: the day its tag was pushed, which is
what publishes it (`git for-each-ref --format='%(creatordate:short)'
refs/tags/v0.1.N`). The page shows it in a <time> element and the feed dates
the entry with it; a release without one shows its month and dates the entry
the first of the month. `tags` is a comma
list of `breaking` (the release rejects a program the previous one accepted,
established by running both) and `shipped`. `headline` is optional; without it
the entry shows its leading <h3>, or failing that the first sentence of its
first paragraph. Nobody sets `Latest`: it goes on the highest version.

Writing the notes for a release is: add site/releases/0.1.N.html, then run
`python3 scripts/build_site.py`. scripts/check_releases.py fails when a version
up to the one in glyph-compiler/Cargo.toml has no file, so a release cannot go
out without notes.

scripts/build_site.py hands this module the page that carries a
`{{versions:...}}` placeholder (site/pages/versions/index.html) and gets back
several pages rendered from it: /versions/ with the newest month's releases,
and /versions/YYYY-MM/ for every month with a release. It also gets
/versions/feed.xml, an Atom feed of the newest releases.

Placeholders, in that page's body:

  {{versions:months}}    the month selector and the breaking-only toggle
  {{versions:releases}}  this page's releases, newest first
  {{versions:pager}}     links to the next newer and older month
  {{versions:script}}    the toggle, copy-link and old-anchor script

Any other page may quote the notes with `{{releases:...}}`, which
build_site.py expands through expand_quotes():

  {{releases:count}}          how many releases have notes
  {{releases:breaking}}       how many are marked Breaking
  {{releases:latest}}         the newest version
  {{releases:breaking-cost}}  the newest Breaking releases, each with the
                              sentence its own note gives about the example
                              apps, quoted as written

Every release has the anchor id="<version>", for example #0.1.149, and its
permanent link is /versions/YYYY-MM/#0.1.149. The id is the version exactly as
people write it; ids may contain dots and start with a digit in HTML, and
nothing here selects a release by id in CSS (the script uses getElementById
and the CSS uses :target), so there is nothing to escape.

/versions/ and the newest month's page show the same releases. /versions/ is
the canonical one of the two, because it is the address the nav, the docs and
outside links use; the newest month's page points its canonical at it. Once a
newer month exists, that page stops being newest, its content stops being a
duplicate, and its canonical becomes itself.

Python standard library only.
"""
from __future__ import annotations

import html
import json
import pathlib
import re
from dataclasses import dataclass

import site_i18n

ROOT = pathlib.Path(__file__).resolve().parent.parent
RELEASES = ROOT / "site" / "releases"
ORIGIN = "https://glyphlang.io"

# Versions that were never released with notes: 0.1.1 was never published,
# and 0.1.33 was skipped. scripts/check_releases.py allows these two only.
KNOWN_GAPS = {"0.1.1", "0.1.33"}

KEYS = {"version", "month", "date", "tags", "headline"}
TAGS = {"breaking", "shipped"}
OPEN_NEWEST = 3   # how many of the newest releases show their notes expanded
FEED_SIZE = 30

MONTH_NAMES = ["January", "February", "March", "April", "May", "June", "July",
               "August", "September", "October", "November", "December"]


class VersionsError(Exception):
    pass


@dataclass
class Release:
    version: str
    month: str          # YYYY-MM
    date: str | None    # YYYY-MM-DD, the day it shipped
    tags: list[str]
    headline: str       # HTML
    headline_from: str  # "front matter" | "h3" | "sentence"
    body: str           # the notes' inner HTML, byte for byte as written
    path: pathlib.Path

    @property
    def key(self) -> tuple[int, ...]:
        return vkey(self.version)

    @property
    def breaking(self) -> bool:
        return "breaking" in self.tags

    @property
    def permalink(self) -> str:
        return f"/versions/{self.month}/#{self.version}"

    @property
    def day(self) -> str:
        """The ship date, or the first of the month for a release without one."""
        return self.date or f"{self.month}-01"


def vkey(v: str) -> tuple[int, ...]:
    return tuple(int(x) for x in v.split("."))


def month_name(month: str, short: bool = False) -> str:
    year, m = month.split("-")
    name = MONTH_NAMES[int(m) - 1]
    return f"{name[:3] if short else name} {year}"


# ---------------------------------------------------------------- data


def parse(path: pathlib.Path) -> Release:
    where = path.relative_to(ROOT).as_posix()
    text = path.read_text()
    if not text.startswith("---\n"):
        raise VersionsError(f"{where}: no front matter (the file must start with '---')")
    try:
        header, body = text[4:].split("\n---\n", 1)
    except ValueError:
        raise VersionsError(f"{where}: front matter is not closed with '---'") from None
    fm: dict[str, str] = {}
    for n, line in enumerate(header.split("\n"), start=2):
        if not line.strip():
            continue
        m = re.match(r"([a-z]+): (.+)$", line)
        if not m or m.group(1) not in KEYS:
            raise VersionsError(f"{where}: line {n} is not one of {sorted(KEYS)} as 'key: value': {line!r}")
        if m.group(1) in fm:
            raise VersionsError(f"{where}: {m.group(1)!r} given twice")
        fm[m.group(1)] = m.group(2).strip()
    for key in ("version", "month"):
        if key not in fm:
            raise VersionsError(f"{where}: front matter needs {key!r}")
    version = fm["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise VersionsError(f"{where}: version {version!r} is not X.Y.Z")
    if path.name != f"{version}.html":
        raise VersionsError(f"{where}: says version {version}, so the file must be site/releases/{version}.html")
    month = fm["month"]
    if not re.fullmatch(r"\d{4}-(0[1-9]|1[0-2])", month):
        raise VersionsError(f"{where}: month {month!r} is not YYYY-MM")
    date = fm.get("date")
    if date is not None:
        if not re.fullmatch(r"\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])", date):
            raise VersionsError(f"{where}: date {date!r} is not YYYY-MM-DD")
        if not date.startswith(month + "-"):
            raise VersionsError(f"{where}: date {date} is not in month {month}")
    tags = [t.strip() for t in fm.get("tags", "").split(",") if t.strip()]
    for t in tags:
        if t not in TAGS:
            raise VersionsError(f"{where}: unknown tag {t!r} (one of {sorted(TAGS)})")
    if body.endswith("\n"):
        body = body[:-1]
    if not body.strip():
        raise VersionsError(f"{where}: the notes are empty")
    if "headline" in fm:
        headline, source = fm["headline"], "front matter"
    else:
        headline, source = derive_headline(body, where)
    return Release(version, month, date, tags, headline, source, body, path)


def derive_headline(body: str, where: str) -> tuple[str, str]:
    """The entry's leading <h3>, or the first sentence of its first <p>."""
    m = re.match(r"\s*<h3>(.*?)</h3>", body, re.S)
    if m:
        return m.group(1).strip(), "h3"
    m = re.search(r"<p>(.*?)</p>", body, re.S)
    if not m:
        raise VersionsError(f"{where}: no headline in the front matter, and the notes "
                            "start with neither an <h3> nor a <p> to take one from")
    return first_sentence(m.group(1)), "sentence"


def first_sentence(para: str) -> str:
    """The HTML up to the first sentence end that is outside any tag or
    element, so a cut never leaves a <code> or <strong> open."""
    depth, i = 0, 0
    while i < len(para):
        c = para[i]
        if c == "<":
            j = para.index(">", i)
            tag = para[i:j + 1]
            if tag.startswith("</"):
                depth -= 1
            elif not tag.endswith("/>") and not re.match(r"<(br|img|wbr)\b", tag):
                depth += 1
            i = j + 1
            continue
        if c in ".!?" and depth == 0:
            nxt = para[i + 1:i + 2]
            prev = para[max(0, i - 4):i]
            if (nxt == "" or nxt.isspace()) and not re.search(r"(\be\.g|\bi\.e|\bvs)$", prev):
                return para[:i + 1].strip()
        i += 1
    return para.strip()


def load() -> list[Release]:
    """Every release, newest first."""
    if not RELEASES.is_dir():
        raise VersionsError(f"{RELEASES.relative_to(ROOT)} is missing")
    out = [parse(p) for p in sorted(RELEASES.glob("*.html"))]
    if not out:
        raise VersionsError("site/releases/ holds no release")
    out.sort(key=lambda r: r.key, reverse=True)
    for newer, older in zip(out, out[1:]):
        if newer.month < older.month:
            raise VersionsError(
                f"{newer.path.relative_to(ROOT)}: {newer.version} is dated {newer.month}, "
                f"before {older.version} ({older.month})")
    return out


def months(releases: list[Release]) -> list[str]:
    """Every month with a release, newest first."""
    return sorted({r.month for r in releases}, reverse=True)


# ---------------------------------------------------------------- markup


def esc(s: str) -> str:
    return html.escape(s, quote=True)


def count_label(n: int, b: int) -> str:
    s = f"{n} release{'s' if n != 1 else ''}"
    return s + (f" · {b} breaking" if b else "")


def months_html(releases: list[Release], current: str, on_index: bool) -> str:
    out = ['      <nav class="month-nav" aria-label="Releases by month">']
    for m in months(releases):
        rs = [r for r in releases if r.month == m]
        b = sum(r.breaking for r in rs)
        attrs = ""
        if m == current:
            attrs = ' class="active" aria-current="true"' if on_index else ' class="active" aria-current="page"'
        out.append(f'        <a href="/versions/{m}/"{attrs}><span class="mn-month">{month_name(m, short=True)}</span>'
                   f' <span class="mn-count">· {count_label(len(rs), b)}</span></a>')
    out.append("      </nav>")
    rs = [r for r in releases if r.month == current]
    b = sum(r.breaking for r in rs)
    out += [
        '      <div class="month-head">',
        f'        <h2 class="month-title">{month_name(current)}</h2>',
        f'        <p class="month-sum">{count_label(len(rs), b).replace(" · ", ", ")}, '
        f"{rs[-1].version} to {rs[0].version}</p>" if len(rs) > 1 else
        f'        <p class="month-sum">{count_label(len(rs), b).replace(" · ", ", ")}, {rs[0].version}</p>',
        '        <label class="breaking-only" hidden><input type="checkbox" /> Breaking only</label>',
        "      </div>",
    ]
    return "\n".join(out) + "\n"


def release_html(r: Release, latest: str, open_set: set[str]) -> str:
    tags = []
    if r.version == latest:
        tags.append('<span class="rel-tag latest">Latest</span>')
    if r.breaking:
        tags.append('<span class="rel-tag breaking">Breaking</span>')
    if "shipped" in r.tags:
        tags.append('<span class="rel-tag">Shipped</span>')
    head = "\n".join(f"            {t}" for t in tags)
    attrs = f' id="{esc(r.version)}" data-hl="{"fm" if r.headline_from == "front matter" else r.headline_from}"'
    if r.breaking:
        attrs += " data-breaking"
    opened = " open" if r.version in open_set else ""
    # The month is the page's h2 and each release its h3, so the notes'
    # own subheads, written as <h3> in site/releases/, render one level down.
    notes = re.sub(r"<(/?)h3\b", r"<\1h4", r.body)
    return (
        f'        <article class="release"{attrs}>\n'
        '          <div class="rel-head">\n'
        f'            <h3 class="rel-ver">{esc(r.version)}</h3>\n'
        + (head + "\n" if head else "")
        + f'            {date_html(r)}\n'
        f'            <a class="rel-link" href="{r.permalink}" data-url="{ORIGIN}{r.permalink}"'
        f' aria-label="Link to {esc(r.version)}">Link</a>\n'
        "          </div>\n"
        f'          <details class="rel-details"{opened}>\n'
        f'            <summary><span class="rel-headline">{r.headline}</span>'
        '<span class="rel-toggle" aria-hidden="true"></span></summary>\n'
        '          <div class="rel-body">\n'
        f"{notes}\n"
        "          </div>\n"
        "          </details>\n"
        "        </article>\n"
    )


def date_html(r: Release) -> str:
    if r.date:
        y, m, d = r.date.split("-")
        shown = f"{int(d)} {MONTH_NAMES[int(m) - 1][:3]} {y}"
        return f'<time class="rel-date" datetime="{r.date}">{shown}</time>'
    return f'<time class="rel-date" datetime="{r.month}">{month_name(r.month)}</time>'


def month_h1(month: str) -> str:
    return f"<h1>{month_name(month)} releases</h1>"


def releases_html(releases: list[Release], shown: list[Release], month: str) -> str:
    latest = releases[0].version
    open_set = {r.version for r in releases[:OPEN_NEWEST]}
    body = "".join(release_html(r, latest, open_set) for r in shown)
    return ('      <div class="releases">\n' + body +
            f'        <p class="rel-empty" hidden>No release in {month_name(month)} is marked Breaking.</p>\n'
            "      </div>\n")


def pager_html(releases: list[Release], current: str) -> str:
    ms = months(releases)
    i = ms.index(current)
    newer = ms[i - 1] if i > 0 else None
    older = ms[i + 1] if i + 1 < len(ms) else None
    if not newer and not older:
        return ""
    out = ['      <nav class="month-pager" aria-label="Other months">']
    if newer:
        out.append(f'        <a href="/versions/{newer}/" class="newer">&larr; Newer: {month_name(newer)}</a>')
    if older:
        out.append(f'        <a href="/versions/{older}/" class="older">Older: {month_name(older)} &rarr;</a>')
    out.append("      </nav>")
    return "\n".join(out) + "\n"


def script_html(releases: list[Release]) -> str:
    where = json.dumps({r.version: r.month for r in releases}, separators=(",", ":"))
    return SCRIPT.replace("__WHERE__", where)


SCRIPT = """    <script>
      (function () {
        // An anchor for a release that lives on another month's page goes there.
        var where = __WHERE__;
        var id = decodeURIComponent(location.hash.slice(1));
        if (id && !document.getElementById(id) && where[id]) {
          location.replace("/versions/" + where[id] + "/" + location.search + "#" + id);
          return;
        }
        function reveal() {
          var t = location.hash && document.getElementById(decodeURIComponent(location.hash.slice(1)));
          if (!t || !t.classList.contains("release")) return;
          var d = t.querySelector("details");
          if (d) d.open = true;
          if (t.hidden) set(false);
        }
        var list = document.querySelector(".releases");
        var toggle = document.querySelector(".breaking-only");
        var box = toggle && toggle.querySelector("input");
        var empty = document.querySelector(".rel-empty");
        var items = Array.prototype.slice.call(document.querySelectorAll(".release"));
        var months = Array.prototype.slice.call(document.querySelectorAll(".month-nav a, .month-pager a"));
        function set(on) {
          box.checked = on;
          var shown = 0;
          items.forEach(function (el) {
            el.hidden = on && !el.hasAttribute("data-breaking");
            if (!el.hidden) shown++;
          });
          empty.hidden = shown > 0;
          var q = new URLSearchParams(location.search);
          if (on) q.set("breaking", "1"); else q.delete("breaking");
          var s = q.toString() ? "?" + q.toString() : "";
          history.replaceState(null, "", location.pathname + s + location.hash);
          months.forEach(function (a) { a.search = s; });
        }
        if (toggle && list) {
          toggle.hidden = false;
          set(new URLSearchParams(location.search).get("breaking") === "1");
          box.addEventListener("change", function () { set(box.checked); });
        }
        reveal();
        window.addEventListener("hashchange", reveal);
        document.querySelectorAll(".rel-link").forEach(function (a) {
          a.addEventListener("click", function (e) {
            if (!navigator.clipboard) return;
            e.preventDefault();
            history.replaceState(null, "", a.getAttribute("href"));
            navigator.clipboard.writeText(a.dataset.url).then(function () {
              a.textContent = "Copied";
              setTimeout(function () { a.textContent = "Link"; }, 1400);
            });
          });
        });
      })();
    </script>
"""


# ---------------------------------------------------------------- pages


def pages(fm: dict, body: str, where: str) -> list[tuple[str, dict, str]]:
    """The versions template rendered once per page: [(url, front matter, body)]."""
    releases = load()
    ms = months(releases)
    newest = ms[0]
    out = []
    for url, month in [("/versions/", newest)] + [(f"/versions/{m}/", m) for m in ms]:
        on_index = url == "/versions/"
        shown = [r for r in releases if r.month == month]
        blocks = {
            "months": months_html(releases, month, on_index),
            "releases": releases_html(releases, shown, month),
            "pager": pager_html(releases, month),
            "script": script_html(releases),
        }

        def sub(m: re.Match) -> str:
            if m.group(1) not in blocks:
                raise VersionsError(f"{where}: unknown placeholder {{{{versions:{m.group(1)}}}}}")
            return blocks[m.group(1)]
        page_body = re.sub(r"\{\{versions:([a-z]+)\}\}", sub, body)
        page_fm = dict(fm)
        if not on_index:
            # Each month's page has its own heading, matching its title.
            page_body, n = re.subn(r"<h1>.*?</h1>", month_h1(month), page_body, count=1, flags=re.S)
            if n != 1:
                raise VersionsError(f"{where}: no <h1> to give the month's page its own heading")
            b = sum(r.breaking for r in shown)
            span = shown[0].version if len(shown) == 1 else f"{shown[-1].version} to {shown[0].version}"
            page_fm["title"] = f"{month_name(month)} releases · Glyph"
            page_fm["description"] = (f"What changed in each Glyph release of {month_name(month)}: "
                                      f"{span}, {count_label(len(shown), b).replace(' · ', ', ')}. "
                                      "Each one with its ship date and its notes.")
            if month == newest:
                page_fm["canonical"] = ORIGIN + "/versions/"
        # A page of releases last changed when its newest release shipped.
        page_fm["lastmod"] = max(r.day for r in shown)
        out.append((url, page_fm, page_body))
    return out


def feed() -> str:
    """An Atom feed of the newest releases, each entry dated the day it
    shipped (a release with no `date` gets the first of its month)."""
    releases = load()
    newest = releases[:FEED_SIZE]

    def stamp(day: str) -> str:
        return f"{day}T00:00:00Z"

    lines = [
        '<?xml version="1.0" encoding="utf-8"?>',
        '<feed xmlns="http://www.w3.org/2005/Atom">',
        "  <title>Glyph releases</title>",
        "  <subtitle>What changed in each release of Glyph, newest first.</subtitle>",
        f'  <link rel="alternate" type="text/html" href="{ORIGIN}/versions/" />',
        f'  <link rel="self" type="application/atom+xml" href="{ORIGIN}/versions/feed.xml" />',
        f"  <id>{ORIGIN}/versions/</id>",
        f"  <updated>{stamp(max(r.day for r in newest))}</updated>",
        "  <author><name>The Glyph project</name></author>",
    ]
    for r in newest:
        title = f"Glyph {r.version}" + (" (breaking)" if r.breaking else "")
        summary = html.unescape(re.sub(r"<[^>]+>", "", r.headline))
        lines += [
            "  <entry>",
            f"    <title>{html.escape(title, quote=False)}</title>",
            f'    <link rel="alternate" type="text/html" href="{ORIGIN}{r.permalink}" />',
            f"    <id>{ORIGIN}{r.permalink}</id>",
            f"    <updated>{stamp(r.day)}</updated>",
            f"    <summary>{html.escape(summary, quote=False)}</summary>",
            "  </entry>",
        ]
    lines.append("</feed>")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- quotes

BREAKING_COST_SHOWN = 6
_SENTENCE_END = re.compile(r"(?<=[.!?])\s+(?=(?:<[^>]+>)*[A-Z])")


def apps_sentence(r: Release) -> str | None:
    """The sentence in a release's notes that says what happened to the
    example apps, as written, or None when the notes say nothing about them."""
    for m in re.finditer(r"<p>(.*?)</p>", r.body, re.S):
        for sentence in _SENTENCE_END.split(m.group(1)):
            if "example apps" in re.sub(r"<[^>]+>", "", sentence):
                return " ".join(sentence.split())
    return None


def breaking_cost(releases: list[Release], loc: str = "en") -> str:
    """The newest Breaking releases with what each note says about the
    example apps. The quotes stay English on a translated page (the notes
    are dated records) and are marked so; the sentence for a note that
    says nothing is translated."""
    rows = []
    for r in [r for r in releases if r.breaking][:BREAKING_COST_SHOWN]:
        said = apps_sentence(r)
        if said:
            text = f"&ldquo;{said}&rdquo;" if loc == "en" else f'<span lang="en">&ldquo;{said}&rdquo;</span>'
        else:
            text = site_i18n.strings(loc)("versions.no_quote")
        rows.append(f'          <li><a href="{r.permalink}">{r.version}</a>: {text}</li>\n')
    return '        <ul class="cost-list">\n' + "".join(rows) + "        </ul>\n"


def expand_quotes(body: str, where: str, loc: str = "en") -> str:
    """Replace every `{{releases:...}}` placeholder in a page body, in the
    language of `loc`."""
    if "{{releases:" not in body:
        return body
    releases = load()
    blocks = {
        "count": lambda: str(len(releases)),
        "breaking": lambda: str(sum(r.breaking for r in releases)),
        "latest": lambda: releases[0].version,
        "breaking-cost": lambda: breaking_cost(releases, loc),
    }

    def sub(m: re.Match) -> str:
        name = m.group(1)
        if name not in blocks:
            raise VersionsError(f"{where}: unknown placeholder {{{{releases:{name}}}}}")
        return blocks[name]()
    return re.sub(r"\{\{releases:([a-z-]+)\}\}", sub, body)


def newest_source() -> pathlib.Path:
    """The source file of the newest release's notes."""
    return load()[0].path
