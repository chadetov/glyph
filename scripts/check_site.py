#!/usr/bin/env python3
"""Site consistency checks for web/ (run in CI and locally).

Fails (exit 1) on:
  - an internal link that resolves to no file (href/src starting with "/")
  - an HTML file that isn't well-formed (unbalanced tags)
  - an answers sub-nav that is missing an existing answer page, or points at one
    that doesn't exist

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
        m = re.search(r'<div class="subnav">(.*?)</div>', text, re.DOTALL)
        if not m:
            continue
        listed = set(slug_re.findall(m.group(1)))
        missing = dirs - listed
        extra = listed - dirs
        if missing:
            errors.append(f"answers/{d}: sub-nav missing pages {sorted(missing)}")
        if extra:
            errors.append(f"answers/{d}: sub-nav lists nonexistent pages {sorted(extra)}")


def main():
    if not os.path.isdir(WEB):
        print(f"no {WEB}/ directory; run from the repo root", file=sys.stderr)
        return 1
    check_links_and_wellformed()
    check_answers_subnav()
    if errors:
        print("site check FAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("site check OK (links resolve, HTML well-formed, answers sub-nav consistent)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
