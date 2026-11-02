# Launch materials

Drafts for the preview announcement, rewritten against 0.1.128. Posting and
outreach are done by hand; nothing here is published by a workflow.

| File | What |
|---|---|
| [`announcement.md`](announcement.md) | The announcement post |
| [`show-hn.md`](show-hn.md) | Show HN title and body |
| [`release-notes-preview.md`](release-notes-preview.md) | Where the compiler stands at 0.1.128, for a first-time reader |
| [`onboarding-first-100.md`](onboarding-first-100.md) | How to find the first users and what to collect from them |

The rules these drafts follow:

- What Glyph catches is shown with the programs in [`catches/`](../../catches/),
  which `scripts/check_catches.py` runs both ways in CI. If a case is removed
  there, it comes out of these drafts too.
- The productivity hypothesis is described as unmeasured. No speedup figure
  appears anywhere.
- Every number comes from a file or a command. Before posting, re-read the
  counts (test suite, gap ledger, apps, release version) against the release
  being announced, since all of them move.
- The open edges are named in the post itself, starting with G257.
- Glyph code blocks are compiled by `scripts/check_docs_compile.py` like the
  rest of `docs/`.
