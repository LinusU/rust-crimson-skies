# F14-D.6: the airframe collection of the retail baseline inventory, and where the original states an airframe at all

Date: 2026-10-03. Task: #489 "Populate the airframe and flight-configuration
collections of the retail baseline inventory" (follow-up of #389 / F14-D.2 and
#486 / F14-D.3). Feature sheet:
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, stage
`### F14-D`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`; evidence
contract `docs/contracts/CLI-EVIDENCE.md`. Required capability: **`retail`**,
used read-only. `gpu` and `audio` were available and **not used**: nothing is
rendered or played, no original run happened, and nothing here claims
`verified_original`.

## What this stage changes about the F14-D verdict

The baseline inventory now holds a tenth populated collection. Its report over
the owner's installation reads (measured on the reviewed tree, after F14-D.5's
faction and paint-mask collections landed on `main`; see **Review record**):

| collection | rows | source |
| --- | --- | --- |
| `install_file` | 228 | the F02 inventory |
| `mission` | 24 | the shared campaign walk |
| `script` | 53 | the mission and scenario program archives |
| `ia_scenario` | 8 | F14-D.1's classified scenario directories |
| `multiplayer_scenario` | 21 | F14-D.1's classified scenario directories |
| `multiplayer_rules` | 4 | F14-D.2's mode table |
| `world` | 8 | F14-D.3's world-group readers |
| `faction` | 11 | F14-D.5's paint records |
| `paint_mask` | 184 | F14-D.5's verified library members |
| **`airframe`** | **11** | **`ZBD/interp.zbd`'s loading scripts** |

The coverage denominator did **not** move: 53 declared roots (24 campaign
missions plus 29 scenario directories), 159 reachable rows, 393 unreachable,
0 ready, 0 unresolved references. An airframe is not launchable content, so the
eleven new rows are counted as unreachable unknowns in
`coverage.unreachable_by_kind.airframe` rather than entering the denominator.

## Where the installation states an airframe at all

The task asked this question first, and the answer is measured rather than
assumed. Four places were examined on the owner's installation, read-only:

1. **`ZBD/interp.zbd` — the loading-script container (the answer).** Its script
   `support\planes.gw` builds the shared airframe archive: for each airframe it
   binds a source model and a root name, includes a surgery script once, and
   that included script's `NewObject3D %planeOutput%` line creates the root; the
   container then writes `ZBD/planes.zbd`. That is a declaration in the
   original's own bytes, and F11-D2 already measures the walk
   (`docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md`): **eleven**
   declared roots, zero unreadable lines. This stage turns that measured
   declaration into catalog rows.
2. **The per-plane `.zrd` members of `ZBD/zrdr.zbd` are not statistics.** The
   archive's own member index lists `bloodhawk.zrd`, `warhawk.zrd`,
   `warhawk_hook.zrd`, `plane_props.zrd`, `player_plane_destruct.zrd` and
   `plane_splash.zrd`. The record a mission's data spells for one of them pairs
   the member's path with the token `ANIMATION_DEFINITION_FILE`, and the member
   bodies read as **animation definitions** (activation, object-active states,
   sequence definitions, sound and animation calls). No mass, thrust, lift,
   armour, damage zone, gun fit or construction value appears in them.
3. **`GOSDATA/ASSETS/crimson.rof`'s UI scripts hold no airframe values.** The
   container carries `ASSETS/SCRIPTS/` with `AIRFRAME.SCRIPT`, `ARMOR.SCRIPT`,
   `ENGINE.SCRIPT`, `GUNS.SCRIPT`, `HARDPOINTS.SCRIPT`,
   `PLANECONSTRUCTION.SCRIPT`, `MULTIPLAYERLOBBY_PLANE.SCRIPT`,
   `MULTIPLAYER_PLANEDEF.SCRIPT` and others. Read: `AIRFRAME.SCRIPT` is a dialog
   body whose every value arrives through `callback($$E$$, …)` /
   `callback($$A$$, 2507, …)` native calls; `MULTIPLAYER_PLANEDEF.SCRIPT` fetches
   the plane list and each plane's statistics the same way
   (`callback($$E$$, 5029, …)`, `5061`, `5067`, `5010`, `5015`). These scripts
   name **no** airframe and store **no** airframe number: the original reads its
   own airframe data through native behavior bindings this engine has not
   decoded (F38/F13). That is where the per-aircraft statistics live, and this
   stage cannot read them.
4. **`strings.dll` names airframes without defining them.** Its `RT_STRING`
   block carries `MSG_VEH_*` message keys over faction airframes
   (`MSG_VEH_KG_BLOODHAWK`, `MSG_VEH_BLAKE_BLOODHAWK`, …) and includes at least
   one airframe the shared roster does not declare (`MSG_VEH_BRIT_DEVASTATOR`).
   A message key is a string id for user-interface text, not an airframe
   identity, and a faction-scoped key is not the identity a `SceneRootRef` must
   name — so it is a **lead**, recorded here and filed as a follow-up, and it
   produced no row.

**Consequence for this stage.** The airframe collection is an *identity and a
declaration* collection: one row per airframe the original declares, with its
root, the container it lands in and the exact line that named it. Every row says
explicitly that no original **statistic** of that airframe has been read. The
`engine`, `armor`, `gun`, `ammo` and `hardpoint_equipment` collections
`IDENTITY-CONTENT` also requires hold **no** row, because no production reader
in this workspace locates those families anywhere in the installation, and
inventing a source spelling for them would be the guess `AGENTS.md` rule 4
forbids. They are named in the evidence report's `UNKNOWN_LIMITATIONS` and in the
follow-up tasks created from #489.

## What this stage adds

- `crates/cs_content/src/catalog/baseline.rs`: the `airframe` collection —
  `AIRFRAME_SCRIPT_IMAGE`, `AIRFRAME_TUNING_CLAIM`, the
  `CLAIM_AIRFRAME_DECLARATION` edge claim, the two new `BaselineError` variants
  (`AirframeRoster`, `AirframeLine`), `airframe_rows`,
  `airframe_roster_declarations`, `unique_script`, `declaring_line_span`,
  `airframe_row`, `airframe_unknowns`, `roster_issue_label`, and the
  `collection_status` record. No other file of the crate changed; the module
  needs no new dependency (`cs_formats` and `cs_types` were already there).
- `crates/cs_content/tests/accept_f14_d_6_airframe_collection.rs` (new): seven
  `accept_f14_d_6_*` tests plus one `#[ignore]`d retail test (the seventh
  synthetic test was added by the review; see **Review record** below).
- `crates/cs_content/tests/evidence_report_f14_d_6.rs` (new): the evidence
  harness, deliberately not named with the acceptance prefix.
- `docs/findings/evidence/F14-D.6.json` (new): the validated acceptance report.

## Design decisions

- **The rows come from the producing stage's parser.** They are
  `cs_content::scene::discover_airframe_roster`'s rows (F11-D2) over
  `cs_formats::interp::decode_interp` of the container, not a roster derived for
  this task. `cs_content::scene` ships no idiom on purpose; the baseline needs
  one to *hold* the collection, so the measured shapes are stated once in
  `airframe_roster_declarations` with their own `observed_tool` provenance. That
  is the same arrangement F14-D.1's `reader_dirs::classify` uses for a reader
  directory, and the retail row count and identities are pinned by a test, so a
  wrong idiom fails instead of quietly returning nothing.
- **The declaration's span is measured, not written down.** It is the extent of
  the declaring script inside the container the walk just decoded. A retail
  offset copied into the table would be stale on any other installation, and
  `SourceSpan::new` does not validate against a file size — so the span is
  derived from the bytes every time, and a container without the declaring
  script gets **no** span (the discovery then reports
  `declaring_script_absent`), which is the honest answer.
- **A row is located by the line that named it, and that line is looked up.**
  `declaring_line_span` finds the stored line record at the discovery's offset
  in the same decoded container and spans it; a miss is
  `BaselineError::AirframeLine`, not a span invented around the number. The
  retail test checks each span against the declaring script's own measured
  extent *and* that it starts at a stored line.
- **The identity is the declared root, never the model spelling.** F11 non-
  negotiable behavior 3: a model name is not an airframe. The model spelling
  stays provenance in the producing discovery and is copied into neither identity
  nor `display_name` — and `display_name` is `None` because the installation
  states no display name for an airframe at all (the four places examined above
  confirm it: the UI scripts fetch names through a native callback).
- **Two explicit unknowns and a missing consumer, per row.** Under
  `f14.d.6.airframe_statistics`, no original statistic of this airframe has been
  read (measured above); under the producing stage's own
  `f11d.roster-availability-undiscovered`, building a scene root is not evidence
  that any mode lets a player choose it. Per the numeric contract a missing
  value never becomes a zero or an SI assumption. `MissingRuntimeConsumer` says
  the third thing: nothing in the engine consumes the row yet.
- **A collection that reads nothing is a named gap, not an absent collection.**
  A container the decoder refuses, a container with no declaring script and a
  container that declares no airframe all yield `rows: 0` plus a `diagnostic`,
  and the discovery's own findings are counted under their stable labels
  (`roster_issue`, `roster_unknown` and one label per finding kind), so a report
  reader can tell "this installation declares none" from "this installation
  declares something I could not read".
- **No collection record for the five families with no located source.** A
  `CollectionStatus` must name the file its rows come from; for engines,
  armour, guns, ammunition and hardpoints no such file has been measured, so a
  record would have to carry a `source` spelling that is a guess. They are named
  in the findings, in the evidence report's limitations and in follow-up tasks
  instead.

## What the retail run measured

`$CS_GAME_DIR`, production discovery and fingerprint, production
`decode_interp`, production `discover_airframe_roster`, and the production
`retail_baseline` that the acceptance test and the evidence harness both run.
Evidence artifact `baseline-report.json`, hashed by
`docs/findings/evidence/F14-D.6.json`. **No string text from the installation is
reproduced in this file**; the rows are identified by their catalog ids and
counts, and the four research locations above are described by container and
member kind.

| quantity | value |
| --- | --- |
| `airframe` rows | **11** |
| roster findings (`roster_issue`) | **0** |
| roster unknowns (`roster_unknown`) | **2** (availability, forced assignments) |
| collection diagnostic | `null` |
| declared roots, re-walked independently by the evidence harness | **11**, the same set |
| rows that are closure roots | **0** |
| rows whose edge resolves inside the inventory | **11 of 11** |
| unresolved references | **0** |
| declared roots (the denominator) | **53**, unchanged |
| coverage ready | **0** |
| `accept_f14_d_6_*` tests executed | **8** (7 synthetic, 1 retail), all passing |

The eleven identities are F11-D2's measured roster; the catalog renders them in
canonical id order while the declaration order the container creates them in is
pinned by F11-D2's own acceptance test. `player_autogyro` — the exceptional
configuration F25 cares about — is one of the eleven, declared by the same
script.

## Test inventory

| `accept_f14_d_6_*` test | Covers | Fails when |
| --- | --- | --- |
| `a_declared_roster_becomes_one_row_per_airframe` (cs_content/tests/accept_f14_d_6_airframe_collection.rs) | the mapping arm over a synthetic installation: two declared roots become two rows in canonical id order; each row's origin is installation data over the **naming line's** span, checked to lie inside the declaring script's extent *and* to start at a stored line of the authored container; two `None` display names; distinct offsets; `parsed`/`not_normalized`/`unavailable`; the reason codes `["unknown","unknown","missing_runtime_consumer"]` in order with `f14.d.6.airframe_statistics` then `f11d.roster-availability-undiscovered`, each unknown carrying why; one static edge onto `install_file/zbd_2f_interp.zbd` with `observed_tool` provenance at the row's own span; the row's fingerprint equal to that inventory row's; the record's source/language/rows/gaps/diagnostic; the report's `airframe` count and collection fragment; no synthetic origin; and the report is byte-stable for the same installation | a row is keyed by the model spelling instead of the declared root, a display name is invented, a span is taken over the whole container or no longer covers the naming line, an unknown is dropped or replaced by a value, the edge moves off the container's inventory row, the fingerprint stops matching it, or the report stops being deterministic |
| `a_container_that_declares_no_airframe_is_named_not_invented` | a container that binds the container variable and writes it but never creates a root: **no** row, `no_airframes_declared: 1`, a diagnostic naming the container, and the campaign mission rows and the launchable count unchanged — a collection's failure never takes the missions with it | a container that creates no root yields an airframe, a finding is dropped, or another collection's rows disappear |
| `a_container_without_the_declaring_script_is_named_not_invented` (added by the review) | a container that reads as the interp container but holds a script that is not `support\planes.gw`, so the idiom has no script to measure its provenance span over: **no** row, `declaring_script_absent: 1` and `no_airframes_declared: 1` under their own labels, `roster_unknown: 1` (no row exists to be unavailable), a diagnostic quoting the finding itself, and the mission row and launchable count intact | the absent declaring script is passed over silently, its finding is not counted or is counted under a label of somebody else's, or a row is borrowed from another script's bytes |
| `an_unreadable_line_is_a_finding_and_the_other_row_survives` | a `set` line stored with four arguments, which is not the declared shape: the airframe the container really declares still becomes a row, `roster_issue: 1` and `line_unreadable: 1` on the record, and no diagnostic because the collection holds rows | the unreadable line is silently skipped, the real row is lost with it, or the finding is not counted |
| `an_unreadable_or_absent_loading_container_is_a_named_gap` | (a) a container that is not an INTERP container at all and (b) an installation with no container: both `rows: 0` with a diagnostic naming the source, the mission row and the root intact, and no `airframe` key in the report | a refused container is reported as an empty reading, or its diagnostic is dropped |
| `airframes_are_not_launchable_and_the_denominator_does_not_move` | `ContentKind::Airframe.is_launchable()` is false; one root; `launchable_count == original_launchable_count == 1`; `coverage.roots == 1`; `reachable == 3` (mission, program, file); `unresolved_references == 0`; `unreachable_by_kind.airframe == 2`; the catalog is neither fully nor retail ready | the denominator moves when the collection is populated, a row becomes a closure root, or an edge dangles |
| `the_airframe_edges_resolve_and_the_counts_agree` | the report's own accounting: the record's `rows` equals the catalog's count, the report names the collection once and the row once, and every airframe edge points at a row this inventory holds; the collection source is an installation-relative spelling | the record and the catalog disagree, or an edge points at a row that does not exist |
| `retail_the_installation_declares_its_shared_airframe_roster` (`#[ignore = "requires CS_GAME_DIR"]`) | **the real installation**: production discovery and fingerprint, production `decode_interp`, the eleven identities equal to F11-D2's measured roster, `player_autogyro` among them, each row's span inside the declaring script's measured extent and inside the inventoried size and starting at a stored line, one static edge per row onto the container's inventory row at the row's own span with `observed_tool` provenance, the fingerprint of the inventoried container's bytes, the three reason codes and both claim ids per row, no display name, no consumer and nothing ready; the record's `roster_issue: 0` / `roster_unknown: 2` and no diagnostic; 11 distinct naming-line offsets; **no** airframe row is a root, the denominator is still the launchable rows, `unreachable_by_kind.airframe == 11`; and the report says `airframe: 11` with no synthetic origin | the corpus changes, the idiom stops finding an airframe or finds a non-airframe, a row's container stops matching the inventory, a row loses an unknown, a row is promoted to a root, a measured span stops covering its naming line, or the report claims completeness |

The evidence harness (`evidence_report_f14_d_6_writes_the_acceptance_report`,
deliberately **not** named with the task prefix) derives
`private/evidence/F14-D.6/acceptance.json` and `baseline-report.json` from the
recorded acceptance log, production discovery of `$CS_GAME_DIR`, the production
baseline over that installation, and an **independent walk** of the container's
own `set planeOutput <root>` lines — a second derivation of the same root set that
never calls the roster discovery, so the collection is compared against a second
reading of the bytes rather than against itself. It refuses a stale
`CS_CANDIDATE_TREE` and a missing log, and writes a failing report when the
acceptance run failed.

## Sensitivity check

Every mutation below was applied to `crates/cs_content/src/catalog/baseline.rs`,
the acceptance file was run (the six non-retail tests the implementer had at the
time, in 0.01 s), and the file was restored. **All six are killed.** The review
added a seventh probe against the same file; see **Review record** below.

| # | Mutation | Killed by |
| --- | --- | --- |
| 1 | the `airframe_rows` call in `retail_baseline` is replaced by an empty row set | all six tests: the mapping test's identity list (`left: [], right: ["player_first", "player_second"]`), the no-airframe and absent-container tests' diagnostics, and the denominator test's `unreachable_by_kind.airframe` |
| 2 | the row is keyed by `discovered.root_name()` — the scene-node reference — instead of the declared root | the mapping test, on the identity list |
| 3 | the row is located by the container as a whole (`offset 0`, the file length) instead of by its naming line | the mapping test: the span no longer starts at a stored line of the declaring script, and it no longer lies inside that script's extent |
| 4 | the two explicit unknowns are recorded as a state (`not_normalized`) instead | the mapping test, on `row.unsupported_codes()` — `left: ["not_normalized", "not_normalized", "missing_runtime_consumer"]` |
| 5 | the two claims are swapped, so the row no longer says which unknown is which | the mapping test, on the ordered claim list |
| 6 | the collection's `gaps` are cleared | three tests: the mapping test (`roster_issue`/`roster_unknown` gone), the no-airframe test (`no_airframes_declared: 1` gone) and the unreadable-line test (`line_unreadable: 1` gone) |

The retail test additionally pins the eleven identities, the eleven distinct
naming-line offsets and the two claim ids against the real installation, so
mutations 2, 3 and 5 fail there too — at the cost of a 150 s run over the
installation, which is why the fast synthetic arms carry the same checks.

## Review record

Reviewer: `bunny-alpha-2/bunny-alpha-2` again (Rally #489, review claim of
2026-10-03T05:04:30Z), in a fresh session with an empty context. **That is the
implementer's own agent instance, so this review is not independent
original-reference evidence**, and nothing here is raised above `checked`. The
reviewer re-derived the retail facts from `$CS_GAME_DIR` with production code
rather than trusting the implementer's notes, and found:

1. **A regression the implementer missed (repaired here).** This stage's own
   stage-level test, `accept_f14_d_baseline.rs`'s
   `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`,
   asserted `catalog.len() == files + 2*launchable + mode_rows + world_rows`.
   Eleven airframe rows are not launchable, so every other total in that test
   still held, but the row count did not: the test failed over the real
   installation with `left: 357, right: 346`. The repair does **not** filter the
   rows out and does **not** relax the equality. It now derives the
   non-launchable part of the catalog from the rows themselves (`unaccounted`),
   asserts that it equals the sum of every collection this test names, and keeps
   the total as `files + 2*launchable + unaccounted`. A collection added later
   without appearing in that sum now fails the completeness check instead of
   slipping past it — which matters, because the rebase onto `main` brought in
   **F14-D.5**, whose `faction` (11) and `paint_mask` (184) rows had broken the
   same equality on `main` itself and were counted here too. This is the spec's
   own instruction for this stage ("repair discovered regressions without
   weakening the specification"), and it is the only change to a file outside
   this stage's own.
2. **A branch no test covered (covered here).** A container that reads as the
   interp container but holds **no** declaring script takes the
   `unique_script` → `None` path: the idiom then has no script to measure its
   provenance span over, the discovery reports `declaring_script_absent`, and the
   collection must still name the gap rather than borrow another script's bytes.
   That path was untested; `a_container_without_the_declaring_script_is_named_not_invented`
   now covers it, and renaming that finding's stable gap label
   (`declaring_script_absent` → `absent_script`) is killed by it.
3. **The rebase onto `main` resolved one file by hand.**
   `catalog/baseline.rs` is the file every F14-D collection grows, so F14-D.5 and
   this stage both appended to the same module doc, import list, claim-constant
   block and collection block, and the merge conflicted in six places. Each was
   resolved by keeping **both** sides' additions, and the result was then checked
   mechanically rather than by eye: every item of both parents is present exactly
   once, no item is new, and every function's own doc comment sits directly above
   it (a concatenation that had interleaved the two doc blocks would have
   documented `faction_rows` with the airframe text). `cargo check
   --all-targets`, clippy and the full suite all follow.

Checked and found sound, with the measurement restated here rather than assumed:
the naming line's span really is the stored record (`InterpLine::offset()` is the
`size` word and `data_offset() + size` its end, so the extent covers the whole
record); the collection's `files` lookup uses the same `logical_key()` convention
as the neighbouring collections, so an installation that spells the path with
either separator is found; a colliding id would fail the whole baseline through
`insert` rather than silently replace a row; the closure reports the airframes as
unreachable unknowns and moves neither the root set nor the ready count; the
evidence harness's second derivation really never calls
`discover_airframe_roster`; `docs/findings/evidence/F14-D.6.json` validated with
`--require-pass` before and after the regeneration; and no protected path is
touched by this branch.

Not this branch's problem, and left alone: `tools/tests/
test_evidence_review_identity.py` fails its two runtime-harness pins on
`origin/main` as well (verified in a clean worktree at `db047e41`), which #562
owns. The measured roster idiom now exists in **three** copies — the production
`airframe_roster_declarations` in `catalog/baseline.rs`, which measures its
provenance span from the container, and two test-local `retail_roster_declarations`
helpers in `crates/cs_content/tests/scene.rs` and
`crates/cs_app/tests/camera/coverage.rs` that hard-code retail byte offsets; a
follow-up task was filed for them rather than editing another stage's test here.

## Unknowns and limitations (all recorded, none guessed)

- **No original airframe statistic has been read.** Every row says so under
  `f14.d.6.airframe_statistics`. **Affected content:** every flight, damage,
  ordnance and construction number in the engine — all of it stays
  `Resolved::Unknown` or authored design (`cs_content::flight_tuning` holds one
  `Origin::Designed` synthetic airframe). **Resolving tasks:** F24, F26, F29,
  F27 and F44 together with F38 (the native behavior bindings), because F14-D.6's
  research measured that the original's airframe values reach its own hangar
  through native callbacks this engine has not decoded.
- **`engine`, `armor`, `gun`, `ammo` and `hardpoint_equipment` hold no row.**
  No production reader locates those families in the installation, and the
  research above found no file that states them. **Affected content:** every
  engine, armour, gun, ammunition and hardpoint record, and the whole content
  side of F24/F27/F29/F44. **Resolving tasks:** the follow-up tasks created from
  #489, one per collection, each of which must first *measure* where the content
  is stated rather than assume a file.
- **Only the shared airframe archive's roster is discovered.** A mission-only
  airframe lives in a per-chapter `gamez.zbd` and is not declared by the loading
  script, and the eleven bare-named scene variants are scene roots, not declared
  airframes. **Affected content:** every airframe a mission spawns (53 303 of the
  installation's 56 620 stored node records are per-chapter).
  **Resolving task:** the roster follow-up over the per-chapter `LoadGameGen`
  idiom, already filed by F11-D2's findings.
- **Nothing references an airframe yet**, so the rows are unreachable from the
  declared roots and stay counted in `coverage.unreachable_by_kind` and
  `unreachable_needing_classification`. **Affected content:** the reachability
  accounting of the collection. **Resolving task:** the stage that lets a mission
  or scenario row point at the airframe it launches in.
- **The roster idiom is a claim with its own provenance**, shipped here and
  falsifiable only by the pinned row count. **Affected content:** the collection's
  reachability for an installation whose loading script spells the idiom
  differently. **Resolving tasks:** F07-D/F13 (the opcode and mission-language
  stages) and F11-E (a shipped consumer).
- **`required_roles` is empty here.** The cockpit requirement F11-D2 measured
  (`support\util\planesurgery.gw` re-adds a `cockpit1` node for every airframe)
  is a claim about node bindings, not about a catalog row, so this collection
  asserts no role. **Affected content:** nothing in this row set;
  **resolving task:** F11-C/F29's name-path binding rules.
- **A collection record is not written for the five families with no located
  source** (see the design decision above). **Affected content:** a report reader
  sees no `collection_status` entry for them; the emptiness is carried by the
  findings note, the evidence report's limitations and the follow-up tasks.
  **Resolving task:** the same per-collection follow-ups.
- **Evidence class.** The rows are derived from original bytes by production
  readers, which makes them `observed_tool`. `retail` is file access, not proof
  that the original executable ran; the loading-script semantics this stage reads
  (`set`, `source`, `NewObject3D`, `GameZWriteZBDFile`, `%NAME%`) are the
  corpus's own spelling and were not verified against the running engine. No
  original run happened and nothing claims `verified_original`.
- **Independent review is still outstanding for this work.** AGENTS.md asks a
  different agent instance or model with a fresh context to review format and
  catalog work. The review recorded above was made by the implementer's own
  instance in a fresh session, so it is **not** independent, and
  `docs/findings/evidence/F14-D.6.json` says so in `review.identity` rather than
  leaving the field to a later reader. **Affected content:** the independence of
  this stage's format and catalog claims. **Resolving task:** an owner-scheduled
  review by another agent instance or model; until then nothing here is above
  `checked`, and no agent review replaces the owner's human approval.
- **Fixture scope.** The seven unignored tests are synthetic and newly authored;
  only the `#[ignore]`d retail test reads original data. Nothing derived from it
  beyond ids, counts, offsets and digests is committed.
- **The four research locations above are read-only research.** No extracted
  member, script text or roster is committed; the ROF/reader-archive inspection
  used private temporary files outside this checkout.

## Sources used

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (stage
  `### F14-D`, non-negotiable behavior 4, AC02/AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (the required collections, the numeric
  contract, and the rule that a collection cannot exclude failed entries).
- `docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`.
- `docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md` (the measured
  roster, the container, the four idioms and the eleven identities).
- `docs/findings/2026-10-03-f14-d-3-world-collection.md` (the collection-record
  pattern this stage follows: gaps, diagnostics and a denominator that must not
  move).
- `crates/cs_formats::interp` (`decode_interp`, stored line records and script
  extents), `crates/cs_assets::install` (production discovery and the two
  fingerprints), `cs_content::config::StringCatalog` and
  `cs_content::multiplayer::discover_modes` (the neighbouring collections' shape).
- The owner's installation, read-only: `ZBD/interp.zbd`, `ZBD/zrdr.zbd`'s own
  member index and its per-plane members, `GOSDATA/ASSETS/crimson.rof`'s
  `ASSETS/SCRIPTS/` members, and `strings.dll`.