#!/usr/bin/env python3
"""Site consistency checks for web/ (run in CI and locally).

Fails (exit 1) on:
  - an internal link that resolves to no file (href/src starting with "/")
  - an HTML file that isn't well-formed (unbalanced tags)
  - an answers sub-nav that is missing an existing answer page, or points at one
    that doesn't exist
  - a self-canonical, indexable page that sitemap.xml does not list, a
    sitemap entry for a page that is not one (or no longer exists), or a
    canonical pointing at a page the sitemap does not list

External links (http/https/mailto) and in-page fragments (#...) are not checked.
Run from the repo root: python3 scripts/check_site.py
"""
import os
import re
import sys
from html.parser import HTMLParser

WEB = "web"
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link",
        "meta", "param", "source", "track", "wbr"}
errors = []


class Balance(HTMLParser):
    def __init__(self):
        super().__init__()
        self.stack = []
        self.bad = []

    def handle_starttag(self, tag, attrs):
        if tag not in VOID:
            self.stack.append(tag)

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        if self.stack and self.stack[-1] == tag:
            self.stack.pop()
        else:
            self.bad.append(tag)


def html_files():
    for root, _dirs, files in os.walk(WEB):
        for f in files:
            if f.endswith(".html"):
                yield os.path.join(root, f)


def resolves(link):
    """Does an internal absolute link ("/foo/", "/foo.txt") map to a real file?

    Models the deployed _site layout (see .github/workflows/playground.yml): the
    site root is web/, and /playground is the top-level playground/ dir merged in
    at deploy time.
    """
    path = link.split("#")[0].split("?")[0]
    if not path.startswith("/"):
        return True  # relative or external handled elsewhere
    rel = path.lstrip("/")
    # /playground[/...] is deployed from the repo-root playground/ dir.
    if rel == "playground" or rel.startswith("playground/"):
        base = os.path.join("playground", rel[len("playground"):].lstrip("/"))
    else:
        base = os.path.join(WEB, rel)
    if path.endswith("/") or rel == "":
        return os.path.isfile(os.path.join(base, "index.html"))
    if os.path.isfile(base):
        return True
    # extensionless: allow /foo -> foo/index.html
    return os.path.isfile(os.path.join(base, "index.html"))


def check_links_and_wellformed():
    href = re.compile(r'(?:href|src)="([^"]+)"')
    for f in sorted(html_files()):
        text = open(f).read()
        b = Balance()
        b.feed(text)
        if b.bad or b.stack:
            errors.append(f"{f}: not well-formed (bad={b.bad[:3]} unclosed={b.stack[-3:]})")
        for link in href.findall(text):
            if link.startswith("/") and not resolves(link):
                errors.append(f"{f}: dead internal link -> {link}")


def check_answers_subnav():
    ans_dir = os.path.join(WEB, "answers")
    if not os.path.isdir(ans_dir):
        return
    dirs = {d for d in os.listdir(ans_dir)
            if os.path.isdir(os.path.join(ans_dir, d))}
    # Every answer page's sub-nav must list exactly the set of answer dirs.
    slug_re = re.compile(r'href="/answers/([a-z-]+)/"')
    for d in sorted(dirs):
        idx = os.path.join(ans_dir, d, "index.html")
        if not os.path.isfile(idx):
            continue
        text = open(idx).read()
        m = re.search(r'<div class="subnav[^"]*">(.*?)</div>', text, re.DOTALL)
        if not m:
            errors.append(f"answers/{d}: no sub-nav listing the answers")
            continue
        listed = set(slug_re.findall(m.group(1)))
        missing = dirs - listed
        extra = listed - dirs
        if missing:
            errors.append(f"answers/{d}: sub-nav missing pages {sorted(missing)}")
        if extra:
            errors.append(f"answers/{d}: sub-nav lists nonexistent pages {sorted(extra)}")


CANONICAL = re.compile(r'<link rel="canonical" href="([^"]+)"')
NOINDEX = re.compile(r'<meta name="robots" content="[^"]*noindex')


def page_meta(path):
    """(canonical URL or None, noindex?) of a served HTML file."""
    text = open(path).read()
    m = CANONICAL.search(text)
    return (m.group(1) if m else None), bool(NOINDEX.search(text))


def check_sitemap():
    """sitemap.xml lists exactly the pages a crawler should index.

    The rule: a served page belongs in the sitemap when it is indexable (no
    robots noindex) and its canonical is its own URL. A page whose canonical
    points elsewhere (the newest month of releases, which duplicates
    /versions/) is left out, because a sitemap entry that names another URL as
    the real one is the conflict search consoles report. A page with no
    canonical counts as its own canonical. Every served page is held to the
    rule in both directions: a page that qualifies and is missing is an
    omission, and a listed page that does not qualify, or serves nothing, is
    an error. A canonical that points elsewhere must point at a listed page,
    so moving a page's canonical cannot quietly drop both from the sitemap.

    The sitemap used to be hand-maintained, so a new page reached the site and
    never reached a crawler. scripts/build_site.py now generates it, with each
    <lastmod> from site/lastmod.json (see its docstring), and this check still
    holds the served set and the sitemap to each other. check_site_built.py
    keeps the dates current.
    """
    sm = os.path.join(WEB, "sitemap.xml")
    if not os.path.isfile(sm):
        errors.append("web/sitemap.xml is missing")
        return
    text = open(sm).read()
    listed = set(re.findall(r"<loc>https://glyphlang\.io(/[^<]*)</loc>", text))
    if "<changefreq>" in text or "<priority>" in text:
        errors.append("sitemap.xml: carries changefreq or priority, which the generator no longer writes")

    pages = {}
    for root, _dirs, files in os.walk(WEB):
        if "index.html" in files:
            rel = os.path.relpath(root, WEB)
            pages["/" if rel == "." else f"/{rel}/"] = os.path.join(root, "index.html")
    # /playground is the repo-root playground/ dir, merged in at deploy time.
    if os.path.isfile(os.path.join("playground", "index.html")):
        pages["/playground/"] = os.path.join("playground", "index.html")

    belongs = set()
    for url, path in sorted(pages.items()):
        canonical, noindex = page_meta(path)
        own = canonical in (None, "https://glyphlang.io" + url)
        if own and not noindex:
            belongs.add(url)
        elif not own and canonical.removeprefix("https://glyphlang.io") not in listed:
            errors.append(f"{url}: canonical {canonical} is not a page in sitemap.xml")
        if url in listed and not own:
            errors.append(f"sitemap.xml: lists {url}, whose canonical is {canonical}")
        if url in listed and noindex:
            errors.append(f"sitemap.xml: lists {url}, which is noindex")

    for path in sorted(belongs - listed):
        errors.append(f"sitemap.xml: no entry for {path}")
    for path in sorted(listed - set(pages)):
        errors.append(f"sitemap.xml: entry for {path}, which serves no page")


def main():
    if not os.path.isdir(WEB):
        print(f"no {WEB}/ directory; run from the repo root", file=sys.stderr)
        return 1
    check_links_and_wellformed()
    check_answers_subnav()
    check_sitemap()
    if errors:
        print("site check FAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("site check OK (links resolve, HTML well-formed, sub-nav and sitemap complete)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
