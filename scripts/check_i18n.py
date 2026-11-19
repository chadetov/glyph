#!/usr/bin/env python3
"""The site's translations are whole, honest, and say how current they are.

Fails (exit 1) on what is broken:
  - a translation with no English page, or of a page that is never translated
    (the release notes), or under an unknown locale directory
  - front matter that does not parse, sets a structural key (layout, nav, ...),
    or lacks title, description or source_hash
  - a body whose structure differs from the English page: the sequence of
    block elements and classed elements, the {{...}} placeholders that
    expand to generated blocks (numbers may move freely), or any
    <pre> block (code and compiler output stay English, byte for byte)
  - HTML that is not well-formed
  - a strings.json that is not valid JSON, or a translated string that drops
    or invents a {name} the generator fills in
  - any wording, in any of the site's languages, that says the translation
    came from a machine (the notice on a translated page is neutral)

Reports, without failing, what is only out of date:
  - a translation whose source_hash no longer matches its English page (the
    page itself shows a notice and links to English; an English edit is never
    blocked on nine translations)
  - UI strings a locale has not translated yet (they show in English)

    python3 scripts/check_i18n.py                 # check
    python3 scripts/check_i18n.py --hash PATH     # the source_hash for an English page

Python standard library only.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from html.parser import HTMLParser

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build_site  # noqa: E402
import site_i18n  # noqa: E402

ROOT = site_i18n.ROOT

# Wording that would say or imply a machine produced a translation. Matched
# case-insensitively against every translated page and strings file, every
# list against every file, since a sentence can borrow another language's term.
FORBIDDEN = {
    "en": ["machine translat", "machine-translat", "ai translat", "ai-translat", "auto-translat",
           "automatically translated", "translated automatically", "translated by ai",
           "translated by a model", "google translate", "translated with deepl", "deepl translator"],
    "pt-br": ["tradução automática", "traduzido automaticamente", "traduzida automaticamente",
              "tradução por ia", "traduzido por ia", "traduzida por ia", "tradução de máquina"],
    "es": ["traducción automática", "traducido automáticamente", "traducida automáticamente",
           "traducción por ia", "traducido por ia", "traducida por ia"],
    "ja": ["機械翻訳", "自動翻訳", "ai翻訳", "aiによる翻訳", "aiで翻訳", "aiが翻訳"],
    "tr": ["makine çevirisi", "otomatik çeviri", "otomatik olarak çevril", "yapay zeka çevirisi",
           "yapay zekâ çevirisi", "yapay zeka ile çevril", "yapay zekâ ile çevril"],
    "zh-cn": ["机器翻译", "自动翻译", "机翻", "ai翻译", "ai 翻译", "人工智能翻译"],
    "ko": ["기계 번역", "기계번역", "자동 번역", "자동번역", "ai 번역", "ai번역", "인공지능 번역"],
    "de": ["maschinelle übersetzung", "maschinell übersetzt", "automatisch übersetzt",
           "automatische übersetzung", "ki-übersetzung", "von ki übersetzt", "per ki übersetzt"],
    "ru": ["машинный перевод", "машинным переводом", "автоматический перевод", "переведено автоматически",
           "автоматически переведен", "переведено ии", "переведено с помощью ии", "перевод ии"],
    "th": ["แปลด้วยเครื่อง", "แปลอัตโนมัติ", "การแปลอัตโนมัติ", "แปลโดย ai", "แปลโดยเอไอ", "เครื่องแปล"],
}

BLOCK = {"section", "div", "header", "footer", "main", "article", "aside", "nav", "figure",
         "h1", "h2", "h3", "h4", "p", "pre", "ul", "ol", "li", "table", "tr", "td", "th",
         "details", "summary", "button", "script"}
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
        "param", "source", "track", "wbr"}


class Skeleton(HTMLParser):
    """The structural outline of a body, and whether its tags balance."""

    def __init__(self):
        super().__init__(convert_charrefs=False)
        self.shape: list[str] = []
        self.stack: list[str] = []
        self.bad: list[str] = []

    def handle_starttag(self, tag, attrs):
        cls = dict(attrs).get("class")
        if tag in BLOCK or cls:
            self.shape.append(f"{tag}.{cls}" if cls else tag)
        if tag not in VOID:
            self.stack.append(tag)

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        if self.stack and self.stack[-1] == tag:
            self.stack.pop()
        else:
            self.bad.append(tag)


def skeleton(body: str) -> Skeleton:
    sk = Skeleton()
    sk.feed(body)
    sk.close()
    return sk


def pres(body: str) -> list[str]:
    return re.findall(r"<pre\b[^>]*>.*?</pre>", body, re.S)


def placeholders(text: str) -> list[str]:
    return sorted(re.findall(r"\{\{[a-z_]+(?::[a-z-]+)?\}\}", text))


# Placeholders a translation may use freely: single numbers, which grammar
# places differently in each language, and the generated English sentence
# with the catches counts, which a translation writes out in its own words
# around those numbers.
FREE = {"{{version}}", "{{catches:measured}}", "{{catches:count}}", "{{catches:none}}",
        "{{catches:refused}}", "{{catches:gap-count}}", "{{catches:flag}}", "{{catches:lint}}"}


def blocks(text: str) -> list[str]:
    """The placeholders that expand to generated markup, which a translation
    has to keep."""
    return sorted(set(placeholders(text)) - FREE)


def fields(text: str) -> set[str]:
    return set(re.findall(r"\{([a-z_]+)\}", text))


def first_diff(a: list[str], b: list[str]) -> str:
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return f"element {i + 1}: English has <{x}>, the translation <{y}>"
    return f"English has {len(a)} structural elements, the translation {len(b)}"


def check(errors: list[str], notes: list[str]) -> None:
    try:
        translations = site_i18n.discover(build_site.parse_front)
    except (site_i18n.I18nError, build_site.SiteError) as e:
        errors.append(str(e))
        return

    stale = []
    for eng, by_loc in sorted(translations.items()):
        efm, ebody = build_site.parse_page(eng)
        esk = skeleton(ebody)
        for loc, tr in sorted(by_loc.items()):
            where = tr.path.relative_to(ROOT)
            sk = skeleton(tr.body)
            if sk.bad or sk.stack:
                errors.append(f"{where}: not well-formed (bad={sk.bad[:3]} unclosed={sk.stack[-3:]})")
            if sk.shape != esk.shape:
                errors.append(f"{where}: structure differs from {eng.relative_to(ROOT)}: "
                              f"{first_diff(esk.shape, sk.shape)}")
            if blocks(tr.body) != blocks(ebody):
                errors.append(f"{where}: generated blocks differ from English: "
                              f"{blocks(tr.body)} vs {blocks(ebody)}")
            if pres(tr.body) != pres(ebody):
                errors.append(f"{where}: a <pre> block differs from English; code and output stay English")
            for key in ("next_steps", "prev", "next"):
                if key in tr.fm and key not in efm:
                    errors.append(f"{where}: sets {key!r}, which the English page does not have")
            if tr.stale:
                stale.append(f"{where} (source_hash {tr.fm['source_hash']}, English is now "
                             f"{site_i18n.english_hash(eng)})")
    if stale:
        notes.append("translations older than their English page (each shows a notice and links to English):")
        notes += [f"  {s}" for s in stale]

    en = site_i18n.read_strings("en")["ui"]
    site = json.loads((site_i18n.SITE / "site.json").read_text())
    nav_texts = set()
    for entry in site["nav"]:
        nav_texts.add(entry["label"])
        for col in entry.get("columns", []):
            nav_texts.add(col["title"])
            for it in col["items"]:
                nav_texts |= {it["label"], it["desc"]}
        if entry.get("featured"):
            nav_texts |= {entry["featured"][k] for k in ("big", "text", "cta")}
    nav_texts |= {a["label"] for a in site["actions"].values()}
    for loc in site_i18n.TRANSLATED:
        path = site_i18n.I18N / loc / "strings.json"
        if not path.is_file():
            if loc in {t for by in translations.values() for t in by}:
                notes.append(f"site/i18n/{loc}/strings.json is missing; the chrome of its pages is English")
            continue
        try:
            data = site_i18n.read_strings(loc)
        except site_i18n.I18nError as e:
            errors.append(str(e))
            continue
        ui, nav = data["ui"], data["nav"]
        for key, value in ui.items():
            if key not in en:
                notes.append(f"site/i18n/{loc}/strings.json: ui key {key!r} is not in English (unused)")
            elif fields(value) != fields(en[key]):
                errors.append(f"site/i18n/{loc}/strings.json: {key!r} has {sorted(fields(value))}, "
                              f"English has {sorted(fields(en[key]))}")
        for key, value in nav.items():
            if key not in nav_texts:
                notes.append(f"site/i18n/{loc}/strings.json: nav entry {key!r} matches nothing in site.json (unused)")
            elif placeholders(key) != placeholders(value):
                errors.append(f"site/i18n/{loc}/strings.json: nav entry {key!r} changes its {{{{...}}}} placeholders")
        missing_ui = sorted(set(en) - set(ui))
        missing_nav = sorted(nav_texts - set(nav) - {"GitHub"})
        if missing_ui or missing_nav:
            notes.append(f"site/i18n/{loc}/strings.json: {len(missing_ui)} ui and {len(missing_nav)} nav "
                         f"strings not translated (shown in English): {(missing_ui + missing_nav)[:6]}")

    files = [p for _, p in site_i18n.translation_files()]
    files += [site_i18n.I18N / loc / "strings.json" for loc in site_i18n.LOCALES]
    for path in files:
        if not path.is_file():
            continue
        text = path.read_text().lower()
        for lang, phrases in FORBIDDEN.items():
            for phrase in phrases:
                if phrase in text:
                    errors.append(f"{path.relative_to(ROOT)}: says {phrase!r} ({lang}); a translated page "
                                  f"must not say or imply a machine produced it")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--hash", metavar="PATH", help="print the source_hash of an English source page")
    args = ap.parse_args()
    if args.hash:
        print(site_i18n.english_hash(pathlib.Path(args.hash).resolve()))
        return 0
    errors: list[str] = []
    notes: list[str] = []
    check(errors, notes)
    for n in notes:
        print(n)
    if errors:
        print("i18n check FAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1
    n = len(site_i18n.translation_files())
    print(f"i18n check OK ({n} translated pages; structure, placeholders, code blocks and wording)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
