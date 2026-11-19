"""The site's translations: locales, UI strings, translated pages, and the
chrome that ties the languages together.

English is the reference and lives at the root. A translation of the page whose
source is site/pages/<path>/index.html lives at
site/i18n/<locale>/<path>/index.html and is served at /<locale>/<path>/. Only
pages someone translated exist under a locale; there are no stub copies of
English pages. A link in a translated page's body or chrome goes to that
locale's version when one exists and to the English page when it does not.

A translation's front matter carries the keys a translator changes (title,
description, head, next_steps, prev, next) and `source_hash`, the hash of the
English source file it was translated from (`english_hash()`: the first 16 hex
digits of the SHA-256 of the whole file, front matter included, so a changed
English title marks the translation stale too). Everything structural (layout,
nav, subnav, article_class, sitemap) comes from the English page, so the two
cannot disagree about where a page sits. When the English file changes, the
hash stops matching and the translated page renders a notice that the English
version has changed since, with a link to it. scripts/check_i18n.py reports
those and fails on the things that are broken rather than out of date.

UI strings live in site/i18n/<locale>/strings.json, two objects:

  "ui"   keyed strings for the partials and the generator, written in a
         partial as {{t:key}} (HTML-escaped) or {{tj:key}} (a JavaScript
         string literal). site/i18n/en/strings.json holds the English ones.
  "nav"  the site.json nav, gettext style: the English label, description,
         column title or card text is the key, so site.json stays the one
         place the nav is defined. A label with no entry shows in English.

A missing ui key also falls back to English, so adding an English string
never blocks a build; check_i18n.py lists what each locale lacks.

Python standard library only.
"""
from __future__ import annotations

import hashlib
import html
import json
import pathlib
import re
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
PAGES = SITE / "pages"
I18N = SITE / "i18n"
ORIGIN = "https://glyphlang.io"
REPO = "https://github.com/chadetov/glyph"

# URL segment -> (BCP 47 tag for lang and hreflang, name in its own script,
# short label for the narrow header). zh-cn is written as zh-Hans: the text is
# Simplified Chinese for every Simplified reader, not a China-specific
# localization, and the script subtag is what browsers use to pick glyphs.
# The flag shown beside each language in the language menu. A flag stands for
# a country, so each language keeps its name next to it.
FLAGS = {"en": "gb", "pt-br": "br", "es": "es", "ja": "jp", "tr": "tr",
         "zh-cn": "cn", "ko": "kr", "de": "de", "ru": "ru", "th": "th"}


def flag(loc: str) -> str:
    return (f'<img class="lang-flag" src="/flags/{FLAGS[loc]}.svg" width="20" height="15" '
            f'alt="" aria-hidden="true" loading="lazy" decoding="async">')


LOCALES: dict[str, tuple[str, str, str]] = {
    "en": ("en", "English", "EN"),
    "pt-br": ("pt-BR", "Português (Brasil)", "PT"),
    "es": ("es", "Español", "ES"),
    "ja": ("ja", "日本語", "日本"),
    "tr": ("tr", "Türkçe", "TR"),
    "zh-cn": ("zh-Hans", "简体中文", "中文"),
    "ko": ("ko", "한국어", "한국"),
    "de": ("de", "Deutsch", "DE"),
    "ru": ("ru", "Русский", "RU"),
    "th": ("th", "ไทย", "ไทย"),
}
TRANSLATED = [loc for loc in LOCALES if loc != "en"]

# Open Graph wants language_TERRITORY. The territory is the usual one for each
# language; og:locale only tells a preview which language the text is in.
OG_LOCALES = {"en": "en_US", "pt-br": "pt_BR", "es": "es_ES", "ja": "ja_JP", "tr": "tr_TR",
              "zh-cn": "zh_CN", "ko": "ko_KR", "de": "de_DE", "ru": "ru_RU", "th": "th_TH"}

# Script fonts, loaded from Google Fonts only on pages of their locale. IBM
# Plex Sans and Mono are served from /fonts/ (site/fonts.css) and cover Latin,
# Latin Extended and Cyrillic, so pt-br, es, tr, de and ru need none.
FONTS = {
    "ja": ("IBM Plex Sans JP", "IBM+Plex+Sans+JP:wght@400;500;600;700"),
    "ko": ("IBM Plex Sans KR", "IBM+Plex+Sans+KR:wght@400;500;600;700"),
    "th": ("IBM Plex Sans Thai", "IBM+Plex+Sans+Thai:wght@400;500;600;700"),
    "zh-cn": ("Noto Sans SC", "Noto+Sans+SC:wght@400;500;600;700"),
}

# The front-matter keys a translation may set. The rest come from English.
TRANSLATION_KEYS = {"title", "description", "head", "next_steps", "prev", "next", "source_hash"}

# Pages that are never translated: the release notes are a changelog.
UNTRANSLATED = ("versions/",)

STORE = "glyph-lang"        # localStorage: the reader's chosen locale
AUTO_FLAG = "glyph-lang-auto"  # sessionStorage: this page was chosen for them


class I18nError(Exception):
    pass


def english_hash(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()[:16]


def url_for(loc: str, url: str) -> str:
    return url if loc == "en" else f"/{loc}{url}"


# ---------------------------------------------------------------- strings


class Strings:
    """One locale's UI strings, falling back to English key by key."""

    def __init__(self, loc: str, ui: dict[str, str], nav: dict[str, str], en: dict[str, str]):
        self.loc, self.ui, self.nav_map, self.en = loc, ui, nav, en

    def raw(self, key: str) -> str:
        if key in self.ui:
            return self.ui[key]
        if key in self.en:
            return self.en[key]
        raise I18nError(f"no UI string {key!r} in site/i18n/en/strings.json")

    def __call__(self, key: str, **values: str) -> str:
        """The string for `key`, HTML-escaped, with {name} filled from values
        (which are inserted as given, so they may be markup)."""
        text = html.escape(self.raw(key), quote=False).replace('"', "&quot;")
        for name, value in values.items():
            text = text.replace("{" + name + "}", value)
        return text

    def js(self, key: str) -> str:
        return json.dumps(self.raw(key), ensure_ascii=False).replace("</", "<\\/")

    def nav(self, english: str) -> str:
        """A site.json nav string: already markup in English, so the
        translation is used as written."""
        return self.nav_map.get(english, english)


def read_strings(loc: str) -> dict:
    path = I18N / loc / "strings.json"
    if not path.is_file():
        if loc == "en":
            raise I18nError("site/i18n/en/strings.json is missing")
        return {"ui": {}, "nav": {}}
    try:
        data = json.loads(path.read_text())
    except json.JSONDecodeError as e:
        raise I18nError(f"{path.relative_to(ROOT)}: not valid JSON: {e}") from None
    for part in ("ui", "nav"):
        if not isinstance(data.get(part, {}), dict):
            raise I18nError(f"{path.relative_to(ROOT)}: {part!r} must be an object")
    return {"ui": data.get("ui", {}), "nav": data.get("nav", {})}


_cache: dict[str, Strings] = {}


def strings(loc: str) -> Strings:
    if loc not in _cache:
        en = read_strings("en")["ui"]
        data = read_strings(loc) if loc != "en" else {"ui": en, "nav": {}}
        _cache[loc] = Strings(loc, data["ui"], data["nav"], en)
    return _cache[loc]


def apply_strings(template: str, s: Strings) -> str:
    """Fill {{t:key}} and {{tj:key}} in a partial."""
    template = re.sub(r"\{\{tj:([a-z0-9_.]+)\}\}", lambda m: s.js(m.group(1)), template)
    return re.sub(r"\{\{t:([a-z0-9_.]+)\}\}", lambda m: s(m.group(1)), template)


# ---------------------------------------------------------------- translations


class Translation:
    def __init__(self, loc: str, path: pathlib.Path, english: pathlib.Path, fm: dict, body: str):
        self.loc, self.path, self.english, self.fm, self.body = loc, path, english, fm, body

    @property
    def stale(self) -> bool:
        return self.fm.get("source_hash") != english_hash(self.english)


def english_source(loc: str, path: pathlib.Path) -> pathlib.Path:
    return PAGES / path.relative_to(I18N / loc)


def translation_files() -> list[tuple[str, pathlib.Path]]:
    out = []
    for loc in TRANSLATED:
        base = I18N / loc
        if base.is_dir():
            out += [(loc, p) for p in sorted(base.rglob("index.html"))]
    return out


def discover(parse_front) -> dict[pathlib.Path, dict[str, Translation]]:
    """{English source path: {locale: Translation}}. `parse_front(path, keys)`
    is build_site's front-matter parser."""
    unknown = sorted(d.name for d in I18N.iterdir() if d.is_dir() and d.name not in LOCALES)
    if unknown:
        raise I18nError(f"site/i18n/: unknown locale directories {unknown}; the locales are {sorted(LOCALES)}")
    out: dict[pathlib.Path, dict[str, Translation]] = {}
    for loc, path in translation_files():
        where = path.relative_to(ROOT)
        eng = english_source(loc, path)
        if not eng.is_file():
            raise I18nError(f"{where}: no English page {eng.relative_to(ROOT)} to translate")
        rel = eng.relative_to(PAGES).as_posix()
        if any(rel.startswith(u) for u in UNTRANSLATED):
            raise I18nError(f"{where}: {rel} is not translated (the release notes stay English)")
        fm, body = parse_front(path, TRANSLATION_KEYS)
        for key in ("title", "description", "source_hash"):
            if not fm.get(key):
                raise I18nError(f"{where}: a translation's front matter needs {key!r}")
        if not re.fullmatch(r"[0-9a-f]{16}", fm["source_hash"]):
            raise I18nError(f"{where}: source_hash must be 16 hex digits (scripts/check_i18n.py --hash prints it)")
        out.setdefault(eng, {})[loc] = Translation(loc, path, eng, fm, body)
    return out


def merge_front(english: dict, tr: Translation) -> dict:
    """The front matter a translated page renders with."""
    fm = {k: v for k, v in english.items() if k not in ("canonical", "head", "next_steps", "prev", "next")}
    fm.update({k: v for k, v in tr.fm.items() if k != "source_hash"})
    if "head" in english and "head" not in tr.fm:
        fm["head"] = english["head"]
    for key in ("next_steps", "prev", "next"):
        if key in english and key not in tr.fm:
            fm[key] = english[key]
    return fm


# ---------------------------------------------------------------- page context


class Ctx:
    """What the chrome needs to know about a page's languages."""

    def __init__(self, loc: str, url: str, available: list[str], tr: Translation | None,
                 translated_urls: dict[str, set[str]]):
        self.loc = loc
        self.url = url                    # the English URL of the page
        self.available = available        # locales with this page, English first
        self.tr = tr
        self.translated_urls = translated_urls  # locale -> English URLs translated in it
        self.s = strings(loc)

    @property
    def lang(self) -> str:
        return LOCALES[self.loc][0]

    @property
    def here(self) -> str:
        return url_for(self.loc, self.url)


def og_locale_alternates(ctx: Ctx) -> str:
    """og:locale:alternate lines for the other languages this page is in."""
    return "".join(f'    <meta property="og:locale:alternate" content="{OG_LOCALES[loc]}" />\n'
                   for loc in ctx.available if loc != ctx.loc)


def head_extra(ctx: Ctx) -> str:
    out = []
    if len(ctx.available) > 1:
        for loc in ctx.available:
            out.append(f'    <link rel="alternate" hreflang="{LOCALES[loc][0]}" href="{ORIGIN}{url_for(loc, ctx.url)}" />')
        out.append(f'    <link rel="alternate" hreflang="x-default" href="{ORIGIN}{ctx.url}" />')
    if ctx.loc in FONTS:
        out.append('    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />')
        out.append('    <link href="https://fonts.googleapis.com/css2?family='
                   f'{FONTS[ctx.loc][1]}&display=swap" rel="stylesheet" />')
    if ctx.loc == "en" and len(ctx.available) > 1:
        out.append(autoselect_script(ctx))
    return "\n".join(out) + ("\n" if out else "")


def autoselect_script(ctx: Ctx) -> str:
    """First visit to an English page with no stored choice: go to the
    reader's preferred language when this page has it. A stored choice, of
    any language, means no redirect; without working storage there is none
    either, so nobody is sent back and forth."""
    pages = {loc: url_for(loc, ctx.url) for loc in ctx.available if loc != "en"}
    return f"""    <script>
      (function () {{
        var pages = {json.dumps(pages, separators=(",", ":"))};
        try {{
          if (localStorage.getItem("{STORE}") !== null) return;
          if (/bot|crawl|spider|slurp|lighthouse/i.test(navigator.userAgent)) return;
          var prefs = navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language || ""];
          for (var i = 0; i < prefs.length; i++) {{
            var l = String(prefs[i]).toLowerCase(), loc = null;
            if (/^en\\b/.test(l)) return;
            if (/^pt\\b/.test(l)) loc = "pt-br";
            else if (/^zh\\b/.test(l)) {{
              if (/hant|-tw|-hk|-mo/.test(l)) continue;
              loc = "zh-cn";
            }} else {{
              var base = l.split("-")[0];
              if (["es", "ja", "tr", "ko", "de", "ru", "th"].indexOf(base) >= 0) loc = base;
            }}
            if (!loc) continue;
            if (!pages[loc]) return;
            localStorage.setItem("{STORE}", loc);
            sessionStorage.setItem("{AUTO_FLAG}", "1");
            location.replace(pages[loc] + location.search + location.hash);
            return;
          }}
        }} catch (e) {{}}
      }})();
    </script>"""


GLOBE = ('<svg class="lang-globe" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" '
         'stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">'
         '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9'
         'c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3z"/></svg>')


def switcher(ctx: Ctx) -> str:
    """The header's language menu: a <details> of plain links, so it works
    without JavaScript. Languages this page has not been translated into are
    listed and marked, not linked."""
    s = ctx.s
    lang, name, short = LOCALES[ctx.loc]
    items = []
    for loc, (tag, native, _) in LOCALES.items():
        if loc == ctx.loc:
            items.append(f'<li><a href="{ctx.here}" hreflang="{tag}" lang="{tag}" data-lang="{loc}" '
                         f'aria-current="true">{flag(loc)}<span class="lang-name">{native}</span>'
                         f'<span class="lang-tick" aria-hidden="true"></span></a></li>')
        elif loc in ctx.available:
            items.append(f'<li><a href="{url_for(loc, ctx.url)}" hreflang="{tag}" lang="{tag}" data-lang="{loc}">'
                         f'{flag(loc)}<span class="lang-name">{native}</span></a></li>')
        else:
            items.append(f'<li><span class="lang-off">{flag(loc)}<span class="lang-name" lang="{tag}">{native}</span>'
                         f'<span class="lang-note">{s("lang.unavailable")}</span></span></li>')
    read_en = ""
    if ctx.loc != "en":
        read_en = (f'\n            <a class="lang-en" href="{ctx.url}" hreflang="en" data-lang="en">'
                   f'{s("lang.read_en")}</a>')
    lis = "\n".join(f"              {i}" for i in items)
    return f"""        <details class="lang-menu">
          <summary class="lang-btn" aria-label="{s('lang.label')}: {name} ({short})">{flag(ctx.loc).replace('loading="lazy" ', '')}<span class="lang-cur">{name}</span><span class="lang-short" aria-hidden="true">{short}</span><span class="nav-chev" aria-hidden="true"></span></summary>
          <div class="lang-drop">
            <p class="lang-head">{s("lang.label")}</p>
            <ul>
{lis}
            </ul>{read_en}
          </div>
        </details>
"""


def notices(ctx: Ctx) -> str:
    """The bars above a translated page's content: the notice every
    translation carries, the stale notice when the English page has moved on,
    and the bar shown once after the language was chosen automatically."""
    if ctx.loc == "en":
        return ""
    s = ctx.s
    lang, name, _ = LOCALES[ctx.loc]
    report = f"{REPO}/issues/new?" + urllib.parse.urlencode({
        "title": f"Translation ({ctx.loc}): {ctx.here}",
        "body": f"Page: {ORIGIN}{ctx.here}\nEnglish: {ORIGIN}{ctx.url}\n\nWhat is wrong:\n",
    })
    en_link = f'<a href="{ctx.url}" hreflang="en" data-lang="en">'
    out = [f'    <div class="i18n-auto" role="status" hidden>',
           f'      <p><span>{s("lang.viewing", language=name)}</span><span class="sep" aria-hidden="true">&middot;</span>'
           f'{en_link}{s("lang.read_en")}</a></p>',
           f'      <button class="i18n-x" type="button" aria-label="{s("lang.dismiss")}"><span aria-hidden="true">&times;</span></button>',
           "    </div>",
           '    <div class="i18n-note" role="note">',
           f'      <p>{s("notice.translation")} <a href="{html.escape(report)}">{s("notice.report")}</a></p>']
    if ctx.tr is not None and ctx.tr.stale:
        out.append(f'      <p class="i18n-stale">{s("notice.stale")} {en_link}{s("notice.stale_link")}</a></p>')
    out += ["    </div>",
            "    <script>",
            "      (function () {",
            '        var bar = document.querySelector(".i18n-auto");',
            "        try {",
            f'          if (sessionStorage.getItem("{AUTO_FLAG}") === "1") bar.hidden = false;',
            "        } catch (e) {}",
            '        bar.querySelector(".i18n-x").addEventListener("click", function () {',
            "          bar.hidden = true;",
            "          try {",
            f'            sessionStorage.removeItem("{AUTO_FLAG}");',
            "          } catch (e) {}",
            "        });",
            "      })();",
            "    </script>"]
    return "\n".join(out) + "\n"


CHOICE_SCRIPT = f"""    <script>
      // Translated nav labels vary in length and the bar does not grow, so
      // when its contents overflow it first shortens the language button to
      // its short label, then tightens the spacing.
      (function () {{
        var root = document.documentElement;
        var panel = document.querySelector(".nav-panel");
        if (!panel) return;
        var phone = window.matchMedia("(max-width: 860px)");
        function over() {{
          return panel.scrollWidth > panel.clientWidth + 1;
        }}
        function fit() {{
          root.classList.remove("nav-fit-1", "nav-fit-2");
          if (phone.matches) return;
          if (over()) root.classList.add("nav-fit-1");
          if (over()) root.classList.add("nav-fit-2");
        }}
        fit();
        window.addEventListener("resize", fit);
        if (document.fonts && document.fonts.ready) document.fonts.ready.then(fit);
      }})();
      // The language menu closes on a click or tap outside it, and on Escape,
      // which returns focus to its button.
      (function () {{
        var menu = document.querySelector("details.lang-menu");
        if (!menu) return;
        document.addEventListener("pointerdown", function (e) {{
          if (menu.open && !menu.contains(e.target)) menu.open = false;
        }});
        document.addEventListener("keydown", function (e) {{
          if (e.key === "Escape" && menu.open) {{
            menu.open = false;
            var s = menu.querySelector("summary");
            if (s) s.focus();
          }}
        }});
      }})();
      // A language picked from the menu or a "Read in English" link is the
      // reader's choice from then on; the first-visit redirect never overrides it.
      document.addEventListener("click", function (e) {{
        var a = e.target.closest && e.target.closest("a[data-lang]");
        if (!a) return;
        try {{
          localStorage.setItem("{STORE}", a.getAttribute("data-lang"));
          sessionStorage.removeItem("{AUTO_FLAG}");
        }} catch (err) {{}}
      }});
    </script>
"""


def localize_links(markup: str, ctx: Ctx) -> str:
    """Point internal links at this locale's version of each page, where one
    exists. Links that name a language (hreflang) are left as written."""
    if ctx.loc == "en":
        return markup
    have = ctx.translated_urls.get(ctx.loc, set())

    def sub(m: re.Match) -> str:
        tag = m.group(0)
        if "hreflang=" in tag:
            return tag

        def href(h: re.Match) -> str:
            path, rest = h.group(1), h.group(2) or ""
            return f'href="{url_for(ctx.loc, path)}{rest}"' if path in have else h.group(0)
        return re.sub(r'href="(/[^"#?]*)([#?][^"]*)?"', href, tag, count=1)
    return re.sub(r"<a\b[^>]*>", sub, markup)


def english_island(markup: str, ctx: Ctx) -> str:
    """Mark the blocks generated from catches/ as English inside a translated
    page: the case titles and verdicts are not translated."""
    if ctx.loc == "en":
        return markup
    markup = markup.replace('<figure class="c-hero">', '<figure class="c-hero" lang="en">')
    return markup.replace('<div class="c-cards">', '<div class="c-cards" lang="en">')


def edit_url(ctx: Ctx, english_edit: str) -> str:
    if ctx.tr is None:
        return english_edit
    return f"{REPO}/blob/main/{ctx.tr.path.relative_to(ROOT).as_posix()}"


def sitemap_alternates(url: str, available: list[str]) -> list[str]:
    if len(available) < 2:
        return []
    out = [f'    <xhtml:link rel="alternate" hreflang="{LOCALES[loc][0]}" href="{ORIGIN}{url_for(loc, url)}" />'
           for loc in available]
    out.append(f'    <xhtml:link rel="alternate" hreflang="x-default" href="{ORIGIN}{url}" />')
    return out
