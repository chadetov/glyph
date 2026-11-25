<!-- Markdown copy of https://glyphlang.io/answers/banned-imports/, generated from the same source as the page. Last changed 2026-11-25. -->

25 · Guarded libraries

# Can I stop an agent importing the raw library when we have a wrapper for it?

Yes. List the import path under `glyph.banned_imports` in `package.json` with the module to use instead. An import of it anywhere else is error `E0115`, and its help names your wrapper. The check runs in `glyph check`, `glyph build`, the editor and the MCP server.

package.json

```
"glyph": {
  "src": "src",
  "banned_imports": {
    "axios": {
      "use": "http/client",
      "reason": "requests go through the
                 retry and auth wrapper"
    }
  }
}
```

glyph check

```
[E0115] Error: import: `axios` is banned
in this project: requests go through the
retry and auth wrapper
 3 │ import axios { default as ax }
   │        ──┬──
Help: import `http/client` instead;
`glyph query exports --module http/client`
lists what it offers.
```

The rule lives in the project's own manifest, and the error says what to write instead.

## Why it matters

Most codebases have a few libraries that are meant to be used one way. The HTTP client goes through a wrapper that retries and attaches auth. The database driver goes through the query layer. Raw file access goes through a storage module so tests can swap it.

A person learns this in their first code review. An agent doesn't. It has seen a thousand projects call `axios` directly, so that is what it writes, and the result compiles, passes the tests and skips the retry logic. A ban turns the convention into a compile error the agent reads in the same loop as any other.

## What you write

Each entry maps a banned import path to the replacement, with an optional reason and an optional list of modules that may still use it.

package.json

```
"banned_imports": {
  "axios":  { "use": "http/client", "reason": "requests go through the retry and auth wrapper" },
  "std/fs": { "use": "storage", "allow_in": ["storage", "migrations/"] }
}
```

- **A key covers what is under it** `axios` bans `axios/lib/core` too, and does not ban `axios-retry`. A key can be an npm package, a `std/` module or a module of your own.
- **The wrapper may use it** The module `use` names can always import the banned path. `allow_in` adds others, and an entry ending in `/` covers a directory.
- **The help says what to do** It names the replacement and the query that lists what it exports. It doesn't mention `allow_in`, because telling the agent how to lift the ban would defeat it.

## The escape hatch is covered

`extern_ts` lets a module drop into raw TypeScript, which makes it the obvious way around a ban. The same specifier quoted inside an `extern_ts` body is `E0115` as well:

main.glyph

```
let ax = extern_ts("require('axios')")   // E0115
```

When the package isn't installed either, `E0115` is the only error. The usual "unresolved import" would tell the agent to install it, which is the wrong advice.

## Typos fail loudly

The `glyph` key is a closed schema. An unknown key or field is error `E0116`, reported in `package.json` at the key with its JSON path, so `banned_import` or `alow_in` can't switch a policy off without anyone noticing.

glyph check

```
[E0116] Error: config: `glyph.banned_imports.axios.alow_in` is not a field
Glyph reads (known: `use`, `reason`, `allow_in`); did you mean `allow_in`?
```

A `use` that names no module, a `use` that is itself banned, and an `allow_in` entry naming a module that doesn't exist are all `E0116` too. The full reference is in the [configuration guide](https://github.com/chadetov/glyph/blob/main/docs/guide/configuration.md).

## Where it stands

### Shipping today

- `E0115` and `E0116` in `glyph check`, `glyph build`, the language server and the MCP `glyph_diagnostics` tool.
- The editor and the MCP server re-read `package.json` when it changes, with no restart.
- A nested project with its own marker is governed by its own key. Bans are not inherited.

### Where it stops

- A specifier computed inside raw TypeScript (`extern_ts("require(name)")`) isn't seen. `grep extern_ts` is how to review those.
- Hand-written `.ts` files in a hybrid project, and npm code importing the package itself, are outside what Glyph compiles.
- A `package.json` that doesn't parse isn't a project marker, so its bans don't apply. Making that an error is filed.
- No query lists a project's bans yet; an agent reads `package.json` or meets them as `E0115`. A query verb is filed.

Code examples on this page are checked against Glyph 0.1.188 on every build. Last changed 25 Nov 2026.
