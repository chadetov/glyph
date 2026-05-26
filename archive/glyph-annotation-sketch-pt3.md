# Glyph annotation sketch — part 3 (abandoned direction)

> Pasted from conversation, 2026-05-26. Examples 11–15 in the same series as `glyph-annotation-sketch.md` (1–5) and `glyph-annotation-sketch-pt2.md` (6–10). Same annotation-rich syntactic family. Same abandoned direction (current locked stance is TS-family).
>
> Ideas worth carrying forward are tracked in `docs/open-questions.md`. New questions surfaced by this file: Q22 (content-addressed imports), Q23 (first-class observability + PII redaction). Strengthens Q11 (testing), Q15 (refinement types), Q17 (capabilities).

---

```glyph
// ============================================================================
// GLYPH — Five more functions demonstrating distinct advantages
// ============================================================================


// ----------------------------------------------------------------------------
// [11] REFINEMENT TYPES
// Constraints live in the type, not in defensive `if` checks scattered
// through the body. The compiler proves them at call sites, so agents
// never have to generate (and humans never have to review) input-validation
// boilerplate. Bugs that would be runtime checks become compile errors.
// ----------------------------------------------------------------------------

type Email       = String where matches(/^[^@\s]+@[^@\s]+\.[^@\s]+$/)
type Percentage  = Float  where 0.0 <= self <= 100.0
type NonEmpty<T> = List<T> where self.length > 0
type Port        = Int    where 1 <= self <= 65535
type Slug        = String where matches(/^[a-z0-9-]+$/) and length <= 64

@gid:fn.notify.send_campaign.v1
@intent "Send a campaign to a non-empty list of valid emails"
@pure   false
@effects [network.smtp]
fn send_campaign(
    recipients: NonEmpty<Email>,           // empty list = compile error at call site
    open_rate_goal: Percentage,             // 150.0 = compile error
    smtp_port: Port,                        // 99999 = compile error
    use smtp: cap:smtp.send
) -> CampaignReport {
    // No need to check `if recipients.empty()` — impossible by construction.
    // No need to check `if open_rate_goal > 100` — impossible by construction.
    return recipients
        |> map (addr) => smtp.send(addr, render(addr))
        |> collect    => CampaignReport.from(_, goal: open_rate_goal)
}


// ----------------------------------------------------------------------------
// [12] DETERMINISTIC TIME, RANDOMNESS, AND IO
// Non-determinism is a capability. A function that takes `use clock` is
// trivially testable with a fake clock — agents never write flaky tests,
// and replaying a production trace is exact, not approximate.
// ----------------------------------------------------------------------------

@gid:fn.cache.evict_stale.v1
@intent     "Evict cache entries older than ttl"
@pure       false                       // depends on clock — declared
@effects    [cache.write]
@deterministic given (clock, entries)   // same inputs → same outputs, always
fn evict_stale(
    entries: Map<Key, CacheEntry>,
    ttl:     Duration,
    use clock: cap:time.read,           // capability, not ambient
    use cache: cap:cache.write
) -> EvictionReport {
    let now = clock.now()
    return entries
        |> filter (k, e) => now - e.written_at > ttl
        |> map    (k, _) => cache.delete(k)
        |> collect       => EvictionReport.from(_, at: now)
}
// In tests: pass `cap:time.read.fake(at: "2026-01-01T00:00:00Z")`.
// In replay: pass `cap:time.read.from_trace(trace_id)` — bit-exact reproduction.


// ----------------------------------------------------------------------------
// [13] CONTENT-ADDRESSED IMPORTS
// Dependencies are pinned by cryptographic hash of the source AST, not by
// version strings. "It works on my machine" is impossible. An agent can
// verify the entire dependency graph in one pass; supply-chain attacks via
// version-string typosquatting are structurally prevented.
// ----------------------------------------------------------------------------

@import http     from glyph:std/http       @hash:blake3:7f3a...e2c1 @audit:stdlib
@import jwt      from glyph:std/crypto/jwt @hash:blake3:9d8b...4a17 @audit:stdlib
@import metrics  from org:obs/metrics      @hash:blake3:1c4e...b09f @audit:internal
@import stripe   from vendor:stripe/sdk    @hash:blake3:5e21...d3a8 @audit:third-party @last_reviewed:2026-04-02

@gid:fn.webhook.handle_stripe.v1
@intent  "Verify and dispatch a Stripe webhook"
@effects [network.metrics, queue.publish]
fn handle_stripe(
    req: http.Request,
    secret: SecretRef,                                  // never inlined, never logged
    use mq:    cap:queue.publish,
    use stats: cap:metrics.write
) -> http.Response {
    require stripe.verify_signature(req, secret) else {
        stats.inc("webhook.stripe.invalid_sig")
        return http.Response.unauthorized()
    }

    let event = stripe.parse_event(req.body) ?? return http.Response.bad_request()
    mq.publish(topic: "stripe.events", payload: event)
    stats.inc("webhook.stripe.accepted", tags: { type: event.kind })
    return http.Response.ok()
}


// ----------------------------------------------------------------------------
// [14] PROPERTY-BASED + FUZZ TESTING IS THE LANGUAGE, NOT A LIBRARY
// `@property` and `@fuzz` are first-class. The compiler runs them on every
// build with shrinking. Agents writing or modifying code get instant,
// machine-readable counterexamples — not a 200-line stack trace.
// ----------------------------------------------------------------------------

@gid:fn.text.parse_csv_line.v3
@intent "Parse one CSV line respecting RFC 4180 quoting"
@pure   true
@property forall s: String .
    parse_csv_line(s) is Ok implies
        join_csv_line(parse_csv_line(s).fields) == s.normalize_quotes()
@property forall fields: List<String> .
    parse_csv_line(join_csv_line(fields)).fields == fields
@fuzz     corpus: "data/csv/rfc4180-corpus" runs: 100_000 shrink: true
@fuzz     adversarial: utf8.malformed | utf8.mixed_endian | length.huge
fn parse_csv_line(line: String) -> Ok<{ fields: List<String> }> | CsvError {
    return line
        |> tokenize  .csv_aware
        |> validate  .rfc4180
        |> collect   .fields
}
// If an agent rewrites the body and breaks a property, the build emits:
//   FAIL parse_csv_line @property[1]  counterexample: `"a"",b`  shrunk_from: 18 chars
//   That is the diff feedback — no human translation needed.


// ----------------------------------------------------------------------------
// [15] FIRST-CLASS OBSERVABILITY
// Tracing, metrics, and logs are declared in the signature. Agents cannot
// "forget to add a span" — every function emits structured telemetry by
// construction, and PII redaction is enforced at the type level.
// ----------------------------------------------------------------------------

@gid:fn.checkout.complete_order.v2
@intent "Finalize an order: charge, fulfill, notify"
@trace   span: "checkout.complete_order"
@metrics counter: "orders.completed" by status
@metrics histogram: "orders.completion_ms" buckets: [10, 50, 100, 500, 1000, 5000]
@log     level: info on success
@log     level: error on failure with stack: false      // stack traces are useless to agents
@redact  fields: [card.number, card.cvv, customer.email]  // enforced at log site
@effects [network.stripe, db.write, queue.publish]
fn complete_order(
    order:    Order,
    payment:  PaymentMethod,
    use stripe: cap:stripe.charges.write,
    use db:     cap:db.orders.write,
    use mq:     cap:queue.publish
) -> CompletedOrder | CheckoutError {
    let charge = stripe.charge(order.total, payment)
        ?? return tagged("payment_failed", propagate)

    let saved = db.mark_paid(order.id, charge.id)
        ?? return tagged("persist_failed", propagate)

    mq.publish(topic: "orders.fulfill", payload: saved.id)
    return CompletedOrder { order: saved, charge }

    // The runtime automatically emits:
    //   span:    checkout.complete_order { order_id, charge_id, duration_ms }
    //   metric:  orders.completed{status="ok"}      += 1
    //   metric:  orders.completion_ms                <- duration
    //   log:     { event: "order.completed", order_id, charge_id }
    // — with `card.number`, `card.cvv`, `customer.email` redacted before serialization.
}
```
