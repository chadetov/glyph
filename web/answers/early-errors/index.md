<!-- Markdown copy of https://glyphlang.io/answers/early-errors/, generated from the same source as the page. Last changed 2026-11-21. -->

26 · Caught earlier

# Why does Glyph catch what tsc catches, but earlier?

Because an error against generated TypeScript is an error about code you didn't write. Glyph reports these against your source, with the rule you broke and the edit that fixes it.

status.glyph: an arm that yields nothing, in a position that needs a value

```
fn label(s: Status) -> string {
  let text = match s {
    Loading => {},          // an empty block: the arm produces no value
    Ready => "done",
    Failed(msg) => msg,
  }
  return text
}
```

glyph build

```
[E0223] this `match` arm produces no value, but the `match` is used as a value
   ╰─ End the arm with an expression, or `return` from it. `X => {}` is a no-op
      only where the `match` is a statement.
   note: A value-position arm lowers to `case X: { break; }` when it yields
         nothing, so the value would be `undefined` at run time.
```

The arm case is the one that used to escape entirely. `{}` in arm position is an empty *block*, not an empty record, so the arm ran and produced nothing.

## Why it matters

Glyph compiles to TypeScript and then type-checks the output with `tsc --strict`, so a mistake Glyph doesn't model isn't lost. It comes back one stage later, phrased in terms of the emitted file: a `TS1308` pointing at an `await` in a function you never spelled, or a `TS2366` about a `switch` you didn't write.

The line is mapped back to your `.glyph`, but the explanation isn't. That gap is where the compiler stops teaching.

So each rule Glyph can decide for itself moves up:

- **E0222** `await` outside an `async fn`
- **E0223** a `match` arm that produces no value while the `match` is used as a value
- **E0008** An assignment written without `mut`

Each names the rule, cites the decision behind it, and shows the before and after.

## An arm that produces no value

Before this, the emitted `switch` fell out of its own cases and `text` held `undefined` while its type said `string`. `tsc` catches some shapes of that and misses others.

Glyph now decides it wherever the position is decidable: a `let`, a `mut`, a `return`, or the tail of a function that declares a return type. A statement `match` keeps `X => {}` as the deliberate no-op it is.

## await outside an async fn

`await` is the same move. Glyph has no user-visible `Promise`: an `async fn -> T` is awaited to a `T`, so `await` anywhere else has nothing to suspend.

glyph build, on `fn total() -> number { return await fetch_count() }`

```
[E0222] `await` is only valid inside an `async fn`
   ╰─ Mark the enclosing function `async fn`, or call a non-async function here.

// before: no Glyph diagnostic, then
dist/main.ts:14:18 - error TS1308: 'await' expressions are only allowed
within async functions and at the top levels of modules.
```

**Details which callable decides **

That used to be `tsc`'s `TS1308` on the generated file. It is `E0222` now, and the innermost enclosing callable decides, so a synchronous lambda inside an `async fn` is its own context, the same rule TypeScript uses. The opposite mistake, a call nobody awaits, is `E0238`: `save(1)` on its own line, `let _ = save(1)` and `let n: number = load()` are refused, because the call starts and the program moves on without it. A sync callback that means to fire and forget writes `task.detach(async fn() -> void { await save(1) })`, which reports a failure on stderr.

## An assignment without mut

The third one is about a rule people break on day one. Glyph marks every mutation, so reassigning a binding is `mut x = e` and introducing one is `let x = e`.

E0008 E0302 E0303

A bare `x = e` used to report "unexpected token: Equals", which names a character rather than the rule. It is `E0008` now, and `glyph --explain E0008` spells out why the mark exists: every place a value changes starts with `mut`, so `grep "mut total"` finds all of them.

**Details the `?` operator, E0302 and E0303 **

The `?` operator gets the same treatment from the other direction. `?` expands to a binding plus an early `return` placed before the statement it sits in, so it needs a statement to expand into. It works in a `let`, a `return`, an argument, and now in a `match` arm body. Where it can't work, the message says which position it is and how to bind out of it (`E0302` and `E0303`) instead of claiming the operator isn't implemented.

## The worse case: when tsc is happy too

A `tsc` error in the wrong place is annoying. A clean build on a program that means something else is the real problem, and Glyph had one.

Every top-level name you declare goes into the emitted module verbatim, including a tagged union's variant constructors, so a variant named `Error` emits `export function Error(...)` at module top level. The `new Error(...)` the compiler writes below it then calls your variant.

glyph build, on a union with an `Error` variant

```
[E0110] `Error` cannot name a type, variant, or function: the emitted module
        references the JavaScript global `Error`, and this declaration would
        shadow it
 5 │   | Error(string)
   │     ──────┬─────
   ╰─ Rename it (e.g. `Error` -> `Failure`). The name is legal TypeScript, so
      nothing downstream catches the rebinding.

// before: no Glyph diagnostic, then, twenty lines later
dist/main.ts:31:12 - error TS2345: Argument of type 'string' is not
assignable to parameter of type ...
```

E0110 E0111 E0109

**Details `type Key = string | number`, the guarded list, E0111 **

`type Key = string | number` is the same defect with a friendlier face. In Glyph `A | B` declares a tagged union whose members are variant *constructors*, so that line declares variants called `string` and `number`. It compiled, passed `tsc --strict`, and emitted an `export const number` that shadowed the prelude `number` namespace. Nothing failed until a later `number.to_string` call did.

The span is the declaration, which is where the rename goes. The guarded list is derived from the emitter rather than from a general list of JavaScript globals, and a test greps the emitter and fails when a new global reference appears without a matching entry. That test is the point: `Number` was harmless until `int` shipped and started emitting `Number.isInteger`, which is how this reached a release in the first place. `Date`, `Math`, and `JSON` stay free, because nothing Glyph emits mentions them.

The primitive-union case gets its own code, `E0111`, because rejecting it without explaining it teaches nothing. It says what the line actually parsed as, points at named variants, and names `extern_ts("string | number")` for the boundary where you want the raw TypeScript union. Every name Glyph will not let you use is tabulated in `docs/reference/reserved-words.md`: the 32 keywords, the 33 TypeScript reserved words behind `E0109`, and the shadow list, each with the reason the name is taken.

Worth being clear about what this does not do. You still cannot name a type or a variant `Error`, and the spreadsheet that turned this up ships a variant called `Cellerr` because of it. The program it wanted to be is still not writable; what changed is that the compiler now says so at the declaration instead of letting the build go green and mean something else.

**Details why the name is not simply made usable **

Making the name usable means mangling Glyph names on the way into TypeScript, which changes what a stack trace, a `grep` over `dist/`, and a hand-written `extern/` shim see, so it is not a patch we can make quietly.

## A wrong message is worse than no message

The shadow list started with names the emitter writes as *values*. It was missing the ones it writes as *types*, and that produced the sharpest version of this whole problem.

glyph build, before: fifteen of these, none of them near the cause

```
dist/corpus/infer_output.ts:52:9 - error TS2353: Object literal may only
specify known properties, and 'code' does not exist in type 'Issue'.
```

glyph build, now

```
[E0110] `Issue` cannot name a type, variant, or function: the emitted module
        references the Glyph prelude global `Issue`, and this declaration
        would shadow it
   ╰─ Rename it. The name is legal TypeScript, so nothing downstream catches
      the rebinding.
```

**Details why `Issue` collides, and `Record` **

Every descriptor's `parse` returns `Result<T, Issue[]>`, so the emitted module writes `Issue` whether or not you did. Declare `type Issue = { path: Array<string>, message: string }` in your own module and yours wins, the emitted `Issue` objects carry a `code` field yours has never heard of, and the build fails with a `tsc` error about generated code, at a span pointing somewhere else.

`Record` joined the list at the same time, filed with the JavaScript globals rather than the prelude names because it is a TypeScript built-in and claiming otherwise would make the message wrong about where the name comes from. The drift test that greps the emitter now covers ambient types too, and it had to learn to see `Issue[]` in a type position, which is the reason this reached a release. `Schema`, `Component` and `Option` stay legal on the same rule that keeps `Date` free: the emitter writes them only because you wrote them in an annotation, so declaring one shadows nothing.

### An import that blamed a match

The other one in this class was an import. A local import path resolves from the build root, the directory you hand to `glyph build`, not from the directory of the file doing the importing.

glyph build examples/apps, on `import catalog` inside csvql/

```
[E0104] unresolved import `catalog`: no module `catalog` under the build
        root `examples/apps`. There is a `csvql/catalog.glyph` under the
        root; a local import path is resolved from the build root, not from
        the importing file's directory (D15)
   ╰─ Build that module's own directory as the root, or spell the import path
      as it reads from the root.

// before: no import diagnostic, then
[E0218] non-exhaustive match on string   // on a match over all four values
```

**Details what it used to report, and how it avoids false positives **

Build an enclosing tree and `import catalog` finds nothing, and until now nothing said so. The type degraded to unknown, a `match` over it lost the variants it was matching, and what you got was `E0218`, non-exhaustive match, on a `match` that covered every case. Doing what that message asked would have added a dead `else` arm to correct code.

The risk in a check like this is that it fires on things that are fine, and an import checker that cries wolf on every npm package is worse than the silence it replaced. So the build first collects the module names it can resolve without a `.glyph` file: every `declare module "X"` under `<root>/.types/` and in the bundled Node shim, plus every package in the project's `node_modules`. A name on that list is never reported, even when a local file happens to share its basename. When a project has no `node_modules` at all, the compiler cannot tell an uninstalled dependency from a typo, so it reports only what it can prove: an import that some `.glyph` file under the root answers to.

Resolution itself did not change. The same programs build and the same programs fail. What changed is that the failure now names the import instead of accusing a `match` two files away, which is the whole of it: a compiler that reports the wrong cause costs you the hour spent chasing it, and then the benefit of the doubt on every message after.

## Naming the rule, not the token

The quieter version of the same problem is an error that is correct and still tells you nothing.

glyph check, before and now

```
[E0002] parse: expected `)` after variant payload, found Comma
   ╰─ Add the expected token.

[E0010] a union variant carries one payload, but `Node` lists 6 positional
        fields
   ╰─ Glyph has no tuple payload. Put the fields in one record and name them:
      `Node({ /* name */: Color, /* name */: Tree<K, V>, /* name */: K,
              /* name */: V, /* name */: int, /* name */: Tree<K, V> })`,
      and destructure it by those names in a match arm.
```

**Details the red-black tree, and how the fix names your program **

An agent writing a red-black tree reached for the positional constructor every ML-family language has, and the parser stopped on its first comma with “expected `)`” and the advice “add the expected token”. Adding a `)` there produces a different program, not a working one. A Glyph variant carries one payload, and a payload with several fields is a record. Neither of those sentences was anywhere in the output.

To say that, the parser had to read the whole list before refusing it, which is also what lets it count. The span underlines all six fields rather than the separator it happened to die on, and the suggested record is built from the variant name and the six types in the file, so the fix names your program rather than an example of someone else’s. Field names are the one thing the compiler cannot supply, so it leaves them as holes.

The rule behind the message is a choice, not a gap. `Node(Black, l, x, b, h, r)` is the argument swap that type-checks, and position four tells a reader nothing. The record spelling names every field, a match arm names only the fields it uses, and both of those are things `grep` can find.

## Two more the checker took over in 0.1.121

`Order { id: "a", total: 1 }` is what someone coming from TypeScript writes to build a record, and Glyph has no such form: a value is written on its own and the type goes on the annotation.

glyph check, on a record built the TypeScript way

```
[E0013] a statement cannot begin on the line where
        `return Order` ends
   ╰─ Glyph has no `TypeName { ... }` construction form. Write the literal
      alone and put the type on the annotation: `let o: Order = { id: "a" }`.

// 0.1.121 to 0.1.145: [E0228] on `Order`, then [E0108] on the braces
// before that: [E0108] Warning: lint: unreachable code, and exit 0
```

E0228 E0013 since 0.1.146

**History from E0108 to E0228 to E0013 **

What the compiler used to say about that line was `E0108`, unreachable code, on a build that exited 0, because the name resolved to the type and the braces parsed as a block after the `return`. A type's name written as a value is `E0228`, and the message is read off the declaration, so a record lists its fields, a tagged union lists its variants with their payload shapes, and a string-literal union lists its literals. `Order.parse(raw)` and `Order.is(v)` stay legal, since the receiver of a type's own descriptor is the one expression position a type name belongs in.

That fixed the case where the name was a type and left the parse alone, so `return h() { k: 2 }` still returned `h()` and dropped the braces with a warning. Since 0.1.146 a `{` on the line where a statement ended is `E0013` at parse, whatever the name before it is, and its help gives the same rewrite. Since 0.1.149 the rule covers every statement, not only one opening with a brace: `let s = "a" "b"` used to bind `"a"` and drop `"b"`, and `return x as User` was three statements, since Glyph has no cast.

Each statement in a block starts its own line.

### A container where a primitive is declared

The other one is a prelude container assigned to a primitive. `let x: string = o` where `o: Option<int>` drew nothing from the Glyph checker, and neither did passing a `Nullable<int>` to a parameter declared `int`.

Five containers are covered in both directions now: `Option`, `Result`, `Array`, `Record` and `Nullable` are never a `string`, a `number` or a `bool`, whatever their arguments, and `Nullable<T>` against a scalar is decided one level in against `T`, so `let v: Nullable<int> = 3` stays correct and `let v: Nullable<int> = "three"` is `E0204`.

**Details the case only Glyph can catch, and records since 0.1.130 **

The `Nullable` case belongs on this page more than the others, because it is one `tsc` cannot make: the emitted `const v: number | null = 3` narrows back to `number` before the call is checked, so a boundary type that exists to make `null` visible was passing silently where an `int` was declared, and the Glyph checker was the only place it could have been caught.

The same containers against a declared record or union came next. Since 0.1.130 `let user: User = json.parse(body)` is `E0204`, expected `User`, found `Result<unknown, Array<Issue>>`. Before, only `tsc` said so, which meant `glyph check --no-tsc`, the editor and the playground showed a clean file on the one line a boundary exists to get right.

### A misspelled member on an array or a string

A misspelled member on an array or a string is the other case that used to wait for `tsc`. `xs.lenght` is `E0210` with "did you mean `length`", checked against the members TypeScript's library declares for arrays and strings, the way a record's field read is checked against its fields.

`tsc` would have refused both, so a build that ran it was already red; the change is that the checker, and everything built on it, sees them first.

## And three more, since 0.1.133

### A method read off its string

`let up = s.toUpperCase` type-checks in TypeScript and throws when `up()` runs, because the read drops the string it belongs to. Glyph refuses it as `E0234`, and refuses `.bind(s)` on it too: wrap the call in a lambda instead.

E0234

**Details where the method types come from **

The checker now knows what each array and string method returns, from a table generated out of TypeScript's own declarations.

### Optional chaining on a Result

`r?.label` on a `Result` reads like "propagate the error, then read the field". It is optional chaining, and a `Result` is never null, so it reads the field off the `Result` object and propagates nothing.

E0236

An agent wrote exactly that while building a CSV query tool. It is `E0236` now, on an `Option` as well, and the help says to bind with `?` first.

### An unchecked json.parse

`json.parse(body)` with no type argument is the unchecked read, a `Result<unknown, ...>`. Written where a `Result<User, ...>` was declared, `tsc` used to infer `User` from the declaration and the program crashed on its first field read.

The emitter now writes the `unknown` out, so `glyph check` refuses it through `tsc`.

Glyph's own checker does not yet catch an `unknown` landing inside a declared type, so `--no-tsc` and the editor still miss `Result<unknown, ...>` written where `Result<User, ...>` is declared. (A bare `let user: User = json.parse(body)` is `E0204` from Glyph's own checker, because the outer type is already wrong.) Refusing `unknown` at every depth is planned as G325, on the [release roadmap](https://github.com/chadetov/glyph/blob/main/docs/roadmap/releases.md).

G325

## And two more in 0.1.122

`type Mode = "read" | "write"` declares a finite set of strings, and the Glyph checker compared a value against it by nothing at all. A number, a `bool`, a record and a string outside the set all passed `glyph check --no-tsc` with nothing but unused-variable warnings, while `tsc` refused every one of them on the emitted TypeScript.

So a full `glyph build` was never wrong about this, but the four surfaces that have no `tsc` in the loop were: `--no-tsc`, the language server, the MCP tools and the playground.

glyph check --no-tsc, on a type that lists its legal strings

```
type Mode = "read" | "write"

let a: Mode = 3
let b: Mode = "rw"
let c: Mode = takes_mode(true)
let d: Mode = { x: 1 }
// before: four unused-variable warnings, exit 0
[E0204] type mismatch: expected `Mode`, found `number`
[E0204] type mismatch: expected `Mode`, found `"rw"`
[E0211] argument type mismatch: expected `Mode`, found `bool`
[E0204] type mismatch: expected `Mode`, found `record`
```

**Details how the union is compared **

That is the four `tsc` reported, at the same four statements, without running `tsc`. The rule reads the union as what it is at run time, a `string`, and asks the checker the corresponding `string` question in each direction, so it inherits the rules that already refuse a record or a container against a `string` rather than restating them one shape at a time. Two literal sets are compared by set, so a narrower union goes where a wider one is declared; an alias chain is followed on both sides; an imported union is decided like a local one; and `Nullable<Mode>` is descended with the union intact.

A union nested in a record field or a generic argument was left to `tsc` in 0.1.122 and is decided in 0.1.123. See below.

## 0.1.123: the literal a value is, not the `string` it emits to

`type Mode = "read" | "write"` put the set on the declaration and nothing on the value, so `"read"` was a `string` and `["read", "write"]` an `Array<string>`.

**History what 0.1.122 gave up **

That made two programs indistinguishable: `return ["read", "write"]` where an `Array<Mode>` is declared, which `tsc` compiles, and `return xs` for an `xs: Array<string>`, which it refuses. 0.1.122 declined the pairing rather than get it wrong, and that bought the right program by giving up the wrong one: `{ mode: "nope" }` against a `{ mode: Mode }` drew nothing.

A written string literal now has the one-literal type it spells, an array literal's element type is the join of its elements', and a written array or object literal is checked element by element and field by field against the declared type, so the diagnostic names the literal and underlines it.

glyph check --no-tsc, on a literal one container in

```
type Mode = "read" | "write"
type Cfg = { mode: Mode }

fn ok() -> Array<Mode> { return ["read", "write"] }
fn bad() -> Array<Mode> { return ["read", "nope"] }
fn wrong() -> Cfg { return { mode: "nope" } }
// 0.1.122: nothing, on all three
[E0204] type mismatch: expected `Mode`, found `"nope"`   (twice)
```

**Details where the literal widens, and `Record` keys since 0.1.153 **

The literal widens back to `string` where TypeScript widens it: at a `let` with no annotation, whose emitted TypeScript `let` widens the same way, and at the element binding of a `for` over a written array. So `let l = "read"` followed by `takes_mode(l)` is refused here and `TS2345` there. A `const` emits a TypeScript `const` and keeps its literal, so `takes_mode(CM)` compiles under both. A literal type that arrived through a name is not widened, which is TypeScript's own freshness rule. Every row of that was settled by running `tsc --strict` on the equivalent TypeScript rather than reasoned about.

The key argument of a `Record` is held more strictly than `tsc` holds it, since 0.1.153. TypeScript writes `Record<string, V>` as an index signature and accepts it against a `Record<Mode, V>` in both directions. Glyph refuses both: the open map may hold keys outside `Mode` or miss one, and a `Record<Mode, V>` passed on as the open one can be written a stray key. Going from one to the other is an `is` arm or a `json.parse`.

Two shapes the checker still says nothing about: a literal inside `Some(...)`, because a prelude constructor has no type here yet, and a record literal bound to an unannotated `let`, because no record type is synthesized for an object literal.

## A module under `std/` was dead code with no diagnostic

`std/` and `extern/` name modules the compiler carries, and the resolver reserved them for imports. Nothing stopped a project file from *declaring* one.

glyph check --no-tsc, on src/io.glyph declaring `module std/io`

```
// before: glyph check: 2 module(s) checked, no diagnostics.  (exit 0)

[E0113] `std/io` declares a module under `std/`, a prefix reserved for the
        modules the compiler carries (D15)
 1 │ module std/io
   │ ──────┬──────
   ╰─ Rename the module to a path of your own (`io`, `app/io`).
```

**Details what used to happen, and which paths are reserved **

`module std/io` in your own `src/io.glyph` compiled clean, and then `import std/io { shout }` resolved to the stub and reported that `shout` is not exported: your file was never read.

The prefix is matched as a whole first segment, so `standard`, `externals/io`, `app/std` and `app/extern/helper` are untouched, and importing under either prefix still works the way it always did.

A file that draws no diagnostic and runs no code is the worst shape a compiler can leave you in, because nothing about it looks wrong.

The second one in 0.1.122

## A parser depth limit

The second one is not a `tsc` story at all. The parser is recursive descent with no depth guard, so an expression nested two thousand levels deep ran the process out of stack and aborted with exit 134.

glyph check, on `const x = ` followed by 2,000 nested brackets

```
// before: thread 'main' has overflowed its stack
//         fatal runtime error: stack overflow, aborting   (exit 134)

[E0011] this array literal nests deeper than the parser's limit of 64 levels
   ╰─ Name the inner levels: pull them out into `let` bindings, or into a
      `fn` that returns one of them, so no single expression, type or
      pattern is more than 64 levels deep.
```

**Details why it mattered for the language server **

That is bad on `glyph check` and worse on `glyph lsp` and `glyph mcp`, which read every file under the root: one pathological file took the server down for the whole workspace. It is `E0011` now, raised at the token that would have opened level 65.

The limit is 64 and it was measured rather than picked.

**Details how 64 was measured **

One level of the costliest construct, a nested array literal, costs 7,792 bytes of stack in the release build and 8,810 in the debug one, found by binary-searching the smallest thread stack a given depth will parse in. The thinnest stack the parser runs on is a spawned thread's 2 MiB, which is what the language server's workers get, and 64 levels spend under a third of it in the costlier build. The deepest nesting across the 343 `.glyph` files in this repository is 16. The limit covers expressions, types, patterns, blocks and JSX children alike, and a thousand sibling arrays still parse, because depth is per-construct nesting and not a budget spent over the file.

## When neither compiler is wrong

There is a third shape, and it is the one that reads worst. Glyph reports nothing, `tsc` reports something true, and the program you wrote was fine.

What failed is the translation between them.

poll.glyph, and what tsc said about it before 0.1.89

```
fn classify() -> int {
  let done = false
  match done {
    true => { return 1 },
    false => { return 0 },
  }
}

// no Glyph diagnostic, then
[TS2678] Type 'true' is not comparable to type 'false'.
```

**Details why `tsc` saw `false`, and the app that hit it **

A binding in Glyph has one type for the whole of its life. TypeScript narrows a binding to whatever it last saw assigned, and its `boolean` is the union `true | false`, so `let done = false` reached `tsc` typed `false` and the `true` arm of the emitted `switch` had nothing to match.

A type declared as a set of string literals failed the same way, and so did `==`, which came back as `TS2367` instead: two messages, two codes, one cause. The app that hit it was polling a flag a timer callback sets, which is how you turn an event-based API into a value while Glyph has no `Promise` you can construct by hand. The callback turned out to be beside the point: four lines with no callback do it.

The fix is not a new diagnostic, because there is nothing to diagnose. The emitter now restates the type the checker already decided, at the two places that discriminate over a value: the `match` scrutinee and either side of a `==` or `!=`.

**Details the checks that still fire, and what it costs **

Each side is decided by its own type and ignores what sits next to it, so a comparison between two bindings is covered the same way a comparison against a literal is.

The checks you want still fire. `m == "nope"` where `m` is a `Mode` is still rejected, and now the message names `Mode` instead of the single member `m` happened to be holding. Doing this at the assignment instead would have been less code and would have thrown that away: a cast to `Mode` is accepted where the annotated assignment is not, and the membership check is the part Glyph leaves to `tsc` on purpose.

What it costs is small and worth naming. A cast can narrow as well as confirm, so a value Glyph types as `bool` whose TypeScript type came out `unknown` now passes where the `switch` used to complain. The two types still have to be comparable, so a model that has drifted far enough to matter, `string` against `bool`, reports `TS2352` rather than nothing.

## The same typo, two answers

A misspelled standard-library function used to get a different answer depending on how you had written the import, which is the kind of inconsistency that teaches you not to trust either answer.

the named import, always an E0105

```
import std/string { repeeat }
[E0105] import: `repeeat` is not exported by `std/string`
```

the namespace read, before 0.1.72

```
import std/string
string.repeeat(s, 2)
[TS2551] Property 'repeeat' does not exist on type ... Did you mean 'repeat'?
```

Both spellings go through the export list now, and both say `E0105`, naming the member and the module it is missing from.

**Details why the two spellings differed, and what switching it on found **

The first spelling was checked against the module's export list during resolution. The second was not checked at all, so it fell through to `tsc`, which saw a property access on an object and answered in those terms.

Switching it on immediately found something nobody was looking for: a test fixture had been calling `fs.write`, where the function is `write_text`. It had passed for months because that fixture runs with type-checking off, and nothing else in the compiler looked at namespace members. The export list is now the authority for both spellings, so a separate check keeps it in step with what the runtime actually exports. That check was written, then deliberately broken to confirm it failed, which is how we found its first version had been silently checking nothing.

## Where it stands

### Shipping today

Statements and syntax

- `await` outside an `async fn` E0222
- A call to an async function that nothing awaits, `let _ =` and a typed slot included E0238
- A value-position `match` arm that produces no value E0223
- An assignment missing `mut` E0008
- A variant given several positional payload fields where the record form is the one that exists E0010
- A `?` in a position it cannot propagate from E0302 E0303
- A type's name written where a value is expected E0228
- A statement that begins on the line where another ended, which used to drop the value after it E0013
- A construct nested past 64 levels E0011

Names and modules

- A declaration that would shadow a name the emitted module already depends on E0110 E0111
- The shadow list covers types the emitter writes on its own, so `Issue` and `Record` are caught at the declaration.
- A local import that names no module under the build root E0104
- A name a module does not export, whether you reached it through a named import or off a namespace E0105
- A relative import path, which used to stop in the parser as a generic "expected module path segment" E0101
- A project module declared under `std/` or `extern/`, which used to compile clean and be unreachable E0113

Types

- A prelude container assigned to or passed as a primitive or a declared record or union, including the `Nullable<T>` case `tsc` narrows away E0204 E0211
- A member an array or a string does not have E0210
- The same two codes for a value that is not in a string-literal union's declared set E0204 E0211

Every code

- Every code has a one-line fix, a note explaining the rule, and a long-form `glyph --explain` body; `docs/error-codes.md` lists the catalogue and a test keeps the two in step.

### Getting sharper

- `E0238` sees a call as async only when Glyph's checker has its signature

  **Details**

  `E0238` only knows a call is async when Glyph's checker has its signature, so a promise from an npm package or a node builtin can still be dropped silently. An `async` callback passed to `forEach` or a timer is not refused yet either: that rule needs the callback slots modeled, and it is planned.

- `array.map`, `flat_map` and `zip` still take an `async` callback without complaint

  **Details**

  `array.map`, `flat_map` and `zip` still take an `async` callback without complaint and hand back an array of pending values, which prints as `[object Promise]`; the five predicate-taking array functions do stop it, with `E0211` at the callback. Modeling the other three the same way also rejects `par.all(array.map(items, async fn ...))`, which is how Glyph runs work concurrently, because an array of pending values has no spelling in the language. That is a type-system decision rather than a missing entry, so it is open.

- A top-level `await` is allowed

  **Details**

  A module-level `const` initializer has no enclosing callable, so a top-level `await` is allowed and the emitted ESM runs it; whether Glyph wants implicit async module init is still an open spec question.

- `E0223` can only judge a tail `match` when the function declares a return type, so an arm inside an unannotated lambda still reaches `tsc`.
- `E0110` tells you to rename rather than letting you keep the name

  **Details**

  On the shadow side, two things are only half done. `E0110` tells you to rename rather than letting you keep the name, so `Error`, `Number`, `Object`, `Array` and `Promise` are still off limits for a type or a variant.

- `E0111` stops `type Key = string | number` from meaning the wrong thing without giving you a way to say it

  **Details**

  And `E0111` stops `type Key = string | number` from meaning the wrong thing without giving you a way to say it: a union of two primitive types has no spelling in Glyph except `extern_ts`, whose contents its own checker treats as opaque. Untagged unions touch exhaustiveness, runtime descriptors and `is`, so that one is a type-system decision and it is not scheduled.

- `E0104` reaches `glyph build` and `glyph check` but not the editor

  **Details**

  `E0104` reaches `glyph build` and `glyph check` but not the editor: the LSP analyzes a document's text with no build root, and the workspace folder is not one either, so guessing would produce the false positives the check was designed to avoid.

- A reserved-word parameter still hides behind an unrelated error in the same file, because a failure in the collect pass skips the resolve pass that would have found it.
- The read-site pin follows a type alias only inside the module being emitted

  **Details**

  Separately, the read-site pin that keeps a `bool` or string-literal-union binding matchable follows a type alias only inside the module being emitted, so `pub type Ready = bool` in one module and `let r: catalog.Ready = false` in another still reaches `tsc` as `TS2678`. String-literal unions are covered across modules in all three import spellings, and since 0.1.123 the diagnostic names the declaration (expected `Mode`) under every spelling instead of printing its literal set under one of them.

Code examples on this page are checked against Glyph 0.1.156 on every build. Last changed 21 Nov 2026.
