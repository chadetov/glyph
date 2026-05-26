# Glyph annotation sketch — part 2 (abandoned direction)

> Pasted from conversation, 2026-05-26. Continuation of `glyph-annotation-sketch.md` — examples numbered 6–10, demonstrating capability effects, structured concurrency, totality, first-class migrations, and errors-as-runbooks. Same annotation-rich syntactic family. Same abandoned direction (the current locked stance is TS-family looks-like-TypeScript).
>
> Ideas worth carrying forward are tracked in `docs/open-questions.md`: Q17 capability-based effects, Q18 structured concurrency, Q19 errors-as-runbooks, Q20 loop construct gap, Q21 migrations.

---

```glyph
// ============================================================================
// GLYPH — Five more functions demonstrating distinct advantages
// ============================================================================


// ----------------------------------------------------------------------------
// [6] CAPABILITY-BASED EFFECTS
// No ambient authority. An AI agent cannot accidentally write a function
// that secretly hits the network or filesystem — capabilities must be
// passed explicitly, and the compiler tracks them transitively.
// This makes generated code auditable in a single grep.
// ----------------------------------------------------------------------------

@gid:fn.payments.charge_card.v2
@intent  "Charge a card and persist the receipt"
@capabilities [cap:stripe.charges.write, cap:db.receipts.write, cap:log.audit]
@effects     [network.stripe, db.write, log.append]
@idempotent  on idempotency_key
@raises      [InsufficientFunds, NetworkTimeout, DuplicateCharge]
fn charge_card(
    amount: Money,
    card:   CardToken,
    idempotency_key: Uuid,
    use stripe: cap:stripe.charges.write,    // explicit capability
    use db:     cap:db.receipts.write,        // explicit capability
    use audit:  cap:log.audit                 // explicit capability
) -> Receipt | ChargeError {
    let existing = db.find_by_key(idempotency_key)
    if existing exists { return existing.receipt }      // idempotent replay

    let charge = stripe.create(amount, card, idempotency_key)
        ?? return propagate

    let receipt = Receipt {
        id: charge.id, amount, charged_at: time.now()
    }

    db.insert(receipt) ?? return propagate
    audit.log("charge.success", { id: receipt.id, amount })
    return receipt
}


// ----------------------------------------------------------------------------
// [7] STRUCTURED CONCURRENCY WITH DETERMINISTIC ORDERING
// `parallel { ... }` is a structured block: nothing escapes, errors propagate,
// cancellation is automatic. The agent cannot create dangling tasks or
// race conditions because there is no way to "fire and forget."
// ----------------------------------------------------------------------------

@gid:fn.dashboard.load_overview.v1
@intent "Fetch user dashboard data concurrently with bounded latency"
@effects [network.api]
@timeout 800ms                          // enforced at compile + runtime
@cancellation cooperative
fn load_overview(user_id: UserId, use api: cap:api.read) -> Dashboard {
    parallel {
        let profile  = api.fetch_profile(user_id)       // task A
        let orders   = api.fetch_orders(user_id, last: 30d)   // task B
        let credits  = api.fetch_credits(user_id)       // task C
        let messages = api.fetch_messages(user_id, unread: true) // task D
    } on_error (e) => return Dashboard.degraded(e)
      on_timeout   => return Dashboard.partial(profile?, orders?, credits?, messages?)

    // All four are guaranteed resolved here; ordering of joins is deterministic.
    return Dashboard {
        profile, orders, credits, unread: messages.count
    }
}


// ----------------------------------------------------------------------------
// [8] EXHAUSTIVE PATTERN MATCHING WITH TOTALITY CHECKS
// The compiler refuses to compile if a case is missing. Refactors that add
// a variant produce a compile error at every match site — agents get
// pinpoint diffs instead of silent runtime bugs.
// ----------------------------------------------------------------------------

@gid:type.payment.event.v1
type PaymentEvent = sealed
    | Initiated  { id: PaymentId, amount: Money }
    | Authorized { id: PaymentId, auth_code: String }
    | Captured   { id: PaymentId, captured_at: Timestamp }
    | Refunded   { id: PaymentId, reason: String, amount: Money }
    | Failed     { id: PaymentId, code: ErrorCode, retryable: Bool }

@gid:fn.payment.project_state.v1
@intent "Fold a stream of payment events into the current state"
@pure   true
@total                                  // compiler enforces exhaustiveness
fn project_state(events: Stream<PaymentEvent>) -> PaymentState {
    return events |> reduce (state, e) => match e {
        Initiated  { id, amount }       => state.start(id, amount)
        Authorized { auth_code }        => state.authorize(auth_code)
        Captured   { captured_at }      => state.capture(captured_at)
        Refunded   { amount, reason }   => state.refund(amount, reason)
        Failed     { retryable: true }  => state.mark_retry()
        Failed     { retryable: false } => state.mark_terminal()
    } from PaymentState.empty()
}


// ----------------------------------------------------------------------------
// [9] FIRST-CLASS MIGRATIONS
// Schema and code evolve together. The `@migrates_from` clause is mechanical:
// the compiler generates the upgrade path AND the rollback. AI agents never
// have to invent migration scripts — the language emits them.
// ----------------------------------------------------------------------------

@gid:type.order.v5
@migrates_from type.order.v4
type Order {
    @fid:001 id:           OrderId
    @fid:002 customer_id:  UserId
    @fid:003 items:        List<LineItem>
    @fid:004 total:        Money
    @fid:009 currency:     Currency       = from(total.currency)   // split out in v5
    @fid:010 status:       OrderStatus    = OrderStatus.Pending    // new in v5
}

@gid:fn.order.migrate_v4_to_v5.v1
@intent      "Auto-generated forward migration with verified inverse"
@reversible  via migrate_v5_to_v4
@pure        true
fn migrate_v4_to_v5(old: type.order.v4) -> type.order.v5 {
    return Order {
        @fid:001 = old.@fid:001
        @fid:002 = old.@fid:002
        @fid:003 = old.@fid:003
        @fid:004 = old.@fid:004
        @fid:009 = old.@fid:004.currency
        @fid:010 = derive_status(old)            // pure helper
    }
    @verify roundtrip: migrate_v5_to_v4(migrate_v4_to_v5(x)) == x
}


// ----------------------------------------------------------------------------
// [10] SELF-DESCRIBING ERRORS AS DATA
// Errors carry remediation hints the agent can act on programmatically.
// No string parsing, no stack-trace archaeology — the error IS the runbook.
// ----------------------------------------------------------------------------

@gid:type.error.rate_limit.v1
type RateLimitError {
    @fid:001 endpoint:    String
    @fid:002 limit:       Int
    @fid:003 window:      Duration
    @fid:004 retry_after: Duration
    @fid:005 remediation: Remediation = Remediation.Backoff{ jitter: true }
    @fid:006 docs_url:    Url<https>
}

@gid:fn.client.request_with_remediation.v1
@intent  "Issue a request and let the agent self-heal on known errors"
@effects [network.api]
fn request_with_remediation(
    req: Request,
    use api: cap:api.read
) -> Response | UnrecoverableError {

    loop attempt in 1..=3 {
        return api.send(req) on_error (e) => match e {
            RateLimitError { retry_after, remediation: Backoff } => {
                sleep(retry_after + jitter(0..200ms))
                continue                       // agent-readable: "retry"
            }
            AuthExpiredError { remediation: RefreshToken { endpoint } } => {
                req = req.with_fresh_token_from(endpoint)
                continue
            }
            ValidationError { fields, remediation: FixPayload { hints } } => {
                return UnrecoverableError.from(e)   // agent reads `hints` to patch caller
            }
            _ => return UnrecoverableError.from(e)
        }
    }
    return UnrecoverableError.exhausted_retries(req)
}
```
