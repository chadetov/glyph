<!-- Markdown copy of https://glyphlang.io/answers/repair-protocol/, generated from the same source as the page. Last changed 2026-11-21. -->

23 · Fixing a red build

# When the build fails, does the compiler tell my agent how to fix it?

It tells you what is wrong as data, not as a sentence to parse: the two types, the symbol at fault as a `module::name`, and the finite list of things that could legally stand where the wrong thing stands. `glyph check --agent` adds what the repairing edit must not break. Where the answer is fully determined, `glyph fix` writes it.

glyph check --agent --no-tsc src

```
{
  "code": "E0200",
  "severity": "error",
  "stage": "typecheck",
  "message": "non-exhaustive match on `OrderStatus`: missing variants
              `Paid`, `Cancelled`",
  "help": "Add an arm for each missing variant.",
  "file":   "main.glyph",
  "module": "main",
  "entity": "main::describe",
  "cause":  "orders::OrderStatus",
  "related": ["Pending", "Paid", "Cancelled"],
  "missing_variants": ["Paid", "Cancelled"],
  "expected": null, "actual": null, "alternatives": null,

  "constraints": [
    "preserve exhaustiveness: this match must name every case of
     `OrderStatus`, so add one arm for each of `Paid`, `Cancelled`.",
    "do not add an `else` arm: `OrderStatus` is declared in this project,
     and a catch-all forfeits the guarantee that adding a variant to it
     later forces this match to be updated (D9)."
  ],

  "symbols": [
    { "entity": "orders::OrderStatus", "kind": "union", "pub": true,
      "type": "Pending | Paid({ transaction_id: string }) | Cancelled",
      "exhaustive_match": true,
      "variants": [
        { "name": "Pending", "payload": null, "construct": "Pending" },
        { "name": "Paid",    "payload": "{ transaction_id: string }",
          "construct": "Paid({ transaction_id: string })" },
        { "name": "Cancelled", "payload": null, "construct": "Cancelled" }
      ] }
  ],
  "symbols_absent": [],

  "explain": { "command": "glyph --explain E0200",
               "docs": "https://github.com/chadetov/glyph/blob/main/docs/error-codes.md#e0200" }
}
```

A two-module project. `orders` declares the union; `main` writes a `match` over it and handles one case. `glyph check --agent`, trimmed to the keys that matter here.

## Why it matters

An agent's loop around a compiler is read the error, guess the edit, rebuild. The guessing step is where the damage happens, and it happens because the error is a string.

1. read the error
2. guess the edit
3. rebuild

The compiler knew all of it. It knew the union's variants and their payloads, it knew an `else` would forfeit the seal, and it dropped both one line before the reply.

**Details the two wrong edits a string invites **

An agent that reads "non-exhaustive match on `OrderStatus`" knows a match is short some arms. It does not know what the missing variants carry, so it writes `Paid("tx_1")` and finds out from the next diagnostic. Or it reads the word "exhaustive", adds an `else` arm, and the build goes green having thrown away the guarantee that the diagnostic existed to protect.

## Reading the answer

Three things are worth pulling out.

- **`cause`** `cause` is the symbol at fault, and it is a different thing from `entity`, which is the declaration the diagnostic sits in: here the edit happens in `main::describe` and the shape it has to match comes from `orders::OrderStatus`.
- **`constraints`** `constraints` are not the repair. Each one is something the compiler already enforces, written out so the edit that satisfies the diagnostic does not break the guarantee behind it.
- **`symbols`** `symbols` is the `glyph_symbol` answer for every symbol the diagnostic names, so the agent's next edit needs no second call.

Every key is always present. A fact the compiler does not hold arrives as an explicit `null`, never as a missing key, so absence has one spelling on every code and on both the CLI and the MCP surface.

**Details addresses that chain, the else constraint, symbols_absent **

Both are addresses the other tools take, so an answer chains without re-deriving anything.

The second constraint is why the key exists: an `else` arm is legal, it would turn the build green, and the constraint says what taking it costs.

A symbol the tool refuses appears in `symbols_absent` with the reason, which keeps "we did not look" apart from "we looked and it is not there".

## Where the answer is determined, the compiler writes it

`glyph fix` applies only the repairs the compiler settles on its own: unused imports, an arm head that is not a variant when the checker computed exactly one suggestion, and a field the record does not declare when exactly one declared field is a character away. The `match` above is not one of them.

glyph fix src, on the match above

```
glyph fix: removed 0 unused import(s) across 0 file(s).
glyph fix: declined E0200 in src/main.glyph: the compiler does not
           determine what a missing arm should do, so `glyph fix`
           writes none: add an arm for each of `Paid`, `Cancelled`.
           `glyph check --agent` gives the pattern each case takes.

// src/main.glyph is unchanged, and glyph check still fails with E0200.
```

The compiler knows which cases are missing and the pattern each one takes. It does not know what the program should do when an order is `Paid`, and any body it wrote would be a guess that compiles.

**History what glyph fix wrote up to 0.1.128 **

Up to 0.1.128 `glyph fix` wrote that guess: one arm per case whose body printed the case and called `process.exit(1)`. The program then passed `check`, `tsc` and `build`, and `glyph query variants` reported the match as exhaustive, so the missing case had moved from a compile error to a crash at run time with nothing left to flag it.

**Details where the arms come from, and how declines are reported **

So the rule declines, and names the cases. Everything needed to write the arms is already in the `--agent` answer above: `missing_variants`, and the `construct` spelling of each variant under `symbols`. The arms are yours or your agent's to write, the same as any other code whose behaviour the compiler cannot infer.

What `glyph fix` does write, it writes without guessing, and every rule that cannot settle an answer prints the decline and the reason on the same output as what it applied. Two fields a character away from a typo are two, not one, and it says so rather than picking.

## The code's own explanation, as data

`glyph --explain <CODE> --json` answers with the long-form explanation, the one-line `help`, the docs URL, and a wrong program from the compiler's negative corpus paired with the diagnostic this compiler draws for it.

The diagnostic is compiled when you ask, not stored, so it cannot drift from what the compiler actually says.

glyph --explain E0200 --json

```
{
  "code": "E0200",
  "title": "non-exhaustive match",
  "explanation": "A `match` over a tagged union must handle every variant.
    Unions are sealed (D9): adding a variant later forces every match to be
    updated, so a missing variant cannot silently fall through at runtime ...",
  "help": "Add an arm for each missing variant.",
  "docs": "https://github.com/chadetov/glyph/blob/main/docs/error-codes.md#e0200",
  "counter_example": {
    "name": "alias_of_local_union_not_exhaustive",
    "expected_error": "E0200",
    "files": [ { "path": "alias_of_local_union_not_exhaustive.glyph",
                 "source": "module main\n\ntype Shape =\n  | Circle\n ..." } ],
    "diagnostic": { "code": "E0200",
                    "message": "non-exhaustive match on `Shape`: missing variants `Tri`",
                    "cause": "alias_of_local_union_not_exhaustive::Shape",
                    "related": ["Circle", "Square", "Tri"],
                    "missing_variants": ["Tri"] },
    "corrected": null,
    "corrected_absent": "`tests/negative/` pairs no repaired program with a case:
      an entry is a wrong program and the code it draws. A fix invented here would
      compile nowhere and be checked by nothing."
  }
}
```

A code the corpus does not cover comes back with `counter_example: null` and a sentence saying the corpus holds no case for it, rather than a made-up example. The corpus claims what it covers and nothing about the rest.

**Details the whole corpus, through glyph llms --negative **

`glyph --explain` hands over one case per code. The whole corpus is readable too: `glyph llms --negative E0200` prints every wrong program that draws the code, each compiled when you ask, and where a second corpus pairs a program `tsc --strict` accepts with the Glyph that refuses it, that comes back beside it. With no code it lists 102 cases over 46 of the 70 codes and names the 24 that have none, so coverage is stated rather than inferred from what came back. That and the rest of what the compiler publishes about itself is [its own page](https://glyphlang.io/answers/generated-docs/).

## Where it stands

### Shipping today

- Every diagnostic carries `expected`, `actual`, `cause`, `alternatives`, `related`, `module`, `file` and `explain`, each an explicit `null` when the compiler holds no such fact

  **Details**

  Every diagnostic carries `expected`, `actual`, `cause`, `alternatives` (with `alternatives_kind` saying whether it lists `fields`, `members`, `variants`, `literals` or `exports`, set where the error is raised), `related`, `module`, `file` and `explain`, populated from the error the compiler raised rather than re-derived, and each is an explicit `null` when the compiler holds no such fact.

- `file` is a path relative to the resolution root, and the report names that root in `project_root`, so it opens.
- `glyph check --agent` adds `constraints` and `symbols` per diagnostic.
- `glyph fix` applies three rules (`E0106`, `E0210`, `E0220`) and states every decline with its reason, including every `E0200`, which it reports and never writes. E0106 E0210 E0220
- `glyph --explain <CODE> --json` returns the explanation with a compiled counter-example, and `glyph llms --negative <CODE>` returns the whole corpus of wrong programs that draw it.

### Where it stops

- `cause` on `E0211` is `null`: the symbol at fault at a call argument is a parameter, and `module::fn.param` is not something the identity vocabulary can spell, so the null is deliberate rather than an omission. E0211
- `corrected` on a counter-example is always `null`, because the negative corpus pairs no repaired program with a case, and 24 of the 70 codes have no case at all.
- `glyph fix` writes three rules and nothing else, and it writes no arm for a non-exhaustive `match`: those are left for you or your agent, with the cases and their patterns in the diagnostic.
- Everything it declines, it declines out loud.

Code examples on this page are checked against Glyph 0.1.165 on every build. Last changed 21 Nov 2026.
