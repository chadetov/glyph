# Glyph annotation sketch — part 4 (abandoned direction)

> Pasted from conversation, 2026-05-26. Examples 16–20 in the same series as `glyph-annotation-sketch.md` (1–5), `pt2.md` (6–10), `pt3.md` (11–15). Same annotation-rich syntactic family. Same abandoned direction (current locked stance is TS-family).
>
> Ideas worth carrying forward are tracked in `docs/open-questions.md`. New questions surfaced by this file: Q24 (linear/owned resources), Q25 (compiler-enforced semver), Q26 (complexity annotations), Q27 (bidirectional functions). Plus a small extension to D9 (no `_ => ...` catch-all on sealed unions).

---

```glyph
// ============================================================================
// GLYPH — Five more functions demonstrating distinct advantages
// ============================================================================


// ----------------------------------------------------------------------------
// [16] OWNERSHIP & LINEAR RESOURCES
// Resources that must be closed, released, or consumed exactly once are
// tracked in the type system. The compiler rejects double-close, use-after-
// release, and forgotten-cleanup. Agents cannot generate leaky code because
// leaks are syntactically impossible.
// ----------------------------------------------------------------------------

@gid:fn.storage.stream_upload.v1
@intent  "Stream a file to object storage, guaranteeing handle closure"
@effects [fs.read, network.s3]
fn stream_upload(
    path: Path,
    bucket: BucketName,
    use fs: cap:fs.read,
    use s3: cap:s3.write
) -> UploadReceipt | UploadError {

    let owned handle: FileHandle = fs.open(path) ?? return propagate
    let owned upload: S3Upload   = s3.start_multipart(bucket, path.basename)
        ?? return propagate

    // `owned` values MUST be consumed exactly once on every path.
    // Forgetting to call `.close()` = compile error.
    // Calling `.close()` twice  = compile error.
    // Returning before consuming = compile error.

    return handle
        |> chunks    size: 8.MiB
        |> map_owned (chunk) => upload.put_part(chunk)   // consumes chunk
        |> collect   .parts
        |> consume   (parts) => upload.complete(parts)   // consumes upload
        |> finally   ()      => handle.close()           // consumes handle
}


// ----------------------------------------------------------------------------
// [17] SEMANTIC VERSIONING ENFORCED BY THE COMPILER
// Public API surface is computed from the AST. Removing a field, narrowing
// a type, or adding a required parameter forces a major bump — the compiler
// will not let an agent ship a "patch" that silently breaks downstream code.
// ----------------------------------------------------------------------------

@gid:module.users.public_api.v2
@semver 2.4.1
@public_surface auto                    // computed, not hand-maintained
module users {

    @gid:fn.users.find_by_id.v3
    @semver_change patch since 2.4.0    // body-only edit, no surface change
    @public
    fn find_by_id(id: UserId) -> User | NotFound { ... }

    @gid:fn.users.create.v5
    @semver_change minor since 2.4.0    // added optional field, additive only
    @public
    fn create(
        email: Email,
        name:  String,
        locale: String<bcp47> = "en-US"   // NEW, defaulted → minor bump
    ) -> User { ... }

    @gid:fn.users.delete.v2
    @semver_change major since 2.4.0    // return type widened → breaking
    @public
    @breaking_since 3.0.0
    fn delete(id: UserId) -> DeletionReceipt | NotFound | Forbidden {
        // Was: -> Bool   in 2.x. Compiler refused the 2.4.2 tag and
        // demanded a 3.0.0 release with a migration note.
        ...
    }
}
// On `glyph publish`, the toolchain diffs the AST against the registry and
// either accepts the version or rejects with: "field X removed → requires major".


// ----------------------------------------------------------------------------
// [18] DEAD-CODE & UNREACHABILITY ARE FIRST-CLASS
// Every branch is reachability-analyzed. `unreachable` is a real type with
// no inhabitants. Agents cannot leave dangling `else` arms, stale handlers,
// or copy-pasted dead code — the compiler deletes the ambiguity.
// ----------------------------------------------------------------------------

@gid:type.traffic_light.v1
type TrafficLight = sealed | Red | Yellow | Green

@gid:fn.traffic.next_state.v1
@intent "Advance the light; impossible inputs are statically impossible"
@pure   true
@total
fn next_state(current: TrafficLight, emergency: Bool) -> TrafficLight {
    if emergency { return Red }                  // override

    return match current {
        Red    => Green
        Green  => Yellow
        Yellow => Red
        // No `_ => ...` catch-all allowed on sealed types.
        // Adding `Flashing` to TrafficLight forces this match to update —
        // the compiler points to this exact line, not a runtime panic.
    }
}

@gid:fn.parser.handle_token.v1
@intent "Token dispatch with statically proven exhaustiveness"
@pure   true
fn handle_token(t: Token) -> Ast {
    return match t {
        Ident(name)    => Ast.var(name)
        Number(n)      => Ast.lit(n)
        Op(sym)        => Ast.op(sym)
        // `Eof` was removed from Token last week. The old handler:
        //   Eof => Ast.end()
        // is now flagged as `unreachable` and refuses to compile until removed.
    }
}


// ----------------------------------------------------------------------------
// [19] COST & COMPLEXITY ANNOTATIONS THE COMPILER VERIFIES
// Big-O, allocation count, and worst-case latency are declared and checked.
// Agents writing performance-critical code get immediate feedback when an
// "innocent" refactor (`.sort()` inside a loop) blows the contract.
// ----------------------------------------------------------------------------

@gid:fn.search.find_top_k.v2
@intent     "Return the top-k largest elements from an unsorted stream"
@pure       true
@complexity time: O(n log k)            // verified by static analysis
@complexity space: O(k)                 // not O(n) — the win
@allocations bounded: k + 1             // heap allocations capped
@worst_case latency: 50ms per 1M elements on ref-hardware
fn find_top_k(items: Stream<Int>, k: Int where k > 0) -> List<Int> {
    return items
        |> fold (heap, x) => heap.push_bounded(x, max: k)   // O(log k)
                from MinHeap.empty(capacity: k)
        |> drain_sorted .desc                               // O(k log k)
}
// If an agent rewrites the body using `.sort()`, the compiler emits:
//   FAIL @complexity  declared O(n log k), inferred O(n log n)
//   FAIL @space       declared O(k), inferred O(n)
// — caught at build time, not in production under load.


// ----------------------------------------------------------------------------
// [20] BIDIRECTIONAL FUNCTIONS (PARSE ↔ PRINT, ENCODE ↔ DECODE)
// One declaration generates BOTH directions and proves they round-trip.
// Agents stop writing the eternal pair of "serialize / deserialize" functions
// that drift apart over time. Drift is structurally impossible.
// ----------------------------------------------------------------------------

@gid:bifn.iso8601.v1
@intent   "Bidirectional ISO-8601 ↔ Timestamp"
@pure     true
@inverse  proven                        // compiler-checked round-trip
bifn iso8601 :: String<iso8601> <-> Timestamp {
    forward (s) -> Timestamp {
        return s |> lex.iso8601 |> build.timestamp
    }
    inverse (t) -> String<iso8601> {
        return t |> format.iso8601_extended
    }
    @property forall t: Timestamp . iso8601.forward(iso8601.inverse(t)) == t
    @property forall s: String<iso8601> . iso8601.inverse(iso8601.forward(s)) == s.canonical()
}

@gid:bifn.urlsafe_b64.v1
@intent   "URL-safe base64 encoding with proven round-trip"
@pure     true
@inverse  proven
bifn urlsafe_b64 :: Bytes <-> String<base64url> {
    forward (b) -> String<base64url> { return b |> encode.base64.urlsafe.no_pad }
    inverse (s) -> Bytes              { return s |> decode.base64.urlsafe.tolerant }
}

// Usage — the compiler picks the direction by type:
//   let s: String<iso8601> = iso8601(now())          // uses inverse
//   let t: Timestamp       = iso8601("2026-05-26")   // uses forward
// Refactoring one side automatically refactors the other; they cannot drift.
```
