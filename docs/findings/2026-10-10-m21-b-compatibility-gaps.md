# M21-B: Death on the Docks' mission-specific compatibility surface

Date: 2026-10-10. Task: M21-B "Implement and regress mission-specific
compatibility gaps" (#319, `missions/M21.md`, work order `M21-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic` (newly authored `.zrd`
records). Implementer: **bunny-2/bunny-2** (Rally #319, session of
2026-10-10). Nothing here is `verified_original`: no original executable was run
and no mission was played (AGENTS.md rule 8).

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M21's discovered mission-specific behavior is its
mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C5/M01/zrdr.zbd` — and this stage runs it through
production systems (`cs_app::mission_control::survey_mission_control_programs`,
`read_control_member`, `SourceContext::control_program`, `SourceContext::bind`,
`cs_content::mission_control::measure_control_record`,
`cs_app::control_lowering::lower_control_record`,
`cs_app::world::triggers::survey_retail_trigger_volumes`,
`cs_app::mission_start::recover_retail_start_configuration`,
`cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`) and regresses it with twelve
`accept_m21_b_*` tests. Three measured gaps are left **recorded, not worked
around**: see *The measured gaps* below.

## What changed

No production code and no change to `missions/bindings/M21.json`: M21's record
lowers completely, so there was no lowering gap to close and no binding
statement to correct. Added:

* `crates/cs_app/tests/campaign/m21_b.rs` — twelve `accept_m21_b_*` tests
  (ten retail, two synthetic);
* `crates/cs_app/tests/campaign/evidence/m21_b.rs` — the M21-B evidence
  harness, plus its one `mod` line in `evidence.rs`;
* `docs/findings/2026-10-10-m21-b-compatibility-gaps.md` (this note) and
  `docs/findings/evidence/M21-B.json` (the committed report copy).

Wiring only: `crates/cs_app/tests/campaign/main.rs` (`mod m21_b;`).

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as M01-A … M21-A) |
| Reader archive | `ZBD/C5/M01/zrdr.zbd`, 39 369 bytes, SHA-256 `263c9914…fa4874` (M21-A's own program span) |
| Members | 12; exactly one declares numbered blocks — `objectives.zrd`, the 8th member, offset 14 111, 15 956 bytes, SHA-256 `15d979d8…2db61c`. No member out-ranks it: the longest *other* member is `aiv.zrd` (10 498 bytes) and it declares none |
| Detection-zone member | `dzones.zrd`, offset 10 498, 826 bytes, SHA-256 `5518dda7…09da0` — the span the census **and** the trigger survey both report |
| Control record | 63 numbered blocks, 237 directive sites, 28 distinct keys, no block refusal, no record key outside the measured vocabulary |
| Dispositions | 26 measured, 2 terminal (`INSTANTWIN` block 19 → succeeded, `INSTANTLOSS` block 57 → failed), **0 unmeasured, 0 refused** |
| Measured operations | 21 distinct `DirectiveOperation` variants over the 26 measured keys (`INACTIVE1…6` share one) |
| Lowering | mission `mission/ch5-m01`, 63 `RawObjective`s, all 63 conditions lowered, all 237 calls bound, `MissionProgram::validate` clean, 0 unmet requirements, census row **complete** |
| Campaign | `zbd/c5/m01` is one of the census's complete rows; `campaign_ready()` is still false (other rows carry their own gaps) |
| Record-level fields | `MISSION_TIMER [0.0]`; `PLAYER_INIT [1, [-12166, 400, -13112], [0, -90, 0], 0.8, 180]`; the three animation lists are empty; no record-level sound key |
| Aircraft table | 14 records — `player`, `autogyro_1`, `bhatwarhawk_5_1..3`, `bhatbrigand_5_1..3`, `patrolboat_1/2`, `t_truck_1..4`; no `wingman_<n>` record, no `ia.zrd` member |
| Start configuration | `recover_retail_start_configuration("zbd/c5/m01")` → stored pose `[-12166, 400, -13112]`, heading `-90`; airframe `airframe/player_pfighter` (campaign chain row 5, measured engine state) |
| Zone survey | 34 numbered zones over the chapter-5 container (`dzpath1…34`); M21's declaration splits them into 20 `nosnapshot` names and 14 `objective_numbers` pairs, the two sets disjoint and covering all 34, **no declaration gap** |
| Title | `Death on the Docks`, localized row 3500 |
| Whole installation | one `.zrd` text index over every `.zbd` container, plus a one-pass raw-byte scan of every file |

## The three regression priorities, as the record spells them

**Moving guide.** The record spells six `TRAVELERS` sites, five with `player`
as the subject (`dz1` at 1 000, `rfspt4` at 1 500/1 500/1 000, `piratezep` at
1 500) and exactly one with a second actor — block 20, awake from the mission's
first tick, `autogyro_1 APPROACHING [-344, 250, -4347.5] 1000 1
DELETE_ON_SUCCESS`. `autogyro_1` is also the only actor **the control program
itself sets in motion**: always-awake block 2 wakes block 28 on the player's
approach to `dz1`, and block 28 spells `WAKEUP_ENEMIES autogyro_1` and
`START_TAXI autogyro_1` before block 58 retires the actor's objective-target
flag. The one other non-static geometry is block 51's **anchor**, `piratezep`,
whose own `zeppelins.zrd` record declares `max_speed` 25 — so a reader who reads
"moving guide" as *the thing the player follows* instead of *the thing the
program moves* lands on a second, also-measured binding. Which of the two the
firsthand guide's label names is not decidable from record data; both are pinned
and the label stays a research label. `autogyro_1` is declared by this
archive's `aiv.zrd` and by no other member anywhere.

**Structural destruction.** Three shapes, all record data:

* the six warehouse support beams `rfspt4…6` / `lfspt1…3`, declared by **this
  archive's own `targets.zrd`** as `MSG_TRGT_WH_SUPPORTBEAM` with
  `help_label MSG_OBJ_DESTROY`, watched by `INACTIVE1…6` at four different
  `INACTIVE_COMPLETION_COUNT` thresholds — 1 (block 62), 3 (11), 5 (12), 6 (13)
  — plus block 63's 3 over the zeppelin's panels;
* the `steinmann` freighter (`MSG_TRGT_STEINMANN`, category
  `MSG_OBJ_FREIGHTER`), whose `steinmann_sink` animation is always-awake block
  17's entire predicate, with `REMOVE_OBJECTIVE_TARGET steinmann` and
  `snd_MN1Sink` beside it — `steinmann` and `steinmann_sink` are both declared
  by `ZBD/C5/zrdr.zbd#steinmann.zrd`, the chapter world's own script;
* block 63's chained `piratezep gasbagN panels` lookups, threshold 3, whose
  sixth name is **`gasbag6` — a name this archive's `zeppelins.zrd` does not
  spell**. That record's `healthy` list carries `gasbag1…5` with `gasbag5`
  **twice** and no sixth entry (`num_healthy_required` 4), while `gasbag1…5`
  are declared by this archive and `gasbag6` by twenty-two others but not by
  M21.

A second, undeclared quartet sits at block 10: `w_win01…04` at threshold 1. The
`.zrd` index finds them nowhere else; the raw scan below finds them in the
chapter-5 world containers.

**Optional route reward.** Always-awake block 37 spells
`DANGER_ZONES_COMPLETION_COUNT 4` over `dzpath22…27`, wakes the `SECONDARY`
objective 5 (`IDENTITY SECONDARY 2 MSG_BRF_NYM1_OBJ2`, which wakes the
`music_secondaryobj_sg` marker 49 and blocks 58/60) and kills its mirror 6 —
the two mirror blocks carry the *identical* `TRAVELERS player APPROACHING
rfspt4 1500 1` site, so only the wake decides which one runs. Beside it, six
zone gates pair a "yes" block with a "no" block that kill each other: 3/4 over
`dzpath22`, then 38/39 (`dzpath23`), 40/41 (`dzpath24`), 42/43 (`dzpath25`),
44/45 (`dzpath26`), 46/47 (`dzpath27`), each yes block napping the next gate
(41, 43, 45, 47 — and block 4's pair napping 39) for its own measured delay.
Every zone name resolves through the **production** trigger-volume survey: all
six are declared by M21's own `dzones.zrd` — under the `nosnapshot` key only,
never under an `objective_numbers` pair — and all six have a node in
`ZBD/C5`, which carries exactly the 34 numbered zones the declaration accounts
for, with an empty gap list.

## Full-coverage claim behind the gaps

The control document spells **186** distinct text nodes (the whole document,
keys and operands alike). Every one of them is declared by some `.zrd` member
outside M21's control member **except twelve**, and the suite pins the exception
set exactly — then reads all twelve again as raw bytes over the whole
installation in a single pass, which is what the `.zrd` index cannot say:

| Texts | Raw-byte carriers |
| --- | --- |
| `fbgun01`, `fbgun02`, `maagun0*` | `ZBD/C5/M01/zrdr.zbd` **only** — the `WAKEUP_TURRETS` operands of block 1, and `maagun0*` is the wildcard spelling no other file carries |
| `MSG_BRF_NYM1_OBJ1…5` | M21's archive plus the installation's `strings.dll` — the briefing ids are carried by the string table |
| `w_win01…04` | M21's archive plus `ZBD/C5/cam_anim.zbd` and `ZBD/C5/gamez.zbd` — the chapter-5 world containers |

The counter-example the suite keeps beside that claim: `piratezep` is spelled by
111 `(container, member)` pairs across the installation, so the scan
distinguishes a declared name from an undeclared one rather than always
answering "absent".

Two softer classes, recorded so they are not mistaken for record-only ones:
`pzhomebase` and `hooked_to_klondike` are declared by other missions' control
members (and by `pzep_hookup.zrd` / the chapters' `landings.zrd`) but by **no**
member of M21's archive, while `dz1` is declared by this archive's own
`targets.zrd` *and* by five instant-action scenarios' target tables — a
cross-archive name, not a gap.

## The terminal structure

There are exactly two terminal outcomes in the record: `INSTANTWIN` at block 19
and `INSTANTLOSS` at block 57, both bare, both dormant and both entered by
edges. The success latch carries `IDENTITY PRIMARY 5 MSG_BRF_NYM1_OBJ5`,
`WAKE_ANIM pzhomebase` and the `hooked_to_klondike → EXECUTED` animation; the
failure latch carries nothing but its own key.

All **117** spelled addresses lie in `1..=63` under M02-B-FU3's measured
one-based convention: 39 wake addresses over 23 sites, 60 kill addresses over
19 sites, 15 nap addresses over 15 sites (child1 is the delay, never an
address) and 3 dependency gates (blocks 16→27, 60→59, 61→59).

A kill is not an edge. The prerequisite closure of block 19 is the ten blocks
`7, 10, 11, 12, 13, 16, 17, 18, 19, 27`; of block 57 it is `1, 2, 28, 56, 57,
63`. Forty-three blocks start dormant and twenty start awake
(`2, 3, 6, 7, 8, 9, 17, 20, 29, 31, 33, 35, 37, 38, 40, 42, 44, 46, 62, 63`);
exactly one dormant block arms a clock (block 1, at 2 s).

## What is not claimed

`verified` stays `false` and every checklist entry M21-A left unknown is still
listed in `missions/bindings/M21.json` — the control program being measured does
not answer difficulty branches, failure/success precedence, media, rewards or
`closure_sha256`. No mission was played, no original executable was run, no
presentation, audio or visual row was reviewed: those are M21-C, which requires
`human_play` and the owner. The wrong-actor / wrong-session / repeated-event
halves of the sheet's three priorities are runtime observations, and the
sheet's own label "Moving guide" remains a research label rather than a
verified binding. The campaign stays not ready on this stage's evidence.

## Test inventory (`accept_m21_b_*`, 12 tests: 10 retail, 2 synthetic)

Retail, all `#[ignore = "requires CS_GAME_DIR"]`:

1. `…_the_control_program_is_the_member_that_declares_the_blocks` — census,
   independent walk, production control binding and M21-A's mission binding all
   name one archive, one member, three spans and four digests re-derived from
   bytes (whole archive, control member, detection-zone member).
2. `…_every_directive_m21_spells_has_a_disposition_and_none_is_refused` — the
   exact 28-key vocabulary with its site counts summing to 237, 26 measured +
   2 terminal, nothing unmeasured or refused, the five record-level fields and
   their shapes, the empty record-sound list, and the pinned 21-variant
   operation set.
3. `…_the_moving_guide_is_the_actor_the_program_starts_and_then_measures` —
   the six `TRAVELERS` sites with their subjects and anchors, block 20's exact
   site, the one `DELETE_ON_SUCCESS`, the wake/taxi/retire chain, the actor's
   single declaration, the 14 aircraft records, and block 51's moving anchor.
4. `…_the_structural_destruction_is_the_support_beams_the_freighter_and_the_zeppelin_panels`
   — the six thresholds, the beams' `INACTIVE<n>` lookups, block 17's animation
   predicate, block 63's chained lookups, the zeppelin record's `healthy` list
   (with `gasbag5` twice and no `gasbag6`), the target table's own descriptions
   and help labels, the freighter category, the bare `objective` flag on `dz1`,
   and the two `.zrd`-index emptiness checks.
5. `…_the_optional_route_is_the_danger_zone_threshold_and_its_paired_gates` —
   block 37's threshold and zone list, the SECONDARY objective and its music
   marker, the mirror branch, the five paired gates' mutual kills and naps, and
   the production survey's declaration, key order, disjoint sets, node set,
   spelled-zone membership and empty gap list.
6. `…_every_text_the_record_spells_is_declared_outside_it_or_recorded_as_a_gap`
   — the 186-text count, the exact twelve-text exception set, the one-pass raw
   split into three classes, the counter-example, the two softer classes and the
   cross-archive `dz1`.
7. `…_the_two_terminal_latches_are_gated_and_every_address_is_in_range` — the
   dormant/awake sets, the one timed self-wake, both latch sites, the 117
   addresses counted per key, the three gates, both prerequisite closures, the
   kill-is-not-an-entry proofs, the record's one unreachable dormant block (15)
   and the two empty always-awake blocks (8, 9).
8. `…_the_player_start_is_the_aircraft_tables_own_record` — the production start
   configuration (player record, pose, airframe), `PLAYER_INIT` and
   `MISSION_TIMER` re-derived from the record, the absent `ia.zrd`, and the six
   `player` spellings (five proximity subjects and one inactive-member lookup).
9. `…_every_call_binds_every_condition_lowers_and_m21s_record_completes` —
   mission id, 63 objectives, 63 lowered conditions, 237 bound calls, no
   unbound key, clean validation, four met requirements, complete row, and no
   unmet-requirement row for M21 anywhere in the census.
10. `…_m21s_row_is_complete_and_the_campaign_stays_unready` — the complete row,
    an incomplete sibling and the shut campaign gate.

Synthetic, run in CI without original data:

1. `…_a_guide_approach_site_binds_at_its_measured_shape_and_refuses_a_radius_the_ir_cannot_carry`
   — M21's own block-20 shape bound into a validating program; the same site
   with a non-finite radius refused by name, its block's condition damaged to a
   refusal and `MissionProgram::validate` carrying an error.
2. `…_the_zone_threshold_pairs_with_its_evaluator_and_an_unknown_key_refuses`
   — the retail threshold-4 / six-zone pair lowers and binds; the threshold
   alone still stands; an unknown key refuses exactly its own site by name,
   leaves the three measured sites bound and produces no program.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (495 `test result: ok`, no failure) |
| `cargo test --workspace --locked -- accept_m21_b_ --include-ignored` | 0 (12 tests) |
| `python3 tools/validate_evidence.py private/evidence/M21-B/acceptance.json --artifact-root private/evidence/M21-B --require-pass` | 0 |

## Mutation checks run and reverted (both fail as required)

| Mutation | Result |
| --- | --- |
| `crates/cs_app/src/mission_start.rs` `CAMPAIGN_AIRFRAME_ROW` `5` → `6` (production) | `accept_m21_b_the_player_start_is_the_aircraft_tables_own_record` FAILED at `m21_b.rs:2151` (the production airframe no longer matched the pinned campaign chain row), exit 101 |
| `crates/cs_app/src/control_lowering.rs` `zrd_to_value`'s `if f.is_finite()` → `if true` | `accept_m21_b_a_guide_approach_site_binds_at_its_measured_shape_and_refuses_a_radius_the_ir_cannot_carry` FAILED at `m21_b.rs:2507` (the refusal then named `NaN outside -inf..=inf` instead of `not finite`), exit 101 |

Both were reverted and the twelve-test selection was rerun green afterwards.

## Recorded unknowns (not guessed)

- **`gasbag6`** — block 63's sixth chained lookup names a part this archive's
  `zeppelins.zrd` never declares (its `healthy` list spells `gasbag1…5` with
  `gasbag5` twice). What the original resolves that lookup to is unknown.
  Resolving task: **M21-C** (an original reference run) or a static reading in
  the owner's decrypted image.
- **`fbgun01`, `fbgun02`, `maagun0*`** — the `WAKEUP_TURRETS` operands of block
  1 that no other file in the installation carries at all (raw-byte scan, one
  pass over every file). What the original resolves them to is unknown;
  `thug*` beside them *is* declared (`ZBD/C5/zrdr.zbd#thug.zrd`,
  `ZBD/zrdr.zbd#ai.zrd`). Resolving task: **M21-C** / a static reading.
- **`MSG_BRF_NYM1_OBJ1…5`** — carried only by M21's archive and the
  installation's `strings.dll`; the id → text mapping behind them is not decoded
  by this stage. Resolving task: **M21-C** (the briefing text as presented).
- **`w_win01…04`** — no `.zrd` member anywhere spells them; the raw scan finds
  them only in `ZBD/C5/cam_anim.zbd` and `ZBD/C5/gamez.zbd`, which is presence in
  the chapter's world data, not proof of what kind of node they are. Resolving
  task: **M21-C** / a static reading of the chapter-5 world containers.
- **Which actor the "moving guide" label names** — the record offers two
  measured bindings (`autogyro_1`, whose motion the program starts and then
  measures, and `piratezep`, the moving anchor block 51 measures the player
  against). The sheet's label comes from the firsthand guide, so it is not
  resolved to one of them here. Resolving task: **M21-C**.
- **Block 15** — dormant, `-1`, and addressed by nothing: the record's one
  block nothing can enter. Whether the original ever reaches it is unknown.
- **The runtime halves of all three priorities** — what happens at the
  proximity boundary before, at and after its transition, whether the wrong
  actor, the wrong session or a repeated event can satisfy a block, and whether
  the zone gates fire as measured, are runtime observations for **M21-C**.
- **The `objective_numbers` pairs' meaning** — which objective the integers
  18…31 index is unmeasured in `cs_content::world` and stays unmeasured here;
  M21's six spelled zones carry no pair at all.
- The join remains M21-A's inference (`ClaimStatus::Inferred`); no campaign
  definition record or original run was observed.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`;
`$CS_ENGINE_IMAGE` read through `cs_app::mission_start` for the campaign
airframe row; `missions/M21.md`; `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-10-m17-b-compatibility-gaps.md` (the stage this one
follows),
`docs/findings/2026-10-10-m13-b-compatibility-gaps.md`,
`docs/findings/2026-10-09-m07-b-compatibility-gaps.md`
(`TRAVELERS`, `INACTIVE<n>`), `docs/findings/2026-10-10-m06-b-fu3-objective-address-convention.md`
(the one-based address rule).
