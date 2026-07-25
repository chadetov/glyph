# Effective Glyph

Once the syntax is familiar, these are the conventions that make a Glyph codebase
read the way the language intends. They are not rules the compiler enforces; they
are what fluent code looks like.

## Project structure

- One concept per file; the file path is the module path (`billing/invoice.glyph`
  is `module billing/invoice`). There are no barrel files, so the import path is
  the real location.
- Keep `main` thin. It parses arguments and calls into modules; the logic lives
  in `pub` functions elsewhere. A `main` that is one screen of orchestration is a
  good sign.
- Group by domain, not by kind. `billing/`, `auth/`, `feed/`, not
  `types/`, `helpers/`, `utils/`. A reader looking for invoicing finds it in one
  place.

## Visibility

- Export the surface, hide the rest. Mark `pub` only what other modules use, so
  `grep '^pub'` is the module's real API. A helper stays private, and if you
  later need it elsewhere, exposing it is a deliberate one-word change.

## Naming

- Functions are verbs (`load_user`, `parse_body`, `render`), types are nouns
  (`User`, `Invoice`, `FeedError`), tagged-union variants are the case names
  (`Pending`, `Paid`, `NotFound`).
- Snake_case for values and functions, PascalCase for types and variants. The
  formatter won't change your casing, so pick it deliberately.

## Choosing a construct

- Reach for a **tagged union** whenever a value is "one of a few shapes,"
  especially states and results. Reach for a **record** for "a bundle of fields
  that are all present at once."
- Use a **string-literal union** (`"free" | "pro"`) for a closed set of string
  values you validate at the boundary; use a **tagged union** when each case
  carries different data.
- Use an **`interface`** only to constrain a generic. If you're not writing
  `<T: Bound>`, you want a `type` record, not an interface.
- Use **`int`** for a value that must be a whole number at a boundary (an id, a
  count, a page size); use **`number`** when fractions are legal.

## Errors

- Put the failure in the type: `Result<User, LoadError>`, where `LoadError` is a
  tagged union of the real ways it fails, not a bare `string`. The caller can
  then `match` and recover per case.
- Propagate with `?` when the caller can't do anything but pass it up; `match`
  when it can recover. Don't `match` just to re-wrap and re-throw, that's what
  `?` is for.

## Validation

- Validate untrusted input once, at the edge, with `T.parse`, then pass the typed
  value inward. Everything past the boundary can trust the type.
- Mark a generated wire record `@open` when the upstream may add fields; keep
  records you own strict so extra keys are caught.

## Cleanup and concurrency

- Pair an acquisition with a `defer` release on the next line, so the cleanup is
  visible at the point it's set up and runs on every exit.
- Use `std/task.all` to join concurrent I/O; keep the task thunks small and let
  the results come back as a typed array.

The through-line: write the version where the compiler and a plain `grep` can
both see what you meant. That is what "idiomatic" buys you here.
