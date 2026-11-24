# Configuration: the `glyph` key

Glyph has no config file of its own. Its settings live under a `"glyph"` key in
the project's `package.json`, next to everything else npm already reads.

The key's presence is what marks a project root. Import paths resolve from the
project's source directory, and when one project sits inside another, the
nearest marker above a file is the one that governs it. Nothing is inherited
from an outer project.

```json
{
  "name": "orders",
  "type": "module",
  "glyph": {
    "src": "src",
    "banned_imports": {
      "axios":  { "use": "http/client", "reason": "requests go through the retry and auth wrapper" },
      "std/fs": { "use": "storage", "allow_in": ["storage", "migrations/"] }
    }
  }
}
```

## The keys

| key | what it does |
|---|---|
| `src` | The source directory, relative to `package.json`. Without it, `src/` is used when it exists, else the package directory. `glyph init` writes `"src": "src"` |
| `banned_imports` | Import paths the project's code may not use, each naming the import to write instead. See below |
| `imports`, `audit` | The review-currency policy `glyph publish` enforces for third-party imports ([distribution](distribution.md)) |

The key is a closed schema. A key Glyph does not read, or a field inside a ban
it does not read, is error `E0116`, reported in `package.json` at the key itself
with its JSON path:

```
[E0116] Error: config: `glyph.banned_imports.axios.alow_in` is not a field Glyph reads (known: `use`, `reason`, `allow_in`); did you mean `allow_in`?
```

That is deliberate. If an unknown key were ignored, a typo like
`banned_import` would quietly switch the whole policy off and nobody would find
out until the import it was meant to stop had shipped. `src` with a value that
is not a string is `E0116` too.

## Banning an import

A project usually has a few libraries it wants used one way only: an HTTP
client behind a wrapper that adds retries and auth headers, raw file access
behind a storage module, a database driver behind the query layer. A person
learns that from a code review. An agent writing code in the project does not,
and reaches for the library it already knows. `banned_imports` writes the rule
down where the compiler can enforce it.

Each entry maps a banned path to an object:

- **The key** is an import path as written after `import`: an npm package
  (`axios`, `@aws-sdk/client-s3`), a standard-library module (`std/fs`), or a
  module of the project (`db/raw`). It bans that path and every path under it,
  so `axios` bans `axios/lib/core`, and does not ban `axios-retry`. A scoped
  package is written `@scope/pkg`, and a Node builtin by its bare name (`fs`),
  the way Glyph imports it. A key can't be empty, end in `/`, or contain a space
  or `*`.
- **`use`** (required) is the import to write instead. A project module or a
  `std/` module must exist. An npm package must be listed in this
  `package.json`'s dependencies; it does not have to be installed. `use` can't
  itself be banned.
- **`reason`** (optional) is one line, quoted in the error.
- **`allow_in`** (optional) lists project modules that may import the banned
  path anyway, written by their module path (`storage`). An entry ending in `/`
  covers every module in that directory and below it (`migrations/`). Each entry
  must name a module or directory that exists. The `use` module is always
  allowed, so it doesn't need listing.

An import of a banned path from anywhere else is `E0115`:

```
[E0115] Error: import: `axios` is banned in this project: requests go through the retry and auth wrapper
   ╭─[ main:3:8 ]
 3 │ import axios { default as ax }
   │        ──┬──
   │          ╰──── `axios` is banned in this project: requests go through the retry and auth wrapper
   │ Help: import `http/client` instead; `glyph query exports --module http/client` lists what it offers. The ban is set in `glyph.banned_imports` in package.json.
```

A subpath names the key that bans it: `` `axios/lib/core` is banned in this
project (it is under `axios`) ``. When the package isn't installed either, the
ban is the only error you get, because "install axios" would be the wrong
advice.

The help names the replacement and where the ban is set, and when the
replacement is a project or `std/` module it adds the `glyph query exports`
line that lists what it offers. It doesn't mention
`allow_in`: the error is read by whoever wrote the import, often an agent, and
telling it how to lift the ban would defeat the ban. Whoever owns
`package.json` already knows.

`glyph check`, `glyph build`, the editor's diagnostics and the MCP server's
`glyph_diagnostics` tool all report both codes. The editor and the MCP server
re-read `package.json` when it changes, so an edit to a ban shows up without a
restart.

### `extern_ts` is covered

`extern_ts` is the escape hatch for raw TypeScript, which makes it the obvious
way around a ban. A banned specifier quoted inside an `extern_ts` body in a
module the ban applies to is `E0115` as well:

```
let client = extern_ts("require('axios')")   // E0115
```

Single quotes, double quotes and a backtick string without `${` are all read,
in both the expression and the type form. A `node:fs` there counts as `fs`.

### What a ban can't see

- A specifier computed inside raw TypeScript (`extern_ts("require(name)")`).
  `grep extern_ts` is the review route for those.
- Hand-written `.ts` files in a hybrid project. They are compiled by `tsc`, not
  by Glyph.
- npm code importing the package itself. A ban is about this project's source.
- A `package.json` that doesn't parse. It isn't a project marker at all, so its
  bans go with it.

There is no `glyph fix` repair for `E0115`. The replacement usually has a
different API, and choosing how to call it is the point of the ban.
