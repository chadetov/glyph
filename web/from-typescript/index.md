<!-- Markdown copy of https://glyphlang.io/from-typescript/, generated from the same source as the page. Last changed 2026-11-22. -->

Coming from TypeScript

# What changes when you already write TypeScript

Less than you'd expect. Glyph compiles to TypeScript that passes `tsc --strict`, keeps npm and your deploy, and reads almost the same. The differences are a handful of deltas, and each one trades a way to be wrong for a check the compiler makes.

## If you know TypeScript, you know most of this

You don't learn a new language, just these four. They are the ones in every sample on this site.

`match`

An exhaustive `switch`: over a union, the compiler makes you handle every case. In statements and expressions it is also the only conditional: there is no `if`/`else` and no ternary, explained below. JSX has `<if>` and `<match>` elements.

`Result<T, E>`

Errors as return values, not thrown exceptions. You handle one with `match`, or propagate it with the `?` operator.

`fn f() -> T`

A function; the `->` is its return type. A `type X = {...}` is a record, and a `type X = A | B` is a tagged union.

`mut x = e`

Reassignment is explicit and greppable. A plain `let` binding never changes under you; you write `mut` to change one.

## A React component in both

The same component twice: a list of invites with an empty state, a line counting the ones still waiting, and a label on each row. Where the TSX uses `&&` and `? :`, the Glyph uses JSX elements: `<match>`, which takes a value and one `<case>` per pattern, and `<if>` with an optional `<else>`, the lighter form for a plain boolean. Lists use `<for>`, and the attribute is `class`, which compiles to `className`.

InviteList.tsx

```
export type Invite = { email: string; accepted: boolean };

export function InviteList(props: { invites: Invite[] }) {
  const waiting = props.invites.filter((i) => !i.accepted).length;
  return (
    <section>
      {waiting > 0 && <p className="note">{waiting} waiting for a reply</p>}
      {props.invites.length === 0 ? (
        <p className="empty">No invites sent yet.</p>
      ) : (
        <ul>
          {props.invites.map((invite) => (
            <li key={invite.email}>
              {invite.email}
              {invite.accepted ? (
                <span className="ok">joined</span>
              ) : (
                <span className="wait">pending</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
```

invites.glyph

```
module invites

import std/array

pub type Invite = { email: string, accepted: bool }

pub component InviteList(props: { invites: Array<Invite> }) {
  let waiting = array.filter(props.invites, fn(i: Invite) { return !i.accepted }).length
  return <section>
    <if cond={waiting > 0}>
      <p class="note">{waiting} waiting for a reply</p>
    </if>
    <match value={props.invites.length}>
      <case 0>
        <p class="empty">No invites sent yet.</p>
      </case>
      <case else>
        <ul>
          <for invite in={props.invites} key={invite.email}>
            <li>
              {invite.email}
              <if cond={invite.accepted}>
                <span class="ok">joined</span>
              </if>
              <else>
                <span class="wait">pending</span>
              </else>
            </li>
          </for>
        </ul>
      </case>
    </match>
  </section>
}
```

The Glyph half is longer, mostly because each branch is its own element. A `<match>` has to cover every value: drop `<case else>` here and the build stops with `E0218`. Over a union, it has to cover every variant (`E0200`), which is where it pays for the extra lines: [Does it work with React?](https://glyphlang.io/answers/react/) shows one, with hooks and Context.

Editor support and debugging have their own answers: [the tooling answer](https://glyphlang.io/answers/tooling/) covers the language server and the VS Code extension, and [the debugging answer](https://glyphlang.io/answers/debugging/) covers source maps back to the `.glyph` file. For the dev loop, `glyph build --watch` rebuilds the TypeScript each time you save a `.glyph` file. A Vite dev server that imports that output reloads the whole page after each rebuild instead of hot-swapping the component, so component state resets, while your own `.tsx` files keep Vite's hot reload. Write the output outside Vite's build directory (`dist/` by default), because its dev server does not watch that directory.

"Agents" on this site means AI coding assistants such as Claude, Copilot and Cursor. Glyph helps them most, but the checks are the same whether a person or a model wrote the code. If you already run strict TypeScript with zod and eslint, [here is what a language adds over that](https://glyphlang.io/answers/vs-tooled-ts/).

## There is no if statement

A branch on a boolean is a `match` on `true` and `false`. Where you only act on one side, the other arm is an empty block. JSX is the exception: the `<if>` element in the component above renders its children when the condition holds and nothing otherwise.

TypeScript

```
function label(admin: boolean): string {
  return admin ? "admin" : "member";
}

if (verbose) {
  console.log("starting");
}
```

Glyph

```
fn label(admin: bool) -> string {
  return match admin {
    true => "admin",
    false => "member",
  }
}

match verbose {
  true => io.println("starting"),
  false => {},
}
```

The reason is decision D3 in the spec: one construct for branching on a value, so there is one shape to read, write and search for. A value that comes out of a branch always comes out of a `match`, whose result type Glyph's own checker works out instead of handing it to `tsc`. For a union, the same construct is the one that must cover every case (`E0200`). For a boolean that buys little, since `false => {}` says no more than leaving out an `else`.

It costs you something. A one-line ternary becomes a four-line `match`, an early return on a condition is a `match` with an empty arm, and it is the first thing that looks wrong to a TypeScript reader. The spec records the verbosity as an accepted cost. Writing `if` anyway is error `E0006`, whose help shows the `match` form; a ternary is `E0013` with the same help.

## The tax you already pay

If you build with AI agents on a TypeScript codebase, these are the daily costs. Each one has a specific answer in Glyph, linked from the list.

- Agents hallucinate APIs, because TypeScript erases its types and the runtime no longer agrees with compile time. Glyph types carry a runtime check (`T.parse`, `T.is`), see [typed APIs](https://glyphlang.io/answers/typed-apis/).
- Agents paper over uncertainty with `any` and `as unknown as T`, and the compiler lets them. Neither exists in Glyph, see the table below.
- You ask for a one-line change and get a whole file rewritten, because the formatter cascades on every edit. `glyph fmt` has one layout with no options, see [clean diffs](https://glyphlang.io/answers/diff-stability/).
- You keep a hand-written validator next to every type (zod, and a serializer), and an agent renames a field in one but not the others; all of it still compiles. In Glyph the type is the validator.
- You `grep` for a symbol and find ten unrelated matches: overloads, decorators, merged namespaces. Glyph has none of those, see [finding things](https://glyphlang.io/answers/greppability/).

## The off-switches TypeScript gives you, and what Glyph has instead

Each TypeScript construct below tells the checker to stop checking. None of them is Glyph syntax. The middle column is what `glyph check --no-tsc` printed for each one on 0.1.149.

| TypeScript | In Glyph | What you write instead |

|---|---|---|

| `x!` | Not syntax: `E0013` | An `Option<T>` handled with `match`, or passed up with `?` |
| `x as T`, `as unknown as T` | No cast exists: `E0013`, whose help says to validate | `T.parse(x)`, which returns a `Result` |
| `any` | Not a type: `E0103`, unresolved name | `unknown`, narrowed by `T.parse`, `T.is` or a `match` arm `is T` |
| `(v): v is T` | No user-written type guards: `E0002` | The generated `T.is(v)`, which checks the declared shape, so a guard can't check less than it claims |
| `// @ts-ignore`, `// eslint-disable` | An ordinary comment; the error under it still fires | Nothing. No comment or flag turns a Glyph diagnostic off |

`glyph check` has flags that skip a whole pass (`--no-tsc` skips the `tsc --strict` run, `--no-test` skips the `@example` tests), and none that silence one check. The full list of what a TypeScript project accepts and Glyph refuses, each case run under both compilers, is on [the catches page](https://glyphlang.io/catches/).

## One escape hatch, and you can grep for it

When you need TypeScript that Glyph can't spell, `extern_ts("...")` takes one inline type or expression, and `tsc` still checks it. For a whole module, put the `.ts` under `src/extern/` and write `import extern/name`; relative imports are otherwise illegal, so that path is the only way in. Both forms contain the word `extern`, so `grep -rn extern src/` lists every place the checker hands over to TypeScript.

src/clock.glyph

```
module clock

pub fn now_ms() -> number {
  return extern_ts("Date.now()")
}
```

## What the emitted TypeScript looks like

Plain TypeScript, one file per module, that you can read and commit. A union becomes a discriminated union plus a value carrying its runtime check, and a `match` becomes a `switch` on the tag. This is `glyph build`'s output for the module on the left, with the import lines trimmed.

plan.glyph

```
module plan

pub type Plan = Free | Pro | Team

pub fn seats(p: Plan) -> number {
  return match p {
    Free => 1,
    Pro => 5,
    Team => 50,
  }
}
```

plan.ts (emitted, trimmed)

```
export type Plan =
  | { tag: "Free" }
  | { tag: "Pro" }
  | { tag: "Team" };

export const Free: Plan = { tag: "Free" };
export const Pro: Plan = { tag: "Pro" };
export const Team: Plan = { tag: "Team" };
export const Plan = {
  is(value: unknown): value is Plan { ... },
  parse(value: unknown) { ... },
  schema: ...,
};

export function seats(p: Plan): number {
  const __m0 = p;
  switch (__m0.tag) {
    case "Free": { return 1; }
    case "Pro": { return 5; }
    case "Team": { return 50; }
    default: throw new Error("non-exhaustive match");
  }
}
```

Delete the `Team` arm and the build stops before any TypeScript is written:

glyph check --no-tsc (trimmed)

```
[E0200] Error: typecheck: non-exhaustive match on `Plan`: missing variants `Team`
   ╭─[ plan:6:10 ]
   │
 6 │ ╭─▶   return match p {
   ┆ ┆
 9 │ ├─▶   }
   │ ╰───────── non-exhaustive match on `Plan`: missing variants `Team`
   │     Help: Add an arm for each missing variant.
```

If you stop using Glyph, the emitted `.ts` is what you keep. It has no runtime dependency beyond a small bundled support directory the build writes next to it.

## npm, and the TypeScript you already have

A `.glyph` module imports an npm package by name (`import kafkajs { Kafka }`), typed from the package's own declarations, and your existing `.ts` imports the emitted Glyph modules like any local file. So you can start with one module inside an app that stays TypeScript. The details are in three answers.

- [Can I use my existing TypeScript, JavaScript, and npm libraries? Importing packages, keeping a .ts shim, and importing Glyph back into .ts.](https://glyphlang.io/answers/imports/)
- [Can I adopt Glyph inside my existing app? The domain core in Glyph, the rest in TypeScript, in a stock Vite scaffold.](https://glyphlang.io/answers/embedding/)
- [Does it work with React? Components, JSX, hooks, Context, and where tsc still does the checking.](https://glyphlang.io/answers/react/)

## Where to go next

- [Recommended See what TypeScript lets through Each case run under tsc --strict and under Glyph, with the error. See the catches](https://glyphlang.io/catches/)
- [Write your first program Install, scaffold, run, break it on purpose, read the error. About ten minutes.](https://glyphlang.io/start/)
- [Try it in the browser The compiler runs in the page, with nothing to install.](https://glyphlang.io/playground/)
