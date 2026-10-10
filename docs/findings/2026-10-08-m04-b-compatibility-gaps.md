# M04-B: The Sinister Sub's mission-specific compatibility surface

Date: 2026-10-08. Task: M04-B "Implement and regress mission-specific
compatibility gaps" (#268, `missions/M04.md`, work order `M04-B`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written) and `synthetic` (newly
authored `.zrd` records). Implementer: **bunny-alpha-2/bunny-alpha-2** (Rally
#268, session of 2026-10-08T23:24Z). No reviewer yet; the implementer's own
run is not independent review and no agent review replaces the owner's human
approval.

approval.

## Review identity

The evidence harness (`evidence_report_m04_b_writes_the_acceptance_report`)
reads `CS_EVIDENCE_REVIEWER` at run time and fills `review.identity` whole, so
the committed `docs/findings/evidence/M04-B.json` cannot contain a hand-over
placeholder: whoever runs the harness writes its own identities. This
implementer's run is recorded as exactly that — the implementing agent at
hand-over, not a review — and the Rally reviewing agent regenerates the report
on the rebased commit with its own `CS_EVIDENCE_REVIEWER` value, saying whether
its context was fresh. M04-B adds no Rally review facts to
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`: that file
records merge events that do not exist yet, and an entry written before the
review would be writing a review fact that has not happened.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M04's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C1/M04/zrdr.zbd` — and this stage runs it through
production systems (`SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`) and regresses it with eight `accept_m04_b_*` tests.
One compatibility gap was left **measured and pinned, not worked around**:
`ANIM_STATE` was refused in both halves of the lowering, so M04's control
record did not lower and the mission stayed Unsupported. The fix was filed
as **M04-B-FU1** (#806) — and it **landed**: the operand-list walk this
document's fix-direction section described is what was implemented, M04 now
lowers completely, and the suite was re-pinned in that same change. The
follow-up record is
`docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`.

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
| Lowering attempt | since #806: **201 of 201 sites bind, all 52 conditions lower, the program validates** — before it, 198 of 201 bound (all three `ANIM_STATE` sites refused) and 50 of 52 conditions lowered, with no program assembled |
| Census context | 53 mission-scoped readers, 40 with control programs; M04's row **complete** since #806; `campaign_ready()` false on other missions' gaps |
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
| `INSTANTWIN` | 32 | block 31 `NAP_OBJECTIVE_WHEN_I_COMPLETE [32, …]` | 1 |
| `INSTANTLOSS` | 41 | block 27 `NAP_OBJECTIVE_WHEN_I_COMPLETE [41, …]` | 1 |

> **Corrected by M06-B-FU3 (#819), 2026-10-10.** This table previously read
> the spelled integer as a zero-based record index (`target − 1`), which
> attributed each latch's edge to the site spelling the *preceding* number:
> it listed block 44's `NAP [31]` for `INSTANTWIN` and block 20's
> `WAKE [22, 40, 50]` for `INSTANTLOSS`. Under the measured convention
> (M02-B-FU3 / #802: the spelled integer is the one-based block number,
> `dec`'d to an index at parse) the true edges are the ones above — both
> latches are reached through naps, and block 20's wake is unrelated to the
> failure latch. See
> `2026-10-10-m06-b-fu3-objective-address-convention.md`.

No other block ends the mission. Blocks 7, 23, 26, 30 and 33 carry no
`BEGIN_DORMANT` at all (they are entered through a predecessor's edge).

The address walk visits 58 spelled addresses — 24 wake, 7 kill, 23 nap and 4
`TICK_DEPENDS_ON_OBJ` gate addresses — and **every one lies in `1..=52`**.
The walk reads the nap's children the way the measured effect splits them:
child0 is the targeted block's number and child1 is the re-wake delay in
seconds, so a nap contributes exactly one address. Reading the delay as an
address would report a dangling block the record never addresses (block 19
naps with a 90-second delay). Block 42's `TICK_DEPENDS_ON_OBJ [40]` is a
*gate on block 40* — the block `OBJECTIVE20` wakes — not on the failure
latch, and the test says so.

## The M04-specific gap, and why it is not papered over

M04's directive vocabulary is **fully measured**: 40 of 40 keys carry a stage
A–D disposition, none is Unmeasured, no block refused to decode and no
record-level key sits outside the vocabulary. When this suite was written
the record did not lower. The single key responsible was `ANIM_STATE`,
spelled at three sites:

| block | operands | spelling |
| --- | --- | --- |
| 23 | 18 | `COMPLETION_COUNT [1]` + eight `ANIM [NAME […], STATE [INVALID]]` descriptors |
| 32 | 2 | `ANIM [NAME [hooked_to_klondike], STATE [EXECUTED]]` |
| 37 | 18 | `COMPLETION_COUNT [3]` + the same eight descriptors |

Two distinct refusals, both measured by the suite — **both closed by
M04-B-FU1 (#806)**:

1. **Condition half (closed).** `cs_script::conditions::anim_state` accepted
   exactly two operands — the tag `ANIM` and one spec record — which is the
   shape M01's three sites happen to spell. Blocks 23 and 37 spell 18
   operands, so both refused with the measured shape named,
   `objective_condition` was unmet, and only 50 of the 52 blocks lowered.
   #806 lowered the measured operand-list walk, so all three sites now carry
   `AnimationStates` evaluators: blocks 23 and 37 hold all eight descriptors
   with `required` 1 and 3 from their `COMPLETION_COUNT` overrides, block 32
   the one `EXECUTED` pair.
2. **Call half (closed).** `cs_app::control_lowering` registered one
   `BindingSpec` signature per measured shape, and
   `cs_script::bindings::HostBindingRegistry::register` refused any signature
   longer than `MAX_CALL_ARGS` (8). `ANIM_STATE`'s 18-operand shape failed
   registration, so all three sites refused as `unknown host call` and
   `call_arguments` was unmet. #806 generalized M02-B-FU1's list-argument
   carrying to `AnimationStates`: the operand list is the call's one
   `Value::List` argument, all three sites bind and `MAX_CALL_ARGS` stays 8.

No `MissionProgram` assembled, `MissionProgram::validate` was never reached,
the census reported M04's row incomplete and `campaign_ready()` stayed false
— the contract's honest reading at the time.

**The fix landed as M04-B-FU1 (#806).** The stage A–D findings measured the
original's parse helper for this key
(`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`,
`ANIM_STATE`): at `0x4691d0` it **walks the value list** for a tag-3 `ANIM`
followed by a tag-4 spec record, appends *every* pair it finds and increments
`required` once per pair; after the walk it reads a `COMPLETION_COUNT` out of
that same list and the integer **overwrites** `required`. M04's spelling is
what that measured walk describes; the engine's single-pair shape was derived
from M01, whose three sites all spell exactly one pair. #806 re-read the
helper and the sibling lookup's scope in the decrypted image — confirmed —
and implemented exactly this: the multi-pair append, the in-list override,
the first-site selection and the list-carried call, with `MAX_CALL_ARGS`
unchanged. The follow-up record is
`docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`.

The synthetic tests now carry the measured arms on authored records so CI
covers them without original data: multi-pair sites lower with the count
override, the operand list binds as one call argument inside the bound, the
first `ANIM_STATE` site alone arms the evaluator, a top-level
`COMPLETION_COUNT` stays inert, and only an operand list past
`MAX_VALUE_ITEMS` still refuses — at the key's registration, never
truncated. The pins moved in the same change that widened the mechanism —
never deleted to get green.

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

## Test inventory (`accept_m04_b_*`, 10 tests — the four M04-B pins, the six the follow-up rewrote)

| Test | What it pins |
| --- | --- |
| `accept_m04_b_the_control_program_is_the_member_that_declares_the_blocks` (retail) | the container's length and digest re-derive from disk; exactly one of the 15 members declares numbered blocks and it is the one the rule picked, the eighth member, with longer members beside it; the record is 52/201/40; an independent walk sees blocks 1..=52 and 201 sites with no refusal and no unclassified record key; the production control binding names the same mission, program, container, digest, member, span and record; the member's own digest re-derives from its bytes |
| `accept_m04_b_every_directive_m04_spells_has_a_disposition_and_none_is_refused` (retail) | the partition is exactly 2 terminal + 38 measured + 0 unmeasured; the sites sum to 201; both outcome keys answer for their own name and are spelled `Bare` |
| `accept_m04_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations` (retail) | the three priorities' keys resolve to the measured operations; `TRAVELERS`' two boundaries with their actors and radii; the four `START_TAXI` vehicle names; the two `WAKE_ANIM` targets; `SET_HELP_LABEL … MSG_OBJ_DEFEND` over both carriers; the seven `DEDG` pairs; both `INACTIVE_COMPLETION_COUNT` thresholds against twelve-member lists that all hang off `piratezep`; `TRAVELERS`' three unknowns are still carried |
| `accept_m04_b_the_terminal_blocks_are_gated_and_every_address_is_in_range` (retail) | `INSTANTWIN` in block 32 and `INSTANTLOSS` in block 41 and nowhere else, both `BEGIN_DORMANT -1`; only blocks 1 and 2 arm a timed self-wake and neither latch is among them; blocks without a dormant marker; the address walk's per-key totals (24/7/23/4 = 58) with no out-of-range address; each latch's in-degree is exactly one completion edge — block 31's nap of 32 and block 27's nap of 41, reconciled from the zero-based misreading by #819; nothing gates on the failure latch, and block 42's gate targets block 40 |
| `accept_m04_b_fu1_the_multi_pair_anim_state_sites_lower_and_m04s_record_completes` (retail, #806) | `ANIM_STATE`'s 3 sites in two shapes (2 operands ×1, 18 ×2); all 201 calls bind with no unbound key; all 52 conditions lower — blocks 23 and 37 hold eight descriptors with `required` 1 and 3 from the in-list `COMPLETION_COUNT`, block 32 the one `EXECUTED` pair; `MissionProgram::validate` accepts and the row is complete |
| `accept_m04_b_fu1_m04_is_complete_and_the_campaign_stays_unready` (retail, #806) | M04 is a complete census row, the campaign gate still stays closed on other missions' gaps, and the row is still reported as measured |
| `accept_m04_b_fu1_a_multi_pair_site_lowers_with_its_count_override` (synthetic, #806) | on authored records: the `COMPLETION_COUNT` + two-descriptor spelling lowers the measured way — both pairs appended in order, the count overwriting `required`, the operand list binding as one call argument |
| `accept_m04_b_fu1_only_an_uncarriable_operand_list_refuses` (synthetic, #806) | an operand list past `MAX_CALL_ARGS` binds as one list argument; a list past `MAX_VALUE_ITEMS` is uncarriable, so the key refuses registration rather than truncating and the damaged block's condition refuses beside it |
| `accept_m04_b_fu1_the_first_site_arms_the_evaluator` (synthetic, #806) | the measured first-match selection: the first `ANIM_STATE` text's operand list arms the block's one evaluator and a second site contributes nothing — not pairs and not a refusal |
| `accept_m04_b_fu1_a_top_level_completion_count_is_inert_and_unbound` (synthetic, #806) | a block-level `COMPLETION_COUNT` directive is never read into the animation evaluator (the parse only looks the key up inside the selected operand list) and its own call still refuses as an unmeasured key |

Every test calls production code (`SourceContext::control_program`,
`survey_mission_control_programs`, `read_control_member`,
`measure_control_record`, `lower_control_record`,
`cs_assets::install::sha256`). No test repeats an expected value read from the
record it checks: the retail assertions re-derive from `$CS_GAME_DIR` through a
second production walk, or compare two independent derivations against each
other. The retail tests are `#[ignore = "requires CS_GAME_DIR"]` (CI skips
them); the two synthetic ones run in CI.

## Mutation probes

Three mutations were applied one at a time, each on top of the committed
branch, observed on `cargo test -p cs_app --test campaign accept_m04_b_ --
--include-ignored` and reverted before the next; `git status --porcelain`
after the last one was clean.

| Mutation | Observed result |
| --- | --- |
| `cs_content::mission_control::terminal_outcome_of`: `INSTANTWIN` → `None` | **2 of 8 fail** — the vocabulary test (the outcomes map loses `INSTANTWIN: Succeeded`) and the gap test (the success latch stops being a terminal key, so it no longer binds as `Lowering::Finish` and the bound-call count moves off 198) |
| `cs_script::bindings::MAX_CALL_ARGS` 8 → 32 (the follow-up's direction) | **1 fails** — the gap pin: no shape is over the bound any more, so `longest > MAX_CALL_ARGS` breaks. The synthetic bound test *adapts* to the new constant and still passes — #806 updated that pair by keeping the constant and moving the width into one list argument instead, and the refusal arm now lives at `MAX_VALUE_ITEMS` |
| `cs_content::mission_control::control_member`: the `objective_blocks_of(member) > 0` filter accepts every member | **6 of 8 fail** — every retail test, because the whole census refuses `zbd/c1/ia1/zrdr.zbd` as ambiguous the moment the rule stops choosing. The two synthetic tests, which author their own records, are unaffected |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked -- accept_m04_b_ --include-ignored` | 0 (8 tests: this stage's 6 retail and 2 synthetic members) |
| `cargo test --workspace --locked` | 0 |

The three mutation probes above were run between the workspace checks, each
reverted before the next; `git status --porcelain` after the last one was
clean. The acceptance run that the evidence report records is the same task
selection, tee'd into `private/evidence/M04-B/cargo-test.log`; the report is
validated with `tools/validate_evidence.py --require-pass` and copied to
`docs/findings/evidence/M04-B.json`.

## Recorded unknowns (not guessed)

- **The join remains an inference** (M04-A's standing unknown): this stage
  consumes it through `SourceContext::control_program` and adds no second
  evidence for the mapping itself.
- **`ANIM_STATE` did not lower** — the measured gap above, closed by
  M04-B-FU1 (#806) through the generalized operand-list walk this document's
  fix-direction section described; M04's row is complete since that landing.
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
