# Translating the site

The English pages in `site/pages/` are the reference. Each language lives in
its own directory here (`pt-br`, `es`, `ja`, `tr`, `zh-cn`, `ko`, `de`, `ru`,
`th`), and a translator only ever edits files inside their language's
directory. Everything below is run from the repository root, with Python 3.11
or later and nothing else installed.

## The workflow

You never edit a translated page's HTML. You edit a segments file, which lists
every piece of text on the English page with an id, the English, and an empty
`tr` for your translation. A script then writes the translated page from the
English source and your segments, so tags, links, classes and code blocks are
the English ones by construction.

For one page, in one language:

```sh
python3 scripts/i18n_extract.py /start/ es     # writes site/i18n/es/start/index.segments.json
# fill in every "tr" in that file
python3 scripts/i18n_apply.py /start/ es       # writes site/i18n/es/start/index.html
python3 scripts/check_i18n.py --locale es      # checks your language, renders it in memory
```

The page argument is the page's address (`/`, `/start/`, `/answers/react/`,
`404`) or its source path. `i18n_apply.py` refuses to write anything while a
segment is empty or breaks a rule below, and tells you which id. `check_i18n.py
--locale` writes nothing, reads only your language's files and the English
ones, and ends with a coverage line:

```
  es: 2 of 48 pages translated; case texts 0 of 275
    in progress: answers/react/index.html 12 of 31 segments
```

The release notes (`/versions/` and its month pages) are never translated.
They are dated records and stay as written; their language menu says so in
every language.

### When the English page changes

Run `i18n_extract.py` again on the same page. Every segment whose English is
unchanged keeps your translation. A new segment arrives empty. A segment whose
English changed arrives empty with `was_en` and `was_tr` beside it, the old
English and your old translation, so you can see what moved. Translate the
empty ones and run `i18n_apply.py` again.

Until you do, the translated page still builds and shows a notice that the
English version has changed, with a link to it. An English edit is never
blocked on a translation.

### Segment kinds

- `title`, `description`, `keywords`: the page's front matter. Keep a title's
  trailing ` · Glyph`.
- `prev.*`, `next.*`, `ns.*`: the previous/next links and the "where to go
  next" cards. These may not contain ` | `.
- `html`: a heading, paragraph, list item, table cell, caption, button or link
  label, with its inline markup.
- `attr:title`, `attr:alt`, `attr:aria-label`: an attribute. One line, no
  markup.
- `script`: a word a page script shows, such as "copied".

Text that appears twice on a page is one segment, translated once.

## What the checks hold you to

In an `html` segment, keep every tag with its attributes exactly as written
(the `href` of a link, the `class` of a span) and keep the content of every
`<code>`, `<kbd>` and `<samp>` element unchanged. Words may move around them,
and so may tags, so a sentence can follow your language's word order. Keep
every `{{...}}` placeholder: `{{catches:count}}` becomes a number,
`{{icon:arrow}}` an icon, and `{{catches:measured}}` a sentence that is
translated in `strings.json`.

`check_i18n.py` also fails when a translated page is not what `i18n_apply.py`
writes from its segments file, which is how it notices a hand edit to the
page. Edit the segments file instead.

## Other files in your directory

- `strings.json` holds the words the generator puts around every page: the
  header and footer (`ui`), the main menu (`nav`, keyed by the English text),
  and the numbered lists of answers and pillars (`subnav`: each entry keyed by
  the address it links to, such as `"/answers/react/"`, with its English label
  in `site/partials/subnav-answers.html` or `subnav-pillars.html`, and each
  group of answers keyed by its English name). A missing entry shows in
  English, and `check_i18n.py` lists what is missing. Keep every `{name}` in a `ui` string; the generator fills it in.
  `date.months` is twelve short month names separated by spaces, and
  `date.format` puts `{day}`, `{month}` and `{year}` in your order.
- `catches.json` holds the texts of the cases on `/catches/` and the home
  page's case cards. Create or update it with
  `python3 scripts/i18n_extract.py catches es`. Each text carries the English it
  was translated from; when the case's English changes, that text shows in
  English again until you redo it. The labels around the cases (the filters,
  the score legend, "Copy link") are `catches.*` entries in `strings.json`.
  Code, compiler output and error codes on those pages stay English.

## What never gets translated

- Code of any kind: anything in `<pre>`, `<code>`, `<kbd>` or `<samp>`, and
  compiler output.
- Product, tool and project names: Glyph, TypeScript, JavaScript, Node, npm,
  npx, tsx, tsc, typescript-eslint, zod, Effect, ReScript, Gleam, Elm, React,
  MCP, GitHub, VS Code.
- Error codes (`E0200`, `TS2366`), gap numbers (`G273`), version numbers,
  CLI commands and flags, file names and paths.
- Language keywords and type names when they name the construct: `match`,
  `Result`, `Option`, `fn`, `record`, `string`. Writing "una unión
  etiquetada" for "a tagged union" is fine; renaming `Result` is not.

## Tone

Write the way one engineer explains something to another: plain, specific,
no hype. If a sentence would sound like an advertisement in your language, it
is wrong in English too. Use the terms developers in your language actually
use, even when that means keeping the English word: Japanese pages keep
"TypeScript" and write "コンパイラ" in katakana; Spanish pages keep "flag",
"diff" and "lint", as Spanish-speaking developers do. Follow what the home
page translation in your directory already does, so the pages read as one
site.

Do not use em dashes. Use a period, a comma, a colon or parentheses, as the
sentence needs.

## The notice and what the site says about translations

Every translated page shows the same short notice: that the page is a
translation, that the English version is the reference, and a link to report
a problem. Do not add to it or reword it on a page.

Beyond that notice, a page says nothing about how it was translated.
`check_i18n.py` fails on wording that describes a translation as automatic,
in every language, so keep such remarks out of pages, segments and
`strings.json`.
