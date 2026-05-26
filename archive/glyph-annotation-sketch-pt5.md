# Glyph annotation sketch — part 5 (abandoned direction)

> Pasted from conversation, 2026-05-26. Examples 21–25 in the same series. Same annotation-rich syntactic family. Same abandoned direction (current locked stance is TS-family).
>
> Ideas worth carrying forward are tracked in `docs/open-questions.md`. New questions surfaced by this file: Q28 (typestate), Q29 (structured edit API), Q30 (replayable traces as tests), Q31 (executable docs), Q32 (dual human/agent file representation).

---

```glyph
// ============================================================================
// GLYPH — Five more functions demonstrating distinct advantages
// ============================================================================


// ----------------------------------------------------------------------------
// [21] STATE MACHINES AS TYPES (TYPESTATE)
// The state of a value is part of its type. Calling `.send()` on an unbuilt
// request is a compile error, not a runtime exception. Agents writing
// protocol code (HTTP, gRPC, payments, OAuth) cannot violate the protocol —
// invalid transitions don't even parse.
// ----------------------------------------------------------------------------

@gid:type.http.request.v1
typestate HttpRequest {
    state Draft     -> { Built }       on .build()
    state Built     -> { Sent }        on .send()
    state Sent      -> { Received }    on .await_response()
    state Received  -> terminal
}

@gid:fn.api.fetch_user.v1
@intent "Fetch a user profile via a protocol-correct HTTP exchange"
@effects [network.api]
fn fetch_user(id: UserId, use http: cap:http.client) -> User | HttpError {
    let req: HttpRequest<Draft> = HttpRequest.new()
        .url("/users/" + id)
        .header("Accept", "application/json")
        .build()                          // Draft -> Built

    // req.send()        — OK, req is Built
    // req.await_response() — COMPILE ERROR: HttpRequest<Built> has no .await_response()

    let sent: HttpRequest<Sent> = req.send(http)
    let resp: HttpRequest<Received> = sent.await_response()
        ?? return propagate

    // sent.send() again — COMPILE ERROR: HttpRequest<Sent> has no .send()
    // resp.send()       — COMPILE ERROR: terminal state

    return resp.decode_json::<User>()
}


// ----------------------------------------------------------------------------
// [22] STRUCTURED EDITS — THE LANGUAGE EXPOSES AN EDIT API
// Agents don't patch source by string manipulation. They emit `@edit` blocks
// the compiler applies atomically. A failed edit rolls back; a successful
// edit is guaranteed syntactically valid AND type-checked AND test-passing
// before it touches disk. No more "the agent broke the file."
// ----------------------------------------------------------------------------

@gid:edit.add_locale_to_user.v1
@target  type.user.profile.v4
@intent  "Add a `timezone` field with a safe default"
@author  agent:claude
@review  required: human
edit {
    add_field @fid:009 timezone: String<iana_tz> = "UTC"
        after @fid:008
        with_migration auto
        with_test @example {
            UserProfile.example().timezone == "UTC"
        }
}
@verify {
    compiles
    all_tests_pass
    @semver_change minor          // additive only → minor bump auto-applied
    no_callers_broken
}
// If any clause in @verify fails, the edit is rejected as a single unit.
// The agent receives a structured rejection: { failed: "all_tests_pass",
// counterexamples: [...], affected_callers: [...] } — actionable, not a diff.


// ----------------------------------------------------------------------------
// [23] OPEN TELEMETRY AS REPLAYABLE TESTS
// Every production trace can be replayed as a deterministic test. Bug reports
// arrive as `.trace` files; an agent re-runs them locally with bit-exact
// fidelity. "Cannot reproduce" stops being an excuse.
// ----------------------------------------------------------------------------

@gid:fn.pricing.quote.v3
@intent "Compute a price quote; fully deterministic given (inputs, capabilities)"
@pure   false
@effects [db.read]
@replayable                           // marks function as trace-replay safe
fn quote(
    sku: Sku,
    customer: CustomerId,
    use db: cap:db.read,
    use clock: cap:time.read
) -> Quote | QuoteError {
    let product = db.get_product(sku)         ?? return propagate
    let tier    = db.get_tier(customer)       ?? return propagate
    let now     = clock.now()
    return Quote.compute(product, tier, at: now)
}

@gid:test.pricing.regression_2026_05_14.v1
@intent  "Regression test reconstructed from production trace t_8a3f...e91"
@source  trace://prod/2026-05-14T09:12:44Z/t_8a3f02b9e91
@expect  Quote { amount: Money.eur(149.99), valid_until: "2026-05-21" }
test replay(quote) from trace          // re-runs the EXACT db reads, EXACT clock,
                                        // EXACT inputs from production. Pass/fail
                                        // is binary; no fixtures to write.


// ----------------------------------------------------------------------------
// [24] DOCUMENTATION IS EXECUTABLE AND VERIFIED
// `@doc` blocks contain runnable code that the compiler executes on every
// build. Docs cannot rot. Examples in the README are the same artifacts the
// compiler checks. Agents learning a library read docs that are guaranteed
// to compile, type-check, and produce the stated output.
// ----------------------------------------------------------------------------

@gid:fn.collections.group_by.v1
@intent "Group elements by a key function"
@pure   true
@doc """
Groups a list of items by the result of a key function.

# Basic usage
```glyph
@run
let words = ["apple", "ant", "banana", "berry", "cherry"]
let by_first = group_by(words, (w) => w[0])
assert by_first == {
    'a': ["apple", "ant"],
    'b': ["banana", "berry"],
    'c': ["cherry"]
}
```

# Empty input returns empty map
```glyph
@run
assert group_by([], (x) => x) == {}
```

# Stable ordering within groups
```glyph
@run
let nums = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5]
let by_parity = group_by(nums, (n) => n % 2)
assert by_parity[1] == [3, 1, 1, 5, 9, 5, 3, 5]   // insertion order preserved
assert by_parity[0] == [4, 2, 6]
```
"""
fn group_by<T, K>(items: List<T>, key: (T) -> K) -> Map<K, List<T>> {
    return items |> fold (m, x) => m.append_at(key(x), x) from {}
}
// On `glyph build`, every ```glyph @run``` block is compiled and executed.
// Any `assert` failure fails the build. Docs that drift are unshippable.


// ----------------------------------------------------------------------------
// [25] DUAL HUMAN/AGENT LAYOUT
// Source files have two synchronized views: the human view (this file) and
// the agent view (a canonical, line-stable, fully-qualified form). Agents
// edit the agent view; humans read the human view; the compiler keeps them
// in lockstep. Whitespace wars, formatter churn, and "AI reformatted my
// file" diffs all disappear.
// ----------------------------------------------------------------------------

@gid:fn.text.word_count.v1
@intent "Count words in a string, splitting on Unicode whitespace"
@pure   true
@view   human                         // this is the human view
fn word_count(s: String) -> Int {
    return s
        |> split  .unicode_whitespace
        |> filter (w) => !w.empty()
        |> count
}

/* --- AGENT VIEW (auto-generated, line-stable, do not hand-edit) -----------
@gid:fn.text.word_count.v1
@hash:blake3:c41f...a803
@inputs  [{name:"s", type:"String"}]
@outputs Int
@body
  L001  return
  L002    $0 = s
  L003    $1 = pipe($0, split, {mode:"unicode_whitespace"})
  L004    $2 = pipe($1, filter, λ(w) => not(empty(w)))
  L005    $3 = pipe($2, count)
  L006    yield $3
--------------------------------------------------------------------------- */

// Agents diff the agent view: stable line numbers, stable token names, no
// formatter ambiguity. Humans review the human view: readable, idiomatic,
// no noise. The compiler enforces semantic equivalence on every save —
// they can never disagree.
```
