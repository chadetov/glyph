<!-- Markdown copy of https://glyphlang.io/answers/verifiability/, generated from the same source as the page. Last changed 2026-11-24. -->

01 · The core promise

# What stops an agent from silently breaking my code?

A whole class of change that **quietly compiles** in TypeScript (a missing case, an unhandled error, a renamed field) becomes a **compile error the agent has to fix before it can ship**.

status.ts one new state, no other edit  tsc --strict: passes, prints undefined

```
type Status =
  | { tag: "Loading" }
  | { tag: "Ready" }
  | { tag: "Failed"; msg: string }
  | { tag: "Cancelled" };

function label(s: Status) {
  switch (s.tag) {
    case "Loading": return "…";
    case "Ready": return "done";
    case "Failed": return s.msg;
  }
}
```

status.glyph one new variant, and the compiler finds every gap  refused: E0200

```
type Status = Loading | Ready | Failed(string)   // + Cancelled

fn label(s: Status) -> string {
  return match s {
    Loading => "…",
    Ready => "done",
    Failed(msg) => msg,
    // no arm for Cancelled ⇒ this match no longer compiles
  }
}
```

glyph check --no-tsc, 0.1.149 (excerpt)

```
[E0200] Error: typecheck: non-exhaustive match on `Status`: missing variants `Cancelled`
    ╭─[ status:6:10 ]
  6 │ ╭─▶   return match s {
 10 │ ├─▶   }
    │ ╰───────── non-exhaustive match on `Status`: missing variants `Cancelled`
    │     Help: Add an arm for each missing variant.
```

Add a state to a union, and every place that handled the old set stops compiling, pointing at the exact spot to update. The equivalent TypeScript `switch` compiles clean and returns `undefined` at runtime.

## Why it matters

Agents don't usually break code loudly. They break it **confidently**: they add a state, drop a branch, or rename a field, everything still type-checks, and the bug surfaces in production.

The expensive failures are the ones that *compiled and were wrong*. Glyph closes that gap by turning “you probably handled this” into “the compiler proved you did.”

- **Sealed unions** Tagged unions are **sealed**, so `match` must cover every case.
- **Errors as values** Errors are **values** you have to unwrap, not exceptions that vanish.
- **Checked boundaries** And every type carries a runtime descriptor, so data crossing a boundary is checked against its shape.

## A dropped error is a warning

Same for errors: a function that returns `Result<T, E>` is meant to be handled with `match` or propagated with `?` (and the compiler checks the error type lines up). And if you *drop* a `Result` on the floor (call it as a statement and ignore what it returned), Glyph warns you (`E0217`), because a discarded error is a swallowed failure.

glyph check --no-tsc, 0.1.149 (excerpt)

```
13 │   save("a")
[E0217] Warning: typecheck: this `Result` is discarded; its `Err` case is silently ignored
    │ Help: Handle it with `match`, propagate it with `?`, or bind it (`let _ = ...`) to say the discard is intentional.
```

You silence it deliberately with `let _ = ...`, never by accident.

## The standard library's unions are sealed too

The bar is the same for a union the standard library owns. `fs.ErrorKind` is a closed set of six kinds, and the checker knows its shape, so `match e.kind` on a filesystem error needs no `else` arm and fails with `E0200` when you leave a kind out.

E0200 missing kind E0210 unknown field E0218 needs else

**Details field typos, return types, number and string matches **

`e.mesage` is `E0210` rather than something `tsc` catches downstream. Return types travel too: `string.split(text, ",")` is an `Array<string>`, so `for i, part in string.split(text, ",")` binds `i` as a number instead of the string key you'd get from an untyped iterand.

Exhaustiveness isn't just for unions. A `match` on a `number` or `string` with only literal arms can't cover an unbounded domain, so Glyph requires an `else` (`E0218`) rather than letting the uncovered value fall through to a runtime throw.

The rule is the same everywhere: a `match` that doesn't account for a case doesn't compile.

## A misspelled arm is a gap, not a catch-all

A typo can't quietly paper over a gap either. A capitalized arm head is a **reference to a variant**, not a fresh binding.

status.glyph: a misspelled arm is a gap, not a catch-all

```
fn label(s: Status) -> string {
  return match s {
    Loadign => "…",        // typo for Loading, read as a variant, not a binding
    Ready => "done",
    Failed(msg) => msg,
  }
}
```

glyph build

```
[E0220] `Loadign` is not a variant of `Status`; did you mean `Loading`?
   ╰─ a lowercase name would be a binding; a PascalCase name is read as a variant
[E0200] non-exhaustive match on `Status`: missing variant `Loading`
```

**History what older Glyph did with the typo **

A capitalized arm head is a **reference to a variant**, not a fresh binding, so if you misspell `Loading` as `Loadign`, older Glyph read the misspelling as a catch-all that bound every value and made the match look total. Now the head has to name a real variant of the union you're matching. When it doesn't, the build stops with `E0220` and points at the nearest real name, and the variant you actually skipped still shows up as `E0200` right next to it.

## Variants inside a payload

That reading holds one level down, where the variant is the payload of another one. The arm now dispatches on the payload's own tag, whatever the outer variant is.

repl.glyph: the inner arm tests the payload, it doesn't bind it

```
fn step(z: Zipper, line: string) -> Zipper {
  return match parse_command(line) {
    Err(Blank) => z,                          // a comment or an empty line
    Err(e) => { io.println(explain(e)) z },   // every other parse error
    Ok(command) => execute(z, command),
  }
}
```

E0305 duplicate case label

**History the dead arm before 0.1.93, and the E0305 guard **

`Err(Blank)` beside `Err(e)` reads as "ignore a blank line, print every other parse error", and until 0.1.93 it compiled to two `case "Err":` labels on the outer tag whose first bound the whole payload under the name `Blank`. Every error took the first arm, the second was dead code, and nothing said so: Glyph reported no diagnostics and `tsc --strict` accepts a duplicate case label. The arm now dispatches on the payload's own tag, whatever the outer variant is, and the compiler reads the name the way the typechecker does: the payload union's own variant list decides, and only when that list is out of reach does the name's shape decide. For a union declared in the same file the capitalization no longer matters. For one imported from another module the variant list is out of reach today, so a capitalized variant dispatches and a lowercase one stops the build.

It stops because the switch is guarded now. Two arms that would write the same `case` label are `E0305`, not a switch whose second label can never run. That is what makes the class loud instead of silent: a lowering that reaches for the same tag twice fails the build rather than picking the wrong arm at run time.

## Import spelling never turns the check off

How the union got into scope doesn't change any of this. Glyph lets you write a variant arm two ways, and both are checked the same: `import model { Cond, Yes, No }` with bare arms, or `import model` (or `import model as m`) with `model.Yes(_)`.

interp.glyph: the import list came out, the match is still checked

```
import model { Action, All, Any, AtLeast, Clear, Condition, Context, Counter,
               Equals, Flag, FlagSet, Increment, Not, Set, Slot, Text }
import model
import std/result

pub fn evaluate(ctx: model.Context, c: model.Condition) -> result.Result<bool, SlotError> {
  return match c {
    model.Equals({ key, value }) => result.Ok(read_text(ctx, key, "equals")? == value),
    model.AtLeast({ key, least }) => result.Ok(read_count(ctx, key, "at-least")? >= least),
    model.FlagSet({ key }) => result.Ok(read_flag(ctx, key, "flag-set")?),
    model.All({ of }) => all_of(ctx, of),
    model.Any({ of }) => any_of(ctx, of),
    model.Not({ of }) => result.Ok(!(evaluate(ctx, of)?)),
  }
}
```

glyph build, after deleting the last arm

```
[E0200] non-exhaustive match on `Condition`: missing variants Not
```

**Details standard library unions, string enums, and why the list existed **

That holds for the standard library's unions too, so `match o { option.Some(s) => … }` on an `option.Option<string>` is `E0200` for the missing `None` exactly as the bare `Some`/`None` spelling is. Until 0.1.56 the qualified spelling skipped the check entirely and the build reported no diagnostics on a match that threw at run time.

A string enum crosses the boundary too. `type ColType = "text" | "int" | "real" | "bool"` declared in `catalog.glyph` and imported into another module keeps its four values, so a match covering all four compiles with no `else`, and dropping one is `E0200` naming the value you dropped. Until 0.1.57 it was `E0218`, with help text telling you to add an `else` arm. That help was wrong about the code in front of it, and taking it turned a compile error into a runtime fallthrough. All three import spellings work: `import catalog { ColType }`, `import catalog` with `catalog.ColType`, and `import catalog as c` with `c.ColType`.

That import list is where this came from. It was written by hand in every module of a statechart engine, with a comment above it saying a union's constructors don't arrive with its type. The comment was right about the syntax and wrong about the reason: the list was there because the shorter spelling turned the check off, and nobody had noticed, because turning it off is silent. Which form you pick is now a real choice rather than a trap. The named one puts every variant you handle in the import list, which is what a `grep` for `Equals` finds. The qualified one says at each arm which module the variant came from. Neither costs you the check.

## Imported records keep their fields

Records cross now too, and until 0.1.58 they did not. A record therefore keeps its field set, and a bad field is `E0210` naming the record rather than a `tsc` error about an anonymous shape.

glyph build, on a typo'd field of an imported record

```
[E0210] type `Sheet` has no field `rowz`; did you mean `rows`?
// Before 0.1.58: no Glyph diagnostic at all, only a tsc error downstream.
```

table.glyph: the hoists and the comment explaining them are gone

```
// Both annotations are load-bearing. `for i, x in <record field>` picks its
// lowering from the iterand's type as the checker knows it, and a field
// access is not enough: without the `let`, `i` and `j` come out as strings
// and the row/column numbers in a `BadCell` are wrong.
let raw_rows: Array<Array<string>> = sheet.rows
for i, raw_row in raw_rows {
for i, raw_row in sheet.rows {
  let typed: Array<Value> = []
  let cols: Array<ColumnSpec> = spec.columns
  for j, col in cols {
  for j, col in spec.columns {
    …
  }
}

// sheet: Sheet, declared in catalog.glyph and imported here.
// Emitted TypeScript: sheet.rows.entries(), not Object.entries(sheet.rows).
```

**Details the line 11 bug, what tsc misses, how identity crosses **

A type declared in one file and used in another arrived with nothing known about it, which cost two separate checks. A misspelled field on it drew no Glyph error at all, so `sheet.rowz` fell through to `tsc` and came back as a TypeScript message about a shape you never wrote. And `for i, r in sheet.rows` could not tell that `rows` was an array, so it emitted `Object.entries(sheet.rows)` rather than `sheet.rows.entries()`. That binds `i` to the string `"0"`. `i + 1` then concatenates instead of adding, and a row that should be reported as `line 2` reports as `line 11`.

The part worth knowing is which half `tsc` catches. Arithmetic on that index it does catch, though it reports the error against generated TypeScript for a variable Glyph bound on your behalf. What it cannot catch is every place a string is accepted where a number was meant: string interpolation, concatenation, a `record.get` key. Those compile clean under both checkers and print the wrong number. That is the shape of bug this project exists to remove, so a module boundary that reintroduces it is not a boundary Glyph can leave alone.

An imported type now carries an identity across the boundary, keyed on the module that declares it and the name it declares, so all three import spellings resolve to the same type. The declaration itself is fetched on demand and lowered on the declaring module's side, which is the half a consumer cannot do: resolution runs over spans, and an imported declaration's spans belong to another file.

The query engine in `examples/apps/csvql` is where this surfaced. It carried three `let` hoists whose only job was to restate an imported field's type for the emitter, under a four-line comment explaining that without them the row and column numbers in an error message come out wrong. All three are deleted and the app prints byte-identical output for all twelve queries.

One thing still stops at the boundary. Passing a `catalog.Sheet` where a `table.Row` is expected is E0211 naming both types, but an `interface`'s member list does not cross, so a record passed where an imported interface is expected is compared against nothing. Giving it the treatment a record's field set gets would quietly change what structural satisfaction means, which is a language decision rather than a fix.

## PII masking lives on the type

The same "make it type-level, not convention" instinct covers PII. Put `@redact fields: [ssn, dob]` on a type and the compiler generates a `redact()` on its descriptor that masks those fields, so `json.stringify(User.redact(u))` can't leak them.

E0219 unknown field in @redact

Name a field the type doesn't have and it's a compile error (`E0219`), not a redaction that silently does nothing.

## Refinements are checked all the way down

A descriptor is only worth anything if it's the check the type declares, all the way down. A `where` refinement runs its predicate wherever the type appears, not just at a direct `Instant.parse`: as a record field, as an `Array` element, as an `Option` payload, inside a union variant, and through `json.parse<T>`.

1. Calendar
2. Array<WireParticipant>
3. busy
4. start
5. is_instant(value)

The fix is four levels down from the boundary: `Calendar` to `Array<WireParticipant>` to `busy` to `start` to the predicate.

schedule.glyph: the predicate follows the type into the field

```
fn is_instant(value: string) -> bool {
  return match time.parse_iso(value) {
    Some(_) => true,
    None => false,
  }
}

type Instant = string where is_instant(value)

type WireBlock = { start: Instant, end: Instant }
type WireParticipant = { name: string, busy: Array<WireBlock> }
type Calendar = { participants: Array<WireParticipant> }

// json.parse<Calendar>(text) on a file holding "start": "no"
//   => Err. It returned Ok before 0.1.41.
```

**History the confident wrong schedule before 0.1.41 **

The same goes for a record imported from another module. Until 0.1.41 both of those fell back to checking that the key was present, which is not a check of anything.

The scheduling app below is where this surfaced. It reads calendars from a JSON file and needs every timestamp to be a real ISO-8601 instant. Before, the refinement stopped at the top level, so a hand-edited file with `"start": "no"` parsed fine, the instant read as 0, and the app printed a confident schedule and exited 0.

## Tests run as part of the build

Tests live next to the code and run as part of compiling it. An `@example` above a function, or a ```` ```glyph @run``` ```` block inside a `@doc`, is executed by `glyph build` with no flag to remember.

bracket.glyph: break the seeding and the build stops

```
@example seed_order(2) == [1, 2]
@example seed_order(4) == [1, 4, 2, 3]
@example seed_order(8) == [1, 8, 4, 5, 2, 7, 3, 6]
fn seed_order(size: int) -> Array<int> { … }

// glyph build src --out dist
example failed: bracket example #16:
  (seed_order(8)) != ([1, 2, 3, 4, 5, 6, 7, 8])
1 of 23 example(s) failed.            # exit 1, no "tsc passed" line

// glyph build src --out dist --json
{ "ok": false, "errors": 1, "tsc": "passed",
  "examples": { "total": 23, "ran": true, "skipped": false,
    "failures": ["bracket example #16: …"] } }
```

**Details why it can't be skipped quietly, and the JSON channel **

A false one fails the build the same way a type error does, which is the point: an agent rewriting a function body cannot pass the build by leaving the assertion behind. `--no-test` opts out and prints how many tests it skipped, so the bypass is visible in the log.

The JSON output agents read reports the same verdict. `glyph build --json` carries an `examples` object next to the diagnostics, and a failing example makes `ok` false and the exit code non-zero. Until 0.1.43 the examples ran only under `--test`, and the JSON path returned before they ran at all, so the one channel an agent can read printed `"ok": true` on a project whose own assertion was false. If `tsx` isn't on `PATH` the build fails instead of reporting a pass it never checked.

The bracket app below carries 23 of these. `seed_order` builds standard bracket order by doubling, which is the kind of function that is easy to get subtly wrong and easy to describe exactly. Change the doubling rule and the build stops, without a test file, a runner, or a flag.

Found by building with it

## Four more, found by building with it

Each of these compiled clean and passed `tsc --strict`, and each was found by writing an application rather than by reading the compiler. They are the same failure in three places: the types said one thing and the program did another.

### Reading a key that may not be there

reading a key that may not be there: now E0224

```
fn name_of(row: Record<string, string>) -> string {
  return row.naem          // a typo. compiled, and returned undefined
}
```

It is now a compile error pointing at `record.get`, which returns `Option<V>` so the absent case has somewhere to go.

**Details why a map read is different from array indexing **

A map's keys are arbitrary, so the compiler cannot know the key is there. Typing the read as `string` stated something it had not checked, and the value printed as the text "undefined". Writing a key is untouched, and so is array indexing: a bound is a value a program can check, a map key is not.

### A method that answers `undefined`

a lookup that may find nothing: now E0242

```
let name = users.find(fn(u: User) -> bool { return u.id == id }).name
let last = xs.at(-1) ?? 0  // passed tsc --strict too

[E0242] the call to `users.find` returns `User | undefined`; Glyph writes a value that may be missing as an `Option`
Help: pass `users` and the same callback to `array.find`, which returns `Option<User>`, and `match` it
```

A few JavaScript methods say "nothing there" with `undefined`, which Glyph's own checker could not see, so the editor and the agent tools passed the `.name` read. Each one now points at the standard library function that returns an `Option`, and `glyph fix` writes the array ones.

**Details which methods, and what replaces them **

The list is read off TypeScript's declarations: `find`, `findLast` and `at` on an array, and `at`, `codePointAt` and `match` on a string. The replacements are `array.find`, `array.find_last`, `array.get`, `string.char_at`, `string.code_point_at` and `regex.matches`. A `??` default becomes the `None` arm of a `match`. `array.get` answers `None` for a negative index, so the help for `xs.at(-1)` writes `array.get(xs, array.len(xs) - 1)`. A method called on an npm value is not judged, because Glyph does not know its type.

### `==` is value equality everywhere

a test that passed while the code failed

```
@example wrap("a") == wrap("a")   // passed: deep equality
fn same() -> bool {
  return wrap("a") == wrap("a")   // false. no diagnostic
}
```

`==` is now value equality everywhere; primitives still compile to `===`, so ordinary comparisons are unchanged.

**History how a test could pass on broken code **

`==` was documented as value equality and lowered to `===`, which is *reference* equality the moment either side is a record, a tagged union, or an array. The same expression compared structurally in a test and by identity in the program beside it, so a test could report success on code that did not work.

### A loop binding keeps its element type

exhaustiveness that evaporated inside a loop

```
for t in tiers {
  match t {
    "free" => ...          // "pro" unhandled: once merely a warning to add `else`
  }
}
```

It now names the variant you have not handled, which is advice to satisfy the check rather than to switch it off.

**History the catch-all advice it used to give **

The loop binding carried no element type, so a `match` over a string-literal union degraded to a match over `string` and the compiler advised adding a catch-all.

### An index read has its element's type

a typo off `xs[0]`: now E0210

```
fn first_len(xs: Array<string>) -> number {
  let s = xs[0]
  return s.lenght
}

[E0210] type `string` has no field `lenght`; did you mean `length`?
```

`xs[i]` on an `Array<E>` is an `E`, so the editor, `glyph check --no-tsc` and the agent tools check what you do with it: a field read, a method call, a `return`, an argument, an annotated `let`.

**History what was checked before **

Up to 0.1.183 an element read by index had no type on Glyph's side. `return xs[0]` under `-> number`, a misspelled field on `rows[0]`, and `xs[0]` passed to a `number` parameter all went through, and only the `tsc` step of a full build caught them. The type is the one `tsc` gives the emitted code, which leaves `noUncheckedIndexedAccess` off: `E`, not `E | undefined`. Glyph's index throws a `RangeError` past the end instead of handing back `undefined`; use `array.get` when you want an `Option`. A key read on a `Record` is still `E0224` unless its value type is `unknown`, and an index on a `string` or on an npm value is not judged.

### Imported generic unions are checked

a variant left out of a match on an imported generic union

```
import tree { Tree, Node }

fn label(t: Tree<string>) -> string {
  return match t {
    Node({ key: k }) => k    // Leaf unhandled: built clean, threw at run time
  }
}
```

It is `E0200` either way now, at both import spellings and at an open parameter or a concrete instantiation alike.

**History why the check never started **

A union your own module declares is checked whether or not it takes type parameters. One imported from a sibling module was checked only when it took none. `Tree<string>` reaches the cross-module coverage path as an application of an imported union rather than as one, and the gate that starts the check was looking for the second, so the `match` was never counted at all: `tsc --strict` passed and a `Leaf` threw. Delete `<K>` from both files and the same program was `E0200`.

### A `parse` can't claim a check it didn't do

a `parse` that reported success without checking: now E0304

```
type Conn = { id: number, sock: Socket }

Conn.parse(body)   // Ok. `sock` was never checked, and the message said it was
```

Holding a socket in a record is ordinary and still compiles. Validating one is now refused where it is written and in every module that imports the record, naming the field and its type, and the refusal follows the shape: through a nested record, an `Array`, an `Option`.

**Details why presence is not a check, and the fix it asks for **

Every record type carries a runtime descriptor. For a field whose type the compiler cannot see into, a socket or anything reached through `extern_ts`, the generated check was "is it present" while the reported message read field `sock` must be Socket. A boolean that is always true is useless. One that is always true under a message naming a type nothing checked is worse, because `parse` is the thing a boundary is told to trust.

The fix it asks for is the split you would have written anyway, a wire type whose fields are all checkable, then a domain record you build from it. `unknown` is not caught by this, because it claims nothing and presence really is the whole check.

## A signature change, invisible at the call site: closed in 0.1.116

Change a function to take a payload instead of a raw string and every call still passing the old shape built clean.

billing.glyph: a union crosses a `string` parameter unnoticed

```
type PaymentResult = Settled(string) | Declined(string)

fn takes_string(s: string) -> number { return s.length }

fn charge(r: PaymentResult) -> number {
  return takes_string(r)   // compiled clean before 0.1.116
}
```

glyph build, now

```
[E0211] argument type mismatch: expected `string`, found `PaymentResult`
   ╰─ Pass a value of the expected type, or change the parameter's type.
```

**Details where the check fires, and where it stays quiet **

A parameter typed `string`, `number` or `bool` used to accept a value of a declared union or record with no complaint at all, even though the identical call with a mismatched `bool` argument was already `E0211`.

The check reads the declaration, so it only fires where the answer is certain. A primitive alias such as `type Id = string` is read as the `string` it names, so it is decided the same way. A string-literal union (D30) is compared by its literals, and an `interface` value is refused where a `string` is declared, because its value is an object. An `extern_ts` or `typeof` body still says nothing, and `void` is excluded too. It reaches an argument, a `return`, and a `let` annotation alike, since all three share one relation.

The other direction goes through the value's members, as in TypeScript: `5` is refused where `interface HasName { name: string }` is declared, and `"abc"` is accepted where `interface HasLen { length: number }` is, because a string has a `length`. A `bigint` is read through its own members, so it is refused there: it has no `length`. Two pairings are not decided yet and are left to `tsc`: an interface whose members are all optional and share none with the value (G526), and `Option`, `Result` and `Record` against an interface (G527).

### An annotated `const` is typed

A `const` had the matching gap on the receiving side.

catalog.glyph: an annotated const, and the field it does not have

```
type Sheet = { rows: number, cols: number }
const ORIGIN: Sheet = { rows: 0, cols: 0 }

fn f() -> number {
  return ORIGIN.rowz   // compiled clean before 0.1.116
}
```

glyph build, now

```
[E0210] type `Sheet` has no field `rowz`; did you mean `rows`?
```

**Details unannotated consts, and why both fixes are breaking **

A module-level `const` carrying a type annotation lowered to `Ty::Unknown` no matter what the annotation said, so a field typo on it was invisible exactly where the identical annotated `let` inside a function already reported it.

Since 0.1.166 an unannotated `const` has its initializer's type, read the way `tsc` reads the TypeScript `const` the emitter writes: `const MODE = "read"` keeps its literal type, and a literal inside an object or an array widens, so `const ITEMS = [1, 2]` is an `Array<number>` and `ITEMS.push(3)` outside `mut` is E0240. An initializer the checker cannot type leaves the `const` untyped. Both fixes are breaking under `glyph check --no-tsc`, the language server and the playground, since each starts catching a program the previous release accepted; a full `glyph build` was already red through `tsc` in both cases.

### A field the record does not declare

a misspelled optional field: now E0210

```
module profile

pub type Profile = {
  name: string,
  nick?: string,
}

pub fn guest() -> Profile {
  return { name: "guest", nik: "g" }   // [E0210] type `Profile` has no field `nik`; did you mean `nick`?
}
```

A key the declared record does not have used to pass Glyph's checker, so a typo in an optional field threw its value away and only `tsc` noticed. The editor, the agent tools and `glyph check --no-tsc` refuse it now, with the error a read of the same field gives, and `glyph fix` renames `nik` to `nick`.

**Details where it is judged, and what is left alone **

It is judged wherever a written literal meets a declared type: a `let`, a `return`, a call argument, a variant payload, a `mut` assignment, a nested field, an array element, a `match` arm, and the argument of `Some`, `Ok` or `Err`, which `tsc` lets through. A binding that holds a wider record is still accepted, as `tsc` accepts it, and so are fields that come from a spread and keys written for a `Record<K, V>`.

## Where it stands

### Shipping today 21

Matching

- Exhaustive `match` over sealed unions: yours or a sibling module's, generic or not, and the standard library's, such as `fs.ErrorKind` E0200
- Exhaustive `match` over `number`/`string` E0218
- A typo'd variant arm caught as a compile error with a nearest-variant suggestion instead of a silent catch-all E0220
- A variant in payload position dispatching on the payload's own tag rather than binding it
- Two arms that would answer the same tag refused as `E0305` instead of one silently winning
- A `for` binding that keeps its element type
- An index read, `xs[i]`, typed as its element, so its uses are checked

Errors and tests

- `Result` + the `?` operator with error-type checking
- A warning when a `Result` is discarded E0217
- `@example` / `@doc @run` tests that run on every build, `--json` included

Types, records and literals

- A declared union or record passed where a `string`/`number`/`bool` is expected E0211
- A field typo on an annotated module-level `const` E0210
- An object literal key its declared record does not have E0210
- `==` as value equality on every type
- Reading a key out of a map rejected E0224
- A built-in method that answers `undefined`, such as `find` or `at`, refused with its `Option` form named E0242 since 0.1.161
- Object literals and JSX attributes checked against the declared type E0204 E0211 E0210 since 0.1.124

  **Details**

  Since 0.1.124 an object literal has the record type its fields spell, so `let g: string = { x: 1 }`, a literal missing a field the declared record requires, and a literal passed where it does not fit are E0204 or E0211 with the declared type named, at a `let`, a `const`, a `return` and a call argument; and a JSX attribute is checked against the component's props record, so `<Button variant="nope" />` draws E0211 with the values it accepts and an attribute the record does not declare draws E0210, which `glyph fix` renames.

- A missing required field or prop is named E0211 E0229 since 0.1.125

  **Details**

  Since 0.1.125 a record literal or a component element that leaves out a required field names it, in the message and in `missing_fields` on `--json`, and an element missing a required prop is E0211 even when it writes no attributes at all; a component whose one parameter is not a record is E0229.

- Every field of a record compared by the same rule as the record since 0.1.127

  **Details**

  Since 0.1.127 every field of a record is compared by the same rule as the record, so a nested record, a declared type or a string-literal union inside one is refused where `tsc` refuses it, and so are an optional field where a required one is declared, a function whose return does not fit and a local interface's members. A record whose fields are all optional refuses a value that shares none of them, as TS2559 does. A function's parameters are still not compared.

Data boundaries

- Runtime type descriptors
- `@redact` PII masking on the type descriptor E0219

### Getting sharper 14

Matching

- Coverage of an imported union doesn't look inside a variant's payload yet next to deepen

  **Details**

  A type parameter used to switch the check off: through 0.1.90 a `match` on `Tree<K>` could omit a variant, build clean, pass `tsc --strict`, and throw at run time, where the same match on `Tree` was `E0200`. That is closed for a union your own module declares and, as of 0.1.95, for one an import away. What is still shallower on the imported path is the depth rather than the gate: the check that runs for an imported union never looks inside a variant's payload, so `B(X)` over an imported union whose payload is itself a union matches every `B` rather than reporting the missing `Y`, and no type parameter is involved. That is the next thing to deepen.

- A lowercase payload variant of an imported union stops the build E0305

  **Details**

  A variant in payload position is read off the payload union's variant list now, so the capitalization of the name stops mattering for a union your module declares. For one imported from another module there is no list to read and the shape of the name still answers, which means `Err(Blank)` dispatches and `Err(blank)` stops the build at `E0305`: a loud failure on a valid program, and it needs the same imported-union registry the shallow coverage check above needs.

- A constructor pattern over a record payload (`Ok(Point)`) fails in `tsc`, not Glyph

  **Details**

  A constructor pattern over a payload that is a record rather than a union (`Ok(Point)`) fails through `tsc` naming a `.tag` you never wrote, where it should be a Glyph diagnostic naming the type and the pattern.

- A misspelled bare arm reports two lines for the same name E0220 E0103

  **Details**

  The typo check (`E0220`) now covers a union declared in the module and one reached through a namespace import (`model.Nooo(_)`) alike, constructor-shaped or bare: through 0.1.115 a bare misspelled head over an imported union (`Loadign`, no arguments) was credited as covering a variant and said nothing, where the same typo on a local union already reported `E0220`; both spellings report it now. A misspelled bare arm also trips `E0103`, one stage earlier, so you get both lines for the same name.

Types, records and literals

- A literal holding a spread has no type

  **Details**

  A literal holding a spread has no type, so a field missing from it or holding the wrong type is left to `tsc`. A key written beside the spread is still checked against the declared record.

- An imported `interface`'s members and an imported `const`'s type don't cross yet

  **Details**

  Types cross a module boundary intact now: unions keep their exhaustiveness, tagged and string-literal alike, and a record keeps its field set, so a typo'd field on an imported record is `E0210` naming that record's own type and `for i, x` over one of its array fields binds a number with no annotation. What does not cross yet is an `interface`'s member list, so a record passed where an imported interface is expected is compared against nothing; an imported `const` is the same shape one level over: it is still `Ty::Unknown`, so a field typo through a named or namespace import stays silent, whether or not the source module annotated it.

- Deeper flow-narrowing is on the roadmap

Data boundaries

- A `find` or `at` on an npm value is not judged; only an array or a string Glyph has typed is E0242
- Maps from another module or the standard library are read unchecked E0224

  **Details**

  A map that arrives from another module or the standard library is still read unchecked, so E0224 does not yet cover a `sqlite.Row`, and the two-binding `for i, x in xs` still loses its types.

- A type from a plain `.d.ts` has no descriptor; materialize it with `glyph gen dts` E0304

  **Details**

  A type imported from another Glyph module now validates through its own descriptor, as does a `where` refinement in field position; a type imported from a plain `.d.ts` has no descriptor to validate through, and `parse` on a record holding one is now refused (`E0304`) rather than reporting a success it did not check; materialize it with `glyph gen dts` to get a type that validates.

Tooling and the standard library

- Nine `std/string` and `std/array` functions still return untyped values

  **Details**

  The stdlib return types the checker models cover the fixed-arity half of `std/string` and `std/array`; the six functions with an optional trailing argument (`array.slice`, `string.slice`, `string.index_of`, `string.pad_start`, `string.pad_end`, `json.stringify`) and `array.map`/`flat_map`/`zip` are still untyped, so a value straight out of one of those needs an annotation before you iterate it with an index.

- Member access on primitives and arrays has no diagnostic where `tsc` doesn't run

  **Details**

  Member access and calls on `string`, `number`, `bool` and `Array<T>` are left to `tsc` by design, not by gap; Glyph permits raw JS members there, so `s.length` and `xs.length` compile and pass `tsc --strict`, and what stays missing is a diagnostic for the four surfaces that never run `tsc`: `glyph check --no-tsc`, the language server, the MCP server, and the playground.

- An offline build with examples needs `tsx` or `--no-test`

  **Details**

  The example runner shells out to `tsx`, so a project that has examples and no `tsx` on `PATH` fails the build rather than skipping quietly; that is deliberate, but it means an offline build needs `tsx` installed or `--no-test`.

- An `await` in a plain `fn` is caught against your own source E0222

  **Details**

  Async context used to be the one rule Glyph left to `tsc`; an `await` in a plain `fn` is now `E0222` against your own source, with the top-level `const` initializer the one case still allowed. That and two other rules that moved up a stage are in [caught earlier](https://glyphlang.io/answers/early-errors/).

Why this is the first of Glyph's four pillars, and where its guarantee stops, is on the [verifiability pillar page](https://glyphlang.io/pillars/verifiability/).

Code examples on this page are checked against Glyph 0.1.184 on every build. Last changed 24 Nov 2026.
