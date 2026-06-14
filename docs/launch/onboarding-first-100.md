# Onboarding the First 100 Users

*Internal plan. Glyph is v0.1 and early. Everything here is a plan, not a result.*

## Why these 100 matter

The first 100 users are not a market. They are co-designers. The point of this cohort is not adoption numbers but signal: which restrictions earn their keep, which pillars actually land in practice, and where the friction is bad enough that a real user walks away. Glyph's central productivity claim — that agents write correct code faster in Glyph — is currently a hypothesis supported by structural metrics, not a measured study. This cohort is how we start to learn whether it holds, and what we got wrong.

Treat every one of these users as someone whose friction log changes the language. Recruit accordingly: fewer, more invested users beat a spike of curious tire-kickers who never write a second file.

## Who we are looking for

The wedge is verifiability and greppability — the two pillars that fix problems TypeScript developers feel today. So the target is not "people who like new languages." It is people who already feel the specific pain Glyph addresses:

1. **TypeScript developers frustrated with agent-generated code.** The person who has watched an agent write `input as User`, ship it, and crash in production. The person who has seen an agent add a union variant and forget a `switch` case, with `tsc --strict` saying nothing. This is the sharpest match for the two demos we can show today (exhaustiveness and the missing cast escape hatch).
2. **People building with coding agents.** Anyone wiring up agent loops, codegen pipelines, or autonomous PR bots — they have a direct stake in code that is safe for a machine to modify.
3. **Static-typing and FP-leaning TS developers.** People who already reach for `Result`-style error handling, exhaustive matching, and "make illegal states unrepresentable." Glyph gives them as language primitives what they hand-roll.

We are explicitly *not* chasing: people who want a general-purpose language to replace TypeScript wholesale, or people allergic to transpile-to-TS. Glyph compiles to readable TypeScript, runs anywhere TS runs, and uses any npm package — lead with that to defuse the "another language to learn" objection.

## Where to find them

Concrete channels, roughly in order of fit. Each one needs a tailored framing, not a copy-paste.

- **Show HN.** The single highest-leverage launch. Frame it honestly: a typed language that transpiles to TypeScript, built so agents can edit code safely. Lead with the two verifiability demos (a bug `tsc --strict` accepts that Glyph rejects at compile time) and the in-browser playground, because both are things a reader can verify in 60 seconds without installing anything. Be upfront that it is v0.1, that the productivity claim is a hypothesis, and that the token numbers are an approximate proxy. HN punishes overclaiming; honesty is also the better strategy here.
- **Relevant subreddits.** r/typescript, r/programming, r/ProgrammingLanguages, and AI-coding-adjacent subs. Different subs want different angles: r/ProgrammingLanguages wants the design rationale (the four pillars, the no-cast decision, errors as values); r/typescript wants the delta sheet and the concrete bug Glyph catches.
- **Discord/Slack communities.** TypeScript and AI-tooling Discords, agent-framework communities, and the servers around popular coding-agent projects. These are where the "agents write bad code" frustration is voiced daily; show up as a participant, not a billboard.
- **Conference and meetup CFPs.** Submit talks on "writing code that is safe for an agent to modify" to TS, web, and AI-engineering conferences and local meetups. A CFP-driven talk forces a tight narrative and produces a recording we can reuse.
- **Direct outreach.** Hand-pick people already posting about agent-generated-code pain and reach out individually with a link to the playground and one specific demo relevant to what they complained about. Ten thoughtful DMs beat one broadcast.
- **Existing example programs and the playground as bait.** Every public artifact (playground link, a demo gist showing the rejected `as` cast) should be shareable on its own and link back to getting-started.

## The onboarding path

A staircase, lowest commitment first. The goal is to get a user to "I saw Glyph reject a bug TypeScript accepted" as fast as possible, ideally before they install anything.

1. **Playground first (zero install).** The in-browser WebAssembly playground runs with no backend: write Glyph, see the emitted TypeScript and diagnostics instantly. First touch should almost always be a playground link, pre-loaded if possible with one of the verifiability demos so the payoff is immediate.
2. **The TypeScript-developer delta sheet.** For anyone who writes TS, the "Glyph for TypeScript developers" guide is the bridge. It frames Glyph as the deltas from a language they already know: errors as values (`Result` + `match` + `?`), exhaustive `match`, no `any`, no cast escape hatch, one declaration form per symbol. Lead with this, not with a from-scratch language intro.
3. **The five-minute tour and getting-started.** For users ready to read top-to-bottom.
4. **Install via npm.** `npm install -g glyph` or `npx glyph`. The distribution is esbuild-style: a launcher plus per-platform prebuilt binaries, so there is no Rust toolchain to set up. The VS Code extension (with the language server: diagnostics, hover types, go-to-definition, completion, format-on-save) is the recommended editor setup.
5. **A starter task.** Point new users at the 30-minute todo-CLI tutorial (every snippet compiles) as the guided path, then a real starter task in their own domain. The best starter task is one that exercises the wedge: define a tagged union, write an exhaustive `match`, parse untrusted input into a validated type (no `as`), and handle errors with `Result`. That is where the pillars are supposed to land, so that is where we want eyes.

A user who reaches "I wrote my own small program and `glyph build --check` passed" has cleared the bar that matters. Track how many get there.

## What feedback to collect

The deliverable from each user is a **friction log**: a running, low-ceremony record of every moment they got stuck, annoyed, or surprised. We provide a one-page template and ask for raw notes, not polished bug reports. Specifically, we want:

- **Which restrictions annoy, and whether they recanted.** Glyph is deliberately stricter than TypeScript: no `if`/`else` (`match` is the only conditional), required trailing commas, one declaration form per symbol, no cast escape hatch, one fixed formatting layout. For each restriction a user hits, log: did it block them, did it merely surprise them, and after the explanation did they accept it or still want it gone? A restriction that everyone fights and no one comes to value is a candidate for re-examination. (The default response to "this is annoying" is documentation, not loosening — but the friction log is exactly how we learn when that default is wrong.)
- **Which pillars land.** When a user says "oh, that's the point" — log which pillar triggered it. We expect the wedge (verifiability, greppability) to land hardest and the polish (abstraction, diff stability) to land later or not at all on first contact. If verifiability does *not* land — if the exhaustiveness or no-cast demos read as pedantic rather than valuable — that is the most important possible signal and we want it surfaced loudly.
- **Where install, playground, or the toolchain broke.** Platform-specific install failures (the per-platform binary packaging is new), playground confusion, missing tsx/tsc on PATH for `glyph run`/`--check`/`--test`, the `glyph build --out` stale-file caveat. Concrete repro steps.
- **The agent question.** For users running coding agents against Glyph: did the agent produce code that built clean? Did the typecheck-gated structured-edit RPC (`glyph/applyEdit`, which only applies an edit if the result type-checks) behave as hoped? This is the closest we get to evidence for the productivity hypothesis, but it is anecdotal — record it as observation, not proof, and never let it harden into a speedup number.
- **What they expected Glyph to have and it did not.** Deferred typechecker features (a fuller unifier is planned for v1.1) will surface here. We do not claim Glyph catches every type error TypeScript misses — only the demonstrated ones. Mismatches between expectation and reality belong in the log.

Where we publish numbers, keep the caveats attached: the token counts are an approximate dependency-free proxy (not tiktoken-exact), only three functions are measured so far, and a real tokenizer would change the absolute numbers but not the ranking.

## A lightweight cadence

The cadence is deliberately small. The bottleneck is our attention, not user volume.

- **On signup / first contact:** a short personal welcome (not automated where avoidable), a playground link tailored to what they care about, the delta sheet, and the friction-log template. One ask: try the starter task and send raw notes.
- **Weekly:** a short internal triage of every friction log received that week. Tag each item by pillar and by restriction. Watch for the same friction appearing across multiple users — that is the signal that promotes an item from "document it" to "reconsider it."
- **Per cohort milestone (e.g. every ~10 onboarded):** a written internal summary of what landed, what annoyed, and what broke. No external numbers published unless they are honest and caveated.
- **Direct follow-up:** for any user who got far (wrote their own program, ran an agent against it), a real conversation. These are the co-designers; treat their time as the most valuable input we have.

## Guardrails for everyone doing outreach

- Frame Glyph as v0.1 and early in every public touch.
- Never state a productivity speedup. "Agents write correct code faster in Glyph" is a hypothesis these structural metrics support; it has not been measured with a real agent study. Do not invent a multiplier.
- Token numbers are an approximate proxy, always caveated.
- Claim only the verifiability we can demonstrate (exhaustiveness, no unsafe cast). Do not claim Glyph catches every type error TypeScript misses.
- The repo is github.com/chadetov/glyph, licensed MIT OR Apache-2.0. Lead with the playground and the two demos, because they let a skeptic verify the claim themselves in under a minute. That self-verification is the most persuasive thing we have, and it is also the most honest.
