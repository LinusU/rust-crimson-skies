# AUDIT-PLAN-SYNC (#356): plan documents synchronized with the resequenced Rally DAG

**Task:** Rally #356, key `AUDIT-PLAN-SYNC`. **Implementer:** claude-1 (Claude Opus 5.5), fresh session. **Review:** not yet reviewed; an independent reviewer (different agent instance or model, fresh context) is requested.

## Authorization

Owner-authorized documentation/plan maintenance (`allowProtectedChanges=true`). The task description lists these exact protected paths, and only these were changed among protected paths:

- `AGENTS.md`, `docs/00-SCOPE.md`, `docs/TASK-SPLITTING.md`
- `specs/README.md`, `specs/F12-*.md`, `specs/F13-*.md`, `specs/F17-*.md`, `specs/F18-*.md`, `specs/F24-*.md`, `specs/F26-*.md`, `specs/F50-*.md`, `specs/F59-*.md`, `specs/F63-*.md`
- `missions/M01.md`

Other changes: this note, the Rally snapshot next to it, and `tools/tests/test_plan_sync.py`. No runtime code, schemas, contracts, CI or private artifacts changed.

## Live Rally comparison

`docs/findings/2026-09-28-audit-plan-sync-rally-snapshot.json` records the id, key, status and dependencies of every Rally task 1-362, captured with Rally `list_tasks` over all statuses at about 2026-09-28T22:01Z, after the owner's queue updates (Rally events #865-#873, owner note #914).

Before this change, parsing every sheet's `Dependencies:` line (332 stages: 65 features × 4, 24 missions × 3) and comparing it with Rally showed exactly these differences, and nothing else apart from F08-B's split subtasks F08-B.01-.04:

| Stage | Id | Sheet only (removed by the owner) | Rally only (added by the owner) |
| --- | --- | --- | --- |
| F12-D | 48 | | 351, 356 |
| F13-D | 52 | | 356, 358 |
| F17-D | 72 | | 341, 342, 352, 356 |
| F18-B | 86 | | 333, 356 |
| F24-C | 95 | | 356 |
| F24-D | 96 | | 356, 358 |
| F26-D | 104 | | 356, 358 |
| F50-A | 190 | 23 subsystem A stages (73, 82, 83, 93, 97, 105, 111, 112, 129, 133, 141, 142, 143, 144, 145, 161, 162, 170, 171, 172, 186, 187, 188) | 49, 53, 65, 356 |
| F59-C | 238 | | 354, 356 |
| F63-A | 252 | 101, 188, 189, 208, 212, 224, 232, 240, 244, 248 | 5, 354, 356 |
| F63-C | 331 | | 333, 339, 341, 342, 351, 352, 353, 354, 355, 356, 358, 360, 361 |
| M01-B | 259 | 206 (F50-C) | 356, 359 |
| M01-C | 260 | | 360 |

After the change, every stage satisfies *Rally dependencies = sheet dependencies ∪ owner-ruling additions* (plus split subtasks), and the non-sheet tasks #353-#361 are described with their exact Rally dependencies in `specs/README.md`. No existing dependency was removed apart from the three resequencings the owner made in Rally (F50-A, F63-A, M01-B).

## First-mission path

M01-A (#258) → VS-M01-RUNTIME (#359) → M01-B (#259) → VS-M01-CONTROLLED-RUNS (#360) → M01-C (#260). The whole snapshot graph is acyclic. The transitive closure of M01-C contains REF-OWNER-FIRST-CAPTURE (#358) and none of F50-B (#205), F50-C (#206), F50-D (#330), F63-B/C/D. #359 does not depend on #259.

## Full-release requirements kept

- The 23 A stages removed from F50-A are all in the closure of F50-B; the 10 removed from F63-A are all in the closure of F63-B.
- F50-C still gates M02-B to M24-B. F50-D still depends on F50-C and all 24 mission C stages. F63-D still depends on every other feature's D stage, F63-C, F50-D and all mission C stages.
- F63-C holds all 13 added verification tasks, so they are transitive prerequisites of F63-D.
- The capabilities of M01-C, F50-D, F63-C and F63-D are unchanged (`human_play`, `human_review`, `network_real` where they were before).
- F24-C's minimum scenario now names the synthetic wiring acceptance; the original-reference comparison it previously named (AC03) is now an explicit additional F24-D requirement, as the owner instructed in note #914. No other stage's acceptance text changed.

## Test

```sh
python3 -m unittest discover -s tools/tests -p 'test_plan_sync.py' -v
```

Six tests with prefix `accept_audit_plan_sync_` parse the real sheets and `specs/README.md`, then check them against the snapshot. Result: `Ran 6 tests ... OK`. With `specs/`, `missions/` and the three docs restored from `origin/main` (and the same test and snapshot), the run reports `FAILED (failures=4)`, so the tests fail when the synchronization is removed. `test_accept_audit_plan_sync_detects_drift` also mutates single rows in memory (a dropped #333, F50-C back in M01-B, the old F50-A list, a dropped #358 on #360, and an extra Rally edge) and requires each to be reported.

## Limitations

- The snapshot is a point-in-time copy. It does not update itself, and the test cannot query Rally. A later owner ruling must refresh the snapshot and the ruling section together.
- CI does not run Python tests yet. AUDIT-EVIDENCE-INTEGRITY (#353), which adds the Python CI step, is blocked on GitHub workflow permission and is **not merged**. Until then this test runs only locally and in review.
- Documents describe the policy on independent review; Rally does not enforce reviewer assignment. No capability was granted, and no original capture, human evidence or verification claim was produced.
