# auth_api

## What it is

A signup and login HTTP API with sessions, over nine modules. Passwords are
salted and stretched with an iterated HMAC-SHA256, access tokens are signed and
stateless, refresh tokens are stored hashed and rotate on use, and every failure
is one variant of a 21-variant `AuthError` that the router turns into a status
code in a single `match`.

## Running it

```sh
glyph run examples/apps/auth_api          # a scripted transcript, then exit
glyph run examples/apps/auth_api serve    # a real server on :8141
```

## What it changed in Glyph

Shipped **0.1.59** and **0.1.60**, and it paid for the gap in shipped code.

**G79: a boundary rejection said which field was wrong but never which rule.**
Every failing field pushed the same string whatever went wrong. Worse, the
object test was `typeof value !== "object" || value === null`, which an array
passes, so a record with no required fields accepted an array outright. The
app's cost was two record types and two `parse` calls per password-bearing
payload to recover one bit, and before that workaround a signup with no password
field at all answered `weak_password`. The fix added `Issue.code` and messages
that name the rule: `expected Password (string where value.length >= 8)`.

**G80: a module-local type named `Issue` shadowed the prelude one** and broke
every descriptor in that module. Now E0110 at the declaration.

It left a stdlib bill that later releases paid: `std/crypto` had no KDF and no
timing-safe compare, `std/http` could not see the client address, headers were
`Record<string, string>` so two `Set-Cookie` could not coexist, and there was no
`bytes` type, which made a standards-compliant JWT inexpressible.

## What it exercises

A `where` refinement (`Password = string where value.length >= 8 && <= 128`)
used as a plain field, a string-literal union `Role`, one 21-variant error union
matched once, and `async`/`await` on `listen`. Ten `@example` rows.
