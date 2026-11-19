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
links from its `head`), and web/sitemap.xml (generated from the page list). Everything else in web/
(llms.txt, robots.txt, favicon.svg) is left alone. scripts/check_site_built.py fails when web/ and this script disagree.

    python3 scripts/build_site.py            # render into web/
    python3 scripts/build_site.py --out DIR  # render somewhere else

Front matter is `key: value` lines between two `---` lines. A value of `|`
takes the indented lines that follow it. Keys:

  title, description     required; copied into <title> and the meta tag
  canonical              optional; defaults to https://glyphlang.io<url>
  layout                 home | article | wide | index
  article_class          extra class on the article (layout: article)
  nav                    key of the main nav item to mark active
  subnav                 name of a sub-nav, site/partials/subnav-<name>.html
  prev, next             "href | direction label | title" for the prev/next pair
  sitemap                "lastmod changefreq priority" for sitemap.xml
  head                   extra markup for <head> (|-block)

The page body is copied byte for byte, so <pre> blocks reach web/ exactly as
written; the doc gates compile them from there. Two substitutions happen in a
body. {{version}} is the workspace version from glyph-compiler/Cargo.toml, so
the home page's release pill never needs a hand edit and the bump scripts,
which edit web/index.html, leave web/ matching what this script renders.
{{catches:...}} is markup generated from the committed cases in catches/ by
scripts/site_catches.py, so the /catches/ page and every page that quotes it
show what scripts/check_catches.py verified, without running a compiler here.

Python standard library only.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

import site_catches

ROOT = pathlib.Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
PAGES = SITE / "pages"
PARTIALS = SITE / "partials"
ORIGIN = "https://glyphlang.io"
EDIT_BASE = "https://github.com/chadetov/glyph/blob/main/site/pages/"

LAYOUTS = {"home", "article", "wide", "index"}
KEYS = {"title", "description", "canonical", "layout", "article_class", "nav",
        "subnav", "prev", "next", "sitemap", "head"}


class SiteError(Exception):
    pass


def workspace_version() -> str:
    text = (ROOT / "glyph-compiler" / "Cargo.toml").read_text()
    m = re.search(r"\[workspace\.package\][^\[]*?version\s*=\s*\"([^\"]+)\"", text, re.S)
    if not m:
        raise SiteError("could not read [workspace.package] version from glyph-compiler/Cargo.toml")
    return m.group(1)


def parse_page(path: pathlib.Path) -> tuple[dict, str]:
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
        if key not in KEYS:
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


def nav_links(site: dict, active: str | None) -> str:
    out = []
    for item in site["nav"]:
        attrs = ' class="active" aria-current="page"' if item["key"] == active else ""
        out.append(f'          <a href="{item["href"]}"{attrs}>{item["label"]}</a>')
    return "\n".join(out)


def subnav(key: str, url: str, where: str) -> str:
    """A numbered sub-nav from site/partials/subnav-<key>.html, one link per
    line, with the current page's link marked active."""
    path = PARTIALS / f"subnav-{key}.html"
    if not path.is_file():
        raise SiteError(f"{where}: no sub-nav {key!r} (expected {path.relative_to(ROOT)})")
    links, found = [], False
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        m = re.match(r'<a href="([^"]+)">(.*)</a>$', line.strip())
        if not m:
            raise SiteError(f"{path.relative_to(ROOT)}: not a single <a href> link per line: {line!r}")
        attrs = ""
        if m.group(1) == url:
            attrs, found = ' class="active" aria-current="page"', True
        links.append(f'        <a href="{m.group(1)}"{attrs}>{m.group(2)}</a>')
    if not found:
        raise SiteError(f"{where}: sub-nav {key!r} has no entry for {url}")
    return ('    <nav class="subnav-wrap" aria-label="Sections">\n'
            '      <div class="subnav">\n' + "\n".join(links) + "\n      </div>\n    </nav>\n")


def prevnext(fm: dict, where: str) -> str:
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

    out = '      <nav class="prevnext" aria-label="Previous and next">\n'
    out += link(fm["prev"], "") if "prev" in fm else '        <div class="spacer"></div>\n'
    if "next" in fm:
        out += link(fm["next"], ' class="nx cta"')
    return out + "      </nav>\n"


def page_url(path: pathlib.Path) -> str:
    rel = path.relative_to(PAGES).parent.as_posix()
    return "/" if rel == "." else f"/{rel}/"


def render(path: pathlib.Path, site: dict, version: str) -> str:
    where = str(path.relative_to(ROOT))
    fm, body = parse_page(path)
    url = page_url(path)
    body = body.replace("{{version}}", version)
    try:
        body = site_catches.expand(body, where)
    except site_catches.CatchesError as e:
        raise SiteError(str(e)) from None
    head_values = {
        "title": fm["title"],
        "description": fm["description"],
        "canonical": fm.get("canonical", ORIGIN + url),
    }
    head = fill(partial("head.html"), head_values, where)
    if fm.get("head"):
        head += "".join(f"    {line}\n" if line else "\n" for line in fm["head"].split("\n"))
    header = fill(partial("header.html"), {"nav_links": nav_links(site, fm.get("nav"))}, where)
    edit = EDIT_BASE + path.relative_to(PAGES).as_posix()
    footer = fill(partial("footer.html"), {"edit_url": edit}, where)
    sub = subnav(fm["subnav"], url, where) if fm.get("subnav") else ""

    layout = fm["layout"]
    if layout == "home":
        main = f'    <main id="main" class="home">\n{body}    </main>\n'
    elif layout == "article":
        cls = "answer-page" + (f" {fm['article_class']}" if fm.get("article_class") else "")
        main = (f'    <main id="main">\n    <article class="{cls}">\n{body}'
                f"{prevnext(fm, where)}    </article>\n    </main>\n")
    elif layout == "wide":
        main = f'    <main id="main">\n    <div class="wrap">\n{body}    </div>\n    </main>\n'
    else:  # index
        main = f'    <main id="main">\n    <div class="ans-index">\n{body}    </div>\n    </main>\n'
    if layout != "article" and ("prev" in fm or "next" in fm):
        raise SiteError(f"{where}: prev/next is only rendered by the article layout")

    return ("<!doctype html>\n"
            '<html lang="en">\n'
            "  <head>\n"
            f"{head}"
            "  </head>\n"
            "  <body>\n"
            f"{header}"
            f"{sub}"
            f"{main}"
            f"{footer}"
            "  </body>\n"
            "</html>\n")


def sitemap(entries: list[dict]) -> str:
    out = ['<?xml version="1.0" encoding="UTF-8"?>',
           '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
    for e in entries:
        out += ["  <url>", f"    <loc>{ORIGIN}{e['loc']}</loc>"]
        if e.get("lastmod"):
            out.append(f"    <lastmod>{e['lastmod']}</lastmod>")
        if e.get("changefreq"):
            out.append(f"    <changefreq>{e['changefreq']}</changefreq>")
        if e.get("priority"):
            out.append(f"    <priority>{e['priority']}</priority>")
        out.append("  </url>")
    out.append("</urlset>")
    return "\n".join(out) + "\n"


def page_files() -> list[pathlib.Path]:
    return sorted(PAGES.rglob("index.html"),
                  key=lambda p: (p.relative_to(PAGES).parent.as_posix() != ".", p.relative_to(PAGES).as_posix()))


def build() -> dict[str, str]:
    """Every file the generator owns, as {path relative to web/: contents}."""
    site = json.loads((SITE / "site.json").read_text())
    version = workspace_version()
    files: dict[str, str] = {}
    entries = []
    for path in page_files():
        rel = path.relative_to(PAGES).as_posix()
        files[rel] = render(path, site, version)
        fm, _ = parse_page(path)
        url = page_url(path)
        lastmod = changefreq = priority = None
        if fm.get("sitemap"):
            parts = fm["sitemap"].split()
            if len(parts) != 3:
                raise SiteError(f"{rel}: sitemap must be 'lastmod changefreq priority'")
            lastmod, changefreq, priority = parts
        entries.append({"loc": url, "lastmod": lastmod, "changefreq": changefreq, "priority": priority})
    entries += site.get("extra_sitemap", [])
    files["sitemap.xml"] = sitemap(entries)
    for css in sorted(SITE.glob("*.css")):
        files[css.name] = css.read_text()
    return files


def stale_pages(out: pathlib.Path, files: dict[str, str]) -> list[str]:
    """HTML pages under `out` that no source in site/pages/ renders any more."""
    if not out.is_dir():
        return []
    return sorted(p.relative_to(out).as_posix() for p in out.rglob("*.html")
                  if p.relative_to(out).as_posix() not in files)


def write(out: pathlib.Path, files: dict[str, str]) -> None:
    # Every HTML page in web/ is generated, so one with no source is a page
    # that was deleted or moved in site/; leaving it would keep serving it.
    for rel in stale_pages(out, files):
        (out / rel).unlink()
    for rel, text in files.items():
        dst = out / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        if not dst.exists() or dst.read_text() != text:
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
