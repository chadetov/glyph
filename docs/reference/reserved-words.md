# Reserved words

Every name Glyph will not let you use, and why. Three lists, three different
reasons, three different error messages.

Grep for a name here before you spend time on a confusing diagnostic. If it is
not on any of these lists, it is available.

## 1. Glyph keywords (32)

The lexer tokenizes these, so they never reach a binding position. Using one as
a name is a parse error.

```
as         async      await      break      component  const
continue   defer      else       false      fn         for
if         import     in         interface  is         let
loop       match      module     mut        new        owned
pub        record     resource   return     true       type
void       where
```

`if` is reserved but only valid in a JSX directive position: Glyph has no
`if`/`else` statement, and `match` covers the conditional (D9). `record` is
reserved for the record-type form. `new` is reserved so `new Thing()` reads as
an error rather than a call to a function you happened to name `new`.

These are the only 32. Anything not in this list lexes as an ordinary
identifier, which is why the next two lists exist.

Source: `KEYWORDS` in `glyph-compiler/crates/glyph-lexer/src/token.rs`. That
table drives both keyword lookup and "is this a keyword usable as a field
name", so a name here is still fine as a record field, an object key, or after
a dot.

## 2. TypeScript reserved words (33) — `E0109`

TypeScript's reserved set is bigger than Glyph's, so these lex as ordinary
identifiers and would emit a TS binding identifier `tsc` rejects. Glyph rejects
them at the source instead of shipping generated code that cannot compile.

```
arguments  case       catch      class      debugger   default
delete     do         enum       eval       export     extends
finally    function   implements instanceof new        null
package    private    protected  public     static     super
switch     this       throw      try        typeof     var
while      with       yield
```

`eval` and `arguments` are not keywords but cannot be bound in a strict-mode
module, which every emitted TypeScript module is.

Only binding positions are checked. `{ default: v }` and `x.new` are fine, and
so is a record field named `class`.

Source: `is_reserved_ts_word` in
`glyph-compiler/crates/glyph-resolver/src/reserved.rs`.

## 3. Names already bound in every emitted module — `E0110`

The hard list, because these produce TypeScript that compiles. They are legal
identifiers; they are just already taken in the module Glyph emits, so a
top-level declaration using one silently rebinds it and the program means
something other than what it says.

Checked on top-level `fn`, `type`, `const`, `component`, and **tagged-union
variant names**.

### JavaScript globals: no longer reserved

`Object`, `Promise`, `Number` and `Error` used to be on this list, because a
union with an `Error` variant emits `export function Error(...)` at module top
level and the `new Error(...)` the compiler writes below it then called the
variant. They are free now. A module that declares one of them captures the real
global first (`const __glyph_Error = globalThis.Error;`) and the emitter's own
references go through that alias, so a spreadsheet cell can be
`Number | Text | Empty | Error` and read the way its domain reads.

Nothing changes for a module that shadows nothing, which is almost all of them:
the capture lines are only emitted where a name is actually taken.

### Still reserved, and only one

| Name | Why |
|------|-----|
| `Array` | `Array<string>` is how every Glyph program spells an array. A local `type Array` does not merely shadow a global the compiler uses, it redefines a language type for the rest of the module, and capturing cannot help because the name that changed meaning is the one *you* write. |

`Record` is likewise a Glyph type name (`Record<K, V>`) and is reserved for the
same reason.

A test in `reserved.rs` greps `glyph-emit` for the `self.g("X")` calls every
global reference goes through, and fails when one appears that the capture list
does not cover. A global the emitter starts using without an entry would shadow
silently again, which is the defect this whole section exists for.

### Prelude globals

In scope in every module without an import, so a declaration using one replaces
it.

```
assert   bigint   bool     int      Issue    number
par      print    string   unknown  void
```

`void` is also a Glyph keyword (list 1), so it fails earlier.

`Issue` is a type rather than a value, and it is on the list for the same reason
the globals above are: the emitted module writes it whether or not you did.

| Name | Where the emitted module uses it |
| --- | --- |
| `Issue` | `parse(value: unknown): Result<T, Issue[]>` in every descriptor, and `const __issues: Issue[] = []` in a record descriptor |

Prelude types the emitter never writes on its own are **not** reserved, on the
same rule as `Date`: `Schema`, `Component` and `Option` only reach the output
because you wrote them in an annotation, so declaring one shadows nothing.
`Result`, `Ok`, `Err`, `infer_output` and the `schema` factory are emitted under
`__Glyph`-prefixed aliases and cannot be shadowed at all.

Std namespace names (`io`, `math`, `path`, `json`, `array`, `record`, ...) are
**not** on this list. They are only in scope in a module that imports them, and
a declaration colliding with one is already `E0100` (duplicate top-level name)
at the same span. `pub fn path(u: string) -> string` in a module that never
imports `std/path` is fine.

Source: `JS_GLOBALS` and `PRELUDE_GLOBALS` in
`glyph-compiler/crates/glyph-resolver/src/reserved.rs`.

## The primitive-union trap — `E0111`

Not a reserved word, but the same failure mode, and it is the one that looks
most like a working TypeScript program:

```glyph
type Key = string | number
```

In Glyph, `A | B` declares a **tagged union whose members are variant
constructors** (D8). So that line declares two variants named `string` and
`number`, which land straight on list 3. It used to build clean, pass
`tsc --strict`, and emit `export const string` / `export const number` that
shadowed the prelude.

Glyph has no primitive-union syntax. Name each case:

```glyph
type Key =
  | Text(string)
  | Count(number)
```

and `match` over it. For a raw TypeScript union at a boundary, the escape hatch
spells it verbatim and leaves the checking to `tsc`:

```glyph
type Key = extern_ts("string | number")
```

## Related

- [Error codes](../error-codes.md) — `E0100`, `E0109`, `E0110`, `E0111`
- [Language spec](../language/spec.md) — D8 tagged unions, D9 `match`, D15
  imports, D29 `extern_ts`
