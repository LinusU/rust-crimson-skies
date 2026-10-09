# M07-B: The Pilfered Prototype's mission-specific compatibility surface

Date: 2026-10-09. Task: M07-B "Implement and regress mission-specific
compatibility gaps" (#277, `missions/M07.md`, work order `M07-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(`$CS_GAME_DIR` read-only, never written). Implementer: **bunny-2/bunny-2**
(Rally #277, session of 2026-10-09T02:17Z). Reviewer: whoever reviews the
branch — the evidence report's `review.identity` is read at run time from
`CS_EVIDENCE_REVIEWER`, so each agent writes its own; no agent review replaces
the owner's human approval.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M07's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C2/M02/zrdr.zbd` — and this stage binds and
regresses it through the production systems M02-B and M03-B already exercise
(`cs_app::mission_control`, `cs_content::mission_control`,
`cs_content::campaign_bindings::SourceContext::control_program`,
`cs_app::control_lowering`, `cs_script::conditions`) with eight
`accept_m07_b_*` tests. Two lowering gaps kept M07's record from lowering:
the ANIM_STATE mechanism filed as **M04-B-FU1 (#806)** — landed, with M07's
sites re-checked and re-pinned in that change as the scope note on #806
required — and the danger-zones condition filed here as **M07-B-FU1 (#813)**,
which is the one gap that remains.

Nothing here is `verified_original` (AGENTS.md rule 8): the work-order ↔
mission join remains M07-A's inference, directive *effects* are the M01-LC
findings' static readings of the original code, and no original executable has
been run.

## What was read from the installation

`$CS_GAME_DIR` opened read-only. Measured on this installation, all of it
re-derived by the acceptance tests on each run:

| Fact | Value | Where it comes from |
| --- | --- | --- |
| `install_sha256` | `b4e780ab…c631978` | production discovery, re-measured by the test |
| Reader archive | `ZBD/C2/M02/zrdr.zbd`, 78 405 bytes, SHA-256 `6154e4ea…580a5d` | the campaign layout entry at M07-A's campaign position 6 — the same bytes and digest `missions/bindings/M07.json` cites |
| Members offered | 19, every one decoding as a `.zrd` document | `discover_container`, the same enumeration the F13-B script census uses |
| Control member | `objectives.zrd`, offset 18 437, length 16 463, own SHA-256 `2bdee65f…47b06fc` | the **rule** (the only member whose decoded record declares numbered `OBJECTIVE<N>` blocks), never a filename |
| Record | 61 numbered blocks (`OBJECTIVE1`…`OBJECTIVE61`), 231 directive sites, 29 distinct keys | `measure_control_record`, cross-checked by an independent walk |
| Vocabulary partition | 2 implemented keys (`INSTANTWIN` ×1 site, `INSTANTLOSS` ×1 site, both bare), 27 measured keys, **0 unmeasured**, 0 refusals | `MeasuredControlRecord::{implemented, measured, unmeasured}` |
| Record-level fields | `MISSION_TIMER` ×1, `PLAYER_INIT` ×1, the three empty animation lists ×1 each | `record_fields()` |
| Unclassified record keys | none | — |
| Lowering attempt | 61 objectives, 231 calls; since #806: **0 refused calls** (every `ANIM_STATE` site binds its operand list as one argument) and **3 refused conditions** (the `DANGER_ZONES_COMPLETED` sites; before #806 it was 9 refused calls and 8 refused conditions); a program stands and `validate` refuses it on the three blocks | `lower_control_record` through the census row |
| Unmet requirements | `objective_condition` (the three danger-zones sites), `call_arguments` (it carries validate's refusal while a bound program stands) — **two**, where M03 had one | `ControlLowering::unmet()` |
| Census context | 53 mission-scoped readers, 40 with control programs; M07 among the rows that are **not** complete; `campaign_ready()` false | `survey_mission_control_programs` |

The 19 members, in archive order, each with the block count that qualified or
excluded it under the rule:

| member | offset | length | blocks |
| --- | --- | --- | --- |
| `aiv.zrd` | 0 | 14 636 | 0 |
| `dzones.zrd` | 14 636 | 465 | 0 |
| `egen.zrd` | 15 101 | 16 | 0 |
| `location.zrd` | 15 117 | 378 | 0 |
| `map.zrd` | 15 495 | 651 | 0 |
| `mis_anim.zrd` | 16 146 | 1 955 | 0 |
| `net.zrd` | 18 101 | 336 | 0 |
| `objectives.zrd` | 18 437 | 16 463 | **61** |
| `pickups.zrd` | 34 900 | 52 | 0 |
| `startanims.zrd` | 34 952 | 133 | 0 |
| `targets.zrd` | 35 085 | 1 132 | 0 |
| `weather.zrd` | 36 217 | 3 218 | 0 |
| `zeppelins.zrd` | 39 435 | 2 524 | 0 |
| `objcomplete.zrd` | 41 959 | 580 | 0 |
| `pickford_pickup.zrd` | 42 539 | 17 089 | 0 |
| `car_truck_pickford.zrd` | 59 628 | 6 888 | 0 |
| `closegoosedoors.zrd` | 66 516 | 598 | 0 |
| `ladder.zrd` | 67 114 | 8 008 | 0 |
| `zepstate.zrd` | 75 122 | 463 | 0 |

Size and position are measurably **not** the rule, as at M02: the longest
member is `pickford_pickup.zrd` (17 089 bytes, 1.04× the control member), and
the control member is the archive's eighth. The test asserts both facts, so a
future "longest member is the program" or "first member is the program"
regression fails a test.

## What M07-B adds, in owner paths

- `crates/cs_app/tests/campaign/m07_b.rs` (owner path): the eight
  `accept_m07_b_*` tests — six retail, two synthetic. No production code
  changed: the machinery M02-B/M03-B exercise measures M07 as it stands, and
  this stage's job is to run it over M07 and pin what is different.
- `crates/cs_app/tests/campaign/evidence.rs` (owner path): the
  `evidence_report_m07_b_writes_the_acceptance_report` harness plus this
  task's test lists. It is deliberately not prefixed `accept_m07_b_`, so a
  task selection never picks it up as an acceptance test.
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod m07_b;` and a doc paragraph).
- `missions/bindings/` is unchanged: this stage adds no new file there. The
  `M07.json` record keeps its empty `objective_ids` and its "objective graph:
  not bound" unknown — the blocks this stage measure are not objective
  *content ids*, and the product-incompleteness state stays where
  `AUDIT-PLAN-SYNC` puts it (the binding record and `docs/findings/`), never
  narrowed to pass a validator.

## Where the M07 sheet's regression priorities live, and what stays unmeasured

The sheet's priorities are designs; the predicates must come from the source
program and reference runs. This stage locates each one in the measured record
so a later runtime stage knows exactly what to predicate, and assigns no
timing, count or coordinate the record does not spell:

- **Moving pickup.** The control record's only moving-subject predicate is
  `TRAVELERS` (operation `Travelers`), spelled at blocks 44 and 45: two sites
  with distinct subjects (`secfury_5`, `secfury_6`), the `APPROACHING`
  polarity, one shared three-float anchor, one shared float radius, a required
  count of one and `DELETE_ON_SUCCESS`. The animation gates that surround the
  pickup name the trailer segments (`trailer_seg1`…`trailer_seg5`, used by 7,
  5, 4, 2 and 1 sites respectively) and `got_pickford` (block 26) and
  `hooked_to_klondike` (block 61), every one in the `EXECUTED` state. The
  archive also carries `pickford_pickup.zrd` (the longest member),
  `car_truck_pickford.zrd` and `pickups.zrd` — all three declare **no**
  numbered blocks, so they are mission-program members this stage does not
  decode. Whether the pickup interaction itself is authored there, and what
  "moving" means for it, is **unmeasured**; the record's names are pinned, not
  interpreted.
- **Forced plane swap.** The 29-key vocabulary is pinned in full by
  `accept_m07_b_the_sheet_priorities_resolve_to_measured_operations`, and it
  contains **no** player-vehicle change directive. The record's player-side
  data is the `PLAYER_INIT` record field, once. A forced swap, if the mission
  has one, lives in the mission-program members above and stays **unmeasured**
  here.
- **Persistent input ownership.** No control directive touches input
  ownership; likewise **unmeasured** and left to the mission-program members
  and ordinary play (M07-C).

The runtime halves of all three priorities (the wrong-actor / wrong-session /
repeated-event boundaries the sheet demands) need ordinary-play observation
and stay open for M07-C. No reference capture exists for M07
(REF-OWNER-FIRST-CAPTURE is blocked on the owner).

## The objective graph, measured

All 102 cross-objective addresses lie in `1..=61` — 50 wake, 39 kill and 13
nap integers — and the parser `dec`s them into zero-based indices at parse
(`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`),
so every address names a block of this record and none points past it
(contrast M02-B-FU3). Unlike M03, M07 spells no `TICK_DEPENDS_ON_OBJ` gate.

Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT -1`):

- block 25 (`INSTANTLOSS`, bare) is named only by block 24's
  `NAP_OBJECTIVE_WHEN_I_COMPLETE [25, 15.0]`;
- block 27 (`INSTANTWIN`, bare) is named by block 61's wake list `[24, 27]`
  and block 24's kill list `[23, 26, 27]`.

What that precedence does at runtime — in particular whether block 24's kill
of the win block can race its wake — is **unmeasured**; the sheet's
"exact failure/success precedence" row stays open for M07-C, and nothing here
simulates it. The three PRIMARY objectives carry distinct HUD slots (block 1 →
slot 1, block 34 → slot 2, block 26 → slot 3) and one SECONDARY objective
(block 41) carries slot 11; seven `DEDG` sites (blocks 35, 51–56) carry the
enemy-group ids and remaining counts the record spells.

## The measured compatibility gaps, and why they are not papered over

M07's directive vocabulary is **fully measured**: 29 of 29 keys carry a
disposition, none is Unmeasured, no block is unreadable and no record key
falls outside the vocabulary. Yet the record does not lower, and two
requirements are unmet (M03 had one):

1. **`ANIM_STATE` — the M04-B-FU1 mechanism, closed at M07 by #806.** The
   key's disposition is Measured (`AnimationStates`); when this suite was
   written, two independent arms refused it:
   - *Conditions (closed).* `cs_script::conditions::anim_state` accepted
     exactly the tag `ANIM` and **one** spec record. M07 spells 1, 2, 3, 4
     and 5 spec records at its nine sites (blocks 9, 13, 16, 20, 26, 30, 31,
     60, 61), so the five multi-record blocks (13, 16, 20, 30, 60) refused
     `objective_condition`. The original's parse helper at `0x4691d0` walks
     **N** `ANIM`/spec pairs into one `{required, count, records}` header
     (`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`),
     so M07's spelling was always the measured original shape; #806 lowered
     that walk and all nine sites' conditions lower now.
   - *Calls (closed).* The registry's per-signature `MAX_CALL_ARGS` (8) bound
     refused block 60's ten-argument shape, so the whole key failed
     registration and all nine sites refused as `unknown host call`. #806
     generalized M02-B-FU1's list-argument carrying to `AnimationStates`:
     the operand list is the call's one `Value::List` argument and every
     site binds, `MAX_CALL_ARGS` unchanged. M07's numbers were re-checked on
     the #806 branch exactly as the scope note required — see
     `docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`.
2. **`DANGER_ZONES_COMPLETED` — filed here as M07-B-FU1 (#813).**
   Blocks 37, 39 and 41 spell `DANGER_ZONES_COMPLETED [dzpath8/dzpath2/dzpath3]`
   and the condition lowering refuses each with "the danger-zones flag
   evaluator is measured but this build lowers no condition for it, and
   offering one would be a guess at its predicate"
   (`cs_script::conditions`). The operation itself is measured
   (`DangerZoneFlags`), and the installation spells the key in 31 blocks
   across campaigns (T463/T464), so this is a lowering gap, not a data
   unknown; who writes the flag bytes the evaluator reads is the measurement
   #813 must make before offering a predicate.

The synthetic tests carry both mechanisms on authored records, so CI covers
the arms without original data: a multi-record `ANIM_STATE` site lowers with
every pair appended in order (only an operand list past `MAX_VALUE_ITEMS`
still refuses, at the key's registration), and a `DANGER_ZONES_COMPLETED`
site refuses with its named reason while the same block with the measured
inactive-members evaluator lowers. Both follow-ups' pin updates landed in
their own changes — never deleted to get green: #806 rewrote the anim_state
arm, and #813's arm stands unchanged.

The census reports M07's row incomplete and `campaign_ready()` stays false —
on the danger-zones gap alone now.
That is the contract's honest reading — an unlowerable program is
`Unsupported`, never a guessed one — so nothing in this stage loosens it.

## Files

- `crates/cs_app/tests/campaign/m07_b.rs` — the eight acceptance tests.
- `crates/cs_app/tests/campaign/evidence.rs` — the evidence harness and the
  task's test lists.
- `crates/cs_app/tests/campaign/main.rs` — wiring only.
- `docs/findings/evidence/M07-B.json` — the committed copy of the acceptance
  report (the artifacts it hashes stay in `private/evidence/M07-B/`).

## Test inventory (`accept_m07_b_*`, 8 tests)

| Test | What it pins |
| --- | --- |
| `accept_m07_b_the_control_program_is_the_member_that_declares_the_blocks` (retail) | the control binding and M07-A's mission binding name one mission (`mission/ch2-m02`) and one program (`script/c2-m02-zrdr`); the archive's length and digest re-derive from disk; exactly one of the 19 members declares numbered blocks and it is the member the binding names; the member's span lies inside the archive and its digest re-derives from the member's own bytes; a longer member exists and the control member is the eighth, so size and position are not the rule; the census and the work-order join agree on the archive and the record; an independent walk reproduces the member table, the block count, the site count and the whole measurement |
| `accept_m07_b_the_vocabulary_is_fully_disposed_and_no_m07_key_is_refused` (retail) | 61 blocks / 231 sites / 29 keys; sites sum to the record total; the partition is exactly 2 implemented + 27 measured + 0 unmeasured; both outcome keys are bare and answer for their names; the five record fields each occur once; no block refusal and no unclassified record key |
| `accept_m07_b_the_anim_state_gap_closes_and_danger_zones_is_the_remaining_one` (retail) | the mission id and the 61 objectives lower; no call refuses — all nine `ANIM_STATE` sites bind, each carrying its operand list as the call's one list argument, block 60's ten operands included; the three refused conditions are exactly the `DANGER_ZONES_COMPLETED` blocks (37, 39, 41); block 60's evaluator carries five pairs with `required` counting them; a program stands, `validate` refuses it on the three blocks, `objective_condition` and `call_arguments` are the unmet requirements and the row is not complete |
| `accept_m07_b_the_sheet_priorities_resolve_to_measured_operations` (retail) | all 27 measured keys resolve to the operation the shared findings measured; the complete 29-key vocabulary is pinned; the two `TRAVELERS` sites have distinct subjects and otherwise one shape, one anchor, one radius; the nine `ANIM_STATE` sites' per-block spec counts, every state `EXECUTED`, and the exact name multiset (five trailer segments, `got_pickford`, `hooked_to_klondike`); the three pickup-carrying members declare no blocks |
| `accept_m07_b_the_objective_graph_is_closed_and_the_terminal_blocks_are_gated` (retail) | 102 cross-objective addresses (50 wake, 39 kill, 13 nap), every one in `1..=61`; no dependency gate; exactly two terminal blocks, both bare and dormant with no timed wake; the loss block's single nap incoming and the win block's wake/kill incoming pinned edge by edge; the four `IDENTITY` sites' classes and slots; the seven `DEDG` sites' groups and counts |
| `accept_m07_b_the_mission_stays_unready_and_the_campaign_gate_stays_closed` (retail) | M07 is not among the complete missions, the campaign gate is closed, M07 is a measured row, and the row's own accounting has unmet requirements |
| `accept_m07_b_every_spelled_record_lowers_and_an_uncarriable_list_refuses` (synthetic) | the measured arms: one record lowers and assembles; two and five records lower with every pair appended in declaration order, `required` counting them, the ten-operand site binding as one list argument; a list past `MAX_VALUE_ITEMS` is uncarriable, so the key refuses registration rather than truncating and the damaged block's condition refuses beside it |
| `accept_m07_b_a_danger_zones_site_refuses_its_condition_and_the_block_without_it_lowers` (synthetic) | a `DANGER_ZONES_COMPLETED` site refuses its condition with the named reason while its call binds; the same block with the measured inactive-members evaluator lowers and the program assembles |

Every test calls production code (`SourceContext::control_program`,
`SourceContext::read`, `survey_mission_control_programs`,
`measure_control_record`, `lower_control_record`, `discover_container`,
`decode_zrd`, `terminal_outcome_of`, `cs_assets::install::sha256`). No test
repeats an expected value read from the record it checks: the retail
assertions re-derive from `$CS_GAME_DIR` through a second production walk, or
compare two independent derivations against each other. The retail tests are
`#[ignore = "requires CS_GAME_DIR"]` (CI skips them); the synthetic ones run
in CI.

## Mutation probes

Four mutations were applied one at a time, each reverted before the next; the
tree carried none of them afterwards (`git status --porcelain` clean). Every
mutation was observed on the full `accept_m07_b_` selection:

| Mutation | Observed result |
| --- | --- |
| `cs_script::conditions::anim_state`'s arity guard loosened from `!= 2` to `< 2 or odd` (accepting multi-record sites silently) | **2 of 8 fail**: the synthetic ANIM_STATE arm test and the retail gap pin — the five multi-record blocks lower with only their first record and the refused-condition set changes (superseded by #806: the walk now appends every record the measured way, so the probe's premise is gone — its intent, that a multi-record site must never truncate, is what the new synthetic arm pins) |
| the `DANGER_ZONES_COMPLETED` refusal arm removed from `lower_record` | **2 of 8 fail**: the synthetic danger-zones arm test and the retail gap pin — the three blocks lower with no measured predicate behind them |
| `cs_content::mission_control::control_member`'s rule accepts every member (`> 0` → `>= 0`) | **6 of 8 fail**: every retail test that touches the binding — the archive becomes ambiguous and the rule refuses it |
| the test constant `BLOCKS` changed 61 → 60 | **3 of 8 fail**: the identities, gap and graph tests — the pinned block count is a real pin |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m07_b_ --include-ignored` | 0 (8 tests, all passing, with `CS_GAME_DIR` set) |
| `python3 tools/validate_evidence.py private/evidence/M07-B/acceptance.json --artifact-root private/evidence/M07-B --require-pass` | 0 (`structurally_valid: true`) |

## Recorded unknowns (not guessed)

- **The join remains an inference** (M07-A's standing unknown): this stage
  consumes it through `SourceContext::control_program` and adds no second
  evidence for the mapping itself.
- **No directive is implemented by a measured effect.** The 27 measured keys
  carry the M01-LC findings' static readings; a host operation for them is
  future work, and only the two outcome spellings are implemented (as
  `Lowering::Finish`).
- **M07's control record does not lower** — the one measured gap above. M07
  stays `Unsupported`; the campaign gate stays closed; the fix is #813
  (danger-zones conditions). The ANIM_STATE half closed with #806
  (ANIM_STATE, shared with M04 and M06), re-pinned in that change.
- **Runtime predicates are unobserved.** No original executable was run; the
  wrong-actor / wrong-session / repeated-event halves of the sheet's
  priorities and the failure/success precedence between the terminal blocks'
  incoming edges need ordinary-play observation (M07-C).
- **The mission-program members are not decoded here.**
  `pickford_pickup.zrd`, `car_truck_pickford.zrd`, `pickups.zrd`,
  `closegoosedoors.zrd`, `ladder.zrd`, `zeppelins.zrd`, `zepstate.zrd`,
  `aiv.zrd` and the rest carry gameplay this stage does not read; the moving
  pickup's interaction, any forced plane swap and any input-ownership
  transfer live there if anywhere.
- **`PLAYER_INIT`'s value is counted, not interpreted**: one occurrence is
  pinned, its five values are not read here (the M01-LC family reads the
  player configuration through `cs_app::mission_start`; M07's is not bound
  there).
- **M07 is not declared launchable** (`cs_app::mission_launch` declares M01
  only); that belongs to the runtime stages.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`/`control_program`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`; `missions/M07.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-m07-a-source-binding.md`;
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`;
`docs/findings/2026-10-08-m03-b-control-program-gaps.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`;
`crates/cs_app/src/mission_control.rs`; `crates/cs_app/src/control_lowering.rs`;
`crates/cs_script/src/conditions.rs`.
