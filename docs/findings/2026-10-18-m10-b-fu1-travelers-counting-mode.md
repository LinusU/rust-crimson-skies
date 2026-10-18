# M10-B-FU1: TRAVELERS' counting mode as a runtime condition

Date: 2026-10-18. Task: M10-B-FU1 (#812), follow-up of M10-B (#286).
Implementer: **claude-2/claude-2** (Sonnet 5.5). Capability used: `retail`
(read-only `$CS_GAME_DIR`); `$CS_ENGINE_IMAGE` was **not** set for this session,
so no new disassembly was read: every original-behaviour claim below is the
one already recorded, with addresses, in
`2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md` ("`TRAVELERS`").

## The three open questions, answered from that finding

| Question | Answer | Source |
| --- | --- | --- |
| What does a non-string `child0` (M10: `2`) mean? | The raw payload is stored at `+0x598` and is the **AI group id** compared with `vehicle->[+0x388]` for every vehicle in the list at `0x71dabc` (`0x4659b0`). | parse `0x467a70`, count mode `0x465d43` |
| What does the trailing `1` mean? | `child4` → `+0x5b4`, the **required total** (default 1); the objective completes when `+0x5b8 >= +0x5b4` (`0x465df9`) and `+0x5b8` is cleared. | parse, `0x465df9` |
| Does the original compare the polarity with `LEAVING`? | **No.** The parse compares child1 with `APPROACHING` (12 bytes) and stores `1`, else `0`. `LEAVING` is simply "not `APPROACHING`": the outside pole. The old "outside word unknown" refusal was a wrong reading of an already-measured parse. | parse `0x467a70`, polarity switch `0x465a43`..`0x465a5f` |
| How is the counter carried? | In the objective record (`+0x5b8`), accumulating **across ticks**: each evaluation adds the tick's tally (`0x465da4`). | `0x465da4` |

## What changed

* `cs_script::ir::Condition::TravelersCount { group, anchor, radius,
  approaching, required, delete_matched }`, lowered by
  `conditions::lower_block_condition` from a numeric `child0`. A non-int,
  non-string `child0` is still refused (`BadOperands`). Any polarity string
  other than `APPROACHING` now lowers to the outside pole, in subject mode too.
* The running total is **state**: `MissionState` holds `travelers_totals`
  per objective. Evaluation stays pure — `holds_in` reads the total and returns
  the new one; the tick stores it, and clears it for the objective that fires
  (`holds` alone answers one evaluation from a total of zero). The save record
  carries the totals (`SNAPSHOT_VERSION` 4 → 5, older records are refused).
* `MissionFacts::group_members` (group id → positions of the **awake** members;
  the original skips a vehicle whose sleep byte `+0x91d` is set). The live
  writer is `mission_world_observation` (`ScriptedAircraftActor.asleep`, the
  entity `Transform`), reported for exactly the groups whose living count is
  already accounted for; an unobserved group counts nothing.
* Strict comparisons both ways (`d² < r²` inside, `d² > r²` outside), the
  anchor resolved as in subject mode (zeroed point for a chain that never
  resolves; a chain the facts do not carry answers `false`).

## Not claimed / still unknown

* `DELETE_ON_SUCCESS` in the counting mode deletes each matched vehicle as it
  is counted (`0x465a70` → `0x47bab0`). The condition carries the flag and
  names it as the residual unknown
  `cs_script::ir::TRAVELERS_COUNT_DELETE_HOST_EFFECT`; **no host consumer
  deletes the vehicles yet** (M10 block 35 spells it).
* Several objectives that hold on one tick: the original clears `+0x5b8` as the
  objective fires; here only the one objective that fires has its total
  cleared, a held-but-unadmitted objective keeps accumulating. This differs
  from the original only for a mission that has two counting sites holding on
  the same tick; M10's two sites are in different blocks.
* What group id 0 means in the counting mode, and which mode M01's site takes
  at runtime, stay as recorded in the M01 findings.
* No playthrough: the counting condition is exercised by synthetic facts
  (`accept_m10_b_fu1_*`) and by the retail lowering of M10, not by a play.
  M10 readiness otherwise stays with M10-C.

## Checks and mutation

* `cargo test -p cs_script` (all, 133 run: 0 failed), `-p cs_sim`, `-p cs_content` green;
  `cargo test -p cs_app --test cs_app_tests_d -- accept_m10 --include-ignored`
  (18 passed) and `-- accept_m10_b_fu1 --include-ignored` (4 passed) with
  `CS_GAME_DIR`.
* On this machine the retail M12/M13/M08/M07/M04 `*_b` tests and
  `accept_m01_lc_lowering_adapter_m01_lowers_into_a_validated_raw_program`
  fail on mission-id / block-count assertions about the installation
  (for example `mission/ch3-m02` against `mission/ch1-m02`); they assert nothing
  about `TRAVELERS` and fail the same way without this change. Their updated
  `TRAVELERS` expectations (M12's retail lowering row) therefore could **not**
  be run against retail here.
* Mutation, applied, observed and reverted: `total >= required` → `total >
  required` in the counting arm of `MissionState::holds_in` fails 3 of the 6
  `accept_m10_b_fu1_` cs_script tests.
