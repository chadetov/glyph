<!-- Markdown copy of https://glyphlang.io/answers/parse-errors/, generated from the same source as the page. Last changed 2026-11-20. -->

27 · Parse failures

# When a parse fails, do I find out why?

Yes. Every issue names the field, says **which rule the value broke**, and carries a `code` you can branch on. A field constrained by a `where` predicate reports the predicate itself, so the rejection greps back to the line that imposed it.

## Why it matters

A validator that says "this payload is bad" has done half the job. The handler still has to decide: is this a client that forgot a field, or a client that sent a password four characters long? One is a 400, the other is a 422, and if the validator won't say, you end up parsing the same body twice with two different types to recover a bit the validator already had.

That is exactly what happened building an auth API in Glyph. A signup with no `password` key at all came back as `weak_password`, because absent and too-short produced the same string.

## See it

One type, one constraint:

signup.glyph

```
pub type Password = string where value.length >= 8

pub type Signup = {
  email: string,
  password: Password,
}
```

Four bad payloads, four different answers. Each is one `Issue`, shown as `code | path | message`:

Signup.parse(...)

```
// { "email": "user@example.com" }
missing     ["password"]  field `password` is required

// { "email": "user@example.com", "password": "short" }
refinement  ["password"]  expected Password (string where value.length >= 8)

// { "email": 42, "password": "longenough" }
type        ["email"]     field `email` must be string

// [1, 2, 3]
type        []            expected Signup (an object), got an array
```

The second one is the point. `value.length >= 8` is written once, at the type, and the rejection quotes it verbatim. Paste that string out of an HTTP 422 body into `grep` and you land on the declaration that produced it. Nobody has to keep a copy of the rule in the error-message layer, which is how the two drift apart.

The fourth one used to be worse than a bad message. An array is an object to `typeof`, so a record with no required fields accepted one outright, and a posted `[1, 2, 3]` came back as one confusing issue per declared field, none of them mentioning that you had sent a list.

## Nested types answer for themselves

A field whose type has its own descriptor is validated by that type's `parse`, and its issues come back with the field name on the front of the path. So a `Password` nested two levels down reports `["body", "password"]` and still carries its own message. You get the location and the reason from the same issue, without re-running anything.

## The code, not the string

`Issue` carries an optional `code`: `"missing"`, `"type"`, `"refinement"`, `"unexpected"`, or `"depth"`. Branch on that. Matching on message text is the fragility Glyph exists to remove, and the message is for the human reading the response body. `"depth"` is the answer for a body nested more than 1000 objects and arrays deep (`MAX_DEPTH` in `std/schema`): one issue at the field where it happened, where before 0.1.150 the check threw `RangeError: Maximum call stack size exceeded` and took the handler with it.

A 400/422 split that does not read the message

```
fn status_for(issues: Array<Issue>) -> number {
  let first = issues[0]
  return match first.code == "refinement" {
    true => 422,
    false => 400,
  }
}
```

`code` is optional, so an `Issue` you build by hand, and every consumer written before it existed, still compiles unchanged.

## Where it stands

### Shipping today

Records and `where` types report the failing rule, not just the failing field: `"missing"` for an absent required field, `"type"` for the wrong shape (including a non-object or an array), `"refinement"` for a value that passed its base type and failed the predicate, `"unexpected"` for a key the type does not declare, and `"depth"` for a value nested more than 1000 deep. A refinement quotes its predicate. A field whose type has a descriptor delegates to it, so nested issues arrive with the full path. Optional fields (`f?: T`) are never reported as missing.

### Getting sharper

Two edges. A field typed by something with no descriptor of its own, an unconstrained type parameter or a type imported from a hand-written `.d.ts`, still gets the flat check and a `"type"` issue; the compiler does not synthesize a descriptor for those, and running `glyph gen dts` on the declaration is what gives them one. A module that declares its own type called `Issue` used to shadow the prelude one inside the emitted file and break every descriptor in that module; since 0.1.60 that declaration is `E0110` at the line that names the type, so you find out where the problem is instead of reading a `tsc` error about generated code.

Code examples on this page are checked against Glyph 0.1.152 on every build. Last changed 20 Nov 2026.
