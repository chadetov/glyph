<!-- Markdown copy of https://glyphlang.io/answers/react-not-yet/, generated from the same source as the page. Last changed 2026-11-26. -->

14 · React: not yet

# What doesn't Glyph do for React yet?

The rows below. Most of them come down to one fact: Glyph's own checker does not type what comes from React, so `tsc` catches those mistakes in a full build while the editor and `check --no-tsc` do not. A few patterns need a different spelling, Fast Refresh does not work yet, and two features are not planned. Every row has a gap number, and the release that closes a gap removes its row.

## The list

Each row was reproduced against the published compiler in a project with React 18 and `@types/react` 18, except the React 19 row, which used 19. The gap entries hold the exact programs.

| What | Today | Meanwhile | Gap |

|---|---|---|---|

| Hook results | Glyph's own checker does not type what a hook returns. `set_count("x")` on a `useState(0)` pair passes `check --no-tsc` and the editor; a full `glyph check` reports TS2345 at `"x"`. Hover on `count` answers `null`. | Run the full `glyph check` or `glyph build` before you trust a React change. The `tsc` step checks every hook against `@types/react`. | [G257](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |
| Element refs | `useRef(null)` is E0103, because Glyph has no `null`; the empty value is spelled `nullable.from_option(None)`. `<input ref={useRef(0)} />` passes both Glyph and `tsc`. | Write `useRef(nullable.from_option(None))`, or use a callback ref and unwrap the element with `nullable.to_option`, as in the module below. | [G630](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |
| Children | `<Card title="x">hello</Card>` is E0211 when `Card` declares `children: ReactNode`. Written children are not part of the props object Glyph checks, so a required `children` is never filled. | Declare `children?: ReactNode`, or pass the children as an attribute: `children={<p>hello</p>}`. | [G631](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) (its fix files it as G603) |
| Rules of hooks | A hook called inside a `match` arm or a loop compiles, and `tsc` has no rule against it either. React matches hooks by call order, so the component breaks at run time once the order changes between renders. | Call every hook at the top of the component and use its values inside the arms. | [G632](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) (its fix files it as G596) |
| Effect cleanup | `useEffect(fn() { storage.set("k", "v") }, [])` passes `--no-tsc`. The lambda returns its last expression, and React calls whatever an effect returns as its cleanup. A full check reports TS2345. | Write every effect as `fn() -> void { ... }`. The annotation stops the last expression from being returned. | [G633](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) (its fix files it as G594) |
| forwardRef | `ref` on a `component` is E0210, which matches React 18. `forwardRef(fn(props: { label: string }, ref) { ... })` compiles, but only `tsc` checks the props of the component it returns. | Use `forwardRef` and rely on the full check for its props. | [G634](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |
| Context | `createContext`, `<Theme.Provider>` and `useContext` work. Glyph's checker does not type the context value, so `let n: number = useContext(Theme)` is caught by `tsc` only. How Context and custom hooks should look in Glyph is [open question Q44](https://github.com/chadetov/glyph/blob/main/docs/open-questions.md#q44-react-context--effectful-custom-hook-composition). | Use React's API as it is, with the full check. | [G635](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md), [Q44](https://github.com/chadetov/glyph/blob/main/docs/open-questions.md#q44-react-context--effectful-custom-hook-composition) |
| Other hooks | The planned fix for G257 types `useState`, `useReducer`, `useMemo`, `useCallback`, `useRef`, `useEffect` and `useLayoutEffect`. `useId`, `useTransition`, `useDeferredValue`, `useSyncExternalStore`, `useImperativeHandle` and `useContext` are outside it and stay checked by `tsc` only. | Call them as in TypeScript; the full check covers them. | [G636](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |
| @types/react 19 | React 19 works: the todo app passes a full check against `@types/react` 19.3.0. The planned hook types are written against `@types/react` 18, so a project on 19 will keep its hooks checked by `tsc` only after G257 ships. | Either version builds. Stay on 18 if you want Glyph's own hook checks when they land. | [G637](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |
| Fast Refresh | Saving a `.glyph` file under `glyph build --watch` beside a Vite dev server reloads the page, and component state is lost, even with `@vitejs/plugin-react`. Watch mode rewrites every output file, and an emitted module does not import React's JSX runtime, which the plugin needs before it hot-swaps a `.ts` file. | Expect a full reload on each save. `vite build` is not affected. | [G444](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md), [G638](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) (its fix files it as G609) |
| Class components | Not planned. Glyph has no classes, so `class Old extends Component` is E0003. | Write function components. A class component from npm still renders: `react-error-boundary`'s `ErrorBoundary` passes a full check as an element, and covers error boundaries, which React only offers as a class. | [G639](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |
| Server components | Not planned before 1.0. A module cannot start with `"use client"` (E0003), and a component cannot be `async` (E0002). | Keep the files that need a directive in TypeScript and import the emitted Glyph from them, as [the embedding answer](https://glyphlang.io/answers/embedding/) describes. | [G640](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md) |

## The spellings that work

Three of the workarounds in one module: an optional `children`, an effect annotated `-> void`, and a callback ref. It passes `glyph check` with `tsc --strict` against React 18, and the site build compiles it again on every run.

app.glyph

```
module app

import react { useEffect, ReactNode }
import std/dom
import std/nullable
import std/option { Some, None }

component Card(props: { title: string, children?: ReactNode }) {
  return <section>
    <h2>{props.title}</h2>
    {props.children}
  </section>
}

pub component SignIn() {
  useEffect(fn() -> void { dom.set_title("Sign in") }, [])
  return <Card title="Sign in">
    <input ref={fn(el) {
      match nullable.to_option(el) {
        Some(input) => input.focus(),
        None => {},
      }
    }} />
  </Card>
}
```

The element in the callback is typed by `tsc` as `HTMLInputElement | null`, which is why it goes through `nullable.to_option` before `focus()`. Glyph's checker does not see that type, so a misspelled method there is a `tsc` error.

## How the list is kept

A row leaves this page in the same pull request that closes its gap, so the page does not outlive the problem. The plan for each open row is in [the release roadmap](https://github.com/chadetov/glyph/blob/main/docs/roadmap/releases.md), and what works today is on [the React answer](https://glyphlang.io/answers/react/).

## Where it stands

### Shipping today

- Components, JSX, React's hooks imported by name, Context and prop spread, with `tsc` checking everything React declares. See [the React answer](https://glyphlang.io/answers/react/).
- React 18 and React 19 both build.

### On the way

- Typed hook results (G257), then the effect cleanup rule (G633), the rules of hooks (G632) and element refs (G630).
- Written children filling a required `children` (G631), and a Vite plugin with Fast Refresh (G638).
- Not planned: class components (G639), and server components before 1.0 (G640).

Code examples on this page are checked against Glyph 0.1.195 on every build. Last changed 26 Nov 2026.
