"""The site's structured data: one JSON-LD graph per page.

The home page (and each translated home) carries the entities the rest of the
site points at, each with a stable @id:

  https://glyphlang.io/#website   WebSite
  https://glyphlang.io/#project   Organization, "The Glyph project"
  https://glyphlang.io/#software  SoftwareApplication, the compiler on npm
  https://glyphlang.io/#source    SoftwareSourceCode, the GitHub repository

Every other page carries a BreadcrumbList built from its URL path and the
titles of the pages along it. Articles (the answers, the pages for TypeScript
developers, teams and agent builders, the design and pillar pages,
/start/, and each TypeScript case's page under /catches/) add a TechArticle whose dateModified is the page's date from
site/lastmod.json, and /answers/ adds a CollectionPage listing every answer
in sub-nav order. A page's references to the four entities resolve to the
home page's graph; scripts/check_site.py checks that every @id a page
references is defined on that page or on /.

There is no FAQPage (no longer a rich result) and nothing about ratings or
reviews, which do not exist.

Python standard library only.
"""
from __future__ import annotations

import html
import json
import re

ORIGIN = "https://glyphlang.io"
WEBSITE = f"{ORIGIN}/#website"
PROJECT = f"{ORIGIN}/#project"
SOFTWARE = f"{ORIGIN}/#software"
SOURCE = f"{ORIGIN}/#source"
REPO = "https://github.com/chadetov/glyph"
NPM = "https://www.npmjs.com/package/@glyphlang/glyph"
# The licence is MIT OR Apache-2.0: either, at the user's choice.
LICENSES = ["https://opensource.org/licenses/MIT", "https://www.apache.org/licenses/LICENSE-2.0"]
# The npm package ships binaries for darwin-arm64, darwin-x64, linux-arm64,
# linux-x64 and win32-x64 (npm/glyph/package.json, optionalDependencies).
OPERATING_SYSTEMS = "macOS, Linux, Windows"

# URL prefixes whose pages are articles.
ARTICLES = ("/answers/", "/from-typescript/", "/teams/", "/agents/", "/design/", "/pillars/", "/start/",
            "/catches/")


def plain(text: str) -> str:
    """Front-matter text, which may hold entities and inline tags, as text."""
    return html.unescape(re.sub(r"<[^>]+>", "", text)).strip()


def is_article(url: str) -> bool:
    return url not in ("/answers/", "/catches/") and url.startswith(ARTICLES)


def site_nodes(version: str, languages: list[str]) -> list[dict]:
    return [
        {"@type": "WebSite", "@id": WEBSITE, "url": f"{ORIGIN}/", "name": "Glyph",
         "inLanguage": languages, "publisher": {"@id": PROJECT}},
        {"@type": "Organization", "@id": PROJECT, "name": "The Glyph project", "url": f"{ORIGIN}/",
         "logo": {"@type": "ImageObject", "url": f"{ORIGIN}/apple-touch-icon.png", "width": 180, "height": 180},
         "sameAs": [REPO, NPM]},
        {"@type": "SoftwareApplication", "@id": SOFTWARE, "name": "Glyph", "url": f"{ORIGIN}/",
         "applicationCategory": "DeveloperApplication", "operatingSystem": OPERATING_SYSTEMS,
         "softwareRequirements": "Node.js 18 or later", "softwareVersion": version,
         "downloadUrl": NPM, "installUrl": f"{ORIGIN}/start/", "releaseNotes": f"{ORIGIN}/versions/",
         "license": LICENSES, "isAccessibleForFree": True,
         "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
         "publisher": {"@id": PROJECT}},
        {"@type": "SoftwareSourceCode", "@id": SOURCE, "name": "Glyph compiler", "codeRepository": REPO,
         "programmingLanguage": "Rust", "license": LICENSES, "targetProduct": {"@id": SOFTWARE},
         "publisher": {"@id": PROJECT}},
    ]


def breadcrumbs(url: str, titles: dict[str, str], page: str, localize=lambda href: href) -> dict:
    """Home, then each ancestor of `url` that is a page, then the page. On a
    translated page, `titles` holds the translated titles and `localize`
    maps an English URL to this language's when it has one."""
    items, path = [("Glyph", "/")], "/"
    for part in url.strip("/").split("/"):
        path += part + "/"
        if path in titles:
            # The part of a title before " · " is its short name: "Answers
            # · straight replies to ..." is "Answers" in a trail.
            items.append((titles[path].split(" · ")[0], path))
    return {"@type": "BreadcrumbList", "@id": f"{page}#breadcrumb",
            "itemListElement": [{"@type": "ListItem", "position": i, "name": plain(name),
                                 "item": f"{ORIGIN}{localize(href)}"}
                                for i, (name, href) in enumerate(items, start=1)]}


def graph(*, url: str, here: str, lang: str, title: str, description: str, date: str, version: str,
          titles: dict[str, str], languages: list[str], answers: list[str],
          localize=lambda href: href) -> str:
    """The <script type="application/ld+json"> block for one page. `url` is
    the English URL, `here` this rendering's, `titles` the short title of every
    English page by URL, `answers` the answer URLs in sub-nav order."""
    page = f"{ORIGIN}{here}"
    nodes: list[dict] = []
    if url == "/":
        nodes += site_nodes(version, languages)
        nodes.append({"@type": "WebPage", "@id": f"{page}#webpage", "url": page, "name": plain(title),
                      "description": plain(description), "inLanguage": lang, "isPartOf": {"@id": WEBSITE},
                      "about": {"@id": SOFTWARE}, "dateModified": date})
    else:
        nodes.append(breadcrumbs(url, titles, page, localize))
    if is_article(url):
        nodes.append({"@type": "TechArticle", "@id": f"{page}#article", "headline": plain(title)[:110],
                      "description": plain(description), "url": page, "inLanguage": lang,
                      "dateModified": date, "image": f"{ORIGIN}/og.png",
                      "isPartOf": {"@id": WEBSITE}, "about": {"@id": SOFTWARE},
                      "author": {"@id": PROJECT}, "publisher": {"@id": PROJECT},
                      "breadcrumb": {"@id": f"{page}#breadcrumb"}})
    if url == "/answers/":
        nodes.append({"@type": "CollectionPage", "@id": f"{page}#webpage", "url": page, "name": plain(title),
                      "description": plain(description), "inLanguage": lang, "dateModified": date,
                      "isPartOf": {"@id": WEBSITE}, "about": {"@id": SOFTWARE},
                      "breadcrumb": {"@id": f"{page}#breadcrumb"},
                      "mainEntity": {"@type": "ItemList", "numberOfItems": len(answers),
                                     "itemListElement": [{"@type": "ListItem", "position": i,
                                                          "url": f"{ORIGIN}{localize(a)}", "name": plain(titles[a])}
                                                         for i, a in enumerate(answers, start=1)]}})
    data = json.dumps({"@context": "https://schema.org", "@graph": nodes}, ensure_ascii=False,
                      separators=(",", ":"))
    # Nothing in the data may close the script element early.
    data = data.replace("</", "<\\/")
    return f'    <script type="application/ld+json">{data}</script>\n'
