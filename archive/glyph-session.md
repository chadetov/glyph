# Glyph: A Programming Language for AI Agents

> Session notes — language design exploration and Claude Code kickoff prompt.

## Thesis

AI changed how we program. Existing languages are built for human engineers who read almost every line. We need a language so easy to abstract that an AI agent can ingest the information in fewer tokens.

The four pillars Glyph optimizes for:

1. **Abstraction** — dense semantic primitives, intent at the signature level
2. **Verifiability** — invariants, effects, and tests are structural, not bolted on
3. **Diff stability** — edits produce minimal, localized changes
4. **Greppability** — agents can find what they need by structured search, not by reading bodies

For the first time in software engineering history, the assumption that programming languages are written for humans is being questioned.

---

## Design Sketch

A function in Glyph carries far more semantic content in its signature region than mainstream languages do. The body stays roughly the same length as Python, but everything an agent needs to *scan* a codebase is front-loaded above the implementation.

### Key syntactic elements

| Element | Purpose |
|---|---|
| `intent: "..."` | First-class behavioral description, part of the signature |
| `in:` / `out:` | Typed inputs and outputs as records |
| `invariants:` | Pre/post-conditions enforced by the compiler |
| `effects: [...]` | Declared side effects, transitively checked |
| `@do` | Pipeline body with named stages |
| `->` | Stage assignment within a pipeline |
| `?err -> X` | Inline error routing |
| `?warn` / `?empty` | Non-fatal exits |
| `@when` | Guarded early return |
| `@ensure` | Inline assertion of an invariant |
| `\|>` | Left-to-right pipeline operator |
| `.field` | Receiver-implicit field access inside pipelines |
| `@test` | Colocated tests with semantic labels |

---

## Example 1: `process_payment`

A function with linear flow, external effects, and inline error routing.

```glyph
@fn process_payment
  intent: "charge customer card and emit receipt"
  in:  { user: User, amount: Money, card: CardToken }
  out: Result<Receipt, PaymentError>

  invariants:
    - amount > 0
    - user.verified
    - card.not_expired

  effects: [db.write, network.stripe, email.send]

  @do
    validate    -> @ensure invariants
    authorize   -> stripe.auth(card, amount)         ?err -> AuthFailed
    capture     -> stripe.capture(authorize.id)      ?err -> CaptureFailed
    persist     -> db.txn { receipt := Receipt.new(user, amount, capture) }
    notify      -> email.receipt(user, persist)      ?warn
    return persist

  @test
    given: { user: valid_user, amount: 50.usd, card: test_card_4242 }
    expect: Ok(Receipt { amount: 50.usd, status: captured })
```

### What this earns

- **Intent as a first-class field.** Not a comment. Refactors that change behavior must update intent, making drift detectable.
- **Explicit effects.** An agent never has to read the body to know what this touches. A diff that adds `file.write` to effects is a structural change visible at the signature level.
- **Pipeline syntax with named stages.** Each `->` step has a name. Edits to "the capture step" touch one line, not a block.
- **Inline error routing.** `?err -> AuthFailed` is shorter than try/catch and keeps the happy path in one column.
- **Invariants and tests colocated.** The compiler can reject the function if invariants don't hold.
- **Single-token semantic primitives.** `Money`, `Result<>`, `50.usd`, `db.txn` compress 5-20 tokens of boilerplate into one.

---

## Example 2: `rebalance_portfolio`

A function with branching, ranking, and conditional flow.

```glyph
@fn rebalance_portfolio
  intent: "rebalance user holdings to target allocation, minimizing taxable events"
  in:  { portfolio: Portfolio, target: Allocation, tolerance: Percent = 2% }
  out: Result<RebalancePlan, RebalanceError>

  invariants:
    - target.weights.sum == 100%
    - portfolio.holdings.all(h => h.quantity >= 0)
    - tolerance in 0%..10%

  effects: [market.read, tax.compute]
  pure_excluding: effects

  @do
    drift       -> portfolio.diff(target)
    @when drift.max < tolerance
      return Ok(RebalancePlan.noop)

    candidates  -> drift.overweight                  ?empty -> NothingToSell
    lots        -> candidates.flatmap(.tax_lots)

    sells       -> lots
                   |> rank_by(tax_cost: asc, drift_contribution: desc)
                   |> take_until(.cumulative_value >= drift.rebalance_amount)

    buys        -> drift.underweight
                   |> allocate(sells.proceeds, weighted_by: .gap)

    plan        -> RebalancePlan { sells, buys, est_tax: sells.sum(.tax_cost) }
    @ensure plan.net_drift < tolerance
    return Ok(plan)

  @test "no-op when within tolerance"
    given: { portfolio: balanced_portfolio, target: same_allocation }
    expect: Ok(RebalancePlan.noop)

  @test "prefers low-tax lots"
    given: { portfolio: mixed_lots_portfolio, target: shifted_allocation }
    expect.plan.sells.all(.tax_cost < .alternative_lot_tax_cost)
```

### What this adds

- **`|>` pipeline operator.** Chains transformations left-to-right; new filter steps are one-line inserts.
- **`pure_excluding: effects`.** Declares purity modulo declared effects. Enables deterministic property-based test generation.
- **`@when` early return.** Single line, no nested block.
- **Field-access shorthand.** `.tax_cost` inside a pipeline keeps columns narrow and diffs aligned.
- **Property assertions in tests.** `expect.plan.sells.all(...)` asserts relationships, not magic numbers. Test fixtures can be regenerated without breakage.
- **Named tests with intent strings.** Test names are sentences. CI failures already explain what broke.

---

## Open Design Questions

Captured here so they're not lost. Each one needs an RFC before implementation.

1. **Module system.** How do imports work? How does an agent discover what's available without reading every file?
2. **Effect inference vs declaration.** Compiler-inferred and checked, or hand-written and trusted? Transitivity across calls?
3. **Generics and traits.** What does parametric abstraction look like in a token-dense signature?
4. **Async / long-running processes.** How does Glyph express inherently stateful work without losing pipeline clarity?
5. **Greppability vs token-efficiency.** When verbose unique identifiers conflict with short symbols, what's the default?
6. **Test/invariant unification.** Invariants run at runtime; tests run in CI. Should there be one verification model?

---

## Claude Code Kickoff Prompt

The prompt below is designed to seed a fresh Claude Code session in an empty repo. It enforces a design-first workflow (no compiler code until the spec is solid), an RFC process for contested decisions, and a working-notes file for traceability.

````markdown
We're designing a new programming language called **Glyph**, optimized for AI agents as the primary readers and writers of code rather than humans. The core thesis: existing languages waste tokens on human-oriented syntax and force agents to track context across many lines. Glyph optimizes for four properties — **abstraction, verifiability, diff stability, greppability** — even at the cost of human ergonomics.

## Phase 0: Project Setup

Create a new repo `glyph-lang/` with this structure:

```
glyph-lang/
  README.md                  # vision, thesis, design principles
  docs/
    00-manifesto.md          # why Glyph exists, what it optimizes for
    01-design-principles.md  # the four pillars, with rationale
    02-syntax-reference.md   # full grammar (to be filled in)
    03-semantics.md          # execution model, effect system, type system
    04-tooling.md            # compiler, formatter, LSP, agent integrations
    05-comparison.md         # Glyph vs Python/Rust/TypeScript side-by-sides
  examples/
    01-process-payment.glyph
    02-rebalance-portfolio.glyph
    (more to come)
  spec/
    grammar.ebnf             # formal grammar
    effects.md               # effect system spec
    types.md                 # type system spec
  rfcs/
    0001-template.md         # RFC process for design decisions
  .glyph-guide.md            # working notes, open questions, decisions log
```

## Phase 1: Capture the Design

Before writing any compiler code, we need the design solid. Read the two example functions below and reverse-engineer the language from them. Then:

1. Write `docs/00-manifesto.md` arguing why a language for AI agents is needed now, and what changes when humans aren't the primary audience.
2. Write `docs/01-design-principles.md` covering the four pillars with concrete examples of each.
3. Draft `docs/02-syntax-reference.md` as a complete reference, inferring the grammar from the examples and extending consistently. Cover: function definitions, the `@do` pipeline, `@when` / `@ensure`, the `|>` operator, field-access shorthand, effect declarations, invariants, inline `@test` blocks, type annotations, and error routing (`?err`, `?warn`, `?empty`).
4. Draft `spec/grammar.ebnf` formally.

## Phase 2: Open Design Questions

These are unresolved and need RFCs in `rfcs/`. Don't decide them unilaterally — write each as an RFC with options, tradeoffs, and a recommendation, then ask me to pick:

- **RFC-0002: Module system.** How do imports work? How does an agent discover what's available without reading every file?
- **RFC-0003: Effect inference vs declaration.** Are effects inferred by the compiler and checked against declarations, or hand-written and trusted? How are they transitive across calls?
- **RFC-0004: Generics and traits.** What does parametric abstraction look like in a language where signatures are token-dense?
- **RFC-0005: Async / long-running processes.** How does Glyph express something inherently stateful without losing pipeline clarity?
- **RFC-0006: Greppability vs token-efficiency.** When these conflict (verbose unique identifiers vs short symbols), what's the default?
- **RFC-0007: Test/invariant relationship.** Invariants are checked at runtime; tests are checked in CI. Should there be a unified verification model?

## Phase 3: Reference Implementation Plan

Once design is locked, sketch a plan in `docs/04-tooling.md` for:

- Parser (recommend a host language — likely Rust or TypeScript, justify the choice)
- Tree-sitter grammar for editor support
- A transpiler-first approach (Glyph → TypeScript or Python) before a real compiler, so we can dogfood quickly
- LSP server with agent-specific features (effect summaries on hover, semantic diff output, structured grep)

Don't start implementation yet — just the plan.

## Working Style

- Update `.glyph-guide.md` after every meaningful decision with a one-line entry: date, decision, rationale.
- When you hit ambiguity, write an RFC rather than guessing.
- Prefer many small focused docs over a few sprawling ones.
- For every design decision, ask: "does this make Glyph more abstract / verifiable / diff-stable / greppable, or is it cargo-culted from existing languages?"
- No em dashes in the docs. Keep the prose tight and human-sounding.

## Source material

Here are the two example functions that define the language so far. Treat these as the ground truth — the grammar must accommodate them exactly:

[paste the `process_payment` example here]

[paste the `rebalance_portfolio` example here]

Start with Phase 1. When you finish each document, summarize what you wrote and what assumptions you made, then ask me to review before moving on.
````

---

## Suggested Next Step

Once the repo is bootstrapped, add a `benchmarks/` directory with token-count comparisons of Glyph vs Python vs Rust for equivalent functions. That's the empirical claim Glyph is making, and it should be measurable from day one.
