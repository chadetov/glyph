<!-- Markdown copy of https://glyphlang.io/answers/ai-authoring/, generated from the same source as the page. Last changed 2026-11-21. -->

07 · The premise

# Can an AI reliably write a language it's barely seen?

Mostly. Glyph reads like TypeScript: the same primitive and generic types (`string`, `number`, `Array<T>`), most of the same expressions, and npm packages imported by name. It is not a subset of TypeScript: `match`, `Result` and `fn name(x: T) -> R` are syntax TypeScript does not have, and `if`, the ternary and casts are not Glyph. Most of what a model already knows carries over, and the compiler names the places it doesn't.

the correction loop, in one step

```
// agent writes, reaching for a TypeScript habit:
if user.admin { grant() }

[E0006] Glyph has no `if`
   ╰─ `match` is the only conditional:
      match user.admin { true => grant(), false => void, }
```

An agent that guesses wrong doesn't ship the guess. It gets a precise, fixable error and corrects in the same loop.

## Why it matters

It's the fair skeptic's question: an LLM predicts the next token from the huge body of existing-language code, so a language with almost no training data should be hard to write.

The answer is structural: Glyph isn't alien. It looks like TypeScript, so the model already knows most of it; the differences are few, enumerable, and each one fails *fast* at compile time rather than passing silently.

## The fix is one edit away

The error names the Glyph construct and shows the shape, so the fix is one edit away: no source-diving, no guessing. That tight loop is what compensates for the missing training data.

One document, [llms.txt](https://glyphlang.io/llms.txt), gives an agent the program shape, the standard library and every diagnostic with its fix, and `glyph llms` prints the same reference offline.

## Where it stands

### Shipping today

- A TypeScript-shaped syntax.
- Errors with a stable code that name the fix.
- `llms.txt` / `glyph llms` as one reference document for any agent.

### Being proven

- A success-rate benchmark (tries-to-correct against TypeScript on the same task) to turn “it held up” into a measured number.

Code examples on this page are checked against Glyph 0.1.159 on every build. Last changed 21 Nov 2026.
