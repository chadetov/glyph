# Show HN: Glyph, a TypeScript-shaped language that rejects bugs tsc --strict accepts

Glyph compiles to TypeScript, reads like it, and imports npm packages directly.
I built it for codebases where agents do a lot of the editing: the compiler
refuses a set of programs that `tsc --strict` accepts, and it answers questions
about a program (where a union is matched, what breaks if a declaration changes)
so an agent can ask instead of grepping.

A few things it refuses that `tsc --strict` accepts. `row.naem` on a
`Record<string, string>` is typed `string` in TypeScript and returns undefined;
in Glyph any read off a map is E0224, spelled right or not, and you read it with
`record.get`, which returns an `Option`. (`noUncheckedIndexedAccess` gets
TypeScript there too, but it is not in `--strict`.) `const [user, host] =
s.split("@")` types `host` as a string that is undefined when there is no `@`;
Glyph's destructuring `let` has to prove the length and refuses it (E0230). A
`void` handler whose `switch` misses a newly added union case does nothing for
that case; the same handler as a Glyph `match` is E0200. typescript-eslint's
`switch-exhaustiveness-check` catches that one, if you run it. In Glyph none of
these checks can be turned off. Each is a pair of programs in `catches/` in the
repo, and CI runs both halves so the claim fails loudly if it stops being true.
There are five pairs.

What I have not done is measure whether agents produce correct code more often
in Glyph. That is the reason the project exists and it is still a hypothesis.
There is no speedup number anywhere because I do not have one.

Things that are not there yet. Values from npm packages are typed by `tsc` over
the emitted TypeScript, not by Glyph's own checker, so the build catches a wrong
argument to a library function but the editor and the MCP tools do not see it.
The VS Code extension is not on the Marketplace. Releases are frequent and
most recent ones are breaking in the direction of rejecting more programs, so
pin the version. The gap ledger in `docs/dogfooding-gaps.md` lists every defect
found so far with a reproduction; 25 of 301 are open and 7 partly fixed.

Try it with `npx @glyphlang/glyph init hello && cd hello && npm install && npx glyph run`,
or in the browser at https://glyphlang.io/playground/, which compiles one file
and does not run `tsc`. The current release is 0.1.130.

Repo: https://github.com/chadetov/glyph (MIT or Apache-2.0). I would like to
hear about anything Glyph accepts that it should not.
