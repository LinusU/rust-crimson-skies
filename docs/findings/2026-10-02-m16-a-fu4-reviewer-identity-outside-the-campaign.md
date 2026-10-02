# M16-A-FU4: the non-campaign evidence reports now name the real implementer and reviewer

Date: 2026-10-02. Task: M16-A-FU4 "Name the real implementer and reviewer in the
non-campaign evidence reports" (#483), filed while reviewing M16-A-FU2 (#479).
Shared contract: `docs/contracts/CLI-EVIDENCE.md` ("Evidence record minimum":
*reviewer identity/method*; "The reviewer regenerates the report on the rebased
commit and compares it"), plus the owner ruling of 2026-09-28 ("Always record
actual implementer/reviewer identities and whether the context was fresh").
Capabilities used: none beyond reading this repository and the Rally activity
log — the review facts come from Rally, not from `$CS_GAME_DIR`, and no report
was regenerated. Implementer: **bunny-2/bunny-2** (session of
2026-10-02T03:55Z), the same agent instance that implemented and reviewed
#479.

## The gap

#479 added `tools/tests/test_evidence_review_identity.py` so the
campaign-binding reports would stop saying "no reviewer". It scoped itself to
the one harness family it could read, `crates/cs_app/tests/campaign/evidence.rs`,
because that is where the campaign reports come from. The same defect therefore
survived untouched in the rest of `docs/findings/evidence/`: eight reports still
carried a hand-over sentence for a review that has since run and merged.

| report | committed `review.identity` said | merged |
| --- | --- | --- |
| `F02-B.json` | `opencode-1 (implementing agent, self-check; the Rally reviewer regenerates this report on the rebased commit)` | 2026-09-24T01:31Z |
| `F02-C.json` | `devin-1 (implementing agent, self-check; …)` | 2026-09-24T04:38Z |
| `F02-D.json` | `devin-1 (implementing agent, self-check; …)` | 2026-09-24T05:24Z |
| `F04-D.json` | `claude-1 (implementing agent, self-check; …)` | 2026-09-28T16:03Z |
| `F10-D.json` | `… an independent reviewer with a fresh context **must** re-run the four steps above on the rebased commit and compare the regenerated report before anything is merged` | 2026-09-30T01:31Z |
| `F12-K.json` | `implementer: devin-1 (SWE-2 High) …; **not yet independently reviewed** — the Rally reviewer's identity and fresh-context status are recorded at review time` | 2026-09-29T16:57Z |
| `F51-D.json` | `… A separate Rally review claim owns the independent check … and **must record its own identity** there` | 2026-10-01T14:04Z |
| `T340.json` | `claude-1 (implementing agent, self-check; …)` | 2026-09-28T17:09Z |
| `T343.json` | `claude-1 (implementing agent, self-check; …)` | 2026-09-28T17:24Z |
| `T344.json` | `claude-1 (implementing agent, self-check; …)` | 2026-09-28T17:38Z |

`F10-D` and `F51-D` are the same defect written as an order rather than as a
claim — "the reviewer must re-run this", "the reviewer must record its identity
there" — which is why a rule that only looked for `none yet` would have missed
them. They are in this task's scope because the task says "plus any other
`docs/findings/evidence/*.json` still containing a hand-over placeholder", and
because both statements are now false: both stages were reviewed and merged by
the agent that had written them.

## What the Rally log actually says

Committed as
`docs/findings/2026-10-02-m16-a-fu4-rally-review-snapshot.json`, read through
the `rally` MCP server on 2026-10-02T04:00Z. Rally actors are
`<agent instance>/<session label>`, so two claims from the same actor string are
the same agent instance and the second is not independent of the first.

| stage | implement claim | review claim | report written by | verdict |
| --- | --- | --- | --- | --- |
| F02-B (#10) | `opencode-1/opencode-1` 2026-09-23T21:01:57Z | `opencode-1/opencode-1` 22:09:20Z (lease expired), then `Jakob - Devin SWE-2/devin-1` 2026-09-24T01:01:53Z | reviewer | different instance for the review that merged it |
| F02-C (#11) | `Jakob - Devin SWE-2/devin-1` 2026-09-24T03:50:01Z, resumed from `opencode-1/opencode-1` 01:59:10Z | `Jakob - Devin SWE-2/devin-1` 04:10:58Z | implementer | same instance, 14 s after the hand-over |
| F02-D (#12) | `Jakob - Devin SWE-2/devin-1` 2026-09-24T04:38:43Z | `Jakob - Devin SWE-2/devin-1` 04:57:27Z | implementer | same instance, 19 s after |
| F04-D (#20) | `claude-1/claude-1` 2026-09-28T15:30:32Z | `claude-1/claude-1` 15:54:42Z | reviewer | same instance, 17 s after |
| F10-D (#44) | `mimo-1/mimo-1` 2026-09-29T22:58:01Z, resumed from `deepseek-1/deepseek-1` 21:22:44Z | `mimo-1/mimo-1` 2026-09-30T00:36:04Z | reviewer | same instance, 66 s after |
| F51-D (#207) | `deepseek-1/deepseek-1` 2026-10-01T13:02:33Z | `deepseek-1/deepseek-1` 13:46:16Z | implementer | same instance, 17 s after |
| T340 (#340) | `claude-1/claude-1` 2026-09-28T16:48:36Z | `claude-1/claude-1` 17:08:00Z | implementer | same instance, 18 s after |
| T343 (#343) | `claude-1/claude-1` 2026-09-28T17:11:16Z | `claude-1/claude-1` 17:21:15Z | implementer | same instance, 15 s after |
| T344 (#344) | `claude-1/claude-1` 2026-09-28T17:25:02Z | `claude-1/claude-1` 17:35:32Z | implementer | same instance, 15 s after |
| F12-K (#377) | `Jakob - Devin SWE-2/devin-1` 2026-09-29T15:27:43Z | `Jakob - Devin SWE-2/devin-1` 16:45:28Z | implementer | same instance, 41 s after |

Nine of the ten stages were reviewed by the same agent instance that implemented
them; F02-B is the exception, and its report says so. Every review claim follows
its own `submit_for_review` by fourteen to sixty-six seconds, so the log shows a
second *claim* but cannot prove a fresh *context* on its own. Every rewritten
identity therefore states the gap in seconds and says that no fresh context is
claimed, which is the wording #479 established for the campaign family and its
own `M02-A` note models ("a fresh context does not make a reviewer independent").

`report_written_by` is not an assumption. Each report's own `created_at` is
placed against the claim windows above, and it agrees with `git log` on the
report file: F02-B's copy was committed as the merge commit
`48da095`… with `created_at` 01:11:16Z, inside devin-1's review claim, and that
agent's own note says it "regenerated and validated" the report; F04-D's and
F10-D's copies are the reviewed tree's ("Regenerate the F04-D evidence copy on
the reviewed tree", "Record the F10-D evidence regenerated on the rebased
commit"); F51-D's reviewer wrote "regenerated evidence (matched)", which is why
that report is still the implementer's bytes. The other five `created_at`
values all fall inside their implement claim, before the review claim started.

All ten `candidate_tree` objects are present in the object store **and** belong
to a commit reachable from a ref (`git log --all --format='%T %H'`), which is
better than the campaign case where #479 had to record three unreachable trees.
Nothing was re-pointed.

## What changed

- `docs/findings/evidence/{F02-B,F02-C,F02-D,F04-D,F10-D,F12-K,F51-D,T340,T343,T344}.json`:
  the `review.identity` value only. `candidate_tree`, the assertion list, the
  artifact hashes, the test counts, `review.method` and `claim` are byte-identical
  to what was committed.
- The nine harness literals that write those reports, changed to exactly the
  bytes of the report: `crates/cs_assets/tests/evidence_report_f02_b.rs`,
  `tools/cs_inspect/tests/evidence_report_f02_c.rs`,
  `tools/cs_inspect/tests/evidence_report_f02_d.rs`,
  `crates/cs_assets/tests/evidence_report_f04_d.rs`,
  `crates/cs_formats/tests/gamez/evidence.rs`, `crates/cs_app/tests/text/evidence.rs`
  (inside `fn review_identity()`), `crates/cs_formats/tests/zbd/t340.rs`,
  `t343.rs` and `t344.rs`. F12-K's harness is `private/evidence/F12-K/harness.py`
  and is not in Git, so there is no literal to move; the report says so.
- `docs/findings/2026-10-02-m16-a-fu4-rally-review-snapshot.json` (new): the facts
  above, committed because Rally is not reachable from an offline check.
- `tools/tests/test_evidence_review_identity.py`: the extended check, below.

## What the review changed

Reviewer: **bunny-2/bunny-2**, the same agent instance that implemented #483, so
per AGENTS.md this review is **not independent** and is not independent
original-reference evidence. It found four defects and fixed them on this branch;
nothing was regenerated and no claim level moved.

- **The reader's ignore list was matched against the absolute path.** `IGNORED`
  holds `target`, `.git` and `private`, and the test compared it against
  `path.parts` of the full path. A checkout that lives under a directory called
  `private/` — which is how this repository is developed on some machines, and
  how the review ran the check against `origin/main` in a scratch worktree — then
  excluded every harness and the reader found nothing at all. The three names are
  now matched against the path *relative to the checkout*, and a test builds a
  synthetic root under a directory called `private` to pin it.
- **A `"review"` block the reader could not resolve was skipped silently.** The
  `if not ids or not claim: continue` in `read_harness` dropped a harness with no
  task id before its marker or no `claim` after it, which is precisely the
  "quietly stops covering the new one" failure this reader exists to prevent: the
  harness would stop being cross-checked and nothing would say so. It is now
  keyed by its own offset and reported as a problem, with a test for each way it
  can happen.
- **`F10-D`'s identity had no full stop and repeated itself.** The sentence read
  "No agent review replaces the owner's human approval Per the owner directive of
  2026-09-28 … and no agent review replaces the owner's human approval". It now
  ends that clause and adds "and this review did not get it", which is the fact
  the second copy was trying to say. `F12-K` had the same missing full stop
  before "The harness that wrote this report is a private Python script", which
  now also names the path the report's own `review.method` names.
- **`F51-D` claimed a regeneration was "byte-identical", which it could not have
  been.** `created_at` is `iso_utc_now()` in that harness, so a regeneration
  during the review carries the review's own clock. The Rally log says
  "regenerated evidence (matched)"; the identity now says exactly that, and
  records why the committed copy is still the implementer's bytes.

The counts in this note were wrong in three places and are corrected above: 36
`literal` and 7 `runtime` harnesses (not 37 and 6), 38 harnesses compared
byte-for-byte, and six harnesses in five production `src/` files. Of the 29
advisory reports, 26 are honest records and three are #484's.

The corpus has since grown by one: `T374` landed on `main` after this note was
written, and the discovering reader picked it up with no change — 54 reports, 46
harnesses, 30 advisories, and `T374`'s harness and report agree. That is the
point of not listing the harness files, so the numbers here are a reading of one
moment and the check is not.

## What deliberately did *not* change

- **No report was regenerated.** `docs/contracts/CLI-EVIDENCE.md` requires a
  report to match the tree that was tested, and this is a text fix, so re-running
  the four-step sequence would have invalidated `candidate_tree` and the recorded
  test counts for nothing.
- **`claim` stays `implemented` everywhere.** A merge awards `checked` only and
  no agent self-awards a level; the check enforces this.
- **`review.method` is untouched.** It describes the run each report records,
  which for the rewrites is still the implementer's own run.
- **The three reports that do not name both an implementer and a reviewer** —
  `F05-D` (`bunny-1 (Rally reviewing agent for #24; …)`, a reviewer and no
  implementer), `F07-D` (`deepseek-1 (implementing agent)`, an implementer and no
  reviewer) and `F31-D` (`implementer: deepseek-1, the agent that implemented and
  submitted #128`) — are not in this task's scope: none of them carries a
  hand-over placeholder, and rewriting another task's record of a review its
  reviewer never wrote up is a separate, deliberate act. They are filed as
  follow-up **#484** with the one-entry, one-line recipe. The check passes them
  as advisories and says so.
- **The harnesses that take the reviewer from the environment** — F11-D, F14-D
  (`CS_EVIDENCE_REVIEW`, with a hand-over prompt as the `unwrap_or_else`
  fallback) and F15-D, F56-A, T342, T345, T346 (`CS_EVIDENCE_REVIEWER`) — are left
  exactly as they are. Their committed reports are honest already. Two reasons
  not to "fix" the fallback: the reader has no literal to compare, so any rule
  that demanded agreement would be a false rule; and rewriting the default to
  the reviewer's identity would make a regeneration with the variable unset
  claim a review that run did not perform. The reader reports those seven as
  shape `runtime`, exempts them from the agreement rule, pins the exemption in a
  test, and reports any shape it cannot parse as a failure instead of skipping
  it.

## The check

`python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v`
— 14 tests, 20 with the whole `tools/tests`.

**The reader now covers the family instead of one file.** It walks every `*.rs`
under `crates/` and `tools/` and keeps the files that write a `"review"` object,
so a new stage's harness is covered the day it lands. It finds 45 harnesses in
28 files: the 17 campaign ones, and 28 more, of which six harnesses live in
five production `src/` files rather than test files (`F06-D`, `F07-D`, `F08-D`,
`F09-D`, `F13-B` and `F13-C` in `crates/cs_content/src/livery.rs` and
`tools/cs_inspect/src/{interp,script_discovery,textures,zbd}.rs`). A
hand-maintained list would have been a list that rots, which is exactly how
#479's scope became this task.

Getting the identity out of those files took three shapes, and the reader records
which one it found so a fourth cannot pass unnoticed:

| shape | how the identity reaches the report | count |
| --- | --- | --- |
| `literal` | a `jstr` string literal in the harness | 36 |
| `function` | the return value of a `fn …() -> String` in the same file | 2 (F18-D, F51-D) |
| `runtime` | `CS_EVIDENCE_REVIEW` / `CS_EVIDENCE_REVIEWER` at run time | 7 |
| `unknown` | anything else — reported as a failure, never skipped | 0 |

The 38 literal and function harnesses' committed reports are compared
byte-for-byte against what the harness writes, and all 38 agree. 45 of the 53
committed reports have a harness; the other eight (`F05-D`, `F12-D.langui`,
`F12-E`, `F12-G (Rally #368)`, `F12-H`, `F12-J`, `F12-K`, `T351`) have a private
or absent harness and are still covered by the report-side rules.

**The two-level design is kept, and one rule moved down a level.** A hand-over
placeholder in any committed report is still an unconditional failure, as are a
non-empty `review.method`, `claim: implemented`, a harness that drifts from its
report, a harness with no committed report and a shape the reader cannot parse.
The rules that need Rally's facts — name the implementer and every reviewer, say
the review is not independent when the reviewer is the implementer's own
instance, and say something about the reviewer's context — are snapshot-backed.
The context rule used to be unconditional, and that only worked because #479 was
scoped to a family whose reports all happen to use the words `context` or
`fresh`. Applied to all 53 reports it fails eight honest ones that use other
words (`F05-D` "after the review fixes", `F12-H` "in a separate session"), and a
check that fails an honest record teaches its readers to distrust it — the exact
lesson #479's own review drew when M16-A-FU1 failed it. A fact that only the
Rally snapshot can adjudicate belongs behind the snapshot. Nothing observable
changes for the campaign family: its reports are either snapshot-backed or
already honest, and #479's drift test for the context rule still fires.

`PLACEHOLDERS` gained the three wordings this task found — `must re-run`,
`must record its own identity`, `are recorded at review time` — next to the five
#479 added, and a test pins every wording a committed report has actually
shipped with, then drives each entry through the checker on a synthetic report so
a placeholder that is merely listed cannot sit there inert.

The two snapshots are merged for the rules and kept apart for the facts, and a
stage recorded in both is a failure. The advisories are the 29 reports no
snapshot has Rally facts for, printed one line each, including M10-A, M16-A-FU1
and M17-A — the same campaign stages #479 deliberately left out, plus the
non-campaign reports named above.

**It is checked, not hoped for.** On the pre-change corpus (this branch's data
reverted, the check kept) the same selection reports 56 problems across exactly
the ten reports in this note and fails; with the fix it passes. The drift test
named in the task puts F02-B, F04-D, F12-K and T344 — one from each family, and
F12-K from the family with no harness — back to `self-check` and requires each to
be reported, and does the same for renaming the reviewer, drifting the harness
literal, awarding `checked` and stripping the statement about the reviewer's
context.

## Known limitations, not closed by this task

- **Nothing enforces this check on any push.** The CI workflow runs the Rust
  checks, the synthetic-fixture comparison and the binary-file scan only;
  `.github/workflows/ci.yml` is protected and this task did not touch it. The
  follow-up that would fix it, **#480 (M16-A-FU3)**, is **blocked, not pending**:
  its change is written but unpushed because the agent token cannot write the
  workflow file. Until the owner pushes it, a stage can still land with a
  placeholder and the next reviewer has to catch it by hand.
- **Seven harnesses are not cross-checked** against their reports, for the
  `runtime` reason above. That is recorded, pinned and visible in the test output
  rather than papered over.
- **Twenty-nine reports have no Rally facts in either snapshot** and pass as
  advisories. Twenty-six of them are honest records; the other three are #484's
  `F05-D`, `F07-D` and `F31-D`.

## Sources

`docs/contracts/CLI-EVIDENCE.md`; the owner directives of 2026-09-28 in the Rally
project instructions; the Rally activity log of tasks #10, #11, #12, #20, #44,
#207, #340, #343, #344 and #377; `git log` and the object store for the report
provenance; #479's findings note
`docs/findings/2026-10-02-m16-a-fu2-reviewer-identity-in-campaign-evidence.md`.
