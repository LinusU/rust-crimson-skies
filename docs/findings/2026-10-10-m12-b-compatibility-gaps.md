# M12-B: M12's control program, measured, and why it does not validate yet

Date: 2026-10-10. Task: M12-B "Implement and regress mission-specific
compatibility gaps" (#292, `missions/M12.md`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`). Implementer: **swe2-max-1/swe2-max-1** (Devin SWE-2 Max,
Rally #292 implement claim, sessions of 2026-10-10). Reviewer:
**swe2-max-1/swe2-max-1** (Devin SWE-2 Max, Rally #292 review claim of
2026-10-10T09:23Z) — a different session of the same agent and model, run
with a fresh context that re-read the task history, the mission sheet, the
shared contract and the diff; an agent review of the code and tests, not
independent of the implementer's model, not independent original-reference
evidence and not original-run evidence; no agent review replaces the
owner's human approval.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions, the record → `RawProgram` adapter and `SourceContext`’s
control-program binding are M01-LC's/M02-B's; this stage runs them over M12
and pins the result in `crates/cs_app/tests/campaign/m12_b.rs` (six retail
and two synthetic `accept_m12_b_*` tests) plus the evidence harness in
`evidence/m12_b.rs`. Wiring: `campaign/main.rs` and `campaign/evidence.rs`
(`mod m12_b;`).

## Measured

| Fact | Value |
| --- | --- |
| Reader archive | `ZBD/C3/M02/zrdr.zbd`, 16 members, SHA-256 `5a5eb706…05fbda` (the program M12-A bound) |
| Control member | `objectives.zrd`, the only member declaring numbered blocks — not the first, not the longest |
| Blocks / sites / keys | 52 (numbered 1..=52, no gaps) / 187 / 29 |
| Record-level keys | the five measured fields (`MISSION_TIMER`, `PLAYER_INIT`, `RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`), each once, none unclassified |
| Terminal outcomes | `INSTANTWIN` in OBJECTIVE48, one site, bare — **no `INSTANTLOSS` anywhere**, the first measured mission that spells no failure latch |
| Dispositions | 1 implemented, 28 measured, **0 refused**; no unmeasured key |
| Calls | all 187 sites bind to a host call; no unbound key |
| Conditions | 51 of 52 lower, **1 refused** (OBJECTIVE36) |
| Cross-objective edges | 25 addresses over `WAKE`/`KILL`/`NAP_OBJECTIVE_WHEN_I_COMPLETE` and `TICK_DEPENDS_ON_OBJ`, all in `1..=52`; min 4, max 52 |

The success latch is gated: OBJECTIVE48 starts dormant with no timed wake
(`BEGIN_DORMANT -1`), its only incoming edge is OBJECTIVE9's
`WAKE_OBJECTIVE_WHEN_I_COMPLETE`, OBJECTIVE9's only incoming edge is the nap
OBJECTIVE8 schedules, and OBJECTIVE8 ticks only while its dependency gate
OBJECTIVE5 is awake — the chain the record spells is `8 →nap→ 9 →wake→ 48`,
gated on `5` (whose own only incoming edge is the nap OBJECTIVE4 schedules).
The second dependency gate is `15 → 13`. `TICK_DEPENDS_ON_OBJ` addresses a
gate, not a completion effect, so it is excluded from the incoming-edge walk.
Because no block spells a failure outcome, whatever ends the mission short
of success is not a latch this member spells — a fact the escort-survival
priority is measured against.

Sheet priorities. The record carries **no** collision, damage, docking,
pickup, boarding, transfer or interaction directive at all — the whole
29-key vocabulary is pinned, so the actor/interaction halves of the three
priorities are not in this member. What the record spells:

- *Escort survival*: `piratezep` is the only name spelled through three
  evaluators — `WAKEUP_ZEP_TURRETS` (OBJECTIVE1), `INACTIVE1` in-play member
  of OBJECTIVE5 and OBJECTIVE13, and the `TRAVELERS` anchor of OBJECTIVE51
  (`[player, APPROACHING, piratezep, 1500, 1]`), whose completion wakes
  OBJECTIVE10's `COMPLETED_SOUND_GROUP music_missionsuccess_sg`.
- *Door and route clearance*: OBJECTIVE8's `ADD_OBJECTIVE_TARGET
  pzhookpoint`; OBJECTIVE9's `WAKE_ANIM pzhomebase` plus the `ANIM_STATE`
  completion `[ANIM, [NAME, [hooked_to_klondike]], [STATE, [EXECUTED]]]`;
  OBJECTIVE43's `REMOVE_OTHER_TARGET dock2`; `airdock2` is an in-play member
  of blocks 14, 43, 44, 45. The two `PRIMARY` objectives are blocks 8 and 9
  (`MSG_BRF_HAM2_OBJ1/2`), the `SECONDARY` block 15 (`MSG_BRF_HAM2_OBJO`).
- *Oversized collision*: nothing names a collision, a damage value or a
  size; the measurable proximity structures are the three `TRAVELERS` sites
  — all three measured shapes. `WAKEUP_TURRETS` has three sites, one of them
  OBJECTIVE40's `b_turret*`, the one-digit-consuming wildcard the findings
  measured but no measured site had spelled before.

Mission start data, spelled at record level: `MISSION_TIMER` 0, `PLAYER_INIT`
airborne at `(-8198, 200, -11804)` velocity `(0, 95, 0)` — the authored
start as the record carries it, not a walkthrough's.

## The M12-specific gap

One of 52 conditions refuses, so the lowered program assembles but fails
validation (`mission/ch3-m02 objective#35 [condition]: unsupported
instruction`), the `objective_condition` and `call_arguments` rows are
unmet and M12 is not ready:

**`TRAVELERS` counting mode** (OBJECTIVE36 `[4, APPROACHING, unit03, 700,
1]`). A non-string `child0` arms the counting mode, which accumulates a
matching-member count into the objective during evaluation (`+0x5b8`): a
write, so no side-effect-free `Condition` can carry it. This is the same
unimplemented gap **M10-B-FU1** (#812) tracks for M10's two sites; M12's is
the first *counting* site that spells `APPROACHING`, and the first whose two
subject-mode siblings both lower — OBJECTIVE2 anchors on a spelled point
`(-12419.9, 134.0, -10429.8)` (a shape no earlier measured mission carried)
and OBJECTIVE51 on the named `piratezep`. The synthetic test pins all three
shapes plus the refusal of a non-`APPROACHING` polarity, so a shortcut that
accepts every token fails.

## Not claimed

No playthrough, difficulty, optional or presentation row is covered: the
wrong-actor, wrong-session and repeated-event halves of the three priorities
are runtime observations and need M12-C plus human play. The wildcard's
one-digit-consuming intent stays the recorded unknown — the adapter carries
`b_turret*` as spelled data and the suite pins exactly that. The census
keeps M12 unready and the campaign gate closed; nothing is
`verified_original`.

## Mutation checks

Each applied, run against `accept_m12_b_`, observed and reverted:

| Mutation | Result |
| --- | --- |
| `cs_content::mission_control::terminal_outcome_of`: `INSTANTWIN` → `None` | 3 failures (vocabulary partition, lowering "every site binds", synthetic wildcard/win-latch: `INSTANTWIN` becomes an unknown host call) |
| `m12_b.rs`: expected block count 52 → 51 | 3 failures (vocabulary blocks, block-graph numbering `1..=52`, lowering condition-verdict count) |

## Review

Reviewed by **swe2-max-1/swe2-max-1** on 2026-10-10 in a fresh session (a
different session of the same agent name and model as the implementer), against
`missions/M12.md` (`M12-B`), `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md` and the diff. The review re-derived every
claim independently of the implementing session; it is an agent review of the
code and tests, not independent original-reference evidence and not
original-run evidence.

- The branch sat on the latest `origin/main` (`2c0a72ef`) already, so the
  rebase was a no-op; the branch then carried the four stage commits plus the
  reviewer-identity commit.
- Checks on the reviewed tree (`6b3f7380`, tree `ba264c37`), 2026-10-10
  ~09:30–09:45 UTC: `cargo fmt --all -- --check` (0), `cargo clippy --workspace
  --all-targets --all-features --locked -- -D warnings` (0), `cargo test
  --workspace --locked` (0: 495 harnesses, no failures), `cargo test
  --workspace --locked -- accept_m12_b_ --include-ignored` (0: 8 discovered, 8
  executed, 8 passed, 0 failed, 0 ignored).
- Both mutations above re-applied by the reviewer and reverted, each failing
  exactly the same 3 of the 8 tests the implementer recorded, on the same
  assertions; the working tree was clean afterwards.
- Evidence regenerated from that run with the recipe in the `evidence.rs`
  header plus `CS_EVIDENCE_REVIEWER` naming the reviewer (`candidate_tree`
  `ba264c37`, the tree of the commit tested), written to
  `private/evidence/M12-B/acceptance.json` and validated with
  `tools/validate_evidence.py --artifact-root private/evidence/M12-B
  --require-pass` (exit 0). The committed copy in
  `docs/findings/evidence/M12-B.json` is that file; the only field changes from
  the implementer's copy are `candidate_tree`, `created_at`, the artifact hash
  of this run's log, and `review.identity` / `review.method`, which now name
  the reviewer instead of the hand-over placeholder.
- `python3 -m unittest discover -s tools/tests -p
  'test_evidence_review_identity.py'` → 27 tests, OK (M12-B is not in a Rally
  review snapshot yet, so its implementer/reviewer pair is an advisory note
  there, as the reader documents).
- No protected path, no original data and no binary file changed; the branch
  touches only `crates/cs_app/tests/campaign/` (the suite, the harness and the
  two `mod` wiring lines) and `docs/findings/` (this finding and the evidence
  copy).

Not fixed here, unchanged and still open: OBJECTIVE36's `TRAVELERS` counting
mode is the shared unimplemented gap **M10-B-FU1** (#812) tracks, and M12 stays
unready until it is measured.
