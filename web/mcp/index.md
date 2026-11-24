<!-- Markdown copy of https://glyphlang.io/mcp/, generated from the same source as the page. Last changed 2026-11-19. -->

For agents · Model Context Protocol

# Wire your agent into Glyph's compiler

`glyph mcp` runs a **Model Context Protocol server** with eleven tools over your project: what a symbol is, what breaks if you change it, whether a value of one type can go where another is declared, what a declaration depends on, what a module exports, diagnostics checked inside the project, the type at a cursor, where a name is defined, every reference to it, every `match` site over a union, and symbol search. Each one is also a `glyph query` verb, so an agent with no MCP client asks the same questions and gets the same bytes.

## Why an agent wants this

An agent editing a codebase usually works blind. It reads files, guesses at types, and finds out whether it broke something only after a build. Glyph already answers those questions for an editor. `glyph mcp` gives an agent the same answers straight from the compiler, without scraping build output or reading every file to find who calls what. Two of the tools answer questions an editor never asks: `glyph_symbol` returns a union's variants with the syntax that constructs each, which is what an agent needs before it writes the `match`, and `glyph_impact` answers what an edit breaks before the edit exists.

## Set it up

Any MCP client can launch it. Point the client at the `glyph` binary with the `mcp` subcommand, and pass your project root as an argument or run it from the project directory. It speaks MCP over stdio, so there's no port and no daemon.

mcp client config (generic)

```
{
  "mcpServers": {
    "glyph": {
      "command": "glyph",
      "args": ["mcp", "/path/to/your/project"]
    }
  }
}
```

On first connect the client calls `initialize` and `tools/list`; the server reports the eleven tools below. Positions you send follow the LSP convention: a 0-based `line` and a 0-based `character` in UTF-16 code units. Diagnostic ranges come back the way `glyph check --json` prints them, with a 1-based `line` and `col` plus the byte `offset`.

Each tool's description carries the contract you need at the moment of the call, and the full manual lives in `glyph llms --json` under `tools`, keyed by tool name, with each tool's `glyph query` verb beside it. That reply used to carry every manual: eleven tools, 31,446 bytes of description in a 45,509-byte reply. It is 8,627 in a 20,386-byte reply now, and nothing was deleted. Every session pays for `tools/list` on connect and pays for the manual only when it asks. [That document](https://glyphlang.io/answers/generated-docs/) is generated from the compiler's own tables, and a test fails the build if it names a tool this server does not serve.

## The eleven tools

tools exposed by glyph mcp

```
// everything the compiler holds about one symbol: kind, identity,
// visibility, a record's fields, a union's variants with payloads and
// construction syntax, parameters and return, interface members,
// whether a `match` over it must be exhaustive, its `@example`.
// The identity reaches the stdlib: std/result::Result answers with
// Ok(T) and Err(E), and `path` is null beside the reason why
glyph_symbol(entity)                       // or (path, line, character)

// what breaks if you make one named change to one declaration:
// add_variant, remove_variant, rename, change_arity,
// change_signature_type, remove. One verdict per site
glyph_impact(entity, change)

// can a value of `from` go where a `to` is declared? Asked of the
// checker's own comparison. Each side is a `module::name` or a
// Glyph type spelling read in the scope of `path`
glyph_assignable(path, from, to)

// every diagnostic for one file, checked inside its project, in the
// shape `glyph check --json` prints, plus what was not checked
glyph_diagnostics(path)

// the type at a position: an expression, a declaration name, a
// parameter, an annotation, a variant, or a binding
glyph_hover(path, line, character)

// where the name is defined, following imports across modules,
// with the `module::name` so the answer chains into glyph_impact
glyph_definition(path, line, character)

// every edge into a symbol across the project, split by relation
// (CALLS, REFERENCES, GENERATED_FROM), each with its provenance
glyph_references(path, name)               // or by position

// every `match` site over one tagged union and which variants each
// site's arms name, with the union's own variants
glyph_variants(path, name)

// search top-level declarations and union variants by name, each
// entry with its identity, `pub`, kind and one-line signature
glyph_symbols(query)

// what one declaration depends on: every declaration its own source
// names, split by relation (CALLS, REFERENCES, FIELD_ACCESS), each
// edge with both ends and its provenance. The mirror of references
glyph_dependencies(entity)

// what one module makes visible to an importer: every `pub`
// declaration and every variant a `pub` union hoists, each with
// its identity, kind and signature
glyph_exports(module)
```

## See it

A `tools/call` for `glyph_diagnostics` on a file with a field typo returns the compiler's own diagnostic, the same Rust type `glyph check --json` serializes, inside an envelope that also says what the check did not cover.

request

```
{
  "method": "tools/call",
  "params": {
    "name": "glyph_diagnostics",
    "arguments": { "path": "src/user.glyph" }
  }
}
```

result (the tool's JSON content)

```
{
  "path": "src/user.glyph",
  "module": "user",
  "project_root": "src",
  "member": true,
  "diagnostics": [
    {
      "code": "E0210",
      "severity": "error",
      "stage": "typecheck",
      "message": "type `U` has no field `naem`; did you mean `name`?",
      "help": "Check the field name for a typo,
        or add the field to the type.",
      "entity": "user::greet",
      "file": "user",
      "range": {
        "start": { "line": 9, "col": 10, "offset": 99 },
        "end":   { "line": 9, "col": 16, "offset": 105 }
      },
      "union": null,
      "missing_variants": null
    }
  ],
  "unindexed": [],
  "not_run": [ ... ]
}
```

The file is checked inside its project, so a wrong field on an imported record and a `match` missing arms over an imported union are reported here the way `glyph check` reports them. What the tool could not cover is on the answer rather than left for you to assume: `member` says whether the project walk reaches the file, `unindexed` names project files that don't parse, and `not_run` names the three checks `glyph check` makes and this one doesn't: `tsc`, `E0104` for an import naming no module, which needs the build's view of `node_modules`, and `E0400` for a failing `@example`, which is decided by running the emitted project. A test runs both surfaces on six projects and compares the JSON key for key, so the two can't drift.

`glyph_references` is workspace-wide: ask about a symbol and you get its declaration, every use, and each importing module's `import` binding, across every file. It uses the same cross-file identity the editor's rename does, so the answer covers the whole project, not just the file you asked from.

## No MCP client? Same questions on the command line

Every tool has a `glyph query` verb taking the tool's own arguments as flags and printing the tool's JSON on stdout. An answered question exits 0; a refused one writes the reason to stderr and exits 2. The verbs route through the same entry point an MCP client reaches, and an integration test asks both surfaces the same question and compares the answers as text, so a verb that reformatted or summarized would fail it.

the same eleven questions, without a client

```
glyph query symbol       --path src/main.glyph --entity orders::OrderStatus
glyph query impact       --entity orders::OrderStatus --change add_variant --variant Refunded
glyph query assignable   --path src/main.glyph --from 'Nullable<int>' --to int
glyph query diagnostics  --path src/main.glyph
glyph query hover        --path src/main.glyph --line 9 --character 6
glyph query definition   --path src/main.glyph --line 9 --character 6
glyph query references   --path src/orders.glyph --name create
glyph query variants     --path src/orders.glyph --name OrderStatus
glyph query symbols      --query order
glyph query dependencies --path src/main.glyph --entity main::build
glyph query exports      --path src/main.glyph --module orders

// the same two, asked about a module the compiler carries
glyph query symbol       --path src/main.glyph --entity std/result::Result
glyph query exports      --path src/main.glyph --module std/result
```

## When the build is already red

The tools answer questions asked before an edit. After one, the thing to read is `glyph check --agent`, which prints `check --json`'s diagnostics with two more keys on each: `constraints`, the invariants a repairing edit has to keep, and `symbols`, the `glyph_symbol` answer for every symbol the diagnostic names, so the next edit needs no second call. On a `match` missing arms that means the union's variants with their payloads arrive with the error that asked for them, along with the sentence saying an `else` arm would forfeit the guarantee the diagnostic exists to protect. Where the repair is fully determined, `glyph fix` writes it and says what it declined. There is a [walkthrough of the whole loop](https://glyphlang.io/answers/repair-protocol/) with real output.

## Where it stands

### Shipping today

A `glyph mcp [root]` server over stdio with eleven tools: `glyph_symbol`, `glyph_impact`, `glyph_assignable`, `glyph_diagnostics`, `glyph_hover`, `glyph_definition`, `glyph_references` (workspace-wide), `glyph_variants`, `glyph_symbols`, `glyph_dependencies` and `glyph_exports`. Each has a `glyph query` verb printing the same JSON. Answers come from the compiler's own queries, and every fact a tool does not hold is null beside a `<key>_absent` sentence saying why. Hover answers at a name declared in another module, so an imported function at its call site and a field read off an imported record both have a type. An entity identity reaches the stdlib: the prelude `Result` is `std/result::Result`, which is the module the resolver registers it under and the module the emitter writes the import from, so `glyph_symbol` on it answers with `Ok(T)` and `Err(E)` and `glyph_exports` on `std/result` lists the surface. There is no Glyph source behind it, so `path` and `range` are null with that reason rather than missing.

### On the way

The tools only read: you can't rename or apply an edit through MCP. The write side lives on the command line instead, as `glyph fix`. The server rescans the project on each call instead of keeping a cached index, and it skips any file that doesn't parse. The `tools/list` reply is 20,386 bytes, down from 45,509, and that is still context every session pays for on connect.

Beside the tools, an agent can read the language and the diagnostic surface offline: [llms.txt](https://glyphlang.io/llms.txt) is the prose, `glyph llms --json` the same knowledge as data out of the compiler's own tables, and `glyph build --json` the batch path for coded diagnostics across a whole tree.
