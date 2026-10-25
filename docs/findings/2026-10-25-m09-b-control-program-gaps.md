# M09-B: Perils for Blake's mission-specific compatibility surface

Date: 2026-10-25. Task: M09-B "Implement and regress mission-specific
compatibility gaps" (#283, `missions/M09.md`, work order `M09-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written). Implementer: **sonnet-1**
(Claude Sonnet 5.5, session of 2026-10-25). Reviewer: none yet; an agent
review is not independent original-reference evidence and no agent review
replaces the owner's human approval.

Nothing here is `verified_original`. The measurements are facts about the
installation's files read through production code; no original run is claimed.

## What changed

No production code. M09-A bound the identities; the control-program machinery
(`.zrd` reader, census, directive dispositions, record → `RawProgram` lowering)
is shared. This stage runs it over M09's own archive and pins the result in
`crates/cs_app/tests/campaign/m09_b.rs` (eight retail and three synthetic
`accept_m09_b_*` tests) plus the evidence harness
`campaign/evidence/m09_b.rs`. Wiring only: `campaign/mod.rs`,
`campaign/evidence.rs`.

## What M09's record is

| Fact | Value |
| --- | --- |
| Archive | `ZBD/C1/M04/zrdr.zbd`, SHA-256 `5ffc1abd…4263` (the M09-A program) |
| Mission / program | `mission/ch2-m04` / `script/c1-m04-zrdr` |
| Control member | `objectives.zrd` (1 of 15 members declares numbered blocks) |
| Blocks / sites / keys | 52 / 201 / 40 |
| Dispositions | 38 measured, 2 terminal (`INSTANTWIN`, `INSTANTLOSS`), 0 unmeasured |
| Record keys outside the vocabulary | none (`MISSION_TIMER`, `PLAYER_INIT`, `RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`) |
| Lowering | all 201 sites bind, all 52 conditions lower, validation clean, row complete |
| Spelled cross-objective addresses | 58, all in `1..=52` (one-based block numbers, see `2026-10-10-m06-b-fu3-objective-address-convention.md`) |

The census as a whole stays not campaign-ready; M09 is not ready either
(no playthrough, difficulty or presentation evidence: M09-C).

## The sheet's three priorities, located in the record

The sheet names no key, so they are pinned as structure, not as invented values.

* **Weakpoint exposure.** `ANIM_STATE` blocks 23 and 37 watch the same eight
  `destroy_hkzep_*` animations for `INVALID` (23 completes on 1, 37 on 3);
  block 32 (`INSTANTWIN`) waits for `hooked_to_klondike` `EXECUTED`; block 30
  is `INACTIVE1 lkgasbag05 panelleft1`; blocks 26 and 27 list twelve engine
  parts of `piratezep` and need 3 and 6 cleared; block 31 targets `pzhookpoint`.
* **Interrupted attack priority.** The kill edges are 7→36, 15→16,
  16→15,17, 30→46, 36→7, 44→39; `TICK_DEPENDS_ON_OBJ` gates 7 on 4, 18 and 19
  on 29, 42 on 40. Two Bloodhawk waves wake in blocks 20 and 24.
* **Carrier survival.** `INACTIVE1 piratezep` is blocks 29, 40, 45; `DEDG`
  depletions are blocks 7, 22, 25, 28, 42, 43, 44; the two `TRAVELERS` sites
  (blocks 47 at radius 1500, 52 at 500) name `player` as subject and
  `piratezep` as anchor, so neither is the counting mode. `INSTANTLOSS` is
  block 41, named only by block 27's 20-second nap; `INSTANTWIN` is block 32,
  named only by block 31's 1-second nap. Blocks 1 and 2 run on clocks (2 s,
  15 s); blocks 7, 23, 26, 30, 33 never start dormant.

## Unknown, not guessed

What the original does when e.g. the `ANIM_STATE` animation names resolve to no
animation, what `pzhookpoint`/`piratezep` part names resolve to in the world,
and every numeric value outside the record (durability, rewards, difficulty
branches) stay unknown; they belong to the world-binding and play-through
stages (M09-C and the world tasks) and the binding's `unknowns` still carry
them.

## Follow-up noted

`accept_f50_b_unresolved_identities_stay_unresolved_and_the_campaign_stays_unready`
and two `accept_f50_c_*` retail tests (see the M09-A finding) are still stale
for M09; not touched here (protected-adjacent, separate task).
