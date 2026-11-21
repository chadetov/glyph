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

There are 53 cases. By pillar, 45 are evidence for verifiability, 3 for
greppability and 5 for abstraction. None is evidence for diff stability, and the
check prints that zero rather than leaving it off. By kind, 31 are refused, 3
warned, 10 checked at the boundary, 2 stopped at run time, and 7 are gaps Glyph
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
python3 scripts/check_catches.py --write    # refresh every observed.json
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
excerpt on the page is code this check ran.

`observed.json` holds the output the page shows under each excerpt: the Glyph
diagnostic's first line and its `Help:` line, or the lines of the Glyph run
that carry `glyph_output`, and the lines of the TypeScript run that carry
`ts_output`. The check writes it with temporary paths stripped and fails when
what it observes differs from the committed file, so the site, which is
generated without running anything, cannot quote output the tools stopped
printing. Refresh it after a deliberate change with
`python3 scripts/check_catches.py --write [case ...]`.

Extra `*.glyph` files in the directory are built alongside, and extra `*.ts`
files are checked and linted with `ts.ts`.

## The cases

The five cases the corpus started with. On the page, `non-exhaustive-match`
is entry 2, `split-into-a-pair` 10 and `absent-map-key` 16;
`unvalidated-boundary-read` and `unverifiable-parse` are entries of their own.

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
typescript-eslint's `no-unsafe-assignment` does flag the assignment, which is
why the page's first entry is the `as User` form in
`data-from-outside-the-program` and this one is listed separately.
`unverifiable-parse` carries a `ts_ok.ts`, because
`noPropertyAccessFromIndexSignature` fires on the `Record` reads in the
corrected guard too. `split-into-a-pair` interpolates `host` rather than
calling a method on it, so `noUncheckedIndexedAccess` alone says nothing and
`restrict-template-expressions` is what catches it.

The page's other refusals:

| # | Case | Glyph | Fully configured TypeScript |
|---|---|---|---|
| 3 | `trusting-find-with-bang` | `E0210` | lint `no-non-null-assertion` |
| 4 | `error-the-signature-hides` | `E0204` | nothing |
| 6 | `filter-that-keeps-everyone` | `E0211` | lint `no-misused-promises` |
| 8 | `as-unknown-as` | `E0013` | nothing |
| 9 | `lost-update-across-await` | `E0225` | lint `require-atomic-updates` |
| 13 | `number-accepted-as-enum` | `E0211` | lint `no-unsafe-enum-assignment` |
| 14 | `connection-left-open` | `E0206` | nothing |
| 15 | `typo-nobody-sees` | `TS18046`, from the `tsc` pass, not Glyph's checker | lint `no-unsafe-member-access` |
| 17 | `typo-in-an-options-object` | `E0211` | nothing |
| 20 | `template-prints-undefined` | `E0235` | lint `restrict-template-expressions` |
| 21 | `number-that-is-not-a-number` | `E0204` | nothing |
| 23 | `change-hidden-in-a-helper` | `E0008` | nothing |
| 25 | `one-function-many-names` | `E0002` | nothing |
| 30 | `arrow-function-returns-nothing` | `E0003` | flag `allowUnusedLabels: false` |
| 31 | `array-covariance` | `E0211` | nothing |
| 32 | `two-types-same-shape` | `E0211` | nothing |
| 33 | `method-forgets-its-object` | `E0234` | lint `unbound-method` |
| 35 | `duplicate-case` | `E0305` | lint `no-duplicate-case` |

JavaScript traps that do not exist in Glyph. Each is refused because the
feature behind it is not in the language:

| Case | Glyph |
|---|---|
| `trap-map-parseint` | `E0103`, no `parseInt` |
| `trap-regex-lastindex` | `E0003`, no regex literals |
| `trap-foreach-method-this` | `E0003`, no classes |
| `trap-math-max-empty` | `E0103`, no `Math` |
| `trap-array-holes` | `E0228`, no `Array(n)` |

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
| G386 | `gap-g386-or-on-a-number` | `requested \|\| 3`, turning a real `0` into `3` | nothing |
| G388 | `gap-g388-sort-as-text` | `.sort()` on numbers, giving `100,25,9` | lint `require-array-sort-compare` |
| G390 | `gap-g390-index-update-across-await` | a lost update on `c[0]` across `await` | lint `require-atomic-updates` |
| G392 | `gap-g392-find-method-option` | `users.find(...).name`, refused only by the `tsc` pass (`TS2532`) | lint `no-non-null-assertion` |

`null-in-optional-field` was the G391 gap case until 0.1.152 closed it. It is
a `boundary` case now and off the page: `json.parse<Opts>` of
`{"timeout_ms": null}` is an `Err` naming `timeout_ms?: Nullable<number>`, where
the TypeScript spreads the `null` over its default and prints
`timeout x2 = 0`.

`sql-by-interpolation` was the G387 gap case until 0.1.155 closed it. It is a
`refused` case now, E0239, and off the page: `db.query` with
`"... name = '${name}'"` is refused with a help that writes the `?` placeholder
and `[name]`, where the TypeScript returns both rows for `x' OR '1'='1`.

`record-in-template` was the G385 gap case until 0.1.157 closed it. It is a
`refused` case now, with `E0237`, and off the page: `"Welcome back, ${user}!"`
with a record is refused with the help to interpolate a field, `${user.id}`,
where the TypeScript prints `Welcome back, [object Object]!`.

`async-callback-nothing-awaits` was the G384 gap case until 0.1.159 closed it.
It is a `refused` case now, with `E0211`, and off the page:
`ids.forEach(async fn(id: string) -> void { await save(id) })` in a sync
function is refused because `forEach` calls its callback without waiting, with
the help to start each call through `task.detach` or make the function async
and await each call in a loop, where the TypeScript prints `done` before
anything is saved.

## Adding one

A directory with `case.toml`, `ts.ts` and `glyph.glyph`, and `ts_ok.ts` when
the `fullconfig` claim needs a control. Then run the check: it tells you if any
half does not behave the way the case says.

The bar is that both programs do the same thing. If the TypeScript has to be
bent into a different shape to compile, the case is comparing two programs
rather than one language against another, and it belongs in the bin.
