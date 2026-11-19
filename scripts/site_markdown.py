"""Markdown copies of the pages agents read most.

scripts/build_site.py writes web/<path>/index.md next to the HTML of /start/,
/from-typescript/, /agents/, /teams/, /evidence/, /answers/ and every answer,
converted from the same rendered body, so the two never disagree. Each HTML
page links its copy with <link rel="alternate" type="text/markdown">, and
web/llms-pages.txt lists them all. The copies are not in the sitemap.

GitHub Pages cannot pick a format by Accept header, so a copy is a separate
address. web/llms.txt is a byte copy of AGENTS.md (scripts/check_llms_sync.py
holds them together), which is why the list lives in its own file.

The conversion covers what these pages are made of: headings, paragraphs,
lists, tables, code blocks (fenced, one source line per line), inline code,
emphasis and links (made absolute). Navigation, buttons, icons, scripts and
anything aria-hidden are dropped.

Python standard library only.
"""
from __future__ import annotations

import html
import re
from html.parser import HTMLParser

ORIGIN = "https://glyphlang.io"
PAGES = ("/start/", "/from-typescript/", "/agents/", "/teams/", "/evidence/")


def wanted(url: str) -> bool:
    return url in PAGES or url.startswith("/answers/")


SKIP = {"script", "style", "nav", "button", "svg", "template", "noscript", "img"}
BLOCKS = {"p", "div", "section", "article", "header", "footer", "figure", "figcaption", "details",
          "summary", "blockquote", "main", "aside", "table", "dl", "dt", "dd"}
VOID = {"br", "img", "hr", "input", "meta", "link", "wbr", "source"}


# Marks where a classed <span> starts or ends. Labels, chips and numbers sit
# flush against the next word in the markup and are separated by layout, so
# words() turns the mark into a space between two words and drops it
# elsewhere ("first<span>.</span>" stays "first.").
GAP = "\x01"


def words(text: str) -> str:
    text = re.sub(r"(?<=[^\s\x01(\[])\x01+(?=[A-Za-z0-9`*\[(])", " ", text)
    return text.replace(GAP, "")


class Converter(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.out: list[str] = []      # finished blocks
        self.line: list[str] = []     # inline text of the block being built
        self.skip = 0                 # depth inside a skipped element
        self.stack: list[str] = []    # open non-void tags, for skip bookkeeping
        self.lists: list[list] = []   # [kind, counter] per open list
        self.pre: list[str] | None = None
        self.pre_lines = 0
        self.href: list[str | None] = []
        self.row: list[str] | None = None
        self.cell: list[str] | None = None
        self.rows = 0
        self.li_prefix = ""
        self.code: list[str] | None = None   # an inline code span being read
        self.card: list | None = None        # [href, text] of a card link being read

    # -- helpers
    def flush(self, prefix: str | None = None) -> None:
        """End the block being built. Its prefix is `prefix`, or else the
        bullet of the list item it opens (or the indent of one it continues)."""
        text = re.sub(r"[ \t\n]+", " ", "".join(self.line)).strip()
        self.line = []
        text = words(text).strip()
        if text:
            self.out.append((self.pending_prefix() if prefix is None else prefix) + text)

    def emit(self, s: str) -> None:
        if self.cell is not None:
            self.cell.append(s)
        else:
            self.line.append(s)

    def list_prefix(self) -> str:
        depth = len(self.lists) - 1
        kind = self.lists[-1]
        if kind[0] == "ol":
            kind[1] += 1
            return "   " * depth + f"{kind[1]}. "
        return "  " * depth + "- "

    # -- parser
    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag not in VOID:
            self.stack.append(tag)
        if self.skip:
            if tag not in VOID:
                self.skip += 1
            return
        cls = a.get("class") or ""
        if tag in SKIP or a.get("aria-hidden") == "true" or "h-anchor" in cls.split() or "sr-only" in cls:
            if tag not in VOID:
                self.skip = 1
            return
        if self.card is not None:
            if tag not in ("code", "strong", "b", "em", "i") and not (tag == "span" and not cls):
                self.card[1].append(" ")
            return
        if self.pre is not None:
            if tag == "span" and "ln" in cls.split():
                if self.pre_lines:
                    self.pre.append("\n")
                self.pre_lines += 1
            elif tag == "br":
                self.pre.append("\n")
            return
        if tag == "pre":
            self.flush()
            self.pre, self.pre_lines = [], 0
        elif tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            self.flush()
        elif tag in ("ul", "ol"):
            self.flush()
            self.lists.append([tag, 0])
        elif tag == "li":
            self.flush()
            self.li_prefix = self.list_prefix()
        elif tag == "tr":
            self.flush()
            self.row = []
        elif tag in ("td", "th"):
            self.cell = []
        elif tag in BLOCKS:
            self.flush()
            if tag == "summary":
                self.emit("**")
        elif tag == "br":
            self.emit(" ")
        elif tag == "code":
            self.code = []
        elif tag in ("strong", "b"):
            self.emit("**")
        elif tag in ("em", "i"):
            self.emit("*")
        elif tag == "a":
            href = a.get("href")
            if href and href.startswith("/"):
                href = ORIGIN + href
            elif href and href.startswith("#"):
                href = None
            if cls and href:
                # A card: a link wrapped around a block of text. It becomes
                # one list item holding the card's text as the link.
                self.flush()
                self.card = [href, []]
                return
            self.href.append(href)
            if href:
                self.emit("[")
        elif tag == "span" and cls:
            self.emit(GAP)

    def pending_prefix(self) -> str:
        p = self.li_prefix
        self.li_prefix = "  " * len(self.lists) if self.lists else ""
        return p

    def handle_endtag(self, tag):
        if self.stack and self.stack[-1] == tag:
            self.stack.pop()
        if self.skip:
            if tag not in VOID:
                self.skip -= 1
            return
        if self.card is not None:
            if tag == "a":
                text = re.sub(r"\s+", " ", "".join(self.card[1])).strip().replace("]", "\\]")
                if text:
                    self.out.append(f"- [{text}]({self.card[0]})")
                self.card = None
            elif tag not in ("code", "strong", "b", "em", "i", "span"):
                self.card[1].append(" ")
            return
        if self.pre is not None:
            if tag == "pre":
                code = "".join(self.pre).strip("\n")
                fence = "````" if "```" in code else "```"
                self.out.append(f"{fence}\n{code}\n{fence}")
                self.pre = None
            return
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            self.flush("#" * int(tag[1]) + " ")
        elif tag in ("ul", "ol"):
            self.flush()
            self.lists.pop()
            self.li_prefix = "  " * len(self.lists) if self.lists else ""
        elif tag == "li":
            self.flush()
        elif tag in ("td", "th"):
            text = re.sub(r"\s+", " ", "".join(self.cell or [])).strip().replace("|", "\\|")
            if self.row is not None:
                self.row.append(text)
            self.cell = None
        elif tag == "tr":
            if self.row:
                self.out.append("| " + " | ".join(self.row) + " |")
                if self.rows == 0:
                    self.out.append("|" + "---|" * len(self.row))
                self.rows += 1
            self.row = None
        elif tag == "table":
            self.rows = 0
        elif tag in BLOCKS:
            if tag == "summary":
                self.emit("**")
            self.flush()
        elif tag == "code" and self.code is not None:
            text = re.sub(r"\s+", " ", "".join(self.code))
            self.code = None
            if text.strip():
                # A span holding backticks needs a longer fence than it has.
                ticks = "`" * (max((len(r) for r in re.findall(r"`+", text)), default=0) + 1)
                pad = " " if text.startswith("`") or text.endswith("`") else ""
                self.emit(f"{ticks}{pad}{text}{pad}{ticks}")
        elif tag in ("strong", "b"):
            self.emit("**" + GAP)
        elif tag in ("em", "i"):
            self.emit("*")
        elif tag == "a":
            href = self.href.pop() if self.href else None
            if href:
                self.emit(f"]({href})")
        elif tag == "span":
            self.emit(GAP)

    def handle_data(self, data):
        if self.skip:
            return
        if self.card is not None:
            self.card[1].append(data)
        elif self.pre is not None:
            self.pre.append(data)
        elif self.code is not None:
            self.code.append(data)
        else:
            self.emit(data)


def tidy(blocks: list[str]) -> str:
    """Blocks joined with blank lines, except consecutive list items and
    table rows, which stay together."""
    out: list[str] = []
    for b in blocks:
        if not b.startswith("```"):
            # An empty emphasis or code span left by markup with no text.
            b = re.sub(r"\*\*\s*\*\*|(?<!`)``(?!`)", "", b)
        if not b.strip():
            continue
        item = re.match(r"\s*(- |\d+\. |\| )", b)
        prev_item = out and re.match(r"\s*(- |\d+\. |\| )", out[-1])
        sep = "\n" if item and prev_item else "\n\n"
        out.append(sep + b if out else b)
    return "".join(out).strip() + "\n"


def convert(body: str, url: str, title: str, date: str) -> str:
    c = Converter()
    c.feed(body)
    c.close()
    c.flush()
    head = (f"<!-- Markdown copy of {ORIGIN}{url}, generated from the same source as the page. "
            f"Last changed {date}. -->\n\n")
    return head + tidy(c.out)


def listing(entries: list[tuple[str, str, str]]) -> str:
    """web/llms-pages.txt: every Markdown copy, as llms.txt-style links."""
    lines = ["# Glyph pages as Markdown", "",
             "> Markdown copies of the glyphlang.io pages agents read most, generated from the same "
             "source as the HTML on every build. The language reference for agents is "
             f"{ORIGIN}/llms.txt.", ""]
    groups = [("Getting started", [e for e in entries if not e[0].startswith("/answers/")]),
              ("Answers", [e for e in entries if e[0].startswith("/answers/")])]
    for name, items in groups:
        lines += [f"## {name}", ""]
        lines += [f"- [{html.unescape(title)}]({ORIGIN}{url}index.md): {html.unescape(desc)}"
                  for url, title, desc in items]
        lines.append("")
    return "\n".join(lines)
