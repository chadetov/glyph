#!/usr/bin/env python3
"""Render the site from site/ into web/.

Every page's content lives once, in site/pages/<url-path>/index.html: a short
front-matter header followed by the page body, the markup inside the main
content area. The chrome every page shares (head, header and nav, sub-navs,
prev/next, footer, the theme toggle) lives in site/partials/ and site/site.json,
so a nav or footer change is one edit instead of thirty-nine.

web/ stays committed and is still what the Pages workflow deploys. This script
owns every .html file in web/ (one per page under site/pages/, and a page
whose source is gone is deleted), every stylesheet in site/ (site/style.css
for every page, and the per-page ones such as site/catches.css, which a page
links from its `head`; see stylesheet_bundle()), web/sitemap.xml (generated from the page list), and
every file under site/static/, copied to the same path under web/ byte for
byte (the preview image, the raster icons; scripts/site_images.py draws them).
Everything else in web/ (llms.txt, robots.txt, favicon.svg) is left alone,
except web/versions/feed.xml, which it also writes. scripts/check_site_built.py
fails when web/ and this script disagree.

    python3 scripts/build_site.py            # render into web/
    python3 scripts/build_site.py --out DIR  # render somewhere else

Front matter is `key: value` lines between two `---` lines. A value of `|`
takes the indented lines that follow it. Keys:

  title, description     required; copied into <title> and the meta tag
  canonical              optional; defaults to https://glyphlang.io<url>
  layout                 home | article | wide | index
  article_class          extra class on the article (layout: article)
  nav                    key in a site.json nav entry's `match` list; that entry
                         is highlighted as the current section
  subnav                 name of a sub-nav, site/partials/subnav-<name>.html
  prev, next             "href | direction label | title" for the prev/next pair
  next_steps             the "where to go next" block at the end of the page
                         (|-block): a heading line, then the primary step as
                         "label | href | icon | title | description | cta",
                         then each other step as "href | icon | title | description"
  head                   extra markup for <head> (|-block)

The page body is copied byte for byte, so <pre> blocks reach web/ exactly as
written; the doc gates compile them from there. Two substitutions happen in a
body. {{version}} is the workspace version from glyph-compiler/Cargo.toml, so
the home page's release pill never needs a hand edit. The header partial
renders the same version as a badge beside the logo on every page, so a
version bump changes every page: scripts/bump_version.py re-renders web/ after
it moves Cargo.toml, which keeps web/ matching what this script renders.
Every `<h2 class="block-label">` section heading gets an id and a "#" link
to itself. {{releases:...}} quotes the release notes in site/releases/ through
scripts/site_versions.py (counts, and the newest Breaking releases with what
each note says about the example apps). {{catches:...}} is markup generated from the committed cases in catches/ by
scripts/site_catches.py, so the /catches/ page and every page that quotes it
show what scripts/check_catches.py verified, without running a compiler here.
A side-by-side pair of code panes (`<div class="two-up">`, or a case's
`<div class="c-pair">`) whose longest code line would not fit half the
column at the narrowest width it is shown side by side gets the class
`stack`, and site/style.css stacks it rather than letting both halves
scroll. See stack_wide_pairs(). A bare <pre> that holds tool output (a
diagnostic, an error, a run's output, or a block captioned with a `$` prompt)
gets the class `wraps`, so a phone wraps it; see mark_wrapping_blocks().
{{icon:NAME}} is the line icon NAME from site/site.json `icons`, the set the
menu draws from.

The release notes are the one source page that renders as several.
site/pages/versions/index.html carries {{versions:...}} placeholders, and
scripts/site_versions.py fills them from site/releases/<version>.html, one
file per release: /versions/ shows the newest month, /versions/YYYY-MM/ each
month, and web/versions/feed.xml is the Atom feed. Writing a release's notes
is adding its file under site/releases/ and running this script.

sitemap.xml lists every page that is its own canonical (a page whose
canonical points at another URL, such as the newest month of releases, which
duplicates /versions/, is left out), with a <lastmod> the generator works out
instead of anyone writing it. site/lastmod.json, committed, maps each page's
URL to a hash of its content and the date that content last changed. The
hash covers the title, the description and the rendered body (generated
blocks included), but not the chrome every page shares and not the version
number, so a nav edit or a version bump does not re-date every page. When a
page's hash differs from the stored one, its date becomes the build date:
the change being dated is the one in the working tree, which is committed
the same day in practice, and the build needs no git history (CI checks out
one commit). SITE_BUILD_DATE=YYYY-MM-DD overrides the build date. Release
pages are not in the file: each is dated by its newest release. The file
started on 2026-11-19 with every page at that date, which git history
supports: every page's body was rewritten that day when the site moved to
this generator. scripts/check_site_built.py fails when site/lastmod.json is not
what a build would write, which keeps the dates from drifting.

Some pages also get a Markdown copy, web/<path>/index.md, linked from the
page and listed in web/llms-pages.txt; see scripts/site_markdown.py.

site/pages/404.html renders to web/404.html, which GitHub Pages serves for
any address with no file: the normal header and footer, links to the main
sections, a robots noindex tag instead of a canonical, and no sitemap entry.

Translations live in site/i18n/<locale>/ and render to web/<locale>/:
scripts/site_i18n.py holds the locales, the UI strings the partials use as
{{t:key}}, the language menu and the hreflang links, and
scripts/check_i18n.py checks them. Every page, English or not, is rendered
by render() with a site_i18n.Ctx saying which language it is in.

Python standard library only.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import html
import json
import os
import pathlib
import re
import sys

import site_catches
import site_highlight
import site_i18n
import site_jsonld
import site_markdown
import site_versions

ROOT = pathlib.Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
PAGES = SITE / "pages"
PARTIALS = SITE / "partials"
STATIC = SITE / "static"
LASTMOD = SITE / "lastmod.json"
ORIGIN = "https://glyphlang.io"
EDIT_BASE = "https://github.com/chadetov/glyph/blob/main/site/pages/"

LAYOUTS = {"home", "article", "wide", "index"}
KEYS = {"title", "description", "canonical", "layout", "article_class", "nav",
        "subnav", "prev", "next", "head", "next_steps"}


class SiteError(Exception):
    pass


def workspace_version() -> str:
    text = (ROOT / "glyph-compiler" / "Cargo.toml").read_text()
    m = re.search(r"\[workspace\.package\][^\[]*?version\s*=\s*\"([^\"]+)\"", text, re.S)
    if not m:
        raise SiteError("could not read [workspace.package] version from glyph-compiler/Cargo.toml")
    return m.group(1)


def parse_front(path: pathlib.Path, keys: set[str] = KEYS) -> tuple[dict, str]:
    """A source file's front matter and body, allowing only `keys`."""
    text = path.read_text()
    if not text.startswith("---\n"):
        raise SiteError(f"{path}: no front matter (the file must start with '---')")
    try:
        header, body = text[4:].split("\n---\n", 1)
    except ValueError:
        raise SiteError(f"{path}: front matter is not closed with '---'") from None
    fm: dict[str, str] = {}
    lines = header.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        m = re.match(r"([a-z_]+):(?: (.*))?$", line)
        if not m:
            raise SiteError(f"{path}: front matter line {i + 2} is not 'key: value': {line!r}")
        key, value = m.group(1), (m.group(2) or "")
        if key not in keys:
            raise SiteError(f"{path}: unknown front-matter key {key!r}")
        if key in fm:
            raise SiteError(f"{path}: front-matter key {key!r} given twice")
        if value == "|":
            block = []
            i += 1
            while i < len(lines) and (lines[i].startswith("  ") or not lines[i].strip()):
                block.append(lines[i][2:])
                i += 1
            fm[key] = "\n".join(block).rstrip("\n")
            continue
        fm[key] = value
        i += 1
    return fm, body


def parse_page(path: pathlib.Path) -> tuple[dict, str]:
    fm, body = parse_front(path)
    for key in ("title", "description", "layout"):
        if not fm.get(key):
            raise SiteError(f"{path}: front matter needs {key!r}")
    if fm["layout"] not in LAYOUTS:
        raise SiteError(f"{path}: layout {fm['layout']!r} is not one of {sorted(LAYOUTS)}")
    return fm, body


def partial(name: str) -> str:
    return (PARTIALS / name).read_text()


def fill(template: str, values: dict[str, str], where: str) -> str:
    def sub(m: re.Match) -> str:
        key = m.group(1)
        if key not in values:
            raise SiteError(f"{where}: no value for {{{{{key}}}}}")
        return values[key]
    return re.sub(r"\{\{([a-z_]+)\}\}", sub, template)


def answers_count() -> int:
    """The number of answer pages, each one a directory under site/pages/answers/."""
    return sum(1 for d in (PAGES / "answers").iterdir() if (d / "index.html").is_file())


def nav_keys(site: dict) -> set[str]:
    return {k for entry in site["nav"] for k in entry["match"]}


def icon(site: dict, name: str, where: str) -> str:
    """A 24x24 stroked line icon from site.json `icons`, drawn in currentColor."""
    if name not in site["icons"]:
        raise SiteError(f"{where}: no icon {name!r} in site.json icons")
    return ('<svg class="ico" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" '
            'stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">'
            f'{site["icons"][name]}</svg>')


def nav_item(site: dict, it: dict, url: str, where: str, s: site_i18n.Strings) -> str:
    cur = ' aria-current="page"' if it["href"] == url else ""
    count = ""
    if it.get("count") == "answers":
        count = f' <span class="nav-count">{answers_count()}</span>'
    elif it.get("count"):
        raise SiteError(f"site.json: unknown nav count {it['count']!r}")
    return (f'<li><a href="{it["href"]}"{cur}><span class="nav-ico">{icon(site, it["icon"], where)}</span>'
            f'<span class="nav-text"><span class="nav-label">{s.nav(it["label"])}{count}</span>'
            f'<span class="nav-desc">{s.nav(it["desc"])}</span></span></a></li>')


def nav_links(site: dict, active: str | None, url: str, where: str, s: site_i18n.Strings) -> str:
    """The main nav: a <details> dropdown per menu in site.json, its items in
    titled columns with an optional featured card, and a plain link per link
    entry. The entry whose `match` holds the page's `nav:` key is highlighted,
    and the link to the page itself carries aria-current."""
    if active is not None and active not in nav_keys(site):
        raise SiteError(f"{where}: nav key {active!r} is not in any site.json nav entry's match list")
    out = []
    for entry in site["nav"]:
        on = " active" if active in entry["match"] else ""
        if "menu" not in entry:
            cur = ' aria-current="page"' if entry["href"] == url else ""
            out.append(f'            <li class="nav-li-link"><a class="nav-top{on}" href="{entry["href"]}"{cur}>'
                       f'{s.nav(entry["label"])}</a></li>')
            continue
        pid = f'nav-{entry["menu"]}'
        cols = []
        for col in entry["columns"]:
            items = "\n".join(f"                      {nav_item(site, it, url, where, s)}" for it in col["items"])
            cols.append(f'                  <div class="nav-col">\n'
                        f'                    <p class="nav-col-title">{s.nav(col["title"])}</p>\n'
                        f'                    <ul>\n{items}\n                    </ul>\n'
                        f'                  </div>')
        feat = entry.get("featured")
        if feat:
            try:
                big = site_catches.expand(s.nav(feat["big"]), "site.json")
                text = site_catches.expand(s.nav(feat["text"]), "site.json")
            except site_catches.CatchesError as e:
                raise SiteError(str(e)) from None
            cols.append(f'                  <a class="nav-feature" href="{feat["href"]}">'
                        f'<span class="nav-feature-big">{big}</span>'
                        f'<span class="nav-feature-text">{text}</span>'
                        f'<span class="nav-feature-cta">{s.nav(feat["cta"])} {icon(site, "arrow", where)}</span></a>')
        n = len(entry["columns"]) + (1 if feat else 0)
        out.append(f'            <li class="nav-li-menu">\n'
                   f'              <details class="nav-menu">\n'
                   f'                <summary class="nav-top{on}" aria-controls="{pid}">{s.nav(entry["label"])}'
                   f'<span class="nav-chev" aria-hidden="true"></span></summary>\n'
                   f'                <div class="nav-drop cols-{n}{" has-feature" if feat else ""}" id="{pid}">\n' + "\n".join(cols) + "\n"
                   f'                </div>\n'
                   f'              </details>\n'
                   f'            </li>')
    return "\n".join(out)


def subnav_entries(key: str, where: str) -> list[tuple[str | None, str, str]]:
    """The entries of site/partials/subnav-<key>.html as (group, href, label
    markup): one <a href> link per line, under the most recent "# Group" line."""
    path = PARTIALS / f"subnav-{key}.html"
    if not path.is_file():
        raise SiteError(f"{where}: no sub-nav {key!r} (expected {path.relative_to(ROOT)})")
    out, group = [], None
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("# "):
            group = line[2:]
            continue
        m = re.match(r'<a href="([^"]+)">(.*)</a>$', line)
        if not m:
            raise SiteError(f"{path.relative_to(ROOT)}: not a single <a href> link per line: {line!r}")
        out.append((group, m.group(1), m.group(2)))
    return out


def subnav(key: str, url: str, where: str, s: site_i18n.Strings) -> str:
    """A numbered sub-nav row from site/partials/subnav-<key>.html, with the
    current page's link marked active."""
    links, found = [], False
    for _, href, label in subnav_entries(key, where):
        attrs = ""
        if href == url:
            attrs, found = ' class="active" aria-current="page"', True
        links.append(f'        <a href="{href}"{attrs}>{label}</a>')
    if not found:
        raise SiteError(f"{where}: sub-nav {key!r} has no entry for {url}")
    return (f'    <nav class="subnav-wrap" aria-label="{s("subnav.aria")}">\n'
            '      <div class="subnav">\n' + "\n".join(links) + "\n      </div>\n    </nav>\n")


def answers_nav(url: str, where: str, s: site_i18n.Strings) -> tuple[str, str]:
    """The answers navigation as (bar, side). The side list holds every
    answer under its group, the current group open; wide screens show it as a
    sticky sidebar beside the article. Narrower screens show the bar instead:
    where you are, the previous and next answer, and a button that opens the
    same list as a panel."""
    entries = subnav_entries("answers", where)
    hrefs = [href for _, href, _ in entries]
    if url not in hrefs:
        raise SiteError(f"{where}: sub-nav 'answers' has no entry for {url}")
    i = hrefs.index(url)
    cur_group = entries[i][0]

    def num(label: str) -> str:
        m = re.match(r'<span class="n">([^<]*)</span>', label)
        return m.group(1) if m else ""

    def text(label: str) -> str:
        return re.sub(r'<span class="n">[^<]*</span>\s*', "", label)

    groups: list[tuple[str | None, list]] = []
    for g, href, label in entries:
        if not groups or groups[-1][0] != g:
            groups.append((g, []))
        groups[-1][1].append((href, label))
    side = [f'      <nav class="ans-side" id="ans-nav" aria-label="{s("answers.label")}">',
            '        <div class="subnav ans-list">',
            f'          <p class="ans-side-head"><a href="/answers/">{s("answers.all")}</a>'
            f'<button class="ans-close" type="button" aria-label="{s("answers.close")}">'
            '<span aria-hidden="true">&times;</span></button></p>']
    for g, items in groups:
        open_ = " open" if g == cur_group else ""
        side.append(f'          <details class="ans-grp"{open_}><summary>{g}</summary><ol>')
        for href, label in items:
            cur = ' aria-current="page"' if href == url else ""
            side.append(f'            <li><a href="{href}"{cur}><span class="n">{num(label)}</span>'
                        f'<span class="t">{text(label)}</span></a></li>')
        side.append("          </ol></details>")
    side += ["        </div>", "      </nav>"]

    def step(j: int, cls: str, arrow_before: bool) -> str:
        if j < 0 or j >= len(entries):
            return f'<span class="ans-step {cls} off" aria-hidden="true"></span>'
        href, label = entries[j][1], entries[j][2]
        word = s("answers.prev" if arrow_before else "answers.next", answer=f"{num(label)} {text(label)}")
        body = f"&lsaquo; {num(label)}" if arrow_before else f"{num(label)} &rsaquo;"
        return (f'<a class="ans-step {cls}" href="{href}" '
                f'aria-label="{word}">{body}</a>')

    bar = (f'    <nav class="ans-bar" aria-label="{s("answers.bar")}">\n'
           '      <div class="ans-bar-in">\n'
           f'        <p class="ans-crumb"><a href="/answers/">{s("answers.label")}</a><span class="sep" aria-hidden="true">/</span>'
           f'<a class="cur" href="{url}" aria-current="page"><span class="n">{num(entries[i][2])}</span> {text(entries[i][2])}</a></p>\n'
           f'        {step(i - 1, "prev", True)}{step(i + 1, "next", False)}\n'
           f'        <a class="ans-all-link" href="/answers/">{s("answers.all")}</a>\n'
           '        <button class="ans-all" type="button" aria-expanded="false" aria-controls="ans-nav">'
           f'{s("answers.all")}<span class="nav-chev" aria-hidden="true"></span></button>\n'
           '      </div>\n'
           '    </nav>\n')
    return bar, "\n".join(side) + "\n"


def prevnext(fm: dict, where: str, s: site_i18n.Strings) -> str:
    if "prev" not in fm and "next" not in fm:
        return ""

    def link(value: str, cls: str) -> str:
        parts = [p.strip() for p in value.split(" | ")]
        if len(parts) != 3:
            raise SiteError(f"{where}: prev/next must be 'href | label | title', got {value!r}")
        href, direction, title = parts
        return (f'        <a href="{href}"{cls}>\n'
                f'          <span class="dir">{direction}</span>\n'
                f'          <span class="t">{title}</span>\n'
                f"        </a>\n")

    out = f'      <nav class="prevnext" aria-label="{s("prevnext.aria")}">\n'
    out += link(fm["prev"], "") if "prev" in fm else '        <div class="spacer"></div>\n'
    if "next" in fm:
        out += link(fm["next"], ' class="nx cta"')
    return out + "      </nav>\n"


def slug(text: str) -> str:
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"&[a-z]+;|&#\d+;", " ", text).lower()
    s = re.sub(r"[^a-z0-9]+", "-", text).strip("-")
    if len(s) > 60:
        s = s[:61].rsplit("-", 1)[0]
    return s or "section"


def section_anchors(body: str, s: site_i18n.Strings) -> str:
    """Give every `<h2 class="block-label">` section heading an id (kept when
    the source sets one, else a slug of its text, unique on the page) and a
    "#" link to itself, so any section can be linked to."""
    taken = set(re.findall(r'\bid="([^"]+)"', body))

    def sub(m: re.Match) -> str:
        attrs, inner = m.group(1), m.group(2)
        if 'id="' in attrs:
            hid = re.search(r'id="([^"]+)"', attrs).group(1)
        else:
            base = hid = slug(re.sub(r'<span class="ex-label">.*?</span>', "", inner))
            n = 2
            while hid in taken:
                hid, n = f"{base}-{n}", n + 1
            taken.add(hid)
            attrs += f' id="{hid}"'
        return (f'<h2 class="block-label"{attrs}>{inner}'
                f'<a class="h-anchor" href="#{hid}" aria-label="{s("anchor.aria")}">#</a></h2>')
    return re.sub(r'<h2 class="block-label"([^>]*)>(.*?)</h2>', sub, body)


def secondary_spans(n: int) -> list[int]:
    """Column spans, in a six-column grid, that fill every row: rows of
    three, with the remainder first, as a row of two wider cards or one full
    one, so the grid never ends on a short row."""
    if n == 1:
        return [6]
    if n == 2:
        return [3, 3]
    if n == 4:
        return [3] * 4
    lead = [6] if n % 3 == 1 else [3, 3] if n % 3 == 2 else []
    return lead + [2] * (n - len(lead))


def next_steps(site: dict, value: str, where: str) -> str:
    """The shared next-steps block from a page's `next_steps` front matter:
    one dominant primary card, then the other steps in rows that fill evenly."""
    lines = [ln.strip() for ln in value.split("\n") if ln.strip()]
    if len(lines) < 2:
        raise SiteError(f"{where}: next_steps needs a heading line and a primary step")
    heading, primary, rest = lines[0], lines[1], lines[2:]
    parts = [s.strip() for s in primary.split(" | ")]
    if len(parts) != 6:
        raise SiteError(f"{where}: next_steps primary must be 'label | href | icon | title | description | cta'")
    label, href, ico, title, desc, cta = parts
    arrow = icon(site, "arrow", where)
    out = ['      <section class="next-steps">',
           f'        <h2 class="block-label">{heading}</h2>',
           f'        <a class="ns-primary" href="{href}">',
           f'          <span class="ns-body"><span class="ns-eyebrow">{label}</span>'
           f'<span class="ns-title">{title}</span><span class="ns-desc">{desc}</span>'
           f'<span class="ns-cta">{cta} {arrow}</span></span>',
           f'          <span class="ns-art">{icon(site, ico, where)}</span>',
           '        </a>']
    if rest:
        out.append('        <div class="ns-grid">')
        for line, span in zip(rest, secondary_spans(len(rest))):
            f = [s.strip() for s in line.split(" | ")]
            if len(f) != 4:
                raise SiteError(f"{where}: a next_steps step must be 'href | icon | title | description', got {line!r}")
            href, ico, title, desc = f
            out.append(f'          <a class="ns-card span-{span}" href="{href}"><span class="ns-ico">{icon(site, ico, where)}</span>'
                       f'<span class="ns-card-title">{title}</span><span class="ns-card-desc">{desc}</span>'
                       f'<span class="ns-go">{arrow}</span></a>')
        out.append('        </div>')
    out.append('      </section>')
    return "\n".join(out) + "\n"


# The longest code line, in characters, that fits one half of a side-by-side
# pair at the narrowest width the pair is shown side by side. Code is IBM Plex
# Mono at 13px, 7.8px a character, inside 16px of padding a side and a 1px
# border. A .two-up sits in the 760px article column beside the answers
# sidebar, and in at least 773px below 821px where it stacks anyway: halves
# of 372px hold 43 characters. A .c-pair sits in the wide column and stacks
# below 861px: halves of 398px hold 46.
PAIR_FIT = {"two-up": 43, "c-pair": 46}


def _div_end(body: str, start: int) -> int:
    """Index just past the </div> that closes the <div> opening at `start`."""
    depth = 0
    for m in re.compile(r"<(/?)div\b[^>]*>").finditer(body, start):
        depth += -1 if m.group(1) else 1
        if depth == 0:
            return m.end()
    raise SiteError("unbalanced <div> in a side-by-side pair")


def longest_code_line(fragment: str) -> int:
    """The longest line, in characters, of the <pre> blocks in `fragment`
    that keep their lines (an output block marked c-out wraps instead)."""
    longest = 0
    for cls, code in re.findall(r"<pre(\s[^>]*)?>(.*?)</pre>", fragment, re.S):
        if "c-out" in (cls or ""):
            continue
        text = html.unescape(re.sub(r"<[^>]+>", "", code))
        longest = max([longest, *(len(line) for line in text.split("\n"))])
    return longest


def stack_wide_pairs(body: str) -> str:
    """Add `stack` to each side-by-side pair whose code would scroll."""
    out, pos = [], 0
    pattern = re.compile(r'<div class="(two-up|c-pair)((?: [a-z-]+)*)">')
    for m in pattern.finditer(body):
        if m.start() < pos:
            continue
        kind, extra = m.group(1), m.group(2).split()
        end = _div_end(body, m.start())
        tag = m.group(0)
        if "fit" not in extra and "stack" not in extra and longest_code_line(body[m.end():end]) > PAIR_FIT[kind]:
            tag = f'<div class="{kind}{m.group(2)} stack">'
        out += [body[pos:m.start()], tag]
        pos = m.end()
    return "".join(out) + body[pos:]


# What a tool printed rather than what someone wrote: a diagnostic, an error, a
# run's output, or a block captioned with a shell prompt. On a phone these wrap
# (site/style.css, `pre.wraps`), because a reader needs the end of the message
# and the line breaks carry no meaning. Source code keeps its lines and scrolls,
# even where only a comment runs long: a wrapped comment lands at column 0 and
# reads as code.
OUTPUT_FIRST_LINE = re.compile(
    r"^\s*(?:\[[EW]\d{4}\]|(?:\w*Error|error|warning|Warning|rejected):"
    r"|(?:FAILS?|PASS|ok)\b|\d+ (?:errors?|warnings?|match sites|tests?)\b"
    r"|\$ |\d*\s*│|\S+:\d+:\d+ - error|glyph — )")
OUTPUT_CAPTION = re.compile(r"^\$")
CAPTION_BEFORE = re.compile(r'<span class="cap">((?:(?!</span>).)*)</span>\s*$', re.S)


def wraps_on_phone(caption: str, code: str) -> bool:
    text = html.unescape(re.sub(r"<[^>]+>", "", code))
    first = next((line for line in text.split("\n") if line.strip()), "")
    return bool(OUTPUT_CAPTION.match(caption) or OUTPUT_FIRST_LINE.match(first))


def mark_wrapping_blocks(body: str) -> str:
    """Add class `wraps` to each bare <pre> that a phone should wrap. The
    block's text is untouched; only the opening tag changes."""
    out, pos = [], 0
    for m in re.finditer(r"<pre>(.*?)</pre>", body, re.S):
        cap = CAPTION_BEFORE.search(body[max(0, m.start() - 600):m.start()])
        caption = html.unescape(re.sub(r"<[^>]+>", "", cap.group(1))).strip() if cap else ""
        if wraps_on_phone(caption, m.group(1)):
            out += [body[pos:m.start()], '<pre class="wraps">']
            pos = m.start() + len("<pre>")
    return "".join(out) + body[pos:]


def attr(text: str) -> str:
    """Front-matter text (which may hold entities) as an attribute value."""
    return html.escape(html.unescape(text), quote=True)


def short_title(title: str) -> str:
    """A page title without the " · Glyph" every <title> ends with: the
    preview names the site separately (og:site_name)."""
    return re.sub(r"\s*·\s*Glyph$", "", title)


def page_url(path: pathlib.Path) -> str:
    rel = path.relative_to(PAGES).parent.as_posix()
    return "/" if rel == "." else f"/{rel}/"


class LastMod:
    """The date each page's content last changed, kept in site/lastmod.json
    as {url: {"date": YYYY-MM-DD, "hash": 16 hex digits}}."""

    def __init__(self, stored: dict[str, dict], today: str):
        self.stored, self.today = stored, today
        self.now: dict[str, dict] = {}

    @classmethod
    def load(cls) -> "LastMod":
        stored = json.loads(LASTMOD.read_text()) if LASTMOD.is_file() else {}
        today = os.environ.get("SITE_BUILD_DATE") or datetime.date.today().isoformat()
        if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", today):
            raise SiteError(f"SITE_BUILD_DATE {today!r} is not YYYY-MM-DD")
        return cls(stored, today)

    def date(self, url: str, content: str) -> str:
        digest = hashlib.sha256(content.encode()).hexdigest()[:16]
        old = self.stored.get(url)
        date = old["date"] if old and old.get("hash") == digest else self.today
        self.now[url] = {"date": date, "hash": digest}
        return date

    def text(self) -> str:
        lines = [f'  {json.dumps(u)}: {{"date": "{e["date"]}", "hash": "{e["hash"]}"}}'
                 for u, e in sorted(self.now.items())]
        return "{\n" + ",\n".join(lines) + "\n}\n"


# Every page loads one stylesheet: the fonts, site/style.css and
# site/i18n.css, then the sheets its `head` front matter links, in that order.
BASE_SHEETS = ["fonts.css", "style.css", "highlight.css", "i18n.css"]
SHEET_LINK = re.compile(r'<link rel="stylesheet" href="/([a-z0-9-]+\.css)" />')


def page_sheets(head: str, where: str) -> tuple[str, list[str]]:
    """A page's `head` front matter without its stylesheet links, and the
    site stylesheets those links named."""
    sheets, keep = [], []
    for line in head.split("\n"):
        m = SHEET_LINK.fullmatch(line.strip())
        if m:
            if not (SITE / m.group(1)).is_file():
                raise SiteError(f"{where}: head links /{m.group(1)}, and site/{m.group(1)} does not exist")
            sheets.append(m.group(1))
        else:
            keep.append(line)
    return "\n".join(keep).strip("\n"), sheets


# The bundles the pages rendered so far link: {path under web/: page sheets}.
BUNDLES: dict[str, list[str]] = {}


def bundle_name(sheets: list[str]) -> str:
    """css/site.css for the base sheets alone, else css/<page sheets>.css,
    and the bundle recorded for build() to write."""
    name = "css/" + ("_".join(s.removesuffix(".css") for s in sheets) or "site") + ".css"
    if BUNDLES.setdefault(name, sheets) != sheets:
        raise SiteError(f"two different sets of stylesheets would both be {name}")
    return name


def stylesheet_bundle(sheets: list[str]) -> str:
    """The fonts, the base sheets and a page's own sheets as one file, so a
    page waits on one stylesheet request rather than three or four."""
    parts = [f"/* site/{name} */\n{(SITE / name).read_text()}" for name in BASE_SHEETS + sheets]
    return "\n".join(parts)


def human_date(date: str) -> str:
    y, m, d = date.split("-")
    return f"{int(d)} {site_versions.MONTH_NAMES[int(m) - 1][:3]} {y}"


def stamp_answer(body: str, version: str, date: str) -> str:
    """The line that says how current an answer is, as the last item of its
    "Where it stands" row (or at the end, before the next steps, on a page
    without one). The code on every answer page is compiled by
    scripts/check_docs_compile.py on every build, so the version is the one
    the build checked it against; the date is the page's from
    site/lastmod.json, the last time its content changed."""
    when = f'<time datetime="{date}">{human_date(date)}</time>'
    text = (f"Code examples on this page are checked against Glyph {version} on every build. "
            f"Last changed {when}." if "<pre" in body else f"Last changed {when}.")
    line = f'        <p class="page-stamp">{text}</p>\n'
    m = re.search(r'<div class="status-row">', body)
    if m:
        end = _div_end(body, m.start())
        close = body.rindex("</div>", m.start(), end)
        return body[:close] + "  " + line.lstrip() + "      " + body[close:]
    i = body.find('      <section class="next-steps">')
    i = len(body) if i < 0 else i
    return body[:i] + line.replace("        ", "      ", 1) + body[i:]


def render(path: pathlib.Path, site: dict, version: str, fm: dict, body: str, url: str,
           ctx: site_i18n.Ctx, lastmod: LastMod, titles: dict[str, str]) -> tuple[str, str, str | None]:
    """One page, the date its content last changed, and its Markdown copy
    when it has one. `url` is the page's English URL; `ctx` says which
    language this rendering is in and which others the page has."""
    where = str(path.relative_to(ROOT))
    t = ctx.s
    body = re.sub(r"\{\{icon:([a-z-]+)\}\}", lambda m: icon(site, m.group(1), where), body)
    if fm.get("next_steps"):
        body += next_steps(site, fm["next_steps"], where)
    body = section_anchors(body, t)
    try:
        body = site_catches.expand(body, where)
    except site_catches.CatchesError as e:
        raise SiteError(str(e)) from None
    try:
        body = site_versions.expand_quotes(body, where)
    except site_versions.VersionsError as e:
        raise SiteError(str(e)) from None
    body = stack_wide_pairs(body)
    body = mark_wrapping_blocks(body)
    try:
        body = site_highlight.highlight_pres(body, where)
    except site_highlight.HighlightError as e:
        raise SiteError(str(e)) from None
    body = site_i18n.english_island(body, ctx)
    # The content as it stands before the version is filled in, so that a
    # version bump alone does not change any page's date.
    if fm.get("lastmod"):
        date = fm["lastmod"]
    elif fm.get("noindex"):
        date = ""
    else:
        date = lastmod.date(ctx.here, f'{fm["title"]}\n{fm["description"]}\n{body}')
    body = body.replace("{{version}}", version)
    if url.startswith("/answers/") and url != "/answers/" and not fm.get("noindex"):
        body = stamp_answer(body, version, date)
    markdown = None
    if ctx.loc == "en" and site_markdown.wanted(url) and not fm.get("noindex"):
        markdown = site_markdown.convert(body, url, short_title(fm["title"]), date)
    head_values = {
        "title": fm["title"],
        "description": fm["description"],
        "canonical_link": (f'    <link rel="canonical" href="{fm.get("canonical", ORIGIN + ctx.here)}" />\n'
                           if not fm.get("noindex") else '    <meta name="robots" content="noindex" />\n'),
        "og_url": fm.get("canonical", ORIGIN + ctx.here) if not fm.get("noindex") else ORIGIN + "/",
        "markdown_link": (f'    <link rel="alternate" type="text/markdown" href="{url}index.md" />\n'
                          if markdown is not None else ""),
        "og_title": attr(short_title(fm["title"])),
        "og_description": attr(fm["description"]),
        "og_locale": site_i18n.OG_LOCALES[ctx.loc],
        "og_locale_alternates": site_i18n.og_locale_alternates(ctx),
        "jsonld": "" if fm.get("noindex") else site_jsonld.graph(
            url=url, here=ctx.here, lang=ctx.lang, title=short_title(fm["title"]),
            description=fm["description"], date=date, version=version, titles=titles,
            languages=[site_i18n.LOCALES[loc][0] for loc in site_i18n.LOCALES],
            answers=[href for _, href, _ in subnav_entries("answers", where)]),
    }
    extra_head, sheets = page_sheets(fm.get("head", ""), where)
    head_values["stylesheet"] = "/" + bundle_name(sheets)
    head = fill(site_i18n.apply_strings(partial("head.html"), t), head_values, where)
    head += site_i18n.head_extra(ctx)
    if extra_head:
        head += "".join(f"    {line}\n" if line else "\n" for line in extra_head.split("\n"))
    actions = site["actions"]
    header = fill(site_i18n.apply_strings(partial("header.html"), t), {
        "nav_links": nav_links(site, fm.get("nav"), url, where, t),
        "version": version,
        "playground_href": actions["playground"]["href"],
        "playground_label": t.nav(actions["playground"]["label"]),
        "github_href": actions["github"]["href"],
        "github_label": t.nav(actions["github"]["label"]),
        "lang_switcher": site_i18n.switcher(ctx),
    }, where)
    edit = site_i18n.edit_url(ctx, EDIT_BASE + path.relative_to(ROOT).as_posix().removeprefix("site/pages/"))
    footer = fill(site_i18n.apply_strings(partial("footer.html"), t), {"edit_url": edit}, where)
    side = ""
    if fm.get("subnav") == "answers" and fm["layout"] == "article":
        sub, side = answers_nav(url, where, t)
        if "</h1>" not in body:
            raise SiteError(f"{where}: the answers sidebar goes after the page's <h1>, and there is none")
        body = body.replace("</h1>\n", "</h1>\n" + side, 1)
    else:
        sub = subnav(fm["subnav"], url, where, t) if fm.get("subnav") else ""

    layout = fm["layout"]
    if layout == "home":
        main = f'    <main id="main" class="home">\n{body}    </main>\n'
    elif layout == "article":
        cls = "answer-page" + (" has-side" if side else "") + (f" {fm['article_class']}" if fm.get("article_class") else "")
        main = (f'    <main id="main">\n    <article class="{cls}">\n{body}'
                f"{prevnext(fm, where, t)}    </article>\n    </main>\n")
    elif layout == "wide":
        main = f'    <main id="main">\n    <div class="wrap">\n{body}    </div>\n    </main>\n'
    else:  # index
        main = f'    <main id="main">\n    <div class="ans-index">\n{body}    </div>\n    </main>\n'
    if layout != "article" and ("prev" in fm or "next" in fm):
        raise SiteError(f"{where}: prev/next is only rendered by the article layout")

    page = site_i18n.localize_links(f"{header}{sub}{site_i18n.notices(ctx)}{main}{footer}", ctx)
    return date, markdown, ("<!doctype html>\n"
            f'<html lang="{ctx.lang}">\n'
            "  <head>\n"
            f"{head}"
            "  </head>\n"
            "  <body>\n"
            f"{page}"
            f"{site_i18n.CHOICE_SCRIPT}"
            "  </body>\n"
            "</html>\n")


def sitemap(entries: list[dict]) -> str:
    """The sitemap: <loc>, hreflang alternates and <lastmod>. No changefreq or
    priority, which search engines ignore."""
    out = ['<?xml version="1.0" encoding="UTF-8"?>',
           '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">']
    for e in entries:
        out += ["  <url>", f"    <loc>{ORIGIN}{e['loc']}</loc>"]
        out += e.get("alternates", [])
        if e.get("lastmod"):
            out.append(f"    <lastmod>{e['lastmod']}</lastmod>")
        out.append("  </url>")
    out.append("</urlset>")
    return "\n".join(out) + "\n"


def expand_pages(path: pathlib.Path) -> list[tuple[str, dict, str]]:
    """[(url, front matter, body)] for one source page. Almost every page is
    one page; the versions template becomes one page per month of releases."""
    fm, body = parse_page(path)
    if "{{versions:" not in body:
        return [(page_url(path), fm, body)]
    try:
        return site_versions.pages(fm, body, str(path.relative_to(ROOT)))
    except site_versions.VersionsError as e:
        raise SiteError(str(e)) from None


NOT_FOUND = PAGES / "404.html"


def page_files() -> list[pathlib.Path]:
    return sorted(PAGES.rglob("index.html"),
                  key=lambda p: (p.relative_to(PAGES).parent.as_posix() != ".", p.relative_to(PAGES).as_posix()))


# What the last build() worked out site/lastmod.json should hold. write()
# saves it; scripts/check_site_built.py compares it with the committed file.
LASTMOD_TEXT: str | None = None


def build() -> dict[str, str | bytes]:
    """Every file the generator owns, as {path relative to web/: contents}.
    Text files are str; the files copied from site/static/ are bytes. The
    dates for site/lastmod.json are left in LASTMOD_TEXT."""
    global LASTMOD_TEXT
    BUNDLES.clear()
    site = json.loads((SITE / "site.json").read_text())
    version = workspace_version()
    lastmod = LastMod.load()
    files: dict[str, str] = {}
    entries = []
    try:
        translations = site_i18n.discover(parse_front)
        site_i18n.strings("en")
    except site_i18n.I18nError as e:
        raise SiteError(str(e)) from None
    translated_urls: dict[str, set[str]] = {}
    for eng, by_loc in translations.items():
        for loc in by_loc:
            translated_urls.setdefault(loc, set()).add(page_url(eng))
    pages = [(path, expand_pages(path)) for path in page_files()]
    # Every English page's title, for breadcrumbs and the answers list.
    titles = {url: short_title(fm["title"]) for _, expanded in pages for url, fm, _ in expanded}
    md_pages: list[tuple[str, str, str]] = []
    for path, expanded in pages:
        for url, fm, body in expanded:
            by_loc = translations.get(path, {})
            available = ["en"] + [loc for loc in site_i18n.TRANSLATED if loc in by_loc]
            versions = [("en", fm, body, path, None)]
            for loc in available[1:]:
                tr = by_loc[loc]
                versions.append((loc, site_i18n.merge_front(fm, tr), tr.body, tr.path, tr))
            alternates = site_i18n.sitemap_alternates(url, available)
            for loc, vfm, vbody, src, tr in versions:
                try:
                    ctx = site_i18n.Ctx(loc, url, available, tr, translated_urls)
                    here = ctx.here
                    rel = here.strip("/") + "/index.html" if here != "/" else "index.html"
                    date, markdown, files[rel] = render(src, site, version, vfm, vbody, url, ctx, lastmod, titles)
                    if markdown is not None:
                        files[rel.removesuffix("index.html") + "index.md"] = markdown
                        md_pages.append((url, short_title(vfm["title"]), vfm["description"]))
                except site_i18n.I18nError as e:
                    raise SiteError(str(e)) from None
                # Only a page that is its own canonical belongs in the sitemap.
                if vfm.get("canonical", ORIGIN + here) == ORIGIN + here:
                    entries.append({"loc": here, "lastmod": date, "alternates": alternates})
    # GitHub Pages serves web/404.html for any address with no file. It has
    # the site's chrome, is noindex, and is not in the sitemap.
    fm, body = parse_page(NOT_FOUND)
    fm["noindex"] = "yes"
    try:
        _, _, files["404.html"] = render(NOT_FOUND, site, version, fm, body, "/404.html",
                                      site_i18n.Ctx("en", "/404.html", ["en"], None, translated_urls), lastmod,
                                      titles)
    except site_i18n.I18nError as e:
        raise SiteError(str(e)) from None
    try:
        files["versions/feed.xml"] = site_versions.feed()
    except site_versions.VersionsError as e:
        raise SiteError(str(e)) from None
    files["llms-pages.txt"] = site_markdown.listing(md_pages)
    entries += site.get("extra_sitemap", [])
    files["sitemap.xml"] = sitemap(entries)
    LASTMOD_TEXT = lastmod.text()
    # The bundles the pages link, one per combination of page sheets.
    for name, sheets in sorted(BUNDLES.items()):
        files[name] = stylesheet_bundle(sheets)
    # The single sheets too, for anything that still links one by name (a
    # page cached from before the bundles, for one). fonts.css lives only in
    # the bundles: its font URLs are absolute, but on its own it is useless.
    for css in sorted(SITE.glob("*.css")):
        if css.name not in BASE_SHEETS[:1]:
            files[css.name] = css.read_text()
    # Language flags for the header's language menu, with their licence.
    for flag in sorted((SITE / "flags").glob("*")):
        files[f"flags/{flag.name}"] = flag.read_text()
    for asset in sorted(p for p in STATIC.rglob("*") if p.is_file() and p.name != ".DS_Store"):
        files[asset.relative_to(STATIC).as_posix()] = asset.read_bytes()
    return files


def stale_pages(out: pathlib.Path, files: dict[str, str]) -> list[str]:
    """HTML pages and Markdown copies under `out` that no source in
    site/pages/ renders any more."""
    if not out.is_dir():
        return []
    return sorted(p.relative_to(out).as_posix() for p in [*out.rglob("*.html"), *out.rglob("index.md")]
                  if p.relative_to(out).as_posix() not in files)


def write(out: pathlib.Path, files: dict[str, str | bytes]) -> None:
    # Every HTML page in web/ is generated, so one with no source is a page
    # that was deleted or moved in site/; leaving it would keep serving it.
    for rel in stale_pages(out, files):
        (out / rel).unlink()
    if LASTMOD_TEXT is not None and out.resolve() == (ROOT / "web").resolve():
        if not LASTMOD.is_file() or LASTMOD.read_text() != LASTMOD_TEXT:
            LASTMOD.write_text(LASTMOD_TEXT)
    for rel, text in files.items():
        dst = out / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(text, bytes):
            if not dst.exists() or dst.read_bytes() != text:
                dst.write_bytes(text)
        elif not dst.exists() or dst.read_text() != text:
            dst.write_text(text)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", type=pathlib.Path, default=ROOT / "web",
                    help="directory to render into (default: web/)")
    args = ap.parse_args()
    try:
        files = build()
    except SiteError as e:
        print(f"build_site: {e}", file=sys.stderr)
        return 1
    write(args.out, files)
    pages = sum(1 for k in files if k.endswith(".html"))
    sheets = ", ".join(k for k in files if k.endswith(".css"))
    print(f"built {pages} pages, sitemap.xml and {sheets} into {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
