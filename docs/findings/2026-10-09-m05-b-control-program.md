# M05-B: M05's control program, measured, and why it lowers

Date: 2026-10-09. Task: M05-B "Implement and regress mission-specific
compatibility gaps" (#271, `missions/M05.md`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`). Implementer: **claude-2/claude-1** (Sonnet 5.5, 2026-10-09).
Reviewer: not yet recorded; the reviewing agent must add its identity and
whether its context was fresh. An agent review is not independent
original-reference evidence and no agent review replaces the owner's human
approval.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions and the record → `RawProgram` adapter are M01-LC's; this stage
runs them over M05 and pins the result in
`crates/cs_app/tests/campaign/m05_b.rs` (five retail and two synthetic
`accept_m05_b_*` tests) plus the evidence harness in `evidence.rs`. Wiring:
`campaign/main.rs` (`mod m05_b;`).

## Measured

| Fact | Value |
| --- | --- |
| Reader archive | `ZBD/C1/M05/zrdr.zbd`, 14 members, SHA-256 `0ae0341c…a57f8c34fe` (the program M05-A bound) |
| Control member | `objectives.zrd`, the only member declaring numbered blocks |
| Blocks / sites / keys | 58 (numbered 1..=58, no gaps) / 208 / 22 |
| Record-level keys | none outside the measured vocabulary |
| Terminal outcomes | `INSTANTWIN` in block 42, `INSTANTLOSS` in block 4, one site each |
| Dispositions | 2 terminal, 20 measured, **0 refused** |
| Lowering | 208/208 sites bound, 58/58 conditions lowered, validated, program stands, census row complete |

Unlike M02 and M03, M05 spells no `SET_AI_NET` and no key with disagreeing
argument shapes, so there is no compatibility gap at the lowering: nothing is
filed as a follow-up. The census as a whole is still not campaign-ready.

Sheet priorities, resolved to the operations the M01-LC findings measured:

* **Wave lifecycle**: `WAKE_ANIM` in blocks 10-12 and 42, `ANIM_STATE` in blocks
  13..29 (odd) and 42, `WAKEUP_ENEMIES` in 11 blocks (9, 14..30 even, 40), chained by
  `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (26 sites).
* **Protected actor damage**: `INACTIVE1` in 14 blocks (3, 31-39, 43-45, 52),
  `INACTIVE2`/`INACTIVE3` in 43-45, `DEDG` in blocks 2, 7, 46, 47, 48, 50, 51
  (groups `[5,1,2,3,4,3,4]`, remaining count 0), `TRAVELERS` in block 53 (player
  approaching `piratezep`, radius 1500), `TICK_DEPENDS_ON_OBJ` in 48 and 51 (both on 52).
* **Timed completion**: the only timed wake is block 1 (`BEGIN_DORMANT 2.0`);
  blocks 3 and 7 never start dormant; eleven `NAP_OBJECTIVE_WHEN_I_COMPLETE`
  timers, from 1 s to 75 s.

Terminal chain, pinned address by address (spelled value = block number, as in
the M03-B finding): block 4 (`INSTANTLOSS`) is armed only by block 3's 30-second
nap; block 42 (`INSTANTWIN`) is woken by block 41 and **killed** by block 3, so
block 3 (the `INACTIVE1` block, awake from the start) arms the loss and removes
the win. All 92 spelled wake/kill/nap/gate addresses are in `1..=58`. Which
actor `INACTIVE1` in block 3 names was not decoded here.

## Not claimed

No playthrough, failure-cause, retry, difficulty, media or presentation row
(M05-SUCCESS … M05-DIFFICULTY) is covered: those need the runtime and human
play (M05-C). That the program lowers means every site has a measured host-call
binding, not that the mission plays like the original. The unknowns of the
shared directive findings stand (the `INACTIVE` in-play bit's writers, `DEDG`
group ids, `TRAVELERS`' outside polarity, sound-group identities).
`missions/bindings/M05.json` keeps its unknowns list (owner-path file, not
changed here).

## Mutation checks

Each applied, run against `accept_m05_b_`, observed and reverted:

| Mutation | Result |
| --- | --- |
| `m05_b.rs`: expected block count 58 → 57 | 3 failures (control program, lowering, terminal blocks) |
