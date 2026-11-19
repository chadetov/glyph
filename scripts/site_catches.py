"""The site's view of catches/: the /catches/ page and every other place that
quotes it.

scripts/build_site.py calls `expand()` on each page body. A placeholder such
as `{{catches:mistakes}}` is replaced with markup generated from the committed
case files: `case.toml` for the text and scores, the `// show` excerpts of
`ts.ts` and `glyph.glyph` for the code, and `observed.json` for the output the
tools printed. scripts/check_catches.py keeps all three honest, so nothing a
page says about a case is typed twice, and the build itself runs no compiler.

Placeholders:

  {{catches:measured}}       the sentence with the counts by full-config verdict
  {{catches:legend}}         the score legend
  {{catches:filters}}        the filter chips (hidden until the script runs)
  {{catches:mistakes}}       every mistake, ranked
  {{catches:traps}}          the JavaScript traps, as a list
  {{catches:gaps}}           what Glyph does not catch yet, as a list
  {{catches:script}}         the filter and copy-link script
  {{catches:hero}}           one refusal as a TS / Glyph pair, for the home hero
  {{catches:top}}            the three highest-ranked mistakes nothing catches
  {{catches:refused-table}}  every refused mistake, for /hardening/
  {{catches:count}}, {{catches:none}}, {{catches:refused}}, {{catches:gap-count}}
                             single numbers

Python standard library only.
"""
from __future__ import annotations

import html
import json
import pathlib
import re
import tomllib
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parent.parent
CASES = ROOT / "catches"
REPO = "https://github.com/chadetov/glyph"

MARK_OPEN = "// show"
MARK_CLOSE = "// end show"
HELP_LIMIT = 200


class CatchesError(Exception):
    pass


# ---------------------------------------------------------------- data


def load() -> list[dict]:
    """Every case on the page, with its excerpts and observed output."""
    out = []
    for d in sorted(p for p in CASES.iterdir() if p.is_dir()):
        meta = tomllib.loads((d / "case.toml").read_text())
        if "section" not in meta:
            continue
        obs_path = d / "observed.json"
        if not obs_path.exists():
            raise CatchesError(
                f"catches/{d.name}/observed.json is missing; run "
                f"`python3 scripts/check_catches.py --write {d.name}`")
        meta["slug"] = d.name
        meta["kind"] = meta.get("kind", "refused")
        meta["observed"] = json.loads(obs_path.read_text())
        meta["ts_code"] = excerpt(d / "ts.ts")
        meta["glyph_code"] = excerpt(d / "glyph.glyph")
        out.append(meta)
    return out


def excerpt(path: pathlib.Path) -> str:
    """The lines between `// show` and `// end show`, each block dedented,
    blocks separated by one blank line."""
    blocks: list[list[str]] = []
    cur: list[str] | None = None
    for line in path.read_text().splitlines():
        if line.strip() == MARK_OPEN:
            cur = []
        elif line.strip() == MARK_CLOSE:
            if cur is not None:
                blocks.append(cur)
            cur = None
        elif cur is not None:
            cur.append(line)
    out = []
    for b in blocks:
        while b and not b[0].strip():
            b.pop(0)
        while b and not b[-1].strip():
            b.pop()
        depth = min((len(x) - len(x.lstrip()) for x in b if x.strip()), default=0)
        out.append("\n".join(x[depth:] for x in b))
    if not out:
        raise CatchesError(f"{path.relative_to(ROOT)} has no `// show` block")
    return "\n\n".join(out)


def score(c: dict) -> tuple:
    return (-c["likelihood"] * c["severity"], -c["severity"], c["order"])


def mistakes(cases: list[dict]) -> list[dict]:
    return sorted((c for c in cases if c["section"] == "mistake"), key=score)


def listed(cases: list[dict], section: str) -> list[dict]:
    return sorted((c for c in cases if c["section"] == section), key=lambda c: (c["order"], c["slug"]))


def ts_group(c: dict) -> str:
    return c["fullconfig"].split(":")[0]  # none | flag | lint


GLYPH_GROUPS = {
    "build": "refused when you build",
    "data": "checked when the data arrives",
    "warned": "warned",
    "run": "stopped when it runs",
    "design": "not in the language",
}


def glyph_group(c: dict) -> str:
    kind = c["kind"]
    if kind == "refused":
        return "build"
    if kind == "warned":
        return "warned"
    if kind == "runtime":
        return "run"
    # A boundary case either checks data on the way in, or does the right
    # thing because the mistake has no spelling in Glyph (no fallthrough, no
    # Date, == on records compares the data).
    if c.get("glyph_verdict") == GLYPH_GROUPS["data"]:
        return "data"
    return "design"


# ---------------------------------------------------------------- highlighting

TS_KEYWORDS = {
    "abstract", "as", "async", "await", "break", "case", "catch", "class", "const", "continue",
    "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for",
    "from", "function", "if", "implements", "import", "in", "instanceof", "interface", "is",
    "keyof", "let", "new", "null", "of", "private", "public", "readonly", "return", "static",
    "switch", "this", "throw", "true", "try", "type", "typeof", "undefined", "var", "void",
    "while", "yield",
}
GLYPH_KEYWORDS = {
    "async", "await", "break", "component", "continue", "else", "extern_ts", "false", "fn",
    "for", "import", "in", "is", "let", "match", "module", "mut", "owned", "pub", "resource",
    "return", "true", "type", "where", "while",
}
TOKEN = re.compile(
    r"(?P<c>//[^\n]*|/\*.*?\*/)"
    r"|(?P<s>\"(?:\\.|[^\"\\\n])*\"|'(?:\\.|[^'\\\n])*'|`(?:\\.|[^`\\])*`)"
    r"|(?P<w>[A-Za-z_][A-Za-z0-9_]*)"
    r"|(?P<o>.)",
    re.S,
)


def highlight(code: str, lang: str) -> str:
    keywords = TS_KEYWORDS if lang == "ts" else GLYPH_KEYWORDS
    out = []
    for m in TOKEN.finditer(code):
        text = html.escape(m.group(0), quote=False)
        if m.lastgroup == "c":
            out.append(f'<span class="c">{text}</span>')
        elif m.lastgroup == "s":
            out.append(f'<span class="s">{text}</span>')
        elif m.lastgroup == "w":
            word = m.group(0)
            if word in keywords:
                out.append(f'<span class="k">{text}</span>')
            elif word[0].isupper():
                out.append(f'<span class="t">{text}</span>')
            else:
                out.append(text)
        else:
            out.append(text)
    return "".join(out)


def strip_tags(s: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "", s))


# ---------------------------------------------------------------- pieces


def dots(n: int) -> str:
    return '<span class="c-dots" aria-hidden="true">' + "".join(
        '<i class="on"></i>' if i < n else "<i></i>" for i in range(5)) + "</span>"


def scores(c: dict) -> str:
    return (
        '<div class="c-scores">'
        f'<span class="c-score likely" title="Likely: {c["likelihood"]} of 5">Likely {dots(c["likelihood"])}'
        f'<span class="sr">{c["likelihood"]} of 5</span></span>'
        f'<span class="c-score severe" title="Severity: {c["severity"]} of 5">Severity {dots(c["severity"])}'
        f'<span class="sr">{c["severity"]} of 5</span></span>'
        "</div>"
    )


def fullconfig_tag(c: dict) -> str:
    fc = c["fullconfig"]
    if fc == "none":
        return ('<p class="c-cfg none"><b>Nothing catches it</b>'
                "not even with every compiler flag and typescript-eslint strict on</p>")
    if fc.startswith("flag:"):
        option, _, value = fc[len("flag:"):].partition("=")
        setting = f"{option}: {value}" if value else option
        return (f'<p class="c-cfg flag"><b>Off by default</b>'
                f"caught by the compiler flag <code>{html.escape(setting)}</code></p>")
    rule = fc[len("lint:"):]
    return (f'<p class="c-cfg lint"><b>Lint only</b>'
            f"caught by the lint rule <code>{html.escape(rule)}</code></p>")


def output_lines(text: str, cls: str) -> str:
    return "\n".join(f'<span class="{cls}">{html.escape(line, quote=False)}</span>' if line else ""
                     for line in text.splitlines())


def glyph_output(c: dict) -> tuple[str, str]:
    """(caption, highlighted output) for the Glyph side's observed run."""
    g = c["observed"].get("glyph")
    if not g:
        return "", ""
    lines = g["output"].splitlines()
    if g["command"] == "glyph build":
        head, help_ = lines[0], (lines[1] if len(lines) > 1 else "")
        cls = "w" if "] Warning:" in head else "e"
        body = output_lines(head, cls)
        if help_ and len(help_) <= HELP_LIMIT:
            body += "\n" + output_lines(help_, "h")
        return "$ glyph build", body
    cls = {"runtime": "e", "boundary": "o"}.get(c["kind"], "w")
    return "$ glyph run", output_lines(g["output"], cls)


def ts_output(c: dict) -> tuple[str, str]:
    t = c["observed"].get("ts")
    if not t:
        return "", ""
    return "$ tsx ts.ts", output_lines(t["output"], "e")


def code_box(file: str, code: str, lang: str, caption: str, out: str) -> str:
    box = (f'<div class="code c-code"><span class="cap">{file}</span>'
           f"<pre>{highlight(code, lang)}</pre>")
    if out:
        box += f'<span class="cap">{caption}</span><pre class="c-out">{out}</pre>'
    return box + "</div>"


def issue_link(c: dict) -> str:
    q = urllib.parse.urlencode({
        "template": "score.yml",
        "case": c["slug"],
        "title": f"Score: {strip_tags(c['page_title'])}",
    })
    return f"{REPO}/issues/new?{q}"


# ---------------------------------------------------------------- blocks


def mistake_html(c: dict, n: int) -> str:
    slug = c["slug"]
    ggroup = glyph_group(c)
    label = GLYPH_GROUPS[ggroup] if ggroup != "design" else c["glyph_verdict"]
    verdict = c["glyph_verdict"]
    extra = "" if verdict == label else f'<span class="c-verdict">{verdict}</span>'
    warn = " warn" if ggroup == "warned" else ""
    ts_cap, ts_out = ts_output(c)
    gl_cap, gl_out = glyph_output(c)
    return f"""      <article class="catch" id="{slug}" data-ts="{ts_group(c)}" data-glyph="{ggroup}">
        <div class="c-head">
          <span class="c-num">{n}</span>
          <h3><a class="c-anchor" href="#{slug}">{c["page_title"]}</a></h3>
          {scores(c)}
        </div>
        <p class="c-why">{c["why"]}</p>
        <div class="c-pair">
          <div class="c-side ts">
            <div class="c-tags"><span class="c-lang">TypeScript</span><span class="c-verdict">{c["ts_verdict"]}</span></div>
            {code_box("ts.ts", c["ts_code"], "ts", ts_cap, ts_out)}
            <p class="c-result">{c["ts_result"]}</p>
            {fullconfig_tag(c)}
          </div>
          <div class="c-side gl{warn}">
            <div class="c-tags"><span class="c-lang">Glyph</span><span class="c-kind">{label}</span>{extra}</div>
            {code_box("glyph.glyph", c["glyph_code"], "glyph", gl_cap, gl_out)}
            <p class="c-result">{c["glyph_result"]}</p>
          </div>
        </div>
        <p class="c-note">{c["page_note"]}</p>
        <p class="c-links"><a class="c-copy" href="#{slug}" data-slug="{slug}">Copy link</a><a href="{REPO}/tree/main/catches/{slug}">Reproduce</a><a class="c-disagree" href="{html.escape(issue_link(c))}">Disagree with this score?</a></p>
      </article>
"""


def counts(cases: list[dict]) -> dict[str, int]:
    ms = mistakes(cases)
    return {
        "count": len(ms),
        "none": sum(ts_group(c) == "none" for c in ms),
        "flag": sum(ts_group(c) == "flag" for c in ms),
        "lint": sum(ts_group(c) == "lint" for c in ms),
        "refused": sum(c["kind"] == "refused" for c in ms),
        "gap-count": len({c["gap_id"] for c in cases if c["section"] == "gap"}),
    }


def measured(cases: list[dict]) -> str:
    k = counts(cases)
    return (
        f"Every TypeScript example on the list passes <code>tsc --strict</code> with no errors. "
        f"Each one is also checked against a fully configured project: every compiler flag "
        f"stricter than <code>--strict</code>, plus typescript-eslint <code>strictTypeChecked</code>. "
        f"For {k['none']} of the {k['count']} cases, nothing in that setup catches it. "
        f"A compiler flag that is off by default catches {k['flag']}, and a lint rule "
        f"catches {k['lint']}. A project has to turn each of those on, and an agent can "
        f"turn it off again. In Glyph none of them is a setting."
    )


def legend() -> str:
    return """      <div class="c-legend">
        <span class="c-score likely"><b>Likely</b> <span class="c-dots" aria-hidden="true"><i class="on"></i><i class="on"></i><i class="on"></i><i></i><i></i></span> how often an AI agent writes it</span>
        <span class="c-score severe"><b>Severity</b> <span class="c-dots" aria-hidden="true"><i class="on"></i><i class="on"></i><i class="on"></i><i></i><i></i></span> how bad it is when it ships</span>
        <p>The scores are our judgement, 1 to 5, drawn from published studies of agent-written code and from our own agent runs. Nobody has measured these rates directly. The list is sorted by likelihood times severity.</p>
      </div>
"""


def filters(cases: list[dict]) -> str:
    ms = mistakes(cases)

    def chip(group: str, value: str, label: str) -> str:
        n = sum((ts_group(c) if group == "ts" else glyph_group(c)) == value for c in ms)
        return (f'<button type="button" class="c-chip" data-group="{group}" data-value="{value}" '
                f'aria-pressed="false">{label} <span>{n}</span></button>')

    ts_chips = "".join(chip("ts", v, l) for v, l in
                       (("none", "Nothing"), ("flag", "A compiler flag"), ("lint", "A lint rule")))
    gl_chips = "".join(chip("glyph", v, l) for v, l in
                       (("build", "Refused when you build"), ("data", "Checked when the data arrives"),
                        ("warned", "Warned"), ("run", "Stopped when it runs"),
                        ("design", "Not in the language")))
    return f"""      <div class="c-filters" hidden>
        <div class="c-fgroup" role="group" aria-label="What catches it in TypeScript"><span class="c-flabel">What catches it in TypeScript</span>{ts_chips}</div>
        <div class="c-fgroup" role="group" aria-label="How Glyph handles it"><span class="c-flabel">How Glyph handles it</span>{gl_chips}</div>
        <p class="c-fstatus" aria-live="polite"></p>
      </div>
"""


def mistakes_html(cases: list[dict]) -> str:
    return '      <div class="c-list">\n' + "".join(
        mistake_html(c, i) for i, c in enumerate(mistakes(cases), 1)) + "      </div>\n"


def traps_html(cases: list[dict]) -> str:
    items = "".join(f'        <li id="{c["slug"]}">{c["summary"]}</li>\n' for c in listed(cases, "trap"))
    return f'      <ul class="c-traps">\n{items}      </ul>\n'


def gaps_html(cases: list[dict]) -> str:
    groups: dict[str, list[str]] = {}
    for c in listed(cases, "gap"):
        groups.setdefault(c["summary"], []).append(c["gap_id"])
    items = "".join(f'        <li>{summary} <span>{", ".join(ids)}</span></li>\n'
                    for summary, ids in groups.items())
    return f'      <ul class="c-gaps">\n{items}      </ul>\n'


def hero(cases: list[dict]) -> str:
    """The highest-ranked mistake Glyph's own checker refuses that nothing in a
    fully configured TypeScript project catches."""
    pick = next((c for c in mistakes(cases)
                 if c["kind"] == "refused" and c["fullconfig"] == "none" and c["code"].startswith("E")),
                None)
    if pick is None:
        raise CatchesError("no refused mistake with fullconfig = none for the home hero")
    ts_cap, ts_out = ts_output(pick)
    gl_cap, gl_out = glyph_output(pick)
    return f"""          <figure class="c-hero">
            <figcaption><a href="/catches/#{pick["slug"]}">{pick["page_title"]}</a>: {pick["why"]}</figcaption>
            <div class="c-hero-side ts">
              <div class="c-tags"><span class="c-lang">TypeScript</span><span class="c-verdict">{pick["ts_verdict"]}</span></div>
              {code_box("ts.ts", pick["ts_code"], "ts", ts_cap, ts_out)}
            </div>
            <div class="c-hero-side gl">
              <div class="c-tags"><span class="c-lang">Glyph</span><span class="c-kind">refused when you build</span></div>
              {code_box("glyph.glyph", pick["glyph_code"], "glyph", gl_cap, gl_out)}
            </div>
          </figure>
"""


def top(cases: list[dict]) -> str:
    picks = [c for c in mistakes(cases) if c["fullconfig"] == "none"][:3]
    cards = []
    for i, c in enumerate(picks, 1):
        label = GLYPH_GROUPS[glyph_group(c)] if glyph_group(c) != "design" else c["glyph_verdict"]
        cards.append(f"""          <a class="c-card" href="/catches/#{c["slug"]}">
            <span class="c-card-n">{i}</span>
            <h3>{c["page_title"]}</h3>
            <p>{c["why"]}</p>
            <p class="c-card-v"><span class="ts">TypeScript: {c["ts_verdict"]}</span><span class="gl">Glyph: {label}</span></p>
          </a>
""")
    return '        <div class="c-cards">\n' + "".join(cards) + "        </div>\n"


def refused_table(cases: list[dict]) -> str:
    rows = []
    for c in mistakes(cases):
        if c["kind"] != "refused":
            continue
        code = f"<code>{c['code']}</code>"
        if c["code"].startswith("TS"):
            code += ' <span class="muted">(tsc pass)</span>'
        rows.append(
            f'            <tr><td>{code}</td><td><a href="/catches/#{c["slug"]}">{c["page_title"]}</a></td>'
            f'<td><a href="{REPO}/tree/main/catches/{c["slug"]}"><code>catches/{c["slug"]}/</code></a></td></tr>\n')
    return ('        <table class="catches-table">\n'
            "          <thead><tr><th>Glyph says</th><th>What TypeScript let through</th><th>Files</th></tr></thead>\n"
            "          <tbody>\n" + "".join(rows) + "          </tbody>\n        </table>\n")


SCRIPT = """    <script>
      (function () {
        var bar = document.querySelector(".c-filters");
        var items = Array.prototype.slice.call(document.querySelectorAll(".catch"));
        var status = document.querySelector(".c-fstatus");
        var chips = Array.prototype.slice.call(document.querySelectorAll(".c-chip"));
        var state = { ts: "", glyph: "" };
        function read() {
          var q = new URLSearchParams(location.search);
          state.ts = q.get("ts") || "";
          state.glyph = q.get("glyph") || "";
        }
        function write() {
          var q = new URLSearchParams(location.search);
          ["ts", "glyph"].forEach(function (k) {
            if (state[k]) q.set(k, state[k]); else q.delete(k);
          });
          var s = q.toString();
          history.replaceState(null, "", location.pathname + (s ? "?" + s : "") + location.hash);
        }
        function apply() {
          var shown = 0;
          items.forEach(function (el) {
            var ok = (!state.ts || el.dataset.ts === state.ts) && (!state.glyph || el.dataset.glyph === state.glyph);
            el.hidden = !ok;
            if (ok) shown++;
          });
          chips.forEach(function (c) {
            c.setAttribute("aria-pressed", String(state[c.dataset.group] === c.dataset.value));
          });
          status.textContent = state.ts || state.glyph
            ? "Showing " + shown + " of " + items.length + ". Click a selected filter again to clear it."
            : "";
        }
        if (bar) {
          bar.hidden = false;
          read();
          apply();
          chips.forEach(function (c) {
            c.addEventListener("click", function () {
              var g = c.dataset.group;
              state[g] = state[g] === c.dataset.value ? "" : c.dataset.value;
              write();
              apply();
            });
          });
        }
        document.querySelectorAll(".c-copy").forEach(function (a) {
          a.addEventListener("click", function (e) {
            var url = location.origin + location.pathname + "#" + a.dataset.slug;
            if (!navigator.clipboard) return;
            e.preventDefault();
            history.replaceState(null, "", "#" + a.dataset.slug);
            navigator.clipboard.writeText(url).then(function () {
              var prev = a.textContent;
              a.textContent = "Link copied";
              setTimeout(function () { a.textContent = prev; }, 1400);
            });
          });
        });
      })();
    </script>
"""


def expand(body: str, where: str) -> str:
    """Replace every `{{catches:...}}` placeholder in a page body."""
    if "{{catches:" not in body:
        return body
    cases = load()
    k = counts(cases)
    blocks = {
        "measured": lambda: measured(cases),
        "legend": legend,
        "filters": lambda: filters(cases),
        "mistakes": lambda: mistakes_html(cases),
        "traps": lambda: traps_html(cases),
        "gaps": lambda: gaps_html(cases),
        "script": lambda: SCRIPT,
        "hero": lambda: hero(cases),
        "top": lambda: top(cases),
        "refused-table": lambda: refused_table(cases),
        **{name: (lambda v=v: str(v)) for name, v in k.items()},
    }

    def sub(m: re.Match) -> str:
        name = m.group(1)
        if name not in blocks:
            raise CatchesError(f"{where}: unknown placeholder {{{{catches:{name}}}}}")
        return blocks[name]()
    return re.sub(r"\{\{catches:([a-z-]+)\}\}", sub, body)
