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
into the report. Nine of them named no reviewer at all:

| report | committed `review.identity` said | merged |
| --- | --- | --- |
| `M03-A.json` | `reviewer: not yet assigned at hand-over` | 2026-10-01T19:46Z |
| `M04-A.json` | `reviewer: not yet assigned at hand-over` | 2026-10-01T20:46Z |
| `M07-A.json` | `reviewer: not yet assigned at hand-over` | 2026-10-02T01:58Z |
| `M08-A.json` | `reviewer: none yet` | 2026-10-01T23:58Z |
| `M12-A.json` | `reviewer: none yet` | 2026-10-02T00:10Z |
| `M13-A.json` | `reviewer: none yet` | 2026-10-02T00:34Z |
| `M19-A.json` | `reviewer: none yet (the implementer's own run only; an independent review is pending)` | 2026-10-02T02:17Z |
| `M21-A.json` | `reviewer: none yet (the implementer's own run only; an independent review is pending)` | 2026-10-02T02:29Z |
| `M24-A.json` | `reviewer: none yet (the implementer's own run only; an independent review is pending)` | 2026-10-02T02:40Z |

Every one of those stages was reviewed and merged, so "no reviewer" was not true
at merge time. `M01-A`, `M02-A` and `M06-A` already named their reviewer but
wrote the implementer as a bare agent name (`opencode-1`, `bunny-alpha-2`,
`deepseek-1`) rather than the Rally actor string, so the two halves of the
sentence used two different naming conventions for the same agent. M05-A
already used the actor string on both halves and needed nothing. M16-A (#303)
landed on `main` while this task was running and already named a reviewer from a
different agent instance; it only needed the same naming normalization.

## What the Rally log actually says

Committed as
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`, read through
the `rally` MCP server on 2026-10-02T01:22Z and re-read at 02:10Z for the two
stages that landed meanwhile. Rally actors are
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
| M07-A (#276) | `deepseek-1/deepseek-1` 22:49Z | `deepseek-1/deepseek-1` 23:36:18Z, then `claude-2/claude-1` 00:57:15Z and 01:39:38Z | own instance first, another instance last; the merge is `claude-2/claude-1`'s |
| M08-A (#279) | `claude-2/claude-1` 23:16:51Z | `claude-2/claude-1` 23:50:00Z | same instance, not independent |
| M12-A (#291) | `claude-2/claude-1` 23:58:58Z | `claude-2/claude-1` 00:05:28Z | same instance, not independent |
| M13-A (#294) | `claude-2/claude-1` 00:10:14Z | `claude-2/claude-1` 00:15:25Z | same instance, not independent |
| M16-A (#303) | `claude-2/claude-1` 00:23:41Z | `bunny-alpha-1/bunny-alpha-1` 00:58:00Z | different instance, fresh context |
| M19-A (#312) | `claude-2/claude-1` 02:05:09Z | `claude-2/claude-1` 02:12:38Z | same instance six seconds after the hand-over: the same session continuing |
| M21-A (#318) | `claude-2/claude-1` 02:17:45Z | `claude-2/claude-1` 02:22:50Z | same instance nine seconds after the hand-over: the same session continuing |
| M24-A (#327) | `claude-2/claude-1` 02:30:05Z | `claude-2/claude-1` 02:38:40Z | same instance nine seconds after the hand-over: the same session continuing |

Twelve of the fourteen stages this snapshot records were reviewed by the same
agent instance that implemented them. That is the honest state of the record and
it is now what the reports say. M07-A and M16-A are the two exceptions: both were
reviewed by an instance other than the implementer's, and both reports now say
so — M16-A already did, M07-A did not and is fixed here. M17-A (#306), which
landed after this check was on `main` and is deliberately not in the snapshot,
is a same-instance review too, so the campaign corpus is thirteen of fifteen.

Every review claim in the table follows its own `submit_for_review` by 6 seconds
to just over two hours (the long one is M07-A, whose first landing attempt failed
on a rebase conflict), so the log shows a second *claim* but cannot prove a fresh
*context* on its own; the rewritten identities for M03-A, M04-A, M08-A, M12-A,
M13-A, M19-A, M21-A and M24-A therefore state that a fresh context is not
claimed, and M19-A, M21-A and M24-A say outright that their review claim came
seconds after the hand-over and is the same session continuing.
`M01-A`'s existing wording ("a separate session with fresh context"),
`M02-A`'s ("the reviewer's context was fresh … but a fresh context does not make
a reviewer independent"), M07-A's and M16-A's ("a separate session with fresh
context" / "fresh context and a different agent instance from the
implementer's") were left as their authors wrote them, and `M02-A`'s explicit
statement that a fresh context is not independence is the model the new wording
follows.

M07-A is also the one stage with more than one review claim: the lander could
not apply it twice ("rebase conflict with main; a reviewer must rebase it by
hand"), so `deepseek-1/deepseek-1` approved it, then `claude-2/claude-1`
approved it twice and merged it. The snapshot keeps all three claims and the
report names both agents.

## What changed

- `crates/cs_app/tests/campaign/evidence.rs`: the `jstr` `review.identity`
  literal of the M01-A, M02-A, M03-A, M04-A, M06-A, M07-A, M08-A, M12-A, M13-A,
  M16-A, M19-A, M21-A and M24-A harnesses, plus a module-doc paragraph telling a
  new stage to add
  itself to the snapshot. M05-A already named the full actor on both halves and
  is untouched. The literals now name the implementer and the reviewer as the two
  Rally actor strings, say that the review is not independent, and say what is
  and is not claimed about the reviewer's context. Each keeps the sentence that
  the implementer's own run is not independent evidence.
- `docs/findings/evidence/{M01-A,M02-A,M03-A,M04-A,M06-A,M07-A,M08-A,M12-A,M13-A,M16-A,M19-A,M21-A,M24-A}.json`:
  the `review.identity` value, changed to exactly the bytes the matching
  harness writes. Nothing else in those files moved. M07-A's and M16-A's
  reports were written by their reviewers and their `candidate_tree` is the
  reviewed commit; only the reviewer string changed. M19-A's report was written
  by the implementer and its reviewer never regenerated it, so its identity now
  names the same-instance review that actually merged it.
- `docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`: the review
  facts above, committed because Rally is not reachable from an offline check.
- `tools/tests/test_evidence_review_identity.py`: the check described below.

## What deliberately did *not* change

No report was regenerated. `docs/contracts/CLI-EVIDENCE.md` requires the report
to match the tree that was tested, and this change is not about the tested
code, so re-running the four-step sequence would have invalidated
`candidate_tree` and the recorded test counts for nothing. The recorded trees
are therefore left exactly as their runs produced them. For the record:

- All fourteen `candidate_tree` objects are still present in the object store
  (`git cat-file -t <tree>`), so no recorded hash is dangling.
- The commits whose trees are `M03-A` (`77bfbbfc…`), `M08-A` (`e7003e77…`) and
  `M13-A` (`0f944b4d…`) are no longer reachable from any ref: the lander
  rebased and merged them, so those three reports describe a tree that can no
  longer be checked out from this repository. They were not re-pointed. A
  reviewer who wants the report to describe a reachable commit must follow the
  four-step sequence in `evidence.rs` on the reviewed commit; that is a
  separate, deliberate act, not a side effect of this text fix.
- `claim` stays `implemented` in all fourteen. A merge awards `checked` only, and
  no agent self-awards a level; the check enforces this.
- `review.method` is untouched: it describes the run these reports record,
  which for the rewrites is still the implementer's own run. What each reviewer
  actually did is in the Rally log, not in the report.

## The check

`python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v`

It reads the harness source and every committed report and works in two levels.

Every campaign-binding report, snapshot-backed or not, must

- carry no placeholder (`none yet`, `not yet assigned`, `an independent review is
  pending`, `self-check`, …): Rally only merges a stage through `complete_review`
  or its landing queue, so a committed report has always had a reviewer,
- say something about the reviewer's context,
- have a non-empty `review.method`,
- claim `implemented` and nothing above it — a merge awards `checked` only and
  no agent self-awards a level, and
- carry a `review.identity` byte-identical to the harness literal that writes
  it, so a regeneration cannot put a placeholder back.

A stage recorded in the snapshot must additionally

- name the implementer's and every reviewer's Rally actor string the snapshot
  records, and
- say the review is not independent when the reviewer is the implementer's own
  instance.

A stage the snapshot has not caught up with is an advisory, not a failure: a
correct new report must not turn an unrelated branch red while the campaign
lands stages continuously. A test pins both halves of that — a correct unrecorded
stage produces no problem, and the same stage claiming `reviewer: none yet`
produces one.

M17-A (#306) and M16-A-FU1 (#478) are the two stages that landed with the check
already on `main`, and they are what the two-level design is for. Both name an
implementer and a reviewer, both say in words that the review is not
independent, and both use a bare agent name, so the check reports each as an
advisory ("no Rally facts in the snapshot yet") and passes. M17-A is
deliberately left out of the snapshot, with its `reviewing agent:` wording,
rather than rewritten in a task about the records that were wrong. Normalizing
either one later is the same one-entry, one-line recipe as the rest.

M16-A-FU1 earned its advisory by exposing a false positive in the check itself.
It wrote "same agent and model, fresh session", which answers the owner
ruling's question — whether the reviewer's context was fresh — in the words a
report actually uses, and the check failed it anyway because it looked for the
literal token `context`. A check that fails an honest record teaches its readers
to distrust it, so the rule now accepts `context` or `fresh`, with a test on
each side of it. Nothing was changed in M16-A-FU1's report: it is task #478's
file and #478 was still in review when this branch was rebased.

The drift tests mutate a report back to `reviewer: none yet`, rename the
implementer, delete the "not independent" clause, strip the statement about the
reviewer's context, drift the harness literal and award `checked`, and require
each mutation to be reported; others parse the harness's Rust string literals
out of the source so the reader itself is covered, pin the pending-review
template and pin both readings of the context rule.

The check earned its place four times on its first day: M07-A, M19-A, M21-A and
M24-A all merged during this task while their review had already run and merged
them, M07-A with `not yet assigned at hand-over` and the other three with
`reviewer: none yet (… an independent review is pending)`. Each one failed the
check the moment `main` moved, which is how all four landed in this branch
instead of surviving into the next campaign stage. Four out of four is a
template problem, not four coincidences: a new stage's author pastes a
hand-over identity into a new harness, and nobody notices the wording until
something checks it. Keeping the snapshot current is one JSON entry and one
identity line per stage; the harness module doc and the test docstring say so
where the next stage's author reads it, and #480 (M16-A-FU3) is meant to make the
check run on every push instead of only when an agent happens to run it.

Known limitation, filed as follow-up #480 (M16-A-FU3) rather than fixed here:
the CI workflow runs the Rust checks, the synthetic-fixture comparison and the
binary-file scan only, so this check runs when an agent runs it, not on every
push. `.github/workflows/ci.yml` is a protected path and this task did not touch
it. The same applies to `tools/tests/test_plan_sync.py` from AUDIT-PLAN-SYNC
(#356). #480 is **blocked, not pending**: its change is written but unpushed
because the agent token cannot write `.github/workflows/ci.yml`, so until the
owner pushes it nothing enforces this check and a campaign stage can still land
with the placeholder. Nothing about that gap is closed by this task, and the
next stage's reviewer should expect to be the one who catches it.

The check is also scoped to one harness family. It reads
`crates/cs_app/tests/campaign/evidence.rs` and nothing else, so the other
committed reports under `docs/findings/evidence/` keep the hand-over wording
their tasks shipped with (`F02-B`, `F02-C`, `F02-D`, `F04-D`, `F12-K`, `T340`,
`T343`, `T344` all still say `self-check` or `not yet independently reviewed`).
That is the same defect this task removes, in a family it deliberately did not
touch, and it is filed as #483 (M16-A-FU4) rather than fixed here.

## Review

Reviewed on 2026-10-02 by **bunny-2/bunny-2**, the same agent instance that
implemented it, so this is a self-review and is not independent evidence in the
owner directive's sense. The snapshot was re-read against the Rally history of
all fourteen tasks and every actor, claim timestamp and merge event matches; the
`candidate_tree` claims were re-derived from the object store and match; and no
protected path is touched. The following were wrong and are fixed here:

- M07-A's committed `review.identity` called itself "the only campaign-binding
  stage reviewed by another instance". M16-A is too (`bunny-alpha-1` reviewed
  `claude-2`'s work), so the claim is false and, being a superlative about a
  corpus that grows, it would have kept going stale. Removed.
- M24-A's `review.identity` said its review claim started "ten seconds" after the
  hand-over. The recorded gap is 9.495 s (`task.submitted` 02:38:30.683Z,
  `review.started` 02:38:40.178Z), so it is now nine seconds. M19-A's six and
  M21-A's nine were re-derived and are right.
- The harness module doc claimed the check "fails until a new stage is" in the
  snapshot. The shipped design deliberately makes an unrecorded stage an
  advisory so a correct new stage cannot turn an unrelated branch red. The doc
  now describes the two levels, which is what the next stage's author reads.
- The "What changed" section said M05-A and M06-A already named the full actor
  and were untouched. M05-A did; M06-A named the reviewer `deepseek-1/deepseek-1`
  and the implementer `deepseek-1` in the same sentence, the exact defect the
  note opens with, so M06-A is normalized too.
- This note said every review claim followed its submit "by 6 seconds to 55
  minutes". M07-A's last review claim is 2 h 03 m after its submit.
- `PLACEHOLDERS` did not contain the wording three of the four stages that landed
  during this task actually shipped with (`an independent review is pending`), so
  a stage keeping that sentence but dropping `none yet` would have passed. Added,
  with a test.
- The completeness rule that an identity must say something about the reviewer's
  context matched the literal token `context`, and M16-A-FU1 (#478), which
  landed on `main` during this rebase, writes "fresh session". That is a correct
  report failing a check, so the rule now accepts either word, with a test on
  each side of it and a drift assertion that it still fires when the statement
  is gone.

Judgement calls left as the implementer made them, because a report is the
author's own account of their own session and rewriting it is a separate act:
M01-A, M02-A and M17-A assert a fresh context from a 64 s, a 13 s and a 17 s gap
respectively, and M17-A keeps a bare agent name and the label `reviewing agent:`
and stays out of the snapshot. The check passes all three.
