# M18-B: Deceit at Devil's Horn's mission-specific compatibility surface

Date: 2026-10-10. Task: M18-B "Implement and regress mission-specific
compatibility gaps" (#310, `missions/M18.md`, work order `M18-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic` (newly authored `.zrd`
records). Implementer: **bunny-alpha-2/bunny-alpha-2** (Rally #310, session of
2026-10-10). A review round, when Rally runs one, is recorded at the end of
this note; an agent review never replaces the owner's human approval and
nothing here is `verified_original` (AGENTS.md rule 8).

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M18's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C4/M03/zrdr.zbd` — and this stage runs it through
production systems (`SourceContext::control_program`, `SourceContext::bind`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`, `cs_app::animation::mission::bind_mission_animation`
and `cs_sim::objectives::address`'s convention as measured by M02-B-FU3 /
M06-B-FU3) and regresses it with fifteen `accept_m18_b_*` tests. One
compatibility gap is left **measured and recorded, not worked around**: see
*The one measured gap* below.

## What changed

No production code and no change to `missions/bindings/M18.json`: M18's record
lowers completely, so there was no lowering gap to close and no binding
statement to correct. Added:

* `crates/cs_app/tests/campaign/m18_b.rs` — fifteen `accept_m18_b_*` tests
  (twelve retail, three synthetic);
* `crates/cs_app/tests/campaign/evidence/m18_b.rs` — the M18-B evidence
  harness, plus its one `mod` line in `evidence.rs`;
* `docs/findings/2026-10-10-m18-b-compatibility-gaps.md` (this note) and
  `docs/findings/evidence/M18-B.json` (the committed report copy).

Wiring only: `crates/cs_app/tests/campaign/main.rs` (`mod m18_b;`).

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as M01-A … M18-A) |
| Reader archive | `ZBD/C4/M03/zrdr.zbd`, 90 978 bytes, SHA-256 `0eff1e94…87c002` (M18-A's own program span) |
| Members | 17; exactly one declares numbered blocks — `objectives.zrd`, the 8th member, offset 22 381, 16 300 bytes, SHA-256 `83e5f70d…c191d`. The two longer members (`aiv.zrd`, `zep_dock.zrd`) declare none, so neither size nor position picks it |
| Control record | 52 numbered blocks, 238 directive sites, 29 distinct keys, no block refusal, no record key outside the measured vocabulary |
| Dispositions | 27 measured, 2 terminal (`INSTANTWIN` block 25, `INSTANTLOSS` block 11), **0 unmeasured, 0 refused** |
| Lowering | mission `mission/ch4-m03`, 52 `RawObjective`s, all 238 calls bound, all 52 conditions lowered, `MissionProgram::validate` clean, 0 unmet requirements, census row **complete** |
| Campaign | `zbd/c4/m03` is one of the census's 14 complete rows; `campaign_ready()` is still false (other rows carry their own gaps) |
| Record-level fields | `MISSION_TIMER`, `PLAYER_INIT`, and the three (empty) animation lists; no record-level sound key |
| Production bindings | `SourceContext::control_program` reaches the same mission (`mission/ch4-m03`), program (`script/c4-m03-zrdr`), member, span and both digests; M18-A's record cites `ZBD/C4/M03/zrdr.zbd` (0, 90 978) and one row of `langui.dll` |
| Animation binding | `bind_mission_animation(…, "zbd/c4/m03")` reads three reader archives, two carriers (`mis_anim.zbd` 355/355 records, `cam_anim.zbd` 380/380, no blocker) and ten startup rows: 7 playable, 3 refused |
| Whole installation | 184 `.zbd` containers decoded member by member through production discovery for the declaration walk; all 228 files raw-scanned for the byte claims |

## The three regression priorities, as the record spells them

**Alternative action order.** Seven blocks are awake when the mission starts —
2, 8, 9, 10, 14, 46, 50 — and only block 1 arms a clock (2 s); the other 45
blocks spell `BEGIN_DORMANT -1`. The record observes **one** release twice:
blocks 4 and 46 spell the identical five-member chain (`tiedown01..04`,
`zmainclamp`, each `…/healthy`) under the same `INACTIVE_COMPLETION_COUNT 1`,
and both lower to the same `InactiveMembers` condition. Block 46 is eligible
from the first tick and wakes 5 and 47 and kills 2, while block 4 is entered
only by block 3's wake list and naps 5 after 1 s — block 5's two prerequisites
are exactly `[4, 46]`, so the same transition is reachable through either path.
The five per-clamp blocks 26–30 are entered together by block 3's single wake
list (`4, 26, 27, 28, 29, 30`) and no prerequisite edge joins any pair of them:
the record permits those five releases in any order. What the original does at
each boundary — wrong actor, wrong session, a repeated event — is a runtime
observation and stays unmeasured here (M18-C).

**Release dependencies.** `TICK_DEPENDS_ON_OBJ` is the only dependency
directive M18 spells: block 24 on block 23 and block 45 on block 44, two sites,
both `child0 − 1` in the lowered program (the dependency's own zero-based
index, the convention M02-B-FU3 #802 measured and M06-B-FU3 #819 reconciled).
The two dependency blocks are dormant and identical in shape — `BEGIN_DORMANT
-1` plus one `INACTIVE1("piratezep")` — so each lowers to its own in-play
predicate under the default threshold. The gated block keeps its own wake
conjunct and gains the dependency's: block 24 lowers to
`Awake{23} ∧ Awake{22} ∧ EnemyGroupDepletion{group 2, remaining 0}` and block
45 to `Awake{44} ∧ Awake{43}`. Order is therefore a gate on evaluation, never
a timing guess, and the second half of the sheet's rule — that the *wrong*
actor or a *repeated* event cannot satisfy the gate — is a runtime observation
left to M18-C.

**Rescue interaction.** The sheet's label names no key, so nothing here
inventories one. What the record spells on that theme:

* the record's only `TRAVELERS` site, block 3 — `player` `APPROACHING`
  `cargozep1` at radius 1500, the mission's single approach predicate, lowering
  to `Condition::Travelers { subject: ["player"], anchor: Object(["cargozep1"]),
  radius: 1500.0, approaching: true }`;
* the five per-clamp blocks 26–30, each pairing `INACTIVE1(name, healthy)` with
  a `REMOVE_OBJECTIVE_TARGET` of the same name, and the approach block's own
  pair (`tiedown01..04` + `zmainclamp` added, `cargozep1` removed);
* the actors themselves, declared by the mission archive's own members: the
  four tiedowns by `targets.zrd` alone, `zmainclamp` by `targets.zrd` and
  `zep_dock.zrd`, `cargozep1` by six (`aiv`, `egen`, `targets`, `cghookup`,
  `zep_dock`, `zepstate`);
* and the measured **absence** of any docking, pickup, boarding or transfer key
  — asserted against the whole 29-key vocabulary rather than by looking for one
  name, so the interaction family of `docs/contracts/SCRIPT-MISSION.md` has no
  M18 instance in this record.

**Combinations, as the sheet requires.** The two latches' prerequisite
closures are computed over the same graph the priorities live in: the failure
latch (block 11) has the seven-block closure `{10, 11, 14, 15, 16, 17, 35}` —
two chains, one per zeppelin — and the success latch (block 25) the
twenty-one-block closure `{2, 3, 4, 5, 6, 7, 12, 13, 18, 19, 20, 21, 22, 23,
24, 25, 33, 34, 44, 45, 46}`, which contains both dependency gates. They are
disjoint, and the remaining 24 blocks (both latches included) are named by no
prerequisite edge of either. Kill edges are deliberately not prerequisites —
block 35's only predecessor is block 10's nap even though blocks 12 and 50 both
kill it. The 65 spelled addresses (26 wake, 18 kill, 19 nap, 2 gates) all lie
in `1..=52`, none is `0`, and block 9 spells `52`, the block count itself — the
value a zero-based reading would report out of range.

## The one measured gap

**`snd_c4-RM-m3_BlackSwan_27`, the sound group M18's two `STOP_QUEUED_SOUNDS`
sites stop, is declared by no shipped record.** Measured two ways:

* decoded member by member over the whole installation (184 `.zbd` containers,
  production discovery, the production `.zrd` decoder): the name occurs in
  exactly one place — `ZBD/C4/M03/zrdr.zbd`, member `objectives.zrd` — the two
  sites that spell it (blocks 12 and 50);
* a raw byte scan of all 228 files of the installation: the bytes occur in
  exactly one file, `ZBD/C4/M03/zrdr.zbd` itself.

Its sibling `snd_c4-RM-m3_BlackSwan_28`, spelled at the same two sites, is
carried by the shared `ZBD/zrdr.zbd` sounds table as well, and all 23 of the
other 24 distinct sound names the record spells are declared there. So the gap
is one name, not a class.

Nothing here says what the original does with a sound name no shipped file
declares (a stop that matches nothing, a name resolved by a path this build has
not measured, or a table entry this build does not decode); that is recorded as
unknown, not guessed, and filed as **M18-B-FU1** below.

**The five message operands are a different shape, and are not a gap.**
`MSG_BRF_RMM3_OBJ1` … `MSG_BRF_RMM3_OBJ5`, the `IDENTITY` operands of blocks 2,
3, 12, 25 and 31, are declared by no `.zrd` record either — but their bytes are
carried by `strings.dll`, the shipped message table, so they are declared by a
resource outside the `.zrd` family the walk decodes. For contrast,
`MSG_OBJ_DEFEND` (block 18's help label) is declared by 24 `.zrd` sites, 23 of
them outside M18's control member. This matches M13-B's reading of the same operand
(`2026-10-06-m01-lc-directive-meaning.md` measures `IDENTITY`'s `MSG_…` operand
as never read by the measured parse).

## The second question this stage answers

M13-B left its startup animations as an unknown (M13-B-FU2). For M18 the
production animation reader **does** bind the scope: `bind_mission_animation`
reads `zbd/c4/m03` and reports ten startup rows (nine `NEW_GAME_START`, one
`LOAD_GAME_START`), seven playable and three refused with their reasons —
`cargozep1_close_doors` (`Undeclared`: no member of the mission archive, its
world group or the shared root declares it), `deactivate_bmhookup_node`
(`NoRecord`: no carrier record carries it as an `anim_name`) and
`cg1zep_engines_stop` (`AmbiguousDeclaration`: two declaring sites, no measured
rule picks one). The refusals are recorded, never worked around.

What that reader does **not** answer is the control record's own animation
names: four of the six the record spells (`dropped_blacke`,
`deactivate_cghookup_node`, `too_late_to_hook`, `call_destroy_the_cargozep`)
are declared by another member of M18's reader archive, but `hooked_to_klondike`
(block 25's `ANIM_STATE`, the win condition) and `pzhomebase` (block 25's
`WAKE_ANIM`) are declared by **no other member of that archive** — they are
declared elsewhere in the installation (the full-coverage walk checks this).
Whether the original joins an `ANIM_STATE`/`WAKE_ANIM` name through this
reader, through the carrier payload or through a path this build has not
measured is **not** asserted here; filed as **M18-B-FU2**.

## What is not claimed

`verified` stays `false` and every checklist entry M18-A left unknown is still
listed in `missions/bindings/M18.json` — the control program being measured does
not answer difficulty branches, failure/success precedence, media, rewards,
optional/stunt ids or `closure_sha256`. No mission was played, no original
executable was run, no presentation, audio or visual row was reviewed: those are
M18-C, which requires `human_play` and the owner. The wrong-actor /
wrong-session / repeated-event halves of the sheet's priorities are runtime
observations. The campaign stays not ready on this stage's evidence.

## Test inventory (`accept_m18_b_*`, 15 tests: 12 retail, 3 synthetic)

Retail, all `#[ignore = "requires CS_GAME_DIR"]`:

1. `…_the_control_program_is_the_member_that_declares_the_blocks` — census,
   independent walk, production control binding and M18-A's mission binding all
   name one archive, one member, one span and two digests re-derived from bytes.
2. `…_every_directive_m18_spells_has_a_disposition_and_none_is_refused` — the
   exact 29-key vocabulary, 27 measured + 2 terminal, sites summing to 238, and
   the five record-level fields.
3. `…_every_call_binds_every_condition_lowers_and_m18s_record_completes` —
   bindings, validation, unmet rows, per-key site counts.
4. `…_release_dependencies_are_two_gates_that_lower_as_wake_conjuncts` — the
   two gate sites, their dependency blocks, and all four lowered conditions.
5. `…_alternative_action_order_is_two_observers_of_one_release_and_free_per_clamp_blocks`
   — the awake-at-start set, the single clock, the two observers' identical
   chains and predicates, and the five free per-clamp blocks.
6. `…_rescue_interaction_is_the_approach_site_and_the_per_clamp_pairs` — the
   `TRAVELERS` spelling and its lowered condition, the target edits, the
   per-clamp pairs, the actors' declarations, and the absence of any interaction
   key.
7. `…_the_terminal_blocks_are_gated_and_every_address_is_in_range` — both
   latches dormant, 65 addresses in range, the discriminating `52`, and who may
   fire each latch.
8. `…_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory`
   — closures, kill-is-not-a-prerequisite, the 22/30 condition split.
9. `…_every_text_the_record_spells_is_declared_outside_the_control_member_but_six`
   — the full 86-text coverage walk and its measured exception set.
10. `…_the_sound_group_no_shipped_record_declares_is_the_one_blocks_12_and_50_stop`
    — the `.zrd` declaration walk *and* the raw byte scan, with the sibling and
    the message operands as controls.
11. `…_the_production_animation_binding_reads_m18s_own_scope` — the production
    animation reader on M18's scope: archives, carriers, ten startup rows, three
    refusal kinds, and where the record's animation names are declared.
12. `…_m18_is_a_complete_census_row_and_the_campaign_stays_unready`.

Synthetic, run in CI without original data:

1. `…_m18s_travelers_site_lowers_and_a_counting_mode_refuses` — M18's own
   predicate: named subject lowers and validates, numeric subject refuses with
   the counting mode.
2. `…_a_dependency_gate_lowers_as_the_dependency_wake_conjunct` — an authored
   two-block record carrying M18's gate shape, both halves visible.
3. `…_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured` — an
   authored `SET_FACTION` (a key M18 never spells) stays unmeasured, refuses its
   site by name as an unknown host call, stands no program and leaves the record
   incomplete.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m18_b_ --include-ignored` | 0 (15 tests: 12 retail, 3 synthetic) |
| `python3 tools/validate_evidence.py private/evidence/M18-B/acceptance.json --artifact-root private/evidence/M18-B --require-pass` | 0 |

## Mutation checks (implementer, 2026-10-10)

Two mutations, each applied, run and reverted before hand-over; both make the
suite fail, so the assertions are not vacuous:

| Mutation | Result |
| --- | --- |
| Drop `snd_c4-RM-m3_BlackSwan_27` from the exception list of `accept_m18_b_every_text_the_record_spells_is_declared_outside_the_control_member_but_six` | that test FAILED, exit 101, naming the sound group as the difference between the measured set and the expected one |
| `crates/cs_script/src/conditions.rs`, the gate's measured `child0 − 1` → `child0` (production lowering) | `accept_m18_b_release_dependencies_are_two_gates_that_lower_as_wake_conjuncts` FAILED at the gated block's conjunct (`ObjectiveAwake{23} ∧ ObjectiveAwake{23}` instead of `…∧ ObjectiveAwake{22}`) and the synthetic `accept_m18_b_a_dependency_gate_lowers_as_the_dependency_wake_conjunct` FAILED with the same index mismatch, both exit 101 |

Both mutations were reverted with `git checkout`; `git status` was clean
afterwards.

**Host note.** The owner's disk-prune loop removed `.rlib` artifacts from
`bunny-alpha-2/target/debug` while this stage's checks were running (the
signature of `2026-10-04-f54-x7-missing-test-harness-binary.md` and
`2026-10-04-t617-cargo-test-enoent-is-external.md`, here on build artifacts
rather than on a test harness: `error[E0463]: can't find crate` for one crate
after another, always naming the same crate on the third occurrence). The
build was repaired with `cargo clean -p <crate>` for the crates cargo named,
and every check below was re-run to green afterwards.

## Recorded unknowns (not guessed)

- **`snd_c4-RM-m3_BlackSwan_27`** — declared by no shipped record (measured
  above); what the original resolves it to is unknown. Resolving task:
  **M18-B-FU1** (static reading in the owner's decrypted image and/or an
  original reference run under M18-C).
- **`hooked_to_klondike` and `pzhomebase`** — declared by no other member of
  M18's own reader archive, though declared elsewhere in the installation;
  which consumer joins the control record's `ANIM_STATE`/`WAKE_ANIM` names is
  unknown. Resolving task: **M18-B-FU2**.
- **The runtime halves of all three priorities** — what happens at each
  boundary before, at and after the transition, whether the wrong actor, the
  wrong session or a repeated event can satisfy a block or a gate, and which of
  the 24 blocks outside both closures is an optional branch — are runtime
  observations for **M18-C** (`human_play`, owner-gated, task #311).
- **`TRAVELERS`' outside-case polarity** — only `APPROACHING` is a measured
  spelling; the word the original uses for the outside case is unknown (the
  shared unknown the directive's own disposition carries).
- The join remains M18-A's inference (`ClaimStatus::Inferred`); no campaign
  definition record or original run was observed.

## Review (bunny-alpha-2/bunny-alpha-2, 2026-10-10)

Reviewer: **bunny-alpha-2/bunny-alpha-2** (Rally #310 review claim of
2026-10-10T14:57Z, OpenCode, `opencode-go/mimo-v2.6-Flash`), a separate review
session that began from the task history, `missions/M18.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md` and the
branch diff — not from the implementation's conversation. Implementer:
**bunny-alpha-2/bunny-alpha-2** (same Rally agent name and model, implement
claim submitted at `ec484ccd`), so this review is **not independent evidence**
in the AGENTS.md sense: it is a fresh-context re-derivation plus a full re-run
of every check. A Rally merge awards `checked` only, no agent review replaces
the owner's human approval, and nothing here is `verified_original` or
`release_approved`.

What the reviewer verified on the branch:

* only owner paths changed — `crates/cs_app/tests/campaign/` (`m18_b.rs`,
  `evidence/m18_b.rs` and the two `mod` lines) and `docs/findings/`: six text
  files, no protected path, no production crate, no
  `missions/bindings/M18.json`, no binary file, no original data;
* rebased onto `fae48695` (M17-B) without a conflict; the incoming commits
  touch `campaign/main.rs` and `evidence.rs`, the same two files this branch
  touches, so the owner's lighter check set never applied and the full four
  checks re-ran on the rebased tree;
* every retail test drives production code — `survey_mission_control_programs`,
  `read_control_member`, `SourceContext::control_program`,
  `SourceContext::bind`, `bind_mission_animation`, `discover` +
  `discover_container` + `decode_zrd`, `measure_control_record`,
  `lower_control_record`, `MissionProgram::validate` — and the three synthetic
  tests carry the refusal arms (the counting-mode `TRAVELERS`, the dependency
  gate's index, an unmeasured `SET_FACTION`) into CI unignored;
* the prefix resolves to exactly fifteen names in one binary and to nothing
  elsewhere in the workspace, each passes when run alone with
  `--exact --include-ignored`, no test is skipped, weakened or `#[ignore]`d
  beyond the repository's `requires CS_GAME_DIR` convention, and no lint was
  relaxed;
* the sheet's three regression priorities are each pinned against measured
  record data, while the claims the sheet says must not be assumed — wrong
  actor, wrong session, repeated event — are honestly left unmeasured for
  M18-C; every recorded unknown names its resolving task (`M18-B-FU1` #1254,
  `M18-B-FU2` #1255, `M18-C` #311), `claim` stays `implemented`, and the
  committed report copy was byte-identical to the implementer's
  `private/evidence/M18-B/acceptance.json`.

**Change made during review:** none to test or production code — the suite as
submitted needed no fix. The review-authored changes are this section and the
regenerated evidence report.

Mutation checks the reviewer ran and reverted (all fail as required, `git
status` clean afterwards):

| Mutation | Result |
| --- | --- |
| `crates/cs_script/src/conditions.rs`, the gate's measured `child0 − 1` → `child0` (production lowering) | both gate tests FAILED, exit 101: `…release_dependencies_are_two_gates_that_lower_as_wake_conjuncts` at `m18_b.rs:858` (`Awake{23} ∧ Awake{23}` instead of `Awake{23} ∧ Awake{22}`) and the synthetic `…a_dependency_gate_lowers_as_the_dependency_wake_conjunct` at `m18_b.rs:2122` (`Awake{0} ∧ Awake{2}` instead of `…∧ Awake{1}`) |
| `crates/cs_content/src/mission_control.rs`, `terminal_outcome_of`'s `INSTANTWIN` → `Failed` | `…every_directive_m18_spells_has_a_disposition_and_none_is_refused` FAILED at `m18_b.rs:619` (`{"INSTANTLOSS": Failed, "INSTANTWIN": Failed}` against the pinned pair), exit 101 |

Each mutation is in production code the suite does not own, so the failures show
the tests read the lowering and the measurement rather than their own constants.

Checks run by the reviewer on the rebased tree, all green:

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (495 `test result: ok`, no failure) |
| `cargo test --workspace --locked -- accept_m18_b_ --include-ignored` | 0 (15 discovered, 15 executed, 15 passed, 0 failed — the log of record, `private/evidence/M18-B/cargo-test.log`) |
| 15 × `cargo test --locked -p cs_app --test campaign m18_b::<name> -- --exact --include-ignored` | 0 (15/15, `private/evidence/M18-B/exact-runs.log`) |
| `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py'` | 0 (27 tests) |
| `python3 tools/validate_evidence.py private/evidence/M18-B/acceptance.json --artifact-root private/evidence/M18-B --require-pass` | 0 on the reviewer's regenerated report |

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`;
`missions/M18.md`; `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-10-m13-b-compatibility-gaps.md`,
`docs/findings/2026-10-09-m10-b-control-program-gaps.md`,
`docs/findings/2026-10-09-m06-b-compatibility-gaps.md`,
`docs/findings/2026-10-10-m06-b-fu3-objective-address-convention.md`,
`docs/findings/2026-10-06-m01-lc-directive-meaning.md`.
