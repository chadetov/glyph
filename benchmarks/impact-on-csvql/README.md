# What breaks if I add this variant, on a real application

`examples/apps/csvql` is a CSV query engine in this repository: eleven Glyph
files, a SQL parser, a binder, a plan tree, a hash join, an executor and a
renderer. At its centre is one tagged union, `Value`, the typed cell every
stage past the coercer agrees on. Adding a cell kind to it is a change someone
would plausibly make.

This benchmark asks what that change would break, before making it, and then
makes it and checks the answer.

```
glyph 0.1.110 on examples/apps/csvql, 11 files
adding one variant to Value, which has 4 today: Text, Num, Bool, Null

before the edit, glyph_variants predicts (0.034s), in its own words:
  10 match sites across 4 files
   8 will fail compilation
   2 contain a catch-all and will silently absorb it
  and it states what it left out of those figures: nothing

     FAILS    bind.glyph:148     bind::literal_kind
     ABSORBS  exec.glyph:130     exec::total
     ABSORBS  render.glyph:128   render::literal_text
     FAILS    value.glyph:18     value::render
     FAILS    value.glyph:32     value::key
     FAILS    value.glyph:44     value::is_null
     FAILS    value.glyph:53     value::is_num
     FAILS    value.glyph:62     value::rank
     FAILS    value.glyph:71     value::num_of
     FAILS    value.glyph:83     value::text_of

after adding `Blob({ bytes: number })` for real (0.071s):
  the compiler reports 8 E0200 failures, and they are the ones predicted.
  the 2 catch-all sites report nothing.

a text search asked the same question: 20 catch-all arms across the app, 2 of
them in a Value match site (precision 0.10), and nothing at all about the 8
that will fail.

PASS: the prediction and the compiler agree, site for site.
```

## Running it

```bash
./measure.py                 # needs `glyph` on PATH, or GLYPH=<path to binary>
```

The binary needs a `glyph_variants` that answers with a `summary` and a
`consequence` on every site. Both arrived on the 0.1.110 line. 0.1.106 has
neither: it accepts `proposed_variant`, ignores it and answers the lookup
form, and the run stops on that rather than reading a reply with no
consequences as a prediction that nothing breaks.

No node toolchain is needed. `check` runs with `--no-tsc --no-test`, and the
diagnostic the edit produces (E0200, exhaustiveness) comes out of the Glyph
stages before either of those.

The app is copied to a temporary directory and edited there. Nothing under
`examples/apps/` is touched, and the copy is deleted whether the run passes or
fails. Each run writes `results/<timestamp>.json` with the full listing, the
raw diagnostics and the timings.

## The two sites the compiler will never mention

`exec::total` sums the numeric cells in a group:

```glyph
return acc + match v {
  Num({ n }) => n,
  else => 0,
}
```

Add `Blob` and this keeps compiling. A blob cell contributes 0 to a `SUM`, so
the query returns a number that is wrong rather than an error that is visible.
`render::literal_text` has the same shape and routes a blob to
`value.render(v)`. No build, no type error and no test says so.

That is the half worth the benchmark. The eight sites that fail are ones the
compiler already finds, once the edit is made. The two that absorb it are
found only by asking before the edit, and an agent that fixes the eight and
stops has a green tree and two live bugs.

## What holds the run honest

**The instrument is a comparison, not a golden file.** The set of declarations
the tool marks `WILL_FAIL` before the edit has to equal the set of entities the
compiler reports E0200 for after it, and every `ABSORBS` declaration has to
appear nowhere in the compiler's output. Neither side of that equality is a
recorded number, so a regression in the predictor or in the checker breaks the
run rather than being absorbed by it.

**The fixture pins the counts separately.** `fixture/edit.json` holds the ten
entities, split into eight and two. Without it the comparison would still pass
if csvql lost all its match sites and both halves agreed on nothing. The
fixture pins entities rather than line numbers, because a line number moves
under any edit above the site and `module::name` is the identity the tool and
the diagnostics both carry.

**The app has to compile before the edit.** Checked first, so the eight
failures are attributable to the variant rather than to something already
broken.

**Every total is Glyph's, not the script's.** The three summary lines are
printed out of the answer's own `summary` block. A benchmark that re-counted
the list would be a second caller reaching its own figures from one reply,
which is how a count and a list start disagreeing with nobody wrong. What the
script checks is that the answer is consistent with the list it shipped:
`not_counted` empty and `unindexed` empty are the answer stating that its
figures cover the project and that every file was read, and if either says
otherwise the run stops instead of reporting a subset as a whole.

To see that the guards are load-bearing rather than decorative, break them in
`fixture/edit.json` and re-run. Four mutations, four failures:

| mutation | what fails |
|---|---|
| `edit.insert` to a comment, so the edit does nothing | the prediction and the compiler disagree, all 8 predicted and silent |
| drop one entity from `expected.will_fail` | expected 9 sites, got 10 |
| `proposed_variant` to `Null`, which already exists | `glyph_variants` refuses the call |
| `edit.after_line` to a line that is not there | the fixture anchors on a line that occurs 0 times |

A fifth needs no mutation: point `GLYPH` at a pre-0.1.110 binary and the run
stops on the first answer, because a site with no consequence is not a site
with no consequence. Checked against 0.1.106.

## The search baseline

Someone without the tool looks for the silent sites by grepping for catch-all
arms. The baseline in `measure.py` does exactly that, attributing each hit to
the named function it sits in. It finds both real sites, and 18 others: 20 hits,
precision 0.10.

The instructive false positive is `value::cmp` at `value.glyph:112`. It is in
`value.glyph`, it is about comparing `Value`s, its arm is `else =>`, and it is
not a `Value` match site. It matches on a rank, which is a number. A text
search has no notion of which type a match scrutinizes, so its false-positive
rate rises with every catch-all anywhere in the project, while an answer scoped
to the union asked about does not.

Grep also says nothing about the other eight. It cannot resolve a scrutinee's
type, so it cannot separate the ten matches over `Value` from every other match
in the app. The eight it could only get from the compiler, and only after the
edit has been made. The two it can reach, buried in eighteen it cannot rule
out, and the compiler will never mention them at all.

## What this does not show

The run proves the compiler is silent at `exec::total` and
`render::literal_text`. It does not run the app with a blob cell in it and show
a wrong `SUM`, because reaching that needs a blob column in the catalog and the
coercer, which is a much larger edit than the one variant this measures. The
`else => 0` is readable at `exec.glyph:133`.

The eight and two were first observed by hand against 0.1.108 and re-derived
by this script against 0.1.110, unchanged. `fixture/edit.json` records both.

The run reads what the compiler reports, not what the program then does. It
uses `--no-tsc --no-test`, so the app's own `@example` tests do not execute
here.

## Related

`../impact-before-edit/` is the same capability on a purpose-built fixture,
scoped to the `has_catch_all` state and comparing against search alone. This
one is the same question on an application that was not written to flatter the
answer.
