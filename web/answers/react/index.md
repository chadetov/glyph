<!-- Markdown copy of https://glyphlang.io/answers/react/, generated from the same source as the page. Last changed 2026-11-21. -->

13 · Frontend

# Does it work with React: components, hooks, and Context?

Yes. A `component` compiles to a React function component, JSX lowers to `createElement`, React's own hooks are imported by name and their pairs destructured, and reducer state gets exhaustiveness plain TypeScript does not have. The page itself (its title, the mount point, saved state) is reached through `std/dom` and `std/storage`. The gap worth knowing first: Glyph's own checker does not type what a hook returns, so `tsc` checks those values.

counter.glyph, against react@18 and its types

```
import react { useReducer }

type Action = Inc | Dec | Reset

fn reducer(n: number, a: Action) -> number {
  return match a {
    Inc => n + 1,
    Dec => n - 1,
    Reset => 0,
    // add an Action variant and this match is E0200
  }
}

pub component Counter() {
  let [count, dispatch] = useReducer(reducer, 0)
  return <button on_click={fn() { dispatch(Inc) }}>{count}</button>
}
```

React's real `useReducer`, with a reducer over a tagged union. Every action has to be handled, and the compiler checks it.

## Why it matters

A lot of the code agents write is React. Glyph treats it as a first-class target, not a bolt-on: `component` is its own keyword (so `grep "^component "` audits your whole UI).

React's hooks are imported from `react` and called with their real names and types, checked by `tsc` against React's own declarations.

## Reducers and hooks

Add a `Double` action and the reducer stops compiling with `E0200` naming it, which a plain TypeScript `switch` would let ship.

E0200

Glyph's own checker does not type values that come from React, so a wrong argument to `dispatch` or a state setter is reported by `tsc` in the build, not by the editor or `--no-tsc`.

**Details how the hook's pair is destructured **

The pair is destructured with `let [count, dispatch]`, and the emitted TypeScript adds `satisfies readonly [unknown, unknown]`, so `tsc` proves the hook returned a pair and still infers each element's type.

## Context and libraries

React **Context** works today through `createContext` / `useContext`, and member-expression element names (`<ThemeContext.Provider>`) work directly: no workaround.

- **Prop spread** JSX prop spreading works too: `<input {...register("email")} />` lowers to an object spread in the props, so the react-hook-form idiom needs no adapter.
- **Zod types** Zod's value-derived types are first-class: `type FormData = z.infer<typeof schema>` works directly, no escape hatch.

The one thing that differs: a component takes a single props record, not multiple positional params. See [using your libraries](https://glyphlang.io/answers/imports/) and the [hardening page](https://glyphlang.io/hardening/) for exactly where that boundary is.

## Are props checked before tsc?

Yes. Glyph compares each element with its component's props record itself, so a wrong value, a misspelled prop and a missing required one are Glyph errors that name the prop. It works the same whether the component was imported by name or through its module:

main.glyph, with `Card` declared in `ui.glyph`

```
import ui

pub component App() {
  return <div>
    <ui.Card title="Q3" />
    <ui.Card title="Q4" cout={2} />
  </div>
}

// E0211 argument type mismatch: expected `CardProps`, found `record`: missing field `count`
// E0210 type `CardProps` has no field `cout`; did you mean `count`?
// E0211 argument type mismatch: expected `CardProps`, found `record`: missing field `count`
```

You see them in the editor and over MCP, not only in a full build.

**Details missing_fields in --json, and one props record (E0229) **

`--json` carries the missing names as a `missing_fields` list, so an agent can add the props without parsing the message. A component also has to take its props as one record: `component Greeting(name: string)` is E0229 at the parameter, because React only ever passes the props object and that element could never type-check. If you call it as a function rather than render it, declare it with `fn`.

## Is a <match> in JSX checked like a match?

It is the same thing. Each `<case>` takes the pattern a `match` arm takes, and the compiler runs it through the code it uses for the arm.

- **Payload types** The names a case binds have the payload's types, a field the payload record doesn't have is E0210, and a missing variant is E0200.
- **Partial cases** A case that tests a value only covers part of its variant, so it needs a sibling case or an `<case else>` to take the rest.

results.glyph

```
type Page =
  | Loading
  | Loaded({ users: Array<User>, total: number })
  | Failed(string)

component Results(props: { page: Page }) {
  return <match value={props.page}>
    <case Loading><p>Loading...</p></case>
    <case Loaded({ users, totl })><p>{users.length} of {totl}</p></case>
    <case Failed(message)><p>{message}</p></case>
  </match>
}

// E0210 type `{ users: Array<User>, total: number }` has no field `totl`; did you mean `total`?
```

## How does a page start it?

Glyph has no top-level statements, so the page calls an exported `main` itself. The app is two modules: one holding the components, and one that mounts them on `#root` through `std/dom` and react-dom's `createRoot`.

Event attributes are snake case (`on_change` emits React's `onChange`), a handler is a lambda, and an input's text is `e.currentTarget.value`.

app.glyph

```
module app

import react { useState }

pub component App() {
  let [name, set_name] = useState("")
  return <main>
    <input value={name} on_change={fn(e) { set_name(e.currentTarget.value) }} />
    <p>Hello, {name}</p>
  </main>
}
```

main.glyph

```
module main

import react-dom/client { createRoot }
import std/dom
import std/console
import std/option { Some, None }
import app { App }

pub fn main() -> void {
  match dom.element_by_id("root") {
    Some(root) => createRoot(root).render(<App />),
    None => console.error("the page has no #root element"),
  }
}
```

index.html, next to the two modules

```
<div id="root"></div>
<script type="module">
  import { main } from "./dist/gen/main.ts";
  main();
</script>
```

1. `npx glyph build . --out dist/gen`
2. `tsc --strict`
3. `npx vite build --outDir dist/web`

**Details the build, why this main takes nothing, the walkthrough **

`npx glyph build . --out dist/gen` checks the modules, runs `tsc --strict` against React's declarations and writes TypeScript to `dist/gen`. `npx vite build --outDir dist/web` then reads `index.html`, follows the import and bundles it with React. This `main` takes nothing and returns nothing, because the script calls it with no arguments; the `main(argv) -> number` that `glyph init` writes is the one `glyph run` calls under node. The full walkthrough, with the `package.json` and the install line, is [running Glyph in a browser page](https://github.com/chadetov/glyph/blob/main/AGENTS.md#running-glyph-in-a-browser-page) in the reference `glyph llms` prints. CI builds [examples/apps/todo](https://github.com/chadetov/glyph/tree/main/examples/apps/todo) the same way and renders it in headless Chromium.

## Where it stands

### Shipping today

- Components, JSX, fragments (`<>...</>`)
- Member-expression element names (`<Ctx.Provider>`)
- JSX prop spread (`{...register()}`)
- Props checked against the component before `tsc` under every import spelling
- React's own hooks imported by name (`useState`, `useReducer`, `useEffect`) with tsc checking their types and their pairs destructured (`let [count, set_count] = useState(0)`)
- `useReducer` with exhaustive tagged-union actions
- `<match>`/`<case>` checked and lowered as a `match` with the arm's patterns
- Context via `createContext`/`useContext`
- The page through `std/dom` (title, mount point, address), `std/storage` and `std/console`
- JSX text is read as TSX reads it, so `<p>Don't stop</p>` is plain text; `}`, `>`, `//` and an HTML entity such as `&amp;` are refused in text (E0012), with the spelling to use instead. E0012
- A member read on an array is checked, so `{items.foo}` on an `Array<string>` is E0210. E0210
- [examples/apps/todo](https://github.com/chadetov/glyph/tree/main/examples/apps/todo) is a whole app on React 18 that bundles with Vite and renders in a browser.

### On the way

- Glyph's checker does not type a hook's result, so `set_count("x")` on a number state is a `tsc` error in the build rather than a Glyph one in the editor.
- `std/dom` stops at reading the page and setting its title: React owns the DOM it renders, so there are no element setters or event listeners.
- An effect has to be written `fn() -> void { ... }`, because a lambda returns its last expression and React would call it as a cleanup.
- A props type written inline still shows as `record` in these messages; a named one shows its name.
- Embedding the Glyph output in an existing Vite app, with the UI in TypeScript, is in [the deployment guide](https://github.com/chadetov/glyph/blob/main/docs/guide/deployment.md).

Code examples on this page are checked against Glyph 0.1.155 on every build. Last changed 21 Nov 2026.
