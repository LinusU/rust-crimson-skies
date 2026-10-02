# M16-A-FU5: the three evidence reports that named only one agent now name both

Date: 2026-10-02. Task: M16-A-FU5 "Name the missing implementer or reviewer in
the three non-campaign evidence reports that name only one" (#484), filed by
M16-A-FU4 (#483) after it rewrote the ten reports that still carried a hand-over
placeholder. Shared contract: `docs/contracts/CLI-EVIDENCE.md` ("Evidence record
minimum": *reviewer identity/method*; "The reviewer regenerates the report on
the rebased commit and compares it"), plus the owner ruling of 2026-09-28
("Always record actual implementer/reviewer identities and whether the context
was fresh"). Capabilities used: none beyond reading this repository and the
Rally activity log - the review facts come from Rally, not from `$CS_GAME_DIR`,
and no report was regenerated. Implementer: **deepseek-1/deepseek-1** (session
of 2026-10-02T04:2xZ).

## The gap

M16-A-FU2 (#479) and M16-A-FU4 (#483) taught
`tools/tests/test_evidence_review_identity.py` to require every committed
evidence report to name the implementer and the reviewer. Three reports named
only one of the two and carried no hand-over placeholder, so they passed as
advisories rather than failures:

| report | committed `review.identity` said | merged |
| --- | --- | --- |
| `F05-D.json` | `bunny-1 (Rally reviewing agent for #24; regenerated on the rebased commit after the review fixes)` - a reviewer and no implementer | 2026-09-28T22:36:52Z |
| `F07-D.json` | `deepseek-1 (implementing agent)` - a bare agent name, no reviewer | 2026-09-28T21:26:53Z |
| `F31-D.json` | `implementer: deepseek-1, the agent that implemented and submitted #128` - no reviewer | 2026-10-01T09:35:14Z |

The task description named F07-D as Rally #128; the activity log shows F07-D is
**#32** and #128 is F31-D. The number was re-derived from each report's
`candidate_tree`, which is a tree object reachable from a ref in this checkout:

| report | `candidate_tree` | commit carrying that tree |
| --- | --- | --- |
| `F05-D.json` `9530120e…` | `db813b0aa4` "Regenerate the F05-D evidence report on the rebased tree" |
| `F07-D.json` `18ba5f14…` | `f8f15f79fe` "Audit INTERP opcode classes through the CLI and record the retail classification" |
| `F31-D.json` `6eafa516…` | `7ae081e53a` "Record module-prefixed F31-D unit tests in the evidence assertions" |

## What the Rally log actually says

Committed as `docs/findings/2026-10-02-m16-a-fu5-rally-review-snapshot.json`
(schema `rally-review-snapshot/1`), read through the `rally` MCP server on
2026-10-02T04:36Z. Rally actors are `<agent instance>/<session label>`, so two
claims from the same actor string are the same agent instance and the second is
not independent of the first.

| stage | implement claim | review claim | report written by | verdict |
| --- | --- | --- | --- | --- |
| F05-D (#24) | `bunny-1/bunny-1` 2026-09-28T20:29:00Z, submitted 21:20:37Z | `bunny-1/bunny-1` 21:20:49Z, merged 22:36:52Z | reviewer | same instance, 12 s after the hand-over |
| F07-D (#32) | `glm-1/deepseek-1` 2026-09-28T20:29:27Z, submitted 21:17:41Z | `glm-1/deepseek-1` 21:17:57Z, merged 21:26:53Z | implementer | same instance, 16 s after the hand-over |
| F31-D (#128) | `deepseek-1/deepseek-1` 2026-10-01T07:03:40Z, submitted 07:28:32Z | `deepseek-1/deepseek-1` 07:28:49Z, merged 09:35:14Z | reviewer | same instance, 17 s after the hand-over |

Every review claim follows its own hand-over by twelve to seventeen seconds, so
the activity log shows a second *claim* but cannot prove a fresh *context* on
its own. Each rewritten identity therefore states the gap in seconds and says
that no fresh context is claimed, which is the wording #483 established for the
non-campaign family.

`report_written_by` is derived from the report's `created_at` against the claim
windows above: F05-D's copy was regenerated on the rebased tree during its
review (the reviewer's own note records it), F31-D's final copy was written from
a review claim (its review "Resumed review" before regenerating), and F07-D's
copy was written at 21:16:26Z, inside the implement claim and before it was
submitted at 21:17:41Z.

## What changed

- `docs/findings/evidence/{F05-D,F07-D,F31-D}.json`: the `review.identity`
  value only. `candidate_tree`, the assertion list, the artifact hashes, the
  test counts, `review.method` and `claim` are byte-identical to what was
  committed.
- The two harness literals that write those reports, changed to exactly the
  bytes of the report: `tools/cs_inspect/src/interp.rs` (F07-D, a production
  source file) and `tools/cs_inspect/tests/evidence_report_f31_d.rs` (F31-D).
  F05-D has no committed harness on `main`, so there is no literal to move; the
  report says so by naming the implementer and reviewer directly.
- `docs/findings/2026-10-02-m16-a-fu5-rally-review-snapshot.json` (new): the
  facts above. A new snapshot rather than three more entries in the FU4 file,
  because the FU4 file records its own scope as "the stages whose committed
  `review.identity` was still a hand-over placeholder" and appending would make
  that sentence false.
- `tools/tests/test_evidence_review_identity.py`: the check reads the new
  snapshot too, and gains the M16-A-FU5 tests below.

## What deliberately did *not* change

- **No report was regenerated.** `docs/contracts/CLI-EVIDENCE.md` requires a
  report to match the tree that was tested, and this is a text fix, so re-running
  the four-step sequence would have invalidated `candidate_tree` and the recorded
  test counts for nothing.
- **`claim` stays `implemented` everywhere.** A merge awards `checked` only and
  no agent self-awards a level; the check enforces this.
- **`review.method` is untouched.** It describes the run each report records.
- **The eight reports that are legitimately outside the snapshots and the seven
  `CS_EVIDENCE_REVIEW` harnesses** are untouched. They remain advisories or
  `runtime`-shaped, as #483 recorded.

## The check

`python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v`
- 18 tests, 24 with the whole `tools/tests`. The reader now resolves the reports
against three snapshots instead of two, so the three stages stop being printed
as "has no Rally review facts in the snapshots yet". The new tests are:

- `test_accept_m16_a_fu5_snapshot_resolves_every_recorded_stage`: the FU5 tasks
  are unique, in Rally task order, disjoint from the other two snapshots, each
  covered by a report, with well-formed claim timestamps and a review event
  type the snapshot recognises.
- `test_accept_m16_a_fu5_the_three_stages_are_no_longer_advisories`: the whole
  corpus has no problems, and F05-D, F07-D and F31-D are absent from the
  advisory list and present in `reports`.
- `test_accept_m16_a_fu5_detects_drift_in_a_single_agent_report`: each of the
  three fails when the reviewer is renamed, when the pair is collapsed back to
  `none yet`, when the independence sentence is rewritten to `independent`, when
  the context statement is stripped, and when its harness literal drifts from
  the report.

It is checked, not hoped for. On the pre-change corpus (this branch's data
reverted, the check kept) `review_problems` reports the three reports by name -
each is missing the implementer or the reviewer, the statement about the
reviewer's context and the `not independent` statement - so
`test_accept_m16_a_fu5_the_three_stages_are_no_longer_advisories` fails; with
the fix it passes.

## Sources

`docs/contracts/CLI-EVIDENCE.md`; the owner directives of 2026-09-28 in the Rally
project instructions; the Rally activity log of tasks #24, #32 and #128; `git
log --all` for the report provenance; #483's findings note
`docs/findings/2026-10-02-m16-a-fu4-reviewer-identity-outside-the-campaign.md`
and snapshot `docs/findings/2026-10-02-m16-a-fu4-rally-review-snapshot.json`.
