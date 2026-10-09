# M06-B: The Red Menace's mission-specific compatibility surface

Date: 2026-10-09. Task: M06-B "Implement and regress mission-specific
compatibility gaps" (#274, `missions/M06.md`, work order `M06-B`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written) and `synthetic` (newly
authored `.zrd` records). Implementer: **opencode-1/opencode-1** (Rally #274,
session of 2026-10-09). No reviewer yet; the implementer's own run is not
independent review and no agent review replaces the owner's human approval.

## Review identity

The evidence harness (`evidence_report_m06_b_writes_the_acceptance_report`)
reads `CS_EVIDENCE_REVIEWER` at run time and fills `review.identity` whole, so
the committed `docs/findings/evidence/M06-B.json` cannot contain a hand-over
placeholder: whoever runs the harness writes its own identities. This
implementer's run is recorded as exactly that — the implementing agent at
hand-over, not a review — and the Rally reviewing agent regenerates the report
on the rebased commit with its own `CS_EVIDENCE_REVIEWER` value, saying whether
its context was fresh. M06-B adds no Rally review facts to
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json`: that file
records merge events that do not exist yet, and an entry written before the
review would be writing a review fact that has not happened.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M06's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C2/M01/zrdr.zbd` — and this stage runs it through
production systems (`SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`) and regresses it with eight `accept_m06_b_*`
tests. One compatibility gap remains **measured and pinned, not worked
around**: `MissionProgram::validate` refuses the three `ANIM_STATE`
completion-count condition sites, so M06 stays not ready. That fix belongs to
the condition parser and is filed as **M06-B-FU1** (#817); this stage names it
exactly.

Nothing here is `verified_original` (AGENTS.md rule 8): the work-order ↔
mission join remains M06-A's inference, directive *effects* are the stage A–D
findings' static readings of the original code, and no original
executable has been run.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions, the record → `RawProgram` adapter, the `SourceContext::control_program`
binding and M02-B-FU1's list-shaped lowering are M01-LC's, M02-B's and
#800's; this stage runs them over M06 and pins the result in
`crates/cs_app/tests/campaign/m06_b.rs` (six retail and two synthetic
`accept_m06_b_*` tests) plus the evidence harness in `evidence.rs`. Wiring:
`crates/cs_app/tests/campaign/main.rs` (`mod m06_b;` and a doc paragraph).
No `Cargo.toml` or `Cargo.lock` change. `missions/bindings/M06.json` is
unchanged: the blocks this stage measures are not objective *content ids*, so
the record keeps its empty `objective_ids` and its "objective graph: not
bound" unknown exactly as M02-B and M04-B kept theirs.

## What was read from the installation

Measured on this installation, re-derived by the acceptance tests on each run:

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` |
| Reader archive | `ZBD/C2/M01/zrdr.zbd`, 100 905 bytes, SHA-256 `d6e9315d…1fd9f63` — the program span `missions/bindings/M06.json` cites |
| Members offered | 15, every one decoding as a `.zrd` document |
| Control member | `objectives.zrd`, offset 17 166, length 18 651, own SHA-256 `3b315798…353b534c`, the **eighth** member; `barge.zrd` (24 695) and `goosepath.zrd` (23 642) are longer than it |
| Record | 82 numbered blocks (`OBJECTIVE1`…`OBJECTIVE82`, no gaps), 265 directive sites, 26 distinct keys |
| Vocabulary partition | 2 terminal keys (`INSTANTWIN` ×1 site, `INSTANTLOSS` ×1 site, both bare), **24 measured** keys, 0 unmeasured, 0 refusals, 0 unclassified record-level keys |
| Record-level fields | `MISSION_TIMER`, `PLAYER_INIT` and the three empty animation lists — each once |
| Lowering attempt | **265 of 265 sites bind**, no unbound key, 79 of 82 conditions lower; a `RawProgram` stands for all 82 objectives and carries `mission/ch2-m01`, and `MissionProgram::validate` then **refuses** it with one diagnostic naming `objective#8 [condition]` |
| Requirements | `MissionIdentity` met, `ObjectiveIdentity` met, `objective_condition` **unmet** (the three `ANIM_STATE` sites plus the keys' carried unknowns), `call_arguments` **unmet** (it carries validate's refusal among its fields: the accounting never reports a clean row while `validate` failed) |
| Census context | 53 mission-scoped readers, 40 with control programs; M06's row is measured but **not complete**, `campaign_ready()` false |
| Cross-check | `SourceContext::control_program("M06", "The Red Menace")` and `survey_mission_control_programs` agree on the container, its digest, the member, its span and the whole record |

The 15 members, in archive order, each with the block count that qualified or
excluded it under the rule:

| member | offset | length | blocks |
| --- | --- | --- | --- |
| `aiv.zrd` | 0 | 14 150 | 0 |
| `dzones.zrd` | 14 150 | 465 | 0 |
| `egen.zrd` | 14 615 | 374 | 0 |
| `location.zrd` | 14 989 | 378 | 0 |
| `map.zrd` | 15 367 | 651 | 0 |
| `mis_anim.zrd` | 16 018 | 812 | 0 |
| `net.zrd` | 16 830 | 336 | 0 |
| `objectives.zrd` | 17 166 | 18 651 | **82** |
| `startanims.zrd` | 35 817 | 228 | 0 |
| `targets.zrd` | 36 045 | 1 083 | 0 |
| `weather.zrd` | 37 128 | 3 218 | 0 |
| `barge.zrd` | 40 346 | 24 695 | 0 |
| `goosepath.zrd` | 65 041 | 23 642 | 0 |
| `security_destroy.zrd` | 88 683 | 9 531 | 0 |
| `zepstate.zrd` | 98 214 | 463 | 0 |

Size and position are measurably **not** the rule: the two longest members
carry no block at all, and the control member is the eighth, not the first.
The test asserts both facts.

## Where the M06 sheet's regression priorities live

The sheet's priorities are *designs*; the predicates must come from the source
program and reference runs. This stage locates each one in the measured record
— with the actor names the original actually spells — so a later runtime stage
knows exactly what to predicate, and it assigns no timing, count or coordinate
the record does not spell:

- **Subsystem disablement.** Four `INACTIVE_COMPLETION_COUNT` thresholds
  (blocks 50, 72, 73, 74: `4`, `1`, `2`, `3`), each over the same eight
  member chains `g_engine1`…`g_engine8` `healthy_part` — a carrier whose
  engines clear one at a time or four together. The same evaluator with a
  single member is spelled nine times beside it: `kkgate healthy` (block 4)
  and `tugandbarge01…04 thlthy` in blocks 13, 17, 21, 25 and again in 33–36.
  `targets.zrd` carries the engine part as a target of its own
  (`MSG_TRGT_SGOOSE_ENGINE` over `nodes [healthy_part]`), which is how a
  disabled part reaches the player's target info. Five `DEDG` group-depletion
  thresholds (blocks 12, 43, 46, 65, 68) spell `[1,2] [2,0] [1,0] [1,2] [2,0]`
  — record data, not a guess.
- **Multi-step interaction.** The target-flag chain walks `propane` →
  `sprucegoose` → `tugandbarge01…04` across blocks 4, 8, 13, 17, 21 and 25:
  `ADD_OBJECTIVE_TARGET` spells the next object at each step, and
  `REMOVE_OBJECTIVE_TARGET` clears the growing list of the previous ones
  (`[tugandbarge01]`, `[01,02]`, `[01,02,03]`, `[01..04]`). The repeatable
  side is the nap: 35 `NAP_OBJECTIVE_WHEN_I_COMPLETE` sites, each exactly
  `[target block, delay seconds]`, and the measured effect clears the target's
  completed flag so it can complete again. The AI side is eight
  `SET_AI_NET` sites: six single re-pointings of `patrolboat_eg0…eg5` to
  `M2GoosePatrol` (blocks 58–63), then all six to `M2PatrolStop` (block 70)
  and four `hkfirebrand_*` to `M2Third` (block 71).
- **Passenger identity: no program binding exists.** The archive spells four
  named locations — `Airport_terminal`, `Passenger_hangar`, `Crops`, `Coast` —
  each in `location.zrd` and **nowhere else**, and no directive site of the
  control record names any of them. `Passenger_hangar` is the whole archive's
  only passenger-named string; the actor-init member `aiv.zrd` spells no
  passenger (its names are `Player`, `wingman_1/3`, `Big John`, `Buck`, `Jack`,
  `Steele`, `Tex`, the `Security*`/`secfury_*`/`secgyro_*` and
  `hkfirebrand_*`/`patrolboat_eg*` vehicles), and `targets.zrd` defines seven
  targets, none of them a passenger. M06-A's binding still records
  *interaction authorizations* as an unknown, so this stage cannot quietly
  read one into the record. The priority therefore has **no actor or program
  binding to predicate yet**; nothing here invents one, and the search for
  where that entity lives is filed as **M06-B-FU2** (#818).

Every key above is asserted to resolve to the operation the shared findings
measured (`DirectiveOperation::{InactiveMembers, InactiveThreshold,
EnemyGroupDepletion, SetTargetFlag, AssignNet, NapObjective, WakeObjectives,
KillObjectives, DormantStart}`), so a key that silently lost its meaning fails
the run, and `INACTIVE1`'s carried unknown (the untraced writers of the
in-play bit) is asserted to still be there.

## The block graph, and what makes the addressing measurable

Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT -1`):

| latch | block | spelled by | edges into its number |
| --- | --- | --- | --- |
| `INSTANTWIN` | 47 | block 46 `NAP_OBJECTIVE_WHEN_I_COMPLETE [47, 25.0]`, block 50 `KILL_OBJECTIVE_WHEN_I_COMPLETE […, 46, 47]` | 2 |
| `INSTANTLOSS` | 51 | block 50 `NAP_OBJECTIVE_WHEN_I_COMPLETE [51, 30.0]` | 1 |

No other block ends the mission. Of the 64 `BEGIN_DORMANT` markers only blocks
1, 2 and 3 — the mission start — arm a timed self-wake, so neither latch can
fire on its own clock; 18 blocks carry no marker at all and are entered
through a predecessor's edge. The address walk visits 107 spelled integers —
25 wake, 47 kill, 35 nap, no gate key — and every one lies in `1..=82`.

Two values of the record make its addressing **measurable rather than
assumed**; they were the discriminators while this stage was still in flight,
and during that window M02-B-FU3 (#802) settled the rule by measurement:

- block 67 spells `82`, the block count itself. Under the block-number reading
  it is the last block and in range; under an index reading index 82 would sit
  past the end of an 82-element array;
- **nothing spells `50`**, although block 50 naps `51`. Under the block-number
  reading the failure latch has exactly one completion edge; under an index
  reading its predecessor-less number would leave `INSTANTLOSS` unreachable
  and M06 with no failure path at all.

M02-B-FU3 read the original's parse out of the decrypted executable and found
the `dec` (`0x468c40` for the wake array, `0x468cf0` for a nap's target,
`0x4679fc` for the dependency gate) that decrements every objective address
before storing it — with `DEDG`'s two integers and the nap's seconds stored
*without* it as the controls — so **a spelled address is the one-based block
number**, exactly what M06's two discriminators require, and the engine-side
rule now lives in `crates/cs_sim/src/objectives/address.rs` (`[1, objectives]`,
mapping to index `address - 1`). This stage's pins therefore agree with the
landed measurement, and the test's doc comment cites it rather than asserting
a convention of its own.

One inconsistency remains and is **not** this stage's to edit:
`crates/cs_app/tests/campaign/m04_b.rs` still reads an address as a zero-based
index (`incoming(target)` matches `addresses.contains(&(target - 1))`), which
the settled rule contradicts. That is the narrowed scope of **M06-B-FU3**
(#819); M02-B's own pin was reconciled by #802 itself.

## The M06-specific gap, and why it is not papered over

M06's directive vocabulary is **fully measured**: 26 of 26 keys carry a stage
A–D disposition, none is Unmeasured, no block refused to decode and no
record-level key sits outside the vocabulary. Every call binds. Yet the
mission is not ready, because `MissionProgram::validate` refuses the lowered
program — and the sole reason is one key, `ANIM_STATE`, spelled at eight sites
in two shapes:

| shape | sites | blocks |
| --- | --- | --- |
| 2 operands: `ANIM [NAME …, STATE [RUNNING]]` | 5 | 37, 38, 39, 40, 69 |
| 6 operands: `COMPLETION_COUNT [1]` + two `ANIM` descriptors | 3 | 9, 11, 41 |

1. **Condition half (M06's gap).**
   `cs_script::conditions::anim_state` accepts exactly two operands — the tag
   `ANIM` and one spec record — which is the shape M01's and M06's other five
   sites happen to spell. Blocks 9, 11 and 41 spell six, so all three refuse
   with the measured shape named, 79 of 82 conditions lower, `objective_condition`
   is unmet, `MissionProgram::validate` reports `objective#8 [condition]:
   unsupported instruction …`, and `call_arguments` stays unmet behind that
   refusal. Six operands are well inside `MAX_CALL_ARGS`, so this is *not* a
   host-call-bound problem: the key registers and all eight of its sites bind
   as calls.
2. **Call half (closed while this stage was in flight).** When this suite was
   first written (base `99f57073`), M06's fifteen
   `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites all refused: two of them spell
   twelve integers, `registry_for` registered one signature per measured
   shape, and `HostBindingRegistry::register` refused any signature longer
   than `MAX_CALL_ARGS` (8) — so the whole key failed registration and the
   refusal named `binding … too many arguments`, exactly M02-B-FU1's class.
   M02-B-FU1 (#800) landed on `main` during this task; it carries a
   list-taking directive's spelled list as one `Value::List` argument instead
   of a positional row, so the list's length stops being read as an arity. The
   suite was **re-measured on that landing** (as M04-B-FU1's scope note
   instructs): all 265 sites now bind, `unbound_keys` is empty, a program
   stands, and the gap pin above was rewritten to the condition half rather
   than deleted. The synthetic pair still pins the bound itself, on a key that
   takes no index list.

No `MissionProgram` passes validation, the census reports M06's row
incomplete and `campaign_ready()` stays false. That is the contract's honest
reading — an unlowerable program is `Unsupported`, never a guessed one — so
nothing in this stage loosens it.

**The direction the fix likely takes, with its evidence.** The stage A–D
findings measured the original's parse helper for this key
(`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`,
`ANIM_STATE`): at `0x4691d0` it **walks the value list** for a tag-3 `ANIM`
followed by a tag-4 spec record, appends *every* pair it finds and increments
`required` once per pair; after the walk it reads a `COMPLETION_COUNT` out of
that same list and the integer **overwrites** `required`. M06's spelling is
what that measured walk describes. The engine's single-pair shape was derived
from M01, whose three sites all spell exactly one pair. That is evidence, not
a decision: the sibling lookup's scope and the multi-pair append must be
confirmed against the original before anything changes. Filed as **M06-B-FU1** (#817),
which states that the condition half is the same class as M04-B-FU1 (#806,
M04-scoped) and that #806 and this task should be coordinated rather than
duplicated.

The two synthetic tests carry both arms on authored records so CI covers them
without original data: a single animation pair lowers and completes while
M06's `COMPLETION_COUNT` + two-descriptor spelling is refused per site (with
the key still registering, so the refusal is the shape); and a twelve-target
kill list binds as one argument while `SET_AI_NET` with nine pairs — a key
that takes no index list — still refuses the key's whole registration on the
unchanged bound. Whichever follow-up widens either mechanism must update both
pins in its own change — never delete them to get green.

## Not claimed

No playthrough, difficulty branch, media or presentation row (M06-SUCCESS …
M06-DIFFICULTY) is covered: those need the runtime and human play (M06-C). In
particular the sheet's *"wrong actor, wrong session, repeated event"* halves of
all three priorities are **runtime** predicates and stay unobserved; nothing
here simulates them, and the record's before/at/after boundaries are named but
not exercised. What the in-play bit's writers do, what `DEDG`'s three rewritten
member fields feed, what a sound-group handle resolves to, which HUD element
draws a target flag and what a `SET_AI_NET` re-seed copies out of the node
list remain as unknown as the M01-LC findings left them — they are listed on
the measured keys, never dropped.

## Files

- `crates/cs_app/tests/campaign/m06_b.rs` — the eight acceptance tests.
- `crates/cs_app/tests/campaign/evidence.rs` — the evidence harness and the
  task's test lists.
- `crates/cs_app/tests/campaign/main.rs` — wiring only (`mod m06_b;` and a doc
  paragraph).
- `docs/findings/evidence/M06-B.json` — the committed copy of the acceptance
  report (the artifacts it hashes stay in `private/evidence/M06-B/`).

## Test inventory (`accept_m06_b_*`, 8 tests)

| Test | What it pins |
| --- | --- |
| `accept_m06_b_the_control_program_is_the_member_that_declares_the_blocks` (retail) | the container's length and digest re-derive from disk; exactly one of the 15 members declares numbered blocks and it is the one the rule picked, the eighth member, with two longer members beside it; the record is 82/265/26; an independent walk sees blocks 1..=82 and 265 sites with no refusal and no unclassified record key; the production control binding names the same mission, program, container, digest, member, span and record; the member's own digest re-derives from its bytes |
| `accept_m06_b_every_directive_m06_spells_has_a_disposition_and_none_is_refused` (retail) | the partition is exactly 2 terminal + 24 measured + 0 unmeasured; the sites sum to 265; the sorted vocabulary is exactly the 26 keys; both outcome keys answer for their own name and are spelled `Bare` |
| `accept_m06_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations` (retail) | the three priorities' keys resolve to the measured operations; the four engine thresholds over the eight `g_engine*` chains; the nine single-member chains in order; the five `DEDG` pairs; the seven `targets.zrd` descriptions and the engine target's node; the target-flag chain's five adds and five removes with their names; the 35 naps' `[int, float]` shape; the eight `SET_AI_NET` re-pointings; the four locations spelled only in `location.zrd`, no directive naming one, no passenger actor in `aiv.zrd`, and M06-A's interaction unknown still carried; `INACTIVE1`'s unknown still present |
| `accept_m06_b_the_terminal_blocks_are_gated_and_every_address_is_in_range` (retail) | `INSTANTWIN` in block 47 and `INSTANTLOSS` in block 51 and nowhere else, both `BEGIN_DORMANT -1`; 64 markers with only blocks 1–3 arming a timed self-wake; the 18 markerless blocks; the address walk's per-key totals (25/47/35 = 107) with no out-of-range address; `82` is spelled and `50` is not; each latch's spelled edges (47 by block 46's nap and block 50's kill, 51 by block 50's nap) |
| `accept_m06_b_the_anims_state_sites_are_the_only_gap_and_validation_refuses` (retail) | `KILL`'s three shapes (1×8, 3×5, 12×2) with the twelve-operand one past `MAX_CALL_ARGS`, and yet every call binds with no unbound key; a program stands for 82 objectives and `validate` refuses it naming `objective#8 [condition]` and `ANIM_STATE`; 79 of 82 conditions lower and the three refusals name blocks 9, 11 and 41 with the measured shape; `objective_condition` and `call_arguments` are the only unmet rows and the first names the three sites; the row is incomplete |
| `accept_m06_b_the_mission_stays_unready_until_the_refused_sites_are_measured` (retail) | M06 is not a complete census row, the campaign gate stays closed, and the row is still reported as measured |
| `accept_m06_b_a_single_animation_pair_lowers_and_a_completion_count_site_is_refused` (synthetic) | on authored records: one pair is the measured shape and the record completes; M06's `COMPLETION_COUNT [1]` + two-descriptor spelling is refused per site with the measured shape named; six operands are inside the bound, so the key still registers and the refusal is the shape |
| `accept_m06_b_a_wide_kill_list_binds_and_a_wide_non_index_key_still_refuses` (synthetic) | on authored records: a twelve-target kill list binds as one argument and the record completes (the mechanism #800 gave M06); `SET_AI_NET` at 8 pairs registers and at 9 refuses the key's registration, its site refuses and the key is absent from the registry — the bound itself never moved |

Every test calls production code (`SourceContext::control_program`,
`SourceContext::bind`, `survey_mission_control_programs`,
`read_control_member`, `measure_control_record`, `lower_control_record`,
`cs_assets::install::sha256`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`). No test repeats an expected value read from
the record it checks: the retail assertions re-derive from `$CS_GAME_DIR`
through a second production walk, or compare two independent derivations
against each other. The retail tests are `#[ignore = "requires CS_GAME_DIR"]`
(CI skips them); the two synthetic ones run in CI.

## Mutation probes

Four mutations were applied one at a time on top of the committed branch,
observed on `cargo test -p cs_app --test campaign accept_m06_b_ --
--include-ignored` and reverted before the next; `git status --porcelain`
after the last one was clean. They were run before this branch's two rebases,
and neither rebase changed any of the three mutated files
(`cs_content::mission_control`, `cs_script::bindings`,
`cs_app::control_lowering`), so the results still describe this commit.

| Mutation | Observed result |
| --- | --- |
| `cs_content::mission_control::terminal_outcome_of`: `INSTANTWIN` → `None` | **2 of 8 fail** — the vocabulary test (the outcomes map loses `INSTANTWIN: Succeeded` and the key is no longer spelled `Bare`) and the gap pin (the success latch stops being a terminal key, so it no longer binds to `Lowering::Finish` and the bound-call count moves off 265) |
| `cs_script::bindings::MAX_CALL_ARGS` 8 → 32 | **1 fails** — the gap pin's `longest > MAX_CALL_ARGS` assertion: no shape is over the bound any more. The synthetic pair *adapts* to the new constant and still passes, which is exactly the pair M06-B-FU1 (#817) must update together; its refusal arm keeps the mechanism covered either way |
| `cs_content::mission_control::control_member`: the `objective_blocks_of(member) > 0` filter accepts every member | **6 of 8 fail** — every retail test, because the whole census refuses `zbd/c2/m01/zrdr.zbd` as ambiguous the moment the rule stops choosing. The two synthetic tests, which author their own records, are unaffected |
| `cs_app::control_lowering::takes_index_list`: drop `KillObjectives` from the or-pattern (M02-B-FU1's mechanism, removed) | **2 fail** — the gap pin (the twelve-target kill sites refuse again: `unbound_keys` names `KILL_OBJECTIVE_WHEN_I_COMPLETE: … too many arguments`) and the synthetic kill arm, which reports the same refusal on an authored record |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked -- accept_m06_b_ --include-ignored` | 0 (8 tests: 6 retail and 2 synthetic) |
| `cargo test --workspace --locked` | 0 |

The acceptance run that the evidence report records is the same task
selection, tee'd into `private/evidence/M06-B/cargo-test.log`; the report is
validated with `tools/validate_evidence.py --require-pass` and copied to
`docs/findings/evidence/M06-B.json`.

## Recorded unknowns (not guessed)

- **The join remains an inference** (M06-A's standing unknown): this stage
  consumes it through `SourceContext::control_program` and adds no second
  evidence for the mapping itself.
- **`ANIM_STATE` does not lower** — the measured gap above. M06 stays
  `Unsupported`; the campaign gate stays closed; the fix is M06-B-FU1 (#817).
- **Passenger identity has no binding.** The `Passenger_hangar` location is
  referenced by nothing in the mission's reader archive and no actor, target or
  directive carries a passenger. Where (if anywhere) that entity lives —
  another data file, an interaction record, or only a reference run — is
  **M06-B-FU2** (#818); nothing here assigns it a predicate.
- **One sibling suite still reads an address as an index.** The rule itself is
  no longer open: #802 measured the parse's decrement and this record's two
  discriminators agree with it (above). `m04_b.rs`'s `incoming` pin does not,
  and reconciling it is **M06-B-FU3** (#819); no suite other than this
  stage's own was edited.
- **The runtime halves of all three priorities are unobserved.** Whether the
  wrong actor, the wrong session or a repeated event can satisfy an engine
  threshold, a target-flag step or a passenger pickup is ordinary-play
  behaviour (M06-C) and stays open. No reference capture exists for M06
  (REF-OWNER-FIRST-CAPTURE is blocked on the owner).
- **Which other reader members carry gameplay this stage does not decode**
  (`aiv.zrd`, `barge.zrd`, `goosepath.zrd`, `security_destroy.zrd`,
  `zepstate.zrd`, …) is unmeasured here: the control program is one member of
  fifteen. Their *text* names were read only to locate the location and actor
  vocabularies above.
- **`M06.json`'s unknowns are unchanged.** The block graph this stage measures
  is not the binding's `objective_ids`, and the checklist entries M06-A left
  unknown (actor/spawn/route sets, audio cues, difficulty branches, success
  precedence, …) stay unknown there and here.
- **M06 is not launchable** (`cs_app::mission_launch` declares M01 only) and
  no engine consumer runs this record yet; both belong to the runtime stages.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`; `missions/M06.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-01-m06-a-source-binding.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`;
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`;
`docs/findings/2026-10-08-m03-b-control-program-gaps.md`;
`docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`;
`crates/cs_sim/src/objectives/address.rs`;
`crates/cs_app/tests/campaign/m02_b.rs`;
`crates/cs_app/tests/campaign/m03_b.rs`;
`crates/cs_app/src/mission_control.rs`;
`crates/cs_app/src/control_lowering.rs`;
`crates/cs_script/src/conditions.rs`.
