<!-- Markdown copy of https://glyphlang.io/answers/finance/, generated from the same source as the page. Last changed 2026-11-21. -->

16 · Money & safety

# Can I trust it with money, and with input I don't control?

Yes. Money is **exact**, an invariant like non-negative is a **type the boundary enforces**, and untrusted input **cannot reach a SQL or shell sink** without being sanitized. Each of these is a compile-or-parse error if you get it wrong, not a convention someone has to remember three functions away.

std/decimal

```
let price = decimal("10.50")?
let tax   = decimal("0.84")?
price.add(tax).to_string()      # "11.34", exact

decimal("2")?.div(decimal("3")?, 2)   # "0.67", half away from zero
```

the bug it removes

```
// JS number, the default everywhere:
0.1 + 0.2            // 0.30000000000000004
1_000_000_000_000.10 * 1.07   // rounding drift
9007199254740993     // === 9007199254740992

// None of these are your money anymore.
```

A financial system on IEEE-754 `number` is a bug waiting to be reported: `0.1 + 0.2` is not `0.3`, and value past 253 silently loses precision.

## Money is not a float

`std/decimal` is exact base-10 fixed-point over BigInt.

- **Exact arithmetic** Add, subtract, and multiply are exact.
- **Division** Division takes an explicit scale and rounds half away from zero.
- **Bad input** A malformed amount parses to an `Err`, never a silent `NaN`.

## Ids and counters stay exact past 253

An account id or a ledger counter that overflows the float range and rounds is a different kind of catastrophe.

`bigint` is an exact arbitrary-precision integer with `123n` literals, and a record field typed `bigint` validates `typeof === "bigint"` at the boundary, so an id arriving as a JSON `number` is rejected rather than quietly truncated.

## An invariant is a type, not a comment

"The amount is non-negative" and "the rating is 1 to 5" are the checks that get written once and forgotten later. A `where` refinement puts the invariant in the type, and the descriptor enforces it at the boundary.

refinements.glyph

```
type Amount = int where value >= 0
type Rating = int where value >= 1 && value <= 5

Amount.parse(-1)   # Err  (not just "is a number")
Rating.parse(6)    # Err
Amount.parse(3.5)  # Err  (int check too)
```

A value that fails the predicate never becomes an `Amount` in the first place.

## Injection is a compile error

The classic remote-code path is untrusted input reaching a SQL query, a shell command, or an HTML template. `std/taint` makes that a type error.

- **Two types** A value from outside is `Tainted`; a sink you want to protect takes `Trusted`.
- **One way across** The two are structurally distinct, so the only way from one to the other is an explicit `sanitize`.

the safe path compiles

```
fn run_query(sql: Trusted<string>) -> void { ... }

let input: Tainted<string> = taint(req.body)
run_query(sanitize(input, escape))   # ok
```

the injection path does not

```
module query

import std/taint { Tainted, Trusted, taint, expose }

fn run_query(sql: Trusted<string>) -> string {
  return expose(sql)
}

pub fn handle(body: string) -> string {
  let input: Tainted<string> = taint(body)
  return run_query(input)
}

// [E0211] argument type mismatch: expected
//   `taint.Trusted<string>`, found
//   `taint.Tainted<string>` (query.glyph:11)
```

Hand a sink the tainted value directly and Glyph's checker refuses it (E0211) before `tsc` runs.

## Where it stands

### Shipping today

- `std/decimal` for exact money
- `bigint` for exact large integers
- `where` refinements so an invariant is validated at the boundary
- `std/taint` so untrusted input can't reach a protected sink without an explicit `sanitize`
- Each is checked: a bad amount or id fails `parse`, a bad invariant fails `parse`, and an unsanitized value is refused by Glyph's checker.

### Where it stops

- A `where` refinement covers primitive base types today; record and cross-field invariants (`where value.paid <= value.total`) are a planned extension.
- Taint is discipline you opt into by typing a sink `Trusted`, not automatic flow analysis

  **Details**

  Taint is discipline you opt into by typing a sink `Trusted`, not automatic flow analysis: the compiler enforces the boundary you declare, it does not infer taint across the whole program for you.

Code examples on this page are checked against Glyph 0.1.158 on every build. Last changed 21 Nov 2026.
