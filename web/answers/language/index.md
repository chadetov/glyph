<!-- Markdown copy of https://glyphlang.io/answers/language/, generated from the same source as the page. Last changed 2026-11-21. -->

15 · Language completeness

# Is this a real language, or a toy with gaps?

It has the things you reach for in a real codebase: **module visibility**, **interfaces** to constrain generics, **defer** for cleanup, and **concurrency** helpers. Where it leans on JavaScript, it says so instead of pretending otherwise.

visibility: private by default

```
pub fn charge(cents: int) -> Result<Receipt, string> { validate(cents)? ... }

fn validate(cents: int) -> Result<int, string> { ... }  # module-private; importing it elsewhere is an error
```

Declarations are private to their module until you mark them `pub`, so the public surface is one search (`grep '^pub'`) and a helper never leaks by accident.

## Why it matters

A language you can write one file in but not structure a project in isn't one you'll bet a codebase on. The gaps that matter aren't exotic:

- **Visibility** Can I hide a helper?
- **Generic bounds** Can a generic require a capability?
- **Cleanup** Can I release a file handle on every exit path?
- **Concurrency** Can I run two things at once and wait for both?

Glyph answers those four, and it does it without reopening the annotation-heavy direction it deliberately dropped.

## Interfaces constrain generics

An `interface` is structural, like a TypeScript one: it's a set of member signatures a generic can require. No `implements`, no nominal ceremony. Any value with the members qualifies, and `tsc` enforces it.

interfaces constrain generics

```
interface Named {
  fn name() -> string
}

pub fn label<T: Named>(x: T) -> string {
  x.name()
}
```

## `defer` and structured concurrency

`defer` runs cleanup when the block exits, on every path, including an early return or a thrown error. And `std/task` joins concurrent work with one lifetime instead of leaving detached promises.

defer + structured concurrency

```
defer file.close()          # runs on the way out, however you leave

let both = await task.all([fn() { fetch_a() }, fn() { fetch_b() }])
```

**Details ordering, and owned handles **

Multiple defers run last-in-first-out, and it composes with `owned` handles.

## What it inherits from JavaScript

Glyph compiles to TypeScript and runs on a JavaScript engine, so it inherits evaluation order, reference semantics for mutation, and the single-threaded garbage-collected model. The spec states these as guarantees rather than leaving you to find out.

Equality is where it differs: `==` compares values, records field by field, where JavaScript's `===` compares references.

Manual memory and a non-GC model aren't on a roadmap; they're outside what a transpile-to-TypeScript language is.

## There's no `if`, so `match` has to carry everything

The first thing a TypeScript developer asks after the exhaustiveness pitch is whether `match` is really a general-purpose conditional or just a nice way to destructure a union. It's the former, and the place that proves it is assignment.

A `match` that is the whole value of a `let` or a `mut` takes anything a statement takes: an arm can be a block, an arm can `await`, an arm can `break` out of the surrounding loop, and an arm can read the binding it is assigning, which is how you fold state through a loop without an `if`.

from the link checker: fence tracking, and an awaited arm

```
for raw in lines {
  let fence = is_fence(raw)
  mut in_fence = match fence {      # reads the binding it assigns
    true => !in_fence,
    false => in_fence,
  }
}

let cache = match args.offline {
  true => no_cache(),
  false => await fetch_unique(external(links), args.max_concurrency),
}
```

**Details what it compiles to, and where the snippets come from **

Both compile to a plain `switch` that assigns the binding per arm, so the `await` sits in the enclosing async function and the accumulator is a normal variable TypeScript can infer. Both snippets are from [`examples/apps/linkcheck/main.glyph`](https://github.com/chadetov/glyph/blob/main/examples/apps/linkcheck/main.glyph), a Markdown link checker written to find exactly this kind of thing.

A `match` used *inside* a larger expression, as an argument or an operand, still becomes a closure, which means its arms have to be single expressions. Hoisting it to its own `let` lifts that.

## A pattern can name a shape more than one level down

A variant with several fields carries a record, so an arm that wants to recognise a shape has to reach through the record's fields. A field takes any pattern now: a variant tag, a nested constructor, a nested destructure, a literal, an array pattern.

one rotation case of a red-black rebalance, as one arm

```
type Tree =
  | Leaf
  | Node({ color: Color, left: Tree, value: number, right: Tree })

match t {
  Node({ color: Black, left: Node({ color: Red, left: a, value: x, right: b }), value: y, right: c }) =>
    Node({ color: Red, left: Node({ color: Black, left: a, value: x, right: b }), value: y, right: c }),
  other => other,
}
```

**History the workaround before 0.1.90 **

Until 0.1.90 the field position took a name and nothing else: you could rename it, and that was all. Matching a tag there was a compile error, and a nested constructor fell off the parser. The workaround was to bind every field and write a nested `match` per level, which for anything like a red-black tree rebalance is a different program: the arms stop lining up with the cases and each level re-threads the bindings above it.

There is a consequence worth knowing before you write one. A field that tests a value can fail, so the arm no longer counts as covering its variant, and a match whose only `Node` arm is `Node({ color: Red, ... })` is reported non-exhaustive until a sibling arm or an `else` takes the rest.

**Details why it asks for the catch-all, and the array position **

The checker proves coverage over a set of tags, not over a product of fields, so it will not work out that a `Red` arm and a `Black` arm together exhaust `Node`. Asking for the catch-all is the reading that cannot let a match fall off its end at run time. The same applies where there are no variants to count at all: a match on a plain record whose every arm tests a field needs an `else` behind it.

One place this reaches that you may not expect is the array position. `[Black]` as a whole arm used to count as matching any one-element array, so a match that leaned on it certified as exhaustive and then took that arm for `[Red]` as well. A PascalCase element is a variant reference here too, so such a match is now `E0208` on arrays of length 1. Bind the element and match it on its own. The lowering underneath is still the old one, which is why the check is the half that shipped first.

E0208

## And the compiler knows what the `match` produced

The follow-up question is the one that decides whether any of this is load-bearing: once you bind a value out of a branch, does Glyph know its type, or does it shrug and let `tsc` sort it out later? A `match` now takes its arms' type when they agree, and `T.parse` is known to return `Result<T, Array<Issue>>`, which is the shape most real code branches on.

from the expense splitter: no annotation, and `i` is a number

```
let wire = match WireLedger.parse(decoded) {
  Ok(w) => w,
  Err(issues) => {
    return Err(BadInput({ message: "${path} is not a settle ledger: ${describe_issues(issues)}" }))
  },
}

for i, w in wire.expenses {        # i is 0, 1, 2 ... not "0", "1", "2"
  ...
}
```

**History before 0.1.45 **

Until 0.1.45 it shrugged. Every `match` expression typed as unknown, so a value that came out of the only branching construct in the language was untyped from that point on.

**Details what went wrong quietly, and how the join works **

Both halves of that are things that used to go wrong quietly. The loop emits `wire.expenses.entries()`; when the iterand's type is unknown it emits `Object.entries(...)` instead and `i` is the *string* `"0"`, which is legal TypeScript, so the build was clean and the program printed `01:a` where it meant `1:a`. And `wire.expensez` is now `[E0210] type WireLedger has no field expensez` from Glyph, on the `.glyph` line, rather than a `TS2339` on generated TypeScript.

The join is equality at the head: an arm that ends in `return` doesn't vote, and arms whose types disagree leave the result unknown the way it was before. It looks one level deeper only at a type argument, so `None => []` agrees with an `Array<string>` arm instead of sinking the whole `match`, and the loop after it still binds a number. The same chain over `record.get` works for the same reason: all six `std/record` functions carry the value type off the record you pass. The snippet is from [`examples/apps/settle/main.glyph`](https://github.com/chadetov/glyph/blob/main/examples/apps/settle/main.glyph), which carried the annotation this deleted.

### A lookup with an empty fallback

That is the shape this cost the most. `record.get` on a `Record<string, Array<string>>` is an `Option<Array<string>>`, and `[]` is an array of nothing in particular, so the two arms agree on `Array` and the element type comes from the arm that has one.

from the dependency resolver: a lookup, an empty fallback, an indexed walk

```
let path = match record.get(res.reached, name) {
  Some(p) => p,
  None => [],
}

for i, hop in path {               # i is a number, so the indent nests
  let indent = string.repeat(STEP, i + 1)
  mut out = array.push(out, "${indent}${hop}")
}
```

**History before 0.1.55 **

Until 0.1.55 they were compared for equality, `[]` counted as a disagreement, and the loop below printed `01:x` on a build with no diagnostics that `tsc --strict` passed. The code is [`examples/apps/depsolve/main.glyph`](https://github.com/chadetov/glyph/blob/main/examples/apps/depsolve/main.glyph), where it used to open with `let path: Array<string>` under a three-line comment explaining that the annotation was load-bearing.

## Do I have to write `pad_start` myself?

The gap you hit first in a real program isn't a type-system feature. It's reaching for `string.slice` and finding nothing there.

what landed

```
string.repeat(s, n)           string.slice(s, start, end?)       array.fold(xs, init, f)
string.pad_start(s, w, pad?)   string.index_of(s, needle, from?)  array.index_of(xs, x)
string.pad_end(s, w, pad?)     string.replace_all(s, from, to)     array.flat_map(xs, f)
string.trim_start(s) / trim_end(s)

# the 25-line block six apps each carried, and what replaced it:
let total = array.fold(expenses, 0, fn(sum, e) { sum + e.cents })
io.println("${string.pad_end(name, width)}  ${string.pad_start(amount, 9)}")
```

Those eleven names ship now, and the copies are deleted from all seven apps that had them.

**History the copies the apps carried before 0.1.47 **

Five dogfooding trips in a row filed the same list, and until 0.1.47 six apps in this repo carried their own `repeat`, five their own `pad_start`, and all of them had a loop where a fold belonged.

### Where they differ from TypeScript

Three of them part ways with their TypeScript namesakes on purpose.

- **`repeat`** `repeat` returns `""` on a negative count where TS throws, which is what makes `repeat(pad, width - len(s))` safe on a string that's already too long.
- **`index_of`** Both `index_of` functions return `Option<number>` instead of `-1`, because `-1` is a number that type-checks everywhere a real index does and fails a long way from the call.
- **`replace_all`** Replacement ships only as `replace_all`, so there's no first-only form to confuse with `String.prototype.replace`, which quietly does one occurrence.

### Why `fold` matters

`fold` is the one that buys a pillar rather than saving typing. `mut` is the greppable record of every place state changes, and with no fold, `grep mut` comes back full of counting loops that mutate nothing a reviewer cares about.

Deleting the hand-rolled versions took `examples/apps/` from 192 mutation sites to 161.

**Details how the rewrite was checked **

The rewrite was checked by output, not by review: all seven apps ran against fixed inputs before and after, 26 files of stdout, exit codes, and HTTP responses, all byte-identical.

## Can I walk a directory and tell what broke?

`std/fs` shipped six functions and none of them listed a directory.

- **`fs.read_dir`** `fs.read_dir` returns the entry names one level down.
- **`fs.is_dir`** `fs.is_dir` answers with a `bool` the way `fs.exists` does.
- **`fs.stat`** `fs.stat` returns a `FileInfo` of `is_dir`, `is_file`, `size` in bytes, and `modified` in epoch milliseconds, which `time.format_iso` renders directly.

There is no recursive `walk` and no glob; a walk is those three plus `path.join` in about ten lines.

**History the workaround these replaced **

The link checker in this repo reached past the stdlib into node's `readdirSync`, and worked out which paths were directories by trying to read them and inspecting the `errno` that came back. Both halves of that are fixed.

### Failures have names

The failure side changed more. Now the five kinds a program actually recovers from have names, and an errno with no name arrives intact on the `Other` tail.

The checker knows that taxonomy is closed, so a `match` on `err.kind` is held to the same bar as a union you declared yourself: cover the six and you need no `else`, leave one out and the build fails with `E0200` naming `fs.ErrorKind`.

E0200 E0210

walk a tree, and name what went wrong

```
let names = fs.read_dir(dir)?
for name in names {
  let child = path.join([dir, name])
  match fs.is_dir(child) {
    true  => visit(child),
    false => {
      let info = fs.stat(child)?
      report_size(child, info.size)
    },
  }
}

match err.kind {                        # no else arm: E0200 if you miss one
  fs.ErrorKind.NotFound         => io.eprintln("no such path"),
  fs.ErrorKind.NotADirectory    => io.eprintln("that is a file"),
  fs.ErrorKind.IsADirectory     => io.eprintln("that is a directory"),
  fs.ErrorKind.AlreadyExists    => io.eprintln("already there"),
  fs.ErrorKind.PermissionDenied => io.eprintln("cannot read it"),
  fs.ErrorKind.Other({ code })   => io.eprintln("errno ${code}"),
}
```

**Details the old taxonomy, checked fields, and trying the check yourself **

`FsError.kind` was `{ tag: string }` with `NotFound` as the whole taxonomy, so telling “that path is a directory” from “that path is missing” meant comparing an errno string.

`fs.FsError` and `fs.FileInfo` have checked fields too, so `err.mesage` is `E0210` against your Glyph source rather than a `tsc` error against generated TypeScript.

That check is live. Delete the `PermissionDenied` arm from the `match` above and `glyph check --no-tsc` stops with `E0200`: non-exhaustive match on `fs.ErrorKind`, missing variants `PermissionDenied`. When the stdlib model was first written it covered return types only, and a match like this one compiled clean and threw at run time; the field and variant shapes of `fs.FsError`, `fs.FileInfo` and `fs.ErrorKind` are modeled now.

### Two smaller additions

Two smaller ones landed with it, both from the same app.

- **`regex.captures_all`** `regex.captures_all` gives you the capture groups of every match instead of just the matched text, so a flat `key=value` scan stops being a hand-rolled loop.
- **`task.pool_settled`** `task.pool_settled` is `task.pool` with `all_settled`’s failure behaviour: bounded concurrency, one outcome per task, and one dead host costs you that host's result rather than every result the pool had collected.

## How do you know the functions are the right ones?

Because the app that asked for them had to stop working around them before the release counted as done.

A library function that ships and a workaround that outlives it are different claims, so the same change that added these four deleted the link checker's copies: the `readdirSync` import, both `errno` string comparisons, 97 lines of hand-rolled scanner, and the fail-fast pool. 122 lines came out.

### Two behaviour changes, measured

Two of those deletions changed what the program does, and the difference was measured rather than argued.

- **An unreadable directory** Point the old and new versions at a tree containing a directory you cannot read: the new one prints `permission denied` and counts the path as unreadable, and the old one, which could only ask whether the errno was `EISDIR`, left it out of the report entirely.
- **A failing URL** Make one of three URLs throw: `pool_settled` printed all three rows and named the failure, and `pool` printed nothing and died on an unhandled rejection, taking the two good results with it.

**Details the scanner rewrite and the alternation worry **

The scanner rewrite settled a design worry too. Since a capture group that did not participate reads as `""`, the same as one that matched empty, `captures_all` looked unable to tell you which branch of an alternation fired. It can, if you write each branch's group around the whole construct rather than around the part you want: a group that fired then starts with  ` , `[`, `!`, or `<`, and cannot be empty. The possibly-empty piece, the link target, nests inside and is never the thing you ask about. That turned a 180-line character walk into two patterns and a 12-line dispatch, with byte-identical output on the repo's own docs and on a fixture built to break it.

## Money doesn't run on floats

The gap that would end a fintech evaluation on the spot is doing money on IEEE-754 `number`, where `0.1 + 0.2` is not `0.3` and values past 253 quietly lose precision.

`std/decimal` is exact base-10 fixed-point over BigInt: an arbitrary-precision integer with a fixed number of fractional digits, so addition, subtraction, and multiplication are exact and division takes an explicit scale and rounding. A malformed amount parses to an `Err`, never a silent `NaN`.

std/decimal

```
let price = decimal("10.50")?
let tax   = decimal("0.84")?
price.add(tax).to_string()        # "11.34", exact

decimal("2")?.div(decimal("3")?, 2).to_string()   # "0.67", half away from zero
decimal("9007199254740993")?.add(decimal("2")?)   # exact past 2^53
```

### Large whole numbers

For exact large *whole* numbers, account ids and counters, there is the `bigint` type: literals are `123n`, it emits to a real TypeScript `bigint`, and a record field typed `bigint` validates `typeof === "bigint"` at the boundary, so an id sent as a JSON `number` is rejected rather than silently rounded past 253.

## An async callback finally has a type

A function type was `fn(A) -> T` and nothing else, which emits `(a: A) => T`. An async body does not fit that, so a function returning a deferred task, or a record of async handlers, had to go unannotated and the checker stopped looking.

`async fn(A) -> T` is that missing type. It emits `(a0: A) => Promise<T>`, and `async fn()` with no return emits `() => Promise<void>`.

a deferred task, and a map of handlers

```
fn task_for(url: string) -> async fn() -> Fetched {
  return async fn() -> Fetched { return { url: url, body: await fetch_one(url) } }
}

type Handler = async fn(string) -> string

async fn dispatch(routes: Record<string, Handler>, name: string, arg: string) -> string {
  return match record.get(routes, name) {
    Some(h) => await h(arg),
    None    => "no route",
  }
}
```

**Details how Glyph tells plain and async functions apart **

Glyph holds the two apart itself rather than leaving it to `tsc`: a plain function where an async one is expected is `E0204` at a return and `E0211` at a call argument, and the message names both sides.

The one case it doesn't judge is a `void` return, where TypeScript lets any function stand.

## A recursive type, matched all the way down

A union can name itself inside its own payload, with its type parameters, and a match arm can nest a constructor pattern inside another constructor pattern’s field. Put those together and the balancing rules of a red-black tree are four arms: each one names the exact shape it rewrites, and the compiler is the thing checking the shape rather than a comment above it claiming it.

one of the four rotation cases

```
type Tree<K, V> =
  | Leaf
  | Node({ color: Color, left: Tree<K, V>, key: K, value: V, size: int, right: Tree<K, V> })

fn balance<K, V>(t: Tree<K, V>) -> Tree<K, V> {
  return match t {
    Node({ color: Black, left: Node({ color: Red, left: Node({ color: Red, left: a, key: xk, value: xv, right: b }), key: yk, value: yv, right: c }), key: zk, value: zv, right: d }) => node(
      Red,
      node(Black, a, xk, xv, b),
      yk,
      yv,
      node(Black, c, zk, zv, d),
    ),
    // the other three cases, then:
    other => other,
  }
}
```

`examples/apps/leaderboard/main.glyph` is the whole program, a leaderboard that keeps every score in the tree and answers rank, top-N and range queries by walking it instead of re-sorting a log.

## Where it stands

### Shipping today 19

Modules and types

- `pub` visibility enforced on both ways of naming a sibling’s type (the named import and the `lib.Secret` annotation)
- Structural `interface` bounds
- `async fn(A) -> T` as a written type
- A union that names itself in its own payload matched by an arm that nests a constructor pattern inside another one’s field

Control flow and `match`

- `defer`
- `std/task` (`all`/`race`/`all_settled`/`pool`/`pool_settled`)
- `match` as the value of a `let` or a `mut` with blocks, `await`, and loop control in its arms
- A `match` whose arms agree carrying their type into the rest of the program
- A typed `T.parse`

Numbers and semantics

- `std/decimal` for exact money math
- `bigint` for exact large integers
- Digit separators
- A spec section owning the JS-inherited semantics

Standard library

- A standard library with `string.slice`, `index_of`, `repeat`, `pad_start`, `pad_end`, `replace_all`, `trim_start`, `trim_end` and `array.fold`, `index_of`, `flat_map`
- `fs.read_dir`, `fs.is_dir`, `fs.stat`
- An `FsError.kind` taxonomy the checker holds you to
- `regex.captures_all`
- Most of `std/string` and `std/array` carry their return type into your program

  **Details**

  Most of `std/string` and `std/array` now carry their return type into your program, so `string.split` is an `Array<string>` and the loop over it binds a numeric index.

- `std/record` is modeled, with the value type read off the record you pass

  **Details**

  `std/record` is modeled too, with the value type read off the record you pass: `record.get(t, k)` on a `Record<string, Array<string>>` is an `Option<Array<string>>`, and `record.keys(t)` is an `Array<string>`, so `array.sort(record.keys(t), cmp)` keeps its element type.

### Where it stops 11

Visibility

- A private import reports the same message as a missing name

  **Details**

  A private import reports “not exported by M”, the same message as a missing name; telling “private, mark it `pub`” apart from “no such name” is a queued refinement.

- A private value read through a namespace is a `tsc` error, not a Glyph one

  **Details**

  Visibility is checked on type annotations, not on values read through a namespace: `lib.helper()` where `helper` is private is still a `tsc` error rather than a Glyph one.

Async

- A top-level `await` in a module-level `const` initializer is allowed E0222

  **Details**

  Async context is checked now (`E0222`) except in one spot: a module-level `const` initializer has no enclosing callable, so a top-level `await` there is allowed and the emitted ESM runs it.

Inferred types

- Arms that disagree on the type's head leave the result unknown to Glyph

  **Details**

  The arm join still needs the arms to agree on the type's head: when they produce different types the result is unknown to Glyph and only `tsc` checks what you do with it.

- Nine stdlib functions are in the same position

  **Details**

  Nine stdlib functions are in the same position. Six take an optional trailing argument (`array.slice`, `string.slice`, `string.index_of`, `string.pad_start`, `string.pad_end`, `json.stringify`) and the arity check compares one number against one number, so modeling them would report a false error on every call that omits the last argument; `array.map`, `flat_map` and `zip` take their element type from a callback the checker doesn't walk into.

  A value straight out of those nine needs an annotation before you iterate it with an index, and a `match` on `string.index_of` with no `None` arm still builds clean and throws at run time. `array.index_of` is modeled and does report `E0200`.

- Members and call arity against a value the checker couldn't type are unchecked

  **Details**

  Members and call arity against a value the checker couldn't type are still unchecked at that boundary rather than being an error.

Library

- `string` indexes UTF-16 code units, and no `chars` or `char_at` is planned

  **Details**

  On the library side, `string` indexes UTF-16 code units the way `len` and `split` always have, and that is settled: there is no `chars` or `char_at` and none is planned, because a returned surrogate half looks like a character right up to the point where it's written out. Encode with `encoding.hex_encode` and read two hex digits at a time.

- `captures_all` gives no offsets; `fs.read_dir` keeps the OS's order

  **Details**

  `captures_all` hands back groups and no offsets, so a scanner that needs to know where in the line a match started still writes its own loop, and `fs.read_dir` gives you entries in the OS's order, so sort them when the output has to be reproducible.

Patterns

- A payload constructor in a field fails on a union reached through a namespace import E0300

  **Details**

  A nested field pattern has one edge left: a constructor that carries a payload, sitting in a field, on a union reached through a namespace import. `import tree { Node }` then `Node({ left: Node({ key: k }) })` compiles, and `import tree` then `tree.Node({ left: tree.Node({ key: k }) })` is `E0300` on the same union.

- A union generic over its own parameter takes the nested arm and is checked for missing variants E0200

  **Details**

  The other edge is gone: a union generic over its own parameter (`type Tree<K> = Leaf | Node({ left: Tree<K>, key: K, right: Tree<K> })`) used to refuse the nested arm that the same union without the parameter accepts, and now takes it, at an open type parameter and at a concrete instantiation alike. The same lookup was deciding whether the match got checked for missing variants at all, so until this release a `match` on `Tree<K>` could omit `Leaf`, build clean, pass `tsc --strict`, and throw at run time where the non-generic spelling was `E0200`.

  Both spellings report it, and moving the union one import away no longer changes the answer: a `match` on an imported `Tree<string>` that omits `Leaf` is `E0200` naming `Leaf`, the same as the imported non-generic `Tree`. That was a different code path, checked separately, and it went the other way until this release, building clean and throwing at run time.

- Coverage of an imported union never checks its variant payloads

  **Details**

  One thing about an imported union's coverage is still worth knowing before you lean on it: its variant payloads are never checked either, so `B(X)` where `X` is a variant of the payload's own union matches every `B` instead of reporting the missing `Y`, and that one has nothing to do with generics.

  Binding a field, or matching a tag with no payload in one, works in every case.

Code examples on this page are checked against Glyph 0.1.155 on every build. Last changed 21 Nov 2026.
