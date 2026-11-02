# todo

## What it is

A counter and a todo list in the browser, against real React 18. Every hook is
React's own (`useState`, `useEffect`), imported by name and destructured with
`let [count, set_count] = useState(0)`. The page is reached through the
standard library: `std/dom` finds the mount point and sets the title,
`std/storage` keeps the list across a reload, and `std/console` reports a
write the browser refused. The list is saved as one string, one item per line,
each behind a marker: `[x] ` for an item that is done and `[ ] ` for one that
is not. A line with no marker, which is how 0.1.128 saved it, loads as not
done.

## Running it

```sh
cd examples/apps/todo
npm install
npm run bundle              # glyph build into dist/gen, then vite build into dist/web
npx vite preview --outDir dist/web
```

`glyph check` from this directory type-checks it against React's own
declarations with `tsc --strict`. `python3 scripts/check_todo_renders.py` from
the repository root builds and bundles it, then drives it in headless Chromium
and checks the text on the page; CI runs it on every push.

## What it changed in Glyph

Shipped **0.1.128**, and it is the app that release is measured by: it builds
with Vite, renders in a headless browser, and the title, the list and the saved
state behave as written. The first browser run was by hand; the release review
found the committed source showing "2left", a space lost to the formatter bug
G276, and the run became a gate that reads the page's text.

The 0.1.126 round wrote the same program and stopped at
`mut document.title = ...`, because browser globals are E0103 and nothing in the
standard library wrapped the page (G254). On the way it found that a hook's
tuple could not be destructured (G255). 0.1.128 added `std/dom`, `std/storage`
and `std/console`, and D48's `let [a, b] = expr`.

Glyph has no top-level statements, so `main.glyph` declares `main` and the
page's one inline script calls it, which is what `glyph run` does under node.
That script is the only code in the app that is not Glyph.
