"""Syntax highlighting for every code block on the site, at build time.

scripts/build_site.py calls `highlight_pres()` on each page body after every
other expansion, English and translated alike. Each `<pre>` that carries no
token markup yet is given some: its language is read from a `data-lang`
attribute, else from the file name its caption starts with (`main.glyph`,
`form.ts`), else from the content, and a block that looks like none of them
(prose, a diagram) is left as it is. A block that already has `<span class=`
markup was coloured by hand, usually to say something (a diff, the line a
diagnostic is about), and is never touched.

scripts/site_catches.py calls `highlight()` directly for its code excerpts, so
/catches/ and the rest of the site colour code the same way.

The one rule that cannot bend: highlighting adds markup and nothing else. The
text of every block, which scripts/check_docs_compile.py compiles and a reader
copies, is identical before and after, and `highlight_pres()` raises if it is
not. Existing tags inside a block (a link, a `<mark>`) are kept where they are.

Languages:

  glyph   keywords from the lexer's KEYWORDS table, types, strings with their
          `${...}` interpolations, numbers, comments, `->` `=>` `?`, JSX
  ts      TypeScript, JavaScript, TSX, and an HTML page's markup
  json    keys, strings, numbers, true/false/null; `...` elisions are fine
  shell   `$` prompts, commands, flags, strings, comments; the lines without a
          prompt are output and coloured as `out`
  out     compiler and tool output: error codes, Error/Warning labels,
          Help/Note, file:line locations, ariadne's box drawing
  diff    `+` and `-` lines

The colours are in site/highlight.css. Python standard library only.
"""
from __future__ import annotations

import html
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
TOKEN_RS = ROOT / "glyph-compiler" / "crates" / "glyph-lexer" / "src" / "token.rs"

LANGS = ("glyph", "ts", "json", "shell", "out", "diff")


class HighlightError(Exception):
    pass


def _glyph_keywords() -> frozenset[str]:
    """The lexer's reserved words, read from the table the lexer itself uses."""
    src = TOKEN_RS.read_text()
    m = re.search(r"static KEYWORDS: &\[\(&str, Token\)\] = &\[(.*?)\n\];", src, re.S)
    if not m:
        raise HighlightError(f"{TOKEN_RS.relative_to(ROOT)}: no KEYWORDS table to read")
    words = re.findall(r'^\s*\("([a-z_]+)", Token::', m.group(1), re.M)
    if len(words) < 20:
        raise HighlightError(f"{TOKEN_RS.relative_to(ROOT)}: KEYWORDS table read as {words}")
    return frozenset(words)


GLYPH_KEYWORDS = _glyph_keywords()
TS_KEYWORDS = frozenset({
    "abstract", "as", "async", "await", "break", "case", "catch", "class", "const", "continue",
    "declare", "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally",
    "for", "from", "function", "get", "if", "implements", "import", "in", "instanceof",
    "interface", "is", "keyof", "let", "module", "namespace", "new", "null", "of", "private",
    "protected", "public", "readonly", "return", "satisfies", "set", "static", "super", "switch",
    "this", "throw", "true", "try", "type", "typeof", "undefined", "var", "void", "while", "yield",
})
# Lower-case type names, coloured as types in both languages.
PRIMITIVES = frozenset({
    "string", "number", "int", "bool", "boolean", "unknown", "never", "any", "bigint",
    "symbol", "object",
})
SHELL_COMMANDS = frozenset({
    "glyph", "npm", "npx", "node", "cd", "git", "cargo", "python3", "tsc", "tsx", "curl",
    "mkdir", "cat", "grep", "ls", "echo", "pnpm", "yarn", "brew",
})

# ---------------------------------------------------------------- tokens
# A tokenizer returns [(start, end, class)] over the plain text, in order and
# not overlapping. Text outside every token is left uncoloured.

Tokens = list[tuple[int, int, str]]

IDENT = re.compile(r"[A-Za-z_$][A-Za-z0-9_$]*")
NUMBER = re.compile(r"0[xX][0-9a-fA-F_]+n?|0[bB][01_]+n?|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?n?")
SPACE = re.compile(r"\s*")


class _Code:
    """One pass over Glyph or TypeScript source."""

    def __init__(self, text: str, lang: str):
        self.text = text
        self.lang = lang
        self.keywords = GLYPH_KEYWORDS if lang == "glyph" else TS_KEYWORDS
        self.out: Tokens = []

    def add(self, start: int, end: int, cls: str) -> None:
        if end > start:
            self.out.append((start, end, cls))

    def run(self) -> Tokens:
        self.code(0, len(self.text), closer=None)
        return self.out

    # The text before `<` decides whether it opens a JSX tag: at the start of
    # a line, or after `return`, `(`, `=>` or `,`, it does; after an
    # identifier (`Array<T>`, `a < b`) it does not.
    def _jsx_can_start(self, i: int) -> bool:
        j = i - 1
        while j >= 0 and self.text[j] in " \t":
            j -= 1
        if j < 0 or self.text[j] == "\n":
            return True  # the first thing on its line
        ch = self.text[j]
        if ch in "(,=>{}:?;[|&":
            return ch != ">" or (j > 0 and self.text[j - 1] == "=")
        m = re.search(r"([A-Za-z_]+)$", self.text[: j + 1])
        return bool(m and m.group(1) in ("return", "yield", "await", "else", "case", "default"))

    def code(self, i: int, end: int, closer: str | None) -> int:
        """Tokenize code from `i` until `end`, or until an unbalanced
        `closer` (`}` for an interpolation or a JSX expression). Returns the
        index of that closer, or `end`."""
        t = self.text
        depth = 0
        while i < end:
            ch = t[i]
            if closer and ch == "{":
                depth += 1
            elif closer and ch == "}":
                if depth == 0:
                    return i
                depth -= 1
            if t.startswith("//", i):
                j = t.find("\n", i)
                j = end if j < 0 or j > end else j
                self.add(i, j, "c")
                i = j
            elif t.startswith("/*", i):
                j = t.find("*/", i + 2)
                j = end if j < 0 else min(j + 2, end)
                self.add(i, j, "c")
                i = j
            elif ch in "\"'`":
                i = self.string(i, end)
            elif ch == "<" and i + 1 < end and (t[i + 1].isalpha() or t[i + 1] in "/>") \
                    and self._jsx_can_start(i):
                i = self.jsx(i, end)
            elif ch == "@" and i + 1 < end and t[i + 1].isalpha():
                m = IDENT.match(t, i + 1)
                self.add(i, m.end(), "k")
                i = m.end()
            elif ch.isdigit() and (i == 0 or not (t[i - 1].isalnum() or t[i - 1] in "_$")):
                m = NUMBER.match(t, i)
                self.add(i, m.end(), "n")
                i = m.end()
            elif ch.isalpha() or ch in "_$":
                m = IDENT.match(t, i)
                word = m.group(0)
                if word in self.keywords:
                    self.add(i, m.end(), "k")
                elif word[0].isupper() or word in PRIMITIVES:
                    self.add(i, m.end(), "t")
                i = m.end()
            elif t.startswith(("->", "=>"), i):
                self.add(i, i + 2, "op")
                i += 2
            elif t.startswith(("?.", "??"), i):
                self.add(i, i + 2, "op")
                i += 2
            elif ch == "?":
                self.add(i, i + 1, "op")
                i += 1
            else:
                i += 1
        return end

    def string(self, i: int, end: int) -> int:
        """A string literal from its opening quote; returns the index after it.
        `${...}` inside a Glyph string or a TS template literal is code."""
        t = self.text
        quote = t[i]
        if self.lang == "glyph" and t.startswith('"""', i):
            quote = '"""'
        interpolates = (self.lang == "glyph" and quote != "'") or quote == "`"
        start = i
        i += len(quote)
        while i < end:
            if t[i] == "\\":
                i += 2
                continue
            if t.startswith(quote, i):
                i += len(quote)
                self.add(start, i, "s")
                return i
            if quote in "\"'" and t[i] == "\n":
                break
            if interpolates and t.startswith("${", i):
                self.add(start, i, "s")
                self.add(i, i + 2, "ip")
                close = self.code(i + 2, end, closer="}")
                if close >= end:
                    return end
                self.add(close, close + 1, "ip")
                i = start = close + 1
                continue
            i += 1
        self.add(start, min(i, end), "s")
        return min(i, end)

    def jsx(self, i: int, end: int) -> int:
        """A JSX element (or an HTML one) from its `<`; returns the index
        after its closing tag."""
        t = self.text
        depth = 0
        while i < end and t[i] == "<":
            closing = t.startswith("</", i)
            i, name = self.tag(i, end)
            if closing:
                depth -= 1
            elif t[i - 2:i] != "/>":
                depth += 1
            if depth <= 0:
                return i
            if name == "script" and not closing:
                stop = t.find("</script", i, end)
                stop = end if stop < 0 else stop
                self.code(i, stop, closer=None)
                i = stop
                continue
            # Text between tags: plain, except `{expr}` and nested tags.
            while i < end and t[i] != "<":
                if t[i] == "{":
                    self.add(i, i + 1, "ip")
                    close = self.code(i + 1, end, closer="}")
                    if close >= end:
                        return end
                    self.add(close, close + 1, "ip")
                    i = close + 1
                else:
                    i += 1
        return i

    def tag(self, i: int, end: int) -> tuple[int, str]:
        """One `<tag attr="v" attr={e}>`, `</tag>` or `<>`; returns the index
        after its `>` and the tag's name."""
        t = self.text
        j = i + (2 if t.startswith("</", i) else 1)
        name = ""
        m = TAG_NAME.match(t, j)
        if m:
            name = m.group(0)
            self.add(m.start(), m.end(), "t")
            j = m.end()
        while j < end:
            ch = t[j]
            if ch == ">":
                return j + 1, name
            if t.startswith("/>", j):
                return j + 2, name
            if ch in "\"'":
                q = t.find(ch, j + 1)
                q = end - 1 if q < 0 else q
                self.add(j, q + 1, "s")
                j = q + 1
            elif ch == "{":
                close = self.code(j + 1, end, closer="}")
                j = close + 1
            elif ch.isalpha():
                a = ATTR_NAME.match(t, j)
                if a.group(0)[0].isupper():
                    self.add(a.start(), a.end(), "t")
                j = a.end()
            else:
                j += 1
        return end, name


TAG_NAME = re.compile(r"[A-Za-z][\w.:-]*")
ATTR_NAME = re.compile(r"[\w:-]+")


def code_tokens(text: str, lang: str) -> Tokens:
    return _Code(text, lang).run()


JSON_TOKEN = re.compile(
    r'(?P<str>"(?:\\.|[^"\\])*")'
    r"|(?P<num>-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)"
    r"|(?P<lit>\b(?:true|false|null)\b)"
    r"|(?P<gap>\.\.\.|…)"
    r"|(?P<cm>//[^\n]*)"
)


def json_tokens(text: str) -> Tokens:
    out: Tokens = []
    for m in JSON_TOKEN.finditer(text):
        kind = m.lastgroup
        if kind == "str":
            rest = SPACE.match(text, m.end()).end()
            cls = "pk" if rest < len(text) and text[rest] == ":" else "s"
        else:
            cls = {"num": "n", "lit": "k", "gap": "c", "cm": "c"}[kind]
        out.append((m.start(), m.end(), cls))
    return out


OUT_TOKEN = re.compile(
    r"(?P<code>\[?\b[EW]\d{4}\b\]?|\bTS\d{4,5}\b)"
    r"|(?P<err>\b(?:Error|error|FAILED|failed|WILL_FAIL)\b:?)"
    r"|(?P<warn>\b(?:Warning|warning)\b:?|\[(?:update|warn)\])"
    r"|(?P<ok>\[ok\]|\bOK\b|\bok\b|\bpassed\b)"
    r"|(?P<help>\b(?:Help|help|Note|note):)"
    r"|(?P<loc>(?<![\w/])[\w./-]+\.(?:glyph|tsx?|m?js|json):\d+(?::\d+)?)"
    r"|(?P<tick>`[^`\n]+`)"
    r"|(?P<box>[─-╿]+)"
    r"|(?P<url>https?://[^\s)]+)"
)
OUT_CLASS = {"code": "e", "err": "e", "warn": "w", "ok": "o", "help": "f", "loc": "lc",
             "tick": "q", "box": "bx", "url": "lc"}


def out_tokens(text: str, base: int = 0) -> Tokens:
    return [(base + m.start(), base + m.end(), OUT_CLASS[m.lastgroup])
            for m in OUT_TOKEN.finditer(text)]


SHELL_WORD = re.compile(
    r"(?P<s>\"(?:\\.|[^\"\\\n])*\"|'[^'\n]*')"
    r"|(?P<c>(?<!\S)#[^\n]*)"
    r"|(?P<sep>&&|\|\||[|;])"
    r"|(?P<fl>(?<!\S)--?[A-Za-z][\w-]*(?:=\S*)?)"
    r"|(?P<w>[^\s\"'|;&]+)"
)


def _command_line(text: str, start: int, end: int, out: Tokens, first: bool) -> None:
    """One command's tokens; `first` says the next word names the command."""
    for m in SHELL_WORD.finditer(text, start, end):
        kind = m.lastgroup
        if kind == "s":
            out.append((m.start(), m.end(), "s"))
        elif kind == "c":
            out.append((m.start(), m.end(), "c"))
        elif kind == "sep":
            first = True
        elif kind == "fl":
            out.append((m.start(), m.end(), "fl"))
            first = False
        elif first:
            out.append((m.start(), m.end(), "f"))
            first = False


def shell_tokens(text: str) -> Tokens:
    """Lines with a `$ ` prompt are commands and the rest are their output.
    A block with no prompt at all is a list of commands."""
    out: Tokens = []
    prompted = bool(re.search(r"^\s*\$ ", text, re.M))
    pos = 0
    continued = False
    for line in text.split("\n"):
        end = pos + len(line)
        m = re.match(r"(\s*)\$ ", line)
        if m:
            out.append((pos + m.end(1), pos + m.end(1) + 1, "pr"))
            _command_line(text, pos + m.end(), end, out, first=True)
            continued = line.rstrip().endswith("\\")
        elif continued or not prompted:
            _command_line(text, pos, end, out, first=not continued)
            continued = line.rstrip().endswith("\\")
        else:
            out += out_tokens(line, pos)
        pos = end + 1
    return out


def diff_tokens(text: str) -> Tokens:
    out: Tokens = []
    pos = 0
    for line in text.split("\n"):
        end = pos + len(line)
        if line.startswith("@@"):
            out.append((pos, end, "f"))
        elif line.startswith("+"):
            out.append((pos, end, "add"))
        elif line.startswith("-"):
            out.append((pos, end, "del"))
        pos = end + 1
    return out


def tokens(text: str, lang: str) -> Tokens:
    if lang in ("glyph", "ts"):
        return code_tokens(text, lang)
    if lang == "json":
        return json_tokens(text)
    if lang == "shell":
        return shell_tokens(text)
    if lang == "out":
        return out_tokens(text)
    if lang == "diff":
        return diff_tokens(text)
    raise HighlightError(f"unknown language {lang!r}")


# ---------------------------------------------------------------- rendering


def highlight(code: str, lang: str) -> str:
    """Plain source text to escaped, highlighted HTML."""
    return _render([(code, False)], tokens(code, lang))


TAG = re.compile(r"<[^>]+>")


def _segments(inner: str) -> list[tuple[str, bool]]:
    """An HTML fragment as [(text or tag, is_tag)], text unescaped."""
    out, pos = [], 0
    for m in TAG.finditer(inner):
        if m.start() > pos:
            out.append((html.unescape(inner[pos:m.start()]), False))
        out.append((m.group(0), True))
        pos = m.end()
    if pos < len(inner):
        out.append((html.unescape(inner[pos:]), False))
    return out


def _render(segments: list[tuple[str, bool]], toks: Tokens) -> str:
    """Wrap each token's text in a span. Tags stay where they were; a token
    that runs across one is closed before it and reopened after."""
    out = []
    pos = 0  # offset into the plain text
    k = 0
    for seg, is_tag in segments:
        if is_tag:
            out.append(seg)
            continue
        seg_end = pos + len(seg)
        cur = pos
        while cur < seg_end:
            while k < len(toks) and toks[k][1] <= cur:
                k += 1
            if k < len(toks) and toks[k][0] < seg_end:
                s, e, cls = toks[k]
                if s > cur:
                    out.append(html.escape(text_slice(seg, pos, cur, s), quote=False))
                    cur = s
                stop = min(e, seg_end)
                out.append(f'<span class="{cls}">'
                           f"{html.escape(text_slice(seg, pos, cur, stop), quote=False)}</span>")
                cur = stop
            else:
                out.append(html.escape(text_slice(seg, pos, cur, seg_end), quote=False))
                cur = seg_end
        pos = seg_end
    return "".join(out)


def text_slice(seg: str, seg_start: int, a: int, b: int) -> str:
    return seg[a - seg_start:b - seg_start]


def text_of(inner: str) -> str:
    """The text a reader sees in a block: tags dropped, entities decoded."""
    return html.unescape(TAG.sub("", inner))


# ---------------------------------------------------------------- detection

EXT = {"glyph": "glyph", "ts": "ts", "tsx": "ts", "js": "ts", "mjs": "ts", "cjs": "ts",
       "jsx": "ts", "html": "ts", "json": "json", "diff": "diff", "patch": "diff", "sh": "shell"}
CAPTION_FILE = re.compile(r"^[\w./-]+\.([a-z]+)(?=$|[\s:,(])")


def caption_lang(caption: str) -> str | None:
    """The language a caption names by starting with a file name. A caption
    that is a command (`glyph query ... --path src/main.glyph`) names the
    command's output, so only its first word counts."""
    m = CAPTION_FILE.match(html.unescape(TAG.sub("", caption)).strip())
    return EXT.get(m.group(1)) if m else None


GLYPH_SIGNS = [
    r"^\s*module [\w/]+\s*$", r"^\s*(pub )?fn \w", r"\bfn\s*\(", r"\bmatch\b[^\n]*\{\s*$", r"->",
    r"^\s*import [\w/@.-]+( \{|\s*$)", r"^\s*(pub )?type \w+(<[^>]*>)? *=", r"^\s*(pub )?component \w",
    r"\b(Ok|Err|Some|None)\(", r"\bOption<|\bResult<", r"^\s*let \w+(: [^=]+)? = ", r"^\s*@example\b",
    r"=> [^\n]*,\s*$",
]
TS_SIGNS = [
    r"\bfunction\b", r"^\s*import\b[^\n]*\bfrom\b", r"^\s*export\b", r";\s*$", r"===|!==",
    r"^\s*(const|var) \w+ = ", r"\binterface \w", r"\bnew [A-Z]", r"\) => ", r"\bas [A-Z]\w*\)",
    r"^\s*(let|const) \w+: [\w<>|\[\] ]+;", r"\bconsole\.log\(",
    r"\b(Buffer|JSON|Object|Promise|Math|Number|Date)\.\w+\(",
]
# Output signs are weighted: one strong sign, or two weak ones, make a block
# output. A single line needs only one weak sign, since a one-line diagnostic
# is common and a one-line code block that says "cannot" is not.
OUT_STRONG = [
    r"\[[EW]\d{4}\]", r"\berror TS\d{4}", r"^\s*[╭╰│]", r"^[\w./-]+\.glyph:\d+:\d+",
    r"^\s*\[(ok|update|warn)\]",
]
OUT_WEAK = [r"\b[EW]\d{4}\b", r"\b(Help|Note):", r"has no field|cannot |expected `"]


def _score(text: str, signs: list[str]) -> int:
    return sum(bool(re.search(s, text, re.M)) for s in signs)


def _is_json(text: str) -> bool:
    s = text.lstrip()
    if s[:1] not in "{[\"":
        return False
    if s[:1] == '"' and not re.match(r'"[^"\n]*"\s*:', s):
        return False
    # Every line is JSON-shaped: punctuation, a quoted key, a value, or an
    # elision. Glyph records and TS objects have bare keys and fail here.
    # Strings may wrap across lines in a block trimmed for the page.
    for line in re.sub(r'"(?:\\.|[^"\\])*"', '""', text).split("\n"):
        line = line.strip()
        if not line or line in ("...", "…"):
            continue
        if re.search(r"(^|[{,\[]\s*)[A-Za-z_]\w*\s*:", line) or "=>" in line or "//" in line:
            return False
    return bool(re.search(r'"[^"\n]*"\s*:', text))


def _is_prose(text: str) -> bool:
    """Most lines run four plain words in a row once strings and comments are
    gone, which a line of code almost never does: a quoted spec, a note."""
    lines = [ln for ln in text.split("\n") if ln.strip()]
    wordy = 0
    for line in lines:
        bare = re.sub(r'"[^"\n]*"|`[^`\n]*`|//.*|#.*', "", line)
        wordy += bool(re.search(r"[A-Za-z]+(?:,? [A-Za-z][\w-]*){3}", bare))
    return bool(lines) and wordy * 2 >= len(lines)


def _is_shell(text: str) -> bool:
    lines = [ln for ln in text.split("\n") if ln.strip()]
    if not lines:
        return False
    if re.match(r"\s*\$ ", lines[0]):
        return True
    continued = False
    for line in lines:
        word = line.split()[0]
        if not continued and word not in SHELL_COMMANDS:
            return False
        continued = line.rstrip().endswith("\\")
    return True


def _is_diff(text: str) -> bool:
    lines = [ln for ln in text.split("\n") if ln]
    if not lines or not any(ln.startswith("@@") or ln.startswith("diff --git") for ln in lines):
        return False
    return all(ln[0] in "+- @d\\" for ln in lines)


def detect(text: str, caption: str = "", data_lang: str | None = None) -> str | None:
    """The language of a block, or None to leave it plain."""
    if data_lang:
        if data_lang in ("none", "text", "plain"):
            return None
        if data_lang not in LANGS:
            raise HighlightError(f"data-lang={data_lang!r} is not one of {', '.join(LANGS)}")
        return data_lang
    by_caption = caption_lang(caption) if caption else None
    if by_caption:
        return by_caption
    if _is_json(text):
        return "json"
    if _is_diff(text):
        return "diff"
    if _is_shell(text):
        return "shell"
    glyph, ts = _score(text, GLYPH_SIGNS), _score(text, TS_SIGNS)
    out = 2 * _score(text, OUT_STRONG) + _score(text, OUT_WEAK)
    if out >= (1 if "\n" not in text.strip() else 2) and out >= max(glyph, ts):
        return "out"
    if _is_prose(text):
        return None
    if glyph or ts:
        return "ts" if ts > glyph else "glyph"
    # A list of literals with comments ("2026-01-03"  // UTC midnight).
    if all(re.match(r'\s*("[^"]*"|-?\d[\d.]*)\s*(//.*)?$', ln) for ln in text.split("\n") if ln.strip()):
        return "glyph"
    return None


# ---------------------------------------------------------------- pages

PRE = re.compile(r"<pre(\s[^>]*)?>(.*?)</pre>", re.S)
CAPTION = re.compile(r'<span class="cap">(.*?)</span>', re.S)
DATA_LANG = re.compile(r'\sdata-lang="([^"]*)"')


def caption_before(body: str, start: int) -> str:
    """The caption of the code box a `<pre>` at `start` sits in: the last
    `<span class="cap">` since the previous block or box opened."""
    window = body[max(0, start - 600):start]
    cut = max(window.rfind("</pre>"), window.rfind("<div"))
    caps = CAPTION.findall(window[cut + 1:] if cut >= 0 else window)
    return caps[-1] if caps else ""


def highlight_pres(body: str, where: str = "", log: list | None = None) -> str:
    """Highlight every `<pre>` in a page body that has no token markup yet.
    `log`, when given, collects (where, language or None, reason) per block."""
    out, pos = [], 0
    for m in PRE.finditer(body):
        attrs, inner = m.group(1) or "", m.group(2)
        out.append(body[pos:m.start()])
        pos = m.end()
        if "<span class=" in inner:
            out.append(m.group(0))
            if log is not None:
                log.append((where, None, "marked"))
            continue
        dl = DATA_LANG.search(attrs)
        text = text_of(inner)
        lang = detect(text, caption_before(body, m.start()), dl.group(1) if dl else None)
        if lang is None:
            out.append(m.group(0))
            if log is not None:
                log.append((where, None, "plain"))
            continue
        new = _render(_segments(inner), tokens(text, lang))
        if text_of(new) != text:
            raise HighlightError(f"{where}: highlighting changed the text of a <pre> ({lang})")
        out.append(f"<pre{attrs}>{new}</pre>")
        if log is not None:
            log.append((where, lang, "highlighted"))
    out.append(body[pos:])
    return "".join(out)
