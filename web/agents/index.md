<!-- Markdown copy of https://glyphlang.io/agents/, generated from the same source as the page. Last changed 2026-11-19. -->

For agents and tool builders

# The compiler's interface for agents

An agent can ask Glyph's compiler what a symbol is, what an edit will break, and why a build failed, and get JSON back. `glyph mcp` serves eleven tools over stdio, each one also a `glyph query` verb, and `glyph llms` prints the language reference offline.

## Set it up

`glyph init` writes this `.mcp.json` into a new project, and `glyph agents` writes it (with an `AGENTS.md` pointing at `glyph llms`) into an existing one. Any MCP client that reads the file launches the server; there is no port and no daemon.

.mcp.json, as glyph init writes it

```
{
  "mcpServers": {
    "glyph": {
      "command": "glyph",
      "args": ["mcp"]
    }
  }
}
```

## One call, with its real answer

A file reads a field that its record doesn't declare (`u.naem`). The agent calls `glyph_diagnostics` and gets the compiler's own diagnostic, with the fields the record does declare and the record that owns them. This is the tool's output on 0.1.149, trimmed only where marked.

request

```
{
  "method": "tools/call",
  "params": {
    "name": "glyph_diagnostics",
    "arguments": {
      "path": "src/user.glyph"
    }
  }
}
```

result

```
{
  "diagnostics": [ {
    "code": "E0210",
    "severity": "error",
    "stage": "typecheck",
    "message": "type `U` has no field
      `naem`; did you mean `name`?",
    "alternatives": ["name", "email"],
    "alternatives_kind": "fields",
    "cause": "user::U",
    "entity": "user::greet",
    "file": "user.glyph",
    "range": {
      "start": { "line": 9, "col": 10 },
      "end": { "line": 9, "col": 16 } },
    ...
  } ],
  "member": true,
  "module": "user",
  "not_run": [
    { "what": "tsc", ... },
    { "what": "E0104, ...", ... },
    { "what": "E0400, ...", ... } ],
  "path": "src/user.glyph",
  "project_root": "src",
  "unindexed": []
}
```

`not_run` names what this check did not cover, so an empty `diagnostics` list can't be mistaken for a full build. Without an MCP client, `glyph query diagnostics --path src/user.glyph` prints the same JSON. The full tool list, with every verb, is on [the MCP page](https://glyphlang.io/mcp/).

## Before an edit: what will it break?

`examples/apps/csvql` is an eleven-file CSV query engine with a four-case union, `Value`. Asked what adding a fifth case does, `glyph_impact` lists 10 `match` sites in 4 files: 8 `WILL_FAIL` and 2 `ABSORBS`, where a catch-all arm will swallow the new case and keep compiling. Adding the case for real gives exactly 8 `E0200` errors, at those 8 sites. The 2 absorbing sites stay silent, which is why the answer names them.

the same question on the command line

```
glyph query impact --path value.glyph --entity value::Value \
  --change add_variant --variant Blob
```

## After a red build: what does the fix have to keep?

`glyph check --agent` prints `check --json`'s diagnostics with two more keys on each. `constraints` lists what a repairing edit must not break; for the typo above it reads "the record's fields are closed: `user::U` declares `name`, `email` and no others". `symbols` carries the `glyph_symbol` answer for every symbol the diagnostic names, so the next edit needs no second call. Where the fix is fully determined, `glyph fix` writes it.

## Reading the language offline

`glyph llms` prints the language reference, and `/llms.txt` on this site is the same document. `glyph llms --json` is that knowledge as data, read from the compiler's own tables: 77 diagnostic codes, 24 prelude names, 40 stdlib modules with 340 exports, 49 language decisions, and the eleven tools. `glyph llms --negative` returns 117 wrong programs in 0.1.149 (139 from the next release) with the code each one draws, compiled when you ask. `scripts/check_llms_sync.py` fails when the diagnostic and reserved-word tables in the reference drift from the compiler's own.

offline entry points

```
glyph llms                  # the reference, as prose
glyph llms --json           # the same, as data from the compiler's tables
glyph llms --negative E0200 # wrong programs that draw E0200
glyph --explain E0200       # the long explanation of one code
```

## Each piece in depth

- [Start here The MCP server Setup, the eleven tools with their arguments, the glyph query verbs, and what the server doesn't do yet. Set up the server](https://glyphlang.io/mcp/)
- [Can my agent ask the compiler what a symbol is? glyph_symbol: a union's cases with the syntax that constructs each.](https://glyphlang.io/answers/ask-the-compiler/)
- [Can the compiler say what a change breaks first? Six change kinds, one verdict per site, and the csvql run above in full.](https://glyphlang.io/answers/impact/)
- [Does a failed build tell my agent how to fix it? check --agent, glyph fix, and a walkthrough with real output.](https://glyphlang.io/answers/repair-protocol/)
- [Is the agent's documentation generated? What glyph llms emits, where each number comes from, and what is still missing.](https://glyphlang.io/answers/generated-docs/)
- [Can an AI write a language it has barely seen? What carries over from TypeScript, and how the compiler covers the rest.](https://glyphlang.io/answers/ai-authoring/)
