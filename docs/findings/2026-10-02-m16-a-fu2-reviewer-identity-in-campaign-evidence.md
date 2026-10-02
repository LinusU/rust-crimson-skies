# M16-A-FU2: the campaign-binding evidence reports now name the real implementer and reviewer

Date: 2026-10-02. Task: M16-A-FU2 "Record the actual reviewer in the merged
M01-A..M13-A evidence reports" (#479, follow-up found while reviewing M16-A,
#303). Shared contract: `docs/contracts/CLI-EVIDENCE.md` ("Evidence record
minimum": *reviewer identity/method*; "The reviewer regenerates the report on
the rebased commit and compares it"), plus the owner ruling of 2026-09-28
("Always record actual implementer/reviewer identities and whether the context
was fresh"). Capabilities used: none beyond reading this repository — the
review facts come from the Rally activity log, not from `$CS_GAME_DIR`.
Implementer: **bunny-2/bunny-2** (session of 2026-10-02T01:14Z).

## The gap

Every campaign-binding report under `docs/findings/evidence/` is written by
`crates/cs_app/tests/campaign/evidence.rs` and puts a `review.identity` string
into the report. Five of the nine named no reviewer at all:

| report | committed `review.identity` said |
| --- | --- |
| `M03-A.json` | `reviewer: not yet assigned at hand-over` |
| `M04-A.json` | `reviewer: not yet assigned at hand-over` |
| `M08-A.json` | `reviewer: none yet` |
| `M12-A.json` | `reviewer: none yet` |
| `M13-A.json` | `reviewer: none yet` |

All nine of those stages are merged, and Rally recorded a review claim and a
merge event for every one of them, so "no reviewer" was not true at merge time.
`M01-A`, `M02-A`, `M05-A` and `M06-A` already named their reviewer but wrote
the implementer as a bare agent name (`opencode-1`, `bunny-alpha-2`,
`deepseek-1`) rather than the Rally actor string, so the two halves of the
sentence used two different naming conventions for the same agent.

## What the Rally log actually says

Committed as
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`, read through
the `rally` MCP server on 2026-10-02T01:22Z. Rally actors are
`<agent instance>/<session label>`, so two claims from the same actor string are
the same agent instance and the second is not independent of the first.

| stage | implementer claim | review claim | verdict |
| --- | --- | --- | --- |
| M01-A (#258) | `opencode-1/opencode-1` 05:06:06Z | `opencode-1/opencode-1` 06:47:47Z | same instance, not independent |
| M02-A (#261) | `bunny-alpha-2/bunny-alpha-2` 04:31:13Z | `bunny-alpha-2/bunny-alpha-2` 05:25:06Z | same instance, not independent |
| M03-A (#264) | `claude-2/claude-1` 19:24:58Z | `claude-2/claude-1` 19:35:53Z | same instance, not independent |
| M04-A (#267) | `deepseek-1/deepseek-1` 19:46:53Z | `deepseek-1/deepseek-1` 20:25:42Z | same instance, not independent |
| M05-A (#270) | `bunny-2/bunny-2` 20:17:31Z | `bunny-2/bunny-2` 21:04:12Z | same instance, not independent |
| M06-A (#273) | `deepseek-1/deepseek-1` 20:41:03Z | `deepseek-1/deepseek-1` 21:12:39Z | same instance, not independent |
| M08-A (#279) | `claude-2/claude-1` 23:16:51Z | `claude-2/claude-1` 23:50:00Z | same instance, not independent |
| M12-A (#291) | `claude-2/claude-1` 23:58:58Z | `claude-2/claude-1` 00:05:28Z | same instance, not independent |
| M13-A (#294) | `claude-2/claude-1` 00:10:14Z | `claude-2/claude-1` 00:15:25Z | same instance, not independent |

Not one of the nine campaign-binding stages was reviewed by a different agent
instance. That is the honest state of the record and it is now what the reports
say. Five reviews (`M01-A`, `M02-A`, `M05-A`, `M06-A` and, per the log, the
reviewers of the others) follow their own `submit_for_review` by 7 to 64
seconds, so the log shows a second *claim* but cannot prove a fresh *context*;
the rewritten identities for M03-A, M04-A, M08-A, M12-A and M13-A therefore
state that a fresh context is not claimed. `M01-A`'s existing wording ("a
separate session with fresh context") and `M02-A`'s ("the reviewer's context
was fresh … but a fresh context does not make a reviewer independent") were
left as their authors wrote them, and `M02-A`'s explicit statement that a fresh
context is not independence is the model the new wording follows.

## What changed

- `crates/cs_app/tests/campaign/evidence.rs`: the `jstr` `review.identity`
  literal of the M01-A, M02-A, M03-A, M04-A, M08-A, M12-A and M13-A harnesses.
  M05-A and M06-A already named the full actor and are untouched. The literals
  now name the implementer and the reviewer as the two Rally actor strings,
  say that the review is not independent, and say what is and is not claimed
  about the reviewer's context. Each keeps the sentence that the implementer's
  own run is not independent evidence.
- `docs/findings/evidence/{M01-A,M02-A,M03-A,M04-A,M08-A,M12-A,M13-A}.json`:
  the `review.identity` value, changed to exactly the bytes the matching
  harness writes. Nothing else in those files moved.
- `docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`: the review
  facts above, committed because Rally is not reachable from an offline check.
- `tools/tests/test_evidence_review_identity.py`: the check described below.

## What deliberately did *not* change

No report was regenerated. `docs/contracts/CLI-EVIDENCE.md` requires the report
to match the tree that was tested, and this change is not about the tested
code, so re-running the four-step sequence would have invalidated
`candidate_tree` and the recorded test counts for nothing. The recorded trees
are therefore left exactly as their runs produced them. For the record:

- All nine `candidate_tree` objects are still present in the object store
  (`git cat-file -t <tree>`), so no recorded hash is dangling.
- The commits whose trees are `M03-A` (`77bfbbfc…`), `M08-A` (`e7003e77…`) and
  `M13-A` (`0f944b4d…`) are no longer reachable from any ref: the lander
  rebased and merged them, so those three reports describe a tree that can no
  longer be checked out from this repository. They were not re-pointed. A
  reviewer who wants the report to describe a reachable commit must follow the
  four-step sequence in `evidence.rs` on the reviewed commit; that is a
  separate, deliberate act, not a side effect of this text fix.
- `claim` stays `implemented` in all nine. A merge awards `checked` only, and
  no agent self-awards a level; the check enforces this.
- `review.method` is untouched: it describes the run these reports record,
  which for the five rewrites is still the implementer's own run. What each
  reviewer actually did is in the Rally log, not in the report.

## The check

`python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v`

It reads the harness source and every committed report, resolves them against
the Rally snapshot and fails when a campaign-binding report

- still says a reviewer is missing (`none yet`, `not yet assigned`,
  `self-check`, …) while the snapshot records a review claim for that stage,
- does not contain the implementer's and the reviewer's Rally actor strings,
- does not say the review is not independent when the reviewer is the
  implementer's own instance,
- says nothing at all about the reviewer's context,
- claims a level other than `implemented`, or
- disagrees with the harness that writes it, so a regeneration cannot put a
  placeholder back.

A drift test mutates a report back to `reviewer: none yet`, renames the
implementer, deletes the "not independent" clause, drifts the harness literal
and awards `checked`, and requires each mutation to be reported.

Known limitation, filed as follow-up #480 (M16-A-FU3) rather than fixed here:
the CI workflow runs the Rust checks, the synthetic-fixture comparison and the
binary-file scan only, so this check runs when an agent runs it, not on every
push. `.github/workflows/ci.yml` is a protected path and this task did not touch
it. The same applies to `tools/tests/test_plan_sync.py` from AUDIT-PLAN-SYNC
(#356).
