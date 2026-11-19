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

  - a segments file (scripts/i18n_extract.py) that is not valid JSON, or a
    translation in it that changes a tag, a <code> element's content or a
    placeholder; and a translated page that is not what scripts/i18n_apply.py
    writes from its current segments file (someone edited the page by hand)
  - a catches.json text, or a UI string with markup, that breaks the same rules

Reports, without failing, what is only out of date or not done yet:
  - a translation whose source_hash no longer matches its English page (the
    page itself shows a notice and links to English; an English edit is never
    blocked on nine translations)
  - UI strings, sub-nav labels and case texts a locale has not translated yet
    (they show in English)
  - coverage: for each locale, how many of the translatable pages exist, how
    many are stale, and the segments files still in progress. A page with no
    translation is not an error: an English page can ship before its
    translations, and its language menu says "not translated yet" meanwhile.

    python3 scripts/check_i18n.py                 # check every locale
    python3 scripts/check_i18n.py --locale es     # one locale, and render its pages in memory
    python3 scripts/check_i18n.py --hash PATH     # the source_hash for an English page

--locale reads only that locale's files and site/i18n/en/, renders the
English site and that locale's pages in memory, and writes nothing, so a
translator can run it while others work on other locales in the same tree.

Python standard library only.
"""
from __future__ import annotations

import sys

sys.dont_write_bytecode = True

import argparse  # noqa: E402
import json  # noqa: E402
import pathlib  # noqa: E402
import re  # noqa: E402
from html.parser import HTMLParser  # noqa: E402

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
FREE = {"{{version}}", "{{releases:count}}", "{{releases:breaking}}", "{{releases:latest}}",
        "{{catches:measured}}", "{{catches:count}}", "{{catches:none}}",
        "{{catches:refused}}", "{{catches:gap-count}}", "{{catches:flag}}", "{{catches:lint}}",
        "{{catches:total}}", "{{catches:trap-count}}", "{{catches:gap-cases}}",
        "{{catches:traps-refused}}", "{{catches:refused-all}}"}


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


def page_problems(ebody: str, tbody: str) -> list[str]:
    """What is broken in a translated body against its English body."""
    out = []
    esk, sk = skeleton(ebody), skeleton(tbody)
    if sk.bad or sk.stack:
        out.append(f"not well-formed (bad={sk.bad[:3]} unclosed={sk.stack[-3:]})")
    if sk.shape != esk.shape:
        out.append(f"structure differs from English: {first_diff(esk.shape, sk.shape)}")
    if blocks(tbody) != blocks(ebody):
        out.append(f"generated blocks differ from English: {blocks(tbody)} vs {blocks(ebody)}")
    if pres(tbody) != pres(ebody):
        out.append("a <pre> block differs from English; code and output stay English")
    return out


def check_segments(loc: str, translations: dict, errors: list[str], notes: list[str]) -> dict:
    """Validate a locale's segments files; return {page rel: (done, total)}
    for the ones in progress."""
    import i18n_apply
    import i18n_extract as ex
    progress = {}
    base = site_i18n.I18N / loc
    for spath in sorted(base.rglob("*.segments.json")):
        where = spath.relative_to(ROOT)
        rel = spath.relative_to(base).as_posix().removesuffix(".segments.json") + ".html"
        english = site_i18n.PAGES / rel
        if not english.is_file():
            errors.append(f"{where}: no English page site/pages/{rel}")
            continue
        try:
            data = json.loads(spath.read_text())
            segs = data["segments"]
            assert isinstance(segs, list) and all(isinstance(x, dict) and "id" in x for x in segs)
        except (json.JSONDecodeError, KeyError, AssertionError) as e:
            errors.append(f"{where}: not a segments file ({type(e).__name__}: {e})")
            continue
        for seg in segs:
            tr = seg.get("tr") or ""
            if tr.strip():
                errors += [f"{where}: {m}" for m in
                           i18n_apply.fragment_errors(seg["id"], seg.get("kind", "html"), seg.get("en", ""), tr)]
        current = data.get("source_hash") == site_i18n.english_hash(english)
        done = sum(bool((x.get("tr") or "").strip()) for x in segs)
        tpath = ex.translation_path(english, loc)
        if done < len(segs) or not current:
            progress[rel] = (done, len(segs), current)
        if current and done == len(segs):
            try:
                text, errs, missing = i18n_apply.build_page(english, loc, data)
            except Exception as e:  # noqa: BLE001 - reported, not raised
                errors.append(f"{where}: cannot be applied: {e}")
                continue
            if errs or missing:
                errors += [f"{where}: {m}" for m in errs]
                if missing:
                    progress[rel] = (done, len(segs), current)
            elif not tpath.is_file():
                notes.append(f"{where}: complete; run python3 scripts/i18n_apply.py {rel} {loc}")
            elif tpath.read_text() != text:
                errors.append(f"{tpath.relative_to(ROOT)}: not what scripts/i18n_apply.py writes from "
                              f"{where}; edit the segments file, not the page, then run "
                              f"python3 scripts/i18n_apply.py {rel} {loc}")
    return progress


def check_catches(loc: str, errors: list[str], notes: list[str]) -> tuple[int, int, int]:
    """Validate site/i18n/<loc>/catches.json; return (current translations,
    stale ones, total texts)."""
    import i18n_apply
    import i18n_extract as ex
    english = ex.case_english()
    total = sum(len(f) for f in english.values())
    path = site_i18n.I18N / loc / "catches.json"
    if not path.is_file():
        return 0, 0, total
    where = path.relative_to(ROOT)
    try:
        cases = json.loads(path.read_text())["cases"]
    except (json.JSONDecodeError, KeyError) as e:
        errors.append(f"{where}: not a catches file ({type(e).__name__}: {e})")
        return 0, 0, total
    done = stale = 0
    for slug, fields in cases.items():
        if slug not in english:
            notes.append(f"{where}: case {slug!r} is not in catches/ (unused)")
            continue
        for field, entry in fields.items():
            tr = (entry.get("tr") or "") if isinstance(entry, dict) else ""
            if not tr.strip():
                continue
            if field not in english[slug]:
                notes.append(f"{where}: {slug}.{field} is not a text of that case (unused)")
                continue
            errors += [f"{where}: {m}" for m in
                       i18n_apply.fragment_errors(f"{slug}.{field}", "html", english[slug][field], tr)]
            if entry.get("en") == english[slug][field]:
                done += 1
            else:
                stale += 1
    return done, stale, total


def subnav_texts() -> set[str]:
    out = set()
    for part in sorted(site_i18n.SITE.glob("partials/subnav-*.html")):
        for g, _, label in build_site.subnav_entries(part.stem.removeprefix("subnav-"), str(part)):
            out.add(re.sub(r'<span class="n">[^<]*</span>\s*', "", label))
            if g:
                out.add(g)
    return out


def check(errors: list[str], notes: list[str], locales: list[str] | None = None) -> None:
    import i18n_apply
    import i18n_extract as ex
    try:
        translations = site_i18n.discover(build_site.parse_front, locales)
    except (site_i18n.I18nError, build_site.SiteError) as e:
        errors.append(str(e))
        return

    stale = []
    for eng, by_loc in sorted(translations.items()):
        efm, ebody = (build_site.parse_front(eng) if eng == build_site.NOT_FOUND
                      else build_site.parse_page(eng))
        for loc, tr in sorted(by_loc.items()):
            where = tr.path.relative_to(ROOT)
            errors += [f"{where}: {p}" for p in page_problems(ebody, tr.body)]
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
    sub_texts = subnav_texts()
    scope = locales or site_i18n.TRANSLATED
    coverage = []
    pages_total = ex.translatable_pages()
    for loc in scope:
        progress = check_segments(loc, translations, errors, notes)
        c_done, c_stale, c_total = check_catches(loc, errors, notes)
        have = [eng for eng, by in translations.items() if loc in by]
        n_stale = sum(by[loc].stale for by in translations.values() if loc in by)
        line = (f"  {loc}: {len(have)} of {len(pages_total)} pages translated"
                f"{f' ({n_stale} stale)' if n_stale else ''}; case texts {c_done} of {c_total}"
                f"{f' ({c_stale} stale)' if c_stale else ''}")
        coverage.append(line)
        for rel, (done, total, current) in sorted(progress.items()):
            coverage.append(f"    in progress: {rel} {done} of {total} segments"
                            f"{'' if current else ' (English changed: re-run i18n_extract.py)'}")
    for loc in scope:
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
        ui, nav, sub = data["ui"], data["nav"], data["subnav"]
        for key, value in ui.items():
            if key not in en:
                notes.append(f"site/i18n/{loc}/strings.json: ui key {key!r} is not in English (unused)")
            elif fields(value) != fields(en[key]):
                errors.append(f"site/i18n/{loc}/strings.json: {key!r} has {sorted(fields(value))}, "
                              f"English has {sorted(fields(en[key]))}")
            elif "<" in en[key] or "<" in value:
                errors += [f"site/i18n/{loc}/strings.json: {m}" for m in
                           i18n_apply.fragment_errors(key, "html", en[key], value)]
            elif key == "date.months" and len(value.split()) != 12:
                errors.append(f"site/i18n/{loc}/strings.json: 'date.months' needs twelve names "
                              f"separated by spaces")
        for key, value in sub.items():
            if key not in sub_texts:
                notes.append(f"site/i18n/{loc}/strings.json: subnav entry {key!r} matches nothing in "
                             f"site/partials/subnav-*.html (unused)")
            else:
                errors += [f"site/i18n/{loc}/strings.json: subnav {m}" for m in
                           i18n_apply.fragment_errors(key, "html", key, value)]
        for key, value in nav.items():
            if key not in nav_texts:
                notes.append(f"site/i18n/{loc}/strings.json: nav entry {key!r} matches nothing in site.json (unused)")
            elif placeholders(key) != placeholders(value):
                errors.append(f"site/i18n/{loc}/strings.json: nav entry {key!r} changes its {{{{...}}}} placeholders")
        missing_ui = sorted(set(en) - set(ui))
        missing_nav = sorted(nav_texts - set(nav) - {"GitHub"})
        missing_sub = sorted(sub_texts - set(sub))
        if missing_ui or missing_nav or missing_sub:
            notes.append(f"site/i18n/{loc}/strings.json: {len(missing_ui)} ui, {len(missing_nav)} nav and "
                         f"{len(missing_sub)} subnav strings not translated (shown in English): "
                         f"{(missing_ui + missing_nav + missing_sub)[:6]}")
    if coverage:
        notes.append("coverage (pages exclude the release notes, which stay English):")
        notes += coverage

    files = [p for _, p in site_i18n.translation_files(locales)]
    for loc in (locales or site_i18n.LOCALES):
        base = site_i18n.I18N / loc
        files += [base / "strings.json", base / "catches.json"]
        files += sorted(base.rglob("*.segments.json")) if base.is_dir() else []
    for path in files:
        if not path.is_file():
            continue
        text = path.read_text().lower()
        if path.name.endswith(".json"):
            # Only what a reader sees: the English side of a segments or
            # catches file is the English page, already checked as such.
            text = "\n".join(re.findall(r'"(?:tr|was_tr)": "((?:[^"\\]|\\.)*)"', text)) if (
                path.name != "strings.json") else text
        for lang, phrases in FORBIDDEN.items():
            for phrase in phrases:
                if phrase in text:
                    errors.append(f"{path.relative_to(ROOT)}: says {phrase!r} ({lang}); a translated page "
                                  f"must not say or imply a machine produced it")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--hash", metavar="PATH", help="print the source_hash of an English source page")
    ap.add_argument("--locale", action="append", choices=site_i18n.TRANSLATED,
                    help="check only this locale (repeatable), and render its pages in memory")
    args = ap.parse_args()
    if args.hash:
        print(site_i18n.english_hash(pathlib.Path(args.hash).resolve()))
        return 0
    errors: list[str] = []
    notes: list[str] = []
    check(errors, notes, args.locale)
    rendered = 0
    if args.locale and not errors:
        # Render English and these locales exactly as the build would, in
        # memory: a translation that parses but cannot render fails here
        # rather than in the full build.
        try:
            files = build_site.build(args.locale)
            rendered = sum(1 for k in files if k.endswith(".html") and k.split("/")[0] in args.locale)
        except build_site.SiteError as e:
            errors.append(f"render: {e}")
    for n in notes:
        print(n)
    if errors:
        print("i18n check FAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1
    n = len(site_i18n.translation_files(args.locale))
    scope = f" for {', '.join(args.locale)}; {rendered} pages rendered in memory, nothing written" if args.locale else ""
    print(f"i18n check OK ({n} translated pages{scope}; structure, placeholders, code blocks and wording)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
