# M01-LC-SCENARIOS: where the 13 readers without objective blocks keep their control data

Date: 2026-10-13. Task: `M01-LC-SCENARIOS` (#642), "locate the control program
of the 13 readers that declare no objective blocks". Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`), `synthetic`. Evidence report:
`private/evidence/M01-LC-SCENARIOS/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-SCENARIOS.json`. Test prefix:
`accept_m01_lc_scenarios_`.

## The question this stage answers

`M01-LC-MISSION-PROGRAM` (#630) measured 40 of the installation's 53
mission-scoped reader archives as carrying a control member — the member whose
decoded record declares numbered `OBJECTIVE<N>` blocks — and found **13** that
declare none. Each of the 13 still declares an `objectives.zrd` with no blocks
plus a scenario record beside it, so their control data exists; where it lives
was deliberately not guessed. This stage locates it **by contents, never by
name**, with one falsifiable rule per branch:

> **An instant-action reader's control member is the unique member whose
> decoded record declares the measured scenario descriptor surface** — the
> member for which F42-A's `scenario_mission_type` answers, a flat record
> carrying a `mission_type` field whose value is the one-element text list the
> original writes (`["dogfight_squadron"]`). Zero candidates is a **measured
> absence**; two are a **refusal**, never a choice.

Measured over the owner's installation (`b4e780ab…`), the rule splits the 13
exactly along the population line #630 recorded:

| population | readers | control surface |
| --- | ---: | --- |
| instant action (`ZBD/C{1,1b,1c,2,2b,3,4,5}/IA1`) | 8 | **one descriptor member each** (spelled `ia.zrd`) |
| multiplayer (`ZBD/C1/MP3`, `ZBD/C2B/MP3`, `ZBD/C5/MP{1,2,3}`) | 5 | **no descriptor member anywhere** — a different, thinner surface |

The multiplayer answer is explicitly **a different surface**, not a thinner
dialect of the same one: no member of any multiplayer reader declares a
scenario mode, a player aircraft, a wingman count, an enemy group, an ace or a
zone binding anywhere in its member list. What such a reader carries instead is
measured below, half by half.

The rule is contents, not name, in both directions — and the census proves the
distinction is load-bearing rather than decorative:

* **Corpus-wide uniqueness.** The same walk decoded every member of all 53
  readers. **No member of any of the 40 block-bearing (campaign-mission)
  readers declares a scenario descriptor** (`descriptor_sites_outside_scenarios`
  is empty), so the rule cannot misidentify a mission's member even where it is
  not applied. A synthetic archive proves the same on authored bytes: a real
  descriptor under the name `setup.bin` is selected beside an impostor that is
  *named* `ia.zrd` but declares only an animation definition
  (`accept_m01_lc_scenarios_the_descriptor_is_selected_by_its_contents_not_its_name`).
* **Zero and two are still different answers.** The five multiplayer readers
  are a measured absence carried as rows, and two descriptor members in one
  archive is the same refusal #630's block rule carries for two block-bearing
  members, naming both candidates
  (`accept_m01_lc_scenarios_two_descriptors_are_a_refusal_not_a_choice`).

## What an instant-action descriptor spells

Every descriptor is the flat alternating record F42-A measured (`ia.zrd`'s
shape), and the census publishes **every** key with its measured argument
shape, not only the keys a reader consumes. Over the eight descriptors:

| field | support | measured shape |
| --- | --- | --- |
| `mission_type` | 8/8 | `[text]` — `dogfight_squadron` (c1, c3), `stunt_flying` (c1b, c2, c4, c5), `zeppelin_run` (c1c, c2b) |
| `spawn_points` | 8/8 | flat alternating `mode, [[x,y,z,h]…]` — 4 modes × 8/8/8/4 entries (c1, c1b, c2, c3, c4, c5) or 3 modes × 8/8/6 (c1c, c2b) |
| `player_plane`, `num_wingmen` | 7/8 | `[text]`, `[int]` — `ZBD/C2B/IA1` declares **neither** |
| `dzones` | 6/8 | `[[world node, label]…]` — c1: 5, c1b: 5, c2: 9, c3: 4, c4: 14, c5: 17; `ZBD/C1C/IA1` and `ZBD/C2B/IA1` declare none |
| `group1`…`group4` | 8/8 | flat record of `num_enemies [int]`, `enemy_name [text]`, `enemy_plane [text]`, `enemy_skill [text]` |
| `ace_name/plane/skill`, `ace_stats` (9 ints), `ace_accentID/pattern/decal1-3`, `ace_color1-3` (3 ints) | 8/8 | the named ace and its livery |
| `disallow_missions` | 8/8 | `[text]…` — 1 entry in six readers, 2 in c1c and c2b |
| `zeppelin_type`, `cargo_zeppelin`, `passenger_zeppelin`, `military_zeppelin` | 8/8 | `[text]` — the zeppelin nodes the scenario binds |

Four of the eight declare F42-A's `stunt_flying` mode — the same count F42
measured through its name-based reader, now re-derived through the contents
rule. The readers that declare no `dzones` and no player aircraft declare the
mode anyway, which is why the rule requires **only** the mode: a rule that
demanded the whole surface would drop two original instant-action scenarios.

Beside the descriptor, an instant-action archive carries the same
record-level `objectives.zrd` a campaign mission has minus its blocks — the
five measured record keys (`MISSION_TIMER`, `PLAYER_INIT`, `RESTORE_ANIMS`,
`EXECUTE_ANIMS`, `INVALIDATE_ANIMS`) in all eight — and a `targets.zrd` whose
records are F42's fly-through labels: **zero** instant-action target record
anywhere carries the `objective` marker key.

## What a multiplayer reader spells, and why it is a different surface

The five multiplayer readers declare **no** descriptor member. Their control
data is spread over three thinner surfaces, each measured by its own shape
reader, and none of them is a directive stream:

| surface | measured over the 5 |
| --- | --- |
| `objectives.zrd` record keys | `MISSION_TIMER` only (c1/mp3, c2b/mp3 — 53 bytes); the five measured keys (c5/mp3); the five **plus** `READ_TIME`, `REVIEW_DELAY`, `OBJECTIVE_DELAY`, `PLAY_LOSS_ANIM_WHEN_MAVERICK_ON_RUNWAY` (c5/mp1, c5/mp2 — 428 bytes) |
| `net.zrd` | the bare spawn-point list: 48 entries (c1/mp3, c2b/mp3, c5/mp2, c5/mp3), 80 (c5/mp1) |
| `targets.zrd` markers | c1/mp3, c2b/mp3, c5/mp3: 4 records, 2 `objective`-marked and 2 `other_target`-marked; c5/mp2: 8 records all `objective`-marked; c5/mp1: 1 record, `other_target`-marked only |
| `zeppelins.zrd` records | 2 (the mp3 readers), 0 (c5/mp1), member absent (c5/mp2); instant-action readers place 1 each |

So a multiplayer scenario declares its numbered objectives as **target records
marked `objective`** — F42-A's `targets.zrd` grammar, F56-B's `SlotObjective`
surface — and never as `OBJECTIVE<N>` blocks. That is the measured reason
#630's block rule finds nothing in them: the objectives are spelled on the
target surface, and the record-level member carries only the timer (plus, in
two readers, four spellings the campaign record vocabulary does not measure).

The multiplayer population is **not uniform**, and the split is measured
rather than assumed: **16 of the 21 multiplayer readers declare numbered
objective blocks** and are campaign-mission rows in #630's census. Only the
five named here declare none. "Multiplayer reader" is therefore not a
synonym for "scenario reader" in this corpus.

## `net.zrd` is one surface wherever it appears — campaign readers included

The census measured `net.zrd` in **45** of the 53 mission-scoped readers: all
21 multiplayer readers, 24 campaign-mission readers, and none of the eight
instant-action readers. Every one parses as the same **bare spawn-point
shape** — the root is one list of four-float entries, with **no keys at all** —
and **none** is reported `UnmeasuredShape`. The measured entry counts are
exactly four: `0` (the c4 and c5 campaign missions), `8` (the c1–c3 campaign
missions that declare it), `48` (the mp2/mp3 slots) and `80` (the mp1 slots).

This upgrades F56-B's "spawn-placement candidate" note to a measured shape
statement: `net.zrd` declares spawn points wherever it occurs, and a campaign
mission declares them too (8 or 0). The **fourth** float of an entry is
unmeasured — position (x, y, z) is the obvious reading and stays an inference;
no original executable has been run.

## The denominator is not widened

An instant-action or multiplayer scenario is not a campaign mission, and this
stage keeps the two populations apart at every step:

* The scenario population is derived by the **same** block rule
  (`cs_content::mission_control::control_member`) on the same bytes, and the
  retail test reconciles this census's rows against #630's
  `archives_without_control_program()` on every run — the two walks must name
  the same 13 archives.
* Every member of every scenario row measures **0** numbered blocks, and a
  target record marked `objective` is not a block: a marker never turns a
  scenario reader into a mission reader, and one real block in an archive that
  also declares a descriptor makes it a mission row, not a scenario row
  (`accept_m01_lc_scenarios_an_objective_marker_is_not_an_objective_block`).
* Nothing in this stage lowers a scenario to a `MissionProgram`, and
  `campaign_ready()` is untouched and `false`.

## Files and the one observable failure

- `crates/cs_content/src/scenario_control.rs` (new, owner path):
  `ScenarioMemberError`, `scenario_descriptor_candidates`,
  `scenario_control_member`, `MeasuredScenarioDescriptor`,
  `measure_scenario_descriptor`, `scenario_spawn_modes`,
  `ScenarioObjectiveRecord`, `measure_scenario_objective_record`,
  `NetSpawnSurface`, `measure_net_spawns`, `TargetMarkerCounts`,
  `count_target_markers`, `SCENARIO_DESCRIPTOR_KEY_VOCABULARY`.
- `crates/cs_app/src/scenario_control.rs` (new, owner path):
  `ScenarioCensusError`, `ScenarioMemberRow`, `ScenarioProgram`,
  `RetailScenarioRow`, `RetailScenarioCensus`,
  `survey_scenario_control_programs`, `survey_scenario_control`,
  `measure_scenario_reader`.
- `crates/cs_app/tests/accept_m01_lc_scenarios.rs` (new, owner path): the 7
  `accept_m01_lc_scenarios_` tests (6 synthetic + 1 retail).
- `crates/cs_app/tests/evidence_report_m01_lc_scenarios.rs` (new, owner
  path): the evidence harness, deliberately **not** prefixed
  `accept_m01_lc_scenarios_`.
- Wiring only (AGENTS rule 1): `crates/cs_content/src/lib.rs` and
  `crates/cs_app/src/lib.rs` (one module declaration and one doc paragraph
  each).

**One observable failure:** a scenario whose control member is selected by
member name reports a program it never checked — on this corpus the name
reading happens to agree for the eight instant-action readers, which is
exactly why the witness is authored rather than pointed at retail: the
`setup.bin`/`ia.zrd` impostor pair in
`accept_m01_lc_scenarios_the_descriptor_is_selected_by_its_contents_not_its_name`
fails the moment the rule consults a name, and the two-candidate refusal test
fails the moment an ambiguity is resolved silently. The mirrored failure — a
walk that reads spawn points out of a `net.zrd` whose shape it did not check —
is `accept_m01_lc_scenarios_an_unmeasured_net_shape_is_never_read_as_spawn_points`.

## Design decisions

- **One field is the rule, and it is F42-A's.** The descriptor is recognized
  through `scenario_mission_type`, the reader F42-A already measured, rather
  than through a second reading of the same bytes. Requiring the whole
  descriptor surface would drop `ZBD/C1C/IA1` and `ZBD/C2B/IA1`, which declare
  no zones, and `ZBD/C2B/IA1`, which declares no player aircraft.
- **The name is evidence, never the rule.** All eight selections happen to be
  the member the corpus spells `ia.zrd`; the retail test records that as a
  measured fact about this installation, and the synthetic test proves the
  rule survives a rename in both directions.
- **A measured absence is not an error.** The five multiplayer readers are
  rows carrying `NoDescriptor { scanned }` — reported, not dropped, and not
  resolved to "the member named `net.zrd`", which declares no keys at all and
  could never be a directive stream.
- **`net.zrd` is read only as its measured shape.** A member in any other
  shape is `UnmeasuredShape` and its count is not guessed; on this
  installation none is, and the retail test asserts that in both directions.
- **Unclassified stays unclassified.** The four multiplayer-only record
  spellings (`READ_TIME`, `REVIEW_DELAY`, `OBJECTIVE_DELAY`,
  `PLAY_LOSS_ANIM_WHEN_MAVERICK_ON_RUNWAY`) are counted and reported by name
  rather than absorbed into #630's record vocabulary.
- **The zeppelin fields are not re-measured.** `zeppelins.zrd` records are
  counted; their field vocabulary remains F35's and T574's measurement
  (`docs/findings/2026-10-08-t574-zeppelins-zrd-placed-zeppelin-carrier.md`).
- **Fail-closed everywhere.** A member that does not decode fails the whole
  census; two descriptors refuse; an unreadable shape is never counted; an
  empty census publishes nothing and the acceptance test requires rows.

## Test inventory (`accept_m01_lc_scenarios_`, 7 tests: 6 synthetic + 1 retail)

| Test | Covers |
| --- | --- |
| `the_descriptor_is_selected_by_its_contents_not_its_name` | **the observable failure**: a descriptor under an unseen name beside an `ia.zrd` impostor; the mode, the flat spawn list and the candidates measured |
| `two_descriptors_are_a_refusal_not_a_choice` | the ambiguity refusal names both candidates in member-table order |
| `a_reader_without_a_descriptor_reports_a_measured_absence` | the multiplayer shape: `NoDescriptor`, the record keys with `READ_TIME` kept unclassified, the bare spawn list, the target markers, the zeppelin count |
| `an_objective_marker_is_not_an_objective_block` | both directions of the denominator guard: markers never make a mission, one real block never makes a scenario |
| `an_unmeasured_net_shape_is_never_read_as_spawn_points` | text roots, three-float entries and a double wrapper are refused; an empty list is `Measured(0)` |
| `the_census_walk_keeps_the_populations_apart` | the walk on an authored installation: the block-bearing archive counts as a mission and its descriptor member is reported as a cross-check site, never selected |
| `every_scenario_reader_is_located_by_its_contents` *(retail)* | the whole table above over `$CS_GAME_DIR`, reconciled against #630's census; the 8/5 split, the eight modes, the descriptor field support, the record vocabulary with counts, the four net sizes, the 16 block-bearing MP readers and the target-marker split |

Every test calls production code (the same `measure_scenario_reader` and
`survey_scenario_control_programs` the census runs). The retail test
re-derives every figure from `$CS_GAME_DIR` on each run, so a stale constant
fails rather than passes. All synthetic `.zrd` and reader-archive bytes are
authored here tag by tag; no original game data is committed.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (4649 passed, 0 failed) |
| `cargo test --workspace --locked -- accept_m01_lc_scenarios_ --include-ignored` | 0 (7 tests: 6 synthetic, 1 retail) |
| `python3 tools/validate_evidence.py private/evidence/M01-LC-SCENARIOS/acceptance.json --artifact-root private/evidence/M01-LC-SCENARIOS --require-pass` | 0 |

## Recorded unknowns (not guessed)

- **What any scenario declaration *does* is unmeasured.** No original
  executable has been run. A mode name, a spawn coordinate, an `objective`
  marker and an enemy-group record are declarations; which mode the game
  offers where, how a spawn point is chosen and what completes a multiplayer
  objective are not established here.
- **The fourth float of a spawn entry is unmeasured.** Every entry is
  `[float, float, float, float]`; position is the natural reading of the
  first three and the fourth (heading? speed?) is not established, so no
  consumer may treat the entry as a pose.
- **`disallow_missions`'s values are unmeasured.** Its entries are text
  (`ground_target` in c1); whether they name modes, mission types or anything
  else is not established, so the field is inventoried with its shape and
  nothing reads it.
- **`PLAY_LOSS_ANIM_WHEN_MAVERICK_ON_RUNWAY`, `READ_TIME`, `REVIEW_DELAY`,
  `OBJECTIVE_DELAY` are unclassified record keys.** Measured spellings of two
  readers (c5/mp1, c5/mp2) with measured shapes (`[int]`); their effects are
  not measured and they are not absorbed into the record vocabulary.
- **The wider denominator is not covered.** This census walks the 53
  mission-scoped readers only; the shared reader (`ZBD/zrdr.zbd`) and the
  world-group readers are outside it (F39-D unknown #5; task #597 owns the
  wider census). A scenario surface in those readers is unmeasured here.
- **Nothing here is `verified_original`.** The evidence claim is
  `implemented`; a merged branch is `checked` at most.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::{discover_container, mission_scope}`,
`cs_content::stunts::decode_zrd`; `docs/contracts/SCRIPT-MISSION.md`;
`docs/findings/2026-10-04-m01-lc-mission-program.md` (#630's census and
record vocabulary);
`docs/findings/2026-10-04-f39-e4-count-category-producers.md`;
`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md` and
`docs/findings/evidence/F56-B.json` (the multiplayer slots' `targets.zrd`
objectives and the earlier `net.zrd` spawn-candidate note);
`docs/findings/2026-10-08-t574-zeppelins-zrd-placed-zeppelin-carrier.md`
(`zeppelins.zrd` fields, counted but not re-measured here);
`crates/cs_content/src/stunts.rs` (F42-A's scenario grammar this stage
reuses); `missions/M01.md`. No web source was consulted and no original
executable was run.
