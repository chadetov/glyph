<!-- Markdown copy of https://glyphlang.io/answers/, generated from the same source as the page. Last changed 2026-11-25. -->

Answers

# What engineers ask about Glyph.

Each answer shows a worked example and ends with what does not work yet. Start wherever your doubt is. New to Glyph? The [10-minute Start Here](https://glyphlang.io/start/) is the fastest first run.

## The problem it solves

What an agent's edit can break without anyone noticing, and what Glyph does about each.

- [01 What stops an agent from silently breaking my code? No any, no casts, exhaustive match, runtime-checked types, and colocated tests that run on every build.](https://glyphlang.io/answers/verifiability/)
- [02 Will agent edits stay clean, or reformat my whole file? A one-line change is a one-line diff. One canonical layout, nothing repacked.](https://glyphlang.io/answers/diff-stability/)
- [03 Can an agent (or I) find where anything is defined? One name, one declaration form. grep always lands on it.](https://glyphlang.io/answers/greppability/)

## Deciding whether to adopt it

Why a language rather than more tooling, and what taking it on costs.

- [04 Why TypeScript, and not Go or a new language? Because that's where your ecosystem is, and it's why adoption is per-file.](https://glyphlang.io/answers/why-typescript/)
- [05 Why not just strict TypeScript + zod + eslint + ts-pattern? Non-optionality: the language the agent can't config-disable.](https://glyphlang.io/answers/vs-tooled-ts/)
- [06 Is Glyph worth it if I'm not using AI? Yes: the guarantees are plain engineering wins a human gets directly.](https://glyphlang.io/answers/human-value/)
- [07 Can an AI reliably write a language it's barely seen? Most of what a model knows about TypeScript carries over, and the compiler names the places it doesn't.](https://glyphlang.io/answers/ai-authoring/)
- [08 Will a new Glyph release break my build? Projects pin the compiler exactly, so no release arrives by accident. glyph doctor says one exists; glyph upgrade takes it.](https://glyphlang.io/answers/upgrades/)

## Using it with your stack

npm packages, an existing app, API boundaries, React.

- [09 Can I use my existing TypeScript, JavaScript, and npm libraries? Yes: import any npm package, keep a hand-written .ts shim where you need one, and import Glyph back into .ts.](https://glyphlang.io/answers/imports/)
- [10 Can I adopt Glyph inside my existing app, or is it all-or-nothing? No rewrite: the domain core in Glyph, the rest in TypeScript. The emitted TypeScript is imported by a stock Vite app like any other module.](https://glyphlang.io/answers/embedding/)
- [11 Do I have to hand-write a validator for every API? No. Declaring a type generates its validator.](https://glyphlang.io/answers/typed-apis/)
- [12 Can I generate a typed client and server from my API spec? glyph gen openapi/zod/dts: committed, greppable code.](https://glyphlang.io/answers/codegen/)
- [13 Does it work with React: components, hooks, and Context? First-class JSX, hooks, and exhaustive reducer actions.](https://glyphlang.io/answers/react/)
- [14 How do I share mutable state between functions? A store: a const whose .set/.update are the write-surface.](https://glyphlang.io/answers/state/)

## What you can build with it

The language and standard library, by the kind of program.

- [15 Is this a real language, or a toy with gaps? Module visibility, interface bounds, defer, a match that works as any value and carries its type, and a standard library that has slice, pad_start, and fold.](https://glyphlang.io/answers/language/)
- [16 Can I trust it with money, and with input I don't control? Exact decimals and ids, invariants as types, injection as a compile error.](https://glyphlang.io/answers/finance/)
- [17 Can I serve a web page, or only a JSON API? HTML, redirects, response headers, and form posts, without a hand-written server.](https://glyphlang.io/answers/web-serving/)
- [18 Can I write a program someone types into? io.read_line answers a line as it arrives instead of waiting for stdin to close, so a REPL or a game loop works.](https://glyphlang.io/answers/interactive-cli/)
- [19 Can I write a server, or does the process just exit? main sets up and returns; the process stays alive while sockets and timers are pending, the same rule Node uses.](https://glyphlang.io/answers/long-running/)
- [20 Can I handle binary data, or is everything a string? std/bytes is a sequence of octets, with codecs that refuse malformed input instead of skipping it, and HMACs that take a real key.](https://glyphlang.io/answers/binary/)

## Working with an agent

Questions an agent asks the compiler before and after an edit.

- [21 Can the compiler tell me what a change will break before I make it? glyph_impact: one verdict per site, from a closed list, with the reason, before you touch the file.](https://glyphlang.io/answers/impact/)
- [22 Can my agent ask the compiler what a symbol is before it writes code? glyph_symbol: a union's variants with the syntax that constructs each, in one call. Eleven tools, each with a glyph query verb.](https://glyphlang.io/answers/ask-the-compiler/)
- [23 When the build fails, does the compiler tell my agent how to fix it? The diagnostic carries the symbol at fault, the legal alternatives, and what a repairing edit must not break. glyph fix writes the ones the compiler settles on its own.](https://glyphlang.io/answers/repair-protocol/)
- [24 Is the documentation my agent reads generated, or written by hand and out of date? glyph llms --json emits the diagnostics, the prelude, the stdlib signatures and the tool manual out of the compiler's own tables, and a gate fails when a hand-written copy disagrees.](https://glyphlang.io/answers/generated-docs/)
- [25 Can I stop an agent importing the raw library when we have a wrapper for it? Ban the import path in package.json and name the replacement. An import of it is E0115, and the help points at your wrapper.](https://glyphlang.io/answers/banned-imports/)

## Errors, editors and production

What you see when something is wrong, from the editor to on-call.

- [26 What's the editor, debugging, and error experience? A language server for any LSP editor, and errors with a stable code that name the fix.](https://glyphlang.io/answers/tooling/)
- [27 Why does Glyph catch what tsc catches, but earlier? await context, a match arm with no value, and a missing mut, reported against your source instead of the emitted TypeScript.](https://glyphlang.io/answers/early-errors/)
- [28 When a parse fails, do I find out why? Which field, which rule it broke, the where predicate it violated, and a code to branch on.](https://glyphlang.io/answers/parse-errors/)
- [29 When it breaks in production at 3am, what does on-call see? Readable TS you own, source maps back to your .glyph, and a run that reports what build reports.](https://glyphlang.io/answers/debugging/)
- [Read them in order →](https://glyphlang.io/answers/verifiability/)
