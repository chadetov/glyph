<!-- Markdown copy of https://glyphlang.io/answers/diff-stability/, generated from the same source as the page. Last changed 2026-11-21. -->

02 · Reviewable change

# Will agent edits stay clean, or reformat my whole file?

A one-line change is a **one-line diff**, every time. Glyph has a single canonical layout, so an agent **can't bury a real change under a wall of reformatting**.

TypeScript: add "/billing"

```
 const routes = [
   "/home",
-  "/about"
+  "/about",
+  "/billing"
 ]
```

3 lines moved for 1 change

Glyph: add "/billing"

```
 let routes = [
   "/home",
   "/about",
+  "/billing",
 ]
```

1 line added, exactly the change

Add a route. In TypeScript's common style the previous last line has to churn just to grow a comma. Two lines move for a one-item change.

## Why it matters

When an agent opens a pull request, the diff *is* the review. If a genuine one-line fix arrives as a 200-line reformat, nobody can see what actually changed, and that noise is exactly where a bug slips through.

Diff stability means the diff shows the change and **nothing but the change**.

Two rules do most of the work. Here's each, shown as the diff you'd review.

## Trailing commas, one item per line

Trailing commas are **required**, and a list that doesn't fit on one line goes one element per line rather than being repacked, so inserting, removing, or reordering an entry never disturbs its neighbors.

The same holds for record fields, parameters, match arms, and imports.

The threshold has a cost: a rename that pushes a call past 100 columns expands the whole argument list to one per line.

## One canonical layout

There is exactly one correct formatting, produced by `glyph fmt`. Whitespace, wrapping, and alignment aren't choices an agent can “tidy,” so a semantic edit can't drag reflow along with it. Change one field's type, and that's the whole diff:

user.glyph: change one field, review one line

```
 type User = {
   id: string,
-  age: string,
+  age: number,
   email: string,
 }
```

Because the layout is fixed, two different agents editing the same file produce **byte-identical** formatting, no reflow churn, no “my formatter vs. yours” noise, and a clean `git blame` that points at the commit that actually changed a line, not the one that reformatted it.

## One canonical layout, so does the formatter move my comments?

It used to, and that was a bug worth naming. The rule now: a comment is re-emitted above the item it was written above, inside a record body, a union variant list, an array or object literal, an argument or parameter list, and a `match`.

**History how comments drifted, and the app that surfaced it **

Glyph has no block comments and no doc-comment syntax, so a `//` line is the only way to explain a record field, a union variant, or a `match` arm. The formatter flushed pending comments only at declaration and statement boundaries, which meant a comment written *inside* a construct drifted down to the next declaration and read as documentation for whatever it landed on. Writing Minesweeper in Glyph is what surfaced it. These two declarations come from [the real app](https://github.com/chadetov/glyph/blob/main/examples/apps/minesweeper/main.glyph) (comments trimmed to fit), run through the old formatter and the new one.

0.1.37: one fmt pass

```
type Visibility =
  | Hidden
  | Flagged
  | Revealed

// Face down. Prints as `.`.
// Face down and marked by the player.
// Face up. `Cell.adjacent` is what gets printed.
type Cell = { mine: bool, adjacent: int, visibility: Visibility }
// Number of mined neighbours. Only meaningful
// once mines are placed.
```

three variant comments now document a record

0.1.38: the same file

```
type Visibility =
  // Face down. Prints as `.`.
  | Hidden
  // Face down and marked by the player.
  | Flagged
  // Face up. `Cell.adjacent` is what gets printed.
  | Revealed

type Cell = {
  mine: bool,
  // Number of mined neighbours. Only meaningful
  // once mines are placed.
  adjacent: int,
  visibility: Visibility,
}
```

every comment on what it describes

And a construct holding an interior comment always takes the one-element-per-line form, at any size, so the comment has an item to sit above. So `type Shape = { w: int, h: int }` still collapses to one line, and the moment you document `h` the record stays expanded.

**Details why this counts as a correctness bug **

Why this counts as a correctness bug and not a cosmetic one: nothing failed. The build exited 0, `tsc` passed, and the mangled output was itself a fixed point, so `glyph fmt --check` in CI went green on a file whose comments now said something false. That is the same class of failure Glyph exists to remove, just in the formatter rather than the type system.

## Can `glyph fmt` break a program that builds?

It could, and one case is still open. A formatter your program has to survive is worse than a missing feature, because it damages code that already worked.

- **Arm bodies** The printer now parenthesizes an arm body whose leftmost token is a `{` that would reparse as a block, which is the empty object and anything built on it (`({}).size` comes back as `({}.size)`).
- **The fuzzer** The fuzzer now parses the formatted file and compares the tree with the original's, so a layout that changes what a file means fails on the first pass instead of waiting for someone to notice.

**History the empty record that stopped building **

The fixed one first. An arm meaning “the empty record” is written `true => ({})`. Arm-body position is the only place in the grammar where a leading `{` is ambiguous, and the parser settles it by requiring `key:` or `...` right after the brace. An empty object has neither, so when the formatter dropped the parentheses and reprinted `true => {}`, the arm came back as an empty *block* and the file stopped building: `E0223`, the arm produces no value. You ran a formatter on working code and got code that no longer compiled.

An arm body that already reads unambiguously, like `false => { a: "x" }`, stays bare, so there is no new punctuation to read anywhere else. `fmt` is a fixed point on the result and the emitted TypeScript is unchanged.

**History two more fixed in 0.1.138 **

Two more were fixed in 0.1.138. Inside a block, a line that starts with `(` continues the line above, and up to 0.1.137 the formatter printed `load()?` followed by `.run()` as `(load()?).run()`, so after `let a = 1` the formatted file meant `let a = 1(…).run()`. It now keeps `load()?` with `.run()` indented on the next line, and writes `e?(x)`, `e?[0]` and `e? ?` without parentheses. The other was a union's `where` refinement, which the formatter dropped, turning a program `check` refused into one it accepted.

The formatting rules behind this, and the trade they make, are on the [diff stability pillar page](https://glyphlang.io/pillars/diff-stability/).

## Where it stands

### Shipping today

- Required trailing commas, one-item-per-line lists, and `glyph fmt`'s single canonical layout, round-tripping and idempotent, with format-on-save in the editor.
- `glyph fmt` never moves a comment to somewhere it does not belong.
- Comments are kept above the item they were written above

  **Details**

  Comments are kept above the item they were written above, including inside a record, a literal, an argument list, a `match`, between the operands of an operator, and between the steps of a method chain, which then goes one step per line with the dot leading.

### The edge, and what's next

- A comment at the end of a code line moves to the line above the next item

  **Details**

  A comment is always emitted on its own line, so one written at the end of a code line (`w: int, // width in cells`) moves to the line above the next item. It no longer crosses a declaration boundary, but it does move; keeping it trailing needs the printer to track which line a comment shares.

- A comment the layout has nowhere to put is refused, not moved E0500

  **Details**

  A comment where the layout has nowhere to put it, such as inside an index, after a call's arguments in the middle of a chain, or before a `!` or `await` operand, is not moved any more: `glyph fmt` refuses the file, leaves it as it was, and names the line and where to put the comment, and `glyph check` reports the same thing as warning E0500.

- A comment between JSX attributes or between the fields of an object pattern is refused too, where a one-per-line layout could place it; those layouts are on the list.
- The printer also has no rule for breaking a long method or `+` chain yet, so an overlong chain breaks the innermost argument list instead, which is the wrong place.
- We're also building a diff-size benchmark against the same edits in TypeScript, so this pillar is a number you can check.

Code examples on this page are checked against Glyph 0.1.153 on every build. Last changed 21 Nov 2026.
