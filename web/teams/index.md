<!-- Markdown copy of https://glyphlang.io/teams/, generated from the same source as the page. Last changed 2026-11-21. -->

For teams

# What it guarantees, what adopting it costs, and how you get out

Glyph refuses a specific, checked list of cases that a strict TypeScript project accepts, and it compiles to TypeScript you can keep if you stop using it. It is an early preview: pre-1.0, maintained mainly by one person, and releasing several times a week.

## What it guarantees

Every claim about what Glyph catches is a case in the repository's `catches/` directory: the same program in TypeScript and in Glyph, both run by a CI gate. There are 48 checked cases: 37 TypeScript cases, 5 JavaScript traps that do not exist in Glyph, and 6 holes Glyph does not close yet. Glyph refuses 28 of them when you build (23 TypeScript cases and 5 traps). Of the mistakes it does not refuse, 3 draw a warning, 9 are bad data stopped at a runtime boundary check, and 2 stop at run time. The ones a reviewer of agent-written code leans on most:

| Code | What is refused |

|---|---|

| `E0200` | A `match` that leaves out a case of a union, so adding a case lists every place that has to change |
| `E0013` | A cast (`as`, `as unknown as`); there is no cast, and no `any` |
| `E0204` | Parsed JSON assigned to a typed name without being checked |
| `E0224` | A read from a map typed as if the key were always there |
| `E0217` | A `Result` dropped on the floor (a warning; `let _ =` says it was deliberate) |

[The catches page](https://glyphlang.io/catches/) lists every case with its TypeScript, its Glyph, and the error, and says which ones a fully configured typescript-eslint setup would also have caught.

## Where it stops

The guarantees cover code written in Glyph. An exception thrown inside an npm dependency still bypasses `Result`, and a type imported from a package's `.d.ts` has no runtime check. Inside Glyph, the 6 open holes are listed with their gap numbers in [what it does not catch yet](https://glyphlang.io/catches/#not-yet), and the full ledger is [docs/dogfooding-gaps.md](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md). For a comparison with strict TypeScript plus zod, eslint and ts-pattern, including where that setup is still ahead, see [why not tooled TypeScript](https://glyphlang.io/answers/vs-tooled-ts/).

## Adopting it inside an existing app

There is no rewrite. You write the part you can't afford to get wrong (models, permissions, state transitions, money) in Glyph, and the rest of the app stays TypeScript and imports the emitted modules like any local file. The walkthrough uses a stock Vite scaffold: [Can I adopt Glyph inside my existing app?](https://glyphlang.io/answers/embedding/)

## The exit story

`glyph build` writes plain TypeScript, one file per module, that passes `tsc --strict`. If you stop using Glyph, you commit that output and keep going with `tsc`. The licence is MIT or Apache-2.0, so a fork is always possible. [Coming from TypeScript](https://glyphlang.io/from-typescript/) shows a module next to its emitted file.

## Pinning and upgrades

`glyph init` pins the compiler to an exact version in `package.json`, with no `^`, so no release reaches your build by accident. Three commands handle the rest:

upgrading on purpose

```
glyph doctor     # your version against the latest release; never fails CI for that
glyph upgrade    # moves the project's pin, runs npm install, links the notes
glyph --update   # moves the installed compiler itself
```

Details in [Will a new Glyph release break my build?](https://glyphlang.io/answers/upgrades/)

## Release cadence and the Breaking marker

The [versions page](https://glyphlang.io/versions/) has an entry for every release from 0.1.0 to 0.1.159, 158 in all, and 32 of them shipped between 1 Nov 2026 and 21 Nov 2026. A release is marked Breaking when it rejects a program the previous version accepted, which is how most correctness fixes land. 42 entries carry the marker. It is applied by running the smallest affected program under both versions, not by reading the diff, and releases before 0.1.99 were not tested this way, so an unmarked old release is untested rather than safe.

What a Breaking release has cost in practice: each recent note says what happened when the old and new versions were run over the repository's example apps. These are the newest Breaking releases, quoted from their notes:

- [0.1.159](https://glyphlang.io/versions/2026-11/#0.1.159): The note does not say.
- [0.1.158](https://glyphlang.io/versions/2026-11/#0.1.158): The note does not say.
- [0.1.157](https://glyphlang.io/versions/2026-11/#0.1.157): “Writing this rule turned up a real bug in one of the example apps.”
- [0.1.156](https://glyphlang.io/versions/2026-11/#0.1.156): The note does not say.
- [0.1.155](https://glyphlang.io/versions/2026-11/#0.1.155): The note does not say.
- [0.1.154](https://glyphlang.io/versions/2026-11/#0.1.154): “In the repository, two example apps did, and both were real bugs: the chat server's `main` started `daemon.serve(port)` and returned before the server bound, and the job queue's timer dropped each `drain`.”

The example apps are the project's own code, kept compiling release by release, so a newly refused shape is less likely to turn up in them than in yours. Each note names the shapes it refuses, so read it before upgrading.

## Status

Glyph is pre-1.0. Before 1.0, syntax, the standard library and the shape of the emitted TypeScript may change, and a release may reject code that compiled before. [docs/stability.md](https://github.com/chadetov/glyph/blob/main/docs/stability.md) says what may change and what is held stable. [MAINTAINERS.md](https://github.com/chadetov/glyph/blob/main/.github/MAINTAINERS.md) says plainly that the project is maintained primarily by one person today.

## Objections, answered

- [We don't use AI much. Is it still worth it? The checks are the same whoever writes the code: fewer runtime surprises, smaller diffs, code you can grep.](https://glyphlang.io/answers/human-value/)
- [We already run strict TypeScript and lint. Every one of those checks is opt-in and can be turned off in one line. Glyph's can't.](https://glyphlang.io/answers/vs-tooled-ts/)
- [Can our agents write a language they've barely seen? Most of what a model knows about TypeScript carries over, and the compiler names the rest.](https://glyphlang.io/answers/ai-authoring/)
- [Why compile to TypeScript at all? That is where npm, React and Node are, and it is why adoption can be one file at a time.](https://glyphlang.io/answers/why-typescript/)
- [What does on-call see when it breaks? Readable TypeScript, source maps back to the .glyph file, and stable error codes.](https://glyphlang.io/answers/debugging/)
- [How do we know any of this is true? How each claim on this site is checked, and how to reproduce one yourself.](https://glyphlang.io/evidence/)

## Where to go next

- [Recommended See what TypeScript lets through The cases a strict TypeScript project accepts, ranked, each run under tsc --strict and under Glyph. See the catches](https://glyphlang.io/catches/)
- [Write your first program Install, scaffold, run, break it on purpose, read the error. About ten minutes.](https://glyphlang.io/start/)
- [Try it in the browser The compiler runs in the page, with nothing to install.](https://glyphlang.io/playground/)
