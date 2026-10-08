# M04-B: The Sinister Sub's mission-specific compatibility surface

Date: 2026-10-08. Task: M04-B "Implement and regress mission-specific
compatibility gaps" (#268, `missions/M04.md`, work order `M04-B`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written) and `synthetic` (newly
authored `.zrd` records). Implementer: **bunny-alpha-2/bunny-alpha-2** (Rally
#268, session of 2026-10-08T23:24Z). No reviewer yet; the implementer's own
run is not independent review and no agent review replaces the owner's human
approval.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M04's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C1/M04/zrdr.zbd` — and this stage runs it through
production systems (`SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`) and regresses it with eight `accept_m04_b_*` tests.
One compatibility gap is **measured and pinned, not worked around**:
`ANIM_STATE` is refused in both halves of the lowering, so M04's control
record does not lower and the mission stays Unsupported. That fix belongs to
the condition parser and the lowering subsystem and is filed as **M04-B-FU1**
(#806); this stage names it exactly.

Nothing here is `verified_original` (AGENTS.md rule 8): the work-order ↔
mission join remains M04-A's inference, directive *effects* are the stage A–D
findings' static readings of the original code, and no original executable has
been run.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions, the record → `RawProgram` adapter and the
`SourceContext::control_program` binding are M01-LC's and M02-B's; this stage
runs them over M04 and pins the result in
`crates/cs_app/tests/campaign/m04_b.rs` (six retail and two synthetic
`accept_m04_b_*` tests) plus the evidence harness in `evidence.rs`. Wiring:
`crates/cs_app/tests/campaign/main.rs` (`mod m04_b;` and a doc paragraph).
No `Cargo.toml` or `Cargo.lock` change. `missions/bindings/M04.json` is
unchanged: the blocks this stage measures are not objective *content ids*, so
the record keeps its empty `objective_ids` and its "objective graph: not
bound" unknown exactly as M02-B kept M02's.

## What was read from the installation

Measured on this installation, re-derived by the acceptance tests on each
run:

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` |
| Reader archive | `ZBD/C1/M04/zrdr.zbd`, 74 692 bytes, SHA-256 `5ffc1abd…84c4263` — the program span `missions/bindings/M04.json` cites |
| Members offered | 15, every one decoding as a `.zrd` document |
| Control member | `objectives.zrd`, offset 21 048, length 14 998, own SHA-256 `0bf89dc7…12fb5a`, the **eighth** member; two members are longer than it |
| Record | 52 numbered blocks (`OBJECTIVE1`…`OBJECTIVE52`, no gaps), 201 directive sites, 40 distinct keys |
| Vocabulary partition | 2 terminal keys (`INSTANTWIN` ×1 site, `INSTANTLOSS` ×1 site, both bare), **38 measured** keys, 0 unmeasured, 0 refusals, 0 unclassified record-level keys |
| Record-level fields | `MISSION_TIMER`, `PLAYER_INIT` and the three empty animation lists — each once |
| Lowering attempt | 198 of 201 sites bind; 3 refuse; 50 of 52 conditions lower, 2 refuse; no program assembles |
| Census context | 53 mission-scoped readers, 40 with control programs; M04 among the rows that are **not** complete; `campaign_ready()` false |
| Cross-check | `SourceContext::control_program("M04", "The Sinister Sub")` and `survey_mission_control_programs` agree on the container, its digest, the member, its span and the whole record |

The 15 members, in archive order, each with the block count that qualified or
excluded it under the rule:

| member | offset | length | blocks |
| --- | --- | --- | --- |
| `aiv.zrd` | 0 | 16 861 | 0 |
| `dzones.zrd` | 16 861 | 234 | 0 |
| `egen.zrd` | 17 095 | 758 | 0 |
| `location.zrd` | 17 853 | 378 | 0 |
| `map.zrd` | 18 231 | 651 | 0 |
| `mis_anim.zrd` | 18 882 | 1 830 | 0 |
| `net.zrd` | 20 712 | 336 | 0 |
| `objectives.zrd` | 21 048 | 14 998 | **52** |
| `startanims.zrd` | 36 046 | 260 | 0 |
| `targets.zrd` | 36 306 | 801 | 0 |
| `weather.zrd` | 37 107 | 3 218 | 0 |
| `zeppelins.zrd` | 40 325 | 2 558 | 0 |
| `intro.zrd` | 42 883 | 5 648 | 0 |
| `scenes.zrd` | 48 531 | 23 082 | 0 |
| `zepstate.zrd` | 71 613 | 851 | 0 |

Size and position are measurably **not** the rule: `scenes.zrd` is 1.5× the
control member's length and `aiv.zrd` is longer too, and the control member is
the eighth, not the first. The test asserts both facts.

## Where the M04 sheet's regression priorities live

The sheet's priorities are *designs*; the predicates must come from the source
program and reference runs. This stage locates each one in the measured record
— with the actor names the original actually spells — so a later runtime stage
knows exactly what to predicate, and it assigns no timing, count or coordinate
the record does not spell:

- **Reveal triggers.** Two `TRAVELERS` boundaries, both
  `["player", "APPROACHING", "piratezep", 1500, 1]` (block 47) and
  `[…, 500, 1]` (block 52): the record's only radius triggers, and the
  measured effect never fires at exact equality, so the before/at/after
  boundary is real. Two `WAKEUP_ENEMIES` sites (blocks 20 and 24) name nine
  `blakebloodhawk_*` vehicles. The target-flag vocabulary that makes an object
  appear on the target list is spelled on both sides:
  `ADD_OBJECTIVE_TARGET` (blocks 23, 31), `REMOVE_OBJECTIVE_TARGET` (blocks
  20, 25, 30, 33), `ADD_OTHER_TARGET` (block 25), `REMOVE_OTHER_TARGET`
  (block 23).
- **Launch interruption.** Four `START_TAXI` sites (blocks 8, 9, 10, 11)
  release the parked `blakepeace_2_3`, `_5`, `_4` and `_6` vehicles from the
  AI hold-off byte; two `WAKE_ANIM` sites execute `hangar3_doors` (block 1)
  and `pzhomebase` (block 32); two `WAKEUP_GENERATOR` sites feed `eairg31`
  and `eairg32` (blocks 12, 13). Blocks 1 and 2 are the only blocks whose
  `BEGIN_DORMANT` arms a timed self-wake (at 2 and 15 mission-clock seconds);
  the other 45 markers spell `-1`.
- **Protected carrier.** `SET_HELP_LABEL [[piratezep, rock_zeppelin],
  "MSG_OBJ_DEFEND"]` (block 23) is the help label the defend objective posts,
  and block 25 posts the blank label over the same pair. Seven `DEDG`
  group-depletion thresholds (blocks 7, 22, 25, 28, 42, 43, 44) spell
  `[5,2] [1,0] [2,0] [2,2] [1,0] [2,0] [5,0]` — "N members of the group still
  in play", record data, not a guess. Two `INACTIVE_COMPLETION_COUNT`
  thresholds (blocks 26 and 27) require **3** and **6** of the twelve listed
  members to lose the in-play bit, and every one of those members is a
  three-name chain hanging off `piratezep`: the carrier need not be destroyed
  for either block to complete.

Every key above is asserted to resolve to the operation the shared findings
measured (`DirectiveOperation::{Travelers, WakeEnemies, ReleaseTaxi,
WakeAnimation, FeedGenerator, SetHelpLabel, EnemyGroupDepletion,
InactiveThreshold, InactiveMembers, SetTargetFlag, DependencyGate,
DormantStart}`), so a key that silently lost its meaning fails the run.

## The block graph, and why neither latch can fire early

Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT -1`):

| latch | block | named by | edges into it |
| --- | --- | --- | --- |
| `INSTANTWIN` | 32 | block 44 `NAP_OBJECTIVE_WHEN_I_COMPLETE [31]` | 1 |
| `INSTANTLOSS` | 41 | block 20 `WAKE_OBJECTIVE_WHEN_I_COMPLETE [22, 40, 50]` | 1 |

No other block ends the mission. Blocks 7, 23, 26, 30 and 33 carry no
`BEGIN_DORMANT` at all (they are entered through a predecessor's edge).

The address walk visits 58 spelled addresses — 24 wake, 7 kill, 23 nap and 4
`TICK_DEPENDS_ON_OBJ` gate addresses — and **every one lies in `1..=52`**.
The walk reads the nap's children the way the measured effect splits them:
child0 is the targeted block index and child1 is the re-wake delay in seconds,
so a nap contributes exactly one address. Reading the delay as an address
would report a dangling block the record never addresses (block 19 naps with
a 90-second delay). Block 42's `TICK_DEPENDS_ON_OBJ [40]` is a *gate on block
42* from the failure latch's block, not a wake of it, and the test says so.

## The M04-specific gap, and why it is not papered over

M04's directive vocabulary is **fully measured**: 40 of 40 keys carry a stage
A–D disposition, none is Unmeasured, no block refused to decode and no
record-level key sits outside the vocabulary. Yet the record does not lower.
The single key responsible is `ANIM_STATE`, spelled at three sites:

| block | operands | spelling |
| --- | --- | --- |
| 23 | 18 | `COMPLETION_COUNT [1]` + eight `ANIM [NAME […], STATE [INVALID]]` descriptors |
| 32 | 2 | `ANIM [NAME [hooked_to_klondike], STATE [EXECUTED]]` |
| 37 | 18 | `COMPLETION_COUNT [3]` + the same eight descriptors |

Two distinct refusals, both measured by the suite:

1. **Condition half.** `cs_script::conditions::anim_state` accepts exactly two
   operands — the tag `ANIM` and one spec record — which is the shape M01's
   three sites happen to spell. Blocks 23 and 37 spell 18 operands, so both
   refuse with the measured shape named, `objective_condition` is unmet, and
   only 50 of the 52 blocks lower.
2. **Call half.** `cs_app::control_lowering` registers one `BindingSpec`
   signature per measured shape, and
   `cs_script::bindings::HostBindingRegistry::register` refuses any signature
   longer than `MAX_CALL_ARGS` (8). `ANIM_STATE`'s two shapes are 18 and 2
   operands, so the whole key fails registration, all three sites refuse as
   `unknown host call`, and `call_arguments` is unmet.

No `MissionProgram` assembles, `MissionProgram::validate` is never reached,
the census reports M04's row incomplete and `campaign_ready()` stays false.
That is the contract's honest reading — an unlowerable program is
`Unsupported`, never a guessed one — so nothing in this stage loosens it.

**The direction the fix likely takes, with its evidence.** The stage A–D
findings measured the original's parse helper for this key
(`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`,
`ANIM_STATE`): at `0x4691d0` it **walks the value list** for a tag-3 `ANIM`
followed by a tag-4 spec record, appends *every* pair it finds and increments
`required` once per pair; after the walk it reads a `COMPLETION_COUNT` out of
that same list and the integer **overwrites** `required`. M04's spelling is
what that measured walk describes. The engine's single-pair shape was derived
from M01, whose three sites all spell exactly one pair, and
`conditions.rs` says so in its own doc comment. That is evidence, not a
decision: the sibling lookup's scope and the multi-pair append must be
confirmed against the original before anything changes. Filed as **M04-B-FU1**
(#806), which also states that the call half shares the mechanism already
filed as M02-B-FU1 (#800) — coordinate rather than duplicate, and re-check M04
after #800 lands because #800's acceptance is M02-scoped.

The two synthetic tests carry both refusal arms on authored records so CI
covers them without original data: a single-pair site lowers and completes
while the same key with a `COMPLETION_COUNT` override and a second descriptor
is refused per site; and an `ANIM_STATE` site at the bound of 8 operands
registers while one operand over it refuses the key's whole registration. The
follow-up that changes either bound must update both pins in its own change —
never delete them to get green.

## Not claimed

No playthrough, difficulty branch, media or presentation row (M04-SUCCESS …
M04-DIFFICULTY) is covered: those need the runtime and human play (M04-C). In
particular the sheet's *"wrong actor, wrong session, repeated event"* halves of
all three priorities are **runtime** predicates and stay unobserved; nothing
here simulates them. What `TRAVELERS`' unspelled polarity, which of its two
modes a site takes, what group id 0 means, which HUD element draws a target
flag, what a sound-group handle resolves to and what the in-play bit's writers
do remain as unknown as the M01-LC findings left them — they are listed on the
measured keys, never dropped.

## Files

- `crates/cs_app/tests/campaign/m04_b.rs` — the eight acceptance tests.
- `crates/cs_app/tests/campaign/evidence.rs` — the evidence harness and the
  task's test lists.
- `crates/cs_app/tests/campaign/main.rs` — wiring only (`mod m04_b;` and a doc
  paragraph).
- `docs/findings/evidence/M04-B.json` — the committed copy of the acceptance
  report (the artifacts it hashes stay in `private/evidence/M04-B/`).

## Test inventory (`accept_m04_b_*`, 8 tests)

| Test | What it pins |
| --- | --- |
| `accept_m04_b_the_control_program_is_the_member_that_declares_the_blocks` (retail) | the container's length and digest re-derive from disk; exactly one of the 15 members declares numbered blocks and it is the one the rule picked, the eighth member, with longer members beside it; the record is 52/201/40; an independent walk sees blocks 1..=52 and 201 sites with no refusal and no unclassified record key; the production control binding names the same mission, program, container, digest, member, span and record; the member's own digest re-derives from its bytes |
| `accept_m04_b_every_directive_m04_spells_has_a_disposition_and_none_is_refused` (retail) | the partition is exactly 2 terminal + 38 measured + 0 unmeasured; the sites sum to 201; both outcome keys answer for their own name and are spelled `Bare` |
| `accept_m04_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations` (retail) | the three priorities' keys resolve to the measured operations; `TRAVELERS`' two boundaries with their actors and radii; the four `START_TAXI` vehicle names; the two `WAKE_ANIM` targets; `SET_HELP_LABEL … MSG_OBJ_DEFEND` over both carriers; the seven `DEDG` pairs; both `INACTIVE_COMPLETION_COUNT` thresholds against twelve-member lists that all hang off `piratezep`; `TRAVELERS`' three unknowns are still carried |
| `accept_m04_b_the_terminal_blocks_are_gated_and_every_address_is_in_range` (retail) | `INSTANTWIN` in block 32 and `INSTANTLOSS` in block 41 and nowhere else, both `BEGIN_DORMANT -1`; only blocks 1 and 2 arm a timed self-wake and neither latch is among them; blocks without a dormant marker; the address walk's per-key totals (24/7/23/4 = 58) with no out-of-range address; each latch's in-degree is exactly one completion edge; block 42's gate is a gate, not a wake |
| `accept_m04_b_anims_state_is_the_only_gap_and_the_record_does_not_lower` (retail) | `ANIM_STATE`'s 3 sites in two shapes (18 operands ×2, 2 ×1) with 18 past `MAX_CALL_ARGS`; 198 of 201 calls bind and the three refusals name objective#22/#31/#36; the one unbound key names the bound; 50 of 52 conditions lower and the two refusals name blocks 23 and 37 with the measured shape; no program, no validation; `objective_condition` and `call_arguments` are the only unmet rows; the row is incomplete |
| `accept_m04_b_the_mission_stays_unready_until_anims_state_is_measured` (retail) | M04 is not a complete census row, the campaign gate stays closed, and the row is still reported as measured |
| `accept_m04_b_a_single_animation_pair_lowers_and_a_multi_pair_site_is_refused` (synthetic) | on authored records: one pair is the measured shape and the record completes; a `COMPLETION_COUNT` override plus a second descriptor in the same list is refused per site with the measured shape named; four operands are inside the host-call bound, so the refusal is the shape |
| `accept_m04_b_an_anim_state_site_past_the_host_call_bound_refuses_the_whole_key` (synthetic) | the refusal mechanism at the bound: 8 operands register and their sites bind; 10 operands refuse the key's registration, refuse its site and leave the key out of the registry |

Every test calls production code (`SourceContext::control_program`,
`survey_mission_control_programs`, `read_control_member`,
`measure_control_record`, `lower_control_record`,
`cs_assets::install::sha256`). No test repeats an expected value read from the
record it checks: the retail assertions re-derive from `$CS_GAME_DIR` through a
second production walk, or compare two independent derivations against each
other. The retail tests are `#[ignore = "requires CS_GAME_DIR"]` (CI skips
them); the two synthetic ones run in CI.

## Mutation probes

*(recorded below once run)*

## Checks

*(recorded below once run)*

## Recorded unknowns (not guessed)

- **The join remains an inference** (M04-A's standing unknown): this stage
  consumes it through `SourceContext::control_program` and adds no second
  evidence for the mapping itself.
- **`ANIM_STATE` does not lower** — the measured gap above. M04 stays
  `Unsupported`; the campaign gate stays closed; the fix is #806.
- **The runtime halves of all three priorities are unobserved.** Whether the
  wrong actor, the wrong session or a repeated event can satisfy a reveal, a
  launch interruption or the protected-carrier thresholds is ordinary-play
  behaviour (M04-C) and stays open. No reference capture exists for M04
  (REF-OWNER-FIRST-CAPTURE is blocked on the owner).
- **Which other reader members carry gameplay this stage does not decode**
  (`aiv.zrd`, `zeppelins.zrd`, `scenes.zrd`, `intro.zrd`, `zepstate.zrd`, …)
  is unmeasured here: the control program is one member of fifteen.
- **`M04.json`'s unknowns are unchanged.** The block graph this stage measures
  is not the binding's `objective_ids`, and the checklist entries M04-A left
  unknown (actor/spawn/route sets, audio cues, difficulty branches, success
  precedence, …) stay unknown there and here.
- **M04 is not launchable** (`cs_app::mission_launch` declares M01 only) and
  no engine consumer runs this record yet; both belong to the runtime stages.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`; `missions/M04.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-01-m04-a-source-binding.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`;
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`;
`docs/findings/2026-10-08-m03-b-control-program-gaps.md`;
`crates/cs_app/tests/campaign/m02_b.rs`;
`crates/cs_app/tests/campaign/m03_b.rs`;
`crates/cs_app/src/mission_control.rs`;
`crates/cs_app/src/control_lowering.rs`;
`crates/cs_script/src/conditions.rs`.
