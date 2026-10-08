# M03-B: M03's control program, measured, and why it does not lower yet

Date: 2026-10-08. Task: M03-B "Implement and regress mission-specific
compatibility gaps" (#265, `missions/M03.md`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`). Implementer: **claude-2/claude-2** (Sonnet 5.5, 2026-10-08).
Reviewer: **bunny-2/bunny-2** (Rally review claim of 2026-10-08T21:46Z), a
different agent instance and model with a fresh context. The review re-ran the
whole `accept_m03_b_` suite with `CS_GAME_DIR` on the rebased commit,
regenerated the evidence report from that run and validated it with
`tools/validate_evidence.py --require-pass`; it is an agent review of the code
and tests, not independent original-reference evidence, and no agent review
replaces the owner's human approval.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions and the record → `RawProgram` adapter are M01-LC's; this stage
runs them over M03 and pins the result in `crates/cs_app/tests/campaign/m03_b.rs`
(six retail and two synthetic `accept_m03_b_*` tests) plus the evidence harness
in `evidence.rs`. Wiring: `campaign/main.rs` (`mod m03_b;`).

## Measured

| Fact | Value |
| --- | --- |
| Reader archive | `ZBD/C1B/M03/zrdr.zbd`, 22 members, SHA-256 `5a3051e0…4f76ddc` (the program M03-A bound) |
| Control member | `objectives.zrd` (21318 bytes), the only member declaring numbered blocks |
| Blocks / sites / keys | 55 (numbered 1..=55, no gaps) / 313 / 40 |
| Record-level keys | the five measured ones, no unclassified key |
| Terminal outcomes | `INSTANTWIN` in block 33, `INSTANTLOSS` in block 7, one site each |
| Dispositions | 2 terminal, 37 measured, **1 refused** |

Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT -1`).
Block 33 is named only by block 32 (a nap), block 7 only by blocks 6, 22 and 23
(naps). No other block ends the mission. Every wake/kill/nap/gate address (84
spelled integers) is in `1..=55`: the spelled value is the block number, which
the original `dec`s into an index, so nothing points past the record
(contrast M02-B-FU3).

Sheet priorities: grouped objectives = `DEDG` in blocks 11, 31, 32, 46
(`[3,0] [1,0] [2,0] [3,0]`); world damage state = 111 `INACTIVE<n>` sites with 8
`INACTIVE_COMPLETION_COUNT` thresholds; completion ordering = 20 wake, 15 kill
and 18 nap blocks plus two `TICK_DEPENDS_ON_OBJ` gates (both on block 54).
Their keys resolve to the operations the M01-LC findings measured.

## The M03-specific gaps (why the record does not lower)

Three of 313 sites refuse, so `validation` is `None`, no `MissionProgram`
exists, `call_arguments` is the one unmet requirement and M03 is not ready:

1. **`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`** (block 13 `[14,15]`, block 14 `[49]`).
   Spelled by no other mission. The measured parser keys are `WAKE_OBJECTIVE`
   and `WAKE_OBJECTIVE_WHEN_I_COMPLETE`; whether the original accepts or
   silently ignores the `WAKEUP_` spelling is **unknown**. The two sites have
   two shapes and no majority is taken. Filed as **M03-B-FU1** (#803).
2. **`SET_AI_NET`** (block 10): ten `{actor, net}` pairs exceed the host-call
   argument bound, so the registry refuses the key. This is the shape-bound gap
   already filed as **M02-B-FU1** (#800); not duplicated.

## Not claimed

No playthrough, difficulty branch, media or presentation row (M03-SUCCESS …
M03-DIFFICULTY) is covered: those need the runtime and human play (M03-C). The
meaning of the `INACTIVE` in-play bit, `DEDG` group ids and the stoppoint/
animation targets stays as unknown as the M01-LC findings left it.

## Mutation checks

Each applied, run against `accept_m03_b_`, observed and reverted:

| Mutation | Result |
| --- | --- |
| `m03_b.rs`: expected block count 55 → 54 | 3 failures (control program, lowering, terminal blocks) |
| `cs_content::mission_control::terminal_outcome_of`: `INSTANTWIN` → `None` | 2 failures (dispositions, lowering) |
