<!-- Markdown copy of https://glyphlang.io/answers/vs-tooled-ts/, generated from the same source as the page. Last changed 2026-11-21. -->

05 · The comparison

# Why not just strict TypeScript + zod + eslint + ts-pattern?

Because every one of those guarantees is **opt-in**, and an agent can route around each of them in one line. Glyph's strictness isn't a config file the agent can edit. It's the language. The enforcement point moves from "did we remember to lint this file" to "does it compile."

A well-configured TypeScript setup gets you a long way, and if your team runs all of it in CI you've already closed most of the gaps Glyph targets. It's the alternative worth taking seriously, so here's the mapping:

what tooled TypeScript already gives you

```
no `any`            → noImplicitAny + @typescript-eslint/no-explicit-any
exhaustive match    → ts-pattern .exhaustive() (compile-time, today)
errors as values    → neverthrow / Effect
runtime validation  → zod at every boundary
must-handle errors  → an eslint rule
deterministic diffs → Biome / Prettier
```

What tooled TypeScript already covers. If you have all that wired up and enforced, you're most of the way there. TypeScript has these ideas. What a language adds by making them non-optional comes down to three things.

## 01 Non-optional beats opt-in

Every one of those escape hatches is a config the agent can edit. Glyph has no such hatch to reach for: there is no `as`, no ignore pragma, no way to get a typed value out of `unknown` without a check the compiler can see.

TypeScript: the hatches exist handler.ts

```
const user = body as unknown as User;
// @ts-ignore
doThing(user.mispelled);
// eslint-disable-next-line
return anything as any;
```

Glyph: nothing to disable handler.glyph

```
return match User.parse(body) {
  Ok(user) => use(user),
  Err(_) => fallback(),
}
// no `as`, no ignore pragma,
// no lint to switch off.
```

Strictness the agent cannot config-disable sits at a different enforcement point than strictness it can. Glyph is built on that difference.

**Details what an agent can write past eslint **

Your eslint config only protects code the agent routes *through* it, and the agent can write `as unknown as User`, `@ts-ignore`, `// eslint-disable-next-line`, or simply forget the `.exhaustive()`, and it still compiles and merges.

## 02 The type is the validator: no second artifact

In Glyph the type *is* the validator: declaring `type User = {...}` generates `User.parse` alongside it. One artifact, nothing to keep in sync, nothing to forget.

- **zod: schema first** zod inverts your source of truth: you write the *schema*, derive the type from it, and then keep the type, the schema, and any serializer in sync by hand.
- **Where drift comes in** Keeping those aligned is exactly where an agent introduces drift: it renames a field in the type and not the schema, and both still compile.

## 03 Greppable mutation

In Glyph shared state lives in a store and every change is a literal `.set(`/`.update(`; `grep 'mut '` finds every local mutation. Stock TypeScript has no equivalent.

**Details where a TypeScript write can hide **

In TypeScript a state change can hide behind `=`, `.push()`, `Object.assign`, a setter, or a spread. There's no single token to search for, and no lint rule fully fixes it.

## The failure-mode diff

Same task, an agent adds a variant to a union and forgets to handle it in one of three places:

tooled TS

```
// ts-pattern .exhaustive() catches it,
// IF every site used ts-pattern and
// nobody wrote a default: case or a
// switch that only logs. Those
// compile green and skip the new case.
```

Glyph

```
[E0200] non-exhaustive match on
`Feed`: missing variant `Archived`
// every unhandled match, everywhere,
// fails the build. No opt-in, no
// escape, no forgotten default.
```

## Where tooled TypeScript is still ahead

Four places where it is ahead of Glyph today:

- **Richer validation.** Glyph descriptors are strict by default (they reject undeclared keys), and a `where` refinement puts a predicate on a primitive (`type Email = string where value.includes("@")`), which the descriptor runs at every boundary the type reaches. zod is still ahead in two ways: it ships ready-made checks such as `.email()` and `.url()` that you write yourself in Glyph, and it refines whole objects, while a `where` on a record (`lo <= hi` across two fields) is `E0300` today.
- **Deeper matching.** ts-pattern's guards and deep structural patterns go beyond what Glyph's `match` expresses.
- **"Provable absence" stops at the `.ts` boundary.** Glyph's guarantees hold for code Glyph authored; a thrown exception from an npm dependency still bypasses `Result`, and an imported `.d.ts` type carries no runtime descriptor.
- **Ecosystem and hiring.** The TypeScript tooling, community, and hiring pool are far larger, with years of production use behind them. Glyph is an early preview.

If you've fully wired tooled TypeScript and your agents stay inside it, you've got most of this. Glyph's argument is that "stay inside it" is the load-bearing phrase, and it's the phrase an agent breaks. Non-optionality, one-artifact validation, and greppable mutation are the parts you cannot get by adding more opt-in tools.

**Details ReScript, Gleam or Elm **

If you are open to leaving TypeScript for an older typed language that compiles to JavaScript, [Why not ReScript, Gleam or Elm?](https://glyphlang.io/design/vs-rescript-gleam-elm/) compares Glyph with those.

## Where it stands

### Shipping today

- No `any`/`as`.
- Exhaustive `match`. E0200 E0218
- Discarded-`Result` warning. E0217
- Type-generated validators.
- Greppable mutation.
- All non-optional, none an eslint rule you can disable.

  **Details**

  No `any`/`as`, exhaustive `match` (`E0200`/`E0218`), discarded-`Result` warning (`E0217`), type-generated validators, and greppable mutation: all non-optional, none an eslint rule you can disable.

### Being measured

- The claim that agents produce fewer escaped defects in Glyph than in *well-configured* strict TS is the benchmark we're building, against tooled TS, not naive TS. See the [benchmarks](https://glyphlang.io/benchmarks/).

Code examples on this page are checked against Glyph 0.1.165 on every build. Last changed 21 Nov 2026.
