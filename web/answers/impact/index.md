<!-- Markdown copy of https://glyphlang.io/answers/impact/, generated from the same source as the page. Last changed 2026-11-21. -->

21 · Before you edit

# Can the compiler tell me what a change will break before I make it?

For six kinds of edit, yes. `glyph_impact` takes the declaration you're about to change and the change itself (rename it, drop a variant, change a parameter's type, change its arity, remove it), and answers with **one verdict per site** from a closed list, each with the reason that verdict and no other.

A module declares `label(s: string) -> number`. Three callers reach it: one passes a string literal, one passes a value whose type is an alias for `string` imported from another module, and one reads `label` as a value and never calls it. Asking what `change_signature_type` does to each gets three different verdicts, because they're three different claims:

request

```
{
  "method": "tools/call",
  "params": {
    "name": "glyph_impact",
    "arguments": {
      "entity": "lib::label",
      "change": { "kind": "change_signature_type" }
    }
  }
}
```

result, trimmed to the three sites

```
{
  "entity": "lib::label",
  "impact": [
    {
      "entity": "literal::probe",
      "verdict": "WILL_FAIL",
      "diagnostic": "E0211",
      "because": "every argument at this site is paired with
        a parameter the relation reads in full, so a
        replacement parameter type any of those pairings does
        not accept is E0211 here. The pairings: argument 1
        (`\"x\"`, `\"x\"`) against parameter `s: string`: ..."
    },
    {
      "entity": "imported::probe",
      "verdict": "UNDETERMINED",
      "diagnostic": null,
      "because": "argument 1 (`c`, `Cell`) against parameter
        `s: string`: `Cell` is declared in another module as
        an alias for a primitive, and the checker compares an
        imported declaration only when the declaration at the
        end of its chain of names is a tagged union or a
        record (G215, G226), so a replacement is compared
        against nothing here and only `tsc` on a full `glyph
        build` would see a mismatch"
    },
    {
      "entity": "reader::sizer",
      "verdict": "NOT_INDEXED",
      "diagnostic": null,
      "because": "this site reads the name rather than
        applying it, so no argument is paired with a
        parameter here, and Glyph does not compare a
        function value's parameter types against the type
        its use context expects"
    }
  ]
}
```

Same edit, three sites, three verdicts, and the `because` on each is the actual sentence the compiler produces, not a category label.

**Details the compiler test this mirrors **

This mirrors the compiler's own test for the change (`a_signature_type_change_gets_one_verdict_per_kind_of_site` in `glyph-cli/tests/agent_loop.rs`): the test makes the edit afterward and checks that E0211 lands exactly where `WILL_FAIL` said it would, and that the compiler stays silent at the other two.

## Why it matters

Renaming a function, widening a union, or changing what a parameter accepts, you either grep for the old name and hope you found every call, or you make the edit and read what the compiler says afterward.

Neither approach tells you which sites will keep compiling while quietly becoming wrong: a `match` with a catch-all arm doesn't fail when you add a variant, it just starts routing the new case somewhere nobody chose. That site never shows up in a build. `glyph_impact` answers before the edit, over the same analysis the compiler runs.

## What comes back

The request names an entity by its `module::name` identity and a change from a closed list: `add_variant`, `remove_variant`, `rename`, `change_arity`, `change_signature_type`, `remove`. The verdicts are closed and each means one thing:

| Verdict | Meaning |

|---|---|

| `WILL_FAIL` | the compiler can prove the site stops compiling |
| `ABSORBS` | it can prove the change is taken here silently |
| `SAFE` | it can prove the site stays correct |
| `UNDETERMINED` | it looked at this site and can't establish the consequence |
| `NOT_INDEXED` | the question isn't askable for a site of this shape at all |

**Details why the change is required, and what each entry holds **

The change is required, because a verdict is a fact about an edit; with no edit named, every site would come back as a bare reference instead of a consequence. The answer names the entity, the change, and an `impact` list: one entry per site, each with the declaration it sits in, a `relation`, a `verdict`, a `because`, and the diagnostic code the compiler raises when there is exactly one.

## On a real application

`examples/apps/csvql`, an eleven-file CSV query engine in this repository, has one tagged union, `Value`, with four variants today. Asking `glyph_impact` what adding a fifth (`Blob`) does finds 10 match sites across 4 files: 8 will fail compilation, and 2 contain a catch-all arm that will silently absorb the new variant.

- **Checked by the build** Adding the variant for real, the compiler reports exactly 8 `E0200` failures, at exactly the 8 predicted sites, and says nothing at the 2 `ABSORBS` sites, before or after.
- **A text search instead** A text search for catch-all arms across the same app, the thing you'd do without the tool, finds those same 2 real sites buried in 20 hits (precision 0.10), and says nothing at all about the 8 that fail, because it has no notion of which type a `match` scrutinizes.

## Where it stands

### Shipping today

- Six closed change kinds (`add_variant`, `remove_variant`, `rename`, `change_arity`, `change_signature_type`, `remove`) and five closed verdicts, one entry per affected site with its reason and diagnostic code.
- `change_signature_type` is decided per call argument against the checker's own comparison rules, and a site is the weakest of its arguments.
- A JSX element is decided as the call it lowers to

  **Details**

  A JSX element is decided as the call it lowers to: `<Card title="x" />` is one props record passed to `Card`, and each attribute is listed against the field it is checked against, with the verdict a call argument of the same type would get.

- Coverage is stated per search, and every node carries where it came from

  **Details**

  Coverage (what a search could and couldn't read) is stated per search rather than once for the whole answer, and every node carries whether it came from Glyph source, an `extern_ts` escape, or an opaque `.d.ts`.

### Where it stops

- Depth is exact at one hop and empty past it

  **Details**

  Depth is exact at one hop and empty past it: Glyph never infers a declaration's type from its body, so a callee's type can't reach a caller's signature, and a change to X can only invalidate expressions that name X.

- Ask for depth 2 and the answer comes back with `next_query` naming the exact question instead

  **Details**

  Ask for depth 2 and the answer comes back with `next_query` naming the exact question instead, because what a second hop needs is the repair someone gives the first site, and that repair doesn't exist yet.

- `UNDETERMINED` and `NOT_INDEXED` are different claims

  **Details**

  `UNDETERMINED` and `NOT_INDEXED` are different claims: an argument the checker holds no type for, or a record pairing with a field no rule reads, is `UNDETERMINED` (it looked and couldn't decide); a function read as a value instead of called is `NOT_INDEXED` (the question was never askable there).

- `ABSORBS` is the one to watch: the site keeps compiling and is proved wrong anyway, which a green build will never show you.

Code examples on this page are checked against Glyph 0.1.155 on every build. Last changed 21 Nov 2026.
