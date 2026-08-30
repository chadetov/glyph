# workflow

## What it is

A hierarchical statechart replay engine. It loads a machine definition and an
ordered event log, validates the machine reporting every problem at once rather
than the first, and replays the log: per event it applies updates to a typed
context, walks outward from the active leaf collecting every state that declares
the event, evaluates each candidate's declarative guard tree, takes the innermost
passing one, computes exit and entry sets, runs the actions, and records a trace
step. Guards are data, not strings: there is no expression parser in this
program.

## Running it

```sh
glyph run examples/apps/workflow/main.glyph
glyph run examples/apps/workflow/main.glyph --validate
glyph run examples/apps/workflow/main.glyph --events deadlock-events.json
```

## What it changed in Glyph

It closed the largest exhaustiveness hole in the language.

**G73 (0.1.56): a namespace-qualified `match` over an imported union got no
exhaustiveness check at all.** `import model` plus `model.Yes(_)` arms was
accepted with any subset of the variants covered: no diagnostic, `tsc --strict`
green, and a non-exhaustive-match throw at run time. It reached the prelude
unions too, which is the part that matters: `Option<T>` lost D9 to a one-token
change in how it was imported.

The app's own import lists were the evidence. Every module carried an
eighteen-name variant import, with a comment explaining that a union's
constructors do not arrive with its type. The comment was accurate about the
syntax and wrong about the reason: the author had not chosen the named form for
readability, they had been pushed into it because the namespace form silently
skipped the check.

Proving the fix on the real app rather than on unit tests found a second defect,
because making `Result` decidable also made it classifiable and every `?` in a
function returning the qualified type became an error. The app now carries both
spellings deliberately and builds clean on both.

**G74**: E0200 quoted the missing variant names for local and prelude unions and
left them bare for imported ones.

## What it exercises

Nine modules, a recursive condition union, both import spellings coexisting on
purpose, optional wire fields decoded into a domain type carrying `Option` (the
idiom that resolved G66), and a wire type that reuses the domain's own guard
union so there is no second definition of what a guard looks like.
