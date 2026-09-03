# Impact before the edit

A companion demo to `../verifiability/`, scoped to the has_catch_all bug shape
0.1.107 was built against: a tagged union gains a variant, an exhaustive match
over it fails to compile, and a match with an `else` catch-all keeps compiling
and silently routes the new variant into the catch-all's branch. Nothing in
the build output points at the second site.

`fixture/main.glyph` declares `UserStatus` with two match sites over it
(`describe_exhaustive`, `describe_catchall`) plus an unrelated third match
over a second union, `Direction`, that also has a catch-all. `fixture/main_after.glyph`
is the same file with one line changed: `UserStatus` gains a `Suspended`
variant.

`measure.py` asks two ways which match sites over `UserStatus` are catch-all,
*before* the edit lands:

- **search alone**: grep the file for a catch-all arm (`else =>` / `_ =>`).
  It finds both catch-all sites, `describe_catchall` and `describe_direction`,
  because a text search has no notion of which union a match scrutinizes.
  One of the two is a false positive for anyone asking specifically about
  `UserStatus`.
- **`glyph_variants`**: the MCP tool shipped in 0.1.106, called with
  `{"path": "main.glyph", "name": "UserStatus"}`. It reports exactly the one
  site that matches on `UserStatus`; `describe_direction` never enters the
  answer, because the tool is scoped to the union in the request rather than
  to a syntactic shape.

Then it applies the edit for real (`main_after.glyph`) and checks the other
half of the bug shape through `glyph check --json`: the exhaustive site's
diagnostic (E0200) carries `entity: "main::describe_exhaustive"` (0.1.107),
and `describe_catchall` produces no diagnostic at all, anywhere in the
output, confirming the silent half of the bug still reproduces exactly as
described.

## Running it

```bash
./measure.py               # requires `glyph` on PATH, or set GLYPH=<path>
```

Writes `results/<timestamp>.json` and exits non-zero if any of the above
stops holding. Results are checked into git so the comparison is
reproducible rather than asserted.

## Reading the result

The numbers are precision over catch-all sites found for `UserStatus`, not a
claim about search tools in general: this fixture has one union, two
catch-all sites, and one of them belongs to a different union. Search alone
scores 0.5 (finds the real site, plus the unrelated one); `glyph_variants`
scores 1.0. The comparison is the ratio, not the absolute numbers, and it
only widens as a project grows more match sites and more unions: a text
search's false-positive rate rises with how many catch-alls exist anywhere in
the codebase, while `glyph_variants` stays scoped to the one union asked
about.
