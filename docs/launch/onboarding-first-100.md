# Onboarding the first 100 users

*Internal plan for after the preview announcement. Nothing here has happened
yet.*

## What the first users are for

The first hundred people to write Glyph are a source of findings, not an
adoption number. Most of what changed the compiler in the 0.1.x line came from
building programs and stopping where Glyph could not express something or
accepted something it should not have; the apps under `examples/apps/` and outside
projects that re-test releases found more than internal review did. New users run into the same kind of gap, with programs nobody on the
project would have thought to write. A user who sends one reproduction of
something Glyph lets through has done more for the language than ten who install
it and leave.

They are also the first chance to learn anything about the productivity
hypothesis outside my own head. That learning is anecdotal and should be
written down as observation. It does not turn into a number.

## Who to look for

People who already feel the problems the `catches/` programs show:

1. TypeScript developers reviewing agent-written code who have seen an
   unvalidated `JSON.parse` or a `switch` with a missing case get through
   `tsc --strict` and into production.
2. People building with coding agents (agent loops, codegen, bots that open
   pull requests), who care whether a machine can edit the code safely and who
   can point an agent at `AGENTS.md` and the MCP server on day one.
3. TypeScript developers who already hand-roll `Result` types and exhaustive
   matching and would rather have them in the language.

People looking for a general replacement for TypeScript are not the target, and
neither are people who cannot add a compile step. Glyph emits TypeScript and
imports npm packages, so it can be adopted one module at a time, and that is the
answer to "another language to learn".

## Where to find them

- Show HN, using `show-hn.md`. Lead with the `catches/` pairs and the
  playground, because a reader can check both in a minute without installing
  anything. Say in the post that the productivity claim is unmeasured.
- r/typescript and r/ProgrammingLanguages. The first wants the bugs Glyph
  catches and the TypeScript delta sheet; the second wants the design
  reasoning (no cast, errors as values, exhaustive `match`, one formatter
  layout).
- TypeScript and agent-tooling Discords and Slacks, as a participant answering
  questions where the topic comes up.
- Direct messages to people who have written publicly about agent-generated
  bugs, each with the one `catches/` case closest to what they described.

## The path a new user takes

1. The playground at glyphlang.io/playground, no install. The first link anyone
   gets should open a program that shows a refusal.
2. `docs/guide/for-typescript-developers.md` for anyone who knows TypeScript,
   then `docs/guide/tour.md`.
3. `npx @glyphlang/glyph init hello`, `npm install`, `npx glyph run`. This
   needs Node; `glyph doctor` reports a missing or old `tsx` or `tsc`.
4. `docs/guide/tutorial.md` (a todo CLI) as a guided first program, then
   something from their own work. A good first task touches the parts that
   differ from TypeScript: a union matched exhaustively, input parsed with
   `T.parse` instead of cast, failures returned as `Result`.
5. For agent users: point the agent at the `AGENTS.md` that `glyph init` wrote
   and let it use the MCP server in `.mcp.json`, then record what the agent got
   wrong.

The bar worth counting is a user who wrote a program of their own and got
`glyph check` to pass.

Before announcing, the outside-user run in the readiness pass walks this path cold
and reports where it breaks. Fix what it finds first.

## What to collect

A friction log from each user: short raw notes, written as they go, of every
point where they were stuck, surprised or annoyed. Specifically:

- Programs Glyph accepted that it should have refused. These go straight into
  the gap ledger with a reproduction, the same as an internal finding.
- Things they could not express. The same rule applies as for the dogfood apps:
  record the gap with the smallest program that shows it, and do not let the
  user's workaround stand in for a fix.
- Restrictions that got in the way (no `if`/`else`, required trailing commas,
  no cast, one formatter layout), and whether they still objected after reading
  why the restriction exists. The default answer to an annoying restriction is
  better documentation, and the log is how I find out when that answer is
  wrong.
- Install and toolchain failures: platform, Node version, the exact output.
- For agent users: whether the agent's code built, how many repair rounds it
  took, which diagnostics it misread, and whether it used the MCP tools without
  being told. Recorded as observation. Do not aggregate it into a speedup.

## Cadence

- On first contact: a personal reply, a link chosen for what they care about,
  and the friction-log template. One request: write something and send the raw
  notes.
- Weekly: triage the logs. Every accepted-but-wrong program and every
  inexpressible thing becomes a ledger entry and gets scheduled in
  `docs/roadmap/releases.md`, because a finding that is recorded but not
  scheduled gets rediscovered.
- Every ten or so users: a short internal summary of what landed and what did
  not.
- Anyone who got far (their own program, or an agent run against Glyph) gets a
  conversation.

## Rules for anyone doing outreach

- Say it is a preview at 0.1.x and that releases still break programs.
- Never state or imply a productivity speedup. The hypothesis has not been
  tested.
- Claim only what `catches/` demonstrates and `check_catches.py` keeps true.
  Glyph does not catch every error TypeScript misses, and in some places it
  catches less than `tsc` does on its own (G273), with `tsc` running in the
  build to cover it.
- Say plainly that values from npm are typed by `tsc`, not by Glyph's checker
  (G257). People will find it in their first hour with React.
