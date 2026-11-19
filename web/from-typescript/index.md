<!-- Markdown copy of https://glyphlang.io/from-typescript/, generated from the same source as the page. Last changed 2026-11-19. -->

Coming from TypeScript

# What changes when you already write TypeScript

Less than you'd expect. Glyph compiles to TypeScript that passes `tsc --strict`, keeps npm and your deploy, and reads almost the same. The differences are a handful of deltas, and each one trades a way to be wrong for a check the compiler makes.

## If you know TypeScript, you know most of this

You don't learn a new language, just these four. They are the ones in every sample on this site.

`match`

An exhaustive `switch`: the compiler makes you handle every case. There is no `if`/`else`; `match` is the only conditional, so a case can't be silently forgotten.

`Result<T, E>`

Errors as return values, not thrown exceptions. You handle one with `match`, or propagate it with the `?` operator.

`fn f() -> T`

A function; the `->` is its return type. A `type X = {...}` is a record, and a `type X = A | B` is a tagged union.

`mut x = e`

Reassignment is explicit and greppable. A plain `let` binding never changes under you; you write `mut` to change one.

"Agents" on this site means AI coding assistants such as Claude, Copilot and Cursor. Glyph helps them most, but the checks are the same whether a person or a model wrote the code. If you already run strict TypeScript with zod and eslint, [here is what a language adds over that](https://glyphlang.io/answers/vs-tooled-ts/).

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
