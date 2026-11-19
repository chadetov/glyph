<!-- Markdown copy of https://glyphlang.io/answers/ai-authoring/, generated from the same source as the page. Last changed 2026-11-19. -->

07 · The premise

# Can an AI reliably write a language it's barely seen?

Yes, because Glyph is a **disciplined subset of TypeScript** with an ML core. Most of what a model already knows about TypeScript carries over, and the compiler names the places it doesn't.

## Why it matters

It's the fair skeptic's question: an LLM predicts the next token from the huge body of existing-language code, so a language with almost no training data should be hard to write. The answer is structural: Glyph isn't alien. It looks like TypeScript, so the model already knows most of it; the differences are few, enumerable, and each one fails *fast* at compile time rather than passing silently.

## See it

An agent that guesses wrong doesn't ship the guess. It gets a precise, fixable error and corrects in the same loop:

the correction loop, in one step

```
// agent writes, reaching for a TypeScript habit:
if user.admin { grant() }

[E0006] Glyph has no `if`
   ╰─ `match` is the only conditional:
      match user.admin { true => grant(), false => void, }
```

The error names the Glyph construct and shows the shape, so the fix is one edit away: no source-diving, no guessing. That tight loop is what compensates for the missing training data. In our own adversarial testing, an agent built a real REST API (typed records, tagged-union validation, auth middleware) to a **green build in about two iterations**. And to remove the last friction, one document ([llms.txt](https://glyphlang.io/llms.txt)) takes an agent from zero to correct, runnable Glyph.

## Where it stands

### Shipping today

A subset-of-TypeScript design, Elm-quality errors that name the fix, and `llms.txt` / `glyph llms` one-document onboarding for any agent.

### Being proven

A success-rate benchmark (tries-to-correct against TypeScript on the same task) to turn “it held up” into a measured number.

Code examples on this page are checked against Glyph 0.1.149 on every build. Last changed 19 Nov 2026.
