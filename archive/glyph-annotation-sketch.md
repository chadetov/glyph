# Glyph annotation sketch (abandoned direction)

> Pasted from conversation, 2026-05-26. An earlier — or parallel — exploration of Glyph that prioritized annotation-rich, signature-heavy, agent-readable contracts. Same family as `glyph-session.md`, different syntax. Both directions were superseded by the current TS-family stance.
>
> What's preserved here is the source material verbatim. The ideas worth carrying forward are tracked in `docs/open-questions.md` (Q11 testing model, Q13 stable IDs, Q14 design-by-contract, Q15 refined types).

---

```glyph
// ============================================================================
// GLYPH — A programming language optimized for AI agents
// Design pillars: abstraction · verifiability · diff stability · greppability
// ============================================================================


// ----------------------------------------------------------------------------
// [1] GREPPABILITY + DIFF STABILITY
// Every symbol has a globally unique, stable ID (@gid). Renames don't cascade.
// Every function declares its contract inline. One concept = one grep.
// ----------------------------------------------------------------------------

@gid:fn.auth.verify_token.v3
@intent  "Verify a JWT and return the authenticated principal"
@inputs  token: String<jwt>
@outputs Principal | AuthError
@pure    false                              // touches clock, key store
@effects [time.now, keystore.read]          // explicit, machine-readable
@raises  [TokenExpired, SignatureInvalid, KeyNotFound]
@invariant out.principal.id != null when out is Principal
@since   2026-03-14
fn verify_token(token: String<jwt>) -> Principal | AuthError {
    let parts    = token.split(".")            ?? return SignatureInvalid
    let header   = base64.decode_json(parts[0]) ?? return SignatureInvalid
    let payload  = base64.decode_json(parts[1]) ?? return SignatureInvalid
    let key      = keystore.fetch(header.kid)   ?? return KeyNotFound

    require crypto.verify(key, parts[0..1], parts[2]) else SignatureInvalid
    require payload.exp > time.now()            else TokenExpired

    return Principal { id: payload.sub, scopes: payload.scopes }
}


// ----------------------------------------------------------------------------
// [2] VERIFIABILITY
// Pre/post-conditions are part of the syntax, not comments.
// The compiler emits proof obligations; the AI agent reads them as feedback.
// ----------------------------------------------------------------------------

@gid:fn.math.safe_divide.v1
@intent "Integer division that never panics"
@pure   true
fn safe_divide(numerator: Int, denominator: Int) -> Int | DivByZero
    requires denominator != 0 or returns DivByZero
    ensures  result is Int   implies result * denominator <= numerator
    ensures  result is Int   implies abs(result) <= abs(numerator)
{
    if denominator == 0 { return DivByZero }
    return numerator / denominator
}


// ----------------------------------------------------------------------------
// [3] ABSTRACTION
// Pipelines read top-to-bottom, no nesting, no hidden state.
// Each `|>` stage is independently testable AND independently editable —
// a diff that changes stage 3 cannot textually touch stages 1, 2, 4, 5.
// ----------------------------------------------------------------------------

@gid:fn.orders.compute_invoice.v2
@intent "Build a finalized invoice from a cart"
@pure   true
fn compute_invoice(cart: Cart, customer: Customer) -> Invoice {
    return cart.items
        |> filter   (item)        => item.qty > 0
        |> map      (item)        => price_line(item, customer.tier)
        |> reduce   (acc, line)   => acc.add(line)         from Invoice.empty()
        |> apply    (inv)         => inv.with_tax(customer.region)
        |> apply    (inv)         => inv.with_discount(customer.loyalty)
        |> finalize (inv)         => inv.seal(time.now())
}


// ----------------------------------------------------------------------------
// [4] DIFF STABILITY
// Fields are addressed by @fid, not by position. Adding/removing fields
// produces a one-line diff. AI agents never get "merge conflict" cascades.
// ----------------------------------------------------------------------------

@gid:type.user.profile.v4
type UserProfile {
    @fid:001 id:           UserId
    @fid:002 email:        String<email>
    @fid:003 display_name: String<1..64>
    @fid:004 created_at:   Timestamp
    @fid:007 mfa_enabled:  Bool            = false      // added in v4
    @fid:008 locale:       String<bcp47>   = "en-US"    // added in v4
    // @fid:005 and @fid:006 were retired; numbers are never reused.
}

@gid:fn.user.upgrade_profile.v1
@intent "Idempotently enable MFA and set locale on an existing profile"
@pure   false
@effects [db.write]
fn upgrade_profile(p: UserProfile, locale: String<bcp47>) -> UserProfile {
    return p with {
        @fid:007 = true
        @fid:008 = locale
    }
}


// ----------------------------------------------------------------------------
// [5] AI-NATIVE: machine-readable scaffolding
// `@example` blocks ARE the unit tests. `@reasoning` is parsed by agents.
// The agent can rewrite the body and the compiler will verify all @examples
// still pass without the agent ever running a separate test harness.
// ----------------------------------------------------------------------------

@gid:fn.text.slugify.v2
@intent "Turn human text into a URL-safe slug"
@pure   true
@reasoning {
    step "lowercase first"        because "case-insensitive matching"
    step "strip diacritics"       because "ASCII-only URLs"
    step "collapse whitespace"    because "consecutive spaces → single dash"
    step "drop disallowed chars"  because "RFC 3986 safe set"
}
@example  slugify("Hello, World!")           == "hello-world"
@example  slugify("  Café   au   lait  ")    == "cafe-au-lait"
@example  slugify("Onur's Glyph v1.0")       == "onurs-glyph-v1-0"
@example  slugify("")                        == ""
@property forall s: String . slugify(s).matches(/^[a-z0-9-]*$/)
@property forall s: String . !slugify(s).contains("--")
fn slugify(s: String) -> String {
    return s
        |> normalize  .nfkd
        |> filter     (c) => c.is_ascii()
        |> lowercase
        |> replace    /[^a-z0-9]+/ with "-"
        |> trim       "-"
}
```
