<!-- Markdown copy of https://glyphlang.io/answers/ask-the-compiler/, generated from the same source as the page. Last changed 2026-11-19. -->

22 · Ask the compiler

# Can my agent ask the compiler what a symbol is before it writes code?

In one call. `glyph_symbol` takes the `module::name` a diagnostic already printed, or a position in a file, and answers with a record's fields and their types, a union's variants with their payloads *and the syntax that constructs each*, a callable's parameters and return, an interface's members, and whether a `match` over it must be exhaustive.

## Why it matters

An agent asked to handle a new case in a union it didn't declare has to know what the variants carry. Reading the declaration means finding the file first. Symbol search used to give it a name and a byte range, and a variant listing used to give it `["Pending", "Paid", "Cancelled"]`. From that an agent writes `Paid("tx_1")`, which is not the shape, and finds out from a diagnostic. The compiler had the payload the whole time. The tool dropped it one line before the reply.

That's the pattern the agent surface was built to remove: facts the compiler already computes and memoizes, that no tool hands over. An audit of a three-module project found hover answering two of fourteen positions and every union arriving without its payloads.

## See it

A three-file project with `OrderStatus` declared in `orders.glyph`. Asked from `main.glyph`, which doesn't import it:

glyph query symbol --path src/main.glyph --entity orders::OrderStatus

```
{
  "entity": "orders::OrderStatus",
  "kind": "union",
  "pub": true,
  "type": "Pending | Paid({ transaction_id: string }) | Cancelled",
  "exhaustive_match": true,
  "variants": [
    { "name": "Pending",   "payload": null, "construct": "Pending",
      "payload_absent": "`Pending` is declared with no payload." },
    { "name": "Paid",      "payload": "{ transaction_id: string }",
      "construct": "Paid({ transaction_id: string })" },
    { "name": "Cancelled", "payload": null, "construct": "Cancelled",
      "payload_absent": "`Cancelled` is declared with no payload." }
  ],
  "fields": null,
  "fields_absent": "`orders::OrderStatus` is a union and declares no record fields.",
  "path": "src/orders.glyph",
  "range": { "start": { "line": 2, "character": 4 },
             "end":   { "line": 5, "character": 13 } },
  "origin": "glyph",
  "origin_detail": "`orders::OrderStatus` is declared in src/orders.glyph, a Glyph
    module this project holds, and the compiler parsed and resolved that declaration"
}
```

Trimmed: the answer also carries `generics`, `parameters`, `returns`, `members`, `literals`, `owner`, `async` and `examples`. Every one of them is a pair. A key is null only alongside a `<key>_absent` sentence saying why, so an agent never has to guess whether a missing answer means the symbol has none or the tool didn't look. An unannotated `const` reports no type on that rule: the checker deliberately doesn't infer a declaration's type from its initializer, so printing the initializer's type would tell a caller that uses of the name are checked against something that checks nothing.

## The second question an agent asks

Having the shape, the next thing it wants to know is whether the value it holds can go where it's about to put it. `glyph_assignable` asks the checker's own comparison. Each side is a `module::name` or a Glyph type spelling, read in the scope of the file you name, and the verdict says which of three things happened.

--from 'Nullable<int>' --to int

```
{
  "verdict": "WILL_FAIL",
  "because": "the checker's own comparison
    refuses a `Nullable<number>` value where a
    `number` is declared, so every site that
    writes this pairing is a diagnostic",
  "diagnostics": [
    { "code": "E0204",
      "at": "an annotated `let` or `const`,
             and a `return`" },
    { "code": "E0211", "at": "a call argument" }
  ],
  "from": { "asked": "Nullable<int>",
            "form": "spelling",
            "read_as": "Nullable<number>" },
  "to":   { "asked": "int", "form": "spelling",
            "read_as": "number" }
}
```

--from 'Option<int>' --to 'Option<int>'

```
{
  "verdict": "COMPATIBLE",
  "because": "two generic applications are
    compared by arity, by base and by argument,
    and every one of those pairings is itself
    accepted, so the checker accepts a
    `Option<number>` value where a
    `Option<number>` is declared. This is a rule
    accepting rather than a rule staying silent",
  "diagnostics": null,
  "diagnostics_absent": "the checker refuses
    nothing for this pairing, so no code is
    raised for it anywhere"
}
```

The word is `COMPATIBLE`, not `SAFE`, and the difference is worth a sentence. `SAFE` is `glyph_impact`'s word, and it means a particular site stays correct after a particular edit. This answer is about two types with no site and no edit in the question. The third verdict is `UNDETERMINED`, claimed whenever no rule covers the pairing: two function types come back that way, because the relation underneath compares the returns and says nothing about the parameters. Its silence there is not an acceptance, and the tool won't report it as one.

## The other direction: what does this depend on, and what does that module give me

`glyph_references` answers who uses a symbol. `glyph_dependencies` is the same tables read from the other end: what one declaration depends on, every declaration its own source names, split into `CALLS`, `REFERENCES` and `FIELD_ACCESS`, each edge carrying both ends and where it sits. `glyph_exports` answers what a module makes visible to an importer, from the compiler's own export query rather than from a scan: every `pub` declaration and every variant a `pub` union hoists, each as an entry with its identity, kind and signature. An agent about to write an import can ask what is on the other side instead of opening the file.

Building the mirror of a relation is what caught a bug in the original. Every edge carries a provenance, and `PROVED` is meant to say the compiler read both ends. It was answering `PROVED` for an edge into a module that does not parse, where nothing had been read. It answers `UNDETERMINED` with the reason now, and since `glyph_references` and `glyph_impact` call the same function, both were wrong the same way and both are fixed.

## Eleven tools, and no client required

`glyph mcp` serves eleven: `glyph_symbol`, `glyph_impact`, `glyph_assignable`, `glyph_diagnostics`, `glyph_hover`, `glyph_definition`, `glyph_references`, `glyph_variants`, `glyph_symbols`, `glyph_dependencies` and `glyph_exports`. Each has a `glyph query` verb taking the same arguments as flags and printing the same JSON, routed through the same entry point a client reaches, so an agent that only runs shell commands asks the same questions. `glyph_diagnostics` checks a file inside its project and returns the diagnostic `glyph check --json` prints, from the same Rust type, with a test that runs both surfaces on six projects and compares the JSON key for key. Full details on the [MCP page](https://glyphlang.io/mcp/).

All of that is asked before an edit. After one, the compiler answers in the other direction: `glyph check --agent` prints each diagnostic with the invariants a repairing edit has to keep and the `glyph_symbol` description of every symbol it names, so the shape an agent needs arrives with the error that asked for it. That has [a page of its own](https://glyphlang.io/answers/repair-protocol/).

## Where it stands

### Shipping today

Eleven read tools over MCP, each with a `glyph query` verb. `glyph_symbol` answers by identity or by position, and every fact it doesn't hold is null beside the sentence saying why. The identity reaches the stdlib: the prelude `Result` is `std/result::Result`, which is the module the resolver registers it under and the module the emitter writes the import from, so `glyph query symbol --entity std/result::Result` answers with `Ok(T)` and `Err(E)`, `glyph query exports --module std/result` lists the surface, and an `E0200` over it carries that identity in `cause`. There is no Glyph source behind it, so `path` and `range` are null with that reason rather than missing. Hover answers all fourteen of the positions an audit probed on a three-module project, against two before this work started: a name declared in another module answers from that module's own declaration, so the import binding, an imported function at a call, an imported variant used as a value and a field read off an imported record all have a type. `glyph_dependencies` and `glyph_exports` close the set G221 asked for. `glyph_variants` carries each payload and its construction syntax; `glyph_definition` and `glyph_symbols` carry the `module::name`, so an answer chains into `glyph_impact` without re-deriving anything. `glyph_impact`'s signature-type table decides a container against a container, a structural record against a structural or declared record (local, imported, generic or a second name), a JSX element as the call it lowers to, and a string-literal union on either side with the same comparison `glyph_assignable` makes, so the two tools cannot disagree about one pairing. An argument whose pairing the relation reads in full is `WILL_FAIL` whichever way the comparison came out, because `change_signature_type` names no replacement type and an accepted pairing is one a replacement can be refused at; only a pairing the relation declines is `UNDETERMINED`.

### Where it stops

No tool here writes: no rename, no applied edit. The write side is `glyph fix` on the command line, and it repairs three things rather than anything you ask for: unused imports, an arm head the checker has exactly one suggestion for, and a field name one character from exactly one declared field. A stdlib export the compiler models neither a declaration nor a signature for answers `kind: null` with the reason: `std/result::all` and `std/io::println` are typed in TypeScript the compiler stages and no Glyph pass reads. `glyph_diagnostics` skips three checks `glyph check` makes: `tsc`, the `E0104` check for an import naming no module, which needs the build's view of `node_modules`, and `E0400` for a failing `@example`, which needs the emitted project run. It names all three in `not_run` instead of answering as though it had.

Code examples on this page are checked against Glyph 0.1.152 on every build. Last changed 19 Nov 2026.
