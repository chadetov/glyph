# auth_api

## What it is

A signup and login HTTP API with sessions, over nine modules. Passwords are
salted and stretched with an iterated HMAC-SHA256, access tokens are signed and
stateless, refresh tokens are stored hashed and rotate on use, and every failure
is one variant of a 16-variant `AuthError` that the router turns into a status
code in a single `match`.

## Running it

```sh
glyph run examples/apps/auth_api          # a scripted transcript, then exit
glyph run examples/apps/auth_api serve    # a real server on :8141
```

## What it exercises

A `where` refinement (`Password = string where value.length >= 8 && <= 128`)
used as a plain field, a string-literal union `Role`, one 16-variant error union
matched once, and `async`/`await` on `listen`. Four `@example` rows.

## What it found, and what happened

Building this app surfaced two gaps in the record-validation boundary. Both are
closed; the app's own source is the proof, not just this paragraph.

**G79 (closed in 0.1.59): a boundary rejection said which field was wrong but
never which rule, and a record accepted an array.** Every failing field in a
`parse` call pushed the same string regardless of whether the field was absent,
present with the wrong type, or present but failing its `where` predicate,
so a handler could not answer a 400 versus a 422 without re-deriving what the
validator already knew. The object test was also `typeof value !== "object" ||
value === null`, which an array passes, so `Empty.parse([])` returned `Ok`. The
app paid for this directly: `model.glyph` carried `SignupBodyLoose` and
`ChangePasswordBodyLoose`, a second, unrefined copy of each password-bearing
body shape, and `routes.glyph` ran the strict parse, and on failure ran the
loose parse a second time, purely to tell "missing" apart from "too short". A
signup with no password field at all came back as `weak_password`, which was
wrong.

The fix added `Issue.code` (`"missing" | "type" | "refinement" |
"unexpected"`) and rewrote the refinement message to name the predicate it
checks: `expected Password (string where value.length >= 8)`. `model.glyph` no
longer has the loose types; `routes.glyph` parses each body once and branches
on `code` in `weak_or_malformed` (see `is_refinement` and `weak_or_malformed`).
Running the transcript today, `POST /signup short password` answers **422**
`weak_password` with the refinement's own message, and `POST /signup password
42` (a password that is present but the wrong type) answers **400**
`bad_request`, two different statuses chosen off `code` rather than off message
text.

**G80 (closed in 0.1.60): a module-local type named `Issue` shadowed the
prelude one and broke every descriptor in that module.** A descriptor's `parse`
annotates its error array as `Issue[]`; if a module declared its own `Issue`,
every generated descriptor in that module resolved to the wrong type and failed
`tsc` with TS2353, at a span pointing somewhere unrelated to the real cause.
This app never declared a conflicting `Issue`, so it never hit the bug, but it
depends on the fix holding: `routes.glyph` writes `fn is_refinement(i: Issue) ->
bool`, naming the prelude type directly. `Issue` is now reserved
(`PRELUDE_GLOBALS` in `crates/glyph-resolver/src/reserved.rs`), so redeclaring
it is `E0110` at the declaration instead of a delayed, misattributed type
error.

## What is deliberately still awkward

Nothing in this app is a workaround for either gap above; both are fully
closed and the code was updated to match. One thing worth naming so nobody
mistakes it for an oversight: `passwords.glyph` still derives credentials with
a hand-rolled loop over `crypto.hmac_sha256`, four thousand rounds, rather than
calling a key-derivation function from the standard library, and
`service.glyph` throttles failed logins by the normalized email address rather
than by the caller's network address. Neither is a numbered gap in the ledger,
but neither is fixed either: `std/crypto` has no PBKDF2/scrypt-equivalent
export today, and `std/http`'s `Request` still carries no client address for a
handler to key a limiter on. The app's shape here is not stale; it is the
current honest answer to what the standard library provides.
