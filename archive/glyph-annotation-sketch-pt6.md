# Glyph annotation sketch — part 6 (abandoned direction)

> Pasted from conversation, 2026-05-26. Examples 26–35 in the same series — the biggest batch (ten examples). Same annotation-rich syntactic family. Same abandoned direction (current locked stance is TS-family).
>
> Ideas worth carrying forward are tracked in `docs/open-questions.md` as Q33–Q42. The standouts:
> - Q33 (provenance / taint types) — SQL injection, XSS, command injection as compile errors.
> - Q34 (budgeted execution) — wall-time, memory, LLM-tokens, USD-cost ceilings. Real production concern unique to AI-native code.
> - Q36 (domain units as types) — Mars Climate Orbiter / cents-vs-dollars at the type level.
> - Q40 (type-driven generation) — the most agent-native idea: humans (or LLMs) write the spec block, a generator owns the body, regeneration is contract-preserving by construction.
> - Q41 (FFI with contract inheritance) — mandatory for non-TS interop (C, Rust, Python).

---

```glyph
// ============================================================================
// GLYPH — Ten more functions demonstrating distinct advantages
// ============================================================================


// ----------------------------------------------------------------------------
// [26] PROVENANCE TRACKING
// Every value carries a tamper-evident trail of where it came from. Tainted
// data (user input, network, untrusted files) cannot reach a "sink" (SQL,
// shell, eval, response body) without passing through a declared sanitizer.
// SQL injection, XSS, command injection become compile errors.
// ----------------------------------------------------------------------------

@gid:fn.search.run_query.v1
@intent  "Execute a parameterized search query"
@effects [db.read]
fn run_query(
    raw_input: String<tainted:user>,           // tainted at the type level
    use db: cap:db.read
) -> List<Result> | QueryError {

    // db.execute(raw_input) — COMPILE ERROR:
    //   sink `db.execute` requires String<trusted:sql>,
    //   got String<tainted:user>. Pass through a sanitizer.

    let safe: String<trusted:sql_param> = raw_input
        |> trim
        |> limit_length 200
        |> escape.sql_param                    // declared sanitizer flips taint

    return db.execute(
        sql"SELECT * FROM products WHERE name LIKE ${safe}"
    )
}
// Provenance flows transitively through assignments, pipelines, and function
// calls. Agents cannot accidentally launder taint by passing through a helper.


// ----------------------------------------------------------------------------
// [27] BUDGETED EXECUTION
// Functions can declare hard ceilings on time, memory, CPU, and tool calls.
// The runtime enforces them; the compiler refuses bodies that can't be
// proven to fit. Agents writing recursive or LLM-orchestrating code cannot
// produce runaway loops or runaway bills.
// ----------------------------------------------------------------------------

@gid:fn.agent.summarize_doc.v1
@intent     "Summarize a document using an LLM, with hard budgets"
@effects    [network.llm]
@budget     wall_time:    30s
@budget     memory_peak:  256.MiB
@budget     llm_tokens:   8_000          // total prompt+completion
@budget     llm_calls:    4              // max round-trips
@budget     usd_cost:     0.25
@on_exceed  return Summary.partial(_)    // graceful degradation
fn summarize_doc(
    doc: Document,
    use llm: cap:llm.complete
) -> Summary | SummarizeError {
    return doc
        |> chunk     size: 2_000.tokens
        |> map       (c) => llm.summarize(c)   // budget enforced per-call
        |> reduce    (a, b) => llm.merge(a, b)
        |> finalize  (s) => Summary.seal(s)
}
// If aggregate budget is about to be exceeded, the runtime invokes @on_exceed
// instead of killing the process. Agents get partial results, not panics.


// ----------------------------------------------------------------------------
// [28] FEATURE FLAGS AS LANGUAGE PRIMITIVES
// Flags are typed, scoped, and discoverable. Dead flags fail the build.
// Conflicting flags fail the build. Agents cannot leave behind orphaned
// `if FEATURE_X` checks that haunt the codebase for years.
// ----------------------------------------------------------------------------

@flag:checkout.new_pricing_engine
@owner     team:payments
@created   2026-02-01
@sunset    2026-08-01                    // build fails after this date if still present
@rollout   percentage(20%) | allowlist(beta_users)
@type      Bool

@gid:fn.checkout.compute_price.v4
@intent "Compute price; routes between old and new engine via a typed flag"
@pure   true
fn compute_price(cart: Cart, customer: Customer) -> Money {
    return when @flag:checkout.new_pricing_engine.enabled(customer) {
        true  => price_engine_v2(cart, customer)
        false => price_engine_v1(cart, customer)
    }
}
// On 2026-08-01 the build will refuse to compile until the flag and its
// branch are removed. Agents performing the cleanup get a one-line edit
// to make: replace the `when` block with the surviving branch.


// ----------------------------------------------------------------------------
// [29] DOMAIN UNITS AS TYPES
// Quantities carry units. `Meters + Seconds` is a compile error. Currency
// arithmetic without conversion is a compile error. The Mars Climate Orbiter
// bug, every cents-vs-dollars bug, every milliseconds-vs-seconds bug — all
// gone at the type level.
// ----------------------------------------------------------------------------

@gid:fn.physics.kinetic_energy.v1
@intent "Compute kinetic energy with unit safety"
@pure   true
fn kinetic_energy(mass: Quantity<kg>, velocity: Quantity<m/s>) -> Quantity<J> {
    return 0.5 * mass * velocity^2          // unit inference produces J
}

@gid:fn.billing.convert_charge.v1
@intent "Convert a charge to the customer's currency"
@pure   true
fn convert_charge(
    amount: Money<USD>,
    rate:   ExchangeRate<USD, EUR>
) -> Money<EUR> {
    return amount * rate                    // type-checked currency math
    // return amount + rate     — COMPILE ERROR: Money<USD> + Rate not defined
    // return amount.as_eur()   — COMPILE ERROR: no implicit conversion
}

@gid:fn.scheduling.deadline.v1
@intent "Compute deadline from request timestamp + SLA"
@pure   true
fn deadline(received_at: Timestamp, sla: Duration<ms>) -> Timestamp {
    return received_at + sla
    // return received_at + 5000           — COMPILE ERROR: needs Duration, not Int
    // return received_at + sla.as_seconds() ⊕ sla — COMPILE ERROR: ambiguous unit
}


// ----------------------------------------------------------------------------
// [30] CONTENT-PRESERVING REFACTORS
// `@refactor` blocks describe semantic transformations the compiler verifies
// preserve behavior. Renames, extractions, inlinings, and parameter reorders
// are first-class operations with proof obligations. Agents perform large
// refactors as a single declarative edit instead of N risky text patches.
// ----------------------------------------------------------------------------

@refactor:rename_param.v1
@target  fn.orders.find.v2
@intent  "Rename `cust` to `customer_id` for clarity"
@preserves behavior
refactor {
    rename param "cust" -> "customer_id"
    update_all_callers automatic
    update_all_docs    automatic
}
@verify { compiles all_tests_pass behavior_equivalent_to_pre_edit }

@refactor:extract_function.v1
@target  fn.report.build.v3 lines 42..67
@intent  "Extract aggregation block into compute_totals"
@preserves behavior
refactor {
    extract -> fn.report.compute_totals.v1
    inputs  inferred
    outputs inferred
    visibility module
}
@verify { compiles all_tests_pass behavior_equivalent_to_pre_edit no_perf_regression }


// ----------------------------------------------------------------------------
// [31] DIFFERENTIAL TYPING ACROSS VERSIONS
// A function can declare what it changed RELATIVE to its previous version.
// The compiler proves the delta. Code review for AI-generated edits becomes
// reading a one-line @delta, not diffing 80 lines of body.
// ----------------------------------------------------------------------------

@gid:fn.search.rank.v4
@since  2026-05-26
@delta_from v3 {
    behavior:  "ties now broken by recency instead of alphabetical"
    perf:      "p99 -18% via single-pass scoring"
    surface:   unchanged                  // signature identical, callers safe
    semantics: "score function modified; results MAY differ on tie boundaries"
}
@verify_delta {
    @property forall q, items .
        rank.v4(q, items).top(1) == rank.v3(q, items).top(1)
        unless ties_in_top_1(q, items)
}
@pure true
fn rank(query: Query, items: List<Doc>) -> List<Doc> {
    return items
        |> score   (d) => score_v4(query, d)
        |> sort_by .score desc then .recency desc
}
// Reviewers (human or agent) read @delta_from and trust it because the
// compiler refuses the version bump unless @verify_delta passes.


// ----------------------------------------------------------------------------
// [32] POLICY-AS-TYPES
// Authorization, retention, residency, and compliance rules attach to types.
// Routing PII to the wrong region, retaining data past policy, or returning
// it to an unauthorized caller is a compile error — not a 3am incident.
// ----------------------------------------------------------------------------

@gid:type.medical_record.v1
@classification PHI                              // HIPAA
@residency      [region:eu, region:uk]           // GDPR
@retention      max: 7.years
@access         requires(role:clinician) and purpose(:treatment | :billing)
type MedicalRecord {
    @fid:001 patient_id:  PatientId
    @fid:002 diagnosis:   String      @redact_in(logs, analytics)
    @fid:003 notes:       String      @redact_in(logs, analytics, exports)
}

@gid:fn.records.export.v1
@intent  "Export records for the requesting clinician"
@effects [db.read]
fn export_records(
    patient: PatientId,
    caller:  Principal,
    use db: cap:db.read
) -> List<MedicalRecord> | PolicyError {

    require caller.role == :clinician          else PolicyError.unauthorized
    require caller.region in [:eu, :uk]        else PolicyError.residency
    require caller.purpose in [:treatment]     else PolicyError.purpose

    // db.send_to(region:us, records) — COMPILE ERROR:
    //   MedicalRecord has @residency [eu, uk]; cannot exit region.

    return db.fetch_records(patient)
}
// Auditors get a machine-readable policy graph; agents writing new code
// inherit the constraints automatically through the type system.


// ----------------------------------------------------------------------------
// [33] INCREMENTAL TYPE-DRIVEN GENERATION
// `@generate` declares a target signature + intent + properties. The
// toolchain (LLM or otherwise) fills the body. Generation is bounded by the
// signature, the properties, and the examples — drift is structurally
// prevented. The body is "owned" by the generator; humans edit the spec.
// ----------------------------------------------------------------------------

@gid:fn.text.detect_language.v1
@intent      "Detect natural language of a text snippet (ISO 639-1)"
@pure        true
@generate    by: agent:claude
             prompt: "Detect language using n-gram frequency; no external IO"
@example     detect_language("the quick brown fox")     == "en"
@example     detect_language("le renard brun rapide")   == "fr"
@example     detect_language("der schnelle braune fuchs") == "de"
@property    forall s . detect_language(s).matches(/^[a-z]{2}$/)
@property    forall s where s.length > 50 . detect_language(s) != "unknown"
@budget      latency: 5ms, memory: 1.MiB
fn detect_language(text: String) -> String<iso639_1> {
    @body generated                      // body is regenerable from spec above
    ...
}
// To improve the implementation, you edit the @intent / @example / @property
// block — not the body. Regenerating preserves the contract by construction.


// ----------------------------------------------------------------------------
// [34] CROSS-LANGUAGE FFI WITH CONTRACT INHERITANCE
// External calls (C, Rust, Python, JS) declare their contracts in Glyph.
// The bridge enforces them at the boundary. Agents calling foreign code
// get the same verifiability guarantees as native code.
// ----------------------------------------------------------------------------

@gid:fn.image.resize.v1
@intent     "Resize an image; delegates to libvips via FFI"
@ffi        target: c
            library: "libvips.so.42"     @hash:blake3:f0a1...92be
            symbol:  "vips_resize"
@pure       true
@effects    []                            // declared pure across FFI boundary
@validates  inputs: at_boundary
@validates  outputs: at_boundary
@panics     never                         // panics in C => Err at boundary
fn resize(
    img: Image,
    width:  Int where width  in 1..16384,
    height: Int where height in 1..16384
) -> Image | ResizeError {
    @ffi_call {
        marshal img    -> VipsImage*
        marshal width  -> int
        marshal height -> int
        on_panic        => return ResizeError.foreign_panic
        on_null_return  => return ResizeError.foreign_null
    }
}
// The agent treats `resize` as a normal Glyph function with normal contracts.
// Memory ownership, panic safety, and ABI details live inside @ffi_call.


// ----------------------------------------------------------------------------
// [35] CHANGE PROPAGATION GRAPH
// The compiler maintains a live dataflow graph across the entire codebase.
// Any edit reports its blast radius: which callers, which tests, which
// docs, which downstream services are affected. Agents plan refactors with
// a full impact map — no more "find all references" approximations.
// ----------------------------------------------------------------------------

@gid:fn.user.normalize_email.v2
@intent     "Lowercase + trim email; v2 also strips +tags"
@pure       true
@delta_from v1 { behavior: "strips +tags before @" }
@impact     {
    callers:        47    enumerated below
    tests_affected: 12    enumerated below
    docs_affected:  3     enumerated below
    services_calling_via_api: [auth, billing, notifications]
    breaking_for_callers_that_rely_on_plus_tags: [analytics.dedup.v3]
}
fn normalize_email(e: String<email>) -> String<email> {
    return e
        |> trim
        |> lowercase
        |> strip_plus_tag        // new in v2
}
// Before merge, the agent (or human) sees the full impact block and either
// accepts each downstream effect or revises the change. The graph is updated
// atomically with the commit; CI will not let it drift.
```
