# What Glyph catches that `tsc --strict` does not

Each directory here is one program written twice: a TypeScript program that a
strict project accepts, and the same program in Glyph. A `case.toml` says what
each half does, and `scripts/check_catches.py` runs both, so every claim is
checked rather than asserted. The TypeScript has to keep passing `tsc
--strict`, or the case proves nothing. The Glyph has to keep doing what the case
says, or the guarantee it documents is gone.

The cases are also the source for the "mistakes TypeScript lets through" page.
A case with a `section` is on that page, so the page cannot claim something the
compiler stopped doing.

There are 30 cases. By pillar, 27 are evidence for verifiability, 1 for
greppability and 2 for abstraction. None is evidence for diff stability, and the
check prints that zero rather than leaving it off. By kind, 5 are refused, 3
warned, 9 checked at the boundary, 2 stopped at run time, and 11 are gaps Glyph
does not catch yet. The check prints these counts on every run.

A case that stops being demonstrable gets deleted, not softened. Six have been.
An `await` in a synchronous function looked like a good case until it turned
out `tsc` rejects that too. A local type named `Error` was a case until G63 made
it legal. Four more went in the review before the launch, each for a reason
that was checked by running it:

- A union imported from another module, matched without one of its cases. The
  TypeScript half was a single file, and importing the union changes nothing on
  the TypeScript side, so this was the plain exhaustiveness case again. The
  Glyph behaviour is a compiler test now.
- A typo on a field of an imported record. TypeScript catches that too
  (TS2339) when the import is typed. The case passed only because its
  TypeScript took the value as `any`, which is a different program.
- A `Nullable<int>` passed where an `int` is declared. The TypeScript was safe
  at run time, and `tsc` catches the widened forms the comment worried about.
  Glyph is stricter there, but that is not a bug TypeScript lets through.
- A local type named `Array`. What the case said goes wrong, a later
  `Array<string>` meaning something else, is TS2315 in TypeScript. Glyph refuses
  the name because its emitter needs the global. That was the only greppability
  case.

Run them:

```sh
python3 scripts/check_catches.py            # every case
python3 scripts/check_catches.py duplicate-case invalid-date   # some of them
```

The script installs TypeScript, eslint with typescript-eslint, `tsx` and the
real `@types/node` on every run, because one case imports a socket type from
`net` the way a real project would and the lint verdicts need the type
information. It needs npm and the network, and it fails rather than skips
without them. A run takes about half a minute after the install.

## What a case says

`case.toml` is TOML, read with Python's `tomllib`. Every string is a one-line
`key = "value"`, because `glyph-cli/build.rs` reads `kind`, `code`, `pillar`
and `title`, and a `glyph-wasm` test reads `kind` and `code`, with a one-line
rule.

Only refusals leave this directory. `glyph llms --negative`, `glyph --explain`
and the playground's picker take a case when its `kind` is missing or
`refused` and its code starts with `E`; the picker also needs exactly one
`.glyph` file. Warnings, boundary checks, run-time stops and gaps are evidence
for the page and the gate, not wrong programs the compiler refuses. An unknown
`kind` fails the compiler build and the playground generator rather than being
skipped.

The fields every case has:

- `code`, `pillar`, `title`: the diagnostic, which pillar the case is evidence
  for, and a one-line title.
- `kind`, which decides what the Glyph half is checked for. It is `refused`
  when left out.

| Kind | What the check requires of the Glyph |
|---|---|
| `refused` | The build fails with `code` as an error. A `TS....` code means the refusal comes from the `tsc` pass `glyph build` runs on the emitted program, not from Glyph's own checker, and the case's entry below has to say so. |
| `warned` | The build passes and reports `code` as a warning. |
| `boundary` | The program builds and runs, and its output contains `glyph_output`. This is the check at the boundary turning bad data into an `Err`, or the program simply doing the right thing. |
| `runtime` | The program builds, then stops when it runs, with output containing `glyph_output`. |
| `gap` | A hole Glyph has today, named by `gap_id`. Glyph accepts the program and the run prints the wrong answer in `glyph_output`. With a `TS....` `code`, the hole is that Glyph's own checker (`glyph check --no-tsc`) accepts the program and only the `tsc` pass refuses it, so the editor and the agent tools miss it. When a fix closes the hole, the check fails and says to move the case to the kind it now is and close the gap. |

The fields that check the TypeScript:

- `ts_output`: text the TypeScript program prints or throws when `tsx` runs it.
  A list means every item, and a `\n` inside one pins two lines together (the
  `non-exhaustive-match` case uses that to show nothing was logged between
  them).
- `fullconfig`: whether a fully configured TypeScript project catches the
  mistake. Fully configured means every strictness flag `tsc` has beyond
  `--strict`, plus eslint's recommended rules and typescript-eslint
  `strictTypeChecked`, with `switch-exhaustiveness-check`,
  `strict-boolean-expressions`, `require-array-sort-compare` and
  `require-atomic-updates` turned on.
  - `flag:<option>` (or `flag:<option>=false`): `tsc --strict` with that one
    option reports an error on the program.
  - `lint:<rule>`: the lint run reports that rule.
  - `none`: neither the full flag set nor the lint run reports anything. Where
    something fires that has nothing to do with the mistake, the case carries
    `ts_ok.ts`, the corrected program, and `none` means nothing fires on `ts.ts`
    that does not also fire on `ts_ok.ts`. With a `ts_ok.ts`, a `flag:` or
    `lint:` claim also has to stay quiet on the corrected program.

The page's fields, on every case with a `section` (`mistake`, `trap` or `gap`):
`slug` (the directory name), `order` (its position in that section), and the
display text copied from the page, HTML included. A mistake has
`page_title`, `why`, `likelihood` and `severity` (1 to 5), `ts_verdict`,
`glyph_verdict`, `ts_result`, `glyph_result` and `page_note`. A trap or a gap
has the one-line `summary` the page lists. `title` stays plain text with no
quotes or HTML, because `glyph llms --negative` prints it as it is.

The lines the page shows are marked in the sources with `// show` and
`// end show` on lines of their own, in both `ts.ts` and `glyph.glyph`, so the
excerpt on the page is code this check ran. The diagnostic the page shows under
a refused case is taken from the compiler's real output when the page is
generated, so no case stores it.

Extra `*.glyph` files in the directory are built alongside, and extra `*.ts`
files are checked and linted with `ts.ts`.

## The cases

| Case | What TypeScript accepts | Glyph | Pillar |
|---|---|---|---|
| `non-exhaustive-match` | A `void` handler whose `switch` misses a union case, so that case does nothing and nothing says so | `E0200` | verifiability |
| `absent-map-key` | A read from a `Record<string, string>` typed as `string`, printing "undefined" when the key is not there | `E0224` | verifiability |
| `split-into-a-pair` | `const [user, host] = s.split("@")`, typing `host` as a `string` that is `undefined` when there is no `@` | `E0230` | verifiability |
| `unverifiable-parse` | A `value is Conn` guard that only checks a `net.Socket` field is present, so `parse` returns a typed value it never validated | `E0304` | verifiability |
| `unvalidated-boundary-read` | `const user: User = JSON.parse(body)`, where the annotation launders `any` | `E0204` | verifiability |

`absent-map-key` uses a typo because it makes the hole obvious, but the typo is
not the point. Glyph refuses the correctly spelled `row.name` too, with the same
`E0224`, because a `Record<string, string>` says nothing about which keys exist.
TypeScript's `noUncheckedIndexedAccess` catches this, but it is not part of
`--strict`, and a project has to know to turn it on.

`non-exhaustive-match` is the shape an event handler or reducer takes. A
`switch` that returns a value from each case does not make a fair case, because
`tsc` rejects the missing return (TS2366) unless the function ends in a cast.
typescript-eslint's `switch-exhaustiveness-check` and the `assertNever` pattern
both catch the `void` version when a project adopts them. In Glyph the check is
not something a project opts into, and it cannot be turned off.

In `unvalidated-boundary-read`, `json.parse` returns a
`Result<unknown, Array<Issue>>`, and Glyph's checker refuses it where a `User`
is declared, so `glyph check --no-tsc` fails on it as well as `glyph build`.
In TypeScript `JSON.parse` returns `any`, and no setting changes that.

The page's entries that Glyph does not settle by refusing a program. Each row
is the case, what Glyph does, and what a fully configured TypeScript project
says.

| # | Case | Glyph | Fully configured TypeScript |
|---|---|---|---|
| 1 | `data-from-outside-the-program` | `Err` at the boundary | nothing |
| 5 | `error-nobody-looked-at` | warned, `E0217` | nothing |
| 7 | `type-guard-that-checks-less` | `User.is` is false | nothing |
| 11 | `value-outside-the-list` | `Err` at the boundary | nothing |
| 12 | `config-with-a-setting-missing` | `Err` at the boundary | nothing |
| 18 | `invalid-date` | `None` from `time.parse_iso` | nothing |
| 19 | `computed-value-never-used` | warned, `E0107` | flag `noUnusedLocals` |
| 22 | `comparing-records-with-triple-equals` | `==` compares the data | nothing |
| 24 | `negative-age` | `Err` at the boundary | nothing |
| 26 | `switch-fallthrough` | a `match` arm never falls through | flag `noFallthroughCasesInSwitch` |
| 27 | `median-with-a-fractional-index` | `RangeError` when it runs | nothing |
| 28 | `one-step-past-the-end` | `RangeError` when it runs | nothing |
| 29 | `code-after-return` | warned, `E0108` | flag `allowUnreachableCode: false` |
| 34 | `quantity-of-one-and-a-half` | `Err` at the boundary | nothing |

`type-guard-that-checks-less` takes `x: unknown` where the page shows `x: any`.
With `any`, the only thing the lint run reports is `no-explicit-any`, which is
about the parameter rather than the guard; with `unknown`, nothing reports the
guard claiming `User` after checking one field. `median-with-a-fractional-index`
and `one-step-past-the-end` carry a `ts_ok.ts`, because
`noUncheckedIndexedAccess` fires on the corrected index too.

What Glyph does not catch yet. Each is a `gap` case, and each fails the check
the day its gap closes:

| Gap | Case | What Glyph accepts | Fully configured TypeScript |
|---|---|---|---|
| G380 | `gap-g380-mutation-without-mut` | `xs.sort(f)` with no `mut` reorders the caller's array | nothing |
| G381 | `gap-g381-member-read-on-unknown` | a field read on `unknown`, refused only by the `tsc` pass (`TS18046`) | lint `no-unsafe-member-access` |
| G382 | `gap-g382-string-plus-number` | `input + 1` on a string returned as a number, refused only by the `tsc` pass (`TS2322`) | lint `restrict-plus-operands` |
| G384 | `gap-g384-dropped-async-call` | `forEach(async ...)`, so `done` prints before anything is saved | lint `no-misused-promises` |
| G385 | `gap-g385-record-in-template` | a record in `${}`, printing `[object Object]` | lint `restrict-template-expressions` |
| G386 | `gap-g386-or-on-a-number` | `requested \|\| 3`, turning a real `0` into `3` | nothing |
| G387 | `gap-g387-sql-by-interpolation` | a SQL query built with `${name}`, returning every row | nothing |
| G388 | `gap-g388-sort-as-text` | `.sort()` on numbers, giving `100,25,9` | lint `require-array-sort-compare` |
| G390 | `gap-g390-index-update-across-await` | a lost update on `c[0]` across `await` | lint `require-atomic-updates` |
| G391 | `gap-g391-null-in-optional-field` | a JSON `null` kept in an optional field, so a spread erases the default | nothing |
| G392 | `gap-g392-find-method-option` | `users.find(...).name`, refused only by the `tsc` pass (`TS2532`) | lint `no-non-null-assertion` |

## Adding one

A directory with `case.toml`, `ts.ts` and `glyph.glyph`, and `ts_ok.ts` when
the `fullconfig` claim needs a control. Then run the check: it tells you if any
half does not behave the way the case says.

The bar is that both programs do the same thing. If the TypeScript has to be
bent into a different shape to compile, the case is comparing two programs
rather than one language against another, and it belongs in the bin.
